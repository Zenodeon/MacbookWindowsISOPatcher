//! Shared completion instructions, issue drafts, and complete build-log export.
use crate::{
    cleanup,
    domain::{BuildFiles, Image, MODEL_ID},
    models::ModelProfile,
};
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

pub const VENTOY_URL: &str = "https://www.ventoy.net/";
pub const ITUNES_URL: &str = "https://www.apple.com/itunes/download/win64/";
pub const APPLE_UPDATE_URL: &str = "https://support.apple.com/en-us/118406";
pub const ISSUES_URL: &str = "https://github.com/Zenodeon/MacbookWindowsISOPatcher/issues/new";
pub const INSTALL_STEPS: [&str; 7] = [
    "Prepare a Windows partition using Boot Camp Assistant or another method.",
    "Set up a USB drive with Ventoy, then copy the generated ISO onto it.",
    "Boot the USB, select the ISO in Ventoy, and choose WIMBOOT. Keep the USB connected during Setup.",
    "Complete Windows Setup normally using the Windows partition you prepared.",
    "In Windows, run C:\\BootCamp\\Setup.exe as administrator, then restart.",
    "Search Windows for Apple Software Update. If it is missing, install Apple's desktop iTunes installer, then check for Apple Software Update again.",
    "Install the applicable updates offered by Apple Software Update and restart. Check your MacBook's features and report anything still failing.",
];

pub fn installation_guide(model: &str) -> String {
    let mut text = String::from("MacBook Windows installation\r\n\r\n");
    for (index, step) in INSTALL_STEPS.iter().enumerate() {
        text.push_str(&format!("{}. {}\r\n\r\n", index + 1, step));
    }
    text.push_str(&format!("Ventoy: {VENTOY_URL}\r\nDesktop iTunes: {ITUNES_URL}\r\nApple update guidance: {APPLE_UPDATE_URL}\r\n\r\n"));
    if model == MODEL_ID {
        text.push_str("A1706: keep the included signed Intel Iris 550 graphics driver 21.20.16.5174. Do not run the old IntelHDGraphics64 installer. For boot/setup failures, attach the session folder from MacBookPatcher-Logs on the marked USB and your saved build log. Prepare USB logging using the app or copy MacBookPatcher-Logging.enabled from Support to the USB root. Automatic collection ends at reboot; run Support/Collect setup logs.cmd to recover saved logs later. Older ISOs use A1706Logs.\r\n\r\n");
    }
    text.push_str("Support varies by model and Windows edition. If Setup or any MacBook feature fails, describe what happened and include the Mac model number/identifier, Windows edition/build, build log, and any setup/startup logs.\r\nReport an issue: https://github.com/Zenodeon/MacbookWindowsISOPatcher/issues\r\n");
    text
}

pub fn issue_draft(model: &ModelProfile, images: &[&Image]) -> String {
    let editions = images
        .iter()
        .map(|image| {
            format!(
                "{} (build {})",
                image.name,
                crate::diagnostics::version(image)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let graphics = crate::driver_prefetch::intel_graphics(model.id)
        .map(|profile| format!("{}; Intel driver {}", model.details, profile.version))
        .unwrap_or_else(|_| format!("{}; Apple Boot Camp AMD graphics", model.details));
    let mut body = format!(
        "## Build details\nSelected Mac model: {}\nModel identifier: {}\nModel profile: {}\nWindows editions/build: {}\nGraphics: {}\nApp version: {}\n\n## What happened?\nDescribe the failure and which installation step you reached. If your actual Mac model differs from the selection above, include it here.\n\n## Logs\nAttach the saved build log and any Windows Setup/startup logs.\n",
        model.name,
        model.identifier,
        model.id,
        editions,
        graphics,
        env!("CARGO_PKG_VERSION")
    );
    if model.id == MODEL_ID {
        body.push_str(
            "For A1706 boot/setup problems, attach the session folder from the USB's MacBookPatcher-Logs/A1706 directory and the saved build log. For older ISOs, attach A1706Logs when available.\n",
        );
    }
    let mut url = reqwest::Url::parse(ISSUES_URL).expect("Static GitHub URL");
    url.query_pairs_mut()
        .append_pair(
            "title",
            &format!("[{}] Installation or driver issue", model.identifier),
        )
        .append_pair("body", &body);
    url.into()
}

pub struct LogExport {
    pub bytes: Vec<u8>,
    pub partial_reason: Option<String>,
    source: Option<std::path::PathBuf>,
}
pub fn log_export(files: Option<&BuildFiles>, buffered: &[String]) -> LogExport {
    let retained = (|| -> Result<Vec<u8>> {
        let files = files.context("No retained build log was reported by the worker")?;
        ensure!(
            files.job.is_absolute() && files.log == files.job.join("build.log"),
            "Unexpected retained build log location"
        );
        let name = files
            .job
            .file_name()
            .and_then(|name| name.to_str())
            .context("Missing job folder name")?;
        uuid::Uuid::parse_str(
            name.strip_prefix("build-")
                .context("Unexpected job folder")?,
        )?;
        cleanup::safe_path(&files.log)?;
        ensure!(
            fs::metadata(&files.log)?.is_file(),
            "Retained build log is not a file"
        );
        fs::read(&files.log).context("Could not read the retained build log")
    })();
    match retained {
        Ok(bytes) => LogExport {
            bytes,
            partial_reason: None,
            source: files.map(|files| files.log.clone()),
        },
        Err(error) => {
            let reason = format!("{error:#}");
            let bytes=format!("PARTIAL BUILD LOG\nFull retained build log unavailable: {reason}\nOnly the buffered entries below are included.\n\n{}\n",buffered.join("\n")).into_bytes();
            LogExport {
                bytes,
                partial_reason: Some(reason),
                source: None,
            }
        }
    }
}
pub fn save_log(path: &Path, export: &LogExport) -> Result<()> {
    ensure!(
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("log")),
        "Choose a .log filename for the build log"
    );
    if path.exists()
        && let Some(source) = &export.source
    {
        ensure!(
            path.canonicalize()? != source.canonicalize()?,
            "Choose a different filename; the retained build log cannot be overwritten"
        );
    }
    fs::write(path, &export.bytes).context("Could not save build log")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MODEL_PROFILES;
    #[test]
    fn guide_uses_menu_wimboot_and_checks_updater_before_itunes() {
        let guide = installation_guide(MODEL_ID);
        assert!(guide.contains("or another method") && guide.contains("choose WIMBOOT"));
        assert!(!guide.to_ascii_lowercase().contains("press w"));
        assert!(
            guide
                .find("Search Windows for Apple Software Update")
                .unwrap()
                < guide.find("install Apple's desktop iTunes").unwrap()
        );
        assert!(guide.contains("C:\\BootCamp\\Setup.exe") && guide.contains("A1706Logs"));
        assert!(!installation_guide("a1707-2016").contains("A1706Logs"));
    }
    #[test]
    fn issue_url_encodes_metadata_and_requests_attachments_without_submitting() {
        let image = Image {
            index: 1,
            name: "Windows 10 Pro & Education + test".into(),
            edition: "Professional".into(),
            arch: 9,
            build: 19041,
            revision: None,
            major: 10,
            minor: 0,
            language: "en-US".into(),
            installation_type: "Client".into(),
            size: 1,
        };
        let url = reqwest::Url::parse(&issue_draft(&MODEL_PROFILES[0], &[&image])).unwrap();
        assert_eq!(url.path(), "/Zenodeon/MacbookWindowsISOPatcher/issues/new");
        let query = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        let body = &query["body"];
        assert_eq!(query.len(), 2);
        assert!(
            body.contains("Windows 10 Pro & Education + test (build 19041 (revision unknown))")
        );
        assert!(
            body.contains("MacBookPro13,2")
                && body.contains("21.20.16.5174")
                && body.contains("A1706Logs")
        );
        assert!(query["title"].contains("[MacBookPro13,2]"));
    }
    #[test]
    fn full_log_is_untruncated_and_missing_log_is_labelled_partial() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let job = temp.join(format!("build-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&job).unwrap();
        let files = BuildFiles {
            log: job.join("build.log"),
            job: job.clone(),
        };
        let content = "retained entry\n".repeat(4500);
        fs::write(&files.log, &content).unwrap();
        let export = log_export(Some(&files), &["buffered tail".into()]);
        assert!(export.partial_reason.is_none());
        assert_eq!(export.bytes, content.as_bytes());
        let saved = job.join("saved.log");
        assert!(save_log(&files.log, &export).is_err());
        save_log(&saved, &export).unwrap();
        assert_eq!(fs::read(&saved).unwrap(), export.bytes);
        assert!(save_log(&job.join("missing/failed.log"), &export).is_err());
        assert!(save_log(&job.join("source.iso"), &export).is_err());
        fs::remove_file(&files.log).unwrap();
        let partial = log_export(Some(&files), &["buffered tail".into()]);
        assert!(partial.partial_reason.is_some());
        assert!(
            String::from_utf8(partial.bytes)
                .unwrap()
                .contains("PARTIAL BUILD LOG")
        );
        fs::remove_file(saved).unwrap();
        assert_eq!(job.parent(), Some(temp.as_path()));
        fs::remove_dir(job).unwrap();
    }
    #[test]
    fn invalid_retained_location_falls_back_to_buffered_log() {
        let files = BuildFiles {
            job: std::env::temp_dir().join("unrelated"),
            log: std::env::temp_dir().join("outside.log"),
        };
        let export = log_export(Some(&files), &["buffered entry".into()]);
        assert!(export.partial_reason.is_some());
        assert!(
            String::from_utf8(export.bytes)
                .unwrap()
                .contains("buffered entry")
        );
    }
}
