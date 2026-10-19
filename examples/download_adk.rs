//! Download/signature smoke check. Does not install or request elevation.
use anyhow::{Context, Result};
use macbook_iso_patcher::prerequisites;
use std::path::PathBuf;

fn main() -> Result<()> {
    let cache = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("Usage: download_adk <cache-folder>")?,
    );
    let installer = prerequisites::download_adk(&cache, &|status| println!("{status}"))?;
    println!("Verified installer: {}", installer.display());
    Ok(())
}
