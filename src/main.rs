#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod ui;
fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match macbook_iso_patcher::ipc::parse_worker_args(&args) {
        Ok(Some((port, token))) => {
            if let Err(error) = macbook_iso_patcher::ipc::worker(port, &token) {
                eprintln!("Worker: {error:#}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        _ => (),
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 760.0])
            .with_min_inner_size([820.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MacBook ISO Patcher",
        options,
        Box::new(|cc| Ok(Box::new(ui::PatcherApp::new(cc)))),
    )
}
