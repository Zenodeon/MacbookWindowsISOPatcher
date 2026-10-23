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

#[derive(Debug, Clone)]
pub struct OutputConflict(pub PathBuf);
impl std::fmt::Display for OutputConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "The output already exists: {}",
            crate::platform::display_path(&self.0)
        )
    }
}
impl std::error::Error for OutputConflict {}
fn validate_paths(req: &BuildRequest) -> Result<()> {
    validate_paths_with_credit(req, None)
}
fn validate_paths_with_credit(
    req: &BuildRequest,
    replacement: Option<&crate::cleanup::Fingerprint>,
) -> Result<()> {
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
    match fs::symlink_metadata(&req.output) {
        Ok(_) if replacement.is_none() => return Err(OutputConflict(req.output.clone()).into()),
        Ok(_) => ensure!(
            replacement == Some(&crate::cleanup::fingerprint(&req.output)?),
            "The replacement ISO changed"
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
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
        ensure!(
            !path.join(".storage-moved.json").exists(),
            "Storage at {} was moved. Restart the app to load its new location",
            path.display()
        );
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
    let credit = |path: &Path| -> u64 {
        replacement
            .filter(|_| crate::platform::same_volume(path, &req.output).unwrap_or(false))
            .map_or(0, |f| f.size)
    };
    require_space(
        &cache,
        (8 * GIB).saturating_sub(credit(&cache)),
        "Driver cache (downloads and extraction)",
    )?;
    require_space(
        &work,
        required.saturating_sub(credit(&work)),
        "WorkingSpace",
    )?;
    require_space(
        &output_parent,
        req.inventory
            .iso_size
            .saturating_add(8 * GIB)
            .saturating_sub(credit(&output_parent)),
        "Output ISO folder",
    )?;
    Ok(())
}

fn require_space(path: &Path, required: u64, purpose: &str) -> Result<()> {
    let available = fs2::available_space(path)?;
    let drive = path.ancestors().last().unwrap_or(path);
    ensure!(
        available >= required,
        "{purpose}: {}\nDrive: {}\nAvailable: {} · Required: {}",
        crate::platform::display_path(path),
        crate::platform::display_path(drive),
        crate::cleanup::format_bytes(available),
        crate::cleanup::format_bytes(required)
    );
    Ok(())
}

pub fn preflight(req: &BuildRequest) -> Result<()> {
    workspace_available(&req.workspace)?;
    validate_paths(req)
}

/// Check all normal prerequisites before permanently deleting a verified output.
pub fn preflight_replacement(req: &BuildRequest) -> Result<()> {
    workspace_available(&req.workspace)?;
    ensure!(
        fs::metadata(&req.inventory.iso)?.len() == req.inventory.iso_size
            && platform::modified(&req.inventory.iso)? == req.inventory.iso_modified,
        "The source ISO changed since scanning. Scan it again before replacing the output"
    );
    let candidate = crate::cleanup::output_candidate(&req.output, &req.inventory.iso)?;
    ensure!(
        candidate.id.is_some(),
        "{}",
        candidate.reason.unwrap_or_default()
    );
    validate_paths_with_credit(req, Some(&crate::cleanup::fingerprint(&req.output)?))
}

pub fn increment_output(path: &Path) -> Result<PathBuf> {
    let stem = path
        .file_stem()
        .context("Output filename is missing")?
        .to_string_lossy();
    let (base, start) = stem
        .rsplit_once(" (")
        .and_then(|(base, suffix)| {
            suffix
                .strip_suffix(')')
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|n| *n >= 1)
                .map(|n| (base, n.saturating_add(1)))
        })
        .unwrap_or((&stem, 2));
    for n in start..u64::MAX {
        let next = path.with_file_name(format!("{base} ({n}).iso"));
        match fs::symlink_metadata(&next) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(next),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
    }
    anyhow::bail!("No unused ISO filename is available")
}

/// A manual retry must not overlap a worker surviving a connection failure.
pub fn workspace_available(workspace: &Path) -> Result<()> {
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(workspace.join(".build.lock"))
    {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("Cannot check the retained workspace build lock"),
    };
    fs2::FileExt::try_lock_exclusive(&lock).context(
        "A build worker is still using this workspace. Wait for it to finish before retrying",
    )?;
    Ok(())
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
    emit(Event::BuildFiles(BuildFiles {
        job: job.clone(),
        log: job.join("build.log"),
    }));
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
    // Preparation and the authenticated administrator handoff are complete.
    let mut steps = crate::progress::BuildSteps::new(req.selected.len(), &req.model, 2, emit);
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
    ensure!(
        req.inventory.setup_image.is_none() || fresh.setup_image == req.inventory.setup_image,
        "Windows Setup metadata changed. Scan again"
    );
    steps.advance(emit);
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
    steps.advance(emit);
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
        steps.advance(emit);
    }
    drop(source);
    stage(
        emit,
        "Staging Apple drivers",
        "Copying Boot Camp drivers and Windows setup configuration",
    );
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
    if crate::setup_recipe::SetupRecipe::for_model(&req.model).is_some() {
        crate::diagnostics::write_support(&root.join("Support"), req, job)?;
    }
    steps.advance(emit);
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
            let inf = crate::amd::package(&assets.drivers.join("AMDGraphics"))?;
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
            crate::amd::store(&mount.mount, &inf)?;
        }
        copy_tree(&assets.drivers, &mount.mount.join("Drivers"), cancel)?;
        copy_tree(&assets.bootcamp, &mount.mount.join("BootCamp"), cancel)?;
        if crate::setup_recipe::SetupRecipe::for_model(&req.model).is_some() {
            copy_tree(
                &root.join("Support"),
                &mount.mount.join("MacBookPatcher/Support"),
                cancel,
            )?;
        }
        fs::create_dir_all(mount.mount.join("Windows/Setup/Scripts"))?;
        fs::write(
            mount.mount.join("Windows/Setup/Scripts/SetupComplete.cmd"),
            include_bytes!("../payloads/a1706/SetupComplete.cmd"),
        )?;
        if let Some(intel) = &assets.intel {
            validate_signed_store(&mount.mount, intel, &req.model)?;
        }
        if n == 0 && crate::setup_recipe::SetupRecipe::for_model(&req.model).is_some() {
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
        steps.advance(emit);
    }
    if crate::setup_recipe::SetupRecipe::for_model(&req.model).is_some() {
        let graphics = if req.model == "a1707-2016" {
            assets.drivers.join("AMDGraphics")
        } else {
            assets
                .intel
                .as_ref()
                .context("Intel graphics missing")?
                .clone()
        };
        stage(
            emit,
            "Patching Windows Setup",
            "Signed graphics driver and model-specific external-display startup",
        );
        let boot = root.join("sources/boot.wim");
        let mount = ImageMount::open(&boot, 2, &job.join("mount-boot"), emit, cancel)?;
        if req.model == "a1707-2016" {
            let inf = crate::amd::package(&graphics)?;
            dism(
                vec![
                    format!("/Image:{}", mount.mount.display()),
                    "/Add-Driver".into(),
                    format!("/Driver:{}", inf.display()),
                ],
                emit,
                cancel,
            )?;
            crate::amd::store(&mount.mount, &inf)?;
        } else {
            inject_signed_driver(&mount.mount, &graphics, &req.model, emit, cancel)?;
        }
        write_boot_payload_for_model(&mount.mount, &root.join("Support"), &req.model)?;
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
        prepare_setup_boot_graphics(&mount.mount, &graphics, &req.model, emit)?;
        mount.commit(cancel)?;
        steps.advance(emit);
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
    fs::write(
        root.join(
            if crate::setup_recipe::SetupRecipe::for_model(&req.model).is_some() {
                "README-A1706.txt"
            } else {
                "README-MacBook.txt"
            },
        ),
        format!(
            "Model: {}\r\n\r\n{}",
            req.model,
            crate::completion::installation_guide(&req.model)
        ),
    )?;
    // Keep the temporary ISO on the output filesystem for an atomic, no-clobber publication.
    steps.advance(emit);
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
    steps.advance(emit);
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
    steps.advance(emit);
    emit(Event::Log(format!(
        "Completed. Build log: {}",
        job.join("build.log").display()
    )));
    // Large staging files belong to this UUID job only. Retain job.json and build.log.
    stage(
        emit,
        "Finishing build",
        "Cleaning temporary media and saving the build result",
    );
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
    if model == "a1706-2017" {
        for file in intel_2017_package_files(&inf)? {
            ensure!(
                dir.join(&file).is_file(),
                "2017 Intel package file missing: {}",
                file.display()
            );
        }
    }
    if cfg!(windows) {
        platform::powershell(
            r#"$s=Get-AuthenticodeSignature -LiteralPath $env:PATCHER_CAT; if($s.Status -ne 'Valid'){throw 'Intel driver catalog signature is invalid'}"#,
            &[("PATCHER_CAT", dir.join(profile.catalog).as_os_str())],
        )?;
    }
    Ok(())
}
// The pinned 2017 INF uses a single SourceDisksFiles section, including one
// relative oprom subdirectory. Require every declared file before servicing.
fn intel_2017_package_files(inf: &str) -> Result<Vec<PathBuf>> {
    let mut active = false;
    let mut files = vec![PathBuf::from("iigd_dch.inf"), PathBuf::from("igdlh.cat")];
    for line in inf.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.starts_with('[') {
            active = line.eq_ignore_ascii_case("[SourceDisksFiles]");
            continue;
        }
        if !active || line.is_empty() {
            continue;
        }
        let (name, location) = line
            .split_once('=')
            .context("Invalid 2017 package file declaration")?;
        let name = name.trim();
        ensure!(
            !name.is_empty() && !name.contains(['/', '\\', ':']),
            "Invalid 2017 package filename"
        );
        let folder = location
            .split(',')
            .nth(1)
            .unwrap_or("")
            .trim()
            .trim_start_matches('\\');
        ensure!(
            !folder.contains(['/', '\\', ':']) && folder != ".." && folder != ".",
            "Invalid 2017 package subdirectory"
        );
        files.push(Path::new(folder).join(name));
    }
    ensure!(files.len() > 2, "2017 Intel package file list is missing");
    Ok(files)
}
// SourceDisksFiles describes the download media, including unused firmware
// tools. The DriverStore must contain the files copied by this GPU's install
// section, rather than every source-media entry.
fn intel_2017_store_files(inf: &str) -> Result<Vec<PathBuf>> {
    use std::collections::{BTreeMap, BTreeSet};
    let sources = intel_2017_package_files(inf)?;
    let mut sections: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut current = String::new();
    let mut install = BTreeSet::new();
    for line in inf.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            current = line[1..line.len() - 1].to_ascii_lowercase();
        } else if !line.is_empty() {
            sections.entry(current.clone()).or_default().push(line);
            if let Some((key, value)) = line.split_once('=') {
                let fields: Vec<_> = value.split(',').map(str::trim).collect();
                if key.trim().starts_with('%')
                    && fields
                        .iter()
                        .skip(1)
                        .any(|id| id.eq_ignore_ascii_case(r"PCI\VEN_8086&DEV_5927"))
                {
                    install.insert(fields[0].to_ascii_lowercase());
                }
            }
        }
    }
    ensure!(
        install.len() == 1,
        "2017 Intel install section is missing or ambiguous"
    );
    let install = sections
        .get(install.first().unwrap())
        .context("2017 Intel install section missing")?;
    let mut names = BTreeSet::new();
    for line in install {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        ensure!(
            !key.trim().eq_ignore_ascii_case("Needs")
                && !key.trim().eq_ignore_ascii_case("Include"),
            "2017 Intel install dependencies changed"
        );
        if !key.trim().eq_ignore_ascii_case("CopyFiles") {
            continue;
        }
        for reference in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(name) = reference.strip_prefix('@') {
                names.insert(name.to_ascii_lowercase());
                continue;
            }
            let entries = sections
                .get(&reference.to_ascii_lowercase())
                .with_context(|| format!("2017 Intel CopyFiles section missing: {reference}"))?;
            for entry in entries {
                let fields: Vec<_> = entry.split(',').map(str::trim).collect();
                let source = fields
                    .get(1)
                    .filter(|s| !s.is_empty())
                    .copied()
                    .unwrap_or(fields[0]);
                names.insert(source.trim_matches('"').to_ascii_lowercase());
            }
        }
    }
    ensure!(
        names.contains("igdkmd64.sys"),
        "2017 Intel CopyFiles is missing its graphics kernel"
    );
    let mut required = vec![PathBuf::from("iigd_dch.inf"), PathBuf::from("igdlh.cat")];
    for name in names {
        let file = sources
            .iter()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(&name)))
            .with_context(|| format!("2017 Intel copied file has no source declaration: {name}"))?;
        required.push(file.clone());
    }
    Ok(required)
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
    let found = found.context("Signed Intel package is missing from the driver store")?;
    if model == "a1706-2017" {
        for file in intel_2017_store_files(&text_file(&intel.join(profile.inf))?)? {
            ensure!(
                found.join(&file).is_file(),
                "2017 driver store file missing: {}",
                file.display()
            );
        }
        for entry in WalkDir::new(&found) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let relative = entry.path().strip_prefix(&found)?;
                ensure!(
                    fs::read(entry.path())?
                        == fs::read(intel.join(relative)).with_context(|| format!(
                            "2017 signed package file missing: {}",
                            relative.display()
                        ))?,
                    "2017 driver store differs from the pinned package: {}",
                    relative.display()
                );
            }
        }
    }
    Ok(found)
}
pub fn validate_boot_drivers(root: &Path) -> Result<()> {
    validate_model_boot_drivers(root, MODEL_ID)
}

pub fn validate_model_boot_drivers(root: &Path, model: &str) -> Result<()> {
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
    if matches!(model, "a1706-2017" | "a1708-2016" | "a1707-2016") {
        ensure!(
            root.join("AppleSSD64").is_dir()
                && WalkDir::new(root.join("AppleSSD64"))
                    .into_iter()
                    .filter_map(Result::ok)
                    .any(|e| e.file_type().is_file()
                        && e.path()
                            .extension()
                            .is_some_and(|x| x.eq_ignore_ascii_case("inf"))),
            "Required Apple storage boot driver is missing"
        );
    }
    Ok(())
}
fn prepare_setup_boot_graphics(mount: &Path, intel: &Path, model: &str, emit: &Emit) -> Result<()> {
    if model == MODEL_ID {
        return prepare_a1706_boot_graphics(mount, intel, emit);
    }
    if model == "a1708-2016" {
        let store = validate_signed_store(mount, intel, model)?;
        let binary = fs::read(store.join("igdkmd64.sys"))?;
        ensure!(
            !binary.is_empty() && binary == fs::read(intel.join("igdkmd64.sys"))?,
            "Iris 540 kernel differs from pinned package"
        );
        fs::write(mount.join("Windows/System32/drivers/igdkmd64.sys"), &binary)?;
        for line in a1706_boot_registry(mount, false)?.lines() {
            emit(Event::Log(line.into()));
        }
        return Ok(());
    }
    if model == "a1707-2016" {
        return prepare_amd_boot_graphics(mount, intel, emit, false);
    }
    ensure!(
        model == "a1706-2017",
        "No Setup graphics recipe for this model"
    );
    let store = validate_signed_store(mount, intel, model)?;
    let kernel = store.join("igdkmd64.sys");
    ensure!(
        fs::read(&kernel)? == fs::read(intel.join("igdkmd64.sys"))?,
        "2017 Intel kernel differs from the signed package"
    );
    let inf = text_file(&intel.join("iigd_dch.inf"))?;
    ensure!(
        inf.contains("[igfx_Service_Inst_DS]")
            && inf.lines().any(|l| l
                .trim()
                .eq_ignore_ascii_case(r"ServiceBinary  = %13%\igdkmd64.sys")),
        "2017 Intel service location changed"
    );
    let relative = kernel.strip_prefix(mount)?;
    let image_path = format!(
        r"\SystemRoot\{}",
        relative
            .strip_prefix("Windows")?
            .to_string_lossy()
            .replace('/', r"\")
    );
    emit(Event::Log(format!(
        "Preparing 2017 Iris Plus 650: demand-start igfx from {image_path}"
    )));
    for line in boot_registry_path(mount, false, &image_path)?.lines() {
        emit(Event::Log(line.to_owned()));
    }
    ensure!(
        fs::read(&kernel)? == fs::read(intel.join("igdkmd64.sys"))?,
        "2017 graphics kernel changed during preparation"
    );
    Ok(())
}
fn prepare_amd_boot_graphics(
    mount: &Path,
    graphics: &Path,
    emit: &Emit,
    validate_only: bool,
) -> Result<()> {
    let inf = crate::amd::package(graphics)?;
    let store = crate::amd::store(mount, &inf)?;
    let paths = crate::amd::kernels(&store)?.map(|kernel| -> Result<String> {
        Ok(format!(
            r"\SystemRoot\{}",
            kernel
                .strip_prefix(mount.join("Windows"))?
                .to_string_lossy()
                .replace('/', r"\")
        ))
    });
    let [proxy, kernel] = paths;
    let proxy = proxy?;
    let kernel = kernel?;
    let script = format!(
        "{}\n{}\n{}",
        include_str!("../payloads/a1706/boot-graphics.ps1"),
        include_str!("../payloads/a1706/boot-radeon.ps1"),
        r#"
        $name='MacBookRadeon_'+[Guid]::NewGuid().ToString('N')
        $reg="$env:SystemRoot\System32\reg.exe"
        & $reg load "HKLM\$name" "$env:PATCHER_MOUNT\Windows\System32\config\SYSTEM" | Out-Null
        if($LASTEXITCODE -ne 0){throw "Cannot load offline Radeon SYSTEM hive (code $LASTEXITCODE)"}
        $root=$null
        try {
            $root=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($name,$true)
            if($null -eq $root){throw 'Cannot open offline Radeon SYSTEM hive'}
            if($env:PATCHER_RADEON_VALIDATE_ONLY -ne '1'){Set-RadeonBootGraphics $root $env:PATCHER_PROXY $env:PATCHER_KERNEL | Out-Null}
            Set-RadeonBootGraphics $root $env:PATCHER_PROXY $env:PATCHER_KERNEL -ValidateOnly
        } finally {
            if($null -ne $root){$root.Dispose()}
            & $reg unload "HKLM\$name" | Out-Null
            if($LASTEXITCODE -ne 0){throw "Cannot unload offline Radeon SYSTEM hive; workspace retained"}
        }
    "#
    );
    for line in platform::powershell(
        &script,
        &[
            ("PATCHER_MOUNT", mount.as_os_str()),
            (
                "PATCHER_RADEON_VALIDATE_ONLY",
                std::ffi::OsStr::new(if validate_only { "1" } else { "0" }),
            ),
            ("PATCHER_PROXY", std::ffi::OsStr::new(&proxy)),
            ("PATCHER_KERNEL", std::ffi::OsStr::new(&kernel)),
        ],
    )?
    .lines()
    {
        emit(Event::Log(line.to_owned()));
    }
    crate::amd::store(mount, &inf)?;
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
    boot_registry_path(
        mount,
        validate_only,
        r"\SystemRoot\System32\drivers\igdkmd64.sys",
    )
}
fn boot_registry_path(mount: &Path, validate_only: bool, image_path: &str) -> Result<String> {
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
          if($env:PATCHER_GRAPHICS_VALIDATE_ONLY -ne '1'){Set-A1706BootGraphics $root -ServiceImagePath $env:PATCHER_GRAPHICS_IMAGE_PATH | Out-Null}
          Set-A1706BootGraphics $root -ValidateOnly -ServiceImagePath $env:PATCHER_GRAPHICS_IMAGE_PATH
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
                "PATCHER_GRAPHICS_IMAGE_PATH",
                std::ffi::OsStr::new(image_path),
            ),
            (
                "PATCHER_GRAPHICS_VALIDATE_ONLY",
                std::ffi::OsStr::new(if validate_only { "1" } else { "0" }),
            ),
        ],
    )
    .context("A1706 Windows Setup graphics preparation failed")
}
#[cfg(test)]
fn write_boot_payload(mount: &Path, support: &Path) -> Result<()> {
    write_boot_payload_for_model(mount, support, MODEL_ID)
}
fn write_boot_payload_for_model(mount: &Path, support: &Path, model: &str) -> Result<()> {
    let recipe = crate::setup_recipe::SetupRecipe::for_model(model)
        .context("No Setup startup recipe for this model")?;
    let dir = mount.join("Windows/System32");
    fs::copy(
        support.join("build-manifest.json"),
        dir.join("build-manifest.json"),
    )?;
    let script = recipe.startup();
    let script = script.as_bytes();
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
                "MacBookSetupLogger.exe",
                include_bytes!(concat!(env!("OUT_DIR"), "/MacBookSetupLogger.exe")).as_slice(),
            ),
            (
                "a1706iris.exe",
                include_bytes!(concat!(env!("OUT_DIR"), "/a1706iris.exe")).as_slice(),
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
            if recipe == crate::setup_recipe::SetupRecipe::A1706_2016
                || !matches!(name, "a1706iris.exe" | "a1706gpu.exe")
            {
                fs::write(dir.join(name), bytes)?;
            }
        }
        if matches!(
            recipe,
            crate::setup_recipe::SetupRecipe::A1708_2016
                | crate::setup_recipe::SetupRecipe::A1707_2016
        ) {
            fs::write(
                dir.join("MacBookSetupGPU.exe"),
                include_bytes!(concat!(env!("OUT_DIR"), "/MacBookSetupGPU.exe")),
            )?;
        }
        if recipe == crate::setup_recipe::SetupRecipe::A1706_2017 {
            fs::write(
                dir.join("a1706gpu2017.exe"),
                include_bytes!(concat!(env!("OUT_DIR"), "/a1706gpu2017.exe")),
            )?;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intel_2017_manifest_requires_declared_files_and_rejects_path_escapes() {
        let inf = "[SourceDisksFiles]\nigdkmd64.sys=1\ndg1_oprom.rom=1,\\oprom\n[Other]\nignored=1";
        let files = intel_2017_package_files(inf).unwrap();
        assert!(files.contains(&PathBuf::from("igdkmd64.sys")));
        assert!(files.contains(&Path::new("oprom").join("dg1_oprom.rom")));
        assert_eq!(files.len(), 4);
        for declaration in ["../bad=1", "bad=1,\\..", "bad=1,C:", "bad=1,\\one\\two"] {
            assert!(
                intel_2017_package_files(&format!("[SourceDisksFiles]\n{declaration}")).is_err()
            );
        }
        assert!(intel_2017_package_files("[Other]\na=1").is_err());
    }
    #[test]
    fn intel_2017_store_manifest_follows_selected_copyfiles_and_fails_closed() {
        let inf = "%GPU%=Iris,PCI\\VEN_8086&DEV_5927\n%Other%=OtherGPU,PCI\\VEN_8086&DEV_1234\n[Iris]\nCopyFiles=Kernel,, @direct.dll, Nested\n[Kernel]\nigdkmd64.sys\n[Nested]\nrenamed.rom,source.rom\n[OtherGPU]\nCopyFiles=Unused\n[Unused]\nigsc.dll\n[SourceDisksFiles]\nigdkmd64.sys=1\ndirect.dll=1\nsource.rom=1,\\oprom\nigsc.dll=1";
        let files = intel_2017_store_files(inf).unwrap();
        assert!(files.contains(&PathBuf::from("igdkmd64.sys")));
        assert!(files.contains(&PathBuf::from("direct.dll")));
        assert!(files.contains(&Path::new("oprom").join("source.rom")));
        assert!(!files.contains(&PathBuf::from("igsc.dll")));
        for broken in [
            inf.replace("CopyFiles=Kernel", "CopyFiles=Absent"),
            inf.replace("igdkmd64.sys=1", "unknown.sys=1"),
            inf.replace("[Iris]", "[Missing]"),
            inf.replace("[Iris]", "[Iris]\nNeeds=External"),
            inf.replace("[Iris]", "[Iris]\nInclude=External.inf"),
            inf.replace("[Iris]", "%GPU2%=OtherGPU,PCI\\VEN_8086&DEV_5927\n[Iris]"),
        ] {
            assert!(intel_2017_store_files(&broken).is_err());
        }
    }
    #[test]
    fn intel_2017_store_rejects_missing_changed_and_extra_files() {
        let root = std::env::temp_dir().join(format!("store2017-{}", uuid::Uuid::new_v4()));
        let intel = root.join("intel");
        let mount = root.join("image");
        let store =
            mount.join("Windows/System32/DriverStore/FileRepository/iigd_dch.inf_amd64_fixture");
        fs::create_dir_all(&intel).unwrap();
        fs::create_dir_all(&store).unwrap();
        for (name, bytes) in [
            (
                "iigd_dch.inf",
                "CatalogFile=igdlh.cat\nDriverVer=02/18/2021,27.20.100.9316\n%GPU%=install,PCI\\VEN_8086&DEV_5927\n[install]\nCopyFiles=KernelFiles\n[KernelFiles]\nigdkmd64.sys\n[UnusedFirmware]\nigsc.dll\n[SourceDisksFiles]\nigdkmd64.sys=1\nigsc.dll=1",
            ),
            ("igdlh.cat", "fixture catalog"),
            ("igdkmd64.sys", "fixture kernel"),
        ] {
            fs::write(intel.join(name), bytes).unwrap();
            fs::write(store.join(name), bytes).unwrap();
        }
        fs::write(intel.join("igsc.dll"), "unused firmware tool").unwrap();
        // This valid source-only file is deliberately absent from the store.
        assert!(validate_signed_store(&mount, &intel, "a1706-2017").is_ok());
        fs::remove_file(store.join("igdkmd64.sys")).unwrap();
        assert!(validate_signed_store(&mount, &intel, "a1706-2017").is_err());
        fs::write(store.join("igdkmd64.sys"), "changed kernel").unwrap();
        assert!(validate_signed_store(&mount, &intel, "a1706-2017").is_err());
        fs::copy(intel.join("igdkmd64.sys"), store.join("igdkmd64.sys")).unwrap();
        fs::write(store.join("unrecognized.dll"), "extra").unwrap();
        assert!(validate_signed_store(&mount, &intel, "a1706-2017").is_err());
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn incremental_filenames_skip_existing_names_and_continue_suffixes() {
        let root =
            std::env::temp_dir().join(format!("patcher-output-name-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("Windows.iso");
        fs::write(&first, b"original").unwrap();
        fs::write(root.join("Windows (2).iso"), b"existing").unwrap();
        assert_eq!(
            increment_output(&first).unwrap(),
            root.join("Windows (3).iso")
        );
        assert_eq!(
            increment_output(&root.join("Windows (1).iso")).unwrap(),
            root.join("Windows (3).iso")
        );
        assert_eq!(
            increment_output(&root.join("Windows (7).iso")).unwrap(),
            root.join("Windows (8).iso")
        );
        assert_eq!(
            increment_output(&root.join("Windows (edition).iso")).unwrap(),
            root.join("Windows (edition) (2).iso")
        );
        fs::remove_file(&first).unwrap();
        fs::remove_file(root.join("Windows (2).iso")).unwrap();
        fs::remove_dir(&root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn a1706_boot_payload_packages_native_diagnostics_and_manifest() {
        let root =
            std::env::temp_dir().join(format!("macbook-diagnostics-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("Windows/System32")).unwrap();
        fs::create_dir(root.join("Support")).unwrap();
        fs::write(
            root.join("Support/build-manifest.json"),
            br#"{"schema_version":1,"model_profile":"a1706"}"#,
        )
        .unwrap();
        write_boot_payload(&root, &root.join("Support")).unwrap();
        for name in [
            "MacBookSetupLogger.exe",
            "a1706gpu.exe",
            "a1706iris.exe",
            "a1706cue.exe",
        ] {
            assert!(
                fs::read(root.join("Windows/System32").join(name))
                    .unwrap()
                    .starts_with(b"MZ")
            );
        }
        assert!(
            fs::read_to_string(root.join("Windows/System32/build-manifest.json"))
                .unwrap()
                .contains("a1706")
        );
        assert!(!root.join("Windows/System32/a1706dlg.exe").exists());
        let script = fs::read(root.join("Windows/System32/a1706.cmd")).unwrap();
        assert!(script.is_ascii() && !script.contains(&0));
        assert!(root.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn setup_2017_payload_uses_its_own_helper_and_profile() {
        let root = std::env::temp_dir().join(format!("setup2017-payload-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("Windows/System32")).unwrap();
        fs::create_dir(root.join("Support")).unwrap();
        fs::write(
            root.join("Support/build-manifest.json"),
            br#"{"model_profile":"a1706-2017"}"#,
        )
        .unwrap();
        write_boot_payload_for_model(&root, &root.join("Support"), "a1706-2017").unwrap();
        let script = fs::read_to_string(root.join("Windows/System32/a1706.cmd")).unwrap();
        assert!(script.contains("a1706gpu2017.exe") && script.contains("--profile a1706-2017"));
        assert!(!script.contains("a1706iris.exe"));
        assert!(root.join("Windows/System32/a1706gpu2017.exe").is_file());
        assert!(!root.join("Windows/System32/a1706gpu.exe").exists());
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn setup_2017_requires_storage_inf_and_rejects_2016_graphics() {
        let root = std::env::temp_dir().join(format!("setup2017-storage-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("AppleSSD64")).unwrap();
        assert!(validate_model_boot_drivers(&root, "a1706-2017").is_err());
        fs::write(root.join("AppleSSD64/AppleSSD.inf"), b"fixture").unwrap();
        assert!(validate_model_boot_drivers(&root, "a1706-2017").is_ok());
        fs::create_dir(root.join("IntelHDGraphics64")).unwrap();
        assert!(validate_model_boot_drivers(&root, "a1706-2017").is_err());
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        let intel = root.join("intel");
        fs::create_dir(&intel).unwrap();
        fs::write(intel.join("iigd_dch.inf"),b"[Version]\nDriverVer=11/08/2020,21.20.16.5174\nCatalogFile=igdlh.cat\n%gpu%=install,pci\\ven_8086&dev_1927").unwrap();
        assert!(
            validate_intel_for_model(&intel, "a1706-2017")
                .unwrap_err()
                .to_string()
                .contains("Unexpected Intel driver version")
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "Administrator test on a disposable install.wim copy; explicit fixture paths required"]
    fn intel_2017_disposable_install_image_driver_store() {
        assert!(platform::is_admin());
        let source = PathBuf::from(
            std::env::var_os("MACBOOK_SETUP_TEST_INSTALL_WIM").expect("Stock install.wim"),
        );
        let archive =
            PathBuf::from(std::env::var_os("MACBOOK_SETUP_TEST_INTEL_CAB").expect("Pinned CAB"));
        let root = PathBuf::from(
            std::env::var_os("MACBOOK_SETUP_TEST_ROOT").expect("New disposable directory"),
        );
        assert!(
            !root.exists()
                && root.starts_with(Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache"))
                && root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("is17-")
        );
        fs::create_dir(&root).unwrap();
        let emit: Emit = Arc::new(|e| println!("{e:?}"));
        let cancel = Cancellation::default();
        download::verify(
            &archive,
            crate::driver_prefetch::packages_for_model("a1706-2017").unwrap()[0].hash,
            &cancel,
        )
        .unwrap();
        let intel = root.join("intel");
        platform::run(
            Path::new("C:/Program Files/7-Zip/7z.exe"),
            &[
                "x".into(),
                "-y".into(),
                archive.display().to_string(),
                format!("-o{}", intel.display()),
            ],
            &emit,
            &cancel,
            false,
        )
        .unwrap();
        validate_intel_for_model(&intel, "a1706-2017").unwrap();
        let copied = root.join("install.wim");
        copy_file(&source, &copied, &cancel).unwrap();
        let mounted = ImageMount::open(&copied, 1, &root.join("mount"), &emit, &cancel).unwrap();
        inject_signed_driver(&mounted.mount, &intel, "a1706-2017", &emit, &cancel).unwrap();
        let store = validate_signed_store(&mounted.mount, &intel, "a1706-2017").unwrap();
        println!(
            "Production DriverStore validation passed; igsc.dll present={}",
            store.join("igsc.dll").exists()
        );
        // Discard this test mount; no test image or ISO is published.
        drop(mounted);
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "Administrator test on a disposable stock boot.wim copy; explicit fixture paths required"]
    fn a1706_2017_boot_recipe_persists_driverstore_service_and_payload() {
        assert!(platform::is_admin(), "Administrator access required");
        let source = PathBuf::from(
            std::env::var_os("MACBOOK_SETUP_TEST_BOOT_WIM").expect("Stock boot.wim path"),
        );
        let archive = PathBuf::from(
            std::env::var_os("MACBOOK_SETUP_TEST_INTEL_CAB").expect("Pinned CAB path"),
        );
        let root = PathBuf::from(
            std::env::var_os("MACBOOK_SETUP_TEST_ROOT").expect("New disposable directory"),
        );
        assert!(!root.exists() && root.is_absolute());
        assert!(
            root.starts_with(Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache"))
                && root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("s17-"),
            "Disposable integration root must be a new app-owned cache fixture"
        );
        fs::create_dir_all(&root).unwrap();
        let emit: Emit = Arc::new(|e| println!("{e:?}"));
        let cancel = Cancellation::default();
        let original = download::sha256(&source, &cancel).unwrap();
        download::verify(
            &archive,
            crate::driver_prefetch::packages_for_model("a1706-2017").unwrap()[0].hash,
            &cancel,
        )
        .unwrap();
        let intel = root.join("intel");
        platform::run(
            Path::new("C:/Program Files/7-Zip/7z.exe"),
            &[
                "x".into(),
                "-y".into(),
                archive.display().to_string(),
                format!("-o{}", intel.display()),
            ],
            &emit,
            &cancel,
            false,
        )
        .unwrap();
        let intel = download::find_file(&intel, "iigd_dch.inf")
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        validate_intel_for_model(&intel, "a1706-2017").unwrap();
        let required =
            intel_2017_store_files(&text_file(&intel.join("iigd_dch.inf")).unwrap()).unwrap();
        assert!(!required.contains(&PathBuf::from("igsc.dll")));
        assert!(!required.contains(&PathBuf::from("OpromUpdateTool.exe")));
        println!(
            "2017 selected CopyFiles manifest: {} required files; unused firmware excluded",
            required.len()
        );
        let copied = root.join("boot.wim");
        copy_file(&source, &copied, &cancel).unwrap();
        let mount = root.join("mount-boot");
        let mounted = ImageMount::open(&copied, 2, &mount, &emit, &cancel).unwrap();
        inject_signed_driver(&mounted.mount, &intel, "a1706-2017", &emit, &cancel).unwrap();
        prepare_setup_boot_graphics(&mounted.mount, &intel, "a1706-2017", &emit).unwrap();
        fs::create_dir(root.join("Support")).unwrap();
        fs::write(
            root.join("Support/build-manifest.json"),
            br#"{"model_profile":"a1706-2017"}"#,
        )
        .unwrap();
        write_boot_payload_for_model(&mounted.mount, &root.join("Support"), "a1706-2017").unwrap();
        mounted.commit(&cancel).unwrap();
        let mounted = ImageMount::open(&copied, 2, &mount, &emit, &cancel).unwrap();
        let store = validate_signed_store(&mounted.mount, &intel, "a1706-2017").unwrap();
        let relative = store
            .join("igdkmd64.sys")
            .strip_prefix(&mounted.mount)
            .unwrap()
            .strip_prefix("Windows")
            .unwrap()
            .to_owned();
        let expected = format!(
            r"\SystemRoot\{}",
            relative.to_string_lossy().replace('/', r"\")
        );
        assert!(
            boot_registry_path(&mounted.mount, true, &expected)
                .unwrap()
                .contains("igfx Start=3")
        );
        assert!(
            mounted
                .mount
                .join("Windows/System32/a1706gpu2017.exe")
                .is_file()
        );
        prepare_setup_boot_graphics(&mounted.mount, &intel, "a1706-2017", &emit).unwrap();
        mounted.commit(&cancel).unwrap();
        assert_eq!(original, download::sha256(&source, &cancel).unwrap());
        println!(
            "2017 disposable image verification passed: {}",
            root.display()
        );
    }
    #[test]
    fn retry_preflight_refuses_a_surviving_worker_lock() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!("retry-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        assert!(workspace_available(&root).is_ok());
        let lock = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(root.join(".build.lock"))
            .unwrap();
        fs2::FileExt::try_lock_exclusive(&lock).unwrap();
        assert!(
            workspace_available(&root)
                .unwrap_err()
                .to_string()
                .contains("worker is still using")
        );
        drop(lock);
        assert!(workspace_available(&root).is_ok());
        assert_eq!(root.parent(), Some(temp.as_path()));
        fs::remove_file(root.join(".build.lock")).unwrap();
        fs::remove_dir(root).unwrap();
    }

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
    #[cfg(windows)]
    #[ignore = "Administrator disposable boot/index2 and install/index1 servicing; explicit pinned fixtures required"]
    fn new_2016_profiles_disposable_images() {
        assert!(platform::is_admin(), "Administrator access required");
        let root =
            PathBuf::from(std::env::var_os("MACBOOK_2016_TEST_ROOT").expect("New fixture root"));
        assert!(
            !root.exists()
                && root.starts_with(Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache"))
                && root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("r16-")
        );
        fs::create_dir(&root).unwrap();
        let boot = PathBuf::from(std::env::var_os("MACBOOK_2016_TEST_BOOT").unwrap());
        let install = PathBuf::from(std::env::var_os("MACBOOK_2016_TEST_INSTALL").unwrap());
        let intel = PathBuf::from(std::env::var_os("MACBOOK_2016_TEST_INTEL").unwrap());
        let amd = PathBuf::from(std::env::var_os("MACBOOK_2016_TEST_AMD").unwrap());
        let cancel = Cancellation::default();
        let emit: Emit = Arc::new(|e| println!("{e:?}"));
        println!("Hashing read-only source fixtures");
        let source_hashes = || {
            platform::powershell("(Get-FileHash -Algorithm SHA256 -LiteralPath $env:PATCHER_BOOT).Hash; (Get-FileHash -Algorithm SHA256 -LiteralPath $env:PATCHER_INSTALL).Hash", &[("PATCHER_BOOT",boot.as_os_str()),("PATCHER_INSTALL",install.as_os_str())]).unwrap()
        };
        let original_hashes = source_hashes();
        println!("Validating pinned source packages");
        let inf = crate::amd::package(&amd).unwrap();
        validate_intel_for_model(&intel, "a1708-2016").unwrap();
        let mut failures = vec![];
        for model in ["a1708-2016", "a1707-2016"] {
            for (label, source, index) in [("boot", &boot, 2), ("install", &install, 1)] {
                println!("Servicing {model} {label}");
                let result = (|| -> Result<()> {
                    let job = root.join(format!("{model}-{label}"));
                    fs::create_dir(&job)?;
                    let image = job.join("image.wim");
                    copy_file(source, &image, &cancel)?;
                    let path = job.join("mount");
                    let mounted = ImageMount::open(&image, index, &path, &emit, &cancel)?;
                    if model == "a1707-2016" {
                        dism(
                            vec![
                                format!("/Image:{}", path.display()),
                                "/Add-Driver".into(),
                                format!("/Driver:{}", inf.display()),
                            ],
                            &emit,
                            &cancel,
                        )?;
                        crate::amd::store(&path, &inf)?;
                    } else {
                        inject_signed_driver(&path, &intel, model, &emit, &cancel)?;
                        validate_signed_store(&path, &intel, model)?;
                    }
                    if label == "boot" {
                        prepare_setup_boot_graphics(
                            &path,
                            if model == "a1707-2016" { &amd } else { &intel },
                            model,
                            &emit,
                        )?;
                        let support = job.join("Support");
                        fs::create_dir(&support)?;
                        fs::write(support.join("build-manifest.json"), "{}")?;
                        write_boot_payload_for_model(&path, &support, model)?;
                    }
                    mounted.commit(&cancel)?;
                    let mounted = ImageMount::open(&image, index, &path, &emit, &cancel)?;
                    if model == "a1707-2016" {
                        crate::amd::store(&path, &inf)?;
                        if label == "boot" {
                            prepare_amd_boot_graphics(&path, &amd, &emit, true)?;
                        }
                    } else {
                        validate_signed_store(&path, &intel, model)?;
                        if label == "boot" {
                            a1706_boot_registry(&path, true)?;
                        }
                    }
                    drop(mounted);
                    println!("PASS: {model} {label} committed and reopened");
                    Ok(())
                })();
                if let Err(error) = result {
                    failures.push(format!("{model} {label}: {error:#}"));
                }
            }
        }
        assert_eq!(original_hashes, source_hashes());
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
    #[test]
    #[cfg(windows)]
    fn new_setup_profiles_package_only_the_selected_helper() {
        for model in ["a1708-2016", "a1707-2016"] {
            let root =
                std::env::temp_dir().join(format!("new-setup-payload-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("Windows/System32")).unwrap();
            fs::create_dir_all(root.join("Support")).unwrap();
            fs::write(root.join("Support/build-manifest.json"), "{}").unwrap();
            write_boot_payload_for_model(&root, &root.join("Support"), model).unwrap();
            let dir = root.join("Windows/System32");
            assert!(
                fs::read(dir.join("MacBookSetupGPU.exe"))
                    .unwrap()
                    .starts_with(b"MZ")
            );
            assert!(!dir.join("a1706gpu.exe").exists() && !dir.join("a1706gpu2017.exe").exists());
            assert!(
                fs::read_to_string(dir.join("a1706.cmd"))
                    .unwrap()
                    .contains(model)
            );
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn unverified_models_accept_available_boot_drivers_but_reject_graphics() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!("model-boot-drivers-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        assert!(validate_model_boot_drivers(&root, "a1707-2016").is_err());
        fs::create_dir(root.join("AppleSSD64")).unwrap();
        assert!(validate_model_boot_drivers(&root, "a1707-2016").is_err());
        fs::write(root.join("AppleSSD64/storage.inf"), "fixture").unwrap();
        assert!(validate_model_boot_drivers(&root, "a1707-2016").is_ok());
        assert!(validate_model_boot_drivers(&root, "a1708-2016").is_ok());
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
