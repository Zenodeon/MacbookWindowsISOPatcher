//! A1706 support payload and source-reference metadata. Never uploads diagnostics.
use crate::domain::{BuildRequest, Image, Inventory};
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

pub const MARKER: &str = "MacBookPatcher-Logging.enabled";
pub const LOG_FOLDER: &str = "MacBookPatcher-Logs";
pub const PAYLOAD_VERSION: &str = "a1706-v17-diagnostics-1";
pub const LOGGER: &str = "MacBookSetupLogger.exe";
pub const INSTRUCTIONS: &str = "MacBook ISO Patcher - Setup support tools\r\n\r\nTo enable USB logging, copy MacBookPatcher-Logging.enabled to the USB root (next to the ISO, not inside it). Alternatively use Prepare USB for setup logs in the patcher. Do not rename the marker. Only marked USB storage receives logs; USB drives reported as fixed disks are supported. Multiple marked USBs each receive a copy.\r\n\r\nLogs: MacBookPatcher-Logs\\A1706\\<UTC timestamp>-<session ID>\r\nAutomatic logging runs in the booted installer and stops at reboot. It is not a background service. Keep the USB connected. Without a writable marked USB, WinPE logs remain in RAM and are lost at reboot. Collection failures never stop Setup.\r\n\r\nAfter a reboot, run Collect setup logs.cmd as administrator from this Support directory to recover available saved Windows Setup logs. This creates a separate recovery session, linked by the build manifest. Installed copy: C:\\MacBookPatcher\\Support. If Windows will not boot, run the collector from this ISO's Support folder using the installer command prompt.\r\n\r\nAttach the entire session folder and your saved build log to https://github.com/Zenodeon/MacbookWindowsISOPatcher/issues . Include your actual model, display connection, last visible screen and what failed. Review native logs before sharing; Windows may include paths and account names. No files are uploaded automatically. Older ISOs may instead use A1706Logs.\r\n";

pub fn version(image: &Image) -> String {
    match image.revision {
        Some(revision) => format!("{}.{}", image.build, revision),
        None => format!("{} (revision unknown)", image.build),
    }
}
pub fn reference_notice(inventory: &Inventory, selected: &[u32]) -> Option<String> {
    let images: Vec<_> = inventory
        .images
        .iter()
        .filter(|i| selected.contains(&i.index))
        .collect();
    let matches = !images.is_empty()
        && images
            .iter()
            .all(|i| i.edition == "Professional" && i.build == 19045 && i.revision == Some(3803))
        && inventory
            .setup_image
            .as_ref()
            .is_some_and(|i| i.build == 19041 && i.revision == Some(3803));
    if matches {
        return None;
    }
    let editions = images
        .iter()
        .map(|i| format!("{} {}", i.name, version(i)))
        .collect::<Vec<_>>()
        .join(", ");
    let setup = inventory
        .setup_image
        .as_ref()
        .map(version)
        .unwrap_or_else(|| "unknown; rescan to read Setup metadata".into());
    Some(format!(
        "Different from the working A1706 reference. Your source: {editions}; Setup {setup}. Reference: Windows 10 Pro 19045.3803; Setup 19041.3803. You can still build, but this combination needs testing. Matching versions alone do not verify a generated ISO."
    ))
}
pub fn write_support(dir: &Path, req: &BuildRequest, job: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    let model = crate::models::MODEL_PROFILES
        .iter()
        .find(|m| m.id == req.model)
        .context("Model missing")?;
    let metadata = serde_json::json!({
        "schema_version": 1, "build_id": job.file_name().unwrap_or_default().to_string_lossy(),
        "app_version": env!("CARGO_PKG_VERSION"), "payload_version": PAYLOAD_VERSION,
        "code_revision": option_env!("PATCHER_CODE_REVISION").unwrap_or("unknown"),
        "code_dirty": option_env!("PATCHER_CODE_DIRTY").map(|v| v == "true"),
        "model_profile": model.id, "model_identifier": model.identifier, "model_name": model.name,
        "graphics": model.details, "intel_driver": "21.20.16.5174",
        "source_iso_filename": req.inventory.iso.file_name().unwrap_or_default().to_string_lossy(),
        "windows_images": req.selected_images()?, "setup_image": req.inventory.setup_image,
        "reference_windows": "19045.3803 Professional", "reference_setup": "19041.3803",
        "collection": "WinPE only; manual recovery after reboot; no upload"
    });
    fs::write(
        dir.join("build-manifest.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    fs::write(
        dir.join(MARKER),
        include_bytes!("../payloads/a1706/MacBookPatcher-Logging.enabled"),
    )?;
    fs::write(
        dir.join("Collect setup logs.cmd"),
        include_bytes!("../payloads/a1706/Collect setup logs.cmd"),
    )?;
    fs::write(dir.join("README.txt"), INSTRUCTIONS)?;
    #[cfg(windows)]
    fs::write(
        dir.join(LOGGER),
        include_bytes!(concat!(env!("OUT_DIR"), "/MacBookSetupLogger.exe")),
    )?;
    Ok(())
}
pub fn prepare_usb(root: &Path) -> Result<()> {
    ensure!(cfg!(windows), "USB preparation requires Windows");
    #[cfg(windows)]
    {
        let temp = std::env::temp_dir().join(format!("macbook-logging-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&temp)?;
        let helper = temp.join(LOGGER);
        let result = (|| {
            fs::write(
                &helper,
                include_bytes!(concat!(env!("OUT_DIR"), "/MacBookSetupLogger.exe")),
            )?;
            let normalized = crate::platform::display_path(root);
            let mut cmd = std::process::Command::new(&helper);
            let output = crate::platform::hidden(&mut cmd)
                .args(["--prepare-usb", &normalized])
                .output()?;
            ensure!(
                output.status.success(),
                "USB preparation failed for {normalized} ({}). {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Ok(())
        })();
        let _ = fs::remove_file(helper);
        let _ = fs::remove_dir(temp);
        result
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revisions_and_old_saved_jobs() {
        let raw = r#"{"index":1,"name":"Windows 10 Pro","edition":"Professional","arch":9,"build":19045,"major":10,"minor":0,"language":"en-US","installation_type":"Client","size":1}"#;
        let mut image: Image = serde_json::from_str(raw).unwrap();
        assert_eq!(image.revision, None);
        assert!(version(&image).contains("unknown"));
        image.revision = Some(3803);
        let mut setup = image.clone();
        setup.build = 19041;
        let mut inv = Inventory {
            iso: "source.iso".into(),
            iso_size: 0,
            iso_modified: 0,
            install_file: "install.wim".into(),
            images: vec![image],
            setup_image: Some(setup),
        };
        assert!(reference_notice(&inv, &[1]).is_none());
        inv.images[0].revision = Some(2965);
        assert!(reference_notice(&inv, &[1]).unwrap().contains("19045.2965"));
        let mut json = serde_json::to_value(&inv).unwrap();
        json.as_object_mut().unwrap().remove("setup_image");
        let older: Inventory = serde_json::from_value(json).unwrap();
        assert!(older.setup_image.is_none());
        assert!(reference_notice(&older, &[1]).unwrap().contains("unknown"));
    }
    #[test]
    fn wim_revision_parsing() {
        let images=crate::wim::parse_xml(r#"<WIM><IMAGE INDEX="2"><WINDOWS><VERSION><BUILD>19041</BUILD><SPBUILD>3803</SPBUILD></VERSION></WINDOWS></IMAGE></WIM>"#).unwrap();
        assert_eq!(images[0].revision, Some(3803));
    }
}
