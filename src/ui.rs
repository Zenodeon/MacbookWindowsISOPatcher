use crate::appearance::{self, ThemeMode};
#[path = "ui_pages.rs"]
mod pages;
use eframe::egui::{self, Color32, RichText};
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
enum Message {
    Scanned(Result<Inventory, String>),
    Dismounted(Result<bool, String>),
    AdkStatus(String),
    AdkInstalled(Result<(PathBuf, bool), String>),
    Build(Event),
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

pub struct PatcherApp {
    page: usize,
    iso: String,
    output: String,
    inventory: Option<Inventory>,
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
    error: Option<String>,
    stage: String,
    detail: String,
    download: Option<(String, u64, Option<u64>)>,
    logs: Vec<String>,
    finished: Option<PathBuf>,
    donation: bool,
    donation_clicked: bool,
    screenshot_frames: u32,
    screenshot_sent: bool,
}
impl PatcherApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        appearance::install(&cc.egui_ctx);
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
        settings.theme.apply(&cc.egui_ctx);
        Self {
            page: 0,
            iso: String::new(),
            output: String::new(),
            inventory: None,
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
            logs: vec![],
            finished: None,
            donation: false,
            donation_clicked: false,
            screenshot_frames: 0,
            screenshot_sent: false,
        }
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
                        Err(error) => self.error = Some(error),
                    }
                }
                Message::Dismounted(result) => {
                    self.dismounting = false;
                    match result {
                        Ok(changed) => {
                            self.error = None;
                            self.page = 0;
                            self.inventory = None;
                            self.selected.clear();
                            self.donation = false;
                            self.finished = None;
                            self.notice = Some(if changed {
                                "ISO dismounted.".into()
                            } else {
                                "ISO is already dismounted.".into()
                            });
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Message::Scanned(result) => {
                    self.scanning = false;
                    match result {
                        Ok(inv) => {
                            self.selected.clear();
                            if let Some(pro) = inv
                                .images
                                .iter()
                                .find(|i| i.source_tested() && i.unsupported_reason().is_none())
                            {
                                self.selected.insert(pro.index);
                            }
                            let source = inv.iso.parent().unwrap();
                            self.output =
                                source.join("Win10_A1706_Patched.iso").display().to_string();
                            self.iso = inv.iso.display().to_string();
                            self.inventory = Some(inv);
                            self.page = 1;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Message::Build(event) => match event {
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
                        self.error = None;
                        self.busy = false;
                        self.finished = Some(path);
                        self.stage = "Your patched ISO is ready".into();
                        self.detail =
                            "Read the installation steps below before booting your Mac.".into();
                    }
                    Event::Failed(error) => {
                        self.busy = false;
                        self.error = Some(error);
                        self.stage = "Build stopped".into();
                    }
                    Event::Cancelled => {
                        self.busy = false;
                        self.stage = "Build cancelled".into();
                        self.detail = "The worker has finished its cleanup. Download caches and diagnostic files are retained.".into();
                    }
                },
            }
        }
    }
    fn scan(&mut self, ctx: &egui::Context) {
        if self.busy || self.scanning || self.dismounting || self.installing_adk {
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
            let result = platform::scan(&iso).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::Scanned(result));
            ctx.request_repaint();
        });
    }
    fn dismount(&mut self, ctx: &egui::Context) {
        if self.busy
            || self.scanning
            || self.dismounting
            || self.installing_adk
            || self.iso.trim().is_empty()
        {
            return;
        }
        self.dismounting = true;
        self.error = None;
        self.notice = None;
        let iso = PathBuf::from(self.iso.trim());
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = platform::dismount_iso(&iso).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::Dismounted(result));
            ctx.request_repaint();
        });
    }
    fn install_adk(&mut self, ctx: &egui::Context) {
        if self.busy || self.scanning || self.dismounting || self.installing_adk {
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
    fn request(&self) -> anyhow::Result<BuildRequest> {
        Ok(BuildRequest {
            inventory: self
                .inventory
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Scan an ISO first"))?,
            selected: self.selected.iter().copied().collect(),
            model: MODEL_ID.into(),
            output: PathBuf::from(self.output.trim()),
            workspace: PathBuf::from(self.settings.work.trim()),
            cache: PathBuf::from(self.settings.cache.trim()),
            seven_zip: PathBuf::from(self.settings.seven_zip.trim()),
            oscdimg: PathBuf::from(self.settings.oscdimg.trim()),
        })
    }
    fn begin(&mut self, ctx: &egui::Context) {
        if self.busy || self.scanning || self.dismounting || self.installing_adk {
            return;
        }
        let req = match self.request().and_then(|r| {
            engine::preflight(&r)?;
            Ok(r)
        }) {
            Ok(req) => req,
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                return;
            }
        };
        self.persist_settings();
        self.page = 3;
        self.donation = false;
        self.busy = true;
        self.finished = None;
        self.error = None;
        self.logs.clear();
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
                    dialog.set_file_name("Win10_A1706_Patched.iso").save_file()
                } else {
                    dialog.pick_file()
                };
                if let Some(path) = path {
                    *value = path.display().to_string();
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
                *value = path.display().to_string();
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
                        Err(e) => {
                            self.error = Some(format!(
                                "Could not open your browser: {e}. You can continue without donating."
                            ))
                        }
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
            if self.busy {
                ui.horizontal(|ui| { ui.spinner(); ui.label(if self.cancel.is_cancelled() { "Cancellation requested. Waiting for the current servicing operation to finish safely…" } else { "Keep the app open while the build is running." }); });
            }
            if let Some((name, received, total)) = &self.download {
                ui.label(name);
                if let Some(total) = total.filter(|t| *t > 0) {
                    ui.add(
                        egui::ProgressBar::new(*received as f32 / total as f32).show_percentage(),
                    );
                }
                ui.label(format!(
                    "{:.1} MB downloaded",
                    *received as f64 / 1_000_000.0
                ));
            }
        });
        if let Some(path) = &self.finished {
            Self::section(ui, "Your patched ISO", colors.model, |ui| {
                ui.label(RichText::new(path.display().to_string()).strong());
                if ui.button("Open output folder").clicked()
                    && let Err(error) = platform::open_folder(path.parent().unwrap())
                {
                    self.error = Some(format!("Could not open output folder: {error:#}"));
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
            egui::CollapsingHeader::new("Show build log")
                .default_open(false)
                .show(ui, |ui| {
                    if ui.button("Save visible log…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_file_name("macbook-patcher.log")
                            .save_file()
                    {
                        let _ = std::fs::write(path, self.logs.join("\n"));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("log")
                        .max_height(280.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.logs {
                                ui.monospace(line);
                            }
                        });
                });
        });
    }
}

impl eframe::App for PatcherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.consume();
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
        if !dropped.is_empty() {
            if self.busy
                || self.scanning
                || self.dismounting
                || self.installing_adk
                || self.confirm_dismount
            {
                self.error = Some(
                    "Wait for the current operation to finish before dropping another ISO.".into(),
                );
            } else {
                match dropped_iso(&dropped) {
                    Ok(Some(path)) => {
                        self.iso = path.display().to_string();
                        self.page = 0;
                        self.donation = false;
                        self.finished = None;
                        self.scan(ctx);
                    }
                    Err(error) => self.error = Some(error),
                    Ok(None) => (),
                }
            }
        }
        if self.busy || self.scanning || self.dismounting || self.installing_adk {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if self.busy && ctx.input(|i| i.viewport().close_requested()) {
            self.cancel.cancel();
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
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
                        for (n, label) in ["Windows ISO", "Mac & editions", "Review", "Build"]
                            .iter()
                            .enumerate()
                        {
                            let current = self.page == n;
                            ui.label(
                                RichText::new(*label)
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
                            RichText::new("A1706 profile\nv17 HDMI fix")
                                .size(13.0)
                                .color(muted(ui)),
                        );
                        ui.label(
                            RichText::new("v0.1.0 · Rust desktop")
                                .size(12.0)
                                .color(muted(ui)),
                        );
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
                                self.error = Some(format!("Could not open Ko-fi: {error}"));
                            }
                            if ui
                                .button("GitHub")
                                .on_hover_text("Open the patcher repository")
                                .clicked()
                                && let Err(error) = webbrowser::open(REPO_URL)
                            {
                                self.error = Some(format!("Could not open GitHub: {error}"));
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
                    } else if self.page > 0
                        && ui
                            .add_enabled(!self.dismounting && !self.installing_adk, egui::Button::new(if self.donation {
                                "Back to review"
                            } else {
                                "Back"
                            }))
                            .clicked()
                    {
                        if self.donation {
                            self.donation = false;
                        } else {
                            self.page -= 1;
                        }
                        self.error = None;
                    }
                    if !self.iso.trim().is_empty()
                        && ui.add_enabled(
                            cfg!(windows) && !self.busy && !self.scanning && !self.dismounting && !self.installing_adk,
                            egui::Button::new(if self.dismounting { "Dismounting…" } else { "Dismount ISO" }),
                        ).on_hover_text("Dismount the selected source ISO from Windows. The ISO file stays unchanged.").clicked()
                    {
                        self.confirm_dismount = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.busy && !self.scanning && !self.dismounting && !self.installing_adk && !self.confirm_dismount && !self.donation {
                            if self.page == 1
                                && Self::primary(ui, "Review build", !self.selected.is_empty())
                            {
                                self.page = 2;
                                self.error = None;
                            }
                            if self.page == 2
                                && Self::primary(
                                    ui,
                                    "Build patched ISO",
                                    !self.output.trim().is_empty(),
                                )
                            {
                                match self.request().and_then(|req| engine::preflight(&req)) {
                                    Ok(()) => {
                                        if self.selected.len() > 1 {
                                            self.donation = true;
                                            self.donation_clicked = false;
                                            self.error = None;
                                        } else {
                                            self.begin(ctx);
                                        }
                                    }
                                    Err(e) => self.error = Some(format!("{e:#}")),
                                }
                            }
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(appearance::palette(ctx).canvas)
                    .inner_margin(if compact { 14.0 } else { 30.0 }),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(notice) = &self.notice {
                        ui.label(notice);
                        ui.add_space(10.0);
                    }
                    if let Some(error) = &self.error {
                        ui.group(|ui| {
                            ui.colored_label(
                                appearance::palette(ctx).error,
                                RichText::new("Action needed").strong(),
                            );
                            ui.label(error);
                        });
                        ui.add_space(14.0);
                    }
                    if self.donation {
                        self.donation_page(ui, ctx);
                    } else {
                        match self.page {
                            0 => self.input_page(ui, ctx),
                            1 => self.editions_page(ui),
                            2 => self.review_page(ui),
                            _ => self.build_page(ui),
                        }
                    }
                });
            });
        if ctx.input(|i| !i.raw.hovered_files.is_empty())
            && (self.page != 0
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
                if self.busy || self.scanning || self.dismounting || self.installing_adk {
                    "Finish the current operation first"
                } else {
                    "Drop one Windows ISO to start a new scan"
                },
                egui::FontId::proportional(23.0),
                Color32::WHITE,
            );
        }
        if self.confirm_dismount {
            let modal = egui::Modal::new(egui::Id::new("confirm-dismount")).show(ctx, |ui| {
                ui.set_max_width(380.0);
                ui.heading("Dismount this ISO?");
                ui.label(PathBuf::from(self.iso.trim()).file_name().unwrap_or_default().to_string_lossy());
                ui.label("This ejects the selected ISO from Windows and returns you to the first page. The ISO file stays unchanged.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.confirm_dismount = false;
                    }
                    if Self::primary(ui, "Dismount ISO", true) {
                        self.confirm_dismount = false;
                        self.dismount(ctx);
                    }
                });
            });
            if modal.should_close() {
                self.confirm_dismount = false;
            }
        }
        // Optional renderer-level screenshot for release QA; no OS screen scraping.
        if let Some(path) = std::env::var_os("MACBOOK_PATCHER_SCREENSHOT") {
            self.screenshot_frames += 1;
            if self.screenshot_frames > 3 && !self.screenshot_sent {
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
