//! Download/cache packages only: no extraction, installation, or elevation.
use anyhow::{Context, Result};
use macbook_iso_patcher::{
    domain::{Cancellation, Emit, Event},
    driver_prefetch,
};
use std::{path::PathBuf, sync::Arc};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let model = args
        .next()
        .context("Usage: prefetch_drivers <model-id> <cache-folder> [cancel-after-ms]")?
        .to_string_lossy()
        .into_owned();
    let cache = PathBuf::from(args.next().context("Cache folder is required")?);
    let cancel = Cancellation::default();
    if let Some(delay) = args.next() {
        let delay: u64 = delay.to_string_lossy().parse()?;
        let signal = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            signal.cancel();
        });
    }
    let emit: Emit = Arc::new(|event| {
        if let Event::Stage { name, detail } = event {
            println!("{name}: {detail}");
        }
    });
    let paths = driver_prefetch::download_for_model(&model, &cache, &emit, &cancel)?;
    for path in paths {
        println!("Verified: {}", path.display());
    }
    Ok(())
}
