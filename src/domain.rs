use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
pub const DONATION_URL: &str = "https://ko-fi.com/zenodeon";
pub const MODEL_ID: &str = "a1706";
pub const MODEL_NAME: &str = "MacBook Pro 13-inch (2016, Touch Bar)";
pub const HARDWARE_ID: &str = r"PCI\VEN_8086&DEV_1927&SUBSYS_015D106B";
pub const GIB: u64 = 1024 * 1024 * 1024;
pub const BOOT_ALLOWLIST: &[&str] = &[
    "AppleBluetoothBroadcom64",
    "AppleCamera64",
    "AppleDFR64",
    "AppleDisplayInstaller64",
    "AppleKeyboardInstaller64",
    "AppleMultiTouchTrackPadInstaller64",
    "AppleMultiTouchTrackPadProInstaller64",
    "AppleNullDriver64",
    "AppleODDInstaller64",
    "AppleSOC64",
    "AppleSPIDevice",
    "AppleSPIKeyboard",
    "AppleSPITrackpad",
    "AppleSSD64",
    "AppleUSBCompositeDevice64",
    "AppleWirelessMouse64",
    "AppleWirelessTrackpad64",
    "AsixSetup64",
    "BroadcomBluetooth64",
    "BroadcomBluetoothHID64",
    "BroadcomCardReader64",
    "BroadcomComController64",
    "BroadcomEthernet64",
    "BroadcomWirelessWin8x64",
    "CirrusAudioCS4208x64",
    "IntelEthernetInstaller64",
    "SerialIO",
];
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Image {
    pub index: u32,
    pub name: String,
    pub edition: String,
    pub arch: u32,
    pub build: u32,
    pub major: u32,
    pub minor: u32,
    pub language: String,
    pub installation_type: String,
    pub size: u64,
}
impl Image {
    pub fn unsupported_reason(&self) -> Option<String> {
        if self.arch != 9 {
            return Some("Requires x64 (AMD64) media".into());
        }
        if self.major != 10 || self.minor != 0 || !(19041..=19045).contains(&self.build) {
            return Some(format!(
                "Build {} is outside Windows 10 19041–19045",
                self.build
            ));
        }
        if !self.language.eq_ignore_ascii_case("en-US") {
            return Some("This profile requires English (United States)".into());
        }
        if !self.installation_type.eq_ignore_ascii_case("Client") {
            return Some("Windows client editions only".into());
        }
        if self.edition.is_empty() {
            return Some("Edition metadata is missing".into());
        }
        None
    }
    pub fn source_tested(&self) -> bool {
        self.edition == "Professional"
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub iso: PathBuf,
    pub iso_size: u64,
    pub iso_modified: u64,
    pub install_file: String,
    pub images: Vec<Image>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRequest {
    pub inventory: Inventory,
    pub selected: Vec<u32>,
    pub model: String,
    pub output: PathBuf,
    pub workspace: PathBuf,
    pub cache: PathBuf,
    pub seven_zip: PathBuf,
    pub oscdimg: PathBuf,
}
impl BuildRequest {
    pub fn selected_images(&self) -> Result<Vec<&Image>> {
        ensure!(
            crate::models::MODEL_PROFILES
                .iter()
                .any(|model| model.id == self.model),
            "Unsupported model profile"
        );
        ensure!(!self.selected.is_empty(), "Select at least one edition");
        let mut result = Vec::new();
        for index in &self.selected {
            ensure!(
                !result.iter().any(|i: &&Image| i.index == *index),
                "Duplicate image selection"
            );
            let image = self
                .inventory
                .images
                .iter()
                .find(|i| i.index == *index)
                .ok_or_else(|| anyhow::anyhow!("Image {index} does not exist"))?;
            if let Some(reason) = image.unsupported_reason() {
                bail!("{}: {reason}", image.name);
            }
            result.push(image);
        }
        Ok(result)
    }
    pub fn required_space(&self) -> Result<u64> {
        let expanded: u64 = self.selected_images()?.iter().map(|i| i.size).sum();
        Ok(self
            .inventory
            .iso_size
            .saturating_mul(3)
            .saturating_add(expanded)
            .saturating_add(20 * GIB))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum Event {
    Stage {
        name: String,
        detail: String,
    },
    Download {
        name: String,
        received: u64,
        total: Option<u64>,
    },
    OperationProgress {
        operation: String,
        percent: Option<f32>,
    },
    OverallProgress {
        completed: u32,
        total: u32,
    },
    Log(String),
    Finished(PathBuf),
    Failed(String),
    Cancelled,
}
pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;
#[derive(Clone, Default)]
pub struct Cancellation(pub Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> Result<()> {
        ensure!(!self.is_cancelled(), "Build cancelled");
        Ok(())
    }
}
pub fn stage(emit: &Emit, name: &str, detail: impl Into<String>) {
    emit(Event::Stage {
        name: name.into(),
        detail: detail.into(),
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pro() -> Image {
        Image {
            index: 6,
            name: "Windows 10 Pro".into(),
            edition: "Professional".into(),
            arch: 9,
            build: 19041,
            major: 10,
            minor: 0,
            language: "en-US".into(),
            installation_type: "Client".into(),
            size: 15 * GIB,
        }
    }
    #[test]
    fn compatibility_is_metadata_based() {
        let mut i = pro();
        assert!(i.unsupported_reason().is_none());
        i.build = 22621;
        assert!(i.unsupported_reason().is_some());
        i.build = 19045;
        i.arch = 12;
        assert!(i.unsupported_reason().is_some());
        i.arch = 9;
        i.language = "de-DE".into();
        assert!(i.unsupported_reason().is_some());
    }
    #[test]
    fn home_is_allowed_but_unverified() {
        let mut i = pro();
        i.edition = "Core".into();
        assert!(i.unsupported_reason().is_none());
        assert!(!i.source_tested());
    }
    fn request() -> BuildRequest {
        BuildRequest {
            inventory: Inventory {
                iso: "C:/input.iso".into(),
                iso_size: 5 * GIB,
                iso_modified: 0,
                install_file: "install.esd".into(),
                images: vec![pro()],
            },
            selected: vec![6],
            model: MODEL_ID.into(),
            output: "C:/output.iso".into(),
            workspace: "C:/work".into(),
            cache: "C:/cache".into(),
            seven_zip: "C:/7z.exe".into(),
            oscdimg: "C:/oscdimg.exe".into(),
        }
    }
    #[test]
    fn validates_selections_and_preserves_source_indices() {
        let mut req = request();
        assert_eq!(req.selected_images().unwrap()[0].index, 6);
        req.selected = vec![1];
        assert!(req.selected_images().is_err());
        req.selected = vec![6, 6];
        assert!(req.selected_images().is_err());
        req.selected.clear();
        assert!(req.selected_images().is_err());
        req.selected = vec![6];
        req.model = "a1707".into();
        assert!(req.selected_images().is_err());
    }
    #[test]
    fn estimates_space_from_uncompressed_selected_images() {
        assert_eq!(request().required_space().unwrap(), 50 * GIB);
    }
    #[test]
    fn all_listed_models_can_build_without_becoming_hardware_verified() {
        let mut req = request();
        for model in crate::models::MODEL_PROFILES {
            req.model = model.id.into();
            assert_eq!(req.selected_images().unwrap()[0].index, 6);
            assert_eq!(model.hardware_verified(), model.id == MODEL_ID);
        }
        req.model = "unknown".into();
        assert!(req.selected_images().is_err());
    }
}
