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
fn history_lock(root: &Path) -> Result<ActivityLock> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match activity(root, ".history.lock", true) {
            Ok(lock) => return Ok(lock),
            Err(e)
                if e.chain().any(|e| {
                    e.downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
                }) && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            Err(e) => return Err(e),
        }
    }
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
        format!("{}:{}:{}", info.volume, info.index_high, info.index_low)
    };
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
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
#[derive(Default, Serialize, Deserialize)]
struct History {
    records: Vec<BuildRecord>,
}
pub fn history_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "Zenodeon", "MacBookISOPatcher")
        .expect("User profile required")
        .config_dir()
        .to_owned()
}
fn read_history(root: &Path) -> Result<History> {
    let path = root.join("build-history.json");
    safe_path(&path)?;
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .context("Build history could not be read; it has been preserved"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(History::default()),
        Err(e) => Err(e.into()),
    }
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    safe_path(path)?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temp, serde_json::to_vec_pretty(value)?)?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(temp);
        return Err(error.into());
    }
    Ok(())
}
fn save_history(root: &Path, history: &History) -> Result<()> {
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
fn same_path(a: &Path, b: &Path) -> Result<bool> {
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
fn validate_record(r: &BuildRecord) -> Result<()> {
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
pub struct BuildView {
    pub output_present: bool,
    pub record: BuildRecord,
    pub size: StorageSize,
    pub problem: Option<String>,
}
#[derive(Debug, Default, Clone)]
pub struct Inventory {
    pub cache: CacheInventory,
    pub builds: Vec<BuildView>,
    pub warnings: Vec<String>,
}

fn import_jobs(history: &mut History, workspace: &Path) -> Result<()> {
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
            || history.records.iter().any(|r| r.job == path)
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
                && saved.job == record.job
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
    for record in history.records {
        let mut size = StorageSize::default();
        let measured = (|| -> Result<()> {
            validate_record(&record)?;
            for path in [&record.job]
                .into_iter()
                .chain(record.partial.iter())
                .chain(record.connection_log.iter())
                .chain(record.published.as_ref().map(|_| &record.output))
            {
                let s = if path == &record.job {
                    job_size(path)?
                } else {
                    tree_size(path)?
                };
                size.files += s.files;
                size.bytes += s.bytes;
            }
            Ok(())
        })();
        inventory.builds.push(BuildView {
            output_present: record.output.is_file(),
            record,
            size,
            problem: measured.err().map(|e| format!("{e:#}")),
        });
    }
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
    Builds {
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
) -> Result<u64> {
    validate_record(record)?;
    let _lock = activity(&record.workspace, ".build.lock", true)?;
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
    paths.extend(record.partial.clone());
    paths.extend(record.connection_log.clone());
    if record.output.exists() && record.published.is_some() {
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
    if record.output.exists() && record.published.is_some() {
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
        match delete_one(record, &originals, mounts, discard, check_iso) {
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
pub fn execute(request: CleanupRequest) -> Result<CleanupReport> {
    match request {
        CleanupRequest::Cache { root, protected } => clear_cache(&root, &protected),
        CleanupRequest::Builds { ids, protected } => delete_builds(&ids, &protected),
    }
}
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024 * 1024) as f64)
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
                    images: vec![Image {
                        index: 1,
                        name: "Windows 10 Pro".into(),
                        edition: "Professional".into(),
                        arch: 9,
                        build: 19041,
                        major: 10,
                        minor: 0,
                        language: "en-US".into(),
                        installation_type: "Client".into(),
                        size: GIB,
                    }],
                },
                selected: vec![1],
                model: MODEL_ID.into(),
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
    fn display_helpers_are_stable() {
        assert_eq!(format_date(0), "1970-01-01 (UTC)");
        assert_eq!(format_date(1609459200), "2021-01-01 (UTC)");
        assert_eq!(format_bytes(GIB), "1.00 GiB");
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
