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
    pub progress: Option<DownloadProgress>,
    pub cancel: Cancellation,
    pub id: u64,
}

impl DriverDownloads {
    pub fn reset_after_cleanup(&mut self) {
        self.requested_model = None;
        self.key = None;
        self.ready = false;
        self.error = None;
        self.progress = None;
        self.id += 1;
    }
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
    fn current_driver_transfer(&self) -> Option<&DownloadProgress> {
        let model = self.model()?;
        let (id, cache) = self.drivers.key.as_ref()?;
        if *id != model.id
            || !self.drivers.requested_for(model.id)
            || cache.as_path() != std::path::Path::new(self.settings.cache.trim())
        {
            return None;
        }
        self.drivers.progress.as_ref()
    }
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
            return Some(("Driver download failed".into(), colors.error));
        }
        let status = if let Some(progress) = &self.drivers.progress {
            let DownloadProgress { name, received, total, .. } = progress;
            let package = if name.ends_with(".pkg") {
                "Boot Camp"
            } else {
                "Intel graphics"
            };
            format!(
                "{package} download · {} · {}",
                total.filter(|n| *n > 0).map_or_else(|| format!("{:.1} MB", *received as f64 / 1_000_000.0), |n| format!("{:.0}%", *received as f64 / n as f64 * 100.0)),
                progress.rate_label()
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
        if self.closing
            || self.storage.recovery_failed
            || self.busy
            || self.storage.busy()
            || std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_some()
        {
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
    pub(super) fn build_iso_button(&self, ui: &mut egui::Ui, enabled: bool) -> bool {
        let enabled = enabled && !self.storage.busy() && !self.storage.recovery_failed;
        let colors = appearance::palette(ui.ctx());
        let label = if self.busy {
            if self.cancel.is_cancelled() {
                "Cancelling…"
            } else if self.download.is_some() || self.stage == "Downloading drivers" {
                "Downloading"
            } else if self.stage == "Administrator approval" {
                "Waiting for Windows"
            } else if matches!(
                self.stage.as_str(),
                "Preparing your build"
                    | "Checking cached drivers"
                    | "Verifying drivers"
                    | "Extracting drivers"
            ) {
                "Getting ready"
            } else {
                "Building ISO"
            }
        } else if self.page == Page::Build {
            if self.finished.is_some() {
                "ISO ready"
            } else {
                "Build stopped"
            }
        } else if !self.drivers_ready() {
            if self.drivers.error.is_some() {
                "Download failed"
            } else if self.current_driver_transfer().is_some()
                || self.drivers.stage == "Downloading drivers"
            {
                "Downloading"
            } else if self.drivers.running {
                "Getting ready"
            } else {
                "Waiting for drivers"
            }
        } else if !enabled {
            "Waiting for setup"
        } else {
            "Build ISO"
        };
        let response = ui.add_enabled(
            enabled,
            egui::Button::new(RichText::new(label).color(Color32::WHITE))
                .fill(colors.primary)
                .min_size(egui::vec2(155.0, 42.0)),
        );
        let active = self.busy
            || (self.drivers.running
                && self
                    .model()
                    .is_some_and(|model| self.drivers.requested_for(model.id)));
        let progress = if self.busy || self.page == Page::Build {
            self.build_progress.fraction()
        } else {
            self.current_driver_transfer()
                .and_then(|p| download_fraction(p.received, p.total))
        };
        if active || progress.is_some() {
            let track = egui::Rect::from_min_max(
                response.rect.left_bottom() + egui::vec2(5.0, -6.0),
                response.rect.right_bottom() - egui::vec2(5.0, 3.0),
            );
            ui.painter()
                .rect_filled(track, 2, colors.muted.gamma_multiply(0.35));
            let fill = if let Some(fraction) = progress {
                egui::Rect::from_min_size(
                    track.min,
                    egui::vec2(track.width() * fraction, track.height()),
                )
            } else {
                let phase = (ui.input(|i| i.time) * 0.65).fract() as f32;
                let start = track.left() + (phase * 1.3 - 0.3) * track.width();
                ui.ctx().request_repaint_after(Duration::from_millis(32));
                egui::Rect::from_min_size(
                    egui::pos2(start, track.top()),
                    egui::vec2(track.width() * 0.3, track.height()),
                )
                .intersect(track)
            };
            ui.painter().rect_filled(
                fill,
                2,
                if label == "Building ISO" || (self.page == Page::Build && self.finished.is_some())
                {
                    colors.success
                } else {
                    colors.accent
                },
            );
        }
        response
            .on_disabled_hover_text(if self.busy {
                "The current build stage is shown in the footer."
            } else if self.page == Page::Build && self.finished.is_some() {
                "Your ISO has been created. Open the output folder from the build page."
            } else if self.drivers.error.is_some() {
                "Use Retry in the center of the footer to finish downloading drivers."
            } else if self.drivers_ready() {
                "Complete the output and build setup before starting."
            } else {
                "Finish the selected model's downloads before building."
            })
            .clicked()
    }

    pub(super) fn driver_download_footer(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        model: &str,
        tooltip: &str,
    ) {
        let colors = appearance::palette(ui.ctx());
        if matches!(self.page, Page::Build | Page::Complete) {
            let area = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 38.0));
            ui.put(
                egui::Rect::from_min_size(area.min, egui::vec2(area.width(), 14.0)),
                egui::Label::new(RichText::new(model).size(12.0).color(colors.muted)).truncate(),
            )
            .on_hover_text(tooltip);
            ui.put(
                egui::Rect::from_min_size(
                    area.min + egui::vec2(0.0, 15.0),
                    egui::vec2(area.width(), 17.0),
                ),
                egui::Label::new(
                    RichText::new(format!("{}{} · {}", self.build_progress.label(), self.download.as_ref().map(|p| format!(" · {}", p.rate_label())).unwrap_or_default(), self.stage))
                        .size(11.0)
                        .color(colors.muted),
                )
                .truncate(),
            )
            .on_hover_text(format!("{}\n{}", self.build_progress.label(), self.detail));
            let track =
                egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - 4.0), area.max);
            ui.painter()
                .rect_filled(track, 2, colors.muted.gamma_multiply(0.2));
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    track.min,
                    egui::vec2(
                        track.width() * self.build_progress.fraction().unwrap_or(0.0),
                        track.height(),
                    ),
                ),
                2,
                if self.finished.is_some() {
                    colors.success
                } else {
                    colors.accent
                },
            );
            return;
        }
        let status = if self.busy {
            Some((self.stage.clone(), colors.muted))
        } else {
            self.driver_footer_status(ui.ctx())
        };
        let Some((status, color)) = status else {
            if self.model().is_some() && !self.drivers_ready() && !self.busy && !self.storage.busy()
            {
                if ui
                    .put(
                        rect,
                        egui::Link::new(RichText::new("Download drivers").size(13.0)),
                    )
                    .on_hover_text(model)
                    .clicked()
                {
                    self.drivers.requested_model = self.model().map(|m| m.id);
                    self.drivers.key = None;
                    self.ensure_driver_downloads(ui.ctx());
                }
                return;
            }
            ui.put(
                rect,
                egui::Label::new(RichText::new(model).size(13.0).color(colors.muted)).truncate(),
            )
            .on_hover_text(tooltip);
            return;
        };
        let area = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 38.0));
        let model_rect = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), 14.0));
        ui.put(
            model_rect,
            egui::Label::new(RichText::new(model).size(12.0).color(colors.muted)).truncate(),
        )
        .on_hover_text(tooltip);
        let status_rect = egui::Rect::from_min_size(
            area.min + egui::vec2(0.0, 15.0),
            egui::vec2(area.width(), 17.0),
        );
        if !self.busy
            && let Some(error) = &self.drivers.error
        {
            let status_rect = egui::Rect::from_center_size(
                status_rect.center(),
                egui::vec2(status_rect.width().min(290.0), status_rect.height()),
            );
            let retry = egui::Rect::from_min_max(
                egui::pos2(status_rect.right() - 46.0, status_rect.top()),
                status_rect.max,
            );
            let text = egui::Rect::from_min_max(
                status_rect.min,
                egui::pos2(retry.left() - 52.0, status_rect.bottom()),
            );
            let copy = egui::Rect::from_min_max(
                egui::pos2(retry.left() - 46.0, status_rect.top()),
                egui::pos2(retry.left() - 6.0, status_rect.bottom()),
            );
            if ui
                .put(copy, egui::Link::new(RichText::new("Copy").size(11.0)))
                .on_hover_text("Copy full error")
                .clicked()
            {
                ui.ctx().copy_text(error.clone());
            }
            ui.put(
                text,
                egui::Label::new(
                    RichText::new("Download failed")
                        .size(11.0)
                        .color(colors.error),
                )
                .truncate(),
            )
            .on_hover_text(error);
            if ui
                .put(retry, egui::Link::new(RichText::new("Retry").size(11.0)))
                .on_hover_text(error)
                .clicked()
            {
                self.drivers.key = None;
                ui.ctx().request_repaint();
            }
            return;
        }
        let progress = if self.busy {
            self.download.as_ref()
        } else {
            self.current_driver_transfer()
        };
        let mut fraction = None;
        let (text, detail) = if let Some(progress) = progress {
            let DownloadProgress { name, received, total, .. } = progress;
            let package = if name.ends_with(".pkg") {
                "Boot Camp"
            } else {
                "Intel graphics"
            };
            fraction = download_fraction(*received, *total);
            let detail = if let Some(total) = total.filter(|total| *total > 0) {
                format!(
                    "Downloading {package} · {:.1} / {:.1} MB · {:.0}%",
                    *received as f64 / 1_000_000.0,
                    total as f64 / 1_000_000.0,
                    fraction.unwrap_or(0.0) * 100.0
                )
            } else {
                format!(
                    "Downloading {package} · {:.1} MB received",
                    *received as f64 / 1_000_000.0
                )
            };
            let detail = format!("{detail} · {}", progress.rate_label());
            let text = if area.width() < 400.0 {
                fraction.map_or_else(
                    || format!("Downloading · {}", progress.rate_label()),
                    |value| format!("{:.0}% · {}", value * 100.0, progress.rate_label()),
                )
            } else {
                detail.clone()
            };
            (text, format!("{name}\n{detail}"))
        } else {
            (
                status.clone(),
                if self.busy {
                    self.detail.clone()
                } else {
                    status
                },
            )
        };
        ui.put(
            status_rect,
            egui::Label::new(RichText::new(text).size(11.0).color(color)).truncate(),
        )
        .on_hover_text(detail);
        if self.busy || self.drivers.running || progress.is_some() {
            let track =
                egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - 4.0), area.max);
            ui.painter()
                .rect_filled(track, 2, colors.muted.gamma_multiply(0.2));
            let fill = if let Some(fraction) = fraction {
                egui::Rect::from_min_size(
                    track.min,
                    egui::vec2(track.width() * fraction, track.height()),
                )
            } else {
                let phase = (ui.input(|i| i.time) * 0.65).fract() as f32;
                let start = track.left() + (phase * 1.3 - 0.3) * track.width();
                ui.ctx().request_repaint_after(Duration::from_millis(32));
                egui::Rect::from_min_size(
                    egui::pos2(start, track.top()),
                    egui::vec2(track.width() * 0.3, track.height()),
                )
                .intersect(track)
            };
            ui.painter().rect_filled(fill, 2, colors.accent);
        }
    }
}

fn download_fraction(received: u64, total: Option<u64>) -> Option<f32> {
    total
        .filter(|total| *total > 0)
        .map(|total| (received as f64 / total as f64).clamp(0.0, 1.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_progress_requires_a_total_and_clamps_completed_transfers() {
        assert!(download_fraction(5, None).is_none());
        assert!(download_fraction(5, Some(0)).is_none());
        assert!((download_fraction(5, Some(10)).unwrap() - 0.5).abs() < f32::EPSILON);
        assert!((download_fraction(15, Some(10)).unwrap() - 1.0).abs() < f32::EPSILON);
    }
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
    #[test]
    fn cache_cleanup_revokes_readiness_and_requires_explicit_download_authorization() {
        let mut state = DriverDownloads {
            requested_model: Some(MODEL_ID),
            key: Some((MODEL_ID, "cache".into())),
            ready: true,
            id: 7,
            ..Default::default()
        };
        state.reset_after_cleanup();
        assert!(!state.ready);
        assert!(!state.requested_for(MODEL_ID));
        assert!(state.key.is_none());
        state.finish(7, Ok(()));
        assert!(!state.ready);
        state.requested_model = Some(MODEL_ID);
        assert!(state.requested_for(MODEL_ID));
    }
}
