#![cfg(windows)]
use macbook_iso_patcher::platform;
use std::{fs, path::PathBuf, process::Command};

#[test]
fn gpu_production_loop_handles_delays_errors_and_ambiguity() {
    let output = Command::new(concat!(env!("OUT_DIR"), "/gpu_test.exe"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn native_collector_uses_safe_bounded_disposable_storage() {
    let root =
        std::env::temp_dir().join(format!("macbook-diagnostics-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("spool")).unwrap();
    fs::create_dir_all(root.join("outside")).unwrap();
    platform::powershell(
        "New-Item -ItemType Junction -Path $env:DIAG_LINK -Target $env:DIAG_TARGET | Out-Null",
        &[
            ("DIAG_LINK", root.join("spool/link").as_os_str()),
            ("DIAG_TARGET", root.join("outside").as_os_str()),
        ],
    )
    .unwrap();
    let output = Command::new(concat!(env!("OUT_DIR"), "/diagnostics_test.exe"))
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Fixture retained at {}\n{}\n{}",
        root.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let timeline = fs::read_to_string(root.join("spool/timeline.jsonl")).unwrap();
    let events: Vec<serde_json::Value> = timeline
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(events[0]["stage"], "wpeinit-start");
    assert_eq!(events[1]["result"], "0");
    assert!(
        events
            .iter()
            .all(|v| v["utc"].as_str().unwrap().ends_with('Z'))
    );
    assert!(!root.join("outside/escaped.txt").exists());
    let status = fs::read_to_string(root.join("spool/collection-status.jsonl")).unwrap();
    assert!(
        status.contains("read-error")
            && status.contains("write-error")
            && status.contains("unavailable")
            && status.contains("partial-last-16MB")
            && status.contains("spool-budget-exceeded")
    );
    if std::env::var_os("MACBOOK_DIAGNOSTICS_PREVIEW").is_some() {
        let preview = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".cache/diagnostics-preview/MacBookPatcher-Logs/A1706/Test-session");
        fs::create_dir_all(&preview).unwrap();
        for name in [
            "timeline.jsonl",
            "summary.txt",
            "hardware.txt",
            "volumes.txt",
            "collection-status.jsonl",
            "session-status.json",
        ] {
            fs::copy(root.join("spool").join(name), preview.join(name)).unwrap();
        }
        fs::write(preview.join("README.txt"),"Disposable integration-test session. Stages, failures and destinations were simulated; this is not a Mac installation log.").unwrap();
        let native = root.join("spool/windows-setup");
        for entry in walkdir::WalkDir::new(&native)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_file() {
                let destination = preview
                    .join("windows-setup")
                    .join(entry.path().strip_prefix(&native).unwrap());
                fs::create_dir_all(destination.parent().unwrap()).unwrap();
                fs::copy(entry.path(), destination).unwrap();
            }
        }
    }
    // Never recursively follow the junction during fixture cleanup.
    fs::remove_dir(root.join("spool/link")).unwrap();
    assert!(
        root.starts_with(std::env::temp_dir())
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("macbook-diagnostics-test-")
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn runtime_preserves_sequence_and_never_restarts_an_empty_id() {
    let s = include_str!("../payloads/a1706/a1706.cmd");
    assert!(s.find("--start").unwrap() < s.find("\nwpeinit").unwrap());
    assert!(
        s.find("if not defined GPU goto :gpufailed").unwrap() < s.find("call :gpurestart").unwrap()
    );
    assert!(s.find(":waitiris").unwrap() < s.find("set \"MSG=launching setup").unwrap());
    assert_eq!(s.matches("pnputil /restart-device \"!GPU!\"").count(), 1);
    assert!(!s.contains("A1706Logs") && !s.contains(":try\n") && !s.contains("3&11583659"));
    assert!(s.as_bytes().windows(2).any(|b| b == b"\r\n"));
    assert!(!s.as_bytes().contains(&0));
    assert!(s.contains("--stop") && s.contains("setup-exit"));
}
#[test]
fn iso_support_payload_contains_metadata_and_copyable_marker() {
    let helper = PathBuf::from(concat!(env!("OUT_DIR"), "/MacBookSetupLogger.exe"));
    assert!(helper.is_file());
    let bytes = include_bytes!("../payloads/a1706/Collect setup logs.cmd");
    assert!(!bytes.contains(&0));
    assert!(macbook_iso_patcher::diagnostics::INSTRUCTIONS.contains("lost at reboot"));
    let root =
        std::env::temp_dir().join(format!("macbook-diagnostics-test-{}", uuid::Uuid::new_v4()));
    let json = serde_json::json!({"inventory":{"iso":"C:\\Private User\\source.iso","iso_size":1,"iso_modified":0,"install_file":"install.wim","images":[{"index":1,"name":"Windows 10 Pro","edition":"Professional","arch":9,"build":19045,"revision":2965,"major":10,"minor":0,"language":"en-US","installation_type":"Client","size":1}]},"selected":[1],"model":"a1706","output":"C:\\out.iso","workspace":"C:\\workspace","cache":"C:\\cache","seven_zip":"C:\\7z.exe","oscdimg":"C:\\oscdimg.exe"});
    let req: macbook_iso_patcher::domain::BuildRequest = serde_json::from_value(json).unwrap();
    let dir = root.join("Support");
    macbook_iso_patcher::diagnostics::write_support(&dir, &req, &root.join("build-fixture"))
        .unwrap();
    let raw = fs::read_to_string(dir.join("build-manifest.json")).unwrap();
    assert!(!raw.contains("Private User") && !raw.contains("workspace"));
    let manifest: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(manifest["windows_images"][0]["revision"], 2965);
    assert_eq!(manifest["model_identifier"], "MacBookPro13,2");
    assert!(
        fs::read(dir.join("MacBookSetupLogger.exe"))
            .unwrap()
            .starts_with(b"MZ")
    );
    assert_eq!(
        fs::read(dir.join(macbook_iso_patcher::diagnostics::MARKER)).unwrap(),
        include_bytes!("../payloads/a1706/MacBookPatcher-Logging.enabled")
    );
    if std::env::var_os("MACBOOK_DIAGNOSTICS_PREVIEW").is_some() {
        let preview =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/diagnostics-preview/Support");
        fs::create_dir_all(&preview).unwrap();
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), preview.join(entry.file_name())).unwrap();
        }
        let session = preview
            .parent()
            .unwrap()
            .join("MacBookPatcher-Logs/A1706/Test-session");
        fs::create_dir_all(&session).unwrap();
        fs::copy(
            dir.join("build-manifest.json"),
            session.join("build-manifest.json"),
        )
        .unwrap();
    }
    assert!(root.starts_with(std::env::temp_dir()));
    fs::remove_dir_all(root).unwrap();
}
