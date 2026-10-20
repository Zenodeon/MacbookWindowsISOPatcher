use crate::{
    domain::{Cancellation, Emit, Event, stage},
    platform,
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const INTEL_URL: &str = "https://downloadmirror.intel.com/30195/a08/win64_15.45.5174.exe";
pub const INTEL_SHA256: &str = "aab51027dac8210bca98b6679472b2a00ab15a5d4f8a6b777259200de52e48e2";
pub const APPLE_URL: &str = "https://swcdn.apple.com/content/downloads/28/47/041-88734-A_Q0SKN07BL0/7knxzec1xmojr5x5t7xnu5a4u6lslmlf9v/BootCampESD.pkg";
// Pinned from the official Apple CDN. See docs/driver-sources.md.
pub const APPLE_SHA256: &str = "ace1cc6dd3bbd12e32fcf953b0f1d0ab74f4a77075fbd4733352468c5aa36f34";

pub fn sha256(path: &Path, cancel: &Cancellation) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        cancel.check()?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
pub fn verify(path: &Path, expected: &str, cancel: &Cancellation) -> Result<()> {
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "Package hash has not been pinned"
    );
    ensure!(
        sha256(path, cancel)?.eq_ignore_ascii_case(expected),
        "Integrity check failed for {}. Remove this cache entry and retry",
        path.display()
    );
    Ok(())
}
fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("MacBookISOPatcher/0.1")
        .https_only(true)
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?)
}
pub fn fetch(
    cache: &Path,
    name: &str,
    url: &str,
    hash: &str,
    emit: &Emit,
    cancel: &Cancellation,
) -> Result<PathBuf> {
    fetch_checked(cache, name, url, hash, emit, cancel)
}

fn fetch_checked(
    cache: &Path,
    name: &str,
    url: &str,
    hash: &str,
    emit: &Emit,
    cancel: &Cancellation,
) -> Result<PathBuf> {
    ensure!(
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "Package SHA-256 pin is missing or invalid"
    );
    fs::create_dir_all(cache)?;
    let path = cache.join(name);
    if path.is_file() {
        stage(emit, "Checking cached drivers", name);
        verify(&path, hash, cancel)?;
        return Ok(path);
    }
    let part = cache.join(format!("{name}.{}.part", uuid::Uuid::new_v4()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let transfer = async {
        stage(emit, "Downloading drivers", name);
        let mut response = client()?.get(url).send().await?.error_for_status()?;
        let total = response.content_length();
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&part)?;
        let mut received = 0u64;
        let mut last = Instant::now();
        loop {
            cancel.check()?;
            let Some(bytes) = response.chunk().await? else {
                break;
            };
            f.write_all(&bytes)?;
            received += bytes.len() as u64;
            if last.elapsed() > Duration::from_millis(200) {
                emit(Event::Download {
                    name: name.into(),
                    received,
                    total,
                });
                last = Instant::now();
            }
        }
        if let Some(total) = total {
            ensure!(received == total, "Incomplete download of {name}");
        }
        f.sync_all()?;
        drop(f);
        stage(emit, "Verifying drivers", name);
        verify(&part, hash, cancel)?;
        fs::rename(&part, &path)?;
        emit(Event::Download {
            name: name.into(),
            received,
            total,
        });
        Ok::<(), anyhow::Error>(())
    };
    let result = runtime.block_on(async {
        tokio::select! {
            result = transfer => result,
            _ = async { while !cancel.is_cancelled() { tokio::time::sleep(Duration::from_millis(100)).await; } } => Err(anyhow::anyhow!("Build cancelled")),
        }
    });
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result?;
    Ok(path)
}

pub fn extract(
    tool: &Path,
    archive: &Path,
    output: &Path,
    emit: &Emit,
    cancel: &Cancellation,
) -> Result<()> {
    fs::create_dir_all(output)?;
    platform::run(
        tool,
        &[
            "x".into(),
            "-y".into(),
            "-bd".into(),
            archive.display().to_string(),
            format!("-o{}", output.display()),
        ],
        emit,
        cancel,
        true,
    )
}
pub fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| {
            e.file_type().is_file() && e.file_name().to_string_lossy().eq_ignore_ascii_case(name)
        })
        .map(|e| e.into_path())
}
pub fn find_dir(root: &Path, name: &str) -> Option<PathBuf> {
    walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| {
            e.file_type().is_dir() && e.file_name().to_string_lossy().eq_ignore_ascii_case(name)
        })
        .map(|e| e.into_path())
}

pub struct Assets {
    pub intel: Option<PathBuf>,
    pub bootcamp: PathBuf,
    pub drivers: PathBuf,
}
struct ExtractionGuard {
    cache: PathBuf,
    root: PathBuf,
    keep: bool,
}
impl Drop for ExtractionGuard {
    fn drop(&mut self) {
        if !self.keep {
            let _ = remove_extraction(&self.cache, &self.root);
        }
    }
}
pub fn remove_extraction(cache: &Path, root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let cache = cache.canonicalize()?;
    let root = root.canonicalize()?;
    ensure!(
        root.parent() == Some(cache.as_path()),
        "Extraction cleanup escaped the cache"
    );
    let name = root
        .file_name()
        .context("Extraction directory name missing")?
        .to_string_lossy();
    ensure!(
        name.strip_prefix("extract-")
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok()),
        "Not an app-owned extraction directory"
    );
    fs::remove_dir_all(root)?;
    Ok(())
}
pub fn prepare(cache: &Path, tool: &Path, emit: &Emit, cancel: &Cancellation) -> Result<Assets> {
    prepare_for_model(crate::domain::MODEL_ID, cache, tool, emit, cancel)
}

pub fn prepare_for_model(
    model: &str,
    cache: &Path,
    tool: &Path,
    emit: &Emit,
    cancel: &Cancellation,
) -> Result<Assets> {
    let _activity = crate::cleanup::cache_activity(cache)?;
    ensure!(
        tool.is_file(),
        "Install 7-Zip or select 7z.exe under Build tools"
    );
    let packages = crate::driver_prefetch::packages_for_model(model)?;
    let paths = crate::driver_prefetch::download_for_model(model, cache, emit, cancel)?;
    let apple = packages
        .iter()
        .zip(&paths)
        .find(|(package, _)| package.filename.ends_with(".pkg"))
        .context("Apple package missing from model manifest")?
        .1;
    let dir = cache.join(format!("extract-{}", uuid::Uuid::new_v4()));
    let mut guard = ExtractionGuard {
        cache: cache.to_owned(),
        root: dir.clone(),
        keep: false,
    };
    stage(
        emit,
        "Extracting drivers",
        "Preparing the selected model's signed Boot Camp drivers",
    );
    let intel_dir = if model != "a1707-2016" {
        let graphics = crate::driver_prefetch::intel_graphics(model)?;
        if model == "a1706-2017" {
            extract(tool, &paths[0], &dir.join("intel"), emit, cancel)?;
        } else {
            extract_intel(tool, &paths[0], &dir.join("intel"), emit, cancel)?;
        }
        let inf = find_file(&dir.join("intel"), graphics.inf)
            .context("Intel package has no matching graphics INF")?;
        let folder = inf.parent().unwrap().to_owned();
        crate::engine::validate_intel_for_model(&folder, model)?;
        Some(folder)
    } else {
        None
    };
    extract(tool, apple, &dir.join("apple-pkg"), emit, cancel)?;
    // xar -> bzip2 Payload -> cpio -> WindowsSupport.dmg.
    let mut tree = dir.join("apple-pkg");
    for depth in 0..4 {
        if find_file(&tree, "WindowsSupport.dmg").is_some() {
            break;
        }
        let next = find_file(&tree, "Payload")
            .or_else(|| find_file(&tree, "Payload~"))
            .or_else(|| {
                walkdir::WalkDir::new(&tree)
                    .into_iter()
                    .filter_map(Result::ok)
                    .find(|e| {
                        e.file_type().is_file()
                            && e.file_name().to_string_lossy().ends_with(".cpio")
                    })
                    .map(|e| e.into_path())
            })
            .context("Cannot find Apple Payload archive")?;
        let output = dir.join(format!("apple-layer-{depth}"));
        extract(tool, &next, &output, emit, cancel)?;
        tree = output;
    }
    let dmg = find_file(&tree, "WindowsSupport.dmg")
        .context("Apple package has no WindowsSupport.dmg")?;
    let dmg_root = dir.join("dmg");
    extract(tool, &dmg, &dmg_root, emit, cancel)?;
    let mut support = dmg_root.clone();
    if find_dir(&support, "BootCamp").is_none() {
        let hfs = walkdir::WalkDir::new(&dmg_root)
            .into_iter()
            .filter_map(Result::ok)
            .find(|e| e.file_type().is_file() && e.path().extension().is_some_and(|v| v == "hfs"))
            .map(|e| e.into_path())
            .context("Cannot find Boot Camp filesystem in DMG")?;
        support = dir.join("hfs");
        extract(tool, &hfs, &support, emit, cancel)?;
    }
    let bootcamp =
        find_dir(&support, "BootCamp").context("BootCamp folder missing from Apple package")?;
    ensure!(
        bootcamp.join("Setup.exe").is_file(),
        "BootCamp Setup.exe is missing"
    );
    let pe = find_dir(&support, "$WinPEDriver$").context("Apple WinPE driver folder is missing")?;
    let drivers = dir.join("drivers");
    fs::create_dir(&drivers)?;
    copy_apple_driver_tree(&pe, &drivers, cancel)?;
    // Radeon display/audio drivers are already loose INF packages in this bundle.
    // Keep them for both choices: the 15-inch model's external outputs use Radeon.
    if model == "a1707-2016" {
        let amd = bootcamp.join("Drivers/AMD/AMDGraphics/Packages/Drivers");
        ensure!(
            find_any_inf(&amd),
            "Apple Radeon driver packages are missing"
        );
        crate::engine::copy_tree(&amd, &drivers.join("AMDGraphics"), cancel)?;
    }
    // Apple packages several device INF folders inside driver installer EXEs.
    let installers: Vec<_> = walkdir::WalkDir::new(bootcamp.join("Drivers"))
        .into_iter()
        .filter_entry(|entry| {
            !(model == "a1707-2016" && entry.file_type().is_dir() && entry.file_name() == "AMD")
        })
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|v| v.eq_ignore_ascii_case("exe"))
        })
        .map(|e| e.into_path())
        .collect();
    for (n, exe) in installers.iter().enumerate() {
        cancel.check()?;
        if exe
            .file_stem()
            .is_some_and(|name| apple_intel_graphics(&name.to_string_lossy()))
        {
            continue;
        }
        let unpacked = dir.join("installers").join(n.to_string());
        // Not every Apple EXE is a 7-Zip-readable archive. Only extract recognized ones.
        let mut probe = std::process::Command::new(tool);
        platform::hidden(&mut probe).args(["l", "-bd"]).arg(exe);
        if probe.output()?.status.success() {
            extract(tool, exe, &unpacked, emit, cancel)?;
            if find_any_inf(&unpacked) {
                let name = exe.file_stem().context("Installer filename missing")?;
                let dest = drivers.join(name);
                if dest.exists() {
                    fs::remove_dir_all(&dest)?;
                }
                crate::engine::copy_tree(&unpacked, &dest, cancel)?;
            }
        }
    }
    for name in crate::domain::BOOT_ALLOWLIST
        .iter()
        .filter(|_| model == crate::domain::MODEL_ID)
    {
        ensure!(
            drivers.join(name).is_dir(),
            "Required Apple driver folder {name} is missing after extraction"
        );
    }
    guard.keep = true;
    Ok(Assets {
        intel: intel_dir,
        bootcamp,
        drivers,
    })
}
fn apple_intel_graphics(name: &str) -> bool {
    name.eq_ignore_ascii_case("IntelHDGraphics64") || name.eq_ignore_ascii_case("IntelIrisSetup")
}

fn copy_apple_driver_tree(source: &Path, destination: &Path, cancel: &Cancellation) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        cancel.check()?;
        let entry = entry?;
        if apple_intel_graphics(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            crate::engine::copy_tree(&entry.path(), &target, cancel)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn find_any_inf(root: &Path) -> bool {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .any(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|v| v.eq_ignore_ascii_case("inf"))
        })
}

/// Intel's signed EXE is a WiX Burn bundle: 7-Zip opens only its UX cabinet.
/// Recover the attached cabinet using the manifest's size/hash, then restore
/// original payload paths from that same manifest, without running the installer.
fn extract_intel(
    tool: &Path,
    exe: &Path,
    output: &Path,
    emit: &Emit,
    cancel: &Cancellation,
) -> Result<()> {
    extract(tool, exe, output, emit, cancel)?;
    if find_file(output, "igdlh64.inf").is_some() {
        return Ok(());
    }
    #[derive(serde::Deserialize)]
    struct Burn {
        #[serde(rename = "Container", default)]
        containers: Vec<Container>,
        #[serde(rename = "Payload", default)]
        payloads: Vec<Payload>,
    }
    #[derive(serde::Deserialize)]
    struct Container {
        #[serde(rename = "@Id")]
        id: String,
        #[serde(rename = "@FileSize")]
        size: u64,
        #[serde(rename = "@Hash")]
        hash: String,
    }
    #[derive(serde::Deserialize)]
    struct Payload {
        #[serde(rename = "@FilePath")]
        path: String,
        #[serde(rename = "@SourcePath")]
        source: String,
        #[serde(rename = "@Hash")]
        hash: String,
        #[serde(rename = "@FileSize")]
        size: u64,
        #[serde(rename = "@Container", default)]
        container: String,
    }
    let raw = fs::read(output.join("0")).context("Intel Burn manifest missing")?;
    let text = std::str::from_utf8(&raw)?.trim_start_matches('\u{feff}');
    let manifest: Burn = quick_xml::de::from_str(text)?;
    let container = manifest
        .containers
        .iter()
        .find(|c| c.id == "WixAttachedContainer")
        .context("Intel attached container missing")?;
    let data = fs::read(exe)?;
    let mut found = None;
    for (offset, window) in data.windows(4).enumerate() {
        if window != b"MSCF" || offset + 36 > data.len() {
            continue;
        }
        cancel.check()?;
        let size = u32::from_le_bytes(data[offset + 8..offset + 12].try_into()?) as usize;
        if size as u64 != container.size
            || offset.checked_add(size).is_none_or(|end| end > data.len())
        {
            continue;
        }
        let bytes = &data[offset..offset + size];
        if hex::encode(sha1::Sha1::digest(bytes)).eq_ignore_ascii_case(&container.hash) {
            found = Some(bytes);
            break;
        }
    }
    let cabinet = output.join("attached.cab");
    fs::write(
        &cabinet,
        found.context("Intel attached cabinet integrity check failed")?,
    )?;
    drop(data);
    let raw_payloads = output.join("raw");
    extract(tool, &cabinet, &raw_payloads, emit, cancel)?;
    for payload in manifest
        .payloads
        .into_iter()
        .filter(|p| p.container == container.id)
    {
        cancel.check()?;
        let relative = PathBuf::from(payload.path.replace('\\', "/"));
        ensure!(
            relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "Unsafe Intel payload path"
        );
        ensure!(
            Path::new(&payload.source).components().count() == 1,
            "Unsafe Intel cabinet member"
        );
        let input = raw_payloads.join(&payload.source);
        ensure!(
            fs::metadata(&input)?.len() == payload.size,
            "Intel payload size mismatch: {}",
            payload.path
        );
        ensure!(
            hex::encode(sha1::Sha1::digest(fs::read(&input)?)).eq_ignore_ascii_case(&payload.hash),
            "Intel payload hash mismatch: {}",
            payload.path
        );
        let dest = output.join("package").join(relative);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(input, dest)?;
    }
    ensure!(
        find_file(&output.join("package"), "igdlh64.inf").is_some(),
        "Intel original INF missing after Burn extraction"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apple_driver_copy_excludes_old_intel_graphics_and_preserves_other_devices() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!("apple-driver-filter-{}", uuid::Uuid::new_v4()));
        let source = root.join("source");
        let destination = root.join("result");
        for name in [
            "IntelHDGraphics64",
            "IntelIrisSetup",
            "AppleSPIKeyboard",
            "IntelEthernetInstaller64",
        ] {
            fs::create_dir_all(source.join(name)).unwrap();
            fs::write(source.join(name).join("driver.inf"), b"fixture").unwrap();
        }
        copy_apple_driver_tree(&source, &destination, &Cancellation::default()).unwrap();
        assert!(!destination.join("IntelHDGraphics64").exists());
        assert!(!destination.join("IntelIrisSetup").exists());
        assert!(destination.join("AppleSPIKeyboard/driver.inf").is_file());
        assert!(
            destination
                .join("IntelEthernetInstaller64/driver.inf")
                .is_file()
        );
        assert!(
            crate::domain::BOOT_ALLOWLIST
                .iter()
                .all(|name| !apple_intel_graphics(name))
        );
        assert_eq!(root.parent(), Some(temp.as_path()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_corrupt_cache_and_honors_cancellation() {
        let path = std::env::temp_dir().join(format!("driver-test-{}", uuid::Uuid::new_v4()));
        fs::write(&path, b"abc").unwrap();
        let cancel = Cancellation::default();
        assert!(
            verify(
                &path,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                &cancel
            )
            .is_ok()
        );
        fs::write(&path, b"corrupt").unwrap();
        assert!(
            verify(
                &path,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                &cancel
            )
            .is_err()
        );
        cancel.cancel();
        assert!(sha256(&path, &cancel).is_err());
        fs::remove_file(&path).unwrap();
    }
    #[test]
    fn extraction_cleanup_rejects_non_owned_directories() {
        let root = std::env::temp_dir().join(format!("cache-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let other = root.join("my-files");
        fs::create_dir(&other).unwrap();
        assert!(remove_extraction(&root, &other).is_err());
        assert!(other.exists());
        fs::remove_dir(&other).unwrap();
        fs::remove_dir(&root).unwrap();
    }
}
