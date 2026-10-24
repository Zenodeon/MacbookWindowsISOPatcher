use super::*;

impl PatcherApp {
    pub(super) fn model_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Choose your MacBook",
            "Choose your model. External-display graphics drivers are selected automatically.",
        );
        let colors = appearance::palette(ui.ctx());
        Self::section(ui, "MacBook models", colors.model, |ui| {
            for (index, model) in MODEL_PROFILES.iter().enumerate() {
                let checked = self.model_selected == Some(index);
                let width = ui.available_width();
                let inner_width = (width - 28.0).max(1.0);
                let title = ui.painter().layout(
                    model.name.into(),
                    egui::FontId::proportional(16.0),
                    ui.visuals().text_color(),
                    inner_width,
                );
                let details = ui.painter().layout(
                    format!("{} · {}", model.identifier, model.details),
                    egui::FontId::proportional(12.0),
                    colors.muted,
                    inner_width,
                );
                let status_height = 30.0;
                let height = 24.0 + title.size().y + details.size().y + status_height;
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
                let fill = if checked || response.hovered() {
                    colors.source
                } else {
                    colors.surface
                };
                ui.painter().rect(
                    rect,
                    10,
                    fill,
                    egui::Stroke::new(
                        1.0_f32,
                        if checked {
                            colors.accent
                        } else {
                            ui.visuals().widgets.noninteractive.bg_stroke.color
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                let origin = rect.min + egui::vec2(14.0, 10.0);
                ui.painter()
                    .galley(origin, title.clone(), ui.visuals().text_color());
                let detail_y = origin.y + title.size().y + 3.0;
                ui.painter().galley(
                    egui::pos2(origin.x, detail_y),
                    details.clone(),
                    colors.muted,
                );
                if response.clicked() {
                    if !checked {
                        self.drivers.selection_changed();
                    }
                    self.model_selected = Some(index);
                    self.error = None;
                }
                let status_rect = egui::Rect::from_min_size(
                    egui::pos2(origin.x, detail_y + details.size().y + 5.0),
                    egui::vec2(inner_width, status_height),
                );
                let mut row = ui.new_child(
                    egui::UiBuilder::new()
                        .id_salt(model.id)
                        .max_rect(status_rect)
                        .layout(
                            egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        ),
                );
                row.spacing_mut().button_padding = egui::vec2(10.0, 5.0);
                let badge = egui::Frame::new()
                    .fill(if model.hardware_verified() {
                        colors.ready
                    } else {
                        Color32::from_rgba_unmultiplied(
                            colors.warning.r(),
                            colors.warning.g(),
                            colors.warning.b(),
                            24,
                        )
                    })
                    .corner_radius(5)
                    .inner_margin(egui::Margin::symmetric(7, 3))
                    .show(&mut row, |ui| {
                        ui.label(
                            RichText::new(if model.hardware_verified() {
                                "Hardware Verified"
                            } else {
                                "Unverified"
                            })
                            .size(12.0)
                            .color(if model.hardware_verified() {
                                colors.success
                            } else {
                                colors.warning
                            }),
                        );
                    });
                badge.response.on_hover_text(if model.hardware_verified() {
                    "Windows 10 Pro was verified in the original project using external HDMI with the internal display disconnected. This app's generated ISO still needs hardware testing."
                } else {
                    "This model has not been hardware verified. You can build after its drivers finish downloading."
                });
                if checked {
                    row.label(RichText::new("Selected").size(12.0).color(colors.accent));
                }
                ui.add_space(6.0);
            }
            ui.hyperlink_to(
                "Identify your MacBook model",
                "https://support.apple.com/en-us/108052",
            );
        });
    }
    pub(super) fn section(
        ui: &mut egui::Ui,
        title: &str,
        fill: Color32,
        content: impl FnOnce(&mut egui::Ui),
    ) {
        egui::Frame::new()
            .fill(fill)
            .corner_radius(12)
            .inner_margin(18.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(title).size(18.0).strong());
                ui.add_space(4.0);
                content(ui);
            });
        ui.add_space(14.0);
    }

    pub(super) fn input_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        Self::heading(
            ui,
            "Start with a Windows ISO",
            "Drop a file or browse. Your original ISO stays unchanged.",
        );
        let colors = appearance::palette(ctx);
        Self::section(ui, "Selected Mac", colors.rail, |ui| {
            if let Some(model) = self.model() {
                ui.label(RichText::new(model.name).strong());
                ui.label(format!("{} · {}", model.identifier, model.details));
                if model.hardware_verified() {
                    ui.label("External HDMI fix for a disconnected internal display.");
                } else {
                    ui.colored_label(
                        colors.warning,
                        "Unverified external-display Setup recipe. Please report your test results.",
                    );
                }
                if model.id == "a1707-2016" {
                    ui.label(format!(
                        "Graphics package: Apple Boot Camp Radeon Pro {}.",
                        macbook_iso_patcher::amd::VERSION
                    ));
                } else if let Ok(graphics) =
                    macbook_iso_patcher::driver_prefetch::intel_graphics(model.id)
                {
                    ui.label(format!("Graphics package: Intel {}.", graphics.version));
                }
            }
        });
        Self::section(ui, "Source file", colors.source, |ui| {
            ui.add_enabled_ui(
                !self.scanning && !self.dismounting && !self.installing_adk,
                |ui| {
                    let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 98.0),
                        egui::Sense::click(),
                    );
                    ui.painter().rect_filled(rect, 8, colors.drop);
                    ui.painter().rect_stroke(
                        rect,
                        8,
                        egui::Stroke::new(
                            1.0_f32,
                            if hovering || response.hovered() {
                                colors.accent
                            } else {
                                ui.visuals().widgets.noninteractive.bg_stroke.color
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center() - egui::vec2(0.0, 12.0),
                        egui::Align2::CENTER_CENTER,
                        if hovering {
                            "Release to scan your ISO"
                        } else {
                            "Drop your ISO here"
                        },
                        egui::FontId::proportional(20.0),
                        ui.visuals().text_color(),
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0.0, 16.0),
                        egui::Align2::CENTER_CENTER,
                        "or click to browse",
                        egui::FontId::proportional(14.0),
                        colors.muted,
                    );
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Windows ISO", &["iso"])
                            .pick_file()
                    {
                        self.iso = platform::display_path(&path);
                        self.scan(ctx);
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.iso)
                                .hint_text("Or paste an ISO path…")
                                .desired_width((ui.available_width() - 105.0).max(120.0)),
                        );
                        if Self::primary(
                            ui,
                            "Scan ISO",
                            !self.iso.trim().is_empty() && cfg!(windows),
                        ) {
                            self.scan(ctx);
                        }
                    });
                },
            );
            if self.scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Reading Windows editions…");
                });
            } else {
                ui.label(
                    RichText::new("Windows 10 · 64-bit · Any language · Multiple editions")
                        .size(13.0)
                        .color(colors.muted),
                );
            }
            egui::CollapsingHeader::new("Compatibility details").show(ui, |ui| {
                ui.label("• Windows 10, 64-bit (x64), builds 19041–19045.");
                ui.label("• All languages and client editions supported.");
                ui.label("• Single-edition and multi-edition ISOs supported.");
                ui.label("• Windows 11 and split SWM images are not supported.");
            });
        });
        if !cfg!(windows) {
            ui.colored_label(
                colors.error,
                "ISO scanning and patching currently require Windows.",
            );
        }
    }

    pub(super) fn editions_page(&mut self, ui: &mut egui::Ui) {
        Self::heading(
            ui,
            "Choose Windows editions",
            "Include one edition or keep several choices in Windows Setup.",
        );
        let colors = appearance::palette(ui.ctx());
        if let Some(model) = self.model() {
            ui.label(RichText::new(model.name).color(colors.muted));
        }
        ui.add_space(12.0);
        let model_verified = self.model().is_some_and(|model| model.hardware_verified());
        Self::section(ui, "Windows editions", colors.source, |ui| {
            if let Some(inv) = &self.inventory {
                for image in &inv.images {
                    let reason = image.unsupported_reason();
                    let checked = self.selected.contains(&image.index);
                    let mut text = egui::text::LayoutJob::default();
                    text.append(
                        &format!("{}\n", image.name),
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(15.0),
                            color: ui.visuals().text_color(),
                            ..Default::default()
                        },
                    );
                    let verified = model_verified && image.source_tested() && reason.is_none();
                    text.append(
                        if verified {
                            "Hardware Verified"
                        } else {
                            "Unverified"
                        },
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(12.0),
                            color: if verified {
                                colors.success
                            } else {
                                colors.warning
                            },
                            ..Default::default()
                        },
                    );
                    if checked {
                        text.append(
                            "  ·  Selected",
                            0.0,
                            egui::TextFormat {
                                font_id: egui::FontId::proportional(12.0),
                                color: colors.accent,
                                ..Default::default()
                            },
                        );
                    }
                    if !image.language.is_empty() {
                        text.append(
                            &format!("  ·  {}", image.language),
                            0.0,
                            egui::TextFormat {
                                font_id: egui::FontId::proportional(12.0),
                                color: colors.muted,
                                ..Default::default()
                            },
                        );
                    }
                    if let Some(reason) = &reason {
                        text.append(
                            &format!("\n{reason}"),
                            0.0,
                            egui::TextFormat {
                                font_id: egui::FontId::proportional(12.0),
                                color: colors.muted,
                                ..Default::default()
                            },
                        );
                    }
                    let fill = if checked {
                        if ui.visuals().dark_mode {
                            Color32::from_rgb(43, 64, 89)
                        } else {
                            Color32::from_rgb(212, 230, 253)
                        }
                    } else {
                        colors.surface
                    };
                    let row = ui.add_enabled(
                        reason.is_none(),
                        egui::Button::new(text)
                            .selected(checked)
                            .fill(fill)
                            .corner_radius(10)
                            .stroke(egui::Stroke::new(
                                if checked { 1.5_f32 } else { 1.0_f32 },
                                if checked {
                                    colors.accent
                                } else {
                                    ui.visuals().widgets.noninteractive.bg_stroke.color
                                },
                            ))
                            .min_size(egui::vec2(ui.available_width(), 64.0))
                            .wrap(),
                    );
                    if row.on_hover_text(if verified {
                        "Windows 10 Pro was verified on A1706 hardware in the original project. ISOs generated by this app still need hardware testing. Click to select or deselect."
                    } else { "This edition has not been verified on the target hardware. Click to select or deselect." }).clicked() {
                        if checked { self.selected.remove(&image.index); } else { self.selected.insert(image.index); }
                    }
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new(format!("{} selected", self.selected.len())).strong());
        });
    }

    pub(super) fn review_page(&mut self, ui: &mut egui::Ui) {
        let hardware_verified = self.model().is_some_and(|model| model.hardware_verified());
        let description = if hardware_verified {
            "Iris graphics, the HDMI fix, and Boot Camp drivers will be included.".to_owned()
        } else if self.model().is_some_and(|model| model.id == "a1706-2017") {
            "Iris Plus 650 graphics 27.20.100.9316, external-display Setup startup, and Apple Boot Camp drivers will be included.".to_owned()
        } else if self.model().is_some_and(|model| model.id == "a1707-2016") {
            format!(
                "Radeon Pro graphics {}, external-display Setup startup, and Apple Boot Camp drivers will be included.",
                macbook_iso_patcher::amd::VERSION
            )
        } else {
            let version = self
                .model()
                .and_then(|model| {
                    macbook_iso_patcher::driver_prefetch::intel_graphics(model.id).ok()
                })
                .map(|graphics| graphics.version)
                .unwrap_or("");
            format!(
                "Intel graphics {version}, external-display Setup startup, and Apple Boot Camp drivers will be included."
            )
        };
        Self::heading(ui, "Review your build", &description);
        let colors = appearance::palette(ui.ctx());
        if let Some(model) = self.model() {
            ui.label(RichText::new(model.name).strong());
            if !hardware_verified {
                ui.colored_label(
                    colors.warning,
                    "Unverified hardware. You can build after the driver download finishes.",
                );
            }
            ui.hyperlink_to(
                "Report success or a problem on GitHub",
                format!("{REPO_URL}/issues/new"),
            )
            .on_hover_text("Include your Mac model, Windows edition, and what worked or failed.");
            ui.add_space(12.0);
        }
        Self::section(ui, "Included editions", colors.source, |ui| {
            if let Some(inv) = &self.inventory {
                for image in &inv.images {
                    if self.selected.contains(&image.index) {
                        ui.label(&image.name);
                    }
                }
            }
            if ui
                .add_enabled(
                    !self.closing && !self.dismounting && !self.installing_adk,
                    egui::Button::new("Edit editions"),
                )
                .clicked()
            {
                self.page = Page::Editions;
            }
        });
        let missing_tools = !PathBuf::from(&self.settings.oscdimg).is_file()
            || !PathBuf::from(&self.settings.seven_zip).is_file()
            || self.installing_adk;
        Self::section(
            ui,
            if missing_tools {
                "Missing downloads"
            } else {
                "Build tools ready"
            },
            if missing_tools {
                colors.missing
            } else {
                colors.ready
            },
            |ui| {
                if missing_tools {
                    ui.colored_label(
                        colors.warning,
                        "Install the missing tools or select their files below.",
                    );
                } else {
                    ui.colored_label(colors.success, "All required build tools are installed.");
                }
                if PathBuf::from(&self.settings.seven_zip).is_file() {
                    ui.colored_label(colors.success, "7-Zip · Installed");
                }
                if PathBuf::from(&self.settings.oscdimg).is_file() && !self.installing_adk {
                    ui.colored_label(colors.success, "ADK Deployment Tools · Installed");
                }
                if !PathBuf::from(&self.settings.seven_zip).is_file() {
                    if ui.button("Download 7-Zip").clicked() {
                        let _ = webbrowser::open(platform::SEVEN_ZIP_URL);
                    }
                    Self::file_field(
                        ui,
                        "7-Zip (7z.exe)",
                        &mut self.settings.seven_zip,
                        "exe",
                        false,
                    );
                }
                if !PathBuf::from(&self.settings.oscdimg).is_file() || self.installing_adk {
                    if ui
                        .add_enabled(
                            !self.installing_adk && !self.dismounting,
                            egui::Button::new("Download & install ADK tools"),
                        )
                        .clicked()
                    {
                        self.install_adk(ui.ctx());
                    }
                    ui.label(
                        RichText::new("Installs Microsoft Deployment Tools. Windows will request administrator approval.")
                            .size(13.0)
                            .color(colors.muted),
                    );
                    if self.installing_adk {
                        ui.horizontal_wrapped(|ui| {
                            ui.spinner();
                            ui.label(&self.adk_status);
                        });
                    }
                    ui.hyperlink_to(
                        "Microsoft download page / manual installation",
                        platform::ADK_URL,
                    );
                    Self::file_field(
                        ui,
                        "ISO tool (oscdimg.exe)",
                        &mut self.settings.oscdimg,
                        "exe",
                        false,
                    );
                }
                if missing_tools && ui.button("Detect installed tools again").clicked() {
                    self.settings.oscdimg = platform::find_oscdimg().display().to_string();
                    self.settings.seven_zip = platform::find_seven_zip().display().to_string();
                }
            },
        );
        Self::section(ui, "Build setup", colors.model, |ui| {
            egui::CollapsingHeader::new("Tools & download cache")
                .default_open(false)
                .show(ui, |ui| {
                    Self::file_field(
                        ui,
                        "7-Zip (7z.exe)",
                        &mut self.settings.seven_zip,
                        "exe",
                        false,
                    );
                    Self::file_field(
                        ui,
                        "Microsoft ADK (oscdimg.exe)",
                        &mut self.settings.oscdimg,
                        "exe",
                        false,
                    );
                    if ui.button("Detect installed tools again").clicked() {
                        self.settings.oscdimg = platform::find_oscdimg().display().to_string();
                        self.settings.seven_zip = platform::find_seven_zip().display().to_string();
                    }
                    ui.add_enabled_ui(false, |ui| {
                        Self::folder_field(ui, "Driver download cache", &mut self.settings.cache)
                    })
                    .response
                    .on_disabled_hover_text(macbook_iso_patcher::relocation::LOCKED_REASON);
                });
            egui::CollapsingHeader::new("What happens during the build?").show(ui, |ui| {
                ui.label("Downloads about 610 MB of verified Intel and Apple drivers once, then reuses the cache.");
                ui.label("Administrator approval is required. Your original ISO stays unchanged.");
            });
        });
        Self::section(ui, "Output & WorkingSpace", colors.surface, |ui| {
            Self::file_field(ui, "Save patched ISO", &mut self.output, "iso", true);
            self.workspace_field(ui);
            if let Ok(req) = self.request()
                && let Ok(space) = req.required_space()
            {
                ui.label(
                    RichText::new(format!(
                        "WorkingSpace: allow {} free, plus room for your output ISO.",
                        macbook_iso_patcher::cleanup::format_bytes(space)
                    ))
                    .size(13.0)
                    .color(colors.muted),
                );
            }
        });
    }
}
