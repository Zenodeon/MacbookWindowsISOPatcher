use super::*;
use macbook_iso_patcher::cleanup::{self, CleanupRequest, OutputCandidate};

pub(super) struct OutputDialog {
    pub path: PathBuf,
    pub retry: BuildRetry,
    pub candidate: Option<OutputCandidate>,
    pub checking: bool,
    pub confirming: bool,
    pub problem: Option<String>,
    pub copied: bool,
}
impl PatcherApp {
    pub(super) fn handle_preflight_error(
        &mut self,
        error: anyhow::Error,
        retry: BuildRetry,
        ctx: &egui::Context,
    ) {
        if let Some(conflict) = error.downcast_ref::<engine::OutputConflict>() {
            let path = conflict.0.clone();
            self.error = None;
            self.output_conflict = Some(OutputDialog {
                path: path.clone(),
                retry,
                candidate: None,
                checking: true,
                confirming: false,
                problem: None,
                copied: false,
            });
            let source = self
                .inventory
                .as_ref()
                .map(|i| i.iso.clone())
                .unwrap_or_default();
            let tx = self.sender.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result =
                    cleanup::output_candidate(&path, &source).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Message::OutputChecked(result));
                ctx.request_repaint();
            });
        } else {
            self.show_build_error(format!("{error:#}"), retry);
        }
    }
    pub(super) fn resume_output_action(&mut self, retry: BuildRetry, ctx: &egui::Context) {
        match retry {
            BuildRetry::Review => self.review_build(ctx),
            BuildRetry::Begin => self.begin(ctx),
        }
    }
    fn choose_output(&mut self, next: Option<PathBuf>, retry: BuildRetry, ctx: &egui::Context) {
        if let Some(next) = next {
            self.output = platform::display_path(&next);
            self.output_conflict = None;
            self.resume_output_action(retry, ctx);
        }
    }
    fn replace_output(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.output_conflict else {
            return;
        };
        if dialog.checking {
            return;
        }
        let Some(id) = dialog.candidate.as_ref().and_then(|c| c.id.clone()) else {
            return;
        };
        let expected = dialog
            .candidate
            .as_ref()
            .and_then(|c| c.fingerprint.clone());
        let path = dialog.path.clone();
        dialog.checking = true;
        dialog.problem = None;
        let req = self.request();
        let drivers_ready = self.drivers_ready();
        let retained_workspace = self.last_build_workspace.clone();
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<_> {
                let req = req?;
                anyhow::ensure!(
                    drivers_ready,
                    "Finish downloading this model's drivers before replacing the ISO"
                );
                if let Some(workspace) = retained_workspace {
                    engine::workspace_available(&workspace)?;
                }
                anyhow::ensure!(
                    req.output == path,
                    "The selected output changed; review it again"
                );
                anyhow::ensure!(
                    expected.as_ref() == Some(&cleanup::fingerprint(&path)?),
                    "The ISO changed after confirmation. It has been preserved."
                );
                engine::preflight_replacement(&req)?;
                ipc::cleanup_elevated(CleanupRequest::Isos {
                    ids: vec![id],
                    protected: vec![req.inventory.iso],
                })
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::OutputDeleted(result));
            ctx.request_repaint();
        });
    }
    pub(super) fn output_popup(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &self.output_conflict else {
            return;
        };
        let colors = appearance::palette(ctx);
        let busy = dialog.checking;
        let confirming = dialog.confirming;
        let path = dialog.path.clone();
        let retry = dialog.retry;
        let eligible = dialog.candidate.as_ref().is_some_and(|c| c.id.is_some());
        let mut action = 0;
        let message = format!(
            "The output already exists: {}{}{}",
            platform::display_path(&path),
            dialog
                .candidate
                .as_ref()
                .and_then(|c| c.reason.as_ref())
                .map(|r| format!("\n{r}"))
                .unwrap_or_default(),
            dialog
                .problem
                .as_ref()
                .map(|r| format!("\n{r}"))
                .unwrap_or_default()
        );
        let modal=egui::Modal::new(egui::Id::new("action-needed")).frame(egui::Frame::new().fill(colors.surface).corner_radius(16).inner_margin(24.0)).show(ctx,|ui|{
            ui.set_width((ctx.content_rect().width()-96.0).clamp(240.0,540.0));
            ui.label(RichText::new(if confirming{"Replace this ISO?"}else{"An ISO already exists"}).size(23.0).strong());
            ui.add_space(12.0);
            egui::ScrollArea::vertical().id_salt("output-conflict-body").max_height((ctx.content_rect().height()-235.0).max(100.0)).show(ui,|ui|{
                egui::Frame::new().fill(colors.source).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).strong());
                    ui.add(egui::Label::new(platform::display_path(&path)).wrap().selectable(true));
                    if let Some(c)=&dialog.candidate{ui.label(format!("ISO size: {}",cleanup::format_bytes(c.size)));}
                });
                ui.add_space(12.0);
                if confirming {ui.label(RichText::new("Deletion is permanent. The old ISO cannot be restored if the new build fails.").color(colors.warning));ui.label("The previous workspace and build logs will be kept. The new build starts from step 1.");}
                if let Some(reason)=dialog.candidate.as_ref().and_then(|c|c.reason.as_ref()){ui.colored_label(colors.warning,reason);}
                if let Some(error)=&dialog.problem{ui.colored_label(colors.warning,error);}
                if busy {ui.horizontal(|ui|{ui.spinner();ui.label(if confirming{"Checking prerequisites and replacing ISO…"}else{"Checking the existing ISO…"});});}
                ui.add_space(12.0);
                ui.add_enabled_ui(!busy,|ui|{
                    if confirming {
                        if Self::primary(ui,"Delete ISO and build new",eligible){action=5;}
                    } else {
                        if ui.add_enabled(eligible,egui::Button::new("Delete ISO and build new")).clicked(){action=1;}
                        if ui.button("Increment filename and build").clicked(){action=2;}
                        if ui.button("Rename and build").clicked(){action=3;}
                    }
                });
            });
            ui.add_space(16.0);
            ui.horizontal(|ui|{
                if ui.add_enabled(!busy,egui::Button::new("Cancel")).clicked(){action=4;}
                if ui.add_enabled(!busy,egui::Button::new(if dialog.copied{"Copied"}else{"Copy error"})).clicked(){action=6;}
            });
        });
        if modal.should_close() && !busy {
            action = 4;
        }
        match action {
            1 => {
                if let Some(d) = &mut self.output_conflict {
                    d.confirming = true;
                }
            }
            2 => match engine::increment_output(&path) {
                Ok(next) => {
                    self.choose_output(Some(next), retry, ctx);
                }
                Err(e) => {
                    if let Some(d) = &mut self.output_conflict {
                        d.problem = Some(format!("{e:#}"));
                    }
                }
            },
            3 => {
                if let Some(next) = rfd::FileDialog::new()
                    .add_filter("ISO", &["iso"])
                    .set_directory(path.parent().unwrap_or(&path))
                    .set_file_name(path.file_name().unwrap_or_default().to_string_lossy())
                    .save_file()
                {
                    self.choose_output(Some(next), retry, ctx);
                }
            }
            4 => {
                if confirming {
                    if let Some(d) = &mut self.output_conflict {
                        d.confirming = false;
                    }
                } else {
                    self.output_conflict = None;
                }
            }
            5 => self.replace_output(ctx),
            6 => {
                ctx.copy_text(message);
                if let Some(d) = &mut self.output_conflict {
                    d.copied = true;
                }
            }
            _ => {}
        }
        if busy {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dialog(path: PathBuf) -> OutputDialog {
        OutputDialog {
            path,
            retry: BuildRetry::Begin,
            candidate: None,
            checking: false,
            confirming: true,
            problem: None,
            copied: false,
        }
    }
    #[test]
    fn rename_cancel_preserves_popup_and_output() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        app.output = "C:/unchanged.iso".into();
        app.output_conflict = Some(dialog(app.output.clone().into()));
        app.choose_output(None, BuildRetry::Begin, &ctx);
        assert_eq!(app.output, "C:/unchanged.iso");
        assert!(app.output_conflict.as_ref().unwrap().confirming);
        assert!(!app.busy);
    }
    #[test]
    fn failed_or_cancelled_deletion_never_resumes_build() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        for result in [
            Err("UAC cancelled".into()),
            Ok(cleanup::CleanupReport {
                errors: vec!["ISO mounted".into()],
                ..Default::default()
            }),
            Ok(Default::default()),
        ] {
            app.output_conflict = Some(dialog("C:/existing.iso".into()));
            app.sender.send(Message::OutputDeleted(result)).unwrap();
            app.consume();
            assert!(app.output_resume.is_none());
            assert!(app.output_conflict.as_ref().unwrap().problem.is_some());
            assert!(!app.busy);
        }
    }
    #[test]
    fn successful_deletion_reenters_normal_preflight_and_explains_followup_failure() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        app.output_conflict = Some(dialog("C:/previous.iso".into()));
        app.sender
            .send(Message::OutputDeleted(Ok(cleanup::CleanupReport {
                deleted: vec!["C:/previous.iso".into()],
                ..Default::default()
            })))
            .unwrap();
        app.consume();
        assert!(app.output_conflict.is_none());
        let retry = app.output_resume.take().unwrap();
        assert!(matches!(retry, BuildRetry::Begin));
        app.resume_output_action(retry, &ctx);
        assert!(!app.busy);
        let error = &app.error.as_ref().unwrap().message;
        assert!(error.contains("old ISO was deleted"));
        assert!(error.contains("could not start"));
    }
    #[test]
    fn typed_conflict_uses_output_dialog_but_generic_errors_do_not() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        app.handle_preflight_error(
            anyhow::anyhow!("The output already exists. Choose a new filename"),
            BuildRetry::Review,
            &ctx,
        );
        assert!(app.output_conflict.is_none() && app.error.is_some());
        app.handle_preflight_error(
            engine::OutputConflict("C:/missing-preview.iso".into()).into(),
            BuildRetry::Review,
            &ctx,
        );
        assert!(app.output_conflict.is_some() && app.error.is_none());
    }
}
