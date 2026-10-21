use crate::{
    domain::{Cancellation, Emit, Event, Inventory},
    wim,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader},
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
        readers.push(thread::spawn(move || {
            for line in BufReader::new(pipe).split(b'\n').flatten() {
                let text = String::from_utf8_lossy(&line).replace('\0', "");
                if !text.trim().is_empty() {
                    emit(Event::Log(text.trim().to_owned()));
                }
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

pub fn reveal_file(path: &Path) -> Result<()> {
    ensure!(
        path.is_file(),
        "ISO file does not exist: {}",
        display_path(path)
    );
    #[cfg(windows)]
    {
        Command::new(system_binary("explorer.exe"))
            .arg(format!("/select,{}", display_path(path)))
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
    ensure!(
        path.is_dir(),
        "Folder does not exist: {}",
        display_path(path)
    );
    #[cfg(windows)]
    {
        Command::new(system_binary("explorer.exe"))
            .arg(path)
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

#[cfg(test)]
mod path_display_tests {
    use super::*;
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
