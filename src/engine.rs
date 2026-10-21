use crate::{
    domain::*,
    download,
    platform::{self, MountedIso},
    wim,
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PreparedBuild {
    pub request: BuildRequest,
    pub assets_root: PathBuf,
    #[serde(default)]
    pub connection_log: Option<PathBuf>,
}

fn copy_file(source: &Path, target: &Path, cancel: &Cancellation) -> Result<()> {
    let mut input = File::open(source)?;
    let mut output = File::create(target)?;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        cancel.check()?;
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n])?;
    }
    Ok(())
}

pub fn copy_tree(source: &Path, dest: &Path, cancel: &Cancellation) -> Result<()> {
    ensure!(
        source.is_dir(),
        "Source folder missing: {}",
        source.display()
    );
    fs::create_dir_all(dest)?;
    for entry in WalkDir::new(source).follow_links(false) {
        cancel.check()?;
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "Symbolic links are not allowed in build inputs"
        );
        let target = dest.join(entry.path().strip_prefix(source)?);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            copy_file(entry.path(), &target, cancel)?;
        }
    }
    Ok(())
}

fn validate_paths(req: &BuildRequest) -> Result<()> {
    req.selected_images()?;
    ensure!(
        cfg!(windows),
        "Patching is currently available on Windows only"
    );
    ensure!(
        req.inventory.iso.is_absolute()
            && req.output.is_absolute()
            && req.workspace.is_absolute()
            && req.cache.is_absolute(),
        "Build paths must be absolute"
    );
    ensure!(
        !req.output.exists(),
        "The output already exists. Choose a new filename"
    );
    ensure!(
        req.output
            .extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("iso")),
        "The output must end in .iso"
    );
    ensure!(
        req.output != req.inventory.iso,
        "The original ISO cannot be overwritten"
    );
    ensure!(req.seven_zip.is_file(), "7-Zip is missing");
    ensure!(
        req.oscdimg.is_file(),
        "Install Windows ADK Deployment Tools, or select oscdimg.exe"
    );
    for path in [&req.workspace, &req.cache] {
        fs::create_dir_all(path)?;
    }
    let output_parent = req.output.parent().context("Output parent is missing")?;
    ensure!(output_parent.is_dir(), "Output folder does not exist");
    let work = req.workspace.canonicalize()?;
    let cache = req.cache.canonicalize()?;
    let output_parent = output_parent.canonicalize()?;
    ensure!(
        !output_parent.starts_with(&cache),
        "Save the output outside the download cache"
    );
    let required = req.required_space()?;
    ensure!(
        fs2::available_space(&cache)? >= 8 * GIB,
        "The driver cache needs at least {} free for downloading and extraction",
        crate::cleanup::format_bytes(8 * GIB)
    );
    ensure!(
        fs2::available_space(&work)? >= required,
        "Not enough workspace disk space; allow at least {}",
        crate::cleanup::format_bytes(required)
    );
    ensure!(
        fs2::available_space(output_parent)? >= req.inventory.iso_size.saturating_add(8 * GIB),
        "Output disk needs room for the patched ISO (source size plus at least {})",
        crate::cleanup::format_bytes(8 * GIB)
    );
    Ok(())
}

pub fn preflight(req: &BuildRequest) -> Result<()> {
    validate_paths(req)
}

/// All network downloads and third-party archive extraction run without elevation.
/// Copy a complete, job-specific input tree; the worker validates it again.
pub fn prepare(req: BuildRequest, emit: &Emit, cancel: &Cancellation) -> Result<PreparedBuild> {
    validate_paths(&req)?;
    let assets = download::prepare_for_model(&req.model, &req.cache, &req.seven_zip, emit, cancel)?;
    let assets_root = assets
        .drivers
        .ancestors()
        .find(|p| {
            p.file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with("extract-"))
        })
        .context("Driver extraction root missing")?
        .to_owned();
    let manifest = serde_json::json!({"intel": assets.intel, "bootcamp": assets.bootcamp, "drivers": assets.drivers});
    fs::write(
        assets_root.join("assets.json"),
        serde_json::to_vec(&manifest)?,
    )?;
    Ok(PreparedBuild {
        request: req,
        assets_root,
        connection_log: None,
    })
}

fn dism(args: Vec<String>, emit: &Emit, cancel: &Cancellation) -> Result<()> {
    let mut all = vec!["/English".into()];
    all.extend(args);
    platform::run(
        &platform::system_binary("dism.exe"),
        &all,
        emit,
        cancel,
        false,
    )
}

struct ImageMount {
    mount: PathBuf,
    emit: Emit,
    active: bool,
}
impl ImageMount {
    fn open(
        wim: &Path,
        index: u32,
        mount: &Path,
        emit: &Emit,
        cancel: &Cancellation,
    ) -> Result<Self> {
        fs::create_dir_all(mount)?;
        // Arm cleanup before mounting: cancellation can arrive while DISM finishes.
        let guard = Self {
            mount: mount.to_owned(),
            emit: emit.clone(),
            active: true,
        };
        dism(
            vec![
                "/Mount-Image".into(),
                format!("/ImageFile:{}", wim.display()),
                format!("/Index:{index}"),
                format!("/MountDir:{}", mount.display()),
                "/CheckIntegrity".into(),
            ],
            emit,
            cancel,
        )?;
        Ok(guard)
    }
    fn commit(mut self, cancel: &Cancellation) -> Result<()> {
        dism(
            vec![
                "/Unmount-Image".into(),
                format!("/MountDir:{}", self.mount.display()),
                "/Commit".into(),
                "/CheckIntegrity".into(),
            ],
            &self.emit,
            cancel,
        )?;
        self.active = false;
        Ok(())
    }
}
impl Drop for ImageMount {
    fn drop(&mut self) {
        if self.active {
            (self.emit)(Event::Log("Discarding app-owned image mount…".into()));
            if let Err(error) = dism(
                vec![
                    "/Unmount-Image".into(),
                    format!("/MountDir:{}", self.mount.display()),
                    "/Discard".into(),
                ],
                &self.emit,
                &Cancellation::default(),
            ) {
                (self.emit)(Event::Log(format!(
                    "Mount cleanup requires recovery: {error:#}"
                )));
            }
        }
    }
}

pub fn run(prepared: PreparedBuild, emit: Emit, cancel: Cancellation) -> Result<PathBuf> {
    ensure!(
        platform::is_admin(),
        "The build worker must run as administrator"
    );
    let req = &prepared.request;
    let _cache_activity = crate::cleanup::cache_activity(&req.cache)?;
    validate_paths(req)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(req.workspace.join(".build.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("Another build is using this working folder")?;
    recover_mounts(&req.workspace, &emit, &cancel)?;
    ensure!(
        fs::metadata(&req.inventory.iso)?.len() == req.inventory.iso_size
            && platform::modified(&req.inventory.iso)? == req.inventory.iso_modified,
        "The source ISO changed since scanning. Scan it again"
    );
    // Do not run arbitrary user-supplied executables with elevation.
    let signature = platform::powershell(
        r#"$s=Get-AuthenticodeSignature -LiteralPath $env:PATCHER_TOOL; if($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation'){throw 'oscdimg.exe must have a valid Microsoft signature'}"#,
        &[("PATCHER_TOOL", req.oscdimg.as_os_str())],
    );
    signature.context("ISO tool verification failed. Use the Microsoft ADK copy of oscdimg.exe")?;
    let job = req
        .workspace
        .join(format!("build-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&job)?;
    fs::write(job.join("job.json"), serde_json::to_vec_pretty(req)?)?;
    crate::cleanup::register(&job, req, prepared.connection_log.clone())?;
    let file = Arc::new(Mutex::new(
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(job.join("build.log"))?,
    ));
    let original = emit;
    let emit: Emit = Arc::new(move |event| {
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(
                file,
                "{}",
                serde_json::to_string(&event).unwrap_or_default()
            );
        }
        original(event);
    });
    emit(Event::Log(format!(
        "Build workspace and log: {}",
        job.display()
    )));
    let result = run_job(&prepared, &job, &emit, &cancel);
    let state = if result.is_ok() {
        crate::cleanup::BuildState::Complete
    } else if cancel.is_cancelled() {
        crate::cleanup::BuildState::Cancelled
    } else {
        crate::cleanup::BuildState::Failed
    };
    if let Err(error) = crate::cleanup::finish(&job, state) {
        emit(Event::Log(format!(
            "Build history update failed; workspace retained: {error:#}"
        )));
    }
    if result.is_err() {
        emit(Event::Log(format!(
            "Build files retained for diagnosis: {}",
            job.display()
        )));
    }
    result
}

fn load_assets(prepared: &PreparedBuild, cancel: &Cancellation) -> Result<download::Assets> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        intel: Option<PathBuf>,
        bootcamp: PathBuf,
        drivers: PathBuf,
    }
    let root = prepared.assets_root.canonicalize()?;
    ensure!(
        root.starts_with(prepared.request.cache.canonicalize()?),
        "Assets must be inside the app cache"
    );
    let manifest: Manifest = serde_json::from_slice(&fs::read(root.join("assets.json"))?)?;
    for p in [&manifest.bootcamp, &manifest.drivers]
        .into_iter()
        .chain(manifest.intel.iter())
    {
        ensure!(
            p.canonicalize()?.starts_with(&root),
            "Asset path escaped extraction root"
        );
    }
    for package in crate::driver_prefetch::packages_for_model(&prepared.request.model)? {
        download::verify(
            &prepared.request.cache.join(package.filename),
            package.hash,
            cancel,
        )?;
    }
    ensure!(
        manifest.intel.is_some() == (prepared.request.model != "a1707-2016"),
        "Graphics assets do not match the selected model"
    );
    if let Some(intel) = &manifest.intel {
        validate_intel_for_model(intel, &prepared.request.model)?;
    }
    Ok(download::Assets {
        intel: manifest.intel,
        bootcamp: manifest.bootcamp,
        drivers: manifest.drivers,
    })
}

fn run_job(
    prepared: &PreparedBuild,
    job: &Path,
    emit: &Emit,
    cancel: &Cancellation,
) -> Result<PathBuf> {
    let req = &prepared.request;
    let assets = load_assets(prepared, cancel)?;
    stage(
        emit,
        "Checking Windows media",
        "Rechecking image metadata before servicing",
    );
    let source = MountedIso::open(&req.inventory.iso)?;
    let fresh = platform::scan_root(&req.inventory.iso, &source.root)?;
    ensure!(
        fresh.images == req.inventory.images && fresh.install_file == req.inventory.install_file,
        "Windows image metadata changed. Scan again"
    );
    let root = job.join("iso-root");
    stage(
        emit,
        "Copying Windows media",
        "Your original ISO stays unchanged",
    );
    fs::create_dir(&root)?;
    for entry in WalkDir::new(&source.root).follow_links(false) {
        cancel.check()?;
        let entry = entry?;
        let rel = entry.path().strip_prefix(&source.root)?;
        let normalized = rel
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        if [
            "sources/install.wim",
            "sources/install.esd",
            "sources/ei.cfg",
            "sources/pid.txt",
        ]
        .contains(&normalized.as_str())
        {
            continue;
        }
        ensure!(!entry.file_type().is_symlink(), "Source ISO contains links");
        let dest = root.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dest)?;
        } else if entry.file_type().is_file() {
            copy_file(entry.path(), &dest, cancel)?;
        }
    }
    ensure!(
        !root.join("$WinPEDriver$").exists(),
        "Source contains custom WinPE drivers. Use original Microsoft media"
    );
    let selected = req.selected_images()?;
    let target = root.join("sources/install.wim");
    for (n, image) in selected.iter().enumerate() {
        stage(
            emit,
            "Exporting selected editions",
            format!("{} ({}/{})", image.name, n + 1, selected.len()),
        );
        dism(
            vec![
                "/Export-Image".into(),
                format!(
                    "/SourceImageFile:{}",
                    source
                        .root
                        .join("sources")
                        .join(&req.inventory.install_file)
                        .display()
                ),
                format!("/SourceIndex:{}", image.index),
                format!("/DestinationImageFile:{}", target.display()),
                "/Compress:max".into(),
                "/CheckIntegrity".into(),
            ],
            emit,
            cancel,
        )?;
    }
    drop(source);
    let pe_drivers = root.join("$WinPEDriver$");
    fs::create_dir(&pe_drivers)?;
    for folder in BOOT_ALLOWLIST {
        if req.model != MODEL_ID && !assets.drivers.join(folder).is_dir() {
            continue;
        }
        copy_tree(
            &assets.drivers.join(folder),
            &pe_drivers.join(folder),
            cancel,
        )?;
    }
    validate_model_boot_drivers(&pe_drivers, &req.model)?;
    copy_tree(&assets.bootcamp, &root.join("BootCamp"), cancel)?;
    fs::write(root.join("autounattend.xml"), ANSWER_FILE)?;
    let edition_cfg = if selected.len() == 1 {
        format!(
            "[EditionID]\r\n{}\r\n[Channel]\r\nRetail\r\n[VL]\r\n0\r\n",
            selected[0].edition
        )
    } else {
        "[Channel]\r\nRetail\r\n[VL]\r\n0\r\n".into()
    };
    fs::write(root.join("sources/ei.cfg"), edition_cfg)?;
    for (n, image) in selected.iter().enumerate() {
        stage(
            emit,
            "Patching Windows editions",
            format!("{} ({}/{})", image.name, n + 1, selected.len()),
        );
        let mount = ImageMount::open(
            &target,
            n as u32 + 1,
            &job.join("mount-install"),
            emit,
            cancel,
        )?;
        if let Some(intel) = &assets.intel {
            inject_signed_driver(&mount.mount, intel, &req.model, emit, cancel)?;
        }
        if req.model == "a1707-2016" {
            dism(
                vec![
                    format!("/Image:{}", mount.mount.display()),
                    "/Add-Driver".into(),
                    format!("/Driver:{}", assets.drivers.join("AMDGraphics").display()),
                    "/Recurse".into(),
                ],
                emit,
                cancel,
            )?;
        }
        copy_tree(&assets.drivers, &mount.mount.join("Drivers"), cancel)?;
        copy_tree(&assets.bootcamp, &mount.mount.join("BootCamp"), cancel)?;
        fs::create_dir_all(mount.mount.join("Windows/Setup/Scripts"))?;
        fs::write(
            mount.mount.join("Windows/Setup/Scripts/SetupComplete.cmd"),
            include_bytes!("../payloads/a1706/SetupComplete.cmd"),
        )?;
        if let Some(intel) = &assets.intel {
            validate_signed_store(&mount.mount, intel, &req.model)?;
        }
        if n == 0 && req.model == MODEL_ID {
            let support = job.join("boot-support");
            fs::create_dir_all(support.join("en-US"))?;
            for rel in ["DisplaySwitch.exe", "en-US/DisplaySwitch.exe.mui"] {
                let source = mount.mount.join("Windows/System32").join(rel);
                if source.is_file() {
                    fs::copy(&source, support.join(rel))?;
                }
            }
        }
        mount.commit(cancel)?;
    }
    if req.model == MODEL_ID {
        let intel = assets
            .intel
            .as_ref()
            .context("A1706 Intel graphics missing")?;
        stage(
            emit,
            "Patching Windows Setup",
            "Signed Iris driver and v17 HDMI startup sequence",
        );
        let boot = root.join("sources/boot.wim");
        let mount = ImageMount::open(&boot, 2, &job.join("mount-boot"), emit, cancel)?;
        inject_signed_driver(&mount.mount, intel, &req.model, emit, cancel)?;
        write_boot_payload(&mount.mount)?;
        for rel in ["DisplaySwitch.exe", "en-US/DisplaySwitch.exe.mui"] {
            let source = job.join("boot-support").join(rel);
            if source.is_file() {
                fs::copy(&source, mount.mount.join("Windows/System32").join(rel))?;
            }
        }
        ensure!(
            mount
                .mount
                .join("Windows/System32/DisplaySwitch.exe")
                .is_file(),
            "DisplaySwitch.exe missing from Windows image"
        );
        validate_signed_store(&mount.mount, intel, &req.model)?;
        prepare_a1706_boot_graphics(&mount.mount, intel, emit)?;
        mount.commit(cancel)?;
    }
    stage(
        emit,
        "Verifying output images",
        "Checking edition mapping and boot-path payloads",
    );
    let images = wim::read_images(&target)?;
    ensure!(
        images.len() == selected.len(),
        "Output edition count does not match selection"
    );
    for (n, expected) in selected.iter().enumerate() {
        ensure!(
            images[n].index == n as u32 + 1 && images[n].edition == expected.edition,
            "Output edition mapping mismatch"
        );
    }
    ensure!(
        !ANSWER_FILE.contains("DiskConfiguration") && !ANSWER_FILE.contains("ImageInstall"),
        "Answer file contains disk automation"
    );
    if req.model == MODEL_ID {
        fs::write(root.join("README-A1706.txt"), INSTALL_GUIDE)?;
    } else {
        fs::write(
            root.join("README-MacBook.txt"),
            format!(
                "Model: {}\r\nExternal display graphics: {}\r\nUnverified build using this model's selected graphics and Apple Boot Camp drivers.\r\nRun C:\\BootCamp\\Setup.exe after Windows installation.\r\nReport success or problems at https://github.com/Zenodeon/MacbookWindowsISOPatcher/issues\r\n",
                req.model,
                if req.model == "a1707-2016" {
                    "AMD Radeon Pro"
                } else {
                    crate::driver_prefetch::intel_graphics(&req.model)?.version
                }
            ),
        )?;
    }
    // Keep the temporary ISO on the output filesystem for an atomic, no-clobber publication.
    let partial = req
        .output
        .parent()
        .unwrap()
        .join(format!(".a1706-{}.partial.iso", uuid::Uuid::new_v4()));
    crate::cleanup::track_partial(job, &partial)?;
    stage(
        emit,
        "Building patched ISO",
        "Creating BIOS and UEFI bootable media",
    );
    let args = oscdimg_args(&root, &partial);
    let built = platform::run(&req.oscdimg, &args, emit, cancel, true);
    if let Err(error) = built {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    stage(
        emit,
        "Checking finished ISO",
        "Reading the final ISO and verifying its selected editions",
    );
    let final_check = (|| -> Result<()> {
        let mounted = MountedIso::open(&partial)?;
        let output_images = wim::read_images(&mounted.root.join("sources/install.wim"))?;
        ensure!(
            output_images == images,
            "Finished ISO image metadata did not match"
        );
        ensure!(
            mounted.root.join("BootCamp/Setup.exe").is_file(),
            "Boot Camp installer is missing from final ISO"
        );
        validate_model_boot_drivers(&mounted.root.join("$WinPEDriver$"), &req.model)?;
        cancel.check()?;
        Ok(())
    })();
    if let Err(error) = final_check {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    // hard_link fails if destination exists; unlike rename on Unix it never replaces a file.
    platform::publish_iso(&partial, &req.output)
        .context("Could not publish ISO. The validated temporary ISO is retained")?;
    crate::cleanup::published(job).context("ISO was created, but its ownership record could not be saved. The ISO and workspace have been retained")?;
    emit(Event::Log(format!(
        "Completed. Build log: {}",
        job.join("build.log").display()
    )));
    // Large staging files belong to this UUID job only. Retain job.json and build.log.
    ensure!(
        root.canonicalize()?.starts_with(job.canonicalize()?),
        "Cleanup path escaped build workspace"
    );
    if let Err(error) = fs::remove_dir_all(&root) {
        emit(Event::Log(format!(
            "ISO is ready; temporary media cleanup failed: {error}"
        )));
    }
    Ok(req.output.clone())
}

fn inject_signed_driver(
    mount: &Path,
    intel: &Path,
    model: &str,
    emit: &Emit,
    cancel: &Cancellation,
) -> Result<()> {
    ensure!(
        download::find_file(&mount.join("Windows/INF"), "igdiris64.inf").is_none(),
        "Unsigned slim Iris driver found in source"
    );
    if validate_signed_store(mount, intel, model).is_ok() {
        emit(Event::Log(
            "The exact signed Intel graphics package is already present; skipping duplicate import"
                .into(),
        ));
        return Ok(());
    }
    dism(
        vec![
            format!("/Image:{}", mount.display()),
            "/Add-Driver".into(),
            format!(
                "/Driver:{}",
                intel
                    .join(crate::driver_prefetch::intel_graphics(model)?.inf)
                    .display()
            ),
        ],
        emit,
        cancel,
    )
}
fn text_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    if bytes.starts_with(&[0xff, 0xfe]) {
        Ok(String::from_utf16(
            &bytes[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|x| u16::from_le_bytes([x[0], x[1]]))
                .collect::<Vec<_>>(),
        )?)
    } else {
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}
pub fn validate_intel(dir: &Path) -> Result<()> {
    validate_intel_for_model(dir, MODEL_ID)
}
pub fn validate_intel_for_model(dir: &Path, model: &str) -> Result<()> {
    let profile = crate::driver_prefetch::intel_graphics(model)?;
    let inf = text_file(&dir.join(profile.inf))?;
    let compact = inf
        .lines()
        .map(|l| {
            l.split(';')
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    ensure!(
        compact.lines().any(|line| line.starts_with("driverver=")
            && line.ends_with(&format!(",{}", profile.version))),
        "Unexpected Intel driver version"
    );
    ensure!(
        compact.contains(&format!("catalogfile={}", profile.catalog))
            && dir.join(profile.catalog).is_file(),
        "Signed Intel catalog missing"
    );
    ensure!(
        supports_generic_gpu(&compact, profile.device_id),
        "Intel driver does not support this model's graphics hardware"
    );
    if cfg!(windows) {
        platform::powershell(
            r#"$s=Get-AuthenticodeSignature -LiteralPath $env:PATCHER_CAT; if($s.Status -ne 'Valid'){throw 'Intel driver catalog signature is invalid'}"#,
            &[("PATCHER_CAT", dir.join(profile.catalog).as_os_str())],
        )?;
    }
    Ok(())
}
fn supports_generic_gpu(compact_inf: &str, device_id: &str) -> bool {
    let hardware = format!("pci\\ven_8086&dev_{device_id}");
    compact_inf
        .lines()
        .filter(|line| !line.starts_with(';'))
        .filter_map(|line| line.split_once('='))
        .any(|(key, value)| {
            key.starts_with('%') && value.split(',').skip(1).any(|id| id.trim() == hardware)
        })
}
fn validate_signed_store(mount: &Path, intel: &Path, model: &str) -> Result<PathBuf> {
    let profile = crate::driver_prefetch::intel_graphics(model)?;
    let store = mount.join("Windows/System32/DriverStore/FileRepository");
    let expected = fs::read(intel.join(profile.inf))?;
    let mut found = None;
    for entry in WalkDir::new(&store).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        ensure!(
            name != "igdiris64.inf",
            "Unsigned Iris package is still in the driver store"
        );
        if name.ends_with(".inf") {
            let text = text_file(entry.path())?.to_ascii_lowercase();
            if text.contains(&format!("dev_{}", profile.device_id)) {
                let catalog = text
                    .lines()
                    .filter_map(|l| l.split(';').next())
                    .find_map(|line| {
                        let (key, value) = line.split_once('=')?;
                        key.trim()
                            .starts_with("catalogfile")
                            .then(|| value.trim().trim_matches('"').to_owned())
                    })
                    .context("An unsigned Intel graphics INF was found in the image")?;
                ensure!(
                    entry.path().parent().unwrap().join(catalog).is_file(),
                    "An Intel graphics package is missing its catalog"
                );
                if text.contains(profile.version) && name == profile.inf {
                    ensure!(
                        fs::read(entry.path())? == expected,
                        "An altered Intel graphics INF was found"
                    );
                }
            }
        }
        if name == profile.inf && fs::read(entry.path())? == expected {
            let catalog = entry.path().parent().unwrap().join(profile.catalog);
            ensure!(
                fs::read(catalog)? == fs::read(intel.join(profile.catalog))?,
                "Intel catalog changed"
            );
            found = Some(entry.path().parent().unwrap().to_owned());
        }
    }
    found.context("Signed Intel package is missing from the driver store")
}
pub fn validate_boot_drivers(root: &Path) -> Result<()> {
    validate_model_boot_drivers(root, MODEL_ID)
}

fn validate_model_boot_drivers(root: &Path, model: &str) -> Result<()> {
    let mut count = 0;
    for child in fs::read_dir(root)? {
        let child = child?;
        ensure!(
            child.file_type()?.is_dir(),
            "Boot driver entry must be a directory"
        );
        count += 1;
        let name = child.file_name().to_string_lossy().to_string();
        ensure!(
            BOOT_ALLOWLIST.contains(&name.as_str()),
            "Forbidden boot-path driver folder: {name}"
        );
    }
    ensure!(count > 0, "No compatible Apple boot driver folders found");
    for name in BOOT_ALLOWLIST.iter().filter(|_| model == MODEL_ID) {
        ensure!(root.join(name).is_dir(), "Boot driver {name} missing");
    }
    Ok(())
}
fn prepare_a1706_boot_graphics(mount: &Path, intel: &Path, emit: &Emit) -> Result<()> {
    emit(Event::Log(
        "Preparing A1706 offline graphics startup (BasicDisplay enabled, Intel demand-start)"
            .into(),
    ));
    let store = validate_signed_store(mount, intel, MODEL_ID)?;
    // DISM stages this non-boot-start package without necessarily creating its
    // service. The original, iterated image already had this binary/service.
    // Recreate that end state from the unmodified, signed package on fresh ISOs.
    let binary = fs::read(store.join("igdkmd64.sys"))
        .context("Signed A1706 driver store is missing igdkmd64.sys")?;
    ensure!(
        !binary.is_empty() && binary == fs::read(intel.join("igdkmd64.sys"))?,
        "A1706 graphics kernel binary differs from the signed Intel package"
    );
    let target = mount.join("Windows/System32/drivers/igdkmd64.sys");
    fs::write(&target, &binary).context("Could not prepare the A1706 graphics kernel binary")?;
    let report = a1706_boot_registry(mount, false)?;
    ensure!(
        fs::read(&target)? == binary,
        "A1706 graphics kernel binary changed during preparation"
    );
    for line in report.lines() {
        emit(Event::Log(line.to_owned()));
    }
    Ok(())
}

fn a1706_boot_registry(mount: &Path, validate_only: bool) -> Result<String> {
    let script = format!(
        "{}\n{}",
        include_str!("../payloads/a1706/boot-graphics.ps1"),
        r#"
        $name='A1706_'+[Guid]::NewGuid().ToString('N')
        $reg="$env:SystemRoot\System32\reg.exe"
        & $reg load "HKLM\$name" "$env:PATCHER_MOUNT\Windows\System32\config\SYSTEM" | Out-Null
        if($LASTEXITCODE -ne 0){throw "Cannot load offline A1706 SYSTEM hive (code $LASTEXITCODE)"}
        $root=$null
        try {
          $root=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($name,$true)
          if($null -eq $root){throw 'Cannot open loaded A1706 SYSTEM hive'}
          if($env:PATCHER_GRAPHICS_VALIDATE_ONLY -ne '1'){Set-A1706BootGraphics $root | Out-Null}
          Set-A1706BootGraphics $root -ValidateOnly
        } finally {
          if($null -ne $root){$root.Dispose()}
          & $reg unload "HKLM\$name" | Out-Null
          if($LASTEXITCODE -ne 0){throw "Cannot unload offline A1706 SYSTEM hive (code $LASTEXITCODE); workspace retained"}
        }
    "#
    );
    platform::powershell(
        &script,
        &[
            ("PATCHER_MOUNT", mount.as_os_str()),
            (
                "PATCHER_GRAPHICS_VALIDATE_ONLY",
                std::ffi::OsStr::new(if validate_only { "1" } else { "0" }),
            ),
        ],
    )
    .context("A1706 Windows Setup graphics preparation failed")
}
fn write_boot_payload(mount: &Path) -> Result<()> {
    let dir = mount.join("Windows/System32");
    let script = include_bytes!("../payloads/a1706/a1706.cmd");
    ensure!(
        script.is_ascii() && !script.contains(&0),
        "Startup script must be ASCII with no NULs"
    );
    fs::write(dir.join("a1706.cmd"), script)?;
    fs::write(
        dir.join("winpeshl.ini"),
        include_bytes!("../payloads/a1706/winpeshl.ini"),
    )?;
    #[cfg(windows)]
    {
        for (name, bytes) in [
            (
                "a1706iris.exe",
                include_bytes!(concat!(env!("OUT_DIR"), "/a1706iris.exe")).as_slice(),
            ),
            (
                "a1706dlg.exe",
                include_bytes!(concat!(env!("OUT_DIR"), "/a1706dlg.exe")).as_slice(),
            ),
            (
                "a1706cue.exe",
                include_bytes!(concat!(env!("OUT_DIR"), "/a1706cue.exe")).as_slice(),
            ),
            (
                "a1706gpu.exe",
                include_bytes!(concat!(env!("OUT_DIR"), "/a1706gpu.exe")).as_slice(),
            ),
        ] {
            fs::write(dir.join(name), bytes)?;
        }
    }
    Ok(())
}

fn recover_mounts(workspace: &Path, emit: &Emit, cancel: &Cancellation) -> Result<()> {
    let json = platform::powershell(
        "ConvertTo-Json -Compress -InputObject @((Get-WindowsImage -Mounted) | ForEach-Object { $_.Path })",
        &[],
    )?;
    let mounts: Vec<PathBuf> = if json.trim().is_empty() {
        vec![]
    } else {
        serde_json::from_str(&json)?
    };
    let workspace = workspace.canonicalize()?;
    for path in mounts {
        cancel.check()?;
        let Ok(path) = path.canonicalize() else {
            continue;
        };
        let Some(job) = path.parent() else {
            continue;
        };
        if job.parent() != Some(workspace.as_path()) {
            continue;
        }
        if !path
            .file_name()
            .is_some_and(|v| v == "mount-boot" || v == "mount-install")
        {
            continue;
        }
        let name = job.file_name().unwrap().to_string_lossy();
        if !name
            .strip_prefix("build-")
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        {
            continue;
        }
        let Some(recorded) = fs::read(job.join("job.json"))
            .ok()
            .and_then(|data| serde_json::from_slice::<BuildRequest>(&data).ok())
        else {
            continue;
        };
        if !crate::models::MODEL_PROFILES
            .iter()
            .any(|model| model.id == recorded.model)
            || recorded.workspace.canonicalize().ok().as_ref() != Some(&workspace)
        {
            continue;
        }
        stage(
            emit,
            "Recovering interrupted build",
            format!("Discarding app-owned mount {}", path.display()),
        );
        dism(
            vec![
                "/Unmount-Image".into(),
                format!("/MountDir:{}", path.display()),
                "/Discard".into(),
            ],
            emit,
            cancel,
        )?;
    }
    Ok(())
}
pub fn oscdimg_args(root: &Path, output: &Path) -> Vec<String> {
    vec![
        "-m".into(),
        "-u2".into(),
        "-udfver102".into(),
        "-lESD-ISO".into(),
        format!(
            "-bootdata:2#p0,e,b{}#pEF,e,b{}",
            root.join("boot/etfsboot.com").display(),
            root.join("efi/microsoft/boot/efisys.bin").display()
        ),
        root.display().to_string(),
        output.display().to_string(),
    ]
}
pub const ANSWER_FILE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
  <settings pass="windowsPE">
    <component name="Microsoft-Windows-PnpCustomizationsWinPE" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
      <DriverPaths><PathAndCredentials wcm:action="add" wcm:keyValue="1"><Path>\$WinPEDriver$</Path></PathAndCredentials></DriverPaths>
    </component>
    <component name="Microsoft-Windows-Setup" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS"><UserData><AcceptEula>true</AcceptEula></UserData></component>
  </settings>
  <settings pass="specialize">
    <component name="Microsoft-Windows-PnpCustomizationsNonWinPE" processorArchitecture="amd64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
      <DriverPaths><PathAndCredentials wcm:action="add" wcm:keyValue="1"><Path>C:\Drivers</Path></PathAndCredentials></DriverPaths>
    </component>
  </settings>
</unattend>
"#;
pub const INSTALL_GUIDE: &str = "MacBook Pro A1706 — installation\r\n\r\n1. In macOS, create the BOOTCAMP partition using Boot Camp Assistant. Do not give it this ISO.\r\n2. Copy this ISO to a Ventoy USB and use WIMBOOT (press w). Keep the USB connected.\r\n3. Select and format only BOOTCAMP in Windows Setup. Leave the Mac and recovery partitions alone.\r\n4. After reaching Windows, check that Iris 550 uses driver 21.20.16.5174.\r\n5. Run C:\\BootCamp\\Setup.exe as administrator, then reboot for Touch Bar and full trackpad support. Do not run the old IntelHDGraphics64 installer.\r\n\r\nOnly Pro was validated in the source project. Other editions and this app's generated ISOs require hardware testing. The HDMI fix targets the 2016 A1706 with its internal display disconnected.\r\n\r\nLogs are saved under A1706Logs on writable media.\r\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "Elevated DISM test on a disposable copy of boot.wim; requires explicit fixture paths"]
    fn a1706_fresh_boot_image_prepares_and_persists_graphics_service() {
        assert!(
            platform::is_admin(),
            "Run this integration test as administrator"
        );
        let source = PathBuf::from(
            std::env::var_os("MACBOOK_A1706_TEST_BOOT_WIM").expect("Provide a stock boot.wim"),
        );
        let archive = PathBuf::from(
            std::env::var_os("MACBOOK_A1706_TEST_INTEL_EXE")
                .expect("Provide the pinned Intel archive"),
        );
        let root = PathBuf::from(
            std::env::var_os("MACBOOK_A1706_TEST_ROOT")
                .expect("Provide a new disposable test directory"),
        );
        assert!(!root.exists(), "Test directory must be new");
        fs::create_dir_all(&root).unwrap();
        let emit: Emit = Arc::new(|event| println!("{event:?}"));
        let cancel = Cancellation::default();
        let original_hash = download::sha256(&source, &cancel).unwrap();
        let package = crate::driver_prefetch::packages_for_model(MODEL_ID).unwrap();
        download::verify(&archive, package[0].hash, &cancel).unwrap();
        let copied_wim = root.join("boot.wim");
        fs::copy(&source, &copied_wim).unwrap();
        download::extract_intel(
            Path::new("C:/Program Files/7-Zip/7z.exe"),
            &archive,
            &root.join("intel"),
            &emit,
            &cancel,
        )
        .unwrap();
        let inf = download::find_file(&root.join("intel"), "igdlh64.inf").unwrap();
        let intel = inf.parent().unwrap();
        validate_intel_for_model(intel, MODEL_ID).unwrap();
        let mount_path = root.join("mount-boot");
        let mounted = ImageMount::open(&copied_wim, 2, &mount_path, &emit, &cancel).unwrap();
        inject_signed_driver(&mounted.mount, intel, MODEL_ID, &emit, &cancel).unwrap();
        // Reproduce the reported failure after a successful signed DISM import.
        let before = a1706_boot_registry(&mounted.mount, true).unwrap_err();
        assert!(
            format!("{before:#}").contains("igfx service is missing"),
            "Unexpected pre-fix error: {before:#}"
        );
        prepare_a1706_boot_graphics(&mounted.mount, intel, &emit).unwrap();
        mounted.commit(&cancel).unwrap();
        let mounted = ImageMount::open(&copied_wim, 2, &mount_path, &emit, &cancel).unwrap();
        let report = a1706_boot_registry(&mounted.mount, true).unwrap();
        assert!(report.contains("BasicDisplay Start=1; igfx Start=3"));
        let store = validate_signed_store(&mounted.mount, intel, MODEL_ID).unwrap();
        assert_eq!(
            fs::read(store.join("igdkmd64.sys")).unwrap(),
            fs::read(mounted.mount.join("Windows/System32/drivers/igdkmd64.sys")).unwrap()
        );
        // A second run must preserve the same validated end state.
        prepare_a1706_boot_graphics(&mounted.mount, intel, &emit).unwrap();
        mounted.commit(&cancel).unwrap();
        assert_eq!(download::sha256(&source, &cancel).unwrap(), original_hash);
        println!(
            "Disposable A1706 image passed; retained at {}",
            root.display()
        );
    }

    #[test]
    fn a1706_rejects_changed_kernel_binary_before_writing_image() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!("a1706-kernel-test-{}", uuid::Uuid::new_v4()));
        let intel = root.join("intel");
        let mount = root.join("image");
        let store = mount.join("Windows/System32/DriverStore/FileRepository/igdlh64.inf_test");
        fs::create_dir_all(&intel).unwrap();
        fs::create_dir_all(&store).unwrap();
        for dir in [&intel, &store] {
            fs::write(
                dir.join("igdlh64.inf"),
                b"[Version]\nCatalogFile=igdlh.cat\n; DEV_1927\n",
            )
            .unwrap();
            fs::write(dir.join("igdlh.cat"), b"fixture catalog").unwrap();
        }
        fs::write(intel.join("igdkmd64.sys"), b"expected kernel").unwrap();
        fs::write(store.join("igdkmd64.sys"), b"changed kernel").unwrap();
        let emit: Emit = Arc::new(|_| {});
        let error = prepare_a1706_boot_graphics(&mount, &intel, &emit).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("differs from the signed Intel package")
        );
        assert!(!mount.join("Windows/System32/drivers/igdkmd64.sys").exists());
        assert_eq!(root.parent(), Some(temp.as_path()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn gpu_matching_requires_a_generic_device_mapping_not_an_oem_only_or_comment_match() {
        assert!(supports_generic_gpu(
            "%gpu%=ikbld_w10_ds,pci\\ven_8086&dev_5927",
            "5927"
        ));
        assert!(!supports_generic_gpu(
            "%gpu%=install,pci\\ven_8086&dev_5927&subsys_0000103c",
            "5927"
        ));
        assert!(!supports_generic_gpu(
            "; %gpu%=install,pci\\ven_8086&dev_5927",
            "5927"
        ));
        assert!(!supports_generic_gpu(
            "%gpu%=install,pci\\ven_8086&dev_1927",
            "5927"
        ));
    }
    #[test]
    fn unverified_models_accept_available_boot_drivers_but_reject_graphics() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!("model-boot-drivers-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        assert!(validate_model_boot_drivers(&root, "a1707-2016").is_err());
        fs::create_dir(root.join("AppleSSD64")).unwrap();
        assert!(validate_model_boot_drivers(&root, "a1707-2016").is_ok());
        assert!(validate_model_boot_drivers(&root, MODEL_ID).is_err());
        fs::create_dir(root.join("IntelHDGraphics64")).unwrap();
        assert!(validate_model_boot_drivers(&root, "a1707-2016").is_err());
        assert_eq!(root.parent(), Some(temp.as_path()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn iso_has_both_boot_entries_without_optimization() {
        let args = oscdimg_args(Path::new("C:/a b/media"), Path::new("C:/out.iso"));
        assert!(!args.contains(&"-o".into()));
        assert!(args.contains(&"-lESD-ISO".into()));
        assert!(args[4].contains("#p0,e,b"));
        assert!(args[4].contains("#pEF,e,b"));
    }
    #[test]
    fn answer_file_never_automates_disks() {
        for forbidden in [
            "DiskConfiguration",
            "ImageInstall",
            "WillWipeDisk",
            "ProductKey",
        ] {
            assert!(!ANSWER_FILE.contains(forbidden));
        }
    }
    #[test]
    fn runtime_waits_for_gpu_before_setup() {
        let script = include_str!("../payloads/a1706/a1706.cmd");
        assert!(
            script.find(":waitiris").unwrap() < script.find("set \"MSG=launching setup").unwrap()
        );
        assert!(script.contains("a1706gpu.exe"));
        assert!(!script.contains("3&11583659"));
        assert_eq!(
            script.matches("pnputil /restart-device \"!GPU!\"").count(),
            1
        );
        assert!(!script.contains("/ForceUnsigned"));
        assert!(!script.as_bytes().contains(&0));
    }
}
