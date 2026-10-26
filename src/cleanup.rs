//! App-owned storage inventory, history and bounded deletion.
use crate::{domain::*, driver_prefetch, models::MODEL_PROFILES, platform};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

pub struct ActivityLock(File);
impl Drop for ActivityLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}
pub fn activity(root: &Path, name: &str, exclusive: bool) -> Result<ActivityLock> {
    safe_path(root)?;
    ensure!(
        !root.join(".storage-moved.json").exists(),
        "This folder was moved by another app instance. Restart the app to use the new location"
    );
    fs::create_dir_all(root)?;
    let path = root.join(name);
    safe_path(&path)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    if exclusive {
        fs2::FileExt::try_lock_exclusive(&file)
    } else {
        fs2::FileExt::try_lock_shared(&file)
    }
    .context("Storage is in use by another operation. Wait for it to finish, then retry")?;
    Ok(ActivityLock(file))
}
pub fn cache_activity(root: &Path) -> Result<ActivityLock> {
    activity(root, ".cache.lock", false)
}
pub(crate) fn history_lock(root: &Path) -> Result<ActivityLock> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match activity(root, ".history.lock", true) {
            Ok(lock) => return Ok(lock),
            Err(e) if lock_busy(&e) && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Err(e) => return Err(e),
        }
    }
}

pub(crate) fn lock_busy(error: &anyhow::Error) -> bool {
    error.chain().any(|e| {
        e.downcast_ref::<std::io::Error>().is_some_and(|e| {
            e.kind() == std::io::ErrorKind::WouldBlock
                || (cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)))
        })
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Fingerprint {
    pub size: u64,
    pub modified: u128,
    pub identity: String,
}
pub fn fingerprint(path: &Path) -> Result<Fingerprint> {
    safe_path(path)?;
    let file = File::open(path)?;
    let meta = file.metadata()?;
    ensure!(
        meta.is_file(),
        "Expected a regular file: {}",
        path.display()
    );
    let modified = meta.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
    #[cfg(windows)]
    let identity = {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        #[derive(Default)]
        struct Info {
            attributes: u32,
            created: [u32; 2],
            accessed: [u32; 2],
            written: [u32; 2],
            volume: u32,
            size_high: u32,
            size_low: u32,
            links: u32,
            index_high: u32,
            index_low: u32,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut Info) -> i32;
        }
        let mut info = Info::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        ensure!(
            info.links == 1,
            "Files with hard links cannot be automatically deleted"
        );
        format!("{}:{}:{}", info.volume, info.index_high, info.index_low)
    };
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            meta.nlink() == 1,
            "Files with hard links cannot be automatically deleted"
        );
        format!("{}:{}", meta.dev(), meta.ino())
    };
    Ok(Fingerprint {
        size: meta.len(),
        modified,
        identity,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuildState {
    Building,
    Complete,
    Failed,
    Cancelled,
    Unknown,
}
impl BuildState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Building => "Interrupted / in progress",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::Unknown => "Incomplete / unknown",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRecord {
    pub id: String,
    pub job: PathBuf,
    pub workspace: PathBuf,
    pub source: PathBuf,
    pub output: PathBuf,
    pub partial: Option<PathBuf>,
    pub connection_log: Option<PathBuf>,
    pub model: String,
    pub editions: Vec<String>,
    pub created: u64,
    pub state: BuildState,
    pub published: Option<Fingerprint>,
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub(crate) struct History {
    pub(crate) records: Vec<BuildRecord>,
}
pub fn history_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "Zenodeon", "MacBookISOPatcher")
        .expect("User profile required")
        .config_dir()
        .to_owned()
}
pub(crate) fn read_history(root: &Path) -> Result<History> {
    let path = root.join("build-history.json");
    safe_path(&path)?;
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .context("Build history could not be read; it has been preserved"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(History::default()),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    safe_path(path)?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    {
        use std::io::Write;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(temp);
        return Err(error.into());
    }
    Ok(())
}
pub(crate) fn save_history(root: &Path, history: &History) -> Result<()> {
    write_json(&root.join("build-history.json"), history)
}
fn request_record(job: &Path, req: &BuildRequest) -> Result<BuildRecord> {
    let id = job
        .file_name()
        .and_then(|v| v.to_str())
        .and_then(|v| v.strip_prefix("build-"))
        .context("Invalid build directory")?;
    uuid::Uuid::parse_str(id)?;
    ensure!(
        job.parent().map(fs::canonicalize).transpose()? == Some(req.workspace.canonicalize()?),
        "Build directory escaped its workspace"
    );
    ensure!(
        MODEL_PROFILES.iter().any(|m| m.id == req.model),
        "Unknown build model"
    );
    safe_path(job)?;
    safe_path(&req.output)?;
    ensure!(
        req.inventory.iso.is_absolute(),
        "Source ISO path must be absolute"
    );
    ensure!(
        !same_path(&req.output, &req.inventory.iso)?,
        "The original ISO is protected"
    );
    ensure!(
        req.output
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("iso")),
        "Invalid output ISO path"
    );
    Ok(BuildRecord {
        id: id.into(),
        job: job.to_owned(),
        workspace: req.workspace.clone(),
        source: req.inventory.iso.clone(),
        output: req.output.clone(),
        partial: None,
        connection_log: None,
        model: req.model.clone(),
        editions: req
            .selected_images()?
            .iter()
            .map(|v| v.name.clone())
            .collect(),
        created: platform::now(),
        state: BuildState::Building,
        published: None,
    })
}
pub fn register(job: &Path, req: &BuildRequest, connection_log: Option<PathBuf>) -> Result<()> {
    let root = history_dir();
    let _lock = history_lock(&root)?;
    let mut history = read_history(&root)?;
    let mut record = request_record(job, req)?;
    record.connection_log = connection_log;
    validate_record(&record)?;
    write_json(&job.join("build-state.json"), &record)?;
    history.records.retain(|r| r.id != record.id);
    history.records.push(record);
    save_history(&root, &history)
}
fn update_record(job: &Path, change: impl FnOnce(&mut BuildRecord) -> Result<()>) -> Result<()> {
    let root = history_dir();
    let _lock = history_lock(&root)?;
    let mut history = read_history(&root)?;
    let record = history
        .records
        .iter_mut()
        .find(|r| r.job == job)
        .context("Build history entry is missing")?;
    change(record)?;
    validate_record(record)?;
    write_json(&job.join("build-state.json"), record)?;
    save_history(&root, &history)
}
pub fn track_partial(job: &Path, path: &Path) -> Result<()> {
    update_record(job, |r| {
        r.partial = Some(path.to_owned());
        Ok(())
    })
}
pub fn finish(job: &Path, state: BuildState) -> Result<()> {
    update_record(job, |r| {
        if state == BuildState::Complete {
            if r.published.is_none() {
                r.published = Some(fingerprint(&r.output)?);
            }
            r.partial = None;
        }
        r.state = state;
        Ok(())
    })
}
pub fn published(job: &Path) -> Result<()> {
    finish(job, BuildState::Complete)
}

pub fn safe_path(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute(),
        "Cleanup requires an absolute path: {}",
        path.display()
    );
    for ancestor in path.ancestors() {
        ensure!(
            !ancestor
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
            "Parent traversal is not allowed"
        );
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => ensure!(
                !reparse(&meta),
                "Links and junctions are protected: {}",
                ancestor.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
pub(crate) fn same_path(a: &Path, b: &Path) -> Result<bool> {
    let a = if a.exists() {
        a.canonicalize()?
    } else {
        std::path::absolute(a)?
    };
    let b = if b.exists() {
        b.canonicalize()?
    } else {
        std::path::absolute(b)?
    };
    #[cfg(windows)]
    {
        Ok(a.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .eq_ignore_ascii_case(b.to_string_lossy().trim_start_matches(r"\\?\")))
    }
    #[cfg(not(windows))]
    {
        Ok(a == b)
    }
}
pub(crate) fn validate_record(r: &BuildRecord) -> Result<()> {
    uuid::Uuid::parse_str(&r.id)?;
    ensure!(r.source.is_absolute(), "Source ISO path must be absolute");
    for p in [&r.job, &r.workspace, &r.output] {
        safe_path(p)?;
    }
    ensure!(
        r.job.file_name().and_then(|v| v.to_str()) == Some(format!("build-{}", r.id).as_str()),
        "Build identity does not match its folder"
    );
    ensure!(
        r.job
            .parent()
            .is_some_and(|p| same_path(p, &r.workspace).unwrap_or(false)),
        "Build folder escaped its workspace"
    );
    ensure!(
        !same_path(&r.source, &r.output)?,
        "Original ISO is protected"
    );
    ensure!(
        r.output
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("iso")),
        "Invalid ISO output"
    );
    if let Some(path) = &r.partial {
        safe_path(path)?;
        ensure!(
            !path.exists() || path.is_file(),
            "A temporary ISO path now points to a directory; it has been preserved"
        );
        ensure!(
            path.parent()
                .is_some_and(|p| same_path(p, r.output.parent().unwrap()).unwrap_or(false)),
            "Temporary ISO escaped its output folder"
        );
        let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
        ensure!(
            name.strip_prefix(".a1706-")
                .and_then(|s| s.strip_suffix(".partial.iso"))
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok()),
            "Unrecognized temporary ISO"
        );
    }
    if let Some(path) = &r.connection_log {
        safe_path(path)?;
        ensure!(
            !path.exists() || path.is_file(),
            "A connection log path now points to a directory; it has been preserved"
        );
        ensure!(
            path.parent()
                .is_some_and(|p| same_path(p, &r.workspace).unwrap_or(false)),
            "Connection log escaped its workspace"
        );
        ensure!(
            path.file_name()
                .and_then(|v| v.to_str())
                .and_then(|s| s.strip_prefix("worker-connection-"))
                .and_then(|s| s.strip_suffix(".log"))
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok()),
            "Unrecognized connection log"
        );
    }
    Ok(())
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StorageSize {
    pub files: u64,
    pub bytes: u64,
}
pub fn tree_size(path: &Path) -> Result<StorageSize> {
    safe_path(path)?;
    if !path.exists() {
        return Ok(StorageSize::default());
    }
    let mut size = StorageSize::default();
    for entry in WalkDir::new(path).follow_links(false) {
        let entry = entry?;
        let meta = fs::symlink_metadata(entry.path())?;
        ensure!(
            !reparse(&meta),
            "Links and junctions are protected: {}",
            entry.path().display()
        );
        if meta.is_file() {
            size.files += 1;
            size.bytes += meta.len();
        }
    }
    Ok(size)
}
// Mounted image contents already live in the WIM. Never walk into them while
// estimating a job; mount ownership is checked and the image is discarded first.
fn job_size(path: &Path) -> Result<StorageSize> {
    safe_path(path)?;
    if !path.exists() {
        return Ok(StorageSize::default());
    }
    let mut size = StorageSize::default();
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !(e.path().parent() == Some(path)
                && (e.file_name() == "mount-install" || e.file_name() == "mount-boot"))
        })
    {
        let entry = entry?;
        let meta = fs::symlink_metadata(entry.path())?;
        ensure!(
            !reparse(&meta),
            "Links and junctions are protected: {}",
            entry.path().display()
        );
        if meta.is_file() {
            size.files += 1;
            size.bytes += meta.len();
        }
    }
    Ok(size)
}
#[derive(Debug, Default, Clone)]
pub struct CacheInventory {
    pub paths: Vec<PathBuf>,
    pub size: StorageSize,
}
fn package_names() -> Vec<&'static str> {
    let mut names = Vec::new();
    for model in MODEL_PROFILES {
        if let Ok(packages) = driver_prefetch::packages_for_model(model.id) {
            for p in packages {
                if !names.contains(&p.filename) {
                    names.push(p.filename);
                }
            }
        }
    }
    names
}
pub fn cache_inventory(root: &Path) -> Result<CacheInventory> {
    safe_path(root)?;
    let mut cache = CacheInventory::default();
    if !root.exists() {
        return Ok(cache);
    }
    let packages = package_names();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let package = packages.iter().any(|n| {
            name == *n
                || name
                    .strip_prefix(&format!("{n}."))
                    .and_then(|s| s.strip_suffix(".part"))
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        });
        let extraction = name
            .strip_prefix("extract-")
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok());
        if (package && entry.file_type()?.is_file()) || (extraction && entry.file_type()?.is_dir())
        {
            let size = tree_size(&path)?;
            cache.size.files += size.files;
            cache.size.bytes += size.bytes;
            cache.paths.push(path.clone());
        }
        if name == "adk-setup" {
            safe_path(&path)?;
            for setup in fs::read_dir(&path)? {
                let setup = setup?;
                if uuid::Uuid::parse_str(&setup.file_name().to_string_lossy()).is_err() {
                    continue;
                }
                let setup_path = setup.path();
                safe_path(&setup_path)?;
                if !setup_path.is_dir() {
                    continue;
                }
                let recognized = fs::read_dir(&setup_path)?.all(|v| {
                    v.is_ok_and(|v| {
                        v.file_name() == "adksetup.exe" || v.file_name() == "adksetup.part"
                    })
                });
                if recognized {
                    let size = tree_size(&setup_path)?;
                    cache.size.files += size.files;
                    cache.size.bytes += size.bytes;
                    cache.paths.push(setup_path);
                }
            }
        }
    }
    Ok(cache)
}
#[derive(Debug, Clone)]
pub struct ArtifactView {
    pub path: PathBuf,
    pub present: bool,
    pub size: StorageSize,
    pub problem: Option<String>,
}
impl ArtifactView {
    pub fn available(&self) -> bool {
        self.present && self.problem.is_none()
    }
    fn missing(path: PathBuf) -> Self {
        Self {
            path,
            present: false,
            size: StorageSize::default(),
            problem: None,
        }
    }
}
fn scan_artifact(path: &Path, directory: bool) -> ArtifactView {
    scan_artifact_with(path, directory, &|p| fs::symlink_metadata(p))
}
fn scan_artifact_with(
    path: &Path,
    directory: bool,
    metadata: &impl Fn(&Path) -> std::io::Result<fs::Metadata>,
) -> ArtifactView {
    let mut view = ArtifactView::missing(path.to_owned());
    let result = (|| -> Result<()> {
        safe_path(path)?;
        let meta = match metadata(path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        ensure!(
            if directory {
                meta.is_dir()
            } else {
                meta.is_file()
            },
            "Location has the wrong file type: {}",
            platform::display_path(path)
        );
        view.present = true;
        view.size = if directory {
            job_size(path)?
        } else {
            tree_size(path)?
        };
        Ok(())
    })();
    view.problem = result
        .err()
        .map(|e| format!("{}: {e:#}", platform::display_path(path)));
    view
}
#[derive(Debug, Clone)]
pub struct BuildView {
    pub workspace: ArtifactView,
    pub output: ArtifactView,
    pub partial: Option<ArtifactView>,
    pub connection_log: Option<ArtifactView>,
    pub verified_iso: bool,
    pub record: BuildRecord,
    pub size: StorageSize,
    pub problem: Option<String>,
}
impl BuildView {
    pub fn workspace_artifacts(&self) -> impl Iterator<Item = &ArtifactView> {
        [&self.workspace]
            .into_iter()
            .chain(self.partial.iter())
            .chain(self.connection_log.iter())
            .filter(|a| a.path.starts_with(&self.record.workspace) && a.path != self.record.output)
    }
    pub fn workspace_visible(&self) -> bool {
        self.workspace_artifacts()
            .any(|a| a.present || a.problem.is_some())
    }
    pub fn workspace_problem(&self) -> Option<String> {
        if let Err(e) = validate_record(&self.record) {
            return Some(format!("{e:#}"));
        }
        if self.output.present && self.output.path.starts_with(&self.record.job) {
            return Some(
                "Move the generated ISO outside this workspace before deleting it.".into(),
            );
        }
        let errors: Vec<_> = self
            .workspace_artifacts()
            .filter_map(|a| a.problem.clone())
            .collect();
        (!errors.is_empty()).then(|| errors.join("\n"))
    }
    pub fn workspace_bytes(&self) -> u64 {
        self.workspace_artifacts()
            .filter(|a| {
                a.available()
                    && (a.path == self.workspace.path
                        || !self.workspace.available()
                        || !a.path.starts_with(&self.workspace.path))
            })
            .map(|a| a.size.bytes)
            .sum::<u64>()
            .saturating_sub(
                if self.output.available()
                    && self.workspace.available()
                    && self.output.path.starts_with(&self.workspace.path)
                {
                    self.output.size.bytes
                } else {
                    0
                },
            )
    }
    pub fn workspace_title(&self) -> String {
        self.record
            .job
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
    pub fn workspace_status(&self) -> &str {
        if self.workspace_problem().is_some() {
            "Needs attention"
        } else if !self.workspace.available() {
            "Workspace logs only"
        } else if self.record.state == BuildState::Complete {
            "Workspace retained"
        } else {
            self.record.state.label()
        }
    }
    pub fn can_delete_workspace(&self) -> bool {
        self.workspace_problem().is_none() && self.workspace_artifacts().any(|a| a.available())
    }
    pub fn artifacts(&self) -> impl Iterator<Item = &ArtifactView> {
        [&self.workspace, &self.output]
            .into_iter()
            .chain(self.partial.iter())
            .chain(self.connection_log.iter())
    }
    pub fn visible(&self) -> bool {
        self.artifacts().any(|a| a.present || a.problem.is_some()) || self.problem.is_some()
    }
    pub fn can_delete_iso(&self) -> bool {
        self.verified_iso && self.output.available() && self.problem.is_none()
    }
    pub fn can_delete(&self) -> bool {
        self.problem.is_none()
            && (self.workspace.available()
                || self.can_delete_iso()
                || self
                    .partial
                    .iter()
                    .chain(self.connection_log.iter())
                    .any(|a| a.available()))
    }
    pub fn title(&self) -> String {
        let Some(path) = self
            .artifacts()
            .find(|a| a.available() && a.path == self.output.path)
            .or_else(|| self.artifacts().find(|a| a.available()))
            .map(|a| &a.path)
        else {
            return self.record.model.clone();
        };
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
    pub fn status(&self) -> String {
        if self.problem.is_some() {
            return "Needs attention".into();
        }
        let location = match (self.workspace.available(), self.output.available()) {
            (true, true) => "",
            (true, false) => "workspace only",
            (false, true) => "ISO only",
            (false, false) => "build files only",
        };
        let state = if self.output.available() && !self.verified_iso {
            "Unverified output"
        } else if self.record.state == BuildState::Complete && !self.verified_iso {
            "ISO missing"
        } else {
            self.record.state.label()
        };
        if location.is_empty() {
            state.into()
        } else {
            format!("{state} · {location}")
        }
    }
    pub fn complete(&self) -> bool {
        self.record.state == BuildState::Complete && self.can_delete_iso()
    }
    pub fn deletable_artifacts(&self, iso_only: bool) -> Vec<&ArtifactView> {
        if iso_only {
            return if self.can_delete_iso() {
                vec![&self.output]
            } else {
                vec![]
            };
        }
        [&self.workspace]
            .into_iter()
            .chain(self.partial.iter())
            .chain(self.connection_log.iter())
            .chain(self.verified_iso.then_some(&self.output))
            .filter(|a| a.available())
            .collect()
    }
}
#[derive(Debug, Default, Clone)]
pub struct Inventory {
    pub cache: CacheInventory,
    pub builds: Vec<BuildView>,
    pub warnings: Vec<String>,
}
impl Inventory {
    pub fn workspace_problem(&self, view: &BuildView) -> Option<String> {
        if self
            .builds
            .iter()
            .any(|v| v.output.present && v.output.path.starts_with(&view.record.job))
        {
            return Some(
                "Move the generated ISO outside this workspace before deleting it.".into(),
            );
        }
        view.workspace_problem()
    }
    pub fn workspace_bytes(&self, view: &BuildView) -> u64 {
        let others: std::collections::BTreeMap<_, _> = self
            .builds
            .iter()
            .filter(|v| {
                v.output.available()
                    && v.output.path != view.output.path
                    && view.workspace.available()
                    && v.output.path.starts_with(&view.workspace.path)
            })
            .map(|v| (v.output.path.clone(), v.output.size.bytes))
            .collect();
        view.workspace_bytes().saturating_sub(others.values().sum())
    }
    pub fn space_label(&self) -> &'static str {
        if !self.warnings.is_empty()
            || self
                .builds
                .iter()
                .filter(|v| v.workspace_visible())
                .any(|v| self.workspace_problem(v).is_some())
        {
            "Known space used"
        } else {
            "Total space used"
        }
    }
    pub fn workspace_count(&self) -> usize {
        self.builds
            .iter()
            .filter(|v| v.workspace.available())
            .map(|v| v.workspace.path.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }
    pub fn total_bytes(&self) -> u64 {
        let artifacts: std::collections::BTreeMap<_, _> = self
            .builds
            .iter()
            .flat_map(BuildView::workspace_artifacts)
            .filter(|a| a.available())
            .map(|a| (a.path.clone(), a.size.bytes))
            .collect();
        artifacts
            .iter()
            .filter(|(path, _)| {
                !artifacts
                    .keys()
                    .any(|other| other != *path && path.starts_with(other))
            })
            .map(|(_, bytes)| *bytes)
            .sum::<u64>()
            .saturating_sub(
                self.builds
                    .iter()
                    .filter(|v| {
                        v.output.available()
                            && artifacts.keys().any(|p| v.output.path.starts_with(p))
                    })
                    .map(|v| (v.output.path.clone(), v.output.size.bytes))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .values()
                    .sum(),
            )
    }
}

pub fn inspect_build(record: BuildRecord) -> BuildView {
    let workspace = scan_artifact(&record.job, true);
    let output = scan_artifact(&record.output, false);
    let partial = record.partial.as_deref().map(|p| scan_artifact(p, false));
    let connection_log = record
        .connection_log
        .as_deref()
        .map(|p| scan_artifact(p, false));
    let mut errors = Vec::new();
    if let Err(e) = validate_record(&record) {
        errors.push(format!("{e:#}"));
    }
    let mut verified_iso = false;
    if output.available()
        && let Some(published) = &record.published
    {
        match fingerprint(&record.output) {
            Ok(actual) if &actual == published => verified_iso = true,
            Ok(_) => errors
                .push("The generated ISO changed or was replaced; it has been preserved".into()),
            Err(e) => errors.push(format!("ISO could not be checked: {e:#}")),
        }
    }
    let mut size = StorageSize::default();
    for artifact in [&workspace, &output]
        .into_iter()
        .chain(partial.iter())
        .chain(connection_log.iter())
    {
        if let Some(error) = &artifact.problem {
            errors.push(error.clone());
        }
        // Don't count a file twice if it lives inside the job directory.
        if artifact.path == workspace.path
            || !workspace.available()
            || !artifact.path.starts_with(&workspace.path)
        {
            size.bytes += artifact.size.bytes;
            size.files += artifact.size.files;
        }
    }
    BuildView {
        workspace,
        output,
        partial,
        connection_log,
        verified_iso,
        record,
        size,
        problem: (!errors.is_empty()).then(|| errors.join("\n")),
    }
}

pub(crate) fn import_jobs(history: &mut History, workspace: &Path) -> Result<()> {
    safe_path(workspace)?;
    if !workspace.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(workspace)? {
        let path = entry?.path();
        if !path
            .file_name()
            .and_then(|v| v.to_str())
            .and_then(|s| s.strip_prefix("build-"))
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
            || history
                .records
                .iter()
                .any(|r| same_path(&r.job, &path).unwrap_or(false))
        {
            continue;
        }
        safe_path(&path)?;
        let Ok(bytes) = fs::read(path.join("job.json")) else {
            continue;
        };
        let Ok(req) = serde_json::from_slice::<BuildRequest>(&bytes) else {
            continue;
        };
        let Ok(mut record) = request_record(&path, &req) else {
            continue;
        };
        if let Ok(bytes) = fs::read(path.join("build-state.json")) {
            if let Ok(saved) = serde_json::from_slice::<BuildRecord>(&bytes)
                && saved.id == record.id
                && same_path(&saved.job, &record.job).unwrap_or(false)
                && validate_record(&saved).is_ok()
            {
                record = saved;
            }
        } else {
            record.state = BuildState::Unknown;
            record.created = fs::metadata(&path)?
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_secs();
            if fs::read_to_string(path.join("build.log"))
                .is_ok_and(|s| s.contains("Completed. Build log:"))
                && record.output.is_file()
            {
                record.state = BuildState::Complete;
                record.published = Some(fingerprint(&record.output)?);
            }
        }
        history.records.push(record);
    }
    Ok(())
}
pub fn inventory(cache_root: &Path, workspace: &Path) -> Result<Inventory> {
    inventory_at(&history_dir(), cache_root, workspace)
}
fn inventory_at(root: &Path, cache_root: &Path, workspace: &Path) -> Result<Inventory> {
    let _lock = history_lock(root)?;
    let mut history = read_history(root)?;
    let mut inventory = Inventory::default();
    if let Err(e) = import_jobs(&mut history, workspace) {
        inventory
            .warnings
            .push(format!("Could not import older builds: {e:#}"));
    }
    save_history(root, &history)?;
    drop(_lock);
    match cache_inventory(cache_root) {
        Ok(cache) => inventory.cache = cache,
        Err(e) => inventory
            .warnings
            .push(format!("Cache could not be scanned: {e:#}")),
    }
    inventory.builds = history
        .records
        .into_iter()
        .map(inspect_build)
        .filter(BuildView::visible)
        .collect();
    inventory
        .builds
        .sort_by_key(|v| std::cmp::Reverse(v.record.created));
    Ok(inventory)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CleanupRequest {
    Cache {
        root: PathBuf,
        protected: Vec<PathBuf>,
    },
    Workspaces {
        ids: Vec<String>,
        #[serde(default)]
        protected: Vec<PathBuf>,
    },
    Builds {
        ids: Vec<String>,
        #[serde(default)]
        protected: Vec<PathBuf>,
    },
    Isos {
        ids: Vec<String>,
        #[serde(default)]
        protected: Vec<PathBuf>,
    },
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CleanupReport {
    pub deleted: Vec<String>,
    pub errors: Vec<String>,
    pub bytes: u64,
    pub requires_admin: bool,
}
impl CleanupReport {
    pub fn message(&self) -> String {
        format!(
            "Removed {} item(s). Reclaimed {}.",
            self.deleted.len(),
            format_bytes(self.bytes)
        )
    }
}
fn remove_owned(path: &Path) -> Result<()> {
    safe_path(path)?;
    if !path.exists() {
        return Ok(());
    }
    tree_size(path)?; // Validate the entire tree before deleting any of it.
    for entry in WalkDir::new(path).follow_links(false).contents_first(true) {
        let entry = entry?;
        safe_path(entry.path())?;
        let meta = fs::symlink_metadata(entry.path())?;
        ensure!(!reparse(&meta), "Cleanup encountered a link or junction");
        if meta.is_dir() {
            fs::remove_dir(entry.path())?;
        } else {
            #[cfg(windows)]
            #[allow(clippy::permissions_set_readonly_false)]
            // Windows attribute only; preserves its ACL.
            if meta.permissions().readonly() {
                let mut p = meta.permissions();
                p.set_readonly(false);
                fs::set_permissions(entry.path(), p)?;
            }
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
fn protected(path: &Path, originals: &[PathBuf]) -> Result<()> {
    for original in originals {
        if same_path(path, original)? {
            bail!("Original source ISO is protected: {}", original.display());
        }
        let absolute = std::path::absolute(original)?;
        if absolute
            .ancestors()
            .any(|p| same_path(p, path).unwrap_or(false))
        {
            bail!(
                "Folder contains a protected source ISO: {}",
                original.display()
            );
        }
        if path.is_file()
            && original.is_file()
            && fingerprint(path)?.identity == fingerprint(original)?.identity
        {
            bail!("A source ISO shares this file identity");
        }
    }
    Ok(())
}
pub fn clear_cache(root: &Path, originals: &[PathBuf]) -> Result<CleanupReport> {
    clear_cache_at(&history_dir(), root, originals)
}
fn clear_cache_at(
    history_root: &Path,
    root: &Path,
    originals: &[PathBuf],
) -> Result<CleanupReport> {
    let _lock = activity(root, ".cache.lock", true)?;
    let _history_lock = history_lock(history_root)?;
    let history = read_history(history_root)?;
    drop(_history_lock);
    // Older workers already hold workspace locks, even if they predate cache
    // locking. Keep those workspaces idle for the entire clear operation too.
    let mut workspaces: Vec<_> = history
        .records
        .iter()
        .map(|r| r.workspace.clone())
        .filter(|p| p.join(".build.lock").is_file())
        .collect();
    workspaces.sort();
    workspaces.dedup();
    let _workspace_locks: Vec<_> = workspaces
        .iter()
        .map(|p| activity(p, ".build.lock", true))
        .collect::<Result<_>>()?;
    let originals: Vec<_> = originals
        .iter()
        .cloned()
        .chain(history.records.into_iter().map(|r| r.source))
        .collect();
    let cache = cache_inventory(root)?;
    let mut report = CleanupReport::default();
    for path in cache.paths {
        let result = (|| -> Result<u64> {
            protected(&path, &originals)?;
            let bytes = tree_size(&path)?.bytes;
            remove_owned(&path)?;
            Ok(bytes)
        })();
        match result {
            Ok(bytes) => {
                report.bytes += bytes;
                report.deleted.push(platform::display_path(&path));
            }
            Err(e) => {
                report.requires_admin |= e.chain().any(|e| {
                    e.downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
                });
                report
                    .errors
                    .push(format!("{}: {e:#}", platform::display_path(&path)));
            }
        }
    }
    Ok(report)
}
#[derive(Debug, Clone, Deserialize)]
struct MountedImage {
    path: PathBuf,
    image: PathBuf,
    index: u32,
}
fn mounted_paths() -> Result<Vec<MountedImage>> {
    let json = platform::powershell(
        "ConvertTo-Json -Compress -InputObject @((Get-WindowsImage -Mounted) | ForEach-Object { [pscustomobject]@{path=$_.Path;image=$_.ImagePath;index=$_.ImageIndex} })",
        &[],
    )?;
    if json.trim().is_empty() {
        Ok(vec![])
    } else {
        Ok(serde_json::from_str(&json)?)
    }
}
fn discard_mount(path: &Path) -> Result<()> {
    platform::powershell(
        r#"$ErrorActionPreference='Continue'; $dism="$env:SystemRoot\System32\dism.exe"
        $detail=(& $dism /English /Unmount-Image "/MountDir:$env:PATCHER_CLEAN_MOUNT" /Discard 2>&1 | Out-String)
        $code=$LASTEXITCODE
        if($code -ne 0 -and $detail -match 'c1420117') {
            Start-Sleep -Milliseconds 500
            $detail=(& $dism /English /Unmount-Image "/MountDir:$env:PATCHER_CLEAN_MOUNT" /Discard 2>&1 | Out-String)
            $code=$LASTEXITCODE
        }
        if($code -ne 0){throw "Could not discard the app-owned image mount. Its workspace was retained. $detail"}"#,
        &[("PATCHER_CLEAN_MOUNT", path.as_os_str())],
    )?;
    ensure!(
        !mounted_paths()?
            .iter()
            .any(|m| same_path(&m.path, path).unwrap_or(false)),
        "Windows still reports the image as mounted; its workspace was retained"
    );
    Ok(())
}
fn delete_one(
    record: &BuildRecord,
    originals: &[PathBuf],
    mounts: &[MountedImage],
    discard: &impl Fn(&Path) -> Result<()>,
    check_iso: &impl Fn(&Path) -> Result<()>,
    workspace_only: bool,
) -> Result<u64> {
    validate_record(record)?;
    let _lock = activity(&record.workspace, ".build.lock", true)?;
    if workspace_only {
        ensure!(
            !record.output.exists()
                || !record
                    .output
                    .canonicalize()?
                    .starts_with(record.job.canonicalize().unwrap_or(record.job.clone())),
            "Move the generated ISO outside this workspace before deleting it"
        );
    }
    if record.job.exists() {
        let req: BuildRequest = serde_json::from_slice(&fs::read(record.job.join("job.json"))?)
            .context("Build ownership record is unreadable; workspace retained")?;
        let expected = request_record(&record.job, &req)?;
        ensure!(
            same_path(&expected.source, &record.source)?
                && same_path(&expected.output, &record.output)?
                && expected.model == record.model,
            "Build ownership record changed; workspace retained"
        );
    }
    let mut paths = vec![record.job.clone()];
    paths.extend(
        record
            .partial
            .iter()
            .chain(record.connection_log.iter())
            .filter(|p| !workspace_only || p.starts_with(&record.workspace))
            .cloned(),
    );
    if !workspace_only && record.output.exists() && record.published.is_some() {
        ensure!(
            Some(fingerprint(&record.output)?) == record.published,
            "The generated ISO changed or was replaced; it has been preserved"
        );
        paths.insert(0, record.output.clone());
    }
    for path in &paths {
        protected(path, originals)?;
        if path == &record.job {
            job_size(path)?;
        } else {
            tree_size(path)?;
        }
    }
    for mount in mounts {
        if mount
            .path
            .ancestors()
            .any(|p| same_path(p, &record.job).unwrap_or(false))
        {
            ensure!(
                mount
                    .path
                    .parent()
                    .is_some_and(|p| same_path(p, &record.job).unwrap_or(false))
                    && mount
                        .path
                        .file_name()
                        .is_some_and(|v| v == "mount-install" || v == "mount-boot"),
                "An unrecognized image mount is inside this workspace; nothing was deleted"
            );
            let boot = mount.path.file_name().is_some_and(|v| v == "mount-boot");
            let expected = record.job.join(if boot {
                "iso-root/sources/boot.wim"
            } else {
                "iso-root/sources/install.wim"
            });
            ensure!(
                same_path(&mount.image, &expected)?
                    && if boot {
                        mount.index == 2
                    } else {
                        mount.index >= 1 && mount.index as usize <= record.editions.len()
                    },
                "The mounted image does not belong to this build; nothing was deleted"
            );
            // Preserve the original path spelling used to mount this job (including
            // Windows 8.3 aliases), rather than substituting the discovery path.
            discard(&record.job.join(mount.path.file_name().unwrap()))?;
        }
    }
    // Refuse output ISOs mounted in Explorer; deleting a build does not eject an unrelated session.
    if !workspace_only && record.output.exists() && record.published.is_some() {
        check_iso(&record.output)?;
    }
    let mut bytes = 0;
    for path in paths {
        if path == record.output && path.exists() {
            ensure!(
                Some(fingerprint(&path)?) == record.published,
                "Generated ISO changed during cleanup; it has been preserved"
            );
        }
        bytes += tree_size(&path)?.bytes;
        remove_owned(&path)?;
    }
    Ok(bytes)
}
pub fn delete_builds(ids: &[String], protected: &[PathBuf]) -> Result<CleanupReport> {
    ensure!(
        platform::is_admin(),
        "Build cleanup requires administrator access"
    );
    delete_builds_at_protected(
        &history_dir(),
        ids,
        protected,
        &mounted_paths()?,
        &discard_mount,
        &ensure_iso_not_mounted,
    )
}
fn ensure_iso_not_mounted(path: &Path) -> Result<()> {
    platform::powershell(
        "$i=Get-DiskImage -ImagePath $env:PATCHER_CLEAN_ISO -ErrorAction SilentlyContinue; if($i -and $i.Attached){throw 'The generated ISO is mounted. Dismount it and retry.'}",
        &[("PATCHER_CLEAN_ISO", path.as_os_str())],
    )?;
    Ok(())
}
#[cfg(test)]
fn delete_builds_at(
    root: &Path,
    ids: &[String],
    mounts: &[MountedImage],
    discard: &impl Fn(&Path) -> Result<()>,
    check_iso: &impl Fn(&Path) -> Result<()>,
) -> Result<CleanupReport> {
    delete_builds_at_protected(root, ids, &[], mounts, discard, check_iso)
}
fn delete_builds_at_protected(
    root: &Path,
    ids: &[String],
    protected: &[PathBuf],
    mounts: &[MountedImage],
    discard: &impl Fn(&Path) -> Result<()>,
    check_iso: &impl Fn(&Path) -> Result<()>,
) -> Result<CleanupReport> {
    let history = {
        let _lock = history_lock(root)?;
        read_history(root)?
    };
    let originals: Vec<_> = history
        .records
        .iter()
        .map(|r| r.source.clone())
        .chain(protected.iter().cloned())
        .collect();
    let mut report = CleanupReport::default();
    for id in ids {
        let Some(record) = history.records.iter().find(|r| &r.id == id) else {
            report
                .errors
                .push(format!("Build {id} is no longer in history"));
            continue;
        };
        match delete_one(record, &originals, mounts, discard, check_iso, false) {
            Ok(bytes) => {
                report.bytes += bytes;
                report.deleted.push(id.clone());
                let _lock = history_lock(root)?;
                let mut latest = read_history(root)?;
                latest.records.retain(|r| &r.id != id);
                save_history(root, &latest)?;
            }
            Err(e) => report
                .errors
                .push(format!("{}: {e:#}", platform::display_path(&record.output))),
        }
    }
    Ok(report)
}
fn delete_workspaces_at(
    root: &Path,
    ids: &[String],
    protected_sources: &[PathBuf],
    mounts: &[MountedImage],
    discard: &impl Fn(&Path) -> Result<()>,
) -> Result<CleanupReport> {
    let history = {
        let _lock = history_lock(root)?;
        read_history(root)?
    };
    // Protect every recorded output, including changed outputs, inside any selected job.
    let originals: Vec<_> = history
        .records
        .iter()
        .flat_map(|r| [r.source.clone(), r.output.clone()])
        .chain(protected_sources.iter().cloned())
        .collect();
    let mut report = CleanupReport::default();
    for id in ids {
        let Some(record) = history.records.iter().find(|r| &r.id == id) else {
            report
                .errors
                .push(format!("Build {id} is no longer in history"));
            continue;
        };
        let contained_output = history.records.iter().any(|r| {
            r.output.exists()
                && r.output
                    .ancestors()
                    .any(|p| same_path(p, &record.job).unwrap_or(false))
        });
        if contained_output {
            report.errors.push(format!(
                "{}: Move the generated ISO outside this workspace before deleting it",
                platform::display_path(&record.job)
            ));
            continue;
        }
        match delete_one(record, &originals, mounts, discard, &|_| Ok(()), true) {
            Ok(bytes) => {
                report.bytes += bytes;
                report.deleted.push(id.clone());
            }
            Err(e) => report
                .errors
                .push(format!("{}: {e:#}", platform::display_path(&record.job))),
        }
    }
    // Retain publication identities and sources for future output replacement.
    Ok(report)
}

fn delete_isos_at(
    root: &Path,
    ids: &[String],
    protected_sources: &[PathBuf],
    check_iso: &impl Fn(&Path) -> Result<()>,
) -> Result<CleanupReport> {
    let history = {
        let _lock = history_lock(root)?;
        read_history(root)?
    };
    let originals: Vec<_> = history
        .records
        .iter()
        .map(|r| r.source.clone())
        .chain(protected_sources.iter().cloned())
        .collect();
    let mut report = CleanupReport::default();
    for id in ids {
        let Some(record) = history.records.iter().find(|r| &r.id == id) else {
            report
                .errors
                .push(format!("Build {id} is no longer in history"));
            continue;
        };
        let result = (|| -> Result<u64> {
            validate_record(record)?;
            let _lock = activity(&record.workspace, ".build.lock", true)?;
            let published = record
                .published
                .as_ref()
                .context("No verified generated ISO is recorded; nothing was deleted")?;
            protected(&record.output, &originals)?;
            if !record.output.exists() {
                return Ok(0);
            }
            ensure!(
                fingerprint(&record.output)? == *published,
                "The generated ISO changed or was replaced; it has been preserved"
            );
            check_iso(&record.output)?;
            // Recheck after the potentially slow mount query. This operation
            // never discards WIM mounts or touches any workspace contents.
            safe_path(&record.output)?;
            protected(&record.output, &originals)?;
            ensure!(
                fingerprint(&record.output)? == *published,
                "Generated ISO changed during cleanup; it has been preserved"
            );
            fs::remove_file(&record.output)?;
            Ok(published.size)
        })();
        match result {
            Ok(bytes) => {
                report.bytes += bytes;
                report.deleted.push(platform::display_path(&record.output));
            }
            Err(e) => report
                .errors
                .push(format!("{}: {e:#}", platform::display_path(&record.output))),
        }
    }
    // Keep history, including the publication fingerprint. A later scan shows
    // workspace-only status, and any replacement ISO still fails identity checks.
    Ok(report)
}

#[derive(Debug, Clone)]
pub struct OutputCandidate {
    pub path: PathBuf,
    pub size: u64,
    pub fingerprint: Option<Fingerprint>,
    pub id: Option<String>,
    pub reason: Option<String>,
}
pub fn output_candidate(path: &Path, source: &Path) -> Result<OutputCandidate> {
    output_candidate_at(&history_dir(), path, source)
}
fn output_candidate_at(root: &Path, path: &Path, source: &Path) -> Result<OutputCandidate> {
    let size = fs::symlink_metadata(path)?.len();
    let mut candidate = OutputCandidate {
        path: path.to_owned(),
        size,
        fingerprint: None,
        id: None,
        reason: None,
    };
    let check = (|| -> Result<(String, Fingerprint)> {
        safe_path(path)?;
        let _lock = history_lock(root)?;
        let history = read_history(root)?;
        let originals: Vec<_> = history
            .records
            .iter()
            .map(|r| r.source.clone())
            .chain([source.to_owned()])
            .collect();
        protected(path, &originals)?;
        let actual = fingerprint(path)?;
        let matching: Vec<_> = history
            .records
            .iter()
            .filter(|r| same_path(path, &r.output).unwrap_or(false))
            .collect();
        ensure!(
            !matching.is_empty(),
            "This ISO is not a verified app-generated output. Choose another filename."
        );
        let record = matching
            .into_iter()
            .filter(|r| r.published.as_ref() == Some(&actual))
            .max_by_key(|r| r.created)
            .context("The ISO changed or was replaced; choose another filename")?;
        validate_record(record)?;
        let _activity = activity(&record.workspace, ".build.lock", true)?;
        ensure!(
            record.published.as_ref() == Some(&fingerprint(path)?),
            "The ISO changed or was replaced; choose another filename"
        );
        Ok((record.id.clone(), actual))
    })();
    match check {
        Ok((id, actual)) => {
            candidate.id = Some(id);
            candidate.size = actual.size;
            candidate.fingerprint = Some(actual);
        }
        Err(e) => candidate.reason = Some(format!("{e:#}")),
    }
    Ok(candidate)
}

pub fn execute(request: CleanupRequest) -> Result<CleanupReport> {
    match request {
        CleanupRequest::Cache { root, protected } => clear_cache(&root, &protected),
        CleanupRequest::Workspaces { ids, protected } => {
            ensure!(
                platform::is_admin(),
                "Workspace cleanup requires administrator access"
            );
            delete_workspaces_at(
                &history_dir(),
                &ids,
                &protected,
                &mounted_paths()?,
                &discard_mount,
            )
        }
        CleanupRequest::Builds { ids, protected } => delete_builds(&ids, &protected),
        CleanupRequest::Isos { ids, protected } => {
            ensure!(
                platform::is_admin(),
                "ISO cleanup requires administrator access"
            );
            delete_isos_at(&history_dir(), &ids, &protected, &ensure_iso_not_mounted)
        }
    }
}
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} bytes")
    }
}
pub fn format_date(timestamp: u64) -> String {
    let z = timestamp as i64 / 86400 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} (UTC)")
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        keep: bool,
    }
    impl Fixture {
        fn new() -> Self {
            let root = PathBuf::from(platform::display_path(
                &std::env::temp_dir().canonicalize().unwrap(),
            ))
            .join(format!("macbook-cleanup-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            Self { root, keep: false }
        }
        fn path(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }
        fn file(&self, name: &str, data: &[u8]) -> PathBuf {
            let path = self.path(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, data).unwrap();
            path
        }
        fn record(&self) -> BuildRecord {
            let workspace = self.path("builds");
            fs::create_dir_all(&workspace).unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            let job = workspace.join(format!("build-{id}"));
            fs::create_dir(&job).unwrap();
            let source = self.file("original.iso", b"original source");
            let output = self.file(&format!("{id}.iso"), b"generated output");
            let req = BuildRequest {
                inventory: crate::domain::Inventory {
                    iso: source,
                    iso_size: 15,
                    iso_modified: 0,
                    install_file: "install.wim".into(),
                    setup_image: None,
                    images: vec![Image {
                        index: 1,
                        name: "Windows 10 Pro".into(),
                        edition: "Professional".into(),
                        arch: 9,
                        build: 19041,
                        revision: None,
                        major: 10,
                        minor: 0,
                        language: "en-US".into(),
                        installation_type: "Client".into(),
                        size: GIB,
                    }],
                },
                selected: vec![1],
                model: MODEL_ID.into(),
                display_target: crate::domain::DisplayTarget::External,
                output,
                workspace,
                cache: self.path("cache"),
                seven_zip: PathBuf::new(),
                oscdimg: PathBuf::new(),
            };
            fs::write(job.join("job.json"), serde_json::to_vec(&req).unwrap()).unwrap();
            fs::write(job.join("build.log"), "Completed. Build log: test").unwrap();
            let mut r = request_record(&job, &req).unwrap();
            r.state = BuildState::Complete;
            r.published = Some(fingerprint(&r.output).unwrap());
            r
        }
        fn history(&self, records: Vec<BuildRecord>) -> PathBuf {
            let root = self.path("config");
            fs::create_dir_all(&root).unwrap();
            save_history(&root, &History { records }).unwrap();
            root
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.keep {
                return;
            }
            if let Ok(root) = self.root.canonicalize()
                && root.parent() == std::env::temp_dir().canonicalize().ok().as_deref()
                && root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix("macbook-cleanup-test-"))
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
            {
                let _ = remove_owned(&root);
            }
        }
    }
    #[test]
    fn workspace_cleanup_retains_outputs_and_history_even_when_output_changed() {
        for changed in [false, true] {
            let f = Fixture::new();
            let r = f.record();
            let config = f.history(vec![r.clone()]);
            if changed {
                fs::write(&r.output, b"changed external output").unwrap();
            }
            let view = inspect_build(r.clone());
            assert!(view.can_delete_workspace());
            let expected = view.workspace_bytes();
            let report =
                delete_workspaces_at(&config, std::slice::from_ref(&r.id), &[], &[], &|_| Ok(()))
                    .unwrap();
            assert!(report.errors.is_empty(), "{:?}", report.errors);
            assert_eq!(report.bytes, expected);
            assert!(!r.job.exists());
            assert!(r.output.exists());
            assert!(r.source.exists());
            assert_eq!(read_history(&config).unwrap().records.len(), 1);
            assert!(!inspect_build(r).workspace_visible());
        }
    }
    #[test]
    fn workspace_cleanup_preserves_external_partial_files_and_blocks_nested_iso() {
        let f = Fixture::new();
        let mut r = f.record();
        let partial = r
            .output
            .parent()
            .unwrap()
            .join(format!(".a1706-{}.partial.iso", uuid::Uuid::new_v4()));
        fs::write(&partial, b"partial").unwrap();
        r.partial = Some(partial.clone());
        let config = f.history(vec![r.clone()]);
        let view = inspect_build(r.clone());
        assert!(!view.workspace_artifacts().any(|a| a.path == partial));
        let report =
            delete_workspaces_at(&config, std::slice::from_ref(&r.id), &[], &[], &|_| Ok(()))
                .unwrap();
        assert!(report.errors.is_empty());
        assert!(partial.exists());
        let mut r = f.record();
        let nested = r.job.join("finished.iso");
        fs::copy(&r.output, &nested).unwrap();
        r.output = nested.clone();
        r.published = Some(fingerprint(&nested).unwrap());
        let config = f.history(vec![r.clone()]);
        assert!(!inspect_build(r.clone()).can_delete_workspace());
        let report =
            delete_workspaces_at(&config, std::slice::from_ref(&r.id), &[], &[], &|_| Ok(()))
                .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(nested.exists());
        assert!(r.job.exists());
    }
    #[test]
    fn another_builds_output_inside_a_workspace_is_excluded_and_protected() {
        let f = Fixture::new();
        let a = f.record();
        let mut b = f.record();
        b.output = a.job.join("another-output.iso");
        fs::write(&b.output, b"another generated output").unwrap();
        b.published = Some(fingerprint(&b.output).unwrap());
        let config = f.history(vec![a.clone(), b.clone()]);
        let inventory = inventory_at(&config, &f.path("cache"), &a.workspace).unwrap();
        let view = inventory
            .builds
            .iter()
            .find(|v| v.record.id == a.id)
            .unwrap();
        assert!(
            inventory
                .workspace_problem(view)
                .unwrap()
                .contains("Move the generated ISO")
        );
        assert_eq!(
            inventory.workspace_bytes(view) + fs::metadata(&b.output).unwrap().len(),
            view.workspace.size.bytes
        );
        let expected: u64 = inventory
            .builds
            .iter()
            .map(|v| inventory.workspace_bytes(v))
            .sum();
        assert_eq!(inventory.total_bytes(), expected);
        let report =
            delete_workspaces_at(&config, std::slice::from_ref(&a.id), &[], &[], &|_| Ok(()))
                .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].contains("Move the generated ISO"));
        assert!(a.job.exists());
        assert!(b.output.exists());
    }
    #[test]
    fn workspace_cleanup_respects_active_lock_and_failed_unmount() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let lock = activity(&r.workspace, ".build.lock", true).unwrap();
        assert_eq!(
            delete_workspaces_at(&config, std::slice::from_ref(&r.id), &[], &[], &|_| Ok(()))
                .unwrap()
                .errors
                .len(),
            1
        );
        drop(lock);
        let mount = r.job.join("mount-install");
        fs::create_dir_all(&mount).unwrap();
        let mounts = [MountedImage {
            path: mount,
            image: r.job.join("iso-root/sources/install.wim"),
            index: 1,
        }];
        assert_eq!(
            delete_workspaces_at(
                &config,
                std::slice::from_ref(&r.id),
                &[],
                &mounts,
                &|_| bail!("unmount failed")
            )
            .unwrap()
            .errors
            .len(),
            1
        );
        assert!(r.job.exists());
        assert!(r.output.exists());
    }
    #[test]
    fn output_choices_only_offer_verified_unchanged_outputs() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        assert_eq!(
            output_candidate_at(&config, &r.output, &r.source)
                .unwrap()
                .id,
            Some(r.id.clone())
        );
        assert!(
            output_candidate_at(&config, &r.output, &r.output)
                .unwrap()
                .id
                .is_none()
        );
        let unknown = f.file("unknown.iso", b"unknown");
        assert!(
            output_candidate_at(&config, &unknown, &r.source)
                .unwrap()
                .id
                .is_none()
        );
        fs::write(&r.output, b"changed").unwrap();
        assert!(
            output_candidate_at(&config, &r.output, &r.source)
                .unwrap()
                .id
                .is_none()
        );
        fs::remove_file(&r.output).unwrap();
        assert!(output_candidate_at(&config, &r.output, &r.source).is_err());
    }
    #[test]
    fn reused_output_names_match_the_current_publication_not_an_older_record() {
        let f = Fixture::new();
        let old = f.record();
        let mut current = f.record();
        fs::copy(&current.output, &old.output).unwrap();
        // Content may match, but the publication timestamp/identity must match the current record.
        current.output = old.output.clone();
        current.published = Some(fingerprint(&current.output).unwrap());
        let config = f.history(vec![old.clone(), current.clone()]);
        let candidate = output_candidate_at(&config, &current.output, &current.source).unwrap();
        assert_eq!(candidate.id, Some(current.id));
        assert!(old.job.exists());
    }
    #[test]
    fn output_choices_refuse_hardlinks() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        fs::hard_link(&r.output, f.path("alias.iso")).unwrap();
        assert!(
            output_candidate_at(&config, &r.output, &r.source)
                .unwrap()
                .id
                .is_none()
        );
        let report =
            delete_isos_at(&config, std::slice::from_ref(&r.id), &[], &|_| Ok(())).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(r.output.exists());
    }
    #[test]
    fn cache_counts_and_clears_only_known_artifacts() {
        let f = Fixture::new();
        let cache = f.path("cache");
        f.file("cache/win64_15.45.5174.exe", b"12345");
        f.file(
            &format!("cache/win64_15.45.5174.exe.{}.part", uuid::Uuid::new_v4()),
            b"123",
        );
        f.file(
            &format!("cache/extract-{}/payload.inf", uuid::Uuid::new_v4()),
            b"1234567",
        );
        f.file(
            &format!("cache/adk-setup/{}/adksetup.exe", uuid::Uuid::new_v4()),
            b"12",
        );
        let unknown = f.file("cache/notes.txt", b"keep");
        let iso = f.file("cache/original.iso", b"keep ISO");
        let inventory = cache_inventory(&cache).unwrap();
        assert_eq!(inventory.size.files, 4);
        assert_eq!(inventory.size.bytes, 17);
        let report = clear_cache_at(&f.path("config"), &cache, &[]).unwrap();
        assert!(report.errors.is_empty());
        assert_eq!(report.bytes, 17);
        assert!(unknown.exists());
        assert!(iso.exists());
        assert!(cache.exists());
        assert_eq!(cache_inventory(&cache).unwrap().size.files, 0);
    }
    #[test]
    fn cache_lock_prevents_deletion_during_download() {
        let f = Fixture::new();
        let file = f.file("cache/win64_15.45.5174.exe", b"123");
        let _active = cache_activity(&f.path("cache")).unwrap();
        assert!(clear_cache_at(&f.path("config"), &f.path("cache"), &[]).is_err());
        assert!(file.exists());
    }
    #[test]
    fn cache_keeps_a_protected_iso_inside_an_extraction() {
        let f = Fixture::new();
        let iso = f.file(
            &format!("cache/extract-{}/original.iso", uuid::Uuid::new_v4()),
            b"original",
        );
        let report = clear_cache_at(
            &f.path("config"),
            &f.path("cache"),
            std::slice::from_ref(&iso),
        )
        .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(iso.exists());
    }
    #[test]
    fn legacy_jobs_are_imported_once_and_survive_missing_outputs() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![]);
        let first = inventory_at(&config, &f.path("cache"), &r.workspace).unwrap();
        assert_eq!(first.builds.len(), 1);
        assert_eq!(first.builds[0].record.state, BuildState::Complete);
        assert!(first.builds[0].record.published.is_some());
        fs::remove_file(&r.output).unwrap();
        let next = inventory_at(&config, &f.path("cache"), &r.workspace).unwrap();
        assert_eq!(next.builds.len(), 1);
        assert!(next.builds[0].size.bytes > 0);
    }
    #[test]
    fn deleting_a_build_removes_output_workspace_and_history_but_keeps_source() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let report = delete_builds_at(
            &config,
            std::slice::from_ref(&r.id),
            &[],
            &|_| Ok(()),
            &|_| Ok(()),
        )
        .unwrap();
        assert!(report.errors.is_empty());
        assert!(!r.output.exists());
        assert!(!r.job.exists());
        assert_eq!(fs::read(&r.source).unwrap(), b"original source");
        assert!(read_history(&config).unwrap().records.is_empty());
    }
    #[test]
    fn replaced_iso_is_preserved_with_its_workspace() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        fs::remove_file(&r.output).unwrap();
        fs::write(&r.output, b"replacement").unwrap();
        let report = delete_builds_at(
            &config,
            std::slice::from_ref(&r.id),
            &[],
            &|_| Ok(()),
            &|_| Ok(()),
        )
        .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert_eq!(fs::read(&r.output).unwrap(), b"replacement");
        assert!(r.job.exists());
        assert_eq!(read_history(&config).unwrap().records.len(), 1);
    }
    #[test]
    fn current_source_inside_a_workspace_is_preserved_before_any_deletion() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let current_source = r.job.join("new-original.iso");
        fs::write(&current_source, b"new source").unwrap();
        let report = delete_builds_at_protected(
            &config,
            std::slice::from_ref(&r.id),
            std::slice::from_ref(&current_source),
            &[],
            &|_| Ok(()),
            &|_| Ok(()),
        )
        .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(r.output.exists());
        assert!(r.job.exists());
        assert_eq!(fs::read(current_source).unwrap(), b"new source");
    }
    #[test]
    fn active_build_lock_keeps_all_artifacts() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let _active = activity(&r.workspace, ".build.lock", true).unwrap();
        let report = delete_builds_at(
            &config,
            std::slice::from_ref(&r.id),
            &[],
            &|_| Ok(()),
            &|_| Ok(()),
        )
        .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(r.output.exists());
        assert!(r.job.exists());
    }
    #[test]
    fn failed_unmount_preserves_that_build_and_reports_partial_success() {
        let f = Fixture::new();
        let a = f.record();
        let b = f.record();
        let mount = b.job.join("mount-install");
        fs::create_dir(&mount).unwrap();
        let config = f.history(vec![a.clone(), b.clone()]);
        let report = delete_builds_at(
            &config,
            &[a.id.clone(), b.id.clone()],
            &[MountedImage {
                path: mount,
                image: b.job.join("iso-root/sources/install.wim"),
                index: 1,
            }],
            &|_| bail!("test unmount failure"),
            &|_| Ok(()),
        )
        .unwrap();
        assert_eq!(report.deleted.len(), 1);
        assert_eq!(report.errors.len(), 1);
        assert!(!a.output.exists());
        assert!(b.output.exists());
        assert!(b.job.exists());
        assert_eq!(read_history(&config).unwrap().records.len(), 1);
    }
    #[test]
    fn mounted_iso_is_preserved() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let report = delete_builds_at(
            &config,
            std::slice::from_ref(&r.id),
            &[],
            &|_| Ok(()),
            &|_| bail!("ISO is mounted"),
        )
        .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(r.output.exists());
        assert!(r.job.exists());
    }
    #[test]
    fn unrelated_wim_inside_a_build_is_not_discarded() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let mount = r.job.join("mount-install");
        fs::create_dir(&mount).unwrap();
        let called = std::cell::Cell::new(false);
        let report = delete_builds_at(
            &config,
            std::slice::from_ref(&r.id),
            &[MountedImage {
                path: mount,
                image: f.path("someone-elses.wim"),
                index: 1,
            }],
            &|_| {
                called.set(true);
                Ok(())
            },
            &|_| Ok(()),
        )
        .unwrap();
        assert!(!called.get());
        assert_eq!(report.errors.len(), 1);
        assert!(r.output.exists());
        assert!(r.job.exists());
    }
    #[test]
    fn original_iso_and_path_escape_are_rejected() {
        let f = Fixture::new();
        let mut r = f.record();
        r.output = r.source.clone();
        assert!(validate_record(&r).is_err());
        r = f.record();
        r.job = r.workspace.join("..").join(format!("build-{}", r.id));
        assert!(validate_record(&r).is_err());
        assert!(r.source.exists());
    }
    #[test]
    fn missing_files_are_idempotent_and_unverified_outputs_are_kept() {
        let f = Fixture::new();
        let mut r = f.record();
        r.published = None;
        r.state = BuildState::Unknown;
        let config = f.history(vec![r.clone()]);
        let report = delete_builds_at(
            &config,
            std::slice::from_ref(&r.id),
            &[],
            &|_| Ok(()),
            &|_| Ok(()),
        )
        .unwrap();
        assert!(report.errors.is_empty());
        assert!(r.output.exists());
        assert!(!r.job.exists());
    }
    #[cfg(windows)]
    #[test]
    fn junctions_are_not_followed_or_deleted() {
        let f = Fixture::new();
        let target = f.file("outside/keep.txt", b"keep");
        let extraction = f.path(&format!("cache/extract-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&extraction).unwrap();
        let link = extraction.join("linked");
        platform::powershell("$null=New-Item -ItemType Junction -Path $env:CLEAN_TEST_LINK -Target $env:CLEAN_TEST_TARGET",&[("CLEAN_TEST_LINK",link.as_os_str()),("CLEAN_TEST_TARGET",target.parent().unwrap().as_os_str())]).unwrap();
        assert!(cache_inventory(&f.path("cache")).is_err());
        assert!(target.exists());
        fs::remove_dir(link).unwrap();
    }
    #[test]
    fn iso_only_deletion_keeps_workspace_history_and_source_for_later_cleanup() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let job_json = fs::read(r.job.join("job.json")).unwrap();
        let report =
            delete_isos_at(&config, std::slice::from_ref(&r.id), &[], &|_| Ok(())).unwrap();
        assert!(report.errors.is_empty());
        assert_eq!(report.bytes, r.published.as_ref().unwrap().size);
        assert!(!r.output.exists());
        assert_eq!(fs::read(r.job.join("job.json")).unwrap(), job_json);
        assert!(r.job.join("build.log").exists());
        assert!(r.source.exists());
        assert_eq!(
            read_history(&config).unwrap().records[0].published,
            r.published
        );
        let again = delete_isos_at(&config, std::slice::from_ref(&r.id), &[], &|_| {
            bail!("Absent ISO must not query mounts")
        })
        .unwrap();
        assert!(again.errors.is_empty());
        assert_eq!(again.bytes, 0);
        let full = delete_builds_at(&config, &[r.id], &[], &|_| Ok(()), &|_| Ok(())).unwrap();
        assert!(full.errors.is_empty());
        assert!(!r.job.exists());
        assert!(read_history(&config).unwrap().records.is_empty());
    }
    #[test]
    fn iso_only_deletion_refuses_replacements_and_retains_history() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        fs::write(&r.output, b"replacement ISO").unwrap();
        let report = delete_isos_at(&config, &[r.id], &[], &|_| Ok(())).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert_eq!(fs::read(&r.output).unwrap(), b"replacement ISO");
        assert!(r.job.exists());
        assert_eq!(read_history(&config).unwrap().records.len(), 1);
    }
    #[test]
    fn iso_only_deletion_rechecks_after_mount_query() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let report = delete_isos_at(&config, &[r.id], &[], &|path| {
            fs::write(path, b"replaced during query")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert_eq!(fs::read(&r.output).unwrap(), b"replaced during query");
        assert!(r.job.exists());
    }
    #[test]
    fn iso_only_deletion_respects_active_lock() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let _lock = activity(&r.workspace, ".build.lock", true).unwrap();
        let report = delete_isos_at(&config, &[r.id], &[], &|_| Ok(())).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(r.output.exists());
        assert!(r.job.exists());
    }
    #[test]
    fn iso_only_deletion_refuses_mounted_isos_and_current_sources() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let report = delete_isos_at(&config, std::slice::from_ref(&r.id), &[], &|_| {
            bail!("ISO is mounted")
        })
        .unwrap();
        assert_eq!(report.errors.len(), 1);
        let report = delete_isos_at(&config, &[r.id], std::slice::from_ref(&r.output), &|_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(r.output.exists());
        assert!(r.job.exists());
    }
    #[test]
    fn iso_only_deletion_refuses_unverified_outputs() {
        let f = Fixture::new();
        let mut r = f.record();
        r.published = None;
        let config = f.history(vec![r.clone()]);
        let report = delete_isos_at(&config, &[r.id], &[], &|_| Ok(())).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(r.output.exists());
        assert!(r.job.exists());
    }
    #[test]
    fn inventory_tracks_existing_artifacts_and_keeps_empty_history() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        let scan = || inventory_at(&config, &f.path("cache"), &r.workspace).unwrap();
        let both = scan();
        let view = &both.builds[0];
        assert!(
            view.workspace.available()
                && view.output.available()
                && view.can_delete_iso()
                && view.complete()
        );
        assert_eq!(both.workspace_count(), 1);
        let mut duplicated = both.clone();
        duplicated.builds.push(view.clone());
        assert_eq!(duplicated.workspace_count(), 1);
        assert_eq!(duplicated.total_bytes(), both.total_bytes());
        assert_eq!(
            view.title(),
            r.output.file_name().unwrap().to_string_lossy()
        );
        fs::remove_file(&r.output).unwrap();
        let workspace_only = scan();
        let view = &workspace_only.builds[0];
        assert!(view.workspace.available() && !view.output.available());
        assert!(!view.can_delete_iso() && view.can_delete() && !view.complete());
        assert!(view.status().contains("workspace only"));
        assert_eq!(view.title(), r.job.file_name().unwrap().to_string_lossy());
        assert_eq!(view.deletable_artifacts(false).len(), 1);
        remove_owned(&r.job).unwrap();
        assert!(scan().builds.is_empty());
        assert_eq!(read_history(&config).unwrap().records.len(), 1);
    }
    #[test]
    fn inventory_iso_only_has_no_workspace_count_or_dead_paths() {
        let f = Fixture::new();
        let r = f.record();
        let config = f.history(vec![r.clone()]);
        remove_owned(&r.job).unwrap();
        let inventory = inventory_at(&config, &f.path("cache"), &r.workspace).unwrap();
        let view = &inventory.builds[0];
        assert_eq!(inventory.workspace_count(), 0);
        assert_eq!(inventory.total_bytes(), 0);
        assert!(!view.workspace_visible());
        assert!(!view.workspace.available() && view.output.available() && view.can_delete_iso());
        assert!(view.status().contains("ISO only"));
        assert_eq!(
            view.deletable_artifacts(false)
                .iter()
                .map(|a| a.path.clone())
                .collect::<Vec<_>>(),
            vec![r.output]
        );
    }
    #[test]
    fn inventory_skips_missing_partial_and_logs_but_counts_remaining_logs() {
        let f = Fixture::new();
        let mut r = f.record();
        r.partial = Some(
            r.output
                .parent()
                .unwrap()
                .join(format!(".a1706-{}.partial.iso", uuid::Uuid::new_v4())),
        );
        r.connection_log = Some(
            r.workspace
                .join(format!("worker-connection-{}.log", uuid::Uuid::new_v4())),
        );
        let config = f.history(vec![r.clone()]);
        let view = inspect_build(r.clone());
        assert!(view.problem.is_none(), "{:?}", view.problem);
        assert!(!view.partial.as_ref().unwrap().available());
        assert!(!view.connection_log.as_ref().unwrap().available());
        assert_eq!(view.deletable_artifacts(false).len(), 2);
        assert_eq!(read_history(&config).unwrap().records.len(), 1);
        fs::write(r.connection_log.as_ref().unwrap(), b"connection diagnostic").unwrap();
        remove_owned(&r.job).unwrap();
        fs::remove_file(&r.output).unwrap();
        let view = inspect_build(r);
        assert!(view.visible() && view.can_delete() && !view.can_delete_iso());
        assert_eq!(view.size.bytes, 21);
        assert_eq!(view.deletable_artifacts(false).len(), 1);
    }
    #[test]
    fn inventory_changed_output_is_visible_but_not_deletable_or_complete() {
        let f = Fixture::new();
        let r = f.record();
        fs::write(&r.output, b"replacement output").unwrap();
        let view = inspect_build(r);
        assert!(view.output.available());
        assert!(
            view.problem
                .as_ref()
                .unwrap()
                .contains("changed or was replaced")
        );
        assert!(!view.can_delete_iso() && !view.can_delete() && !view.complete());
        assert_eq!(view.status(), "Needs attention");
    }
    #[test]
    fn artifact_scan_distinguishes_access_errors_from_missing_files() {
        let f = Fixture::new();
        let path = f.path("unreadable");
        let missing = scan_artifact(&path, true);
        assert!(!missing.present && missing.problem.is_none());
        let denied = scan_artifact_with(&path, true, &|_| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert!(!denied.available() && denied.problem.is_some());
        let mut view = inspect_build(f.record());
        view.workspace = denied;
        view.problem = view.workspace.problem.clone();
        assert!(view.visible() && !view.can_delete());
    }
    #[test]
    fn inventory_scans_other_artifacts_even_when_one_has_wrong_type() {
        let f = Fixture::new();
        let r = f.record();
        fs::remove_file(&r.output).unwrap();
        fs::create_dir(&r.output).unwrap();
        let view = inspect_build(r);
        assert!(view.workspace.available());
        assert!(
            view.output
                .problem
                .as_ref()
                .unwrap()
                .contains("wrong file type")
        );
        assert!(!view.output.available());
        assert!(view.size.bytes > 0);
    }
    #[test]
    fn display_helpers_are_stable() {
        assert_eq!(format_date(0), "1970-01-01 (UTC)");
        assert_eq!(format_date(1609459200), "2021-01-01 (UTC)");
        assert_eq!(format_bytes(1_000_000_000), "1.00 GB");
        assert_eq!(format_bytes(1_000_000), "1.0 MB");
        assert_eq!(format_bytes(1_000), "1.0 KB");
    }

    #[test]
    fn relocated_build_can_be_inspected_and_deleted_with_ownership_checks() {
        use crate::relocation::{self, Gate, Kind, Request};
        let f = Fixture::new();
        let mut record = f.record();
        let new_output = record.workspace.join("generated.iso");
        fs::rename(&record.output, &new_output).unwrap();
        record.output = new_output;
        record.published = Some(fingerprint(&record.output).unwrap());
        let mut req: BuildRequest =
            serde_json::from_slice(&fs::read(record.job.join("job.json")).unwrap()).unwrap();
        req.output = record.output.clone();
        write_json(&record.job.join("job.json"), &req).unwrap();
        write_json(&record.job.join("build-state.json"), &record).unwrap();
        let history = f.history(vec![record.clone()]);
        let settings = serde_json::json!({"cache":f.path("cache"),"work":record.workspace});
        let settings_path = f.path("local/settings.json");
        relocation::save_settings(&settings_path, &settings).unwrap();
        let destination = f.path("moved-builds");
        relocation::execute(
            Request {
                kind: Kind::Workspace,
                source: record.workspace,
                destination: destination.clone(),
                other_root: f.path("cache"),
                protected_sources: vec![record.source.clone()],
                settings_path,
                settings,
                history_root: history.clone(),
                gate: Gate {
                    model_selection: true,
                    before_build: true,
                    idle: true,
                },
            },
            &|_| {},
        )
        .unwrap();
        let inventory = inventory_at(&history, &f.path("cache"), &destination).unwrap();
        assert_eq!(inventory.builds.len(), 1);
        assert!(inventory.builds[0].complete());
        assert!(inventory.builds[0].can_delete());
        let report =
            delete_builds_at(&history, &[record.id], &[], &|_| Ok(()), &|_| Ok(())).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(record.source.exists());
        assert!(!destination.join("generated.iso").exists());
        assert!(read_history(&history).unwrap().records.is_empty());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Requires administrator access and Microsoft ADK; uses only disposable generated media"]
    fn windows_generated_iso_and_owned_wim_cleanup() {
        assert!(
            platform::is_admin(),
            "Run this opt-in test as administrator"
        );
        let mut f = Fixture::new();
        let mut r = f.record();
        let capture = f.path("capture");
        fs::create_dir(&capture).unwrap();
        fs::write(capture.join("test.txt"), b"cleanup test").unwrap();
        let emit: Emit = std::sync::Arc::new(|_| {});
        let cancel = Cancellation::default();
        let wim = r.job.join("iso-root/sources/install.wim");
        fs::create_dir_all(wim.parent().unwrap()).unwrap();
        let mount = r.job.join("mount-install");
        fs::create_dir(&mount).unwrap();
        platform::run(
            &platform::find_oscdimg(),
            &[
                "-n".into(),
                capture.display().to_string(),
                f.path("generated.iso").display().to_string(),
            ],
            &emit,
            &cancel,
            false,
        )
        .unwrap();
        fs::copy(f.path("generated.iso"), &r.output).unwrap();
        r.published = Some(fingerprint(&r.output).unwrap());
        let config = f.history(vec![r.clone()]);
        platform::run(
            &platform::system_binary("dism.exe"),
            &[
                "/English".into(),
                "/Capture-Image".into(),
                format!("/ImageFile:{}", wim.display()),
                format!("/CaptureDir:{}", capture.display()),
                "/Name:CleanupTest".into(),
            ],
            &emit,
            &cancel,
            false,
        )
        .unwrap();
        f.keep = true; // If any assertion fails with a live mount, retain the fixture for recovery.
        platform::run(
            &platform::system_binary("dism.exe"),
            &[
                "/English".into(),
                "/Mount-Image".into(),
                format!("/ImageFile:{}", wim.display()),
                "/Index:1".into(),
                format!("/MountDir:{}", mount.display()),
            ],
            &emit,
            &cancel,
            false,
        )
        .unwrap();
        let report = delete_builds_at(
            &config,
            std::slice::from_ref(&r.id),
            &mounted_paths().unwrap(),
            &discard_mount,
            &ensure_iso_not_mounted,
        )
        .unwrap();
        assert!(
            report.errors.is_empty(),
            "{:?}; fixture retained: {}",
            report.errors,
            f.root.display()
        );
        assert!(!r.output.exists());
        assert!(!r.job.exists());
        assert_eq!(fs::read(&r.source).unwrap(), b"original source");
        assert!(
            !mounted_paths()
                .unwrap()
                .iter()
                .any(|p| same_path(&p.path, &mount).unwrap_or(false))
        );
        f.keep = false;
    }
}
