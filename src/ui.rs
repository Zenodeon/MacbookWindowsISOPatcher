use eframe::egui::{self, Color32, RichText, Vec2};
use macbook_iso_patcher::{domain::*, engine, ipc, platform};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};

const BLUE: Color32 = Color32::from_rgb(24, 76, 145);
const MUTED: Color32 = Color32::from_rgb(78, 88, 103);
enum Message {
    Scanned(Result<Inventory, String>),
    Build(Event),
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Settings {
    work: String,
    cache: String,
    seven_zip: String,
    oscdimg: String,
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
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = Color32::from_rgb(248, 249, 251);
        style.visuals.override_text_color = Some(Color32::from_rgb(29, 38, 51));
        style.visuals.selection.bg_fill = BLUE;
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style.spacing.button_padding = Vec2::new(14.0, 9.0);
        style.spacing.interact_size.y = 34.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        cc.egui_ctx.set_style(style);
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
        ui.label(RichText::new(description).color(MUTED));
        ui.add_space(20.0);
    }
    fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
        ui.add_enabled(
            enabled,
            egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(BLUE),
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
    fn input_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        Self::heading(
            ui,
            "Start with your Windows ISO",
            "Choose original Microsoft media. We’ll scan the editions before making any changes.",
        );
        ui.add_enabled_ui(!self.scanning, |ui| {
            Self::file_field(ui, "Windows ISO", &mut self.iso, "iso", false);
            ui.add_space(12.0);
            if Self::primary(
                ui,
                "Scan Windows editions",
                !self.iso.trim().is_empty() && cfg!(windows),
            ) {
                self.scan(ctx);
            }
        });
        if self.scanning {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Mounting the ISO read-only and reading image metadata…");
            });
        }
        ui.add_space(28.0);
        ui.separator();
        ui.add_space(16.0);
        ui.label(RichText::new("First supported model").strong());
        ui.label(MODEL_NAME);
        ui.label("A1706 · MacBookPro13,2 · Intel Iris Graphics 550 · T1");
        ui.add_space(12.0);
        ui.label("This profile reproduces the v17 external HDMI fix for a Mac with its internal display disconnected.");
        ui.label(RichText::new("Supported media: x64 Windows 10, en-US, build 19041–19045. Windows 11 and split SWM images are not supported yet.").color(MUTED));
        if !cfg!(windows) {
            ui.colored_label(Color32::DARK_RED, "This build can show the interface, but ISO scanning and patching currently require Windows.");
        }
    }
    fn editions_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Choose your Mac and Windows editions",
            "Include one edition or keep several choices in Windows Setup.",
        );
        ui.label(RichText::new("MacBook model").strong());
        egui::ComboBox::from_id_salt("model")
            .selected_text("A1706 — MacBook Pro 13-inch, 2016")
            .width(420.0)
            .show_ui(ui, |ui| {
                ui.selectable_label(true, "A1706 — MacBook Pro 13-inch, 2016")
                    .clicked();
            });
        ui.label(RichText::new("MacBookPro13,2 · Iris 550 · Touch Bar · T1 (not T2)").color(MUTED));
        ui.add_space(18.0);
        ui.label(RichText::new("Editions found in your ISO").strong());
        if let Some(inv) = &self.inventory {
            for image in &inv.images {
                let reason = image.unsupported_reason();
                let enabled = reason.is_none();
                let mut checked = self.selected.contains(&image.index);
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(enabled, egui::Checkbox::new(&mut checked, &image.name)).changed() {
                            if checked { self.selected.insert(image.index); } else { self.selected.remove(&image.index); }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("{} · build {}", image.language, image.build)).size(13.0).color(MUTED));
                        });
                    });
                    let status = reason.unwrap_or_else(|| if image.source_tested() { "Pro was validated in the source project; this app’s output still needs hardware testing.".into() } else { "Compatible metadata · hardware testing pending".into() });
                    ui.label(RichText::new(status).size(13.0).color(MUTED));
                });
            }
        }
        ui.add_space(10.0);
        ui.label(format!("{} edition(s) selected", self.selected.len()));
    }
    fn review_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Review your patched ISO",
            "The build adds the signed Iris driver, the v17 HDMI startup sequence, and Apple Boot Camp drivers.",
        );
        if let Some(inv) = &self.inventory {
            ui.label(RichText::new("Included editions").strong());
            for i in &inv.images {
                if self.selected.contains(&i.index) {
                    ui.label(&i.name);
                }
            }
        }
        ui.add_space(12.0);
        Self::file_field(ui, "Save patched ISO", &mut self.output, "iso", true);
        ui.add_space(12.0);
        Self::folder_field(
            ui,
            "Working folder (large temporary files)",
            &mut self.settings.work,
        );
        if let Ok(req) = self.request()
            && let Ok(space) = req.required_space()
        {
            ui.label(RichText::new(format!("Allow at least {:.1} GiB free in the working folder, plus space for the output ISO.", space as f64 / GIB as f64)).color(MUTED));
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new("Build tools and download cache").default_open(!PathBuf::from(&self.settings.oscdimg).is_file()).show(ui, |ui| {
            Self::file_field(ui, "7-Zip (7z.exe)", &mut self.settings.seven_zip, "exe", false);
            if !PathBuf::from(&self.settings.seven_zip).is_file() && ui.link("Install 7-Zip").clicked() { let _ = webbrowser::open(platform::SEVEN_ZIP_URL); }
            Self::file_field(ui, "Microsoft ADK (oscdimg.exe)", &mut self.settings.oscdimg, "exe", false);
            if !PathBuf::from(&self.settings.oscdimg).is_file() {
                ui.label("Install Windows ADK and select Deployment Tools. The WinPE add-on is not required.");
                if ui.link("Open Microsoft ADK installation guide").clicked() { let _ = webbrowser::open(platform::ADK_URL); }
                if ui.button("Detect installed tools again").clicked() {
                    self.settings.oscdimg = platform::find_oscdimg().display().to_string();
                    self.settings.seven_zip = platform::find_seven_zip().display().to_string();
                }
            }
            Self::folder_field(ui, "Verified driver download cache", &mut self.settings.cache);
        });
        ui.add_space(12.0);
        ui.label("Driver download: Intel 15.45.34.5174 and Apple Boot Camp 041-88734 (about 610 MB total). Verified downloads are reused.");
        ui.label(RichText::new("Administrator approval is needed for image servicing. Partition selection stays manual. Your original ISO is preserved.").color(MUTED));
    }
    fn donation_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        Self::heading(
            ui,
            "Support the project, if you’d like",
            "Your ISO will include multiple Windows editions. Donations help support the patcher.",
        );
        ui.label("Payment is optional. Both options below let you build the same ISO.");
        ui.add_space(22.0);
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
            RichText::new("The app does not check or record whether you donate.").color(MUTED),
        );
    }
    fn build_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(ui, &self.stage, &self.detail);
        if self.busy {
            ui.horizontal(|ui| { ui.spinner(); ui.label(if self.cancel.is_cancelled() { "Cancellation requested. Waiting for the current servicing operation to finish safely…" } else { "Keep the app open while the build is running." }); });
        }
        if let Some((name, received, total)) = &self.download {
            ui.label(name);
            if let Some(total) = total.filter(|t| *t > 0) {
                ui.add(egui::ProgressBar::new(*received as f32 / total as f32).show_percentage());
            }
            ui.label(format!(
                "{:.1} MB downloaded",
                *received as f64 / 1_000_000.0
            ));
        }
        if let Some(path) = &self.finished {
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
            ui.label(RichText::new("Hardware testing of this app’s output remains required. Do not run the old Intel graphics installer.").color(MUTED));
        }
        ui.add_space(12.0);
        egui::CollapsingHeader::new("Build log")
            .default_open(true)
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
    }
}

impl eframe::App for PatcherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.consume();
        if self.busy || self.scanning {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if self.busy && ctx.input(|i| i.viewport().close_requested()) {
            self.cancel.cancel();
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        egui::SidePanel::left("workflow")
            .exact_width(205.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(234, 238, 244))
                    .inner_margin(22.0),
            )
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("MacBook\nISO Patcher").size(24.0).strong());
                ui.add_space(32.0);
                for (n, label) in ["Windows ISO", "Mac & editions", "Review", "Build"]
                    .iter()
                    .enumerate()
                {
                    let current = self.page == n;
                    ui.label(
                        RichText::new(*label)
                            .size(16.0)
                            .color(if current { BLUE } else { MUTED })
                            .strong(),
                    );
                    if current {
                        ui.label(RichText::new("Current step").size(12.0).color(MUTED));
                    }
                    ui.add_space(16.0);
                }
                ui.add_space(24.0);
                ui.separator();
                ui.add_space(10.0);
                ui.label(
                    RichText::new("A1706 profile\nv17 HDMI fix")
                        .size(13.0)
                        .color(MUTED),
                );
                ui.label(
                    RichText::new("v0.1.0 · Rust desktop")
                        .size(12.0)
                        .color(MUTED),
                );
            });
        egui::TopBottomPanel::bottom("actions")
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(16.0))
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
                            .button(if self.donation {
                                "Back to review"
                            } else {
                                "Back"
                            })
                            .clicked()
                    {
                        if self.donation {
                            self.donation = false;
                        } else {
                            self.page -= 1;
                        }
                        self.error = None;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.busy && !self.scanning && !self.donation {
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
                    .fill(Color32::from_rgb(248, 249, 251))
                    .inner_margin(30.0),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(error) = &self.error {
                        ui.group(|ui| {
                            ui.colored_label(
                                Color32::from_rgb(155, 33, 33),
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
