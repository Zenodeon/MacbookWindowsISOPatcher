//! Journaled storage relocation. The original stays intact until settings and
//! build ownership metadata have committed. Recovery never guesses which copy
//! of user data should be removed.
use crate::{
    cleanup,
    domain::{BuildRequest, Cancellation},
    download,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const LOCKED_REASON: &str =
    "Return to model selection and wait for current operations to finish to move the cache folder.";
pub const WORKING_SPACE_LOCKED_REASON: &str =
    "Wait for current operations to finish before changing WorkingSpace.";
const JOURNAL: &str = "storage-relocation.json";
const MARKER: &str = ".relocation-id";
const MOVED: &str = ".storage-moved.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    Cache,
    Workspace,
}
impl Kind {
    pub fn locked_reason(self) -> &'static str {
        match self {
            Self::Cache => LOCKED_REASON,
            Self::Workspace => WORKING_SPACE_LOCKED_REASON,
        }
    }
    fn field(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Workspace => "work",
        }
    }
}
#[derive(Clone, Copy)]
pub struct Gate {
    pub model_selection: bool,
    pub before_build: bool,
    pub idle: bool,
}
impl Gate {
    pub fn check(self, kind: Kind) -> Result<()> {
        let stage_allowed = match kind {
            Kind::Cache => self.model_selection,
            Kind::Workspace => self.before_build,
        };
        ensure!(stage_allowed && self.idle, kind.locked_reason());
        Ok(())
    }
}
pub struct Request {
    pub kind: Kind,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub other_root: PathBuf,
    pub protected_sources: Vec<PathBuf>,
    pub settings_path: PathBuf,
    pub settings: Value,
    pub history_root: PathBuf,
    pub gate: Gate,
}
#[derive(Clone, Debug)]
pub struct Progress {
    pub files: u64,
    pub total_files: u64,
    pub bytes: u64,
    pub total_bytes: u64,
    pub phase: String,
}
#[derive(Debug)]
pub struct Outcome {
    pub settings: Value,
    pub warning: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    id: String,
    source: PathBuf,
    destination: PathBuf,
    staging: PathBuf,
    settings_path: PathBuf,
    old_settings: Value,
    new_settings: Value,
    history_root: PathBuf,
    old_history: Value,
    new_history: Value,
    committed: bool,
    destination_created: bool,
}

pub fn settings_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("com", "Zenodeon", "MacBookISOPatcher")
        .expect("User profile required");
    dirs.cache_dir()
        .parent()
        .expect("Application directory")
        .join("settings.json")
}
pub fn save_settings(path: &Path, settings: &impl Serialize) -> Result<()> {
    fs::create_dir_all(path.parent().context("Settings folder missing")?)?;
    cleanup::write_json(path, settings).context("Could not save application settings")
}
pub fn load_settings(local: &Path, legacy: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(local) {
        Ok(bytes) => {
            serde_json::from_slice::<Value>(&bytes)?;
            Ok(Some(bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::read(legacy) {
            Ok(bytes) => {
                let value: Value = serde_json::from_slice(&bytes)?;
                save_settings(local, &value)?;
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        },
        Err(e) => Err(e.into()),
    }
}

// Normalize Windows spelling, including canonical paths with a verbatim prefix.
fn key(path: &Path) -> String {
    let text = crate::platform::display_path(path);
    #[cfg(windows)]
    {
        text.trim_start_matches(r"\??\")
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    }
    #[cfg(not(windows))]
    {
        text.trim_end_matches('/').to_owned()
    }
}
pub(crate) fn within(path: &Path, root: &Path) -> bool {
    let path = key(path);
    let root = key(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|tail| tail.starts_with(std::path::MAIN_SEPARATOR))
}
fn remap(path: &Path, source: &Path, destination: &Path) -> PathBuf {
    if !within(path, source) {
        return path.to_owned();
    }
    let skip = source.components().count();
    // Canonical Windows paths can introduce a verbatim prefix; use matching
    // trailing components instead of string byte offsets.
    let relative: PathBuf = path.components().skip(skip).collect();
    destination.join(relative)
}
fn root(path: &Path) -> Result<PathBuf> {
    cleanup::safe_path(path)?;
    ensure!(
        path.file_name().is_some() && path.parent().is_some_and(|p| p != path),
        "Choose a folder, not a drive root"
    );
    let ancestor = path
        .ancestors()
        .find(|p| p.exists())
        .context("No existing parent folder")?;
    let normalized = ancestor.canonicalize()?.join(path.strip_prefix(ancestor)?);
    cleanup::safe_path(&normalized)?;
    Ok(normalized)
}
fn overlaps(a: &Path, b: &Path) -> bool {
    within(a, b) || within(b, a)
}
fn ignored(relative: &Path) -> bool {
    relative.components().count() == 1
        && relative.file_name().is_some_and(|name| {
            [".cache.lock", ".build.lock", ".history.lock", MARKER, MOVED]
                .iter()
                .any(|s| name == *s)
        })
}
fn files(root: &Path) -> Result<Vec<(PathBuf, u64)>> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false).min_depth(1) {
        let entry = entry?;
        cleanup::safe_path(entry.path())?;
        let relative = entry.path().strip_prefix(root)?.to_owned();
        if ignored(&relative) {
            continue;
        }
        ensure!(
            entry.file_type().is_file() || entry.file_type().is_dir(),
            "Unsupported file: {}",
            entry.path().display()
        );
        if entry.file_type().is_file() {
            files.push((relative, entry.metadata()?.len()));
        }
    }
    files.sort();
    Ok(files)
}
fn owned_tree(path: &Path, id: &str) -> Result<()> {
    cleanup::safe_path(path)?;
    ensure!(
        fs::read_to_string(path.join(MARKER)).is_ok_and(|saved| saved == id),
        "Relocation ownership marker missing: {}",
        path.display()
    );
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        cleanup::safe_path(entry?.path())?;
    }
    fs::remove_dir_all(path)?;
    Ok(())
}
fn rollback(journal: &Journal) -> Result<()> {
    save_settings(&journal.settings_path, &journal.old_settings)?;
    cleanup::write_json(
        &journal.history_root.join("build-history.json"),
        &journal.old_history,
    )?;
    if journal.destination.exists()
        && fs::read_to_string(journal.destination.join(MARKER)).is_ok_and(|s| s == journal.id)
    {
        owned_tree(&journal.destination, &journal.id)?;
        if !journal.destination_created {
            fs::create_dir(&journal.destination)?;
        }
    }
    if journal.staging.exists() {
        // A crash can happen between creating the empty staging directory and
        // writing its marker. Never recursively remove an unmarked directory.
        if !journal.staging.join(MARKER).exists()
            && fs::read_dir(&journal.staging)?.next().is_none()
        {
            fs::remove_dir(&journal.staging)?;
        } else {
            owned_tree(&journal.staging, &journal.id)?;
        }
    }
    if journal.source.join(MOVED).exists() {
        fs::remove_file(journal.source.join(MOVED))?;
    }
    Ok(())
}
pub fn recover(settings_path: &Path) -> Result<Option<String>> {
    let parent = settings_path.parent().context("Settings folder missing")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let _lock = loop {
        match cleanup::activity(parent, ".relocation.lock", true) {
            Ok(lock) => break lock,
            Err(error) if cleanup::lock_busy(&error) && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Err(error) => return Err(error),
        }
    };
    let path = parent.join(JOURNAL);
    if !path.exists() {
        return Ok(None);
    }
    let journal: Journal = serde_json::from_slice(&fs::read(&path)?)?;
    ensure!(
        journal.settings_path == settings_path,
        "Relocation journal belongs to another settings file"
    );
    uuid::Uuid::parse_str(&journal.id)?;
    cleanup::safe_path(&journal.source)?;
    cleanup::safe_path(&journal.destination)?;
    ensure!(
        journal.staging
            == journal
                .destination
                .parent()
                .context("Destination parent missing")?
                .join(format!(".patcher-move-{}", journal.id)),
        "Invalid relocation staging location"
    );
    ensure!(
        !overlaps(&journal.source, &journal.destination) && !within(parent, &journal.source),
        "Invalid relocation journal paths"
    );
    let _history = cleanup::history_lock(&journal.history_root)?;
    let message = if journal.committed {
        ensure!(
            fs::read_to_string(journal.destination.join(MARKER)).is_ok_and(|id| id == journal.id),
            "The committed destination cannot be verified. Its files and the original folder were preserved"
        );
        save_settings(settings_path, &journal.new_settings)?;
        cleanup::write_json(
            &journal.history_root.join("build-history.json"),
            &journal.new_history,
        )?;
        format!(
            "Recovered the move to {}. Any remaining original files at {} were retained; inspect them before removing them.",
            journal.destination.display(),
            journal.source.display()
        )
    } else {
        rollback(&journal)?;
        "An interrupted folder move was rolled back. Your original folder is still selected.".into()
    };
    fs::remove_file(path)?;
    if journal.committed {
        fs::remove_file(journal.destination.join(MARKER))?;
    }
    Ok(Some(message))
}

pub fn recovery_pending(settings_path: &Path) -> bool {
    settings_path
        .parent()
        .is_some_and(|p| p.join(JOURNAL).exists())
}

fn unmounted(workspace: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        // Registry inspection does not require elevation. Include every string
        // value so old and new WIMMount registry schemas are both covered.
        let json = crate::platform::powershell(
            r#"$ErrorActionPreference='Stop'; $r='HKLM:\SOFTWARE\Microsoft\WIMMount\Mounted Images'; $values=@(); if(Test-Path -LiteralPath $r){foreach($k in Get-ChildItem -LiteralPath $r){$p=Get-ItemProperty -LiteralPath $k.PSPath; foreach($v in $p.PSObject.Properties){if($v.Value -is [string]){$values += $v.Value}}}}; ConvertTo-Json -Compress -InputObject @($values)"#,
            &[],
        )?;
        let paths: Vec<String> = serde_json::from_str(json.trim())?;
        ensure!(
            !paths.iter().any(|p| within(Path::new(p), workspace)),
            "This WorkingSpace has a retained WIM mount. Clean up its mounted build before moving it"
        );
    }
    for entry in walkdir::WalkDir::new(workspace)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !e.file_type().is_symlink())
    {
        let entry = entry?;
        if entry.file_type().is_dir()
            && ["mount-boot", "mount-install"]
                .iter()
                .any(|n| entry.file_name() == *n)
        {
            ensure!(
                fs::read_dir(entry.path())?.next().is_none(),
                "A retained image mount exists at {}; clean it up before moving WorkingSpace",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn prepare_history(
    history: &mut cleanup::History,
    source: &Path,
    staging: &Path,
    destination: &Path,
    kind: Kind,
    protected: &[PathBuf],
) -> Result<()> {
    if kind == Kind::Workspace {
        cleanup::import_jobs(history, source)?;
    }
    for record in &mut history.records {
        ensure!(
            !within(&record.source, source),
            "The folder contains an original source ISO: {}",
            record.source.display()
        );
        if kind != Kind::Workspace || !within(&record.job, source) {
            continue;
        }
        cleanup::validate_record(record)?;
        let original_job = record.job.clone();
        let mut request: BuildRequest =
            serde_json::from_slice(&fs::read(original_job.join("job.json"))?)?;
        ensure!(
            cleanup::same_path(&request.workspace, &record.workspace)?
                && cleanup::same_path(&request.inventory.iso, &record.source)?
                && cleanup::same_path(&request.output, &record.output)?,
            "Build ownership metadata does not match; original WorkingSpace retained"
        );
        ensure!(
            !within(&request.inventory.iso, source),
            "WorkingSpace contains the original source ISO"
        );
        if within(&record.output, source) && record.output.exists() {
            ensure!(
                record.published.as_ref() == Some(&cleanup::fingerprint(&record.output)?),
                "An output ISO has changed or is unverified; preserve it outside WorkingSpace before moving"
            );
            record.published = Some(cleanup::fingerprint(&remap(
                &record.output,
                source,
                staging,
            ))?);
        }
        request.workspace = remap(&request.workspace, source, destination);
        request.output = remap(&request.output, source, destination);
        request.cache = remap(&request.cache, source, destination);
        record.job = remap(&record.job, source, destination);
        record.workspace = remap(&record.workspace, source, destination);
        record.output = remap(&record.output, source, destination);
        record.partial = record
            .partial
            .as_ref()
            .map(|p| remap(p, source, destination));
        record.connection_log = record
            .connection_log
            .as_ref()
            .map(|p| remap(p, source, destination));
        let staged_job = remap(&original_job, source, staging);
        cleanup::write_json(&staged_job.join("job.json"), &request)?;
        cleanup::write_json(&staged_job.join("build-state.json"), record)?;
    }
    for path in protected {
        ensure!(
            !within(path, source),
            "This folder contains the selected source ISO"
        );
    }
    Ok(())
}

pub fn execute(request: Request, progress: &dyn Fn(Progress)) -> Result<Outcome> {
    request.gate.check(request.kind)?;
    let settings_parent = request
        .settings_path
        .parent()
        .context("Settings folder missing")?;
    let _operation = cleanup::activity(settings_parent, ".relocation.lock", true)?;
    ensure!(
        !settings_parent.join(JOURNAL).exists(),
        "An interrupted move needs recovery. Restart the app before moving another folder"
    );
    let source = root(&request.source)?;
    let destination = root(&request.destination)?;
    let other = root(&request.other_root)?;
    ensure!(
        !overlaps(&source, &destination),
        "Source and destination folders must not overlap"
    );
    ensure!(
        !overlaps(&destination, &other) && !overlaps(&source, &other),
        "The driver cache and WorkingSpace must be separate, non-overlapping folders"
    );
    ensure!(
        !within(settings_parent, &source) && !within(&request.history_root, &source),
        "Application settings and history cannot be moved as storage"
    );
    ensure!(
        !destination.exists()
            || (destination.is_dir() && fs::read_dir(&destination)?.next().is_none()),
        "Choose an empty or nonexistent destination folder"
    );
    for protected in &request.protected_sources {
        ensure!(
            !within(protected, &source),
            "This folder contains the selected source ISO"
        );
    }
    fs::create_dir_all(&source)?;
    let _source_lock = cleanup::activity(
        &source,
        match request.kind {
            Kind::Cache => ".cache.lock",
            Kind::Workspace => ".build.lock",
        },
        true,
    )?;
    // Builds use both roots. Lock the companion root as well, including when
    // another app instance is using it with a different workspace selection.
    let _other_lock = cleanup::activity(
        &other,
        match request.kind {
            Kind::Cache => ".build.lock",
            Kind::Workspace => ".cache.lock",
        },
        true,
    )?;
    let entries = files(&source)?;
    if request.kind == Kind::Workspace && !entries.is_empty() {
        unmounted(&source)?;
    }
    let _history_lock = cleanup::history_lock(&request.history_root)?;
    let old_history = cleanup::read_history(&request.history_root)?;
    // Detect protected sources before copying, including sources recorded by
    // older jobs which have not yet been imported into history.
    let mut history = old_history.clone();
    if request.kind == Kind::Workspace {
        cleanup::import_jobs(&mut history, &source)?;
    }
    if request.kind == Kind::Workspace {
        for (relative, _) in &entries {
            if relative.file_name().is_some_and(|n| n == "job.json") {
                let job = source.join(relative).parent().unwrap().to_owned();
                cleanup::import_jobs(&mut history, job.parent().context("Job parent missing")?)?;
                ensure!(
                    history.records.iter().any(|r| key(&r.job) == key(&job)),
                    "Unrecognized build metadata at {}; original folder retained",
                    job.display()
                );
            }
        }
    }
    for r in &history.records {
        ensure!(
            !within(&r.source, &source),
            "This folder contains an original source ISO: {}",
            r.source.display()
        );
    }
    for (relative, _) in &entries {
        if relative
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("iso"))
        {
            let path = source.join(relative);
            ensure!(
                history.records.iter().any(|r| key(&r.output) == key(&path)
                    || r.partial.as_ref().is_some_and(|p| key(p) == key(&path))),
                "An unrecognized ISO is protected at {}; move it outside this folder first",
                path.display()
            );
        }
    }
    for r in &history.records {
        if !within(&r.job, &source) {
            ensure!(
                !within(&r.output, &source)
                    && !r
                        .connection_log
                        .as_ref()
                        .is_some_and(|p| within(p, &source)),
                "This folder contains artifacts of a build in another workspace; move those artifacts separately first"
            );
        }
    }
    let total_bytes = entries.iter().try_fold(0u64, |sum, (_, size)| {
        sum.checked_add(*size).context("Folder size overflow")
    })?;
    require_capacity(
        &destination,
        fs2::available_space(destination.parent().unwrap())?,
        total_bytes,
    )?;
    let id = uuid::Uuid::new_v4().to_string();
    let staging = destination
        .parent()
        .unwrap()
        .join(format!(".patcher-move-{id}"));
    let mut new_settings = request.settings.clone();
    new_settings[request.kind.field()] = Value::String(crate::platform::display_path(&destination));
    let mut journal = Journal {
        id: id.clone(),
        source: source.clone(),
        destination: destination.clone(),
        staging: staging.clone(),
        settings_path: request.settings_path.clone(),
        old_settings: request.settings,
        new_settings: new_settings.clone(),
        history_root: request.history_root.clone(),
        old_history: serde_json::to_value(&old_history)?,
        new_history: serde_json::to_value(&old_history)?,
        committed: false,
        destination_created: !destination.exists(),
    };
    let journal_path = settings_parent.join(JOURNAL);
    cleanup::write_json(&journal_path, &journal)?;
    let result = (|| -> Result<()> {
        fs::create_dir(&staging)?;
        fs::write(staging.join(MARKER), &id)?;
        let mut state = Progress {
            files: 0,
            total_files: entries.len() as u64,
            bytes: 0,
            total_bytes,
            phase: "Copying files".into(),
        };
        progress(state.clone());
        // Keep empty subdirectories as well as regular files.
        for entry in walkdir::WalkDir::new(&source)
            .follow_links(false)
            .min_depth(1)
        {
            let entry = entry?;
            if entry.file_type().is_dir() {
                fs::create_dir_all(staging.join(entry.path().strip_prefix(&source)?))?;
            }
        }
        let mut buffer = vec![0u8; 1024 * 1024];
        let mut hashes = Vec::new();
        let mut last_progress = std::time::Instant::now();
        for (relative, _) in &entries {
            use sha2::{Digest, Sha256};
            let mut input = fs::File::open(source.join(relative))?;
            let mut output = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(staging.join(relative))?;
            let mut hash = Sha256::new();
            loop {
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                output.write_all(&buffer[..n])?;
                hash.update(&buffer[..n]);
                state.bytes += n as u64;
                if last_progress.elapsed() >= std::time::Duration::from_millis(100) {
                    progress(state.clone());
                    last_progress = std::time::Instant::now();
                }
            }
            output.sync_all()?;
            hashes.push(hex::encode(hash.finalize()));
            state.files += 1;
        }
        state.phase = "Verifying copied files".into();
        state.files = 0;
        state.bytes = 0;
        progress(state.clone());
        for ((relative, size), hash) in entries.iter().zip(&hashes) {
            ensure!(
                download::sha256(&staging.join(relative), &Cancellation::default())? == *hash,
                "Copied file did not verify: {}",
                relative.display()
            );
            ensure!(
                download::sha256(&source.join(relative), &Cancellation::default())? == *hash,
                "Original files changed during the move; original location retained"
            );
            state.files += 1;
            state.bytes += size;
            progress(state.clone());
        }
        ensure!(
            files(&source)? == entries,
            "Folder contents changed during the move"
        );
        prepare_history(
            &mut history,
            &source,
            &staging,
            &destination,
            request.kind,
            &request.protected_sources,
        )?;
        journal.new_history = serde_json::to_value(&history)?;
        cleanup::write_json(&journal_path, &journal)?;
        state.phase = "Saving new folder location".into();
        progress(state);
        if destination.exists() {
            fs::remove_dir(&destination)?;
        }
        #[cfg(windows)]
        crate::platform::publish_iso(&staging, &destination)?;
        #[cfg(not(windows))]
        fs::rename(&staging, &destination)?;
        cleanup::save_history(&request.history_root, &history)?;
        save_settings(&request.settings_path, &new_settings)?;
        fs::write(source.join(MOVED), serde_json::to_vec(&new_settings)?)?;
        journal.committed = true;
        cleanup::write_json(&journal_path, &journal)?;
        Ok(())
    })();
    if let Err(error) = result {
        match rollback(&journal) {
            Ok(()) => {
                fs::remove_file(&journal_path)?;
            }
            Err(rollback_error) => {
                return Err(error.context(format!(
                    "Recovery is pending; restart the app. Rollback failed: {rollback_error:#}"
                )));
            }
        }
        return Err(error);
    }
    // Windows does not allow removing open lock files. The moved marker makes
    // old instances refuse new operations before the source lease is released.
    drop(_source_lock);
    let removal = (|| -> Result<()> {
        for entry in walkdir::WalkDir::new(&source).follow_links(false) {
            cleanup::safe_path(entry?.path())?;
        }
        fs::remove_dir_all(&source)?;
        Ok(())
    })();
    let mut warnings = Vec::new();
    if let Err(e) = removal {
        // remove_dir_all may have removed the marker before encountering a
        // locked file. Keep stale app instances away from retained contents.
        if cleanup::safe_path(&source).is_ok() && source.is_dir() {
            let _ = fs::write(source.join(MOVED), serde_json::to_vec(&new_settings)?);
        }
        warnings.push(format!(
            "The new location is saved. Some original files remain at {}: {e:#}",
            source.display()
        ));
    }
    match fs::remove_file(journal_path) {
        Ok(()) => {
            if let Err(e) = fs::remove_file(destination.join(MARKER)) {
                warnings.push(format!("Could not remove the move marker: {e}"));
            }
        }
        Err(e) => warnings.push(format!("Restart the app to finish saving this move: {e}")),
    }
    let warning = (!warnings.is_empty()).then(|| warnings.join("\n"));
    Ok(Outcome {
        settings: new_settings,
        warning,
    })
}

fn require_capacity(destination: &Path, free: u64, bytes: u64) -> Result<()> {
    let required = bytes
        .checked_add(1024 * 1024)
        .context("Folder size overflow")?;
    ensure!(
        free >= required,
        "Destination {} has {} free; moving requires at least {}",
        destination.display(),
        cleanup::format_bytes(free),
        cleanup::format_bytes(required)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("patcher-move-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        fn request(&self, kind: Kind) -> Request {
            fs::create_dir_all(self.path("state")).unwrap();
            let settings = serde_json::json!({"cache": self.path("cache"), "work": self.path("work"), "theme":"Dark"});
            save_settings(&self.path("state/settings.json"), &settings).unwrap();
            Request {
                kind,
                source: self.path(if kind == Kind::Cache { "cache" } else { "work" }),
                destination: self.path("destination"),
                other_root: self.path(if kind == Kind::Cache { "work" } else { "cache" }),
                protected_sources: vec![],
                settings_path: self.path("state/settings.json"),
                settings,
                history_root: self.path("history"),
                gate: Gate {
                    model_selection: true,
                    before_build: true,
                    idle: true,
                },
            }
        }
        fn file(&self, relative: &str, contents: &[u8]) -> PathBuf {
            let path = self.path(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn moves_cache_and_persists_exact_destination() {
        let f = Fixture::new();
        let request = f.request(Kind::Cache);
        f.file("cache/package.pkg", b"verified content");
        fs::create_dir_all(f.path("cache/empty")).unwrap();
        fs::create_dir(f.path("destination")).unwrap();
        let outcome = execute(request, &|_| {}).unwrap();
        assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
        assert!(!f.path("cache").exists());
        assert_eq!(
            fs::read(f.path("destination/package.pkg")).unwrap(),
            b"verified content"
        );
        assert!(f.path("destination/empty").is_dir());
        let saved: Value =
            serde_json::from_slice(&fs::read(f.path("state/settings.json")).unwrap()).unwrap();
        assert_eq!(saved, outcome.settings);
        assert_eq!(saved["theme"], "Dark");
        assert!(
            cleanup::same_path(
                Path::new(saved["cache"].as_str().unwrap()),
                &f.path("destination")
            )
            .unwrap()
        );
    }
    #[test]
    fn empty_missing_folder_can_move() {
        let f = Fixture::new();
        let request = f.request(Kind::Cache);
        execute(request, &|_| {}).unwrap();
        assert!(f.path("destination").is_dir());
        assert!(!f.path("cache").exists());
    }
    #[test]
    fn conflicts_and_overlaps_preserve_source() {
        let f = Fixture::new();
        let request = f.request(Kind::Cache);
        f.file("cache/keep", b"keep");
        f.file("destination/keep", b"unrelated");
        assert!(
            execute(request, &|_| {})
                .unwrap_err()
                .to_string()
                .contains("empty")
        );
        let mut request = f.request(Kind::Cache);
        request.destination = f.path("cache/nested");
        assert!(execute(request, &|_| {}).is_err());
        assert_eq!(fs::read(f.path("cache/keep")).unwrap(), b"keep");
        assert_eq!(fs::read(f.path("destination/keep")).unwrap(), b"unrelated");
    }
    #[test]
    fn gates_and_locks_are_enforced_by_backend() {
        let f = Fixture::new();
        let mut request = f.request(Kind::Cache);
        request.gate.model_selection = false;
        assert!(
            execute(request, &|_| {})
                .unwrap_err()
                .to_string()
                .contains("Return to model")
        );
        let mut request = f.request(Kind::Cache);
        request.gate.idle = false;
        assert!(execute(request, &|_| {}).is_err());
        let request = f.request(Kind::Cache);
        let _lease = cleanup::cache_activity(&request.source).unwrap();
        assert!(execute(request, &|_| {}).is_err());
        assert!(!f.path("destination").exists());
        assert!(require_capacity(&f.path("destination"), 100, 100).is_err());
    }

    #[test]
    fn workspace_moves_are_allowed_during_setup_but_not_builds() {
        let f = Fixture::new();
        let mut request = f.request(Kind::Workspace);
        request.gate.model_selection = false;
        assert!(request.gate.check(Kind::Cache).is_err());
        execute(request, &|_| {}).unwrap();
        assert!(f.path("destination").is_dir());

        let blocked = Fixture::new();
        let mut request = blocked.request(Kind::Workspace);
        request.gate.model_selection = false;
        request.gate.before_build = false;
        assert!(
            execute(request, &|_| {})
                .unwrap_err()
                .to_string()
                .contains("WorkingSpace")
        );
        assert!(!blocked.path("destination").exists());
    }
    #[test]
    fn verification_failure_restores_settings_and_source() {
        let f = Fixture::new();
        let request = f.request(Kind::Cache);
        f.file("cache/package", b"before");
        let changed = Cell::new(false);
        let result = execute(request, &|p| {
            if p.phase == "Verifying copied files" && !changed.replace(true) {
                fs::write(f.path("cache/package"), b"after!").unwrap();
            }
        });
        assert!(result.is_err());
        assert_eq!(fs::read(f.path("cache/package")).unwrap(), b"after!");
        assert!(!f.path("destination").exists());
        assert!(!f.path("state/storage-relocation.json").exists());
        let saved: Value =
            serde_json::from_slice(&fs::read(f.path("state/settings.json")).unwrap()).unwrap();
        assert!(saved["cache"].as_str().unwrap().ends_with("cache"));
    }
    #[test]
    fn local_settings_import_once_and_keep_all_preferences() {
        let f = Fixture::new();
        let legacy = f.file(
            "legacy/settings.json",
            br#"{"work":"F:\\work","cache":"F:\\cache","theme":"Dark","other":42}"#,
        );
        let local = f.path("state/settings.json");
        let first = load_settings(&local, &legacy).unwrap().unwrap();
        fs::write(&legacy, b"{}").unwrap();
        let second = load_settings(&local, &legacy).unwrap().unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&first).unwrap(),
            serde_json::from_slice::<Value>(&second).unwrap()
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&second).unwrap()["other"],
            42
        );
    }

    #[test]
    fn concurrent_startup_waits_for_another_recovery() {
        let f = Fixture::new();
        let request = f.request(Kind::Cache);
        let lease = cleanup::activity(
            request.settings_path.parent().unwrap(),
            ".relocation.lock",
            true,
        )
        .unwrap();
        let thread = std::thread::spawn(move || recover(&request.settings_path));
        std::thread::sleep(std::time::Duration::from_millis(50));
        drop(lease);
        assert!(thread.join().unwrap().unwrap().is_none());
    }
    #[test]
    fn interrupted_moves_roll_back_or_finish_without_deleting_originals() {
        for committed in [false, true] {
            let f = Fixture::new();
            let request = f.request(Kind::Cache);
            f.file("cache/keep", b"original");
            let id = uuid::Uuid::new_v4().to_string();
            let staging = f.path(&format!(".patcher-move-{id}"));
            fs::create_dir(&request.destination).unwrap();
            fs::write(request.destination.join(MARKER), &id).unwrap();
            fs::write(request.destination.join("keep"), b"original").unwrap();
            fs::create_dir_all(&request.history_root).unwrap();
            let mut new_settings = request.settings.clone();
            new_settings["cache"] = serde_json::to_value(&request.destination).unwrap();
            let journal = Journal {
                id,
                source: request.source,
                destination: request.destination,
                staging,
                settings_path: request.settings_path.clone(),
                old_settings: request.settings,
                new_settings,
                history_root: request.history_root,
                old_history: serde_json::json!({"records":[]}),
                new_history: serde_json::json!({"records":[]}),
                committed,
                destination_created: true,
            };
            cleanup::write_json(&f.path("state/storage-relocation.json"), &journal).unwrap();
            save_settings(&journal.settings_path, &journal.new_settings).unwrap();
            recover(&journal.settings_path).unwrap().unwrap();
            assert_eq!(fs::read(f.path("cache/keep")).unwrap(), b"original");
            assert_eq!(f.path("destination").exists(), committed);
            let saved: Value =
                serde_json::from_slice(&fs::read(&journal.settings_path).unwrap()).unwrap();
            assert_eq!(
                saved,
                if committed {
                    journal.new_settings
                } else {
                    journal.old_settings
                }
            );
        }
    }
    #[test]
    fn moves_between_actual_volumes_when_available() {
        let f = Fixture::new();
        let mut request = f.request(Kind::Cache);
        let other_parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        if key(f.0.ancestors().last().unwrap()) == key(other_parent.ancestors().last().unwrap()) {
            return;
        }
        let destination = other_parent.join(format!("patcher-move-test-{}", uuid::Uuid::new_v4()));
        request.destination = destination.clone();
        f.file("cache/package", b"cross-volume data");
        let result = execute(request, &|_| {});
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            fs::read(destination.join("package")).unwrap(),
            b"cross-volume data"
        );
        fs::remove_dir_all(destination).unwrap();
    }
    #[test]
    fn source_isos_and_retained_mounts_are_protected() {
        let f = Fixture::new();
        let request = f.request(Kind::Cache);
        f.file("cache/original.iso", b"source");
        assert!(execute(request, &|_| {}).is_err());
        let request = f.request(Kind::Workspace);
        f.file("work/build-old/mount-install/Windows/system", b"mounted");
        assert!(
            execute(request, &|_| {})
                .unwrap_err()
                .to_string()
                .contains("mount")
        );
        assert!(
            f.path("work/build-old/mount-install/Windows/system")
                .exists()
        );
    }
    #[cfg(windows)]
    #[test]
    fn sharing_violations_and_junctions_preserve_originals() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        let request = f.request(Kind::Cache);
        let path = f.file("cache/package", b"locked");
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(execute(request, &|_| {}).is_err());
        drop(handle);
        let request = f.request(Kind::Cache);
        let outside = f.file("outside/keep", b"protected");
        let link = f.path("cache/linked");
        crate::platform::powershell("$null=New-Item -ItemType Junction -Path $env:MOVE_TEST_LINK -Target $env:MOVE_TEST_TARGET", &[("MOVE_TEST_LINK", link.as_os_str()), ("MOVE_TEST_TARGET", outside.parent().unwrap().as_os_str())]).unwrap();
        assert!(execute(request, &|_| {}).is_err());
        fs::remove_dir(link).unwrap();
        assert_eq!(fs::read(outside).unwrap(), b"protected");
    }

    #[cfg(windows)]
    #[test]
    fn cleanup_failure_keeps_verified_destination_selected() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        let request = f.request(Kind::Cache);
        let path = f.file("cache/package", b"retained until unlocked");
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&path)
            .unwrap();
        let result = execute(request, &|_| {}).unwrap();
        assert!(result.warning.is_some());
        assert_eq!(
            fs::read(f.path("destination/package")).unwrap(),
            b"retained until unlocked"
        );
        assert!(
            cleanup::same_path(
                Path::new(result.settings["cache"].as_str().unwrap()),
                &f.path("destination")
            )
            .unwrap()
        );
        assert!(f.path("cache/.storage-moved.json").exists());
        drop(handle);
    }
    #[test]
    fn workspace_move_preserves_build_ownership_and_paths() {
        let f = Fixture::new();
        let request = f.request(Kind::Workspace);
        let source = f.file("original.iso", b"original");
        let output = f.file("work/patched.iso", b"generated");
        let id = uuid::Uuid::new_v4().to_string();
        let job = f.path(&format!("work/build-{id}"));
        fs::create_dir_all(&job).unwrap();
        let connection_log = f.file(
            &format!("work/worker-connection-{}.log", uuid::Uuid::new_v4()),
            b"log",
        );
        let req = BuildRequest {
            inventory: crate::domain::Inventory {
                iso: source.clone(),
                iso_size: 8,
                iso_modified: 0,
                install_file: "install.wim".into(),
                setup_image: None,
                images: vec![],
            },
            selected: vec![1],
            model: crate::domain::MODEL_ID.into(),
            output: output.clone(),
            workspace: f.path("work"),
            cache: f.path("cache"),
            seven_zip: PathBuf::new(),
            oscdimg: PathBuf::new(),
        };
        cleanup::write_json(&job.join("job.json"), &req).unwrap();
        let record = cleanup::BuildRecord {
            id: id.clone(),
            job: job.clone(),
            workspace: req.workspace.clone(),
            source: source.clone(),
            output: output.clone(),
            partial: None,
            connection_log: Some(connection_log),
            model: req.model.clone(),
            editions: vec![],
            created: 0,
            state: cleanup::BuildState::Complete,
            published: Some(cleanup::fingerprint(&output).unwrap()),
        };
        cleanup::write_json(&job.join("build-state.json"), &record).unwrap();
        fs::create_dir_all(&request.history_root).unwrap();
        cleanup::save_history(
            &request.history_root,
            &cleanup::History {
                records: vec![record],
            },
        )
        .unwrap();
        let history_root = request.history_root.clone();
        let result = execute(request, &|_| {}).unwrap();
        assert!(result.warning.is_none(), "{result:?}");
        let history = cleanup::read_history(&history_root).unwrap();
        let record = &history.records[0];
        cleanup::validate_record(record).unwrap();
        assert!(record.job.is_dir());
        assert!(record.connection_log.as_ref().unwrap().is_file());
        assert_eq!(
            Some(cleanup::fingerprint(&record.output).unwrap()),
            record.published
        );
        let moved: BuildRequest =
            serde_json::from_slice(&fs::read(record.job.join("job.json")).unwrap()).unwrap();
        assert_eq!(moved.output, record.output);
        assert_eq!(moved.workspace, record.workspace);
        assert_eq!(moved.inventory.iso, source);
        assert!(source.is_file());
        assert!(!job.exists());
    }
}
