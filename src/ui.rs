use crate::appearance::{self, ThemeMode};
#[path = "ui_log.rs"]
mod build_log;
#[path = "ui_downloads.rs"]
mod downloads;
#[path = "ui_pages.rs"]
mod pages;
#[path = "ui_settings.rs"]
mod storage_settings;
use downloads::DriverDownloads;
#[cfg(debug_assertions)]
#[path = "ui_preview.rs"]
mod preview;
use eframe::egui::{self, Color32, RichText};
use macbook_iso_patcher::models::{MODEL_PROFILES, ModelProfile};
use macbook_iso_patcher::{domain::*, engine, ipc, platform, prerequisites};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};

const REPO_URL: &str = "https://github.com/Zenodeon/MacbookWindowsISOPatcher";
fn muted(ui: &egui::Ui) -> Color32 {
    appearance::palette(ui.ctx()).muted
}

fn fitted_window_size(monitor: egui::Vec2) -> egui::Vec2 {
    let preferred = egui::vec2(1240.0, 700.0);
    let scale = ((monitor.x - 48.0) / preferred.x)
        .min((monitor.y - 96.0) / preferred.y)
        .clamp(0.1, 1.0);
    preferred * scale
}

fn dropped_iso(files: &[egui::DroppedFile]) -> Result<Option<PathBuf>, String> {
    if files.is_empty() {
        return Ok(None);
    }
    if files.len() != 1 {
        return Err("Drop one Windows ISO at a time.".into());
    }
    let path = files[0]
        .path
        .clone()
        .ok_or("Drop a local ISO file from File Explorer.")?;
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("iso"))
    {
        return Err("Choose a Windows .iso file.".into());
    }
    if !path.is_file() {
        return Err("The dropped ISO cannot be read. Choose an existing local file.".into());
    }
    Ok(Some(path))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Model,
    Iso,
    Editions,
    Review,
    Build,
}

enum Message {
    Scanned(Result<(Inventory, platform::MountedIso), String>),
    Dismounted(Result<bool, String>, Option<platform::MountedIso>),
    AdkStatus(String),
    AdkInstalled(Result<(PathBuf, bool), String>),
    DriverEvent(u64, Event),
    DriversDone(u64, Result<(), String>),
    Build(Event),
    StorageScanned(Result<macbook_iso_patcher::cleanup::Inventory, String>),
    StorageCleaned(
        macbook_iso_patcher::cleanup::CleanupRequest,
        Result<macbook_iso_patcher::cleanup::CleanupReport, String>,
    ),
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Settings {
    work: String,
    cache: String,
    seven_zip: String,
    oscdimg: String,
    #[serde(default)]
    theme: ThemeMode,
}

#[derive(Clone, Copy)]
enum BuildRetry {
    Review,
    Begin,
}

struct ActionError {
    message: String,
    retry: Option<BuildRetry>,
    copied: bool,
}

pub struct PatcherApp {
    page: Page,
    scroll_page: (Page, bool),
    model_selected: Option<usize>,
    drivers: DriverDownloads,
    storage: storage_settings::StorageSettings,
    iso: String,
    output: String,
    inventory: Option<Inventory>,
    iso_mount: Option<platform::MountedIso>,
    closing: bool,
    close_ready: bool,
    selected: BTreeSet<u32>,
    settings: Settings,
    settings_file: PathBuf,
    receiver: mpsc::Receiver<Message>,
    sender: mpsc::Sender<Message>,
    busy: bool,
    scanning: bool,
    dismounting: bool,
    confirm_dismount: bool,
    installing_adk: bool,
    adk_status: String,
    window_fitted: bool,
    notice: Option<String>,
    cancel: Cancellation,
    error: Option<ActionError>,
    stage: String,
    detail: String,
    download: Option<(String, u64, Option<u64>)>,
    build_progress: macbook_iso_patcher::progress::BuildStatus,
    logs: Vec<String>,
    finished: Option<PathBuf>,
    donation: bool,
    donation_clicked: bool,
    screenshot_frames: u32,
    screenshot_sent: bool,
}
impl PatcherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::new_with_context(&cc.egui_ctx)
    }
    fn new_with_context(ctx: &egui::Context) -> Self {
        appearance::install(ctx);
        let dirs = directories::ProjectDirs::from("com", "Zenodeon", "MacBookISOPatcher")
            .expect("User profile is required");
        let settings_file = dirs.config_dir().join("settings.json");
        let mut settings: Settings = std::fs::read(&settings_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if settings.work.is_empty() {
            settings.work = dirs.data_local_dir().join("builds").display().to_string();
        }
        if settings.cache.is_empty() {
            settings.cache = dirs.cache_dir().display().to_string();
        }
        if settings.seven_zip.is_empty() {
            settings.seven_zip = platform::find_seven_zip().display().to_string();
        }
        if settings.oscdimg.is_empty() {
            settings.oscdimg = platform::find_oscdimg().display().to_string();
        }
        let (sender, receiver) = mpsc::channel();
        // Renderer QA override is transient; never changes saved preferences.
        if std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_some() {
            match std::env::var("MACBOOK_PATCHER_THEME").as_deref() {
                Ok("dark") => settings.theme = ThemeMode::Dark,
                Ok("light") => settings.theme = ThemeMode::Light,
                _ => (),
            }
        }
        settings.theme.apply(ctx);
        let app = Self {
            page: Page::Model,
            scroll_page: (Page::Model, false),
            model_selected: None,
            drivers: DriverDownloads::default(),
            storage: storage_settings::StorageSettings::default(),
            iso: String::new(),
            output: String::new(),
            inventory: None,
            iso_mount: None,
            closing: false,
            close_ready: false,
            selected: BTreeSet::new(),
            settings,
            settings_file,
            sender,
            receiver,
            busy: false,
            scanning: false,
            dismounting: false,
            confirm_dismount: false,
            installing_adk: false,
            adk_status: String::new(),
            window_fitted: false,
            notice: None,
            cancel: Cancellation::default(),
            error: None,
            stage: String::new(),
            detail: String::new(),
            download: None,
            build_progress: Default::default(),
            logs: vec![],
            finished: None,
            donation: false,
            donation_clicked: false,
            screenshot_frames: 0,
            screenshot_sent: false,
        };
        #[cfg(debug_assertions)]
        let app = {
            let mut app = app;
            app.configure_preview();
            app
        };
        app
    }
    fn persist_settings(&self) {
        if let Some(parent) = self.settings_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_vec_pretty(&self.settings) {
            let _ = std::fs::write(&self.settings_file, data);
        }
    }
    fn consume(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Message::StorageScanned(result) => {
                    self.storage.scanning = false;
                    match result {
                        Ok(inventory) => {
                            self.storage.apply_inventory(inventory);
                        }
                        Err(error) => {
                            self.storage.error = Some(error);
                            self.storage.confirmation = None;
                            self.storage.execute_after_scan = false;
                            self.storage.ready_cleanup = None;
                        }
                    }
                }
                Message::StorageCleaned(request, result) => {
                    self.storage.deleting = false;
                    if matches!(
                        request,
                        macbook_iso_patcher::cleanup::CleanupRequest::Cache { .. }
                    ) {
                        self.drivers.reset_after_cleanup();
                    }
                    match result {
                        Ok(report) => {
                            self.storage.report = Some(report);
                        }
                        Err(error) => self.storage.error = Some(error),
                    }
                    // Re-scan on the next frame, where the UI context is available.
                    self.storage.needs_scan = true;
                }
                Message::DriverEvent(id, event) if id == self.drivers.id => match event {
                    Event::Stage { name, .. } => {
                        self.drivers.stage = name;
                        self.drivers.progress = None;
                    }
                    Event::Download {
                        name,
                        received,
                        total,
                    } => self.drivers.progress = Some((name, received, total)),
                    _ => (),
                },
                Message::DriversDone(id, result) => self.drivers.finish(id, result),
                Message::DriverEvent(..) => (),
                Message::AdkStatus(status) => self.adk_status = status,
                Message::AdkInstalled(result) => {
                    self.installing_adk = false;
                    match result {
                        Ok((tool, reboot)) => {
                            self.settings.oscdimg = tool.display().to_string();
                            self.persist_settings();
                            self.notice = Some(if reboot {
                                "ADK tools installed. Windows requests a restart before building."
                                    .into()
                            } else {
                                "ADK tools installed and ready.".into()
                            });
                        }
                        Err(error) => self.show_error(error),
                    }
                }
                Message::Dismounted(result, mount) => {
                    self.dismounting = false;
                    self.iso_mount = mount;
                    match result {
                        Ok(changed) => {
                            self.error = None;
                            self.page = Page::Iso;
                            self.inventory = None;
                            self.selected.clear();
                            self.donation = false;
                            self.finished = None;
                            self.notice = if self.closing {
                                None
                            } else {
                                Some(if changed {
                                    "ISO dismounted.".into()
                                } else {
                                    "ISO is already dismounted.".into()
                                })
                            };
                        }
                        Err(error) => {
                            self.closing = false;
                            self.show_error(error);
                        }
                    }
                }
                Message::Scanned(result) => {
                    self.scanning = false;
                    match result {
                        Ok((inv, mount)) => {
                            self.iso_mount = Some(mount);
                            self.selected.clear();
                            if let Some(pro) = inv
                                .images
                                .iter()
                                .find(|i| i.source_tested() && i.unsupported_reason().is_none())
                            {
                                self.selected.insert(pro.index);
                            }
                            let source = inv.iso.parent().unwrap();
                            let profile_id = self.model().map(|m| m.id).unwrap_or(MODEL_ID);
                            self.output = source
                                .join(format!(
                                    "Win10_{}_{}_Patched.iso",
                                    profile_id.replace('-', "_"),
                                    if profile_id == "a1707-2016" {
                                        "AMD"
                                    } else {
                                        "Intel"
                                    }
                                ))
                                .display()
                                .to_string();
                            self.output =
                                platform::display_path(std::path::Path::new(&self.output));
                            self.iso = platform::display_path(&inv.iso);
                            self.inventory = Some(inv);
                            self.page = Page::Editions;
                            self.error = None;
                        }
                        Err(error) => self.show_error(error),
                    }
                }
                Message::Build(event) => {
                    self.build_progress.apply(&event);
                    match event {
                        Event::OperationProgress { .. } | Event::OverallProgress { .. } => (),
                        Event::Stage { name, detail } => {
                            self.stage = name;
                            self.detail = detail;
                            self.download = None;
                        }
                        Event::Download {
                            name,
                            received,
                            total,
                        } => self.download = Some((name, received, total)),
                        Event::Log(text) => {
                            if self.logs.len() >= 4000 {
                                self.logs.drain(..500);
                            }
                            self.logs.push(text);
                        }
                        Event::Finished(path) => {
                            self.download = None;
                            self.error = None;
                            self.busy = false;
                            self.finished = Some(path);
                            self.stage = "Your patched ISO is ready".into();
                            self.detail =
                                "Read the installation steps below before booting your Mac.".into();
                        }
                        Event::Failed(error) => {
                            self.download = None;
                            self.busy = false;
                            self.show_error(error);
                            self.stage = "Build stopped".into();
                        }
                        Event::Cancelled => {
                            self.download = None;
                            self.busy = false;
                            self.stage = "Build cancelled".into();
                            self.detail = "The worker has finished its cleanup. Download caches and diagnostic files are retained.".into();
                        }
                    }
                }
            }
        }
    }
    fn scan(&mut self, ctx: &egui::Context) {
        if self.busy || self.scanning || self.dismounting || self.installing_adk || self.closing {
            return;
        }
        if self.model().is_none() || self.iso_mount.is_some() {
            self.show_error(
                "Choose your model first, and dismount the current ISO before selecting another.",
            );
            return;
        }
        self.notice = None;
        self.error = None;
        self.inventory = None;
        self.selected.clear();
        self.scanning = true;
        let iso = PathBuf::from(self.iso.trim());
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = platform::scan_mounted(&iso).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::Scanned(result));
            ctx.request_repaint();
        });
    }
    fn dismount(&mut self, ctx: &egui::Context, only_owned: bool) {
        if self.busy || self.scanning || self.dismounting || (!only_owned && self.installing_adk) {
            return;
        }
        let Some(mut mount) = self.iso_mount.take() else {
            return;
        };
        self.dismounting = true;
        self.error = None;
        self.notice = None;
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = if only_owned {
                mount.cleanup()
            } else {
                mount.dismount()
            }
            .map_err(|e| format!("{e:#}"));
            let retained = if result.is_err() { Some(mount) } else { None };
            let _ = tx.send(Message::Dismounted(result, retained));
            ctx.request_repaint();
        });
    }
    fn install_adk(&mut self, ctx: &egui::Context) {
        if self.busy || self.scanning || self.dismounting || self.installing_adk || self.closing {
            return;
        }
        self.installing_adk = true;
        self.error = None;
        self.notice = None;
        self.adk_status = "Preparing ADK download…".into();
        let cache = PathBuf::from(&self.settings.cache);
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let status = |status| {
                let _ = tx.send(Message::AdkStatus(status));
                ctx.request_repaint();
            };
            let result = prerequisites::install_adk(&cache, &status).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::AdkInstalled(result));
            ctx.request_repaint();
        });
    }
    fn model(&self) -> Option<&'static ModelProfile> {
        self.model_selected
            .and_then(|index| MODEL_PROFILES.get(index))
    }
    fn request(&self) -> anyhow::Result<BuildRequest> {
        Ok(BuildRequest {
            inventory: self
                .inventory
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Scan an ISO first"))?,
            selected: self.selected.iter().copied().collect(),
            model: self
                .model()
                .ok_or_else(|| anyhow::anyhow!("Select a MacBook model first"))?
                .id
                .into(),
            output: PathBuf::from(self.output.trim()),
            workspace: PathBuf::from(self.settings.work.trim()),
            cache: PathBuf::from(self.settings.cache.trim()),
            seven_zip: PathBuf::from(self.settings.seven_zip.trim()),
            oscdimg: PathBuf::from(self.settings.oscdimg.trim()),
        })
    }
    fn show_error(&mut self, message: impl Into<String>) {
        self.error = Some(ActionError {
            message: message.into(),
            retry: None,
            copied: false,
        });
    }
    fn show_build_error(&mut self, message: String, retry: BuildRetry) {
        self.error = Some(ActionError {
            message,
            retry: Some(retry),
            copied: false,
        });
    }
    fn review_build(&mut self, ctx: &egui::Context) {
        match self.request().and_then(|req| engine::preflight(&req)) {
            Ok(()) => {
                self.error = None;
                if self.selected.len() > 1 {
                    self.donation = true;
                    self.donation_clicked = false;
                } else {
                    self.begin(ctx);
                }
            }
            Err(error) => self.show_build_error(format!("{error:#}"), BuildRetry::Review),
        }
    }
    fn begin(&mut self, ctx: &egui::Context) {
        if self.storage.busy() {
            return;
        }
        if self.busy || self.scanning || self.dismounting || self.installing_adk || self.closing {
            return;
        }
        if !self.drivers_ready() {
            self.show_error("Finish downloading this model's drivers before building.");
            return;
        }
        let req = match self.request().and_then(|r| {
            engine::preflight(&r)?;
            Ok(r)
        }) {
            Ok(req) => req,
            Err(e) => {
                self.show_build_error(format!("{e:#}"), BuildRetry::Begin);
                return;
            }
        };
        self.persist_settings();
        self.page = Page::Build;
        self.donation = false;
        self.busy = true;
        self.finished = None;
        self.error = None;
        self.logs.clear();
        self.download = None;
        self.build_progress = Default::default();
        self.build_progress.apply(&Event::OverallProgress {
            completed: 0,
            total: macbook_iso_patcher::progress::build_steps(req.selected.len(), &req.model),
        });
        self.cancel = Cancellation::default();
        self.stage = "Preparing your build".into();
        self.detail = "Checking and downloading pinned driver packages.".into();
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        let cancel = self.cancel.clone();
        std::thread::spawn(move || {
            let events = tx.clone();
            let repaint = ctx.clone();
            let emit: Emit = Arc::new(move |e| {
                let _ = events.send(Message::Build(e));
                repaint.request_repaint();
            });
            if let Err(e) = ipc::start(req, emit, cancel.clone()) {
                let event = if cancel.is_cancelled() {
                    Event::Cancelled
                } else {
                    Event::Failed(format!("{e:#}"))
                };
                let _ = tx.send(Message::Build(event));
                ctx.request_repaint();
            }
        });
    }
    fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
        ui.heading(title);
        ui.add_space(4.0);
        ui.label(RichText::new(description).color(muted(ui)));
        ui.add_space(20.0);
    }
    fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
        ui.add_enabled(
            enabled,
            egui::Button::new(RichText::new(label).color(Color32::WHITE))
                .fill(appearance::palette(ui.ctx()).primary),
        )
        .clicked()
    }
    fn file_field(
        ui: &mut egui::Ui,
        label: &str,
        value: &mut String,
        extension: &str,
        save: bool,
    ) -> bool {
        ui.label(RichText::new(label).strong());
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(value)
                        .desired_width((ui.available_width() - 105.0).max(180.0)),
                )
                .changed();
            if ui.button("Browse…").clicked() {
                let dialog = rfd::FileDialog::new().add_filter(extension, &[extension]);
                let path = if save {
                    let filename = PathBuf::from(&*value)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Windows_Patched.iso".into());
                    dialog.set_file_name(filename).save_file()
                } else {
                    dialog.pick_file()
                };
                if let Some(path) = path {
                    *value = platform::display_path(&path);
                    changed = true;
                }
            }
        });
        changed
    }
    fn folder_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
        ui.label(label);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(value)
                    .desired_width((ui.available_width() - 105.0).max(180.0)),
            );
            if ui.button("Browse…").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                *value = platform::display_path(&path);
            }
        });
    }
    fn donation_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        Self::heading(
            ui,
            "Support the project",
            "Your multi-edition build works with or without a donation.",
        );
        Self::section(
            ui,
            "Optional support",
            appearance::palette(ui.ctx()).model,
            |ui| {
                if Self::primary(ui, "Donate on Ko-fi", true) {
                    match webbrowser::open(DONATION_URL) {
                        Ok(_) => self.donation_clicked = true,
                        Err(e) => self.show_error(format!(
                            "Could not open your browser: {e}. You can continue without donating."
                        )),
                    }
                }
                ui.hyperlink_to("ko-fi.com/zenodeon", DONATION_URL);
                ui.add_space(16.0);
                if ui
                    .button(if self.donation_clicked {
                        "Continue to build"
                    } else {
                        "Continue without donating"
                    })
                    .clicked()
                {
                    self.begin(ctx);
                }
                ui.add_space(18.0);
                ui.label(
                    RichText::new("The app does not check or record whether you donate.")
                        .color(muted(ui)),
                );
            },
        );
    }
    fn build_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(ui, &self.stage, &self.detail);
        let colors = appearance::palette(ui.ctx());
        Self::section(ui, "Progress", colors.source, |ui| {
            ui.label(RichText::new(&self.stage).strong());
            ui.label(RichText::new(&self.detail).color(muted(ui)));
            ui.add_space(8.0);
            let transfer = self.download.as_ref().map(|(name, received, total)| {
                (
                    format!("Downloading {name}"),
                    total.filter(|v| *v > 0).map(|total| {
                        (*received as f64 / total as f64 * 100.0).clamp(0.0, 100.0) as f32
                    }),
                )
            });
            let operation = self.build_progress.operation.as_ref().or(transfer.as_ref());
            let bar_fill = if ui.visuals().dark_mode {
                ui.visuals().selection.bg_fill
            } else {
                colors.accent.gamma_multiply(0.35)
            };
            let current_operation = |ui: &mut egui::Ui| {
                ui.label("Current operation");
                if let Some((name, Some(percent))) = operation {
                    ui.add(
                        egui::ProgressBar::new(*percent / 100.0)
                            .fill(bar_fill)
                            .desired_height(24.0)
                            .text(format!("{name} · {percent:.1}%")),
                    );
                } else if self.busy {
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            operation
                                .map(|(name, _)| name.as_str())
                                .unwrap_or("Working on this step…"),
                        );
                    });
                } else {
                    ui.label(if self.finished.is_some() {
                        "Complete"
                    } else {
                        "Stopped"
                    });
                }
            };
            let overall = |ui: &mut egui::Ui| {
                ui.label("Overall build");
                ui.add(
                    egui::ProgressBar::new(self.build_progress.fraction().unwrap_or(0.0))
                        .fill(bar_fill)
                        .desired_height(24.0)
                        .text(self.build_progress.label()),
                );
            };
            current_operation(ui);
            ui.add_space(8.0);
            overall(ui);
            if self.busy {
                ui.add_space(6.0);
                ui.label(RichText::new(if self.cancel.is_cancelled() { "Cancellation requested. Waiting for the current servicing operation to finish safely…" } else { "Keep the app open. Overall progress counts steps, not time remaining." }).size(12.0).color(muted(ui)));
            }
        });
        if let Some(path) = self.finished.clone() {
            Self::section(ui, "Your patched ISO", colors.model, |ui| {
                ui.label(RichText::new(platform::display_path(&path)).strong());
                if ui.button("Open output folder").clicked()
                    && let Err(error) = platform::open_folder(path.parent().unwrap())
                {
                    self.show_error(format!("Could not open output folder: {error:#}"));
                }
                ui.add_space(14.0);
                ui.label("1. Create the BOOTCAMP partition in macOS first.");
                ui.label("2. Boot this ISO through Ventoy WIMBOOT (press w).");
                ui.label("3. Select only BOOTCAMP in Windows Setup.");
                ui.label("4. On the Windows desktop, run C:\\BootCamp\\Setup.exe as administrator and reboot.");
                ui.label(RichText::new("Hardware testing of this app’s output remains required. Do not run the old Intel graphics installer.").color(muted(ui)));
            });
        }
        ui.add_space(12.0);
        Self::section(ui, "Build details", colors.surface, |ui| {
            let default_open = cfg!(debug_assertions)
                && std::env::var("MACBOOK_PATCHER_PREVIEW").as_deref() == Ok("build-log")
                && std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_some();
            if build_log::build_log_view(ui, &self.logs, default_open).save_requested
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("macbook-patcher.log")
                    .save_file()
                && let Err(error) = std::fs::write(path, self.logs.join("\n"))
            {
                self.show_error(format!("Could not save build log: {error}"));
            }
        });
    }
}

impl eframe::App for PatcherApp {
    #[cfg(debug_assertions)]
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_none() {
            return;
        }
        let Ok(target) = std::env::var("MACBOOK_PATCHER_PREVIEW_HOVER") else {
            return;
        };
        let screen = input.screen_rect.unwrap_or_else(|| ctx.content_rect());
        let size = egui::vec2(
            (screen.width() - 88.0).clamp(240.0, 760.0),
            (screen.height() - 100.0).clamp(220.0, 560.0),
        );
        let shell = egui::Rect::from_center_size(screen.center(), size);
        let position = match target.as_str() {
            "close" => egui::pos2(shell.right() - 32.0, shell.bottom() - 20.0),
            "refresh" => egui::pos2(shell.right() - 40.0, shell.top() + 18.0),
            _ => return,
        };
        // Renderer QA only: exercise real hover styling without moving the OS cursor.
        input.events.push(egui::Event::PointerMoved(position));
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.consume();
        let focused = ctx.input(|i| i.focused);
        let snapshot_preview = cfg!(debug_assertions)
            && std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_some()
            && std::env::var("MACBOOK_PATCHER_PREVIEW").is_ok_and(|v| v.starts_with("settings"));
        if self.storage.open && focused && !self.storage.focused && !snapshot_preview {
            self.storage.needs_scan = true;
        }
        self.storage.focused = focused;
        if self.storage.needs_scan && !self.storage.busy() {
            self.storage.needs_scan = false;
            self.scan_storage(ctx);
        }
        if !ctx.input(|i| i.viewport().close_requested()) {
            self.ensure_driver_downloads(ctx);
        }
        if !self.window_fitted
            && let Some(monitor) = ctx.input(|i| i.viewport().monitor_size)
        {
            let size = fitted_window_size(monitor);
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                size.x.min(640.0),
                size.y.min(400.0),
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            self.window_fitted = true;
        }
        let compact = ctx.content_rect().width() < 900.0;
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() && self.error.is_none() && !self.storage.open {
            if self.busy
                || self.scanning
                || self.dismounting
                || self.installing_adk
                || self.confirm_dismount
                || self.closing
                || self.iso_mount.is_some()
            {
                self.show_error("Dismount the current ISO and finish any running operation before choosing another.");
            } else {
                match dropped_iso(&dropped) {
                    Ok(Some(path)) => {
                        if self.model().is_none() {
                            self.show_error("Select a MacBook model first.");
                        } else {
                            self.iso = platform::display_path(&path);
                            self.page = Page::Iso;
                            self.donation = false;
                            self.finished = None;
                            self.scan(ctx);
                        }
                    }
                    Err(error) => self.show_error(error),
                    Ok(None) => (),
                }
            }
        }
        if self.busy
            || self.scanning
            || self.dismounting
            || self.installing_adk
            || self.drivers.running
            || self.storage.busy()
        {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if !self.close_ready
            && ctx.input(|i| i.viewport().close_requested())
            && (self.busy
                || self.scanning
                || self.dismounting
                || self.iso_mount.is_some()
                || self.drivers.running
                || self.storage.busy())
        {
            self.closing = true;
            self.confirm_dismount = false;
            self.drivers.cancel.cancel();
            if self.busy {
                self.cancel.cancel();
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if self.closing
            && !self.busy
            && !self.scanning
            && !self.dismounting
            && !self.drivers.running
            && !self.storage.busy()
        {
            if self.iso_mount.is_some() {
                self.dismount(ctx, true);
            } else {
                self.close_ready = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        egui::SidePanel::left("workflow")
            .exact_width(if compact { 180.0 } else { 220.0 })
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(appearance::palette(ctx).rail)
                    .inner_margin(if compact { 14.0 } else { 22.0 }),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("workflow-scroll")
                    .show(ui, |ui| {
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("MacBook\nISO Patcher")
                                .font(appearance::display_font(if compact { 18.0 } else { 22.0 })),
                        );
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("by").color(muted(ui)));
                            ui.add(egui::Hyperlink::from_label_and_url(
                                RichText::new("zenodeon").font(appearance::display_font(16.0)),
                                "https://zenodeon.com/",
                            ));
                        });
                        ui.add_space(22.0);
                        for (page, label) in [
                            (Page::Model, "MacBook model"),
                            (Page::Iso, "Windows ISO"),
                            (Page::Editions, "Windows editions"),
                            (Page::Review, "Review"),
                            (Page::Build, "Build"),
                        ] {
                            let current = self.page == page;
                            ui.label(
                                RichText::new(label)
                                    .size(16.0)
                                    .color(if current {
                                        appearance::palette(ctx).accent
                                    } else {
                                        muted(ui)
                                    })
                                    .strong(),
                            );
                            if current {
                                ui.label(RichText::new("Current step").size(12.0).color(muted(ui)));
                            }
                            ui.add_space(10.0);
                        }
                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(
                                self.model()
                                    .map(|model| {
                                        format!(
                                            "{}\n{}",
                                            model.identifier,
                                            if model.hardware_verified() {
                                                "v17 HDMI fix"
                                            } else {
                                                "Unverified"
                                            }
                                        )
                                    })
                                    .unwrap_or_else(|| "Select a MacBook model".into()),
                            )
                            .size(13.0)
                            .color(muted(ui)),
                        );
                        ui.label(
                            RichText::new("v0.1.0 · Rust desktop")
                                .size(12.0)
                                .color(muted(ui)),
                        );
                        ui.add_space(12.0);
                        if ui.button("Settings").clicked() {
                            self.storage.open = true;
                            self.storage.navigate_builds(false);
                            self.scan_storage(ctx);
                        }
                        ui.add_space(12.0);
                        ui.label(RichText::new("Appearance").size(13.0).color(muted(ui)));
                        let previous = self.settings.theme;
                        egui::ComboBox::from_id_salt("theme")
                            .selected_text(format!("{:?}", self.settings.theme))
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    ThemeMode::System,
                                    "System",
                                );
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    ThemeMode::Light,
                                    "Light",
                                );
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    ThemeMode::Dark,
                                    "Dark",
                                );
                            });
                        if previous != self.settings.theme {
                            self.settings.theme.apply(ctx);
                            self.persist_settings();
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button("Ko-fi")
                                .on_hover_text("Support zenodeon on Ko-fi")
                                .clicked()
                                && let Err(error) = webbrowser::open(DONATION_URL)
                            {
                                self.show_error(format!("Could not open Ko-fi: {error}"));
                            }
                            if ui
                                .button("GitHub")
                                .on_hover_text("Open the patcher repository")
                                .clicked()
                                && let Err(error) = webbrowser::open(REPO_URL)
                            {
                                self.show_error(format!("Could not open GitHub: {error}"));
                            }
                        });
                    });
            });
        egui::TopBottomPanel::bottom("actions")
            .frame(
                egui::Frame::new()
                    .fill(appearance::palette(ctx).footer)
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(42.0);
                    if self.busy {
                        if ui
                            .add_enabled(
                                !self.cancel.is_cancelled(),
                                egui::Button::new("Cancel build"),
                            )
                            .clicked()
                        {
                            self.cancel.cancel();
                        }
                    } else if self.donation && ui.add_enabled(!self.closing && !self.dismounting && !self.installing_adk, egui::Button::new("Back to review")).clicked() {
                        self.donation = false;
                        self.error = None;
                    } else if self.page == Page::Iso && self.inventory.is_none()
                        && ui.add_enabled(!self.scanning && !self.closing && !self.dismounting, egui::Button::new("Change model")).clicked()
                    {
                        self.page = Page::Model;
                    }
                    if self.inventory.is_some() && !self.donation
                        && ui.add_enabled(
                            cfg!(windows) && !self.busy && !self.scanning && !self.dismounting && !self.installing_adk && !self.closing,
                            egui::Button::new(if self.dismounting { "Dismounting…" } else { "Dismount ISO" }),
                        ).on_hover_text("Dismount the selected source ISO from Windows. The ISO file stays unchanged.").clicked()
                    {
                        self.confirm_dismount = true;
                    }
                    let model_left = ui.cursor().min.x;
                    let model_right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.busy || self.page == Page::Build { self.build_iso_button(ui, false); }
                        if !self.busy && !self.scanning && !self.dismounting && !self.installing_adk && !self.confirm_dismount && !self.donation && !self.closing {
                            if self.page == Page::Model && Self::primary(ui,"Start download & continue to Windows ISO", self.model().is_some() && !self.storage.busy()) {
                                self.drivers.requested_model = self.model().map(|model| model.id);
                                self.ensure_driver_downloads(ctx);
                                self.page = Page::Iso;
                                self.error = None;
                            }
                            if self.page == Page::Editions
                                && Self::primary(ui, "Review build", !self.selected.is_empty())
                            {
                                self.page = Page::Review;
                                self.error = None;
                            }
                            if self.page == Page::Review
                                && self.build_iso_button(ui, !self.output.trim().is_empty() && self.model().is_some() && self.drivers_ready())
                            {
                                self.review_build(ctx);
                            }
                        }
                        ui.cursor().max.x
                    }).inner;
                    if let Some(model) = self.model()
                        && model_right - model_left >= 96.0
                    {
                        let rect = egui::Rect::from_min_max(
                            egui::pos2(model_left + 8.0, ui.min_rect().top()),
                            egui::pos2(model_right - 8.0, ui.min_rect().bottom()),
                        );
                        let label = if rect.width() < 320.0 { model.identifier } else { model.name };
                        let tooltip = format!("{}\n{}", model.name, model.identifier);
                        self.driver_download_footer(ui, rect, label, &tooltip);
                    }
                });
            });
        let scroll_page = (self.page, self.donation);
        let reset_scroll = self.scroll_page != scroll_page;
        self.scroll_page = scroll_page;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(appearance::palette(ctx).canvas)
                    .inner_margin(if compact { 14.0 } else { 30.0 }),
            )
            .show(ctx, |ui| {
                let scroll = egui::ScrollArea::vertical().id_salt("page_content");
                let scroll = if reset_scroll {
                    scroll.vertical_scroll_offset(0.0)
                } else {
                    scroll
                };
                scroll.show(ui, |ui| {
                    if self.closing {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Finishing cleanup before closing…");
                        });
                        ui.add_space(10.0);
                    }
                    if let Some(notice) = &self.notice {
                        ui.label(notice);
                        ui.add_space(10.0);
                    }
                    if self.donation {
                        self.donation_page(ui, ctx);
                    } else {
                        match self.page {
                            Page::Model => self.model_page(ui),
                            Page::Iso => self.input_page(ui, ctx),
                            Page::Editions => self.editions_page(ui),
                            Page::Review => self.review_page(ui),
                            Page::Build => self.build_page(ui),
                        }
                    }
                });
            });
        if ctx.input(|i| !i.raw.hovered_files.is_empty())
            && (self.page != Page::Iso
                || self.busy
                || self.scanning
                || self.dismounting
                || self.installing_adk)
        {
            let rect = ctx.viewport_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("iso-drop-hint"),
            ));
            painter.rect_filled(rect, 0, Color32::from_black_alpha(215));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if self.iso_mount.is_some() {
                    "Dismount the current ISO before choosing another"
                } else if self.model().is_none() {
                    "Select a MacBook model first"
                } else if self.busy || self.scanning || self.dismounting || self.installing_adk {
                    "Finish the current operation first"
                } else {
                    "Drop one Windows ISO to start a new scan"
                },
                egui::FontId::proportional(23.0),
                Color32::WHITE,
            );
        }
        if self.confirm_dismount {
            let colors = appearance::palette(ctx);
            let modal = egui::Modal::new(egui::Id::new("confirm-dismount"))
                .frame(
                    egui::Frame::new()
                        .fill(colors.surface)
                        .corner_radius(16)
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            ctx.style().visuals.widgets.noninteractive.bg_stroke.color,
                        ))
                        .inner_margin(24.0),
                )
                .show(ctx, |ui| {
                    ui.set_width((ctx.content_rect().width() - 96.0).clamp(240.0, 420.0));
                    ui.label(RichText::new("Dismount ISO?").size(23.0).strong());
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "Eject the ISO from Windows and return to the Windows ISO page.",
                        )
                        .color(colors.muted),
                    );
                    ui.add_space(12.0);
                    egui::Frame::new()
                        .fill(colors.drop)
                        .corner_radius(8)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(
                                        PathBuf::from(self.iso.trim())
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy(),
                                    )
                                    .strong(),
                                )
                                .wrap(),
                            );
                        });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Your ISO file stays unchanged.")
                            .size(13.0)
                            .color(colors.muted),
                    );
                    ui.add_space(20.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Self::primary(ui, "Dismount ISO", true) {
                            self.confirm_dismount = false;
                            self.dismount(ctx, false);
                        }
                        if ui.button("Keep mounted").clicked() {
                            self.confirm_dismount = false;
                        }
                    });
                });
            if modal.should_close() {
                self.confirm_dismount = false;
            }
        }
        self.storage_popup(ctx);
        // Process Cancel/Back/Close first, even if a preflight result arrived
        // in this frame. A cancelled confirmation must never launch a worker.
        if let Some(request) = self.storage.ready_cleanup.take() {
            self.start_cleanup(request, ctx);
        }
        if let Some(error) = &self.error {
            let message = error.message.clone();
            let retry = error.retry;
            let copied = error.copied;
            let colors = appearance::palette(ctx);
            let mut retry_clicked = false;
            let mut close_clicked = false;
            let mut copy_clicked = false;
            let modal = egui::Modal::new(egui::Id::new("action-needed"))
                .frame(egui::Frame::new().fill(colors.surface).corner_radius(16)
                    .stroke(egui::Stroke::new(1.0_f32, ctx.style().visuals.widgets.noninteractive.bg_stroke.color))
                    .inner_margin(24.0))
                .show(ctx, |ui| {
                    ui.set_width((ctx.content_rect().width() - 96.0).clamp(240.0, 460.0));
                    ui.label(RichText::new("Action needed").size(23.0).strong());
                    ui.add_space(12.0);
                    egui::ScrollArea::vertical().id_salt("action-error-message")
                        .max_height((ctx.content_rect().height() - 300.0).max(80.0))
                        .show(ui, |ui| { ui.label(RichText::new(&message).color(colors.error)); });
                    if retry.is_some() {
                        ui.add_space(12.0);
                        ui.label(RichText::new("Resolve the issue, then retry. Close this popup to change your build settings.").size(13.0).color(colors.muted));
                    }
                    ui.add_space(20.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if retry.is_some() {
                            retry_clicked = Self::primary(ui, "Retry", true);
                            close_clicked = ui.button("Close").clicked();
                        } else {
                            close_clicked = Self::primary(ui, "Close", true);
                        }
                        copy_clicked = ui.button(if copied { "Copied" } else { "Copy error" }).clicked();
                    });
                });
            if copy_clicked {
                ctx.copy_text(message);
                if let Some(error) = &mut self.error {
                    error.copied = true;
                }
            }
            if retry_clicked || close_clicked || modal.should_close() {
                self.error = None;
            }
            if retry_clicked {
                match retry {
                    Some(BuildRetry::Review) => self.review_build(ctx),
                    Some(BuildRetry::Begin) => self.begin(ctx),
                    None => (),
                }
            }
        }
        // Optional renderer-level screenshot for release QA; no OS screen scraping.
        if let Some(path) = std::env::var_os("MACBOOK_PATCHER_SCREENSHOT") {
            self.screenshot_frames += 1;
            let scanning_fixture = cfg!(debug_assertions)
                && std::env::var("MACBOOK_PATCHER_PREVIEW")
                    .is_ok_and(|v| v == "settings-scanning" || v == "settings-live-checking");
            if self.screenshot_frames > 3
                && !self.screenshot_sent
                && (!self.storage.scanning || scanning_fixture)
                && !self.storage.needs_scan
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.screenshot_sent = true;
            }
            let events = ctx.input(|i| i.events.clone());
            for event in events {
                if let egui::Event::Screenshot { image, .. } = event {
                    if let Ok(file) = std::fs::File::create(&path) {
                        let mut encoder =
                            png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        if let Ok(mut writer) = encoder.write_header() {
                            let bytes: Vec<u8> =
                                image.pixels.iter().flat_map(|p| p.to_array()).collect();
                            let _ = writer.write_image_data(&bytes);
                        }
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod drop_tests {
    use super::*;
    #[test]
    fn build_events_clear_operation_and_download_without_losing_overall_position() {
        for terminal in [
            Event::Cancelled,
            Event::Failed("test failure".into()),
            Event::Finished("test.iso".into()),
        ] {
            let success = matches!(terminal, Event::Finished(_));
            let mut app = PatcherApp::new_with_context(&egui::Context::default());
            app.page = Page::Build;
            app.busy = true;
            for event in [
                Event::OverallProgress {
                    completed: 6,
                    total: 12,
                },
                Event::OperationProgress {
                    operation: "Saving".into(),
                    percent: Some(40.0),
                },
                Event::Download {
                    name: "package".into(),
                    received: 50,
                    total: Some(100),
                },
                terminal,
            ] {
                app.sender.send(Message::Build(event)).unwrap();
            }
            app.consume();
            assert!(!app.busy && app.download.is_none() && app.build_progress.operation.is_none());
            assert_eq!(
                app.build_progress.fraction(),
                Some(if success { 1.0 } else { 0.5 })
            );
        }
    }
    #[test]
    fn startup_fits_smaller_monitors_without_changing_proportions() {
        for monitor in [
            egui::vec2(1920.0, 1080.0),
            egui::vec2(1024.0, 768.0),
            egui::vec2(800.0, 600.0),
        ] {
            let size = fitted_window_size(monitor);
            assert!(size.x <= monitor.x - 48.0 && size.y <= monitor.y - 96.0);
            assert!((size.x / size.y - 1240.0 / 700.0).abs() < 0.001);
        }
        assert_eq!(
            fitted_window_size(egui::vec2(1920.0, 1080.0)),
            egui::vec2(1240.0, 700.0)
        );
    }
    #[test]
    fn accepts_one_local_iso_case_insensitively() {
        let path = std::env::temp_dir().join(format!("drop-test-{}.ISO", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"drop validation fixture").unwrap();
        let file = egui::DroppedFile {
            path: Some(path.clone()),
            ..Default::default()
        };
        assert_eq!(dropped_iso(&[file]).unwrap(), Some(path.clone()));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_multiple_non_iso_and_browser_drops() {
        let file = egui::DroppedFile {
            path: Some("example.txt".into()),
            ..Default::default()
        };
        assert!(dropped_iso(std::slice::from_ref(&file)).is_err());
        assert!(dropped_iso(&[file.clone(), file]).is_err());
        assert!(dropped_iso(&[egui::DroppedFile::default()]).is_err());
        assert_eq!(dropped_iso(&[]).unwrap(), None);
    }
    #[test]
    fn rejects_missing_iso_paths() {
        let path = std::env::temp_dir().join(format!("missing-{}.iso", uuid::Uuid::new_v4()));
        assert!(
            dropped_iso(&[egui::DroppedFile {
                path: Some(path),
                ..Default::default()
            }])
            .is_err()
        );
    }
}
