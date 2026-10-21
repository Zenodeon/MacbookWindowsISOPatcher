use super::*;
use macbook_iso_patcher::completion;

impl PatcherApp {
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
    pub(super) fn complete_page(&mut self, ui: &mut egui::Ui) {
        let colors = appearance::palette(ui.ctx());
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
        Self::section(ui, "Finish setup in Windows", colors.model, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for (index, step) in completion::INSTALL_STEPS[4..].iter().enumerate() {
                ui.label(format!("{}. {step}", index + 5));
            }
            if self.model().is_some_and(|model| model.id == MODEL_ID) {
                ui.label(RichText::new("Keep the included Intel graphics driver. Do not run the old IntelHDGraphics64 installer.").size(13.0).color(muted(ui)));
            }
        });
        Self::section(ui, "Something did not work?", colors.surface, |ui| {
            ui.label("Support varies by Mac model and Windows edition. Describe the failure and attach your saved build log and any Windows Setup/startup logs.");
            if self.model().is_some_and(|model| model.id == MODEL_ID) {
                ui.label("For A1706 boot/setup failures, also attach the USB's A1706Logs folder when available.");
            }
            if self.model().is_some_and(|model| !model.hardware_verified()) {
                ui.colored_label(
                    colors.warning,
                    "This Mac model has not been hardware verified. Please report your results.",
                );
            }
            if ui.button("Report an issue").clicked()
                && let Some(model) = self.model()
            {
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
