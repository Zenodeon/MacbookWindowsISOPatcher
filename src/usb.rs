//! Copy a completed ISO to existing USB storage. Never install or format Ventoy.
use crate::{
    cleanup::{self, Fingerprint},
    diagnostics,
    domain::Cancellation,
};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Checking,
    Copying,
    Verifying,
    EnablingLogging,
}
impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Checking => "Checking USB…",
            Self::Copying => "Copying ISO…",
            Self::Verifying => "Verifying ISO…",
            Self::EnablingLogging => "Enabling logging…",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Progress {
    pub phase: Phase,
    pub bytes: u64,
    pub total: u64,
}
#[derive(Clone, Debug)]
pub struct Conflict {
    pub path: PathBuf,
    pub fingerprint: Fingerprint,
}
#[derive(Clone, Debug)]
pub enum Choice {
    Replace(Conflict),
    KeepBoth,
}
#[derive(Clone, Debug)]
pub struct Published {
    pub path: PathBuf,
    pub display_path: PathBuf,
    pub fingerprint: Fingerprint,
    pub volume: String,
}
#[derive(Clone, Debug)]
pub struct Request {
    pub source: PathBuf,
    pub root: PathBuf,
    pub choice: Option<Choice>,
    pub published: Option<Published>,
}
#[derive(Debug)]
pub enum Outcome {
    Conflict(Conflict),
    Ready(Published),
    LoggingFailed { published: Published, error: String },
}
#[derive(Clone)]
struct Volume {
    root: PathBuf,
    identity: String,
    free: u64,
    max_file: u64,
}

pub fn prepare(
    req: &Request,
    cancel: &Cancellation,
    progress: impl FnMut(Progress),
) -> Result<Outcome> {
    prepare_with(req, cancel, volume, diagnostics::prepare_usb, progress)
}
pub fn open_usb_folder(root: &Path, published: &Published) -> Result<()> {
    ensure!(
        volume(root)?.identity == published.volume,
        "The prepared USB is no longer at this location. Reconnect the original USB."
    );
    crate::platform::open_folder(root)
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn check_cancel(cancel: &Cancellation) -> Result<()> {
    ensure!(!cancel.is_cancelled(), "USB preparation cancelled");
    Ok(())
}
fn unchanged(path: &Path, expected: &Fingerprint) -> Result<()> {
    ensure!(
        cleanup::fingerprint(path)? == *expected,
        "File changed during USB preparation: {}",
        path.display()
    );
    Ok(())
}
fn prepare_with(
    req: &Request,
    cancel: &Cancellation,
    validate: impl Fn(&Path) -> Result<Volume>,
    marker: impl Fn(&Path) -> Result<()>,
    mut progress: impl FnMut(Progress),
) -> Result<Outcome> {
    check_cancel(cancel)?;
    progress(Progress {
        phase: Phase::Checking,
        bytes: 0,
        total: 0,
    });
    let vol = validate(&req.root)?;
    cleanup::safe_path(&vol.root)?;
    let _lock = cleanup::activity(&vol.root, ".MacBookPatcher-prepare.lock", true)?;
    let recheck = || -> Result<()> {
        ensure!(
            validate(&req.root)?.identity == vol.identity,
            "The selected USB was removed or changed. Choose it again."
        );
        cleanup::safe_path(&vol.root)
    };
    let enable = |published: Published, progress: &mut dyn FnMut(Progress)| -> Result<Outcome> {
        if cancel.is_cancelled() {
            return Ok(Outcome::LoggingFailed { published, error:"ISO copied. Logging setup was cancelled. Retry to enable logging without copying again.".into() });
        }
        recheck()?;
        unchanged(&published.path, &published.fingerprint)?;
        progress(Progress {
            phase: Phase::EnablingLogging,
            bytes: 0,
            total: 0,
        });
        match marker(&req.root) {
            Ok(()) => {
                recheck()?;
                Ok(Outcome::Ready(published))
            }
            Err(e) => Ok(Outcome::LoggingFailed {
                published,
                error: format!(
                    "The ISO was copied, but logging could not be enabled. Retry will only add the logging marker.\n\n{e:#}"
                ),
            }),
        }
    };
    if let Some(published) = &req.published {
        ensure!(
            published.volume == vol.identity && published.path.parent() == Some(vol.root.as_path()),
            "The USB changed. Choose the original USB again."
        );
        return enable(published.clone(), &mut progress);
    }
    let source =
        cleanup::fingerprint(&req.source).context("The completed ISO is missing or unreadable")?;
    ensure!(
        req.source
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("iso")),
        "Choose a completed ISO file"
    );
    ensure!(source.size > 0, "The completed ISO is empty");
    ensure!(
        source.size <= vol.max_file,
        "This USB filesystem cannot store the ISO. FAT32 files must be smaller than 4 GB. Use a Ventoy USB with exFAT or NTFS."
    );
    let filename = req
        .source
        .file_name()
        .context("ISO filename is unavailable")?;
    let mut destination = vol.root.join(filename);
    cleanup::safe_path(&destination)?;
    if exists(&destination)? {
        let current = cleanup::fingerprint(&destination)?;
        ensure!(
            current.identity != source.identity,
            "The ISO is already in the selected USB location. Choose another USB."
        );
        match &req.choice {
            Some(Choice::KeepBoth) => destination = crate::engine::increment_output(&destination)?,
            Some(Choice::Replace(expected))
                if expected.fingerprint == current && expected.path == req.root.join(filename) => {}
            _ => {
                return Ok(Outcome::Conflict(Conflict {
                    path: req.root.join(filename),
                    fingerprint: current,
                }));
            }
        }
    } else if matches!(req.choice, Some(Choice::Replace(_))) {
        bail!("The existing ISO disappeared. Check the USB and prepare it again.");
    }
    ensure!(
        source.size <= vol.free,
        "Not enough free space on the USB. Need {} for a verified new copy; the existing ISO is kept until verification finishes.",
        cleanup::format_bytes(source.size)
    );
    let temp = vol
        .root
        .join(format!(".MacBookPatcher-{}.partial", uuid::Uuid::new_v4()));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .context("Cannot write to the selected USB")?;
    let temp_identity = cleanup::fingerprint(&temp)?.identity;
    let result = (|| {
        let mut input = File::open(&req.source)?;
        let mut hash = Sha256::new();
        let mut buffer = vec![0; 1024 * 1024];
        let mut done = 0;
        let mut last = Instant::now();
        progress(Progress {
            phase: Phase::Copying,
            bytes: 0,
            total: source.size,
        });
        loop {
            check_cancel(cancel)?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count])?;
            hash.update(&buffer[..count]);
            done += count as u64;
            if last.elapsed().as_millis() >= 100 {
                progress(Progress {
                    phase: Phase::Copying,
                    bytes: done,
                    total: source.size,
                });
                last = Instant::now();
            }
        }
        output.sync_all()?;
        drop(output);
        progress(Progress {
            phase: Phase::Copying,
            bytes: done,
            total: source.size,
        });
        ensure!(
            done == source.size,
            "Source ISO size changed during copying"
        );
        unchanged(&req.source, &source)?;
        recheck()?;
        let expected = hash.finalize();
        let mut copied = File::open(&temp)?;
        let mut hash = Sha256::new();
        done = 0;
        progress(Progress {
            phase: Phase::Verifying,
            bytes: 0,
            total: source.size,
        });
        loop {
            check_cancel(cancel)?;
            let count = copied.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
            done += count as u64;
            if last.elapsed().as_millis() >= 100 {
                progress(Progress {
                    phase: Phase::Verifying,
                    bytes: done,
                    total: source.size,
                });
                last = Instant::now();
            }
        }
        drop(copied);
        progress(Progress {
            phase: Phase::Verifying,
            bytes: done,
            total: source.size,
        });
        ensure!(
            done == source.size && hash.finalize() == expected,
            "USB copy verification failed. Retry or use another USB."
        );
        ensure!(
            cleanup::fingerprint(&temp)?.identity == temp_identity,
            "Temporary USB copy was replaced during verification"
        );
        let verified = cleanup::fingerprint(&temp)?;
        check_cancel(cancel)?;
        recheck()?;
        unchanged(&req.source, &source)?;
        let mut backup = None;
        if let Some(Choice::Replace(conflict)) = &req.choice {
            unchanged(&destination, &conflict.fingerprint)?;
            let saved = vol
                .root
                .join(format!(".MacBookPatcher-{}.previous", uuid::Uuid::new_v4()));
            move_new(&destination, &saved)?;
            // Check the file actually moved, not just the earlier pathname.
            if let Err(e) = unchanged(&saved, &conflict.fingerprint) {
                move_new(&saved, &destination).with_context(|| {
                    format!(
                        "Existing file changed; it is preserved at {}",
                        saved.display()
                    )
                })?;
                return Err(e);
            }
            backup = Some(saved);
        }
        let publish_result =
            unchanged(&temp, &verified).and_then(|()| move_new(&temp, &destination));
        if let Err(error) = publish_result {
            if let Some(saved) = &backup {
                move_new(saved,&destination).with_context(||format!("Copy could not be published. Previous ISO is preserved at {}. Original error: {error:#}",saved.display()))?;
            }
            return Err(error
                .context("USB destination changed or is in use. Existing files were preserved"));
        }
        let published = Published {
            path: destination.clone(),
            display_path: req.root.join(destination.file_name().unwrap()),
            fingerprint: cleanup::fingerprint(&destination)?,
            volume: vol.identity.clone(),
        };
        if let Some(saved) = backup {
            if let Some(Choice::Replace(conflict)) = &req.choice {
                unchanged(&saved, &conflict.fingerprint)?;
            }
            if let Err(e) = fs::remove_file(&saved) {
                bail!(
                    "The new ISO was copied, but the previous ISO could not be removed from {}: {e}. Check the USB before retrying.",
                    saved.display()
                );
            }
        }
        enable(published, &mut progress)
    })();
    // Only remove this operation's own temporary file, on the original volume.
    if result.is_err() && cleanup::fingerprint(&temp).is_ok_and(|f| f.identity == temp_identity) {
        fs::remove_file(&temp).context("Could not remove this operation's partial USB copy")?;
    }
    result
}

#[cfg(windows)]
fn move_new(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // No REPLACE_EXISTING: a destination race must fail instead of overwrite.
    ensure!(
        unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 8) } != 0,
        "{}",
        std::io::Error::last_os_error()
    );
    Ok(())
}
#[cfg(not(windows))]
fn move_new(from: &Path, to: &Path) -> Result<()> {
    fs::hard_link(from, to)?;
    fs::remove_file(from)?;
    Ok(())
}

#[cfg(windows)]
fn volume(root: &Path) -> Result<Volume> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    cleanup::safe_path(root)?;
    diagnostics::validate_usb(root)?;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetVolumeNameForVolumeMountPointW(root: *const u16, name: *mut u16, size: u32) -> i32;
        fn GetVolumeInformationW(
            root: *const u16,
            label: *mut u16,
            label_size: u32,
            serial: *mut u32,
            max_component: *mut u32,
            flags: *mut u32,
            fs: *mut u16,
            fs_size: u32,
        ) -> i32;
    }
    let root: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut guid = [0u16; 128];
    let mut label = [0u16; 256];
    let mut filesystem = [0u16; 64];
    let mut serial = 0;
    ensure!(
        unsafe { GetVolumeNameForVolumeMountPointW(root.as_ptr(), guid.as_mut_ptr(), 128) } != 0,
        "Cannot identify the selected USB volume: {}",
        std::io::Error::last_os_error()
    );
    ensure!(
        unsafe {
            GetVolumeInformationW(
                guid.as_ptr(),
                label.as_mut_ptr(),
                256,
                &mut serial,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                filesystem.as_mut_ptr(),
                64,
            )
        } != 0,
        "Cannot read USB filesystem: {}",
        std::io::Error::last_os_error()
    );
    let string = |s: &[u16]| {
        String::from_utf16_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())])
    };
    ensure!(
        !string(&label).eq_ignore_ascii_case("VTOYEFI"),
        "Choose the main Ventoy USB partition, not VTOYEFI"
    );
    let id = string(&guid);
    let anchor = PathBuf::from(std::ffi::OsString::from_wide(
        &guid[..guid.iter().position(|c| *c == 0).unwrap()],
    ));
    let free = fs2::available_space(&anchor)?;
    Ok(Volume {
        root: anchor,
        identity: format!("{id}:{serial}"),
        free,
        max_file: if string(&filesystem).eq_ignore_ascii_case("FAT32") {
            u32::MAX as u64
        } else {
            u64::MAX
        },
    })
}
#[cfg(not(windows))]
fn volume(_: &Path) -> Result<Volume> {
    bail!("USB preparation requires Windows")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Fixture {
        dir: PathBuf,
        req: Request,
    }
    impl Fixture {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("macbook-usb-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(dir.join("usb")).unwrap();
            let source = dir.join("Patched.iso");
            fs::write(&source, b"disposable test ISO bytes").unwrap();
            let req = Request {
                source,
                root: dir.join("usb"),
                choice: None,
                published: None,
            };
            Self { dir, req }
        }
        fn volume(&self) -> Volume {
            Volume {
                root: self.req.root.clone(),
                identity: "disposable-volume".into(),
                free: u64::MAX,
                max_file: u64::MAX,
            }
        }
        fn run(&self) -> Result<Outcome> {
            self.run_with(&Cancellation::default(), |_| {})
        }
        fn run_with(
            &self,
            cancel: &Cancellation,
            progress: impl FnMut(Progress),
        ) -> Result<Outcome> {
            prepare_with(
                &self.req,
                cancel,
                |_| Ok(self.volume()),
                |root| {
                    fs::write(root.join("logging-marker"), b"test marker")?;
                    Ok(())
                },
                progress,
            )
        }
        fn no_partials(&self) {
            assert!(!fs::read_dir(&self.req.root).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".partial")
            }));
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert!(self.dir.parent() == Some(std::env::temp_dir().as_path()));
            assert!(
                self.dir
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("macbook-usb-test-")
            );
            fs::remove_dir_all(&self.dir).unwrap();
        }
    }
    #[test]
    fn verified_copy_and_marker_preserve_source_and_unrelated_files() {
        let f = Fixture::new();
        fs::write(f.req.root.join("keep.txt"), b"keep").unwrap();
        let mut phases = vec![];
        let result = f
            .run_with(&Cancellation::default(), |p| phases.push(p))
            .unwrap();
        let Outcome::Ready(published) = result else {
            panic!("Expected ready")
        };
        assert_eq!(
            fs::read(&published.path).unwrap(),
            fs::read(&f.req.source).unwrap()
        );
        assert!(f.req.root.join("logging-marker").exists());
        assert!(f.req.root.join("keep.txt").exists());
        assert!(
            phases
                .iter()
                .any(|p| p.phase == Phase::Copying && p.bytes == p.total)
        );
        assert!(
            phases
                .iter()
                .any(|p| p.phase == Phase::Verifying && p.bytes == p.total)
        );
        f.no_partials();
    }
    #[test]
    fn conflict_requires_choice_and_replace_preserves_old_until_verified() {
        let mut f = Fixture::new();
        let target = f.req.root.join("Patched.iso");
        fs::write(&target, b"previous ISO").unwrap();
        let Outcome::Conflict(conflict) = f.run().unwrap() else {
            panic!("Expected conflict")
        };
        assert!(!f.req.root.join("logging-marker").exists());
        f.req.choice = Some(Choice::Replace(conflict));
        let ready = f
            .run_with(&Cancellation::default(), |p| {
                if p.phase == Phase::Verifying {
                    assert_eq!(fs::read(&target).unwrap(), b"previous ISO");
                }
            })
            .unwrap();
        assert!(matches!(ready, Outcome::Ready(_)));
        assert_eq!(fs::read(&target).unwrap(), fs::read(&f.req.source).unwrap());
        f.no_partials();
    }
    #[test]
    fn keep_both_increments_and_skips_taken_suffixes() {
        let mut f = Fixture::new();
        fs::write(f.req.root.join("Patched.iso"), b"old").unwrap();
        fs::write(f.req.root.join("Patched (2).iso"), b"also old").unwrap();
        f.req.choice = Some(Choice::KeepBoth);
        let Outcome::Ready(p) = f.run().unwrap() else {
            panic!()
        };
        assert_eq!(p.path.file_name().unwrap(), "Patched (3).iso");
        assert_eq!(fs::read(f.req.root.join("Patched.iso")).unwrap(), b"old");
    }
    #[test]
    fn marker_retry_uses_verified_published_copy_without_source_or_recopy() {
        let mut f = Fixture::new();
        let result = prepare_with(
            &f.req,
            &Cancellation::default(),
            |_| Ok(f.volume()),
            |_| bail!("read-only marker"),
            |_| {},
        )
        .unwrap();
        let Outcome::LoggingFailed { published, .. } = result else {
            panic!()
        };
        f.req.published = Some(published.clone());
        fs::remove_file(&f.req.source).unwrap();
        let mut phases = vec![];
        assert!(matches!(
            f.run_with(&Cancellation::default(), |p| phases.push(p.phase))
                .unwrap(),
            Outcome::Ready(_)
        ));
        assert!(!phases.contains(&Phase::Copying));
        assert!(!phases.contains(&Phase::Verifying));
        assert_eq!(
            cleanup::fingerprint(&published.path).unwrap(),
            published.fingerprint
        );
        fs::write(&published.path, b"changed").unwrap();
        assert!(f.run().is_err());
    }
    #[test]
    fn cancellation_and_corruption_remove_only_owned_partial() {
        let f = Fixture::new();
        let cancel = Cancellation::default();
        assert!(
            f.run_with(&cancel, |p| {
                if p.phase == Phase::Copying {
                    cancel.cancel();
                }
            })
            .is_err()
        );
        f.no_partials();
        let corrupted = Cell::new(false);
        assert!(
            f.run_with(&Cancellation::default(), |p| {
                if p.phase == Phase::Verifying && !corrupted.replace(true) {
                    for entry in fs::read_dir(&f.req.root).unwrap() {
                        let path = entry.unwrap().path();
                        if path.extension().is_some_and(|s| s == "partial") {
                            fs::write(path, b"corrupt").unwrap();
                        }
                    }
                }
            })
            .is_err()
        );
        f.no_partials();
        assert!(!f.req.root.join("Patched.iso").exists());
    }
    #[test]
    fn changed_source_and_changed_replacement_are_never_published() {
        let mut f = Fixture::new();
        let target = f.req.root.join("Patched.iso");
        fs::write(&target, b"old").unwrap();
        let Outcome::Conflict(conflict) = f.run().unwrap() else {
            panic!()
        };
        f.req.choice = Some(Choice::Replace(conflict));
        assert!(
            f.run_with(&Cancellation::default(), |p| {
                if p.phase == Phase::Verifying {
                    fs::write(&target, b"changed externally").unwrap();
                }
            })
            .is_err()
        );
        assert_eq!(fs::read(&target).unwrap(), b"changed externally");
        f.no_partials();
        fs::remove_file(&target).unwrap();
        assert!(f.run().is_err()); // Confirmed replacement disappeared.
        f.req.choice = None;
        assert!(
            f.run_with(&Cancellation::default(), |p| {
                if p.phase == Phase::Copying {
                    fs::write(&f.req.source, b"source changed").unwrap();
                }
            })
            .is_err()
        );
        f.no_partials();
    }
    #[test]
    fn destination_race_preserves_new_file_and_usb_removal_stops_publication() {
        let f = Fixture::new();
        let target = f.req.root.join("Patched.iso");
        assert!(
            f.run_with(&Cancellation::default(), |p| {
                if p.phase == Phase::Verifying {
                    fs::write(&target, b"raced file").unwrap();
                }
            })
            .is_err()
        );
        assert_eq!(fs::read(&target).unwrap(), b"raced file");
        f.no_partials();
        fs::remove_file(target).unwrap();
        let removed = Cell::new(false);
        assert!(
            prepare_with(
                &f.req,
                &Cancellation::default(),
                |_| {
                    ensure!(!removed.get(), "USB removed");
                    Ok(f.volume())
                },
                |_| Ok(()),
                |p| {
                    if p.phase == Phase::Copying {
                        removed.set(true);
                    }
                }
            )
            .is_err()
        );
        f.no_partials();
        assert!(!f.req.root.join("Patched.iso").exists());
    }
    #[test]
    fn insufficient_space_file_limits_missing_source_aliases_and_active_lock_fail() {
        let f = Fixture::new();
        for size_limit in [false, true] {
            let mut vol = f.volume();
            if size_limit {
                vol.max_file = 1;
            } else {
                vol.free = 0;
            }
            assert!(
                prepare_with(
                    &f.req,
                    &Cancellation::default(),
                    |_| Ok(vol.clone()),
                    |_| Ok(()),
                    |_| {}
                )
                .is_err()
            );
        }
        let _lock = cleanup::activity(&f.req.root, ".MacBookPatcher-prepare.lock", true).unwrap();
        assert!(f.run().is_err());
        drop(_lock);
        fs::hard_link(&f.req.source, f.req.root.join("Patched.iso")).unwrap();
        assert!(f.run().is_err());
        fs::remove_file(f.req.root.join("Patched.iso")).unwrap();
        fs::remove_file(&f.req.source).unwrap();
        assert!(f.run().is_err());
        f.no_partials();
    }
    #[cfg(windows)]
    #[test]
    fn redirected_usb_is_rejected_and_real_host_directory_is_not_usb_root() {
        let f = Fixture::new();
        assert!(volume(&f.req.root).is_err());
        let link = f.dir.join("junction");
        crate::platform::powershell(
            "New-Item -ItemType Junction -Path $env:USB_LINK -Target $env:USB_TARGET | Out-Null",
            &[
                ("USB_LINK", link.as_os_str()),
                ("USB_TARGET", f.req.root.as_os_str()),
            ],
        )
        .unwrap();
        assert!(cleanup::safe_path(&link).is_err());
        fs::remove_dir(link).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn read_only_storage_and_locked_destination_preserve_files() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        let lock = f.req.root.join(".MacBookPatcher-prepare.lock");
        fs::write(&lock, b"").unwrap();
        let original_permissions = fs::metadata(&lock).unwrap().permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_readonly(true);
        fs::set_permissions(&lock, permissions).unwrap();
        assert!(f.run().is_err());
        fs::set_permissions(&lock, original_permissions).unwrap();
        let target = f.req.root.join("Patched.iso");
        fs::write(&target, b"locked previous ISO").unwrap();
        let handle = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        assert!(f.run().is_err());
        drop(handle);
        assert_eq!(fs::read(target).unwrap(), b"locked previous ISO");
        f.no_partials();
    }
}
