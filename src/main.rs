#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod appearance;
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
            .with_drag_and_drop(true)
            .with_inner_size(
                if cfg!(debug_assertions)
                    && std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_some()
                    && std::env::var_os("MACBOOK_PATCHER_PREVIEW_SMALL").is_some()
                {
                    [800.0, 520.0]
                } else {
                    [1240.0, 700.0]
                },
            )
            .with_min_inner_size([640.0, 400.0])
            .with_resizable(true)
            .with_clamp_size_to_monitor_size(true),
        ..Default::default()
    };
    eframe::run_native(
        "MacBook ISO Patcher",
        options,
        Box::new(|cc| Ok(Box::new(ui::PatcherApp::new(cc)))),
    )
}
