//! Pinned Apple 041-88737 Radeon package, selected Polaris11 Windows 10 sections.
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
pub const INF: &str = "C0307252.inf";
pub const CAT: &str = "C0307252.CAT";
pub const VERSION: &str = "21.19.128.7";
pub const IDS: [&str; 3] = [
    r"PCI\VEN_1002&DEV_67EF&SUBSYS_0160106B&REV_C0",
    r"PCI\VEN_1002&DEV_67EF&SUBSYS_0166106B&REV_C7",
    r"PCI\VEN_1002&DEV_67EF&SUBSYS_0167106B&REV_EF",
];
fn text(path: &Path) -> Result<String> {
    let b = fs::read(path)?;
    if b.starts_with(&[255, 254]) {
        ensure!(b.len().is_multiple_of(2), "Truncated UTF-16 Radeon INF");
        Ok(String::from_utf16(
            &b[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        )?)
    } else {
        Ok(String::from_utf8(b)?)
    }
}
fn sections(inf: &str) -> BTreeMap<String, Vec<String>> {
    let mut all = BTreeMap::<String, Vec<String>>::new();
    let mut current = String::new();
    for line in inf.lines() {
        let l = line
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if l.starts_with('[') && l.ends_with(']') {
            current = l[1..l.len() - 1].into();
        } else if !l.is_empty() {
            all.entry(current.clone()).or_default().push(l);
        }
    }
    all
}
fn value<'a>(
    sections: &'a BTreeMap<String, Vec<String>>,
    section: &str,
    key: &str,
) -> Result<&'a str> {
    sections
        .get(section)
        .and_then(|ls| {
            ls.iter()
                .filter_map(|l| l.split_once('='))
                .find(|(k, _)| k.trim() == key)
                .map(|(_, v)| v.trim())
        })
        .with_context(|| format!("Radeon INF missing {section}/{key}"))
}
/// Return only files used by this GPU's install/coinstaller sections, not all source media.
pub fn required_files(inf: &str) -> Result<Vec<String>> {
    let all = sections(inf);
    ensure!(
        value(&all, "version", "driverver")?
            .rsplit(',')
            .next()
            .is_some_and(|v| v.trim() == VERSION),
        "Unexpected Radeon version"
    );
    ensure!(
        value(&all, "version", "catalogfile")? == CAT.to_ascii_lowercase(),
        "Unexpected Radeon catalog"
    );
    for id in IDS {
        ensure!(
            all.get("ati.mfg.ntamd64.10.0...14310").is_some_and(|ls| ls
                .iter()
                .any(|l| l.split_once('=').is_some_and(|(_, v)| v.trim()
                    == format!("ati2mtag_polaris11ds, {}", id.to_ascii_lowercase())))),
            "Radeon variant missing: {id}"
        );
    }
    for (section, kernel) in [
        ("ati2mtag_proxy_ds", "atikmpag.sys"),
        ("ati2mtag_kmd_ds", "atikmdag.sys"),
    ] {
        for (key, expected) in [
            ("servicetype", "1"),
            ("starttype", "3"),
            ("errorcontrol", "0"),
            ("loadordergroup", "video"),
        ] {
            ensure!(
                value(&all, section, key)? == expected,
                "Unexpected Radeon service configuration"
            );
        }
        ensure!(
            value(&all, section, "servicebinary")? == format!(r"%13%\{kernel}"),
            "Radeon kernel location changed"
        );
    }
    ensure!(
        all.get("ati2mtag_servicesettings")
            .is_some_and(|ls| ls == &["hkr,, powerxpressload, %reg_dword%, 0xffffffff"]),
        "Radeon service registry settings changed"
    );
    let services = all
        .get("ati2mtag_polaris11ds.services")
        .context("Radeon services missing")?;
    ensure!(
        services.iter().any(
            |l| l == "addservice = amdkmdap, 0x00000002, ati2mtag_proxy_ds, r300_eventlog_inst"
        ) && services
            .iter()
            .any(|l| l == "addservice = amdkmdag, 0x00000000, ati2mtag_kmd_ds, r300_eventlog_inst"),
        "Radeon device binding changed"
    );
    let mut files = BTreeSet::new();
    for section in ["ati2mtag_polaris11ds", "ati2mtag_polaris11ds.coinstallers"] {
        let lines = all.get(section).context("Radeon install section missing")?;
        for line in lines {
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            ensure!(
                !matches!(key.trim(), "include" | "needs"),
                "Radeon INF dependencies changed"
            );
            if key.trim() != "copyfiles" {
                continue;
            }
            for group in val.split(',').map(str::trim) {
                let entries = all
                    .get(group)
                    .with_context(|| format!("Radeon CopyFiles section missing: {group}"))?;
                for entry in entries {
                    let fields: Vec<_> = entry.split(',').map(str::trim).collect();
                    let source = fields
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .copied()
                        .unwrap_or(fields[0]);
                    ensure!(
                        !source.is_empty() && !source.contains(['/', '\\', ':']) && source != "..",
                        "Invalid Radeon copy filename"
                    );
                    files.insert(source.to_owned());
                }
            }
        }
    }
    ensure!(
        files.contains("atikmdag.sys") && files.contains("atikmpag.sys"),
        "Radeon kernel CopyFiles missing"
    );
    Ok(files.into_iter().collect())
}
fn unique_file(root: &Path, name: &str) -> Result<PathBuf> {
    let mut matches = vec![];
    for entry in WalkDir::new(root) {
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "Redirected Radeon package path"
        );
        if entry.file_type().is_file() && entry.file_name().eq_ignore_ascii_case(name) {
            matches.push(entry.into_path());
        }
    }
    ensure!(
        matches.len() == 1,
        "Radeon file missing or ambiguous: {name}"
    );
    Ok(matches.remove(0))
}
pub fn package(root: &Path) -> Result<PathBuf> {
    let inf = unique_file(root, INF)?;
    let dir = inf.parent().unwrap();
    let required = required_files(&text(&inf)?)?;
    for name in required {
        unique_file(dir, &name)?;
    }
    let cat = dir.join(CAT);
    ensure!(cat.is_file(), "Radeon signed catalog missing");
    crate::platform::powershell(
        r#"if((Get-AuthenticodeSignature -LiteralPath $env:PATCHER_CAT).Status -ne 'Valid'){throw 'Radeon catalog signature is invalid'}"#,
        &[("PATCHER_CAT", cat.as_os_str())],
    )?;
    Ok(inf)
}
/// Locate actual serviced paths instead of assuming the source-media B* directory layout.
pub fn store(mount: &Path, inf: &Path) -> Result<PathBuf> {
    let source = inf.parent().context("Radeon source directory missing")?;
    let expected = fs::read(inf)?;
    let mut stores = vec![];
    for entry in WalkDir::new(mount.join("Windows/System32/DriverStore/FileRepository")) {
        let entry = entry?;
        if entry.file_type().is_file()
            && entry.file_name().eq_ignore_ascii_case(INF)
            && fs::read(entry.path())? == expected
        {
            stores.push(entry.path().parent().unwrap().to_owned());
        }
    }
    ensure!(
        stores.len() == 1,
        "Pinned Radeon DriverStore missing or ambiguous"
    );
    let dir = stores.remove(0);
    for name in required_files(&text(inf)?)?
        .into_iter()
        .chain([CAT.to_owned()])
    {
        let installed = unique_file(&dir, &name)?;
        let original = unique_file(source, &name)?;
        ensure!(
            fs::read(installed)? == fs::read(original)?,
            "Radeon DriverStore file changed: {name}"
        );
    }
    Ok(dir)
}
pub fn kernels(store: &Path) -> Result<[PathBuf; 2]> {
    Ok([
        unique_file(store, "atikmpag.sys")?,
        unique_file(store, "atikmdag.sys")?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = include_str!("../tests/fixtures/radeon-polaris11-selected.inf");
    fn fixture() -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("radeon-store-{}", uuid::Uuid::new_v4()));
        let src = root.join("source");
        let store = root
            .join("image/Windows/System32/DriverStore/FileRepository/c0307252.inf_amd64_fixture");
        for dir in [&src, &store] {
            fs::create_dir_all(dir.join("B307184")).unwrap();
            fs::write(dir.join(INF), FIXTURE).unwrap();
            fs::write(dir.join(CAT), b"fixture catalog").unwrap();
            for name in required_files(FIXTURE).unwrap() {
                fs::write(dir.join("B307184").join(&name), name.as_bytes()).unwrap();
            }
        }
        (root, src, store)
    }
    #[test]
    fn pinned_radeon_sections_cover_variants_and_service_dependencies() {
        let helper = include_str!("../payloads/a1706/MacBookSetupGPU.c");
        for id in IDS {
            assert!(helper.contains(&id.replace('\\', r"\\")));
        }
        assert!(helper.contains(VERSION));
        let files = required_files(FIXTURE).unwrap();
        assert!(files.len() > 40);
        assert!(files.iter().any(|f| f == "atikmpag.sys"));
        assert!(files.iter().any(|f| f == "atikmdag.sys"));
        for (from, to) in [
            ("21.19.128.7", "99.0"),
            ("SUBSYS_0166106B", "SUBSYS_00000000"),
            (
                "ServiceBinary  = %13%\\atikmpag.sys",
                "ServiceBinary  = %12%\\atikmpag.sys",
            ),
            ("PowerXpressLoad", "WrongSetting"),
            (
                "0x00000002, ati2mtag_Proxy_DS",
                "0x00000000, ati2mtag_Proxy_DS",
            ),
        ] {
            assert_ne!(FIXTURE, FIXTURE.replace(from, to));
            assert!(
                required_files(&FIXTURE.replace(from, to)).is_err(),
                "{from}"
            );
        }
    }
    #[test]
    fn serviced_files_follow_copyfiles_and_actual_kernel_locations() {
        let (root, src, dir) = fixture();
        let image = root.join("image");
        fs::write(src.join("unused-firmware.dll"), b"unused").unwrap();
        assert_eq!(store(&image, &src.join(INF)).unwrap(), dir);
        assert!(
            kernels(&dir)
                .unwrap()
                .iter()
                .all(|p| p.parent() == Some(dir.join("B307184").as_path()))
        );
        let kernel = dir.join("B307184/atikmpag.sys");
        fs::write(&kernel, b"altered").unwrap();
        assert!(store(&image, &src.join(INF)).is_err());
        fs::remove_file(kernel).unwrap();
        assert!(store(&image, &src.join(INF)).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "Requires extracted pinned Apple Radeon package; no image servicing"]
    fn validate_real_pinned_package() {
        let root = PathBuf::from(
            std::env::var_os("MACBOOK_RADEON_PACKAGE").expect("Provide AMDGraphics root"),
        );
        let inf = package(&root).unwrap();
        println!(
            "Validated signed Radeon package: {} ({} required files)",
            inf.display(),
            required_files(&text(&inf).unwrap()).unwrap().len()
        );
    }
}
