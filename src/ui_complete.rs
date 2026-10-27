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
    pub(super) fn open_logging_usb_popup(&mut self) {
        self.usb_popup_open = true;
    }
    fn usb_preparation_allowed(&self) -> bool {
        !self.preparing_usb
            && !self.usb_copy.busy
            && !self.storage.deleting
            && !self.storage.moving
            && !self.closing
    }
    fn prepare_logging_usb(&mut self) {
        if !self.usb_preparation_allowed() {
            return;
        }
        let selected = rfd::FileDialog::new()
            .set_title("Choose the root of your logging USB")
            .pick_folder();
        let Some((id, root)) = self.select_logging_usb(selected) else {
            return;
        };
        self.spawn_usb_preparation(id, root);
    }
    fn spawn_usb_preparation(&self, id: u64, root: PathBuf) {
        let tx = self.sender.clone();
        std::thread::spawn(move || {
            let result = macbook_iso_patcher::diagnostics::prepare_usb(&root)
                .map(|()| root)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::UsbPrepared(id, result));
        });
    }
    fn select_logging_usb(&mut self, selected: Option<PathBuf>) -> Option<(u64, PathBuf)> {
        if !self.usb_preparation_allowed() {
            return None;
        }
        let path = selected?;
        self.usb_logging_id = self.usb_logging_id.wrapping_add(1);
        self.usb_logging = Some(LoggingUsb {
            path: path.clone(),
            status: UsbPreparationStatus::Preparing,
        });
        self.preparing_usb = true;
        self.usb_popup_origin = self.usb_popup_open;
        self.usb_error = None;
        self.usb_error_copied = false;
        self.notice = None;
        Some((self.usb_logging_id, path))
    }
    pub(super) fn reset_logging_usb(&mut self) {
        self.usb_copy.reset();
        self.usb_logging_id = self.usb_logging_id.wrapping_add(1);
        self.usb_logging = None;
        self.preparing_usb = false;
        self.usb_error = None;
        self.usb_error_copied = false;
        self.usb_popup_origin = false;
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
                self.usb_error = Some(error.clone());
                self.usb_error_copied = false;
                if !self.usb_popup_open && !self.usb_popup_origin {
                    self.show_error(error);
                }
            }
        }
    }
    fn logging_usb_selection(&self, ui: &mut egui::Ui) {
        let Some(selected) = &self.usb_logging else {
            return;
        };
        let colors = appearance::palette(ui.ctx());
        egui::Frame::new()
            .fill(colors.source)
            .corner_radius(10)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label("Selected USB");
                    let (label, color) = match selected.status {
                        UsbPreparationStatus::Preparing => ("Preparing USB…", colors.muted),
                        UsbPreparationStatus::Prepared => ("USB prepared", colors.success),
                        UsbPreparationStatus::Failed => ("Preparation failed", colors.warning),
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
    pub(super) fn logging_usb_popup(&mut self, ctx: &egui::Context) {
        if !self.usb_popup_open {
            return;
        }
        let colors = appearance::palette(ctx);
        let mut choose = false;
        let mut retry = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("logging-usb"))
            .frame(egui::Frame::new().fill(colors.surface).corner_radius(16).inner_margin(22.0))
            .show(ctx, |ui| {
                let size = egui::vec2(
                    (ctx.content_rect().width() - 88.0).clamp(240.0, 570.0),
                    (ctx.content_rect().height() - 100.0).clamp(220.0, 410.0),
                );
                let (shell, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let footer = egui::Rect::from_min_max(egui::pos2(shell.left(), shell.bottom()-38.0), shell.max);
                let body = egui::Rect::from_min_max(shell.min, egui::pos2(shell.right(), footer.top()-16.0));
                let mut content = ui.new_child(egui::UiBuilder::new().max_rect(body));
                content.set_clip_rect(body.expand(3.0).intersect(ui.clip_rect()));
                egui::ScrollArea::vertical().id_salt("logging-usb-body").auto_shrink([false, false]).show(&mut content, |ui| {
                    ui.label(RichText::new("Prepare USB for setup logs").size(25.0).strong());
                    ui.add_space(8.0);
                    ui.label("Setup logs help us troubleshoot problems and improve support for your Mac.");
                    ui.add_space(10.0);
                    ui.label("1. Install Ventoy on your USB.");
                    ui.add_space(8.0);
                    ui.label("2. Copy the patched ISO to the main USB partition, not VTOYEFI.");
                    ui.add_space(8.0);
                    ui.label("3. Choose that USB’s root folder below, such as E:\\.");
                    ui.add_space(14.0);
                    self.logging_usb_selection(ui);
                    if let Some(error) = self.usb_error.clone() {
                        ui.add_space(8.0);
                        ui.add(egui::Label::new(RichText::new(&error).color(colors.warning)).wrap().selectable(true));
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(if self.usb_error_copied { "Copied" } else { "Copy error" }).clicked() {
                                ui.ctx().copy_text(error.clone());
                                self.usb_error_copied = true;
                            }
                            retry = ui.add_enabled(self.usb_preparation_allowed(), egui::Button::new("Retry")).clicked();
                        });
                    }
                    if self.storage.deleting || self.storage.moving {
                        ui.add_space(8.0);
                        ui.label("Wait for storage cleanup or the folder move to finish before preparing a USB.");
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new("Adds a logging marker. Keep the USB connected during Windows Setup.").size(13.0).color(colors.muted));
                });
                let mut actions = ui.new_child(egui::UiBuilder::new().max_rect(footer));
                actions.horizontal(|ui| {
                    choose = ui.add_enabled(self.usb_preparation_allowed(), egui::Button::new("Choose USB folder")).clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { close = ui.button("Close").clicked(); });
                });
            });
        if close || modal.should_close() {
            self.usb_popup_open = false;
        }
        if choose {
            self.prepare_logging_usb();
        }
        if retry {
            let path = self
                .usb_logging
                .as_ref()
                .map(|selected| selected.path.clone());
            if let Some((id, root)) = self.select_logging_usb(path) {
                self.spawn_usb_preparation(id, root);
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
        self.completion_link(&completion::issue_draft_for_target(
            model,
            &images,
            self.display_target(),
        ));
    }
    pub(super) fn complete_page(&mut self, ui: &mut egui::Ui) {
        let colors = appearance::palette(ui.ctx());
        let recipe = self
            .model()
            .and_then(|m| macbook_iso_patcher::setup_recipe::SetupRecipe::for_model(m.id));
        Self::heading(
            ui,
            "Your ISO is ready",
            "Save the build logs, then follow these steps on your Mac.",
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
                if model.id == IMAC_MODEL_ID {
                    ui.label(format!(
                        "Windows Setup screen: {}",
                        self.display_target().label()
                    ));
                }
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
        for (index, section) in completion::INSTALL_SECTIONS.iter().enumerate() {
            let color = match index {
                0 | 4 => colors.source,
                1 => colors.surface,
                _ => colors.model,
            };
            Self::section(
                ui,
                &format!("{}. {}", index + 1, section.title),
                color,
                |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    for (step_index, step) in section.steps.iter().enumerate() {
                        ui.label(format!("{}.{} - {step}", index + 1, step_index + 1));
                    }
                    if index == 1 {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Download Ventoy").clicked() {
                                self.completion_link(completion::VENTOY_URL);
                            }
                            if ui.button("Prepare USB").clicked() {
                                self.open_copy_usb();
                            }
                        });
                        ui.add_space(10.0);
                        ui.label("Manual Route (optional)");
                        ui.label(completion::MANUAL_USB_INSTRUCTIONS);
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(
                                    !self.closing && !self.dismounting,
                                    egui::Button::new("Open ISO folder"),
                                )
                                .clicked()
                            {
                                self.open_finished_iso();
                            }
                            if ui.button("Prepare USB for setup logs").clicked() {
                                self.open_logging_usb_popup();
                            }
                        });
                    }
                    if index == 2 && self.model().is_some_and(|m| m.id == IMAC_MODEL_ID) {
                        ui.label(format!("For this iMac, use the {} selected for Windows Setup. Use a wired USB keyboard and mouse. Desktop display settings can be changed later in Windows.", self.display_target().label().to_lowercase()));
                    }
                    if index == 5 && recipe.is_some() {
                        ui.label(
                            RichText::new(
                                "Keep the included graphics driver for your selected Mac model.",
                            )
                            .size(13.0)
                            .color(colors.muted),
                        );
                    }
                },
            );
        }
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
    fn logging_entry_points_open_shared_dialog_without_restarting_preparation() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        app.open_logging_usb_popup();
        assert!(app.usb_popup_open && app.usb_logging.is_none());
        let (id, path) = app.select_logging_usb(Some("E:/".into())).unwrap();
        app.usb_popup_open = false;
        app.page = Page::Complete;
        app.open_logging_usb_popup();
        assert_eq!(app.usb_logging_id, id);
        assert!(app.preparing_usb);
        app.finish_logging_usb(id, Ok(path.clone()));
        app.usb_popup_open = false;
        app.open_logging_usb_popup();
        assert_eq!(app.usb_logging.as_ref().unwrap().path, path);
        assert_eq!(
            app.usb_logging.as_ref().unwrap().status,
            UsbPreparationStatus::Prepared
        );
        assert!(!app.preparing_usb);
    }
    #[test]
    fn standalone_usb_has_no_model_or_workflow_requirement_but_excludes_cleanup() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        assert!(app.model().is_none());
        for page in [
            Page::Model,
            Page::Iso,
            Page::Review,
            Page::Build,
            Page::Complete,
        ] {
            app.page = page;
            app.busy = true;
            app.drivers.running = true;
            assert!(app.usb_preparation_allowed());
        }
        app.storage.deleting = true;
        assert!(app.select_logging_usb(Some("E:/".into())).is_none());
        app.storage.deleting = false;
        app.storage.moving = true;
        assert!(!app.usb_preparation_allowed());
        app.storage.moving = false;
        app.busy = false;
        app.drivers.running = false;
        app.page = Page::Model;
        assert!(app.select_logging_usb(Some("E:/".into())).is_some());
        assert!(app.storage_blocked());
        assert!(!app.storage_move_gate().idle);
    }
    #[test]
    fn popup_can_close_and_reopen_without_losing_task_or_result() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        app.usb_popup_open = true;
        let (id, path) = app.select_logging_usb(Some("E:/".into())).unwrap();
        app.usb_popup_open = false;
        assert!(app.preparing_usb);
        app.finish_logging_usb(id, Err("USB is read-only".into()));
        assert!(app.error.is_none());
        app.usb_popup_open = true;
        assert_eq!(app.usb_error.as_deref(), Some("USB is read-only"));
        assert!(app.select_logging_usb(None).is_none());
        assert!(app.usb_error.is_some());
        let (retry, selected) = app.select_logging_usb(Some(path.clone())).unwrap();
        assert_eq!(selected, path);
        assert!(app.usb_error.is_none());
        app.finish_logging_usb(retry, Ok(path.clone()));
        assert_eq!(app.usb_logging.as_ref().unwrap().path, path);
        assert_eq!(
            app.usb_logging.as_ref().unwrap().status,
            UsbPreparationStatus::Prepared
        );
        app.reset_logging_usb();
        assert!(app.usb_error.is_none() && app.usb_logging.is_none());
    }
    #[test]
    fn active_popup_receives_completion_card_errors_without_duplicate_dialog() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        let (id, _) = app.select_logging_usb(Some("E:/".into())).unwrap();
        app.usb_popup_open = true;
        app.finish_logging_usb(id, Err("USB disconnected".into()));
        assert!(app.error.is_none());
        assert_eq!(app.usb_error.as_deref(), Some("USB disconnected"));
    }
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
