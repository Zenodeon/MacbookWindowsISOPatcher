use std::{env, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-changed=payloads/a1706");
    println!("cargo:rerun-if-changed=app.manifest");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let compiler = cc::Build::new().get_compiler();
    for (name, windows, libs) in [
        ("a1706iris", false, vec![]),
        ("a1706dlg", true, vec!["user32.lib"]),
        ("a1706cue", false, vec!["user32.lib", "winmm.lib"]),
        ("a1706gpu", false, vec!["cfgmgr32.lib"]),
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
    res.compile().expect("Windows resource compilation failed");
}
