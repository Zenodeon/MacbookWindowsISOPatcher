use macbook_iso_patcher::{
    domain::{Cancellation, Emit, MODEL_ID},
    download, engine,
};
use std::{path::PathBuf, sync::Arc};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    anyhow::ensure!(
        (2..=3).contains(&args.len()),
        "Usage: prepare_drivers <cache-folder> <7z.exe> [model]"
    );
    let emit: Emit = Arc::new(|e| println!("{}", serde_json::to_string(&e).unwrap()));
    let model = args
        .get(2)
        .and_then(|value| value.to_str())
        .unwrap_or(MODEL_ID);
    let assets = download::prepare_for_model(
        model,
        &PathBuf::from(&args[0]),
        &PathBuf::from(&args[1]),
        &emit,
        &Cancellation::default(),
    )?;
    if let Some(intel) = &assets.intel {
        engine::validate_intel_for_model(intel, model)?;
    }
    anyhow::ensure!(
        assets.intel.is_some() == (model != "a1707-2016"),
        "Wrong graphics extraction profile"
    );
    if model == "a1707-2016" {
        anyhow::ensure!(
            assets
                .drivers
                .join("AMDGraphics/Display/WT6A_INF/C0307252.inf")
                .is_file(),
            "Radeon display INF was not staged"
        );
    }
    println!(
        "Intel: {}\nBoot Camp: {}\nDrivers: {}",
        assets
            .intel
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Apple AMD package".into()),
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
