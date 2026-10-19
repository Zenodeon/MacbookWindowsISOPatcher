//! Read WIM/ESD metadata without servicing an image or requiring elevation.
use crate::domain::Image;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
#[derive(Debug, Deserialize)]
struct Wim {
    #[serde(rename = "IMAGE", default)]
    images: Vec<XmlImage>,
}
#[derive(Debug, Deserialize)]
struct XmlImage {
    #[serde(rename = "@INDEX")]
    index: u32,
    #[serde(rename = "NAME", default)]
    name: String,
    #[serde(rename = "FLAGS", default)]
    flags: String,
    #[serde(rename = "TOTALBYTES", default)]
    size: u64,
    #[serde(rename = "WINDOWS")]
    windows: Option<Windows>,
}
#[derive(Debug, Deserialize, Default)]
struct Windows {
    #[serde(rename = "ARCH", default)]
    arch: u32,
    #[serde(rename = "EDITIONID", default)]
    edition: String,
    #[serde(rename = "INSTALLATIONTYPE", default)]
    installation: String,
    #[serde(rename = "VERSION", default)]
    version: Version,
    #[serde(rename = "LANGUAGES", default)]
    languages: Languages,
}
#[derive(Debug, Deserialize, Default)]
struct Version {
    #[serde(rename = "MAJOR", default)]
    major: u32,
    #[serde(rename = "MINOR", default)]
    minor: u32,
    #[serde(rename = "BUILD", default)]
    build: u32,
}
#[derive(Debug, Deserialize, Default)]
struct Languages {
    #[serde(rename = "DEFAULT", default)]
    default: String,
}
pub fn parse_xml(xml: &str) -> Result<Vec<Image>> {
    let metadata: Wim = quick_xml::de::from_str(xml.trim_start_matches('\u{feff}'))
        .context("Invalid WIM XML metadata")?;
    ensure!(
        !metadata.images.is_empty(),
        "The Windows image contains no editions"
    );
    let mut result = Vec::new();
    for i in metadata.images {
        ensure!(
            i.index > 0 && !result.iter().any(|v: &Image| v.index == i.index),
            "Invalid/duplicate WIM image index"
        );
        let w = i.windows.unwrap_or_default();
        result.push(Image {
            index: i.index,
            name: i.name,
            edition: if w.edition.is_empty() {
                i.flags
            } else {
                w.edition
            },
            arch: w.arch,
            build: w.version.build,
            major: w.version.major,
            minor: w.version.minor,
            language: w.languages.default,
            installation_type: w.installation,
            size: i.size,
        });
    }
    Ok(result)
}
pub fn read_images(path: &Path) -> Result<Vec<Image>> {
    let mut f = File::open(path).with_context(|| format!("Cannot read {}", path.display()))?;
    let len = f.metadata()?.len();
    let mut header = [0u8; 208];
    f.read_exact(&mut header)?;
    ensure!(&header[..8] == b"MSWIM\0\0\0", "Not a WIM/ESD image");
    let size = u64::from_le_bytes(header[72..80].try_into()?) & 0x00ff_ffff_ffff_ffff;
    let offset = u64::from_le_bytes(header[80..88].try_into()?);
    let original = u64::from_le_bytes(header[88..96].try_into()?);
    ensure!(size == original, "Compressed WIM XML is unsupported");
    ensure!(
        (2..=16 * 1024 * 1024).contains(&size),
        "WIM XML resource size is invalid"
    );
    ensure!(
        offset >= 208 && offset.checked_add(size).is_some_and(|end| end <= len),
        "WIM metadata points outside the file"
    );
    f.seek(SeekFrom::Start(offset))?;
    let mut raw = vec![0u8; size as usize];
    f.read_exact(&mut raw)?;
    ensure!(raw.len().is_multiple_of(2), "WIM XML is not valid UTF-16LE");
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect();
    parse_xml(
        String::from_utf16(&units)
            .context("Invalid UTF-16 WIM metadata")?
            .trim_end_matches('\0'),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_nonstandard_indices_and_entities() {
        let xml = r#"<WIM><IMAGE INDEX="3"><NAME>Home &amp; more</NAME><FLAGS>Core</FLAGS><TOTALBYTES>15000000000</TOTALBYTES><WINDOWS><ARCH>9</ARCH><EDITIONID>Core</EDITIONID><INSTALLATIONTYPE>Client</INSTALLATIONTYPE><LANGUAGES><LANGUAGE>en-US</LANGUAGE><DEFAULT>en-US</DEFAULT></LANGUAGES><VERSION><MAJOR>10</MAJOR><MINOR>0</MINOR><BUILD>19045</BUILD></VERSION></WINDOWS></IMAGE><IMAGE INDEX="9"><NAME>Pro</NAME><FLAGS>Professional</FLAGS></IMAGE></WIM>"#;
        let images = parse_xml(xml).unwrap();
        assert_eq!(images[0].index, 3);
        assert_eq!(images[0].name, "Home & more");
        assert!(images[0].unsupported_reason().is_none());
        assert_eq!(images[1].index, 9);
        assert!(images[1].unsupported_reason().is_some());
    }
    #[test]
    fn rejects_duplicate_indices() {
        assert!(parse_xml(r#"<WIM><IMAGE INDEX="1"/><IMAGE INDEX="1"/></WIM>"#).is_err());
    }
    #[test]
    fn reads_binary_header_and_rejects_out_of_bounds_xml() {
        let path = std::env::temp_dir().join(format!("wim-test-{}.wim", uuid::Uuid::new_v4()));
        let xml: Vec<u8> = r#"<WIM><IMAGE INDEX="7"><NAME>Test</NAME></IMAGE></WIM>"#
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut bytes = vec![0u8; 208];
        bytes[..8].copy_from_slice(b"MSWIM\0\0\0");
        bytes[72..80].copy_from_slice(&(xml.len() as u64).to_le_bytes());
        bytes[80..88].copy_from_slice(&208u64.to_le_bytes());
        bytes[88..96].copy_from_slice(&(xml.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&xml);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_images(&path).unwrap()[0].index, 7);
        bytes[80..88].copy_from_slice(&u64::MAX.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        assert!(read_images(&path).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
