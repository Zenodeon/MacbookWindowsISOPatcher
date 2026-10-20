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
        "a1708-2016" => Ok(&[APPLE_2016_13]),
        "a1707-2016" => Ok(&[APPLE_2016_15]),
        "a1706-2017" => Ok(&[APPLE_2017_13]),
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
    fn models_receive_their_own_apple_packages_and_only_a1706_gets_the_iris_fix() {
        for model in crate::models::MODEL_PROFILES {
            assert!(!packages_for_model(model.id).unwrap().is_empty());
        }
        let a1706 = packages_for_model(MODEL_ID).unwrap();
        let a1708 = packages_for_model("a1708-2016").unwrap();
        assert_eq!(a1706.len(), 2);
        assert_eq!(a1708.len(), 1);
        assert_eq!(a1706[1].url, a1708[0].url);
        assert!(
            packages_for_model("a1707-2016").unwrap()[0]
                .filename
                .contains("041-88737")
        );
        assert!(
            packages_for_model("a1706-2017").unwrap()[0]
                .filename
                .contains("041-89042")
        );
        assert!(packages_for_model("unknown").is_err());
    }
}
