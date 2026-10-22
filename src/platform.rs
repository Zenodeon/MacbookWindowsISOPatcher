use crate::{
    domain::{Cancellation, Emit, Event, Inventory},
    wim,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const ADK_URL: &str =
    "https://learn.microsoft.com/en-us/windows-hardware/get-started/adk-install";

/// Friendly text for file fields; canonical paths remain in the build inventory.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(network) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{network}");
    }
    if let Some(drive) = text.strip_prefix(r"\\?\") {
        let bytes = drive.as_bytes();
        if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1..3] == *b":\\" {
            return drive.to_owned();
        }
    }
    text.into_owned()
}

pub const SEVEN_ZIP_URL: &str = "https://www.7-zip.org/download.html";

pub fn hidden(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
pub fn powershell(script: &str, env: &[(&str, &OsStr)]) -> Result<String> {
    let mut cmd = Command::new(system_binary("WindowsPowerShell/v1.0/powershell.exe"));
    hidden(&mut cmd).env_remove("PSModulePath").args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!("$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); {script}"));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let output = cmd.output().context("Could not start Windows PowerShell")?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
pub fn system_binary(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("System32")
        .join(name)
}
pub fn find_seven_zip() -> PathBuf {
    for base in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(base) {
            let p = PathBuf::from(dir).join("7-Zip/7z.exe");
            if p.is_file() {
                return p;
            }
        }
    }
    PathBuf::new()
}
pub fn find_oscdimg() -> PathBuf {
    let dir =
        std::env::var_os("ProgramFiles(x86)").unwrap_or_else(|| "C:\\Program Files (x86)".into());
    let p = PathBuf::from(dir).join(
        "Windows Kits/10/Assessment and Deployment Kit/Deployment Tools/amd64/Oscdimg/oscdimg.exe",
    );
    if p.is_file() { p } else { PathBuf::new() }
}

pub struct MountedIso {
    pub root: PathBuf,
    iso: PathBuf,
    owned: bool,
}
impl MountedIso {
    /// Explicit user eject can also dismount a previously mounted source.
    pub fn dismount(&mut self) -> Result<bool> {
        let changed = dismount_iso(&self.iso)?;
        self.owned = false;
        Ok(changed)
    }
    /// Closing the app releases only a mount it created itself.
    pub fn cleanup(&mut self) -> Result<bool> {
        if self.owned {
            self.dismount()
        } else {
            Ok(false)
        }
    }
    pub fn open(iso: &Path) -> Result<Self> {
        ensure!(cfg!(windows), "ISO scanning currently requires Windows");
        let script = r#"
        $image=Get-DiskImage -ImagePath $env:PATCHER_ISO -ErrorAction SilentlyContinue
        $owned=(!$image -or !$image.Attached)
        if($owned){$image=Mount-DiskImage -ImagePath $env:PATCHER_ISO -StorageType ISO -Access ReadOnly -PassThru}
        $volume=$image | Get-Volume | Where-Object DriveLetter | Select-Object -First 1
        if(!$volume){if($owned){Dismount-DiskImage -ImagePath $env:PATCHER_ISO | Out-Null};throw 'ISO has no readable volume'}
        [pscustomobject]@{root=($volume.DriveLetter+':\');owned=$owned} | ConvertTo-Json -Compress
        "#;
        let output = powershell(script, &[("PATCHER_ISO", iso.as_os_str())])?;
        #[derive(serde::Deserialize)]
        struct Mount {
            root: PathBuf,
            owned: bool,
        }
        let mount: Mount = serde_json::from_str(&output).context("Cannot read ISO mount result")?;
        Ok(Self {
            root: mount.root,
            iso: iso.to_owned(),
            owned: mount.owned,
        })
    }
}
impl Drop for MountedIso {
    fn drop(&mut self) {
        if self.owned {
            let _ = powershell(
                "Dismount-DiskImage -ImagePath $env:PATCHER_ISO | Out-Null",
                &[("PATCHER_ISO", self.iso.as_os_str())],
            );
        }
    }
}

pub fn modified(path: &Path) -> Result<u64> {
    Ok(fs::metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs())
}

/// Dismount only the selected ISO, without servicing or discarding any WIM image.
pub fn dismount_iso(iso: &Path) -> Result<bool> {
    ensure!(cfg!(windows), "ISO dismounting currently requires Windows");
    let iso = iso
        .canonicalize()
        .context("Choose an existing ISO file to dismount")?;
    ensure!(
        iso.is_file()
            && iso
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("iso")),
        "Select a local .iso file"
    );
    let result = powershell(
        r#"
        $image=Get-DiskImage -ImagePath $env:PATCHER_ISO
        if($image.Attached) {
            Dismount-DiskImage -ImagePath $env:PATCHER_ISO | Out-Null
            if((Get-DiskImage -ImagePath $env:PATCHER_ISO).Attached){throw 'The ISO is still mounted. Close programs using it and retry.'}
            'dismounted'
        } else { 'already-dismounted' }
    "#,
        &[("PATCHER_ISO", iso.as_os_str())],
    )?;
    match result.trim() {
        "dismounted" => Ok(true),
        "already-dismounted" => Ok(false),
        _ => bail!("Could not confirm ISO dismount status"),
    }
}
pub fn scan(iso: &Path) -> Result<Inventory> {
    scan_mounted(iso).map(|(inventory, _mount)| inventory)
}

/// Keep the source mounted for the GUI session. Failed scans still drop their
/// mount guard; non-GUI scans can continue using `scan` for immediate cleanup.
pub fn scan_mounted(iso: &Path) -> Result<(Inventory, MountedIso)> {
    let iso = iso.canonicalize().context("Select an existing ISO file")?;
    ensure!(
        iso.extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("iso")),
        "Select a .iso file"
    );
    let mount = MountedIso::open(&iso)?;
    let inventory = scan_root(&iso, &mount.root)?;
    Ok((inventory, mount))
}
pub fn scan_root(iso: &Path, root: &Path) -> Result<Inventory> {
    ensure!(
        !root.join("$WinPEDriver$").exists(),
        "This ISO contains custom boot-path drivers. Select original Microsoft media"
    );
    ensure!(
        root.join("sources/boot.wim").is_file(),
        "The ISO does not contain Windows Setup (sources/boot.wim)"
    );
    ensure!(
        !root.join("autounattend.xml").exists() && !root.join("unattend.xml").exists(),
        "This ISO includes a custom answer file. Select original Microsoft Windows media"
    );
    let install_file = if root.join("sources/install.wim").is_file() {
        "install.wim"
    } else if root.join("sources/install.esd").is_file() {
        "install.esd"
    } else {
        bail!("No install.wim or install.esd found. Split SWM media is not supported");
    };
    let boot = wim::read_images(&root.join("sources/boot.wim"))?;
    let setup = boot
        .iter()
        .find(|i| i.index == 2)
        .context("Setup image index 2 is missing")?;
    ensure!(
        setup.arch == 9
            && setup.major == 10
            && (19041..=19045).contains(&setup.build)
            && setup.language.eq_ignore_ascii_case("en-US"),
        "Setup must be x64 en-US Windows 10 build 19041–19045"
    );
    Ok(Inventory {
        iso: iso.to_owned(),
        iso_size: fs::metadata(iso)?.len(),
        iso_modified: modified(iso)?,
        install_file: install_file.into(),
        setup_image: Some(setup.clone()),
        images: wim::read_images(&root.join("sources").join(install_file))?,
    })
}

/// Drain both pipes concurrently. A running servicing operation is never killed;
/// cancellation takes effect at its next safe boundary so DISM can close the image.
pub fn run(
    program: &Path,
    args: &[String],
    emit: &Emit,
    cancel: &Cancellation,
    cancellable: bool,
) -> Result<()> {
    cancel.check()?;
    emit(Event::Log(format!(
        "{} {}",
        program.display(),
        args.join(" ")
    )));
    let mut cmd = Command::new(program);
    hidden(&mut cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Cannot start {}", program.display()))?;
    let tool = match program
        .file_name()
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "dism.exe" => Some(crate::progress::Tool::Dism),
        "oscdimg.exe" => Some(crate::progress::Tool::Oscdimg),
        _ => None,
    };
    let operation = tool.map(|tool| tool.operation(args)).unwrap_or_default();
    if tool.is_some() {
        emit(Event::OperationProgress {
            operation: operation.clone(),
            percent: None,
        });
    }
    let mut readers = Vec::new();
    for pipe in [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let emit = emit.clone();
        let operation = operation.clone();
        readers.push(thread::spawn(move || {
            if let Err(error) = crate::progress::read_output(pipe, tool, &operation, &emit) {
                emit(Event::Log(format!(
                    "Could not read command output: {error}"
                )));
            }
        }));
    }
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancellable && cancel.is_cancelled() {
            let _ = child.kill();
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(100));
    };
    for reader in readers {
        let _ = reader.join();
    }
    if tool.is_some() {
        emit(Event::OperationProgress {
            operation: String::new(),
            percent: None,
        });
    }
    cancel.check()?;
    ensure!(
        status.success(),
        "{} failed with exit code {:?}. See the build log",
        program.display(),
        status.code()
    );
    Ok(())
}

pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn IsUserAnAdmin() -> i32;
        }
        unsafe { IsUserAnAdmin() != 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}
pub fn elevate_worker(port: u16, token: &str) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                op: *const u16,
                file: *const u16,
                params: *const u16,
                dir: *const u16,
                show: i32,
            ) -> isize;
        }
        fn wide(v: &OsStr) -> Vec<u16> {
            v.encode_wide().chain(Some(0)).collect()
        }
        let verb = wide(OsStr::new("runas"));
        let exe = wide(std::env::current_exe()?.as_os_str());
        let args = wide(OsStr::new(&format!("--worker {port} {token}")));
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                exe.as_ptr(),
                args.as_ptr(),
                std::ptr::null(),
                0,
            )
        };
        ensure!(
            result > 32,
            "Administrator access was declined or the worker could not start (Windows code {result})"
        );
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (port, token);
        bail!("Building is currently supported on Windows only")
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(windows)]
fn explorer_binary() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
        .join("explorer.exe")
}
pub fn reveal_file(path: &Path) -> Result<()> {
    crate::cleanup::safe_path(path)?;
    let metadata = std::fs::metadata(path)?;
    ensure!(
        metadata.is_file(),
        "ISO file does not exist: {}",
        display_path(path)
    );
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let path = display_path(path);
        ensure!(!path.contains('"'), "Invalid quote in file path");
        // Explorer needs /select,"path", rather than quoting the entire switch
        // and path as a single standard Command argument. No shell is involved.
        Command::new(explorer_binary())
            .raw_arg(format!("/select,\"{path}\""))
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg("-R").arg(path).spawn()?;
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        open_folder(path.parent().context("ISO has no parent folder")?)?;
    }
    Ok(())
}

pub fn open_folder(path: &Path) -> Result<()> {
    crate::cleanup::safe_path(path)?;
    let metadata = std::fs::metadata(path)?;
    ensure!(
        metadata.is_dir(),
        "Folder does not exist: {}",
        display_path(path)
    );
    #[cfg(windows)]
    {
        Command::new(explorer_binary())
            .arg(display_path(path))
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?;
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

pub fn publish_iso(source: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
        }
        let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        // No REPLACE_EXISTING flag. Both paths are on the same filesystem.
        if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 8) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::hard_link(source, target)?;
        fs::remove_file(source)?;
        Ok(())
    }
}

/// Resolve volume identities, including mounted folders, for replacement space credit.
pub fn same_volume(a: &Path, b: &Path) -> Result<bool> {
    #[cfg(windows)]
    fn volume(path: &Path) -> Result<Vec<u16>> {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetVolumePathNameW(file: *const u16, root: *mut u16, len: u32) -> i32;
            fn GetVolumeNameForVolumeMountPointW(root: *const u16, name: *mut u16, len: u32)
            -> i32;
        }
        let input: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut root = vec![0; 32768];
        let mut name = vec![0; 32768];
        if unsafe { GetVolumePathNameW(input.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0
            || unsafe {
                GetVolumeNameForVolumeMountPointW(
                    root.as_ptr(),
                    name.as_mut_ptr(),
                    name.len() as u32,
                )
            } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        name.truncate(name.iter().position(|c| *c == 0).unwrap_or(name.len()));
        Ok(name)
    }
    #[cfg(unix)]
    fn volume(path: &Path) -> Result<u64> {
        use std::os::unix::fs::MetadataExt;
        Ok(fs::metadata(path)?.dev())
    }
    Ok(volume(a)? == volume(b)?)
}

#[cfg(test)]
mod path_display_tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn replacement_credit_uses_resolved_volume_and_publication_never_clobbers() {
        let root =
            std::env::temp_dir().join(format!("patcher-output-space-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let file = root.join("output with spaces.iso");
        let partial = root.join("temporary.iso");
        fs::write(&file, b"existing").unwrap();
        fs::write(&partial, b"new").unwrap();
        assert!(same_volume(&root, &file).unwrap());
        assert!(publish_iso(&partial, &file).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"existing");
        assert!(partial.exists());
        fs::remove_file(file).unwrap();
        fs::remove_file(partial).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn explorer_is_resolved_from_windows_directory_not_system32() {
        assert_eq!(
            explorer_binary().parent(),
            system_binary("cmd.exe").parent().unwrap().parent()
        );
        assert!(explorer_binary().is_file());
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "Opens and closes only disposable test folders in File Explorer"]
    fn windows_open_and_reveal_paths_with_spaces() {
        let root =
            std::env::temp_dir().join(format!("patcher explorer test {}", uuid::Uuid::new_v4()));
        let folder = root.join("workspace with spaces");
        fs::create_dir_all(&folder).unwrap();
        let iso = folder.join("Windows test image.iso");
        fs::write(&iso, b"Disposable Explorer test").unwrap();
        let folder = folder.canonicalize().unwrap();
        let iso = iso.canonicalize().unwrap();
        let verify = |selected: bool| {
            powershell(
                r#"
            $shell=New-Object -ComObject Shell.Application
            $found=$false; $deadline=[DateTime]::UtcNow.AddSeconds(15)
            try {
                while([DateTime]::UtcNow -lt $deadline -and -not $found) {
                    foreach($window in @($shell.Windows())) {
                        try {
                            if($window.Document.Folder.Self.Path -ieq $env:PATCHER_TEST_FOLDER) {
                                if($env:PATCHER_TEST_SELECT -eq '0') {$found=$true}
                                else {foreach($item in @($window.Document.SelectedItems())) {if($item.Path -ieq $env:PATCHER_TEST_ISO){$found=$true}}}
                            }
                        } catch {}
                    }
                    if(-not $found){Start-Sleep -Milliseconds 200}
                }
                if(-not $found){throw 'Explorer did not open the expected path or select the expected file'}
            } finally {
                if($env:PATCHER_TEST_SELECT -eq '1'){foreach($window in @($shell.Windows())) {try {if($window.Document.Folder.Self.Path -ieq $env:PATCHER_TEST_FOLDER){$window.Quit()}} catch {}}}
            }
        "#,
                &[
                    (
                        "PATCHER_TEST_FOLDER",
                        std::ffi::OsStr::new(&display_path(&folder)),
                    ),
                    (
                        "PATCHER_TEST_ISO",
                        std::ffi::OsStr::new(&display_path(&iso)),
                    ),
                    (
                        "PATCHER_TEST_SELECT",
                        std::ffi::OsStr::new(if selected { "1" } else { "0" }),
                    ),
                ],
            )
        };
        open_folder(&folder).unwrap();
        let folder_result = verify(false);
        reveal_file(&iso).unwrap();
        let reveal_result = verify(true);
        // Fixture root is a literal UUID child of Temp, never an app workspace.
        fs::remove_file(&iso).unwrap();
        fs::remove_dir(&folder).unwrap();
        fs::remove_dir(&root).unwrap();
        folder_result
            .context("Opening the folder did not work")
            .unwrap();
        reveal_result
            .context("Selecting the ISO did not work")
            .unwrap();
    }
    #[test]
    fn opening_missing_locations_fails_before_launching_explorer() {
        let missing =
            std::env::temp_dir().join(format!("missing-patcher-{}", uuid::Uuid::new_v4()));
        for result in [open_folder(&missing), reveal_file(&missing)] {
            assert!(result.unwrap_err().chain().any(|e| {
                e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            }));
        }
    }
    #[test]
    fn displays_extended_drive_and_network_paths_in_familiar_form() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\ISO files\Windows.iso")),
            r"C:\ISO files\Windows.iso"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\Windows.iso")),
            r"\\server\share\Windows.iso"
        );
    }
    #[test]
    fn preserves_normal_paths_and_device_namespaces() {
        for path in [
            r"C:\ISO files\Windows.iso",
            r"\\server\share\Windows.iso",
            r"\\?\Volume{abc}\Windows.iso",
            "/tmp/Windows.iso",
        ] {
            assert_eq!(display_path(Path::new(path)), path);
        }
    }
}
