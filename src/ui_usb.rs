use super::*;
use macbook_iso_patcher::{
    cleanup,
    usb::{self, Choice, Conflict, Outcome, Progress, Published, Request},
};

#[derive(Default)]
pub(super) struct UsbCopy {
    pub open: bool,
    pub busy: bool,
    pub epoch: u64,
    pub cancel: Cancellation,
    source: Option<PathBuf>,
    size: Option<u64>,
    root: Option<PathBuf>,
    pub progress: Option<Progress>,
    conflict: Option<Conflict>,
    confirming: bool,
    published: Option<Published>,
    ready: bool,
    error: Option<String>,
    copied: bool,
}
impl UsbCopy {
    pub fn reset(&mut self) {
        self.cancel.cancel();
        // A live copy must finish before changing workflows.
        if !self.busy {
            let epoch = self.epoch.wrapping_add(1);
            *self = Self {
                epoch,
                ..Default::default()
            };
        }
    }
    pub fn finish(&mut self, id: u64, result: Result<Outcome, String>) {
        if id != self.epoch {
            return;
        }
        self.busy = false;
        self.progress = None;
        match result {
            Ok(Outcome::Conflict(conflict)) => {
                self.conflict = Some(conflict);
                self.confirming = false;
            }
            Ok(Outcome::Ready(published)) => {
                self.published = Some(published);
                self.ready = true;
                self.error = None;
            }
            Ok(Outcome::LoggingFailed { published, error }) => {
                self.published = Some(published);
                self.error = Some(error);
            }
            Err(error) => self.error = Some(error),
        }
    }
    fn select(&mut self, selected: Option<PathBuf>) {
        if self.busy {
            return;
        }
        if let Some(path) = selected {
            self.root = Some(path);
            self.published = None;
            self.ready = false;
            self.error = None;
            self.conflict = None;
            self.confirming = false;
            self.copied = false;
        }
    }
}
impl PatcherApp {
    #[cfg(debug_assertions)]
    pub(super) fn preview_copy_usb(&mut self, view: &str) {
        self.page = Page::Complete;
        self.model_selected = Some(0);
        self.finished = Some(PathBuf::from(r"C:\Builds\Win10_a1706_Intel_Patched.iso"));
        self.open_copy_usb();
        self.usb_copy.size = Some(5_200_000_000);
        if view != "copy-usb-empty" {
            self.usb_copy.root = Some(PathBuf::from(r"E:\"));
        }
        let published = Published {
            path: PathBuf::from(r"E:\Win10_a1706_Intel_Patched.iso"),
            display_path: PathBuf::from(r"E:\Win10_a1706_Intel_Patched.iso"),
            fingerprint: cleanup::Fingerprint {
                size: 5_200_000_000,
                modified: 1,
                identity: "preview".into(),
            },
            volume: "preview".into(),
        };
        if view.ends_with("copying") || view.ends_with("verifying") {
            self.usb_copy.busy = true;
            self.usb_copy.progress = Some(Progress {
                phase: if view.ends_with("verifying") {
                    usb::Phase::Verifying
                } else {
                    usb::Phase::Copying
                },
                bytes: 2_800_000_000,
                total: 5_200_000_000,
            });
        } else if view.ends_with("ready") {
            self.usb_copy.published = Some(published);
            self.usb_copy.ready = true;
        } else if view.ends_with("conflict") || view.ends_with("confirm") {
            self.usb_copy.conflict = Some(Conflict {
                path: published.display_path,
                fingerprint: published.fingerprint,
            });
            self.usb_copy.confirming = view.ends_with("confirm");
        } else if view.ends_with("failed") {
            self.usb_copy.published = Some(published);
            self.usb_copy.error=Some("The ISO was copied, but logging could not be enabled. Check USB write protection, then retry. Retry will only add the logging marker.".into());
        }
    }
    pub(super) fn open_copy_usb(&mut self) {
        if !self.usb_copy.busy && self.usb_copy.source != self.finished {
            self.usb_copy.reset();
            self.usb_copy.source = self.finished.clone();
        }
        self.usb_copy.size = self
            .usb_copy
            .source
            .as_ref()
            .and_then(|path| std::fs::metadata(path).ok())
            .map(|m| m.len());
        self.usb_copy.open = true;
    }
    fn usb_copy_allowed(&self) -> bool {
        !self.usb_copy.busy
            && !self.preparing_usb
            && !self.storage.deleting
            && !self.storage.moving
            && !self.closing
            && !self.busy
            && !self.dismounting
    }
    fn start_usb_copy(&mut self, choice: Option<Choice>) {
        if !self.usb_copy_allowed() {
            return;
        }
        let (Some(source), Some(root)) = (self.usb_copy.source.clone(), self.usb_copy.root.clone())
        else {
            return;
        };
        self.usb_copy.epoch = self.usb_copy.epoch.wrapping_add(1);
        let id = self.usb_copy.epoch;
        self.usb_copy.cancel = Cancellation::default();
        let cancel = self.usb_copy.cancel.clone();
        self.usb_copy.busy = true;
        self.usb_copy.error = None;
        self.usb_copy.copied = false;
        self.usb_copy.conflict = None;
        self.usb_copy.confirming = false;
        self.usb_copy.ready = false;
        self.usb_copy.progress = Some(Progress {
            phase: usb::Phase::Checking,
            bytes: 0,
            total: 0,
        });
        let request = Request {
            source,
            root,
            choice,
            published: self.usb_copy.published.clone(),
        };
        let tx = self.sender.clone();
        std::thread::spawn(move || {
            let result = usb::prepare(&request, &cancel, |p| {
                let _ = tx.send(Message::UsbCopyProgress(id, p));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::UsbCopyDone(id, result));
        });
    }
    pub(super) fn copy_usb_popup(&mut self, ctx: &egui::Context) {
        if !self.usb_copy.open {
            return;
        }
        let colors = appearance::palette(ctx);
        let allowed = self.usb_copy_allowed();
        let mut choose = false;
        let mut start = false;
        let mut close = false;
        let mut replace = false;
        let mut keep = false;
        let mut open_folder = false;
        let modal=egui::Modal::new(egui::Id::new("prepare-usb-copy"))
            .frame(egui::Frame::new().fill(colors.surface).corner_radius(16).inner_margin(22.0))
            .show(ctx,|ui| {
                let size=egui::vec2((ctx.content_rect().width()-88.0).clamp(240.0,630.0),(ctx.content_rect().height()-100.0).clamp(220.0,470.0));
                let (shell,_)=ui.allocate_exact_size(size,egui::Sense::hover());
                let footer=egui::Rect::from_min_max(egui::pos2(shell.left(),shell.bottom()-38.0),shell.max);
                let body=egui::Rect::from_min_max(shell.min,egui::pos2(shell.right(),footer.top()-14.0));
                let mut content=ui.new_child(egui::UiBuilder::new().max_rect(body));content.set_clip_rect(body.expand(3.0).intersect(ui.clip_rect()));
                egui::ScrollArea::vertical().id_salt("copy-usb-body").auto_shrink([false,false]).show(&mut content,|ui|{
                    ui.label(RichText::new("Prepare USB").size(25.0).strong());
                    ui.label("Copies your ISO and enables USB logging.");
                    ui.label("Set up a spare USB with Ventoy, then choose its main storage partition.");
                    ui.add_space(12.0);
                    if let Some(source)=&self.usb_copy.source {
                        ui.label(RichText::new(source.file_name().unwrap_or_default().to_string_lossy()).strong());
                        ui.label(self.usb_copy.size.map(cleanup::format_bytes).unwrap_or_else(||"ISO unavailable. Restore the completed ISO to its original location.".into()));
                    } else {ui.colored_label(colors.warning,"No completed ISO is available. Finish a build first.");}
                    ui.add_space(12.0);
                    if let Some(root)=&self.usb_copy.root {
                        egui::Frame::new().fill(colors.source).corner_radius(10).inner_margin(12.0).show(ui,|ui|{
                            ui.set_min_width(ui.available_width());ui.label("Ventoy USB");
                            ui.add(egui::Label::new(platform::display_path(root)).wrap().selectable(true));
                        });
                    }
                    choose=ui.add_enabled(allowed,egui::Button::new("Choose USB folder")).clicked();
                    if let Some(p)=&self.usb_copy.progress {
                        ui.add_space(10.0);ui.horizontal(|ui|{ui.spinner();ui.label(p.phase.label());});
                        if p.total>0 {
                            ui.add(egui::ProgressBar::new((p.bytes as f64/p.total as f64).min(1.0) as f32).desired_width(ui.available_width()));
                            ui.label(format!("{} / {} · {}%",cleanup::format_bytes(p.bytes),cleanup::format_bytes(p.total),(p.bytes as f64/p.total as f64*100.0).min(100.0) as u32));
                        }
                    }
                    if let Some(conflict)=&self.usb_copy.conflict {
                        ui.add_space(10.0);
                        ui.colored_label(colors.warning,if self.usb_copy.confirming {"Replace this ISO? The previous USB copy will be permanently replaced after verification."}else{"An ISO with this filename already exists."});
                        ui.add(egui::Label::new(platform::display_path(&conflict.path)).wrap().selectable(true));
                        ui.label(cleanup::format_bytes(conflict.fingerprint.size));
                    }
                    if let Some(error)=&self.usb_copy.error {
                        ui.add_space(10.0);ui.add(egui::Label::new(RichText::new(error).color(colors.warning)).wrap().selectable(true));
                        if ui.button(if self.usb_copy.copied {"Copied"}else{"Copy error"}).clicked(){ctx.copy_text(error.clone());self.usb_copy.copied=true;}
                    }
                    if self.usb_copy.ready {
                        ui.add_space(10.0);ui.colored_label(colors.success,"USB ready · ISO copied and setup logging enabled");
                        if let Some(published)=&self.usb_copy.published {ui.add(egui::Label::new(platform::display_path(&published.display_path)).wrap().selectable(true));}
                        open_folder=ui.button("Open USB folder").clicked();
                    }
                });
                let mut actions=ui.new_child(egui::UiBuilder::new().max_rect(footer));
                actions.horizontal(|ui|{
                    if self.usb_copy.busy {
                        if ui.add_enabled(!self.usb_copy.cancel.is_cancelled(),egui::Button::new("Cancel preparation")).clicked(){self.usb_copy.cancel.cancel();}
                    } else if self.usb_copy.conflict.is_some() {
                        if self.usb_copy.confirming {replace=ui.add_enabled(allowed,egui::Button::new("Confirm replace")).clicked();}
                        else if ui.add_enabled(allowed,egui::Button::new("Replace")).clicked(){self.usb_copy.confirming=true;}
                        keep=ui.add_enabled(allowed,egui::Button::new("Keep both")).clicked();
                        if ui.button("Cancel").clicked(){self.usb_copy.conflict=None;self.usb_copy.confirming=false;}
                    } else if !self.usb_copy.ready {
                        start=ui.add_enabled(allowed && self.usb_copy.root.is_some() && self.usb_copy.source.is_some(),egui::Button::new(if self.usb_copy.error.is_some(){"Retry"}else{"Prepare USB"})).clicked();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{close=ui.button("Close").clicked();});
                });
            });
        if close || modal.should_close() {
            self.usb_copy.open = false;
        }
        if choose {
            let selected = rfd::FileDialog::new()
                .set_title("Choose the main Ventoy USB root")
                .pick_folder();
            self.usb_copy.select(selected);
        }
        if start {
            self.start_usb_copy(None);
        }
        if keep {
            self.start_usb_copy(Some(Choice::KeepBoth));
        }
        if replace && let Some(conflict) = self.usb_copy.conflict.clone() {
            self.start_usb_copy(Some(Choice::Replace(conflict)));
        }
        if open_folder
            && let Some(root) = &self.usb_copy.root
            && let Some(published) = &self.usb_copy.published
            && let Err(e) = usb::open_usb_folder(root, published)
        {
            self.usb_copy.error = Some(format!("Could not open USB folder: {e:#}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn published() -> Published {
        Published {
            path: "E:/Patched.iso".into(),
            display_path: "E:/Patched.iso".into(),
            fingerprint: cleanup::Fingerprint {
                size: 123,
                modified: 1,
                identity: "test".into(),
            },
            volume: "test-usb".into(),
        }
    }
    #[test]
    fn manual_routes_do_not_start_or_reset_an_active_copy() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        app.finished =
            Some(std::env::temp_dir().join(format!("missing-{}.iso", uuid::Uuid::new_v4())));
        app.open_copy_usb();
        app.usb_copy.busy = true;
        let epoch = app.usb_copy.epoch;
        app.usb_copy.open = false;
        app.open_finished_iso();
        assert!(app.error.is_some()); // Missing ISO remains actionable.
        assert!(app.usb_copy.busy && app.usb_copy.epoch == epoch && !app.usb_copy.open);
        app.open_logging_usb_popup();
        assert!(app.usb_popup_open && !app.preparing_usb);
        assert!(app.usb_copy.busy && app.usb_copy.epoch == epoch);
        app.open_copy_usb(); // Both automatic entry points reuse this operation.
        assert!(app.usb_copy.open && app.usb_copy.busy && app.usb_copy.epoch == epoch);
    }
    #[test]
    fn selecting_usb_never_starts_and_cancelled_picker_preserves_state() {
        let mut state = UsbCopy::default();
        state.select(Some("E:/".into()));
        assert!(!state.busy);
        state.select(None);
        assert_eq!(state.root, Some("E:/".into()));
        state.ready = true;
        state.select(Some("F:/".into()));
        assert!(!state.ready && state.published.is_none());
    }
    #[test]
    fn closing_and_reopening_preserves_live_task_and_results_without_touching_logging_tool() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        app.finished = Some("C:/Patched.iso".into());
        app.open_copy_usb();
        assert_eq!(app.usb_copy.source, app.finished);
        assert!(!app.usb_popup_open);
        app.usb_copy.busy = true;
        app.usb_copy.epoch = 10;
        app.usb_copy.root = Some("E:/".into());
        app.usb_copy.open = false;
        app.open_copy_usb();
        assert!(app.usb_copy.open && app.usb_copy.busy);
        assert_eq!(app.usb_copy.epoch, 10);
        assert!(!app.storage_move_gate().idle);
        app.usb_copy.finish(9, Err("late result".into()));
        assert!(app.usb_copy.busy);
        app.usb_copy.finish(
            10,
            Ok(Outcome::LoggingFailed {
                published: published(),
                error: "marker failed".into(),
            }),
        );
        assert!(app.usb_copy.published.is_some() && !app.usb_copy.ready && !app.usb_copy.busy);
        app.usb_copy.open = false;
        app.open_copy_usb();
        assert!(app.usb_copy.error.is_some());
        app.usb_copy.finish(10, Ok(Outcome::Ready(published())));
        assert!(app.usb_copy.ready && app.usb_copy.error.is_none());
        app.open_logging_usb_popup();
        assert!(app.usb_popup_open);
        assert!(app.usb_logging.is_none());
    }
    #[test]
    fn copying_excludes_duplicate_and_storage_operations_and_reset_cancels_safely() {
        let mut app = PatcherApp::new_with_context(&egui::Context::default());
        app.storage.deleting = true;
        assert!(!app.usb_copy_allowed());
        app.storage.deleting = false;
        app.preparing_usb = true;
        assert!(!app.usb_copy_allowed());
        app.preparing_usb = false;
        app.usb_copy.busy = true;
        assert!(!app.usb_copy_allowed());
        assert!(app.storage_blocked());
        app.usb_copy.reset();
        assert!(app.usb_copy.cancel.is_cancelled() && app.usb_copy.busy);
        app.usb_copy
            .finish(0, Err("USB preparation cancelled".into()));
        app.usb_copy.reset();
        assert!(!app.usb_copy.busy && app.usb_copy.source.is_none());
    }
}
