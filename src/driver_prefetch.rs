//! Packages selected by exact Mac model, based on Apple's Boot Camp distributions.
use crate::{
    domain::{Cancellation, Emit, MODEL_ID},
    download,
};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub struct DriverPackage {
    pub filename: &'static str,
    pub url: &'static str,
    pub hash: &'static str,
}
const APPLE_2016_13: DriverPackage = DriverPackage {
    filename: "BootCampESD-041-88734.pkg",
    url: download::APPLE_URL,
    hash: download::APPLE_SHA256,
};
const INTEL: DriverPackage = DriverPackage {
    filename: "win64_15.45.5174.exe",
    url: download::INTEL_URL,
    hash: download::INTEL_SHA256,
};
const INTEL_KABY: DriverPackage = DriverPackage {
    filename: "intel-27.20.100.9316.cab",
    url: "https://catalog.s.download.windowsupdate.com/c/msdownload/update/driver/drvs/2021/05/8c3b1650-0aef-4e61-b7f3-eaff91e3482a_504a1ab0935cace6f2359d8f857182fb6e23fb1f.cab",
    hash: "bac61afa36281c4bc68c78cdacf4192b7f04fba7d3a7c5a1fa08828ccb49f974",
};

pub struct IntelGraphics {
    pub device_id: &'static str,
    pub version: &'static str,
    pub inf: &'static str,
    pub catalog: &'static str,
}

pub fn intel_graphics(model: &str) -> Result<IntelGraphics> {
    let device_id = match model {
        MODEL_ID => "1927",
        "a1708-2016" => "1926",
        "a1706-2017" => "5927",
        _ => bail!("No Intel graphics profile exists for this Mac model"),
    };
    let kaby_lake = model == "a1706-2017";
    Ok(IntelGraphics {
        device_id,
        version: if kaby_lake {
            "27.20.100.9316"
        } else {
            "21.20.16.5174"
        },
        inf: if kaby_lake {
            "iigd_dch.inf"
        } else {
            "igdlh64.inf"
        },
        catalog: "igdlh.cat",
    })
}
const APPLE_2016_15: DriverPackage = DriverPackage {
    filename: "BootCampESD-041-88737.pkg",
    url: "https://swcdn.apple.com/content/downloads/25/38/041-88737-A_IXJZSM8OLV/fq3xy3duoq3xqp9o2sa8dkjtz5wvjaojhq/BootCampESD.pkg",
    hash: "782b00a1dbaf5950605909620f6f70d0d12375da8ce914d73d33ab57a9d1072c",
};
const APPLE_2017_13: DriverPackage = DriverPackage {
    filename: "BootCampESD-041-89042.pkg",
    url: "https://swcdn.apple.com/content/downloads/32/09/041-89042-A_XVZ2U8XKG2/06ub1qfep6wv3g8bb68smwt3ac25xyng83/BootCampESD.pkg",
    hash: "6e1213a2a8abbc7815f342010f4e9c14aaaab27c2086d1d3fc1a5f1b41859bc9",
};

pub fn packages_for_model(model: &str) -> Result<&'static [DriverPackage]> {
    match model {
        MODEL_ID => Ok(&[INTEL, APPLE_2016_13]),
        "a1708-2016" => Ok(&[INTEL, APPLE_2016_13]),
        "a1707-2016" => Ok(&[APPLE_2016_15]),
        "a1706-2017" => Ok(&[INTEL_KABY, APPLE_2017_13]),
        _ => bail!("No driver package manifest exists for this Mac model"),
    }
}
pub fn download_for_model(
    model: &str,
    cache: &Path,
    emit: &Emit,
    cancel: &Cancellation,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for package in packages_for_model(model)? {
        cancel.check()?;
        let path = download::fetch(
            cache,
            package.filename,
            package.url,
            package.hash,
            emit,
            cancel,
        )?;
        paths.push(path);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn models_receive_their_own_apple_and_compatible_intel_packages() {
        for model in crate::models::MODEL_PROFILES {
            let packages = packages_for_model(model.id).unwrap();
            assert_eq!(
                packages
                    .iter()
                    .filter(|p| p.filename.ends_with(".pkg"))
                    .count(),
                1
            );
        }
        let a1706 = packages_for_model(MODEL_ID).unwrap();
        let a1708 = packages_for_model("a1708-2016").unwrap();
        assert_eq!(a1706[0].hash, a1708[0].hash);
        assert_eq!(a1706[1].url, a1708[1].url);
        assert_eq!(
            packages_for_model("a1706-2017").unwrap()[0].hash,
            INTEL_KABY.hash
        );
        assert_eq!(intel_graphics("a1708-2016").unwrap().device_id, "1926");
        assert_eq!(intel_graphics("a1706-2017").unwrap().device_id, "5927");
        assert!(packages_for_model("unknown").is_err());
    }
    #[test]
    fn dual_gpu_model_uses_radeon_for_external_displays_without_an_intel_replacement() {
        let packages = packages_for_model("a1707-2016").unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].hash, APPLE_2016_15.hash);
        assert!(intel_graphics("a1707-2016").is_err());
    }
}
