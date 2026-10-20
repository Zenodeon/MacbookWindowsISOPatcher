use macbook_iso_patcher::{
    domain::{Cancellation, Emit},
    download, engine,
};
use std::{path::PathBuf, sync::Arc};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(
        args.len() == 2,
        "Usage: prepare_drivers <cache-folder> <7z.exe>"
    );
    let emit: Emit = Arc::new(|e| println!("{}", serde_json::to_string(&e).unwrap()));
    let assets = download::prepare(
        &PathBuf::from(&args[0]),
        &PathBuf::from(&args[1]),
        &emit,
        &Cancellation::default(),
    )?;
    let intel = assets.intel.as_ref().expect("A1706 graphics package");
    engine::validate_intel(intel)?;
    println!(
        "Intel: {}\nBoot Camp: {}\nDrivers: {}",
        intel.display(),
        assets.bootcamp.display(),
        assets.drivers.display()
    );
    let extraction = assets
        .drivers
        .ancestors()
        .find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("extract-"))
        })
        .ok_or_else(|| anyhow::anyhow!("Extraction root missing"))?;
    download::remove_extraction(&PathBuf::from(&args[0]), extraction)?;
    println!("Verified driver extraction completed; temporary extraction files removed.");
    Ok(())
}
