use macbook_iso_patcher::platform;
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("patcher-media-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("sources")).unwrap();
        fs::write(
            root.join("input.iso"),
            b"synthetic test fixture; not bootable",
        )
        .unwrap();
        Self(root)
    }
    fn iso(&self) -> PathBuf {
        self.0.join("input.iso")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn image(index: u32, name: &str, edition: &str, build: u32) -> String {
    format!(
        r#"<IMAGE INDEX="{index}"><NAME>{name}</NAME><WINDOWS><ARCH>9</ARCH><EDITIONID>{edition}</EDITIONID><INSTALLATIONTYPE>Client</INSTALLATIONTYPE><VERSION><MAJOR>10</MAJOR><MINOR>0</MINOR><BUILD>{build}</BUILD></VERSION><LANGUAGES><DEFAULT>en-US</DEFAULT></LANGUAGES></WINDOWS></IMAGE>"#
    )
}
fn wim(path: &Path, xml: &str) {
    let xml: Vec<u8> = xml.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut bytes = vec![0u8; 208];
    bytes[..8].copy_from_slice(b"MSWIM\0\0\0");
    bytes[72..80].copy_from_slice(&(xml.len() as u64).to_le_bytes());
    bytes[80..88].copy_from_slice(&208u64.to_le_bytes());
    bytes[88..96].copy_from_slice(&(xml.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&xml);
    fs::write(path, bytes).unwrap();
}
#[test]
fn scans_wim_and_esd_without_assuming_edition_indices() {
    for name in ["install.wim", "install.esd"] {
        let fixture = Fixture::new();
        wim(
            &fixture.0.join("sources/boot.wim"),
            &format!("<WIM>{}</WIM>", image(2, "Setup", "", 19041)),
        );
        wim(
            &fixture.0.join("sources").join(name),
            &format!(
                "<WIM>{}{}</WIM>",
                image(2, "Windows 10 Home", "Core", 19041),
                image(8, "Windows 10 Pro", "Professional", 19041)
            ),
        );
        let before = fs::read(fixture.iso()).unwrap();
        let inventory = platform::scan_root(&fixture.iso(), &fixture.0).unwrap();
        assert_eq!(inventory.install_file, name);
        assert_eq!(inventory.images.len(), 2);
        assert_eq!(inventory.images[0].index, 2);
        assert_eq!(inventory.images[1].index, 8);
        assert!(inventory.images[0].unsupported_reason().is_none());
        assert_eq!(before, fs::read(fixture.iso()).unwrap());
    }
}
#[test]
fn rejects_custom_answer_files_and_newer_setup() {
    let fixture = Fixture::new();
    wim(
        &fixture.0.join("sources/boot.wim"),
        &format!("<WIM>{}</WIM>", image(2, "Setup", "", 22621)),
    );
    wim(
        &fixture.0.join("sources/install.wim"),
        &format!("<WIM>{}</WIM>", image(1, "Pro", "Professional", 22621)),
    );
    assert!(platform::scan_root(&fixture.iso(), &fixture.0).is_err());
    wim(
        &fixture.0.join("sources/boot.wim"),
        &format!("<WIM>{}</WIM>", image(2, "Setup", "", 19041)),
    );
    fs::write(fixture.0.join("autounattend.xml"), "<unattend/>").unwrap();
    assert!(platform::scan_root(&fixture.iso(), &fixture.0).is_err());
}
