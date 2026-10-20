use super::*;
use macbook_iso_patcher::driver_prefetch;

#[derive(Default)]
pub(super) struct DriverDownloads {
    pub requested_model: Option<&'static str>,
    pub key: Option<(&'static str, PathBuf)>,
    pub running: bool,
    pub ready: bool,
    pub error: Option<String>,
    pub stage: String,
    pub progress: Option<(String, u64, Option<u64>)>,
    pub cancel: Cancellation,
    pub id: u64,
}

impl DriverDownloads {
    pub fn selection_changed(&mut self) {
        self.requested_model = None;
        if self.running {
            self.cancel.cancel();
        }
    }

    pub fn requested_for(&self, model: &str) -> bool {
        self.requested_model == Some(model)
    }

    pub fn finish(&mut self, id: u64, result: Result<(), String>) {
        if id != self.id {
            return;
        }
        self.running = false;
        self.ready = result.is_ok();
        self.error = if self.cancel.is_cancelled() {
            None
        } else {
            result.err()
        };
        if self.cancel.is_cancelled() {
            self.key = None;
            self.ready = false;
        }
        self.progress = None;
    }
}

impl PatcherApp {
    pub(super) fn driver_footer_status(&self, ctx: &egui::Context) -> Option<(String, Color32)> {
        let model = self.model()?;
        if !self.drivers.requested_for(model.id) {
            return None;
        }
        let (id, _) = self.drivers.key.as_ref()?;
        let colors = appearance::palette(ctx);
        if *id != model.id {
            return Some(("Switching driver downloads…".into(), colors.muted));
        }
        if self.drivers_ready() {
            return Some(("Files & drivers downloaded".into(), colors.success));
        }
        if self.drivers.error.is_some() {
            return Some(("Driver download failed · Retry above".into(), colors.error));
        }
        let status = if let Some((name, received, Some(total))) = &self.drivers.progress {
            let package = if name.ends_with(".pkg") {
                "Boot Camp"
            } else {
                "Intel graphics"
            };
            format!(
                "{package} download · {:.0}%",
                *received as f64 / (*total).max(1) as f64 * 100.0
            )
        } else {
            self.drivers.stage.clone()
        };
        Some((status, colors.muted))
    }
    pub(super) fn drivers_ready(&self) -> bool {
        self.model().is_some_and(|model| {
            self.drivers.ready
                && !self.drivers.running
                && self.drivers.key.as_ref().is_some_and(|(id, cache)| {
                    *id == model.id
                        && cache.as_path() == std::path::Path::new(self.settings.cache.trim())
                })
        })
    }
    pub(super) fn ensure_driver_downloads(&mut self, ctx: &egui::Context) {
        // Renderer fixtures never trigger real network transfers.
        if self.closing || self.busy || std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_some() {
            return;
        }
        let Some(model) = self.model() else {
            return;
        };
        // Selecting a row never authorizes a download. Only the start button does.
        if !self.drivers.requested_for(model.id) {
            return;
        }
        let desired = (model.id, PathBuf::from(self.settings.cache.trim()));
        if self.drivers.key.as_ref() == Some(&desired) {
            return;
        }
        if self.drivers.running {
            self.drivers.cancel.cancel();
            return;
        }
        self.drivers.id += 1;
        let id = self.drivers.id;
        self.drivers.key = Some(desired.clone());
        self.drivers.running = true;
        self.drivers.ready = false;
        self.drivers.error = None;
        self.drivers.progress = None;
        self.drivers.stage = "Checking driver downloads…".into();
        self.drivers.cancel = Cancellation::default();
        let cancel = self.drivers.cancel.clone();
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let events = tx.clone();
            let repaint = ctx.clone();
            let emit: Emit = Arc::new(move |event| {
                let _ = events.send(Message::DriverEvent(id, event));
                repaint.request_repaint();
            });
            let result = driver_prefetch::download_for_model(desired.0, &desired.1, &emit, &cancel)
                .map(|_| ())
                .map_err(|error| format!("{error:#}"));
            let _ = tx.send(Message::DriversDone(id, result));
            ctx.request_repaint();
        });
    }
    pub(super) fn driver_download_panel(&mut self, ui: &mut egui::Ui) {
        let Some(model) = self.model() else {
            return;
        };
        if !self.drivers.requested_for(model.id) {
            return;
        }
        let Some((downloading_model, _)) = &self.drivers.key else {
            return;
        };
        let switching = *downloading_model != model.id;
        let ready = self.drivers_ready();
        let colors = appearance::palette(ui.ctx());
        egui::Frame::new()
            .fill(if ready {
                colors.ready
            } else if self.drivers.error.is_some() && !switching {
                colors.missing
            } else {
                colors.source
            })
            .corner_radius(10)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if self.drivers.running {
                        ui.spinner();
                    }
                    ui.label(
                        RichText::new(if switching {
                            "Switching model downloads…"
                        } else if ready {
                            "Files & drivers downloaded"
                        } else if self.drivers.error.is_some() {
                            "Driver download needs attention"
                        } else {
                            "Downloading files & drivers"
                        })
                        .strong(),
                    );
                });
                if switching {
                    ui.label("Stopping the previous download before starting this model’s files.");
                } else if let Some(error) = &self.drivers.error {
                    ui.label(error);
                    if ui.button("Retry driver download").clicked() {
                        self.drivers.key = None;
                        ui.ctx().request_repaint();
                    }
                } else if ready {
                    ui.label(
                        RichText::new("Verified files are cached and will be reused.")
                            .size(13.0)
                            .color(colors.success),
                    );
                } else if let Some((name, received, total)) = &self.drivers.progress {
                    let package = if name.ends_with(".pkg") {
                        "Apple Boot Camp drivers"
                    } else {
                        "Intel graphics"
                    };
                    if let Some(total) = total.filter(|total| *total > 0) {
                        ui.label(
                            RichText::new(format!(
                                "{package} · {:.1} / {:.1} MB",
                                *received as f64 / 1_000_000.0,
                                total as f64 / 1_000_000.0
                            ))
                            .size(13.0),
                        );
                        ui.add(
                            egui::ProgressBar::new(
                                (*received as f64 / total as f64).clamp(0.0, 1.0) as f32,
                            )
                            .desired_height(8.0),
                        );
                    } else {
                        ui.label(format!(
                            "{package} · {:.1} MB received",
                            *received as f64 / 1_000_000.0
                        ));
                    }
                } else {
                    ui.label(
                        RichText::new(&self.drivers.stage)
                            .size(13.0)
                            .color(colors.muted),
                    );
                }
            });
        ui.add_space(14.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_requires_explicit_start_for_each_model() {
        let mut state = DriverDownloads::default();
        assert!(!state.requested_for(MODEL_ID));
        state.requested_model = Some(MODEL_ID);
        assert!(state.requested_for(MODEL_ID));
        assert!(!state.requested_for("a1708-2016"));
        state.selection_changed();
        assert!(!state.requested_for(MODEL_ID));
        assert!(!state.requested_for("a1708-2016"));
    }

    #[test]
    fn changing_selection_cancels_without_authorizing_a_replacement() {
        let mut state = DriverDownloads {
            requested_model: Some(MODEL_ID),
            running: true,
            id: 1,
            ..Default::default()
        };
        state.selection_changed();
        assert!(state.cancel.is_cancelled());
        state.requested_model = Some("a1708-2016");
        state.finish(1, Err("cancelled".into()));
        assert!(state.requested_for("a1708-2016"));
        assert!(!state.running);
        assert!(state.key.is_none());
    }
    #[test]
    fn late_completion_cannot_finish_the_new_models_job() {
        let mut state = DriverDownloads {
            id: 2,
            running: true,
            key: Some(("a1708-2016", "cache".into())),
            ..Default::default()
        };
        state.finish(1, Ok(()));
        assert!(state.running);
        assert!(!state.ready);
        assert_eq!(state.key.as_ref().unwrap().0, "a1708-2016");
    }
    #[test]
    fn cancelled_switch_can_restart_even_if_the_user_switches_back() {
        let mut state = DriverDownloads {
            id: 2,
            running: true,
            key: Some((MODEL_ID, "cache".into())),
            ..Default::default()
        };
        state.cancel.cancel();
        state.finish(2, Err("cancelled".into()));
        assert!(!state.running && !state.ready);
        assert!(state.key.is_none() && state.error.is_none());
    }
    #[test]
    fn failures_wait_for_retry_instead_of_restarting_forever() {
        let mut state = DriverDownloads {
            id: 2,
            running: true,
            key: Some((MODEL_ID, "cache".into())),
            ..Default::default()
        };
        state.finish(2, Err("Network unavailable".into()));
        assert!(!state.running && !state.ready);
        assert!(state.key.is_some());
        assert_eq!(state.error.as_deref(), Some("Network unavailable"));
    }
}
