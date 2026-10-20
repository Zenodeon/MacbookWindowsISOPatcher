//! Debug-only, screenshot-only fixtures. Never present in release builds.
use super::*;

impl PatcherApp {
    pub(super) fn configure_preview(&mut self) {
        if std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_none() {
            return;
        }
        let Ok(view) = std::env::var("MACBOOK_PATCHER_PREVIEW") else {
            return;
        };
        if view == "mounted-close" {
            // Used only by the opt-in Windows lifecycle test with its own data ISO.
            let iso = PathBuf::from(
                std::env::var_os("MACBOOK_PATCHER_PREVIEW_ISO").expect("Lifecycle test ISO"),
            );
            self.iso_mount =
                Some(platform::MountedIso::open(&iso).expect("Mount lifecycle test ISO"));
            self.iso = iso.display().to_string();
            self.model_selected = true;
            self.page = Page::Iso;
            return;
        }
        if view == "models" {
            return;
        }
        self.model_selected = true;
        if view == "iso" {
            self.page = Page::Iso;
            self.iso = "C:\\Preview\\Windows10_English_x64.iso".into();
            return;
        }
        if !["editions", "review-ready", "review-missing", "dismount"].contains(&view.as_str()) {
            return;
        }
        self.iso = "C:\\Preview\\Windows10_English_x64.iso".into();
        self.output = "C:\\Preview\\Win10_A1706_Patched.iso".into();
        self.settings_file = std::env::temp_dir().join("macbook-patcher-preview-settings.json");
        self.inventory = Some(Inventory {
            iso: self.iso.clone().into(),
            iso_size: 5 * GIB,
            iso_modified: 0,
            install_file: "sources/install.wim".into(),
            images: [
                ("Windows 10 Pro", "Professional"),
                ("Windows 10 Home", "Core"),
                ("Windows 10 Education", "Education"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (name, edition))| Image {
                index: i as u32 + 1,
                name: name.into(),
                edition: edition.into(),
                arch: 9,
                build: 19045,
                major: 10,
                minor: 0,
                language: "en-US".into(),
                installation_type: "Client".into(),
                size: 14 * GIB,
            })
            .collect(),
        });
        self.selected.insert(1);
        self.page = if view.starts_with("review") {
            Page::Review
        } else {
            Page::Editions
        };
        if view == "review-ready" {
            // Existing fixture paths drive the UI's file-presence state only.
            // The fake ISO cannot pass build preflight; release has no fixtures.
            let existing = std::env::current_exe().unwrap().display().to_string();
            self.settings.oscdimg = existing.clone();
            self.settings.seven_zip = existing;
        } else if view == "review-missing" {
            self.settings.oscdimg.clear();
            self.settings.seven_zip.clear();
        }
        self.confirm_dismount = view == "dismount";
    }
}
