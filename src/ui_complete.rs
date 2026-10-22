use super::*;
use macbook_iso_patcher::completion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UsbPreparationStatus {
    Preparing,
    Prepared,
    Failed,
}
pub(super) struct LoggingUsb {
    pub path: PathBuf,
    pub status: UsbPreparationStatus,
}

impl PatcherApp {
    fn prepare_logging_usb(&mut self) {
        if self.preparing_usb {
            return;
        }
        let selected = rfd::FileDialog::new()
            .set_title("Choose the root of your logging USB")
            .pick_folder();
        let Some((id, root)) = self.select_logging_usb(selected) else {
            return;
        };
        let tx = self.sender.clone();
        std::thread::spawn(move || {
            let result = macbook_iso_patcher::diagnostics::prepare_usb(&root)
                .map(|()| root)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::UsbPrepared(id, result));
        });
    }
    fn select_logging_usb(&mut self, selected: Option<PathBuf>) -> Option<(u64, PathBuf)> {
        if self.preparing_usb {
            return None;
        }
        let path = selected?;
        self.usb_logging_id = self.usb_logging_id.wrapping_add(1);
        self.usb_logging = Some(LoggingUsb {
            path: path.clone(),
            status: UsbPreparationStatus::Preparing,
        });
        self.preparing_usb = true;
        self.notice = None;
        Some((self.usb_logging_id, path))
    }
    pub(super) fn reset_logging_usb(&mut self) {
        self.usb_logging_id = self.usb_logging_id.wrapping_add(1);
        self.usb_logging = None;
        self.preparing_usb = false;
    }
    pub(super) fn finish_logging_usb(&mut self, id: u64, result: Result<PathBuf, String>) {
        if id != self.usb_logging_id || self.usb_logging.is_none() {
            return;
        }
        self.preparing_usb = false;
        let selected = self.usb_logging.as_mut().unwrap();
        match result {
            Ok(path) => {
                selected.path = path;
                selected.status = UsbPreparationStatus::Prepared;
                self.notice = None;
            }
            Err(error) => {
                selected.status = UsbPreparationStatus::Failed;
                self.show_error(error);
            }
        }
    }
    pub(super) fn save_build_logs(&mut self) {
        let export = completion::log_export(self.build_files.as_ref(), &self.logs);
        let partial = export.partial_reason.is_some();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Build log", &["log"])
            .set_title(if partial {
                "Save partial build log — full log unavailable"
            } else {
                "Save complete build log"
            })
            .set_file_name(if partial {
                "macbook-patcher-partial.log"
            } else {
                "macbook-patcher-build.log"
            })
            .save_file()
        else {
            return;
        };
        match completion::save_log(&path, &export) {
            Ok(()) => {
                self.notice = Some(
                    if partial {
                        "Saved a partial build log. The full retained log was unavailable."
                    } else {
                        "Build log saved."
                    }
                    .into(),
                )
            }
            Err(error) => self.show_error(format!("{error:#}")),
        }
    }
    pub(super) fn open_finished_iso(&mut self) {
        let Some(path) = self.finished.clone() else {
            self.show_error(
                "The generated ISO location is unavailable. Check your selected output folder.",
            );
            return;
        };
        if let Err(error) = platform::reveal_file(&path) {
            self.show_error(format!("Could not open the generated ISO location. If the ISO was moved or deleted, restore it to {} or open its new location manually.\n\n{error:#}",platform::display_path(&path)));
        }
    }
    fn completion_link(&mut self, url: &str) {
        if let Err(error) = webbrowser::open(url) {
            self.show_error(format!("Could not open the link: {error}"));
        }
    }
    fn report_setup_issue(&mut self) {
        let Some(model) = self.model() else {
            return;
        };
        let images = self
            .inventory
            .as_ref()
            .map(|inventory| {
                inventory
                    .images
                    .iter()
                    .filter(|image| self.selected.contains(&image.index))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.completion_link(&completion::issue_draft(model, &images));
    }
    pub(super) fn complete_page(&mut self, ui: &mut egui::Ui) {
        let colors = appearance::palette(ui.ctx());
        let recipe = self
            .model()
            .and_then(|m| macbook_iso_patcher::setup_recipe::SetupRecipe::for_model(m.id));
        Self::heading(
            ui,
            "Your ISO is ready",
            "Save the build logs, then follow these steps on your MacBook.",
        );
        Self::section(ui, "Build complete", colors.ready, |ui| {
            if let Some(path) = &self.finished {
                ui.label(
                    RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).strong(),
                );
                ui.label(
                    RichText::new(platform::display_path(path))
                        .size(13.0)
                        .color(muted(ui)),
                );
            }
            if let Some(model) = self.model() {
                ui.label(model.name);
                ui.label(format!("{} · {}", model.identifier, model.details));
            }
            if let Some(inventory) = &self.inventory {
                let editions = inventory
                    .images
                    .iter()
                    .filter(|image| self.selected.contains(&image.index))
                    .map(|image| image.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" · ");
                ui.label(editions);
            }
        });
        Self::section(ui, "Install Windows", colors.source, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for (index, step) in completion::INSTALL_STEPS[..4].iter().enumerate() {
                ui.label(format!("{}. {step}", index + 1));
            }
            if ui.button("Download Ventoy").clicked() {
                self.completion_link(completion::VENTOY_URL);
            }
        });
        if let Some(recipe) = recipe {
            Self::section(ui, "Prepare USB for setup logs", colors.surface, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label("If Windows Setup doesn’t work, these logs help us understand why and update the patcher to fix issues affecting your Mac model.");
                ui.label("Setup support logs are saved only to a marked USB. Prepare it here, or copy MacBookPatcher-Logging.enabled from the ISO’s Support folder to the USB root.");
                if ui
                    .add_enabled(
                        !self.preparing_usb,
                        egui::Button::new(if self.preparing_usb {
                            "Preparing USB…"
                        } else {
                            "Prepare USB for setup logs"
                        }),
                    )
                    .clicked()
                {
                    self.prepare_logging_usb();
                }
                if let Some(selected) = &self.usb_logging {
                    egui::Frame::new()
                        .fill(colors.source)
                        .corner_radius(10)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Selected USB").strong());
                                let (label, color) = match selected.status {
                                    UsbPreparationStatus::Preparing => {
                                        ("Preparing USB…", colors.muted)
                                    }
                                    UsbPreparationStatus::Prepared => {
                                        ("USB prepared", colors.success)
                                    }
                                    UsbPreparationStatus::Failed => {
                                        ("Preparation failed", colors.warning)
                                    }
                                };
                                if selected.status == UsbPreparationStatus::Preparing {
                                    ui.spinner();
                                }
                                ui.colored_label(color, label);
                            });
                            ui.add(
                                egui::Label::new(platform::display_path(&selected.path))
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                }
                ui.label("Keep the prepared USB connected during Windows Setup.");
                ui.label(format!("Logs: MacBookPatcher-Logs\\{}. Automatic collection ends at reboot; use Support\\Collect setup logs.cmd to recover saved Setup logs afterward.",recipe.log_folder()));
            });
        }

        Self::section(ui, "Finish setup in Windows", colors.model, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for (index, step) in completion::INSTALL_STEPS[4..].iter().enumerate() {
                ui.label(format!("{}. {step}", index + 5));
            }
            if recipe.is_some() {
                ui.label(RichText::new("Keep the included Intel graphics driver. Do not run the old IntelHDGraphics64 installer.").size(13.0).color(muted(ui)));
            }
        });
        Self::section(ui, "Something did not work?", colors.surface, |ui| {
            ui.label("Support varies by Mac model and Windows edition. Describe the failure and attach your saved build log and any Windows Setup/startup logs.");
            if let Some(recipe) = recipe {
                ui.label(format!("For boot/setup failures, attach the session folder in MacBookPatcher-Logs\\{} and your saved build log.",recipe.log_folder()));
                if self.model().is_some_and(|model| model.id == MODEL_ID) {
                    ui.label("Older ISOs may use A1706Logs.");
                }
            }
            if self.model().is_some_and(|model| !model.hardware_verified()) {
                ui.colored_label(
                    colors.warning,
                    "This Mac model has not been hardware verified. Please report your results.",
                );
            }
            if ui.button("Share logs / report an issue")
                .on_hover_text("Opens a GitHub issue draft with your Mac model and Windows details. Attach your setup logs and saved build log yourself; nothing is uploaded automatically.")
                .clicked() {
                self.report_setup_issue();
            }
        });
        ui.push_id("completion-log", |ui| {
            if build_log::build_log_view(ui, &self.logs, false).save_requested {
                self.save_build_logs();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usb_selection_is_immediate_and_only_worker_success_marks_it_prepared() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        let path = PathBuf::from(r"\\?\E:\");
        let (id, selected) = app.select_logging_usb(Some(path.clone())).unwrap();
        assert_eq!(selected, path);
        assert_eq!(
            platform::display_path(&app.usb_logging.as_ref().unwrap().path),
            r"E:\"
        );
        assert!(app.preparing_usb);
        assert_eq!(
            app.usb_logging.as_ref().unwrap().status,
            UsbPreparationStatus::Preparing
        );
        assert!(app.select_logging_usb(Some("F:/".into())).is_none());
        app.sender.send(Message::UsbPrepared(id, Ok(path))).unwrap();
        app.consume();
        assert!(!app.preparing_usb);
        assert_eq!(
            app.usb_logging.as_ref().unwrap().status,
            UsbPreparationStatus::Prepared
        );
    }
    #[test]
    fn failed_usb_preparation_keeps_path_and_copyable_error() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        let path = PathBuf::from("E:/");
        let (id, _) = app.select_logging_usb(Some(path.clone())).unwrap();
        app.sender
            .send(Message::UsbPrepared(id, Err("USB is read-only".into())))
            .unwrap();
        app.consume();
        assert!(!app.preparing_usb);
        let selected = app.usb_logging.as_ref().unwrap();
        assert_eq!(selected.path, path);
        assert_eq!(selected.status, UsbPreparationStatus::Failed);
        let error = app.error.as_ref().unwrap();
        assert_eq!(error.message, "USB is read-only");
        assert!(!error.copied);
    }
    #[test]
    fn picker_cancellation_keeps_selection_and_replacement_sets_pending() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        assert!(app.select_logging_usb(None).is_none());
        assert!(app.usb_logging.is_none());
        let path = PathBuf::from("E:/");
        let (id, _) = app.select_logging_usb(Some(path.clone())).unwrap();
        app.finish_logging_usb(id, Ok(path.clone()));
        assert!(app.select_logging_usb(None).is_none());
        assert_eq!(app.usb_logging.as_ref().unwrap().path, path);
        assert_eq!(
            app.usb_logging.as_ref().unwrap().status,
            UsbPreparationStatus::Prepared
        );
        app.select_logging_usb(Some("F:/".into())).unwrap();
        assert_eq!(app.usb_logging.as_ref().unwrap().path, PathBuf::from("F:/"));
        assert_eq!(
            app.usb_logging.as_ref().unwrap().status,
            UsbPreparationStatus::Preparing
        );
        app.finish_logging_usb(id, Ok(path));
        assert!(app.preparing_usb); // Earlier completion cannot replace the new selection.
    }
    #[test]
    fn new_build_and_workflow_reset_clear_usb_state_and_ignore_late_results() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        for build in [false, true] {
            let (id, path) = app.select_logging_usb(Some("E:/".into())).unwrap();
            if build {
                app.reset_build_state(std::path::Path::new("C:/preview-workspace"), 12);
            } else {
                app.sender
                    .send(Message::Dismounted(Ok(true), None))
                    .unwrap();
                app.consume();
            }
            assert!(app.usb_logging.is_none() && !app.preparing_usb);
            app.sender.send(Message::UsbPrepared(id, Ok(path))).unwrap();
            app.consume();
            assert!(app.usb_logging.is_none() && !app.preparing_usb);
        }
    }
    #[test]
    fn completion_scroll_resets_from_a_scrolled_build_page() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        app.page = Page::Build;
        let mut offset = 0.0;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 300.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    offset = app
                        .page_scroll()
                        .max_height(100.0)
                        .vertical_scroll_offset(200.0)
                        .show(ui, |ui| {
                            for _ in 0..50 {
                                ui.label("Retained build log line");
                            }
                        })
                        .state
                        .offset
                        .y;
                });
            },
        );
        assert_eq!(offset, 200.0);
        app.sender
            .send(Message::Build(Event::Finished("test.iso".into())))
            .unwrap();
        app.consume();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 300.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    offset = app
                        .page_scroll()
                        .max_height(100.0)
                        .show(ui, |ui| {
                            for _ in 0..50 {
                                ui.label("Completion instructions");
                            }
                        })
                        .state
                        .offset
                        .y;
                });
            },
        );
        assert_eq!(offset, 0.0);
    }
    #[test]
    fn finished_navigates_to_completion_and_missing_iso_keeps_page_actionable() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        app.page = Page::Build;
        app.scroll_page = (Page::Build, false);
        app.busy = true;
        app.sender
            .send(Message::Build(Event::Finished(
                std::env::temp_dir().join(format!("missing-{}.iso", uuid::Uuid::new_v4())),
            )))
            .unwrap();
        app.consume();
        assert_eq!(app.page, Page::Complete);
        assert_ne!(app.scroll_page, (app.page, app.donation)); // Existing navigation resets page scroll.
        assert!(!app.busy && !app.can_retry_build());
        app.open_finished_iso();
        assert_eq!(app.page, Page::Complete);
        assert!(
            app.error
                .as_ref()
                .unwrap()
                .message
                .contains("moved or deleted")
        );
    }
}
