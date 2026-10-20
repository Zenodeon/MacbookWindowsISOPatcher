//! Run the same preparation, UAC transport and servicing pipeline as the GUI.
//! Usage: cargo run --example build_iso -- <build-request.json>
use anyhow::{Context, Result, bail, ensure};
use macbook_iso_patcher::{domain::*, ipc, platform};
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if let Some((port, token)) = ipc::parse_worker_args(&args)? {
        return ipc::worker(port, &token);
    }
    ensure!(args.len() == 1, "Usage: build_iso <build-request.json>");
    let mut request: BuildRequest = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    // Re-scan original media instead of trusting a previously retained inventory.
    request.inventory = platform::scan(&request.inventory.iso)?;
    let failure = Arc::new(Mutex::new(None));
    let failed = failure.clone();
    let emit: Emit = Arc::new(move |event| {
        println!("{}", serde_json::to_string(&event).unwrap());
        match event {
            Event::Failed(message) => *failed.lock().unwrap() = Some(message),
            Event::Cancelled => *failed.lock().unwrap() = Some("Build cancelled".into()),
            _ => (),
        }
    });
    ipc::start(request, emit, Cancellation::default()).context("ISO build failed")?;
    if let Some(message) = failure.lock().unwrap().take() {
        bail!("{message}");
    }
    Ok(())
}
