use super::*;
use macbook_iso_patcher::cleanup::{self, CleanupReport, CleanupRequest};

// One independent hit target: opening a location never toggles the build row.
fn storage_path_link(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    enabled: bool,
    hint: &str,
) -> egui::Response {
    let colors = appearance::palette(ui.ctx());
    ui.add_enabled_ui(enabled, |ui| {
        let text = platform::display_path(path);
        let width = ui.available_width().max(80.0);
        let galley = ui.painter().layout(
            text.clone(),
            egui::FontId::proportional(14.0),
            colors.accent,
            (width - 48.0).max(32.0),
        );
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, (galley.size().y + 18.0).max(36.0)),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &text)
        });
        let widgets = &ui.visuals().widgets;
        let fill = if response.hovered() || response.has_focus() {
            widgets.hovered.bg_fill
        } else {
            colors.source
        };
        ui.painter().rect_filled(rect, 8, fill);
        if response.has_focus() {
            ui.painter()
                .rect_stroke(rect, 8, widgets.active.bg_stroke, egui::StrokeKind::Inside);
        }
        let origin = rect.left_center() + egui::vec2(12.0, -8.0);
        let stroke = egui::Stroke::new(1.5_f32, colors.accent);
        let p = |x, y| origin + egui::vec2(x, y);
        ui.painter().add(egui::Shape::line(
            vec![
                p(13.0, 9.0),
                p(13.0, 16.0),
                p(0.0, 16.0),
                p(0.0, 2.0),
                p(5.0, 2.0),
                p(7.0, 5.0),
                p(9.0, 5.0),
            ],
            stroke,
        ));
        ui.painter()
            .line_segment([p(8.0, 10.0), p(17.0, 1.0)], stroke);
        ui.painter().add(egui::Shape::line(
            vec![p(11.0, 1.0), p(17.0, 1.0), p(17.0, 7.0)],
            stroke,
        ));
        ui.painter().galley(
            rect.left_top() + egui::vec2(38.0, 9.0),
            galley,
            colors.accent,
        );
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(hint)
    })
    .inner
}

#[derive(Default)]
pub(super) struct StorageSettings {
    pub needs_scan: bool,
    pub open: bool,
    pub builds_page: bool,
    pub scanning: bool,
    pub deleting: bool,
    pub inventory: Option<cleanup::Inventory>,
    pub selected: BTreeSet<String>,
    pub confirmation: Option<CleanupRequest>,
    pub last_request: Option<CleanupRequest>,
    pub report: Option<CleanupReport>,
    pub error: Option<String>,
    pub copied: bool,
}
impl StorageSettings {
    pub fn busy(&self) -> bool {
        self.scanning || self.deleting
    }
}

impl PatcherApp {
    fn storage_status(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        blocked: bool,
        retry: &mut Option<CleanupRequest>,
        refresh: &mut bool,
    ) {
        let colors = appearance::palette(ctx);
        if let Some(report) = &self.storage.report {
            ui.label(RichText::new(report.message()).color(colors.success));
        }
        let errors = self
            .storage
            .error
            .clone()
            .into_iter()
            .chain(self.storage.report.iter().flat_map(|r| r.errors.clone()))
            .chain(
                self.storage
                    .inventory
                    .iter()
                    .flat_map(|i| i.warnings.clone()),
            )
            .collect::<Vec<_>>()
            .join("\n\n");
        if !errors.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("cleanup-errors")
                .max_height(70.0)
                .show(ui, |ui| {
                    ui.label(RichText::new(&errors).color(colors.error));
                });
            ui.horizontal(|ui| {
                if ui
                    .button(if self.storage.copied {
                        "Copied"
                    } else {
                        "Copy error"
                    })
                    .clicked()
                {
                    ctx.copy_text(errors);
                    self.storage.copied = true;
                }
                if ui
                    .add_enabled(!blocked, egui::Button::new("Retry"))
                    .clicked()
                {
                    *retry = self.storage.last_request.clone();
                    if retry.is_none() {
                        *refresh = true;
                    }
                }
            });
        }
        if self.storage.report.is_some() || self.storage.error.is_some() {
            ui.add_space(10.0);
        }
    }
    pub(super) fn storage_blocked(&self) -> bool {
        self.busy
            || self.scanning
            || self.dismounting
            || self.installing_adk
            || self.drivers.running
            || self.closing
            || self.storage.busy()
    }
    pub(super) fn scan_storage(&mut self, ctx: &egui::Context) {
        if self.storage.busy() {
            return;
        }
        self.storage.scanning = true;
        let cache = PathBuf::from(self.settings.cache.trim());
        let workspace = PathBuf::from(self.settings.work.trim());
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = cleanup::inventory(&cache, &workspace).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::StorageScanned(result));
            ctx.request_repaint();
        });
    }
    pub(super) fn start_cleanup(&mut self, mut request: CleanupRequest, ctx: &egui::Context) {
        if self.storage_blocked() {
            return;
        }
        match &mut request {
            CleanupRequest::Cache { protected, .. } => {
                if !self.iso.trim().is_empty() {
                    protected.push(
                        std::path::absolute(self.iso.trim())
                            .unwrap_or_else(|_| PathBuf::from(self.iso.trim())),
                    );
                }
            }
            CleanupRequest::Builds { ids, protected } | CleanupRequest::Isos { ids, protected } => {
                if let Some(inventory) = &self.inventory {
                    protected.push(inventory.iso.clone());
                }
                if !self.iso.trim().is_empty() {
                    protected.push(
                        std::path::absolute(self.iso.trim())
                            .unwrap_or_else(|_| PathBuf::from(self.iso.trim())),
                    );
                }
                if let Some(inventory) = &self.storage.inventory {
                    ids.retain(|id| inventory.builds.iter().any(|v| &v.record.id == id));
                }
                if ids.is_empty() {
                    return;
                }
            }
        }
        self.storage.confirmation = None;
        self.storage.error = None;
        self.storage.report = None;
        self.storage.copied = false;
        self.storage.last_request = Some(request.clone());
        self.storage.deleting = true;
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = match &request {
                CleanupRequest::Builds { .. } | CleanupRequest::Isos { .. } => ipc::cleanup_elevated(request.clone()),
                CleanupRequest::Cache { root, protected } => {
                    match cleanup::clear_cache(root, protected) {
                        Ok(first) if first.requires_admin => {
                            match ipc::cleanup_elevated(request.clone()) {
                                Ok(mut second) => {
                                    second.deleted.extend(first.deleted);
                                    second.bytes += first.bytes;
                                    Ok(second)
                                }
                                Err(e) => {let count=first.deleted.len();Ok(CleanupReport {deleted:first.deleted,bytes:first.bytes,errors:vec![format!("Administrator cleanup failed after removing {count} cache items: {e:#}")],requires_admin:false})},
                            }
                        }
                        Err(e)
                            if e.chain().any(|e| {
                                e.downcast_ref::<std::io::Error>().is_some_and(|e| {
                                    e.kind() == std::io::ErrorKind::PermissionDenied
                                })
                            }) =>
                        {
                            ipc::cleanup_elevated(request.clone())
                        }
                        result => result,
                    }
                }
            }
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::StorageCleaned(request, result));
            ctx.request_repaint();
        });
    }
    pub(super) fn storage_popup(&mut self, ctx: &egui::Context) {
        if !self.storage.open {
            return;
        }
        let colors = appearance::palette(ctx);
        let blocked = self.storage_blocked();
        let mut close = false;
        let mut refresh = false;
        let mut execute = None;
        let mut retry = None;
        let mut confirm = None;
        let mut cancel_confirmation = false;
        let frame = egui::Frame::new()
            .fill(colors.surface)
            .corner_radius(16)
            .stroke(egui::Stroke::new(
                1.0_f32,
                ctx.style().visuals.widgets.noninteractive.bg_stroke.color,
            ))
            .inner_margin(22.0);
        let modal=egui::Modal::new(egui::Id::new("storage-settings")).frame(frame).show(ctx,|ui|{
            let cache_confirmation=matches!(self.storage.confirmation,Some(CleanupRequest::Cache{..}));
            ui.set_width((ctx.content_rect().width()-88.0).clamp(240.0,if cache_confirmation || !self.storage.builds_page{560.0}else{760.0}));
            if let Some(request)=self.storage.confirmation.clone() {
                let cache=matches!(request,CleanupRequest::Cache{..});
                let iso_only=matches!(request,CleanupRequest::Isos{..});
                let multiple=matches!(&request,CleanupRequest::Builds{ids,..} if ids.len()>1);
                ui.label(RichText::new(if cache {"Clear cache?"}else if iso_only{"Delete ISO?"}else if multiple{"Delete builds?"}else{"Delete build?"}).size(23.0).strong());
                ui.add_space(8.0);
                ui.label(if cache {"Cached drivers and setup downloads will need downloading again."}else if iso_only{"Only the generated ISO will be deleted. The workspace and build logs will be kept."}else{"Generated ISOs, workspaces and build logs will be deleted. Unverified output files are kept."});
                ui.label(RichText::new("Deletion is permanent. Original source ISOs are kept.").color(colors.warning));
                let mut bytes=0;
                ui.add_space(16.0);
                egui::ScrollArea::vertical().id_salt("cleanup-confirm-paths").max_height((ctx.content_rect().height()-370.0).max(70.0)).show(ui,|ui|{
                    if let Some(inventory)=&self.storage.inventory {
                        match &request {
                            CleanupRequest::Cache{root,..}=>{
                                bytes=inventory.cache.size.bytes;
                                egui::Frame::new().fill(colors.source).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                                    ui.set_min_width(ui.available_width());
                                    ui.label(RichText::new("Cache location").strong());
                                    ui.add(egui::Label::new(platform::display_path(root)).wrap().selectable(true));
                                    ui.add_space(12.0);
                                    egui::CollapsingHeader::new(format!("Files to delete ({})",inventory.cache.size.files)).id_salt("cache-confirm-files").show(ui,|ui|{
                                        for path in &inventory.cache.paths {
                                            ui.add(egui::Label::new(platform::display_path(path)).wrap().selectable(true));
                                            ui.add_space(6.0);
                                        }
                                    });
                                });
                            },
                            CleanupRequest::Builds{ids,..} | CleanupRequest::Isos{ids,..}=>{for view in &inventory.builds {if ids.contains(&view.record.id){bytes+=if iso_only{if view.output_present{view.record.published.as_ref().map(|p|p.size).unwrap_or(0)}else{0}}else{view.size.bytes};
                                egui::Frame::new().fill(colors.source).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                                    ui.set_min_width(ui.available_width());
                                    ui.label(RichText::new(view.record.output.file_name().unwrap_or_default().to_string_lossy()).strong());
                                    if !iso_only{ui.label("Workspace");ui.add(egui::Label::new(platform::display_path(&view.record.job)).wrap().selectable(true));}
                                    if view.record.published.is_some() && view.output_present{ui.label("ISO");ui.add(egui::Label::new(platform::display_path(&view.record.output)).wrap().selectable(true));}
                                    if !iso_only{if let Some(path)=&view.record.partial{ui.add(egui::Label::new(platform::display_path(path)).wrap().selectable(true));}
                                    if let Some(path)=&view.record.connection_log{ui.add(egui::Label::new(platform::display_path(path)).wrap().selectable(true));}}
                                });ui.add_space(6.0);}}}
                        }
                    }
                });
                ui.add_space(12.0);ui.label(format!("{}: {}",if cache{"Cache size"}else{"Selected size"},cleanup::format_bytes(bytes)));
                if !cache {ui.label(RichText::new(if iso_only{"Windows may ask for administrator permission to delete the ISO."}else{"Windows may ask for administrator permission to clean image mounts."}).size(13.0).color(colors.muted));}
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                    if Self::primary(ui,if cache{"Clear cache"}else{"Delete permanently"},!blocked){execute=Some(request.clone());}
                    cancel_confirmation=ui.button("Cancel").clicked();
                });
                return;
            }
            if self.storage.builds_page && ui.button("Back to settings").clicked(){self.storage.builds_page=false;}
            ui.horizontal(|ui|{ui.label(RichText::new(if self.storage.builds_page{"Builds"}else{"Settings"}).size(25.0).strong());ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{refresh=ui.add_enabled(!self.storage.busy(),egui::Button::new("Refresh")).clicked();});});
            ui.label(RichText::new(if self.storage.builds_page{"Select builds to delete, or open their locations."}else{"Manage cached downloads and generated ISOs."}).size(13.0).color(colors.muted));
            ui.add_space(14.0);
            let status_start=ui.cursor().top();
            self.storage_status(ui,ctx,blocked,&mut retry,&mut refresh);
            let status_height=ui.cursor().top()-status_start;
            egui::ScrollArea::vertical().id_salt(("storage-content",self.storage.builds_page)).max_height((ctx.content_rect().height()-270.0-status_height).max(60.0)).show(ui,|ui|{
                if !self.storage.builds_page {
                egui::Frame::new().fill(colors.source).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("Cache").size(18.0).strong());
                    if let Some(inventory)=&self.storage.inventory {
                        ui.label(format!("{} cached files",inventory.cache.size.files));
                        ui.horizontal_wrapped(|ui|{ui.label(format!("Cache size: {}",cleanup::format_bytes(inventory.cache.size.bytes)));if ui.add_enabled(!blocked && !inventory.cache.paths.is_empty(),egui::Button::new("Clear cache")).clicked(){confirm=Some(CleanupRequest::Cache{root:PathBuf::from(self.settings.cache.trim()),protected:self.inventory.as_ref().map(|i|vec![i.iso.clone()]).unwrap_or_default()});}});
                    }else{ui.label("Scanning cached downloads…");}
                    ui.label(RichText::new(platform::display_path(&PathBuf::from(self.settings.cache.trim()))).size(12.0).color(colors.muted));
                });
                ui.add_space(16.0);
                egui::Frame::new().fill(colors.canvas).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("Builds").size(18.0).strong());
                    if let Some(inventory)=&self.storage.inventory {
                        ui.label(format!("{} workspaces",inventory.builds.len()));
                        ui.label(format!("Total space used: {}",cleanup::format_bytes(inventory.builds.iter().map(|v|v.size.bytes).sum())));
                    }else{ui.label("Finding saved workspaces…");}
                    ui.label(RichText::new("Generated ISOs and associated workspaces.").size(13.0).color(colors.muted));
                    ui.add_space(8.0);
                    if ui.button("Manage builds").clicked(){self.storage.builds_page=true;}
                });
                } else {
                if let Some(inventory)=&self.storage.inventory {
                    ui.label(format!("{} workspaces · Total space used: {}",inventory.builds.len(),cleanup::format_bytes(inventory.builds.iter().map(|v|v.size.bytes).sum())));
                    ui.add_space(8.0);
                }
                ui.horizontal_wrapped(|ui|{
                    if let Some(inventory)=&self.storage.inventory {
                        let eligible: BTreeSet<_>=inventory.builds.iter().filter(|v|v.problem.is_none()).map(|v|v.record.id.clone()).collect();
                        let all=!eligible.is_empty() && self.storage.selected==eligible;
                        if ui.add_enabled(!blocked && !eligible.is_empty(),egui::Button::new(if all{"Clear selection"}else{"Select all"})).clicked(){if all {self.storage.selected.clear();}else{self.storage.selected=eligible;}}
                        if ui.add_enabled(!blocked && !self.storage.selected.is_empty(),egui::Button::new(format!("Delete selected ({})",self.storage.selected.len()))).clicked(){confirm=Some(CleanupRequest::Builds{ids:self.storage.selected.iter().cloned().collect(),protected:vec![]});}
                    }
                });
                ui.label(RichText::new("Generated ISOs and their associated workspaces.").size(13.0).color(colors.muted));
                ui.add_space(8.0);
                if let Some(inventory)=&self.storage.inventory {
                    if inventory.builds.is_empty(){ui.label("No saved builds yet. Failed or interrupted builds will appear here too.");}
                    for view in &inventory.builds {
                        let r=&view.record;let selected=self.storage.selected.contains(&r.id);
                        let frame=egui::Frame::new().fill(if selected{colors.source}else{colors.canvas}).corner_radius(12).inner_margin(13.0)
                            .stroke(egui::Stroke::new(1.0_f32,if selected{colors.accent}else{ctx.style().visuals.widgets.noninteractive.bg_stroke.color}));
                        frame.show(ui,|ui|{
                            ui.set_min_width(ui.available_width());
                            let selection=ui.allocate_ui_with_layout(egui::vec2(ui.available_width(),0.0),egui::Layout::top_down(egui::Align::Min),|ui|{
                            ui.label(RichText::new(r.output.file_name().unwrap_or_default().to_string_lossy()).strong());
                            let model=MODEL_PROFILES.iter().find(|m|m.id==r.model).map(|m|m.name).unwrap_or(&r.model);
                            ui.label(RichText::new(format!("{model} · {}",r.editions.join(", "))).size(13.0));
                            let state=if r.published.is_some() && !view.output_present{"ISO missing · workspace only"}else if r.published.is_none(){match r.state { cleanup::BuildState::Failed=>"Failed · workspace only",cleanup::BuildState::Cancelled=>"Cancelled · workspace only",cleanup::BuildState::Building=>"Interrupted / in progress · workspace only",_=>"Incomplete / unknown · workspace only"}}else{r.state.label()};
                            ui.label(RichText::new(format!("{} · {state} · {}",cleanup::format_date(r.created),cleanup::format_bytes(view.size.bytes))).size(12.0).color(if r.state==cleanup::BuildState::Complete{colors.success}else{colors.warning}));
                            }).response;
                            if selection.interact(egui::Sense::click()).on_hover_text(if selected{"Deselect this build"}else{"Select this build"}).clicked() && !blocked && view.problem.is_none(){if selected{self.storage.selected.remove(&r.id);}else{self.storage.selected.insert(r.id.clone());}}
                            if let Some(problem)=&view.problem{ui.label(RichText::new(problem).size(12.0).color(colors.error));}
                            let paths=egui::CollapsingHeader::new("Paths").id_salt((&r.id,"paths"));
                            #[cfg(debug_assertions)]
                            let paths=paths.default_open(selected && std::env::var("MACBOOK_PATCHER_PREVIEW").as_deref()==Ok("settings-builds"));
                            paths.show(ui,|ui|{
                                ui.horizontal_top(|ui|{
                                ui.add_sized([78.0,36.0],egui::Label::new(RichText::new("Workspace").size(13.0).color(colors.muted)));
                                if storage_path_link(ui,&r.job,!self.storage.deleting,"Open workspace folder").clicked() && let Err(e)=platform::open_folder(&r.job){self.storage.error=Some(format!("Could not open workspace folder: {e:#}"));self.storage.last_request=None;self.storage.copied=false;}
                                });
                                ui.add_space(6.0);
                                ui.horizontal_top(|ui|{
                                ui.add_sized([78.0,36.0],egui::Label::new(RichText::new("ISO").size(13.0).color(colors.muted)));
                                if storage_path_link(ui,&r.output,!self.storage.deleting,"Open folder with ISO selected").clicked() && let Err(e)=platform::reveal_file(&r.output){self.storage.error=Some(format!("Could not open ISO location: {e:#}"));self.storage.last_request=None;self.storage.copied=false;}
                                });
                                if r.published.is_none(){ui.label("No verified generated ISO is recorded; only its workspace will be removed.");}
                            });
                            ui.add_space(8.0);
                            ui.horizontal(|ui|{
                                if ui.add_enabled(!blocked && view.problem.is_none() && view.output_present && r.published.is_some(),egui::Button::new("Delete ISO")).on_hover_text("Delete only the generated ISO; keep the workspace").clicked(){confirm=Some(CleanupRequest::Isos{ids:vec![r.id.clone()],protected:vec![]});}
                                if ui.add_enabled(!blocked && view.problem.is_none(),egui::Button::new("Delete")).on_hover_text("Delete the generated ISO and workspace").clicked(){confirm=Some(CleanupRequest::Builds{ids:vec![r.id.clone()],protected:vec![]});}
                            });
                        });
                        ui.add_space(7.0);
                    }
                }else{ui.label("Finding saved builds…");}
                }
            });
            ui.add_space(14.0);
            ui.horizontal(|ui|{
                if self.storage.deleting{ui.spinner();ui.label("Cleaning storage… Approve Windows if prompted.");}else if self.storage.scanning{ui.spinner();ui.label("Scanning storage…");}else if blocked{ui.label(RichText::new("Finish the current operation before deleting files.").size(12.0).color(colors.muted));}else if self.storage.builds_page && let Some(inventory)=&self.storage.inventory{ui.label(format!("{} selected · {}",self.storage.selected.len(),cleanup::format_bytes(inventory.builds.iter().filter(|v|self.storage.selected.contains(&v.record.id)).map(|v|v.size.bytes).sum())));}
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{close=ui.add_enabled(!self.storage.deleting,egui::Button::new("Close")).clicked();});
            });
        });
        if let Some(request) = confirm {
            self.storage.confirmation = Some(request);
        }
        if cancel_confirmation || (modal.should_close() && self.storage.confirmation.is_some()) {
            self.storage.confirmation = None;
        } else if (close || modal.should_close()) && !self.storage.deleting {
            self.storage.open = false;
        }
        if refresh {
            self.storage.error = None;
            self.storage.last_request = None;
            self.storage.report = None;
            self.storage.copied = false;
            self.scan_storage(ctx);
        }
        if let Some(request) = execute.or(retry) {
            self.start_cleanup(request, ctx);
        }
    }
}
