//! Opt-in Windows storage test using a tiny generated data ISO, never user media.
#![cfg(windows)]
use macbook_iso_patcher::platform::{self, MountedIso};
use std::{fs, path::PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn both16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    bytes[offset + 2..offset + 4].copy_from_slice(&value.to_be_bytes());
}
fn both32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    bytes[offset + 4..offset + 8].copy_from_slice(&value.to_be_bytes());
}
fn directory_record(identifier: u8) -> [u8; 34] {
    let mut record = [0; 34];
    record[0] = 34;
    both32(&mut record, 2, 20);
    both32(&mut record, 10, 2048);
    record[18..25].copy_from_slice(&[126, 10, 20, 0, 0, 0, 0]);
    record[25] = 2;
    both16(&mut record, 28, 1);
    record[32] = 1;
    record[33] = identifier;
    record
}
fn data_iso() -> Fixture {
    // Minimal ISO 9660 volume: descriptors, two path tables, empty root directory.
    let mut bytes = vec![0u8; 24 * 2048];
    let pvd = &mut bytes[16 * 2048..17 * 2048];
    pvd[0..7].copy_from_slice(b"\x01CD001\x01");
    pvd[8..40].fill(b' ');
    pvd[40..72].fill(b' ');
    pvd[40..50].copy_from_slice(b"PATCHER_QA");
    both32(pvd, 80, 24);
    both16(pvd, 120, 1);
    both16(pvd, 124, 1);
    both16(pvd, 128, 2048);
    both32(pvd, 132, 10);
    pvd[140..144].copy_from_slice(&18u32.to_le_bytes());
    pvd[148..152].copy_from_slice(&19u32.to_be_bytes());
    pvd[156..190].copy_from_slice(&directory_record(0));
    for offset in [813, 830, 847, 864] {
        pvd[offset..offset + 16].copy_from_slice(b"2026102000000000");
    }
    pvd[881] = 1;
    bytes[17 * 2048..17 * 2048 + 7].copy_from_slice(b"\xffCD001\x01");
    for (sector, big) in [(18, false), (19, true)] {
        let table = &mut bytes[sector * 2048..sector * 2048 + 10];
        table[0] = 1;
        table[2..6].copy_from_slice(&if big {
            20u32.to_be_bytes()
        } else {
            20u32.to_le_bytes()
        });
        table[6..8].copy_from_slice(&if big {
            1u16.to_be_bytes()
        } else {
            1u16.to_le_bytes()
        });
    }
    bytes[20 * 2048..20 * 2048 + 34].copy_from_slice(&directory_record(0));
    bytes[20 * 2048 + 34..20 * 2048 + 68].copy_from_slice(&directory_record(1));
    let path = std::env::temp_dir().join(format!("patcher-mount-{}.iso", uuid::Uuid::new_v4()));
    fs::write(&path, bytes).unwrap();
    Fixture(path)
}
fn attached(fixture: &Fixture) -> bool {
    platform::powershell("$i=Get-DiskImage -ImagePath $env:PATCHER_ISO -ErrorAction SilentlyContinue; [bool]($i -and $i.Attached)", &[("PATCHER_ISO", fixture.0.as_os_str())]).unwrap().trim() == "True"
}

#[test]
#[ignore = "Mounts a generated 48 KiB ISO through Windows Storage; run explicitly"]
fn source_mount_ownership_and_cleanup() {
    let fixture = data_iso();
    assert!(!attached(&fixture));
    let mut owned = MountedIso::open(&fixture.0).unwrap();
    assert!(attached(&fixture));
    assert!(owned.cleanup().unwrap());
    assert!(!attached(&fixture));
    drop(owned);

    let owned = MountedIso::open(&fixture.0).unwrap();
    {
        let mut borrowed = MountedIso::open(&fixture.0).unwrap();
        assert!(!borrowed.cleanup().unwrap());
        assert!(
            attached(&fixture),
            "Closing must preserve pre-existing mounts"
        );
    }
    assert!(attached(&fixture));
    drop(owned);
    assert!(
        !attached(&fixture),
        "Dropping an owned session must dismount"
    );

    let mut owned = MountedIso::open(&fixture.0).unwrap();
    let mut borrowed = MountedIso::open(&fixture.0).unwrap();
    assert!(
        borrowed.dismount().unwrap(),
        "Explicit eject may dismount the selected source"
    );
    assert!(!attached(&fixture));
    owned.cleanup().unwrap();
    assert!(
        platform::scan_mounted(&fixture.0).is_err(),
        "The data ISO has no Windows Setup"
    );
    assert!(
        !attached(&fixture),
        "Failed scans must clean up their mount"
    );
}

#[test]
#[ignore = "Starts the GUI renderer and mounts a generated ISO; run explicitly"]
fn normal_gui_close_cleans_owned_mounts_and_preserves_existing_mounts() {
    let fixture = data_iso();
    for previously_mounted in [false, true] {
        let owner = previously_mounted.then(|| MountedIso::open(&fixture.0).unwrap());
        let screenshot = Fixture(
            std::env::temp_dir().join(format!("patcher-close-{}.png", uuid::Uuid::new_v4())),
        );
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_macbook-iso-patcher"));
        command
            .env("MACBOOK_PATCHER_SCREENSHOT", &screenshot.0)
            .env("MACBOOK_PATCHER_PREVIEW", "mounted-close")
            .env("MACBOOK_PATCHER_PREVIEW_ISO", &fixture.0);
        let mut child = platform::hidden(&mut command).spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = platform::dismount_iso(&fixture.0);
                panic!("App did not finish normal close cleanup within 30 seconds");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            screenshot.0.is_file(),
            "GUI must render before requesting normal close"
        );
        assert_eq!(attached(&fixture), previously_mounted);
        drop(owner);
        assert!(!attached(&fixture));
    }
}
