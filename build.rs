use std::{env, path::PathBuf};
fn main() {
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        && output.status.success()
    {
        println!(
            "cargo:rustc-env=PATCHER_CODE_REVISION={}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
    }
    println!("cargo:rerun-if-changed=.git/HEAD");
    if let Ok(output) = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .output()
        && output.status.success()
    {
        println!(
            "cargo:rustc-env=PATCHER_CODE_DIRTY={}",
            !output.stdout.is_empty()
        );
    }
    if let Ok(head) = std::fs::read_to_string(".git/HEAD")
        && let Some(reference) = head.trim().strip_prefix("ref: ")
    {
        println!("cargo:rerun-if-changed=.git/{reference}");
    }
    println!("cargo:rerun-if-changed=payloads/a1706");
    println!("cargo:rerun-if-changed=app.manifest");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let compiler = cc::Build::new().get_compiler();
    for (name, windows, libs) in [
        ("a1706iris", false, vec![]),
        ("a1706cue", false, vec!["user32.lib", "winmm.lib"]),
        ("a1706gpu", false, vec!["cfgmgr32.lib"]),
        ("a1706gpu2017", false, vec!["cfgmgr32.lib", "advapi32.lib"]),
        ("gpu2017_test", false, vec!["cfgmgr32.lib", "advapi32.lib"]),
        (
            "MacBookSetupLogger",
            false,
            vec!["user32.lib", "advapi32.lib", "ole32.lib", "cfgmgr32.lib"],
        ),
        (
            "diagnostics_test",
            false,
            vec!["user32.lib", "advapi32.lib", "ole32.lib", "cfgmgr32.lib"],
        ),
        ("gpu_test", false, vec!["cfgmgr32.lib"]),
    ] {
        let mut cmd = compiler.to_command();
        cmd.args(["/nologo", "/O1", "/MT", "/W3"])
            .arg(format!("/Fo{}", out.join(format!("{name}.obj")).display()))
            .arg(format!("/Fe{}", out.join(format!("{name}.exe")).display()))
            .arg(format!("payloads/a1706/{name}.c"))
            .arg("/link");
        if windows {
            cmd.arg("/SUBSYSTEM:WINDOWS");
        }
        assert!(
            cmd.args(libs)
                .status()
                .expect("MSVC is required for WinPE helpers")
                .success(),
            "Could not compile {name}"
        );
    }
    let mut res = winresource::WindowsResource::new();
    res.set_manifest_file("app.manifest");
    res.set("ProductName", "MacBook ISO Patcher");
    let display_version = format!("{}v", env!("CARGO_PKG_VERSION"));
    res.set("ProductVersion", &display_version);
    res.set("FileVersion", &display_version);
    res.compile().expect("Windows resource compilation failed");
}
