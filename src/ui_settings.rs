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
    pub confirmation_ready: bool,
    pub execute_after_scan: bool,
    pub ready_cleanup: Option<CleanupRequest>,
    pub notice: Option<String>,
    pub focused: bool,
    pub last_request: Option<CleanupRequest>,
    pub report: Option<CleanupReport>,
    pub error: Option<String>,
    pub copied: bool,
}
impl StorageSettings {
    pub fn busy(&self) -> bool {
        self.scanning || self.deleting
    }
    pub fn apply_inventory(&mut self, inventory: cleanup::Inventory) {
        let previous = self
            .confirmation
            .as_ref()
            .and_then(|r| confirmation_manifest(self.inventory.as_ref()?, r));
        self.selected.retain(|id| {
            inventory
                .builds
                .iter()
                .any(|v| &v.record.id == id && v.can_delete())
        });
        let current = self
            .confirmation
            .as_ref()
            .and_then(|r| confirmation_manifest(&inventory, r));
        self.inventory = Some(inventory);
        self.confirmation_ready = current.as_ref().is_some_and(|m| !m.is_empty());
        if self.confirmation.is_some() && !self.confirmation_ready {
            self.confirmation = None;
            self.notice = Some(
                "Selected files are no longer available for deletion. Review the refreshed list."
                    .into(),
            );
        } else if self.execute_after_scan && previous == current {
            self.ready_cleanup = self.confirmation.clone();
        } else if self.execute_after_scan {
            self.notice =
                Some("Files changed. Review the updated paths before confirming again.".into());
        }
        self.execute_after_scan = false;
    }
}

fn confirmation_manifest(
    inventory: &cleanup::Inventory,
    request: &CleanupRequest,
) -> Option<Vec<(PathBuf, u64)>> {
    let mut entries = match request {
        CleanupRequest::Cache { .. } => inventory
            .cache
            .paths
            .iter()
            .map(|p| (p.clone(), inventory.cache.size.bytes))
            .collect::<Vec<_>>(),
        CleanupRequest::Builds { ids, .. } | CleanupRequest::Isos { ids, .. } => {
            let iso_only = matches!(request, CleanupRequest::Isos { .. });
            let mut entries = Vec::new();
            for id in ids {
                let view = inventory.builds.iter().find(|v| &v.record.id == id)?;
                if (iso_only && !view.can_delete_iso()) || (!iso_only && !view.can_delete()) {
                    return None;
                }
                entries.extend(
                    view.deletable_artifacts(iso_only)
                        .into_iter()
                        .map(|a| (a.path.clone(), a.size.bytes)),
                );
            }
            entries
        }
    };
    entries.sort();
    entries.dedup();
    Some(entries)
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
        if let Some(notice) = &self.storage.notice {
            ui.label(RichText::new(notice).color(colors.muted));
            ui.add_space(8.0);
        }
        if let Some(report) = &self.storage.report {
            ui.label(RichText::new(report.message()).color(colors.success));
        }
        let errors = self
            .storage
            .error
            .clone()
            .into_iter()
            .chain(self.storage.report.iter().flat_map(|r| r.errors.clone()))
            .chain(self.storage.inventory.iter().flat_map(|i| {
                i.warnings
                    .clone()
                    .into_iter()
                    .chain(i.builds.iter().filter_map(|v| v.problem.clone()))
            }))
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
        self.storage.confirmation_ready = false;
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
    fn open_storage_location(&mut self, path: &std::path::Path, iso: bool) {
        let result = if iso {
            platform::reveal_file(path)
        } else {
            platform::open_folder(path)
        };
        if let Err(e) = result {
            if e.chain().any(|e| {
                e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            }) {
                self.storage.notice = Some("Location no longer exists.".into());
            } else {
                self.storage.error = Some(format!("Could not open location: {e:#}"));
                self.storage.last_request = None;
                self.storage.copied = false;
            }
            self.storage.needs_scan = true;
        }
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
        self.storage.ready_cleanup = None;
        self.storage.confirmation_ready = false;
        self.storage.error = None;
        self.storage.report = None;
        self.storage.copied = false;
        self.storage.notice = None;
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
        let mut open_location = None;
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
                if !self.storage.confirmation_ready || self.storage.scanning {
                    ui.horizontal(|ui|{ui.spinner();ui.label("Checking which files are still available…");});
                    ui.add_space(16.0);
                    cancel_confirmation=ui.button("Cancel").clicked();
                    return;
                }
                if let Some(notice)=&self.storage.notice{ui.label(RichText::new(notice).color(colors.muted));ui.add_space(8.0);}
                ui.label(if cache {"Cached drivers and setup downloads will need downloading again."}else if iso_only{"Only the generated ISO listed below will be deleted."}else{"Only the listed build files and folders will be deleted. Unverified output files are kept."});
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
                            CleanupRequest::Builds{ids,..} | CleanupRequest::Isos{ids,..}=>{for view in &inventory.builds {if ids.contains(&view.record.id){
                                egui::Frame::new().fill(colors.source).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                                    ui.set_min_width(ui.available_width());
                                    ui.label(RichText::new(view.title()).strong());
                                    for artifact in view.deletable_artifacts(iso_only) {
                                        if artifact.path==view.workspace.path {ui.label("Workspace");}
                                        else if artifact.path==view.output.path {ui.label("ISO");}
                                        bytes+=if artifact.path!=view.workspace.path && view.workspace.available() && artifact.path.starts_with(&view.workspace.path){0}else{artifact.size.bytes};
                                        ui.add(egui::Label::new(platform::display_path(&artifact.path)).wrap().selectable(true));
                                    }
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
                        ui.label(format!("{} workspaces",inventory.workspace_count()));
                        ui.label(format!("{}: {}",inventory.space_label(),cleanup::format_bytes(inventory.total_bytes())));
                    }else{ui.label("Finding saved workspaces…");}
                    ui.label(RichText::new("Generated ISOs and associated workspaces.").size(13.0).color(colors.muted));
                    ui.add_space(8.0);
                    if ui.button("Manage builds").clicked(){self.storage.builds_page=true;refresh=true;}
                });
                } else {
                if let Some(inventory)=&self.storage.inventory {
                    ui.label(format!("{} workspaces · {}: {}",inventory.workspace_count(),inventory.space_label(),cleanup::format_bytes(inventory.total_bytes())));
                    ui.add_space(8.0);
                }
                ui.horizontal_wrapped(|ui|{
                    if let Some(inventory)=&self.storage.inventory {
                        let eligible: BTreeSet<_>=inventory.builds.iter().filter(|v|v.can_delete()).map(|v|v.record.id.clone()).collect();
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
                            ui.horizontal_top(|ui|{
                            let summary_width=(ui.available_width()-190.0).max(100.0);
                            let selection=ui.allocate_ui_with_layout(egui::vec2(summary_width,0.0),egui::Layout::top_down(egui::Align::Min),|ui|{
                            ui.label(RichText::new(view.title()).strong());
                            let model=MODEL_PROFILES.iter().find(|m|m.id==r.model).map(|m|m.name).unwrap_or(&r.model);
                            ui.label(RichText::new(format!("{model} · {}",r.editions.join(", "))).size(13.0));
                            ui.label(RichText::new(format!("{} · {} · {}",cleanup::format_date(r.created),view.status(),cleanup::format_bytes(view.size.bytes))).size(12.0).color(if view.complete(){colors.success}else{colors.warning}));
                            }).response;
                            if selection.interact(egui::Sense::click()).on_hover_text(if selected{"Deselect this build"}else{"Select this build"}).clicked() && !blocked && view.can_delete(){if selected{self.storage.selected.remove(&r.id);}else{self.storage.selected.insert(r.id.clone());}}
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min),|ui|{
                                if ui.add_enabled(!blocked && view.can_delete(),egui::Button::new("Delete")).on_hover_text("Delete the remaining build files, including any verified ISO and workspace").clicked(){confirm=Some(CleanupRequest::Builds{ids:vec![r.id.clone()],protected:vec![]});}
                                if view.can_delete_iso() && ui.add_enabled(!blocked,egui::Button::new("Delete ISO")).on_hover_text("Delete only the generated ISO; keep the workspace").clicked(){confirm=Some(CleanupRequest::Isos{ids:vec![r.id.clone()],protected:vec![]});}
                            });
                            });
                            if let Some(problem)=&view.problem{ui.label(RichText::new(problem).size(12.0).color(colors.error));}
                            if view.artifacts().any(|a|a.available()) {
                            let paths=egui::CollapsingHeader::new("Paths").id_salt((&r.id,"paths"));
                            #[cfg(debug_assertions)]
                            let paths=paths.default_open(selected && std::env::var("MACBOOK_PATCHER_PREVIEW").is_ok_and(|v|v=="settings-builds" || v.starts_with("settings-live")));
                            paths.show(ui,|ui|{
                                if view.workspace.available(){ui.horizontal_top(|ui|{
                                ui.add_sized([78.0,36.0],egui::Label::new(RichText::new("Workspace").size(13.0).color(colors.muted)));
                                if storage_path_link(ui,&r.job,!self.storage.deleting,"Open workspace folder").clicked(){open_location=Some((r.job.clone(),false));}
                                });}
                                if view.output.available(){ui.add_space(6.0);ui.horizontal_top(|ui|{
                                ui.add_sized([78.0,36.0],egui::Label::new(RichText::new("ISO").size(13.0).color(colors.muted)));
                                if storage_path_link(ui,&r.output,!self.storage.deleting,"Open folder with ISO selected").clicked(){open_location=Some((r.output.clone(),true));}
                                });}
                                for artifact in view.partial.iter().chain(view.connection_log.iter()).filter(|a|a.available()) {
                                    ui.add_space(6.0);ui.label(if view.partial.as_ref().is_some_and(|a|a.path==artifact.path){"Temporary ISO"}else{"Connection log"});
                                    if storage_path_link(ui,&artifact.path,!self.storage.deleting,"Open containing folder").clicked(){open_location=Some((artifact.path.clone(),true));}
                                }
                                if view.output.available() && !view.verified_iso{ui.label("Unverified output is kept during cleanup.");}
                            });
                            }
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
            self.storage.notice = None;
            self.storage.execute_after_scan = false;
            self.scan_storage(ctx);
        }
        if let Some((path, iso)) = open_location {
            self.open_storage_location(&path, iso);
            ctx.request_repaint();
        }
        if cancel_confirmation || (modal.should_close() && self.storage.confirmation.is_some()) {
            self.storage.confirmation = None;
            self.storage.execute_after_scan = false;
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
        if let Some(request) = execute {
            self.storage.confirmation = Some(request);
            self.storage.execute_after_scan = true;
            self.scan_storage(ctx);
        } else if let Some(request) = retry {
            self.storage.confirmation = Some(request);
            self.storage.execute_after_scan = false;
            self.scan_storage(ctx);
        }
    }
}

#[cfg(test)]
mod availability_tests {
    use super::*;
    use cleanup::{
        ArtifactView, BuildRecord, BuildState, BuildView, Fingerprint, Inventory, StorageSize,
    };
    fn inventory(workspace: bool, iso: bool) -> Inventory {
        let root = std::env::temp_dir().join("storage-confirmation-state");
        let job = root.join("build-test");
        let output = root.join("generated.iso");
        let artifact = |path: PathBuf, present| ArtifactView {
            path,
            present,
            size: StorageSize {
                files: u64::from(present),
                bytes: if present { 10 } else { 0 },
            },
            problem: None,
        };
        Inventory {
            builds: vec![BuildView {
                workspace: artifact(job.clone(), workspace),
                output: artifact(output.clone(), iso),
                verified_iso: iso,
                partial: None,
                connection_log: None,
                size: StorageSize {
                    files: u64::from(workspace) + u64::from(iso),
                    bytes: 10 * (u64::from(workspace) + u64::from(iso)),
                },
                problem: None,
                record: BuildRecord {
                    id: "test".into(),
                    job,
                    workspace: root.clone(),
                    source: root.join("source.iso"),
                    output,
                    partial: None,
                    connection_log: None,
                    model: "a1706".into(),
                    editions: vec!["Pro".into()],
                    created: 0,
                    state: BuildState::Complete,
                    published: Some(Fingerprint {
                        size: 10,
                        modified: 0,
                        identity: "test".into(),
                    }),
                },
            }],
            ..Default::default()
        }
    }
    fn request() -> CleanupRequest {
        CleanupRequest::Builds {
            ids: vec!["test".into()],
            protected: vec![],
        }
    }
    #[test]
    fn new_confirmation_requires_scan_and_matches_only_current_paths() {
        let mut settings = StorageSettings {
            inventory: Some(inventory(true, true)),
            confirmation: Some(request()),
            ..Default::default()
        };
        assert!(!settings.confirmation_ready);
        settings.apply_inventory(inventory(true, false));
        assert!(settings.confirmation_ready && settings.ready_cleanup.is_none());
        let manifest = confirmation_manifest(
            settings.inventory.as_ref().unwrap(),
            settings.confirmation.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.len(), 1);
        assert!(manifest[0].0.ends_with("build-test"));
    }
    #[test]
    fn disappearing_iso_cancels_iso_confirmation() {
        let mut settings = StorageSettings {
            inventory: Some(inventory(true, true)),
            confirmation: Some(CleanupRequest::Isos {
                ids: vec!["test".into()],
                protected: vec![],
            }),
            execute_after_scan: true,
            ..Default::default()
        };
        settings.apply_inventory(inventory(true, false));
        assert!(settings.confirmation.is_none() && settings.ready_cleanup.is_none());
        assert!(settings.notice.is_some());
    }
    #[test]
    fn changed_manifest_requires_another_confirmation() {
        let mut settings = StorageSettings {
            inventory: Some(inventory(true, true)),
            confirmation: Some(request()),
            execute_after_scan: true,
            ..Default::default()
        };
        settings.apply_inventory(inventory(false, true));
        assert!(settings.confirmation_ready && settings.confirmation.is_some());
        assert!(
            settings.ready_cleanup.is_none()
                && settings.notice.as_ref().unwrap().contains("Files changed")
        );
    }
    #[test]
    fn unchanged_fresh_scan_allows_confirmed_cleanup() {
        let mut settings = StorageSettings {
            inventory: Some(inventory(true, true)),
            confirmation: Some(request()),
            execute_after_scan: true,
            ..Default::default()
        };
        settings.apply_inventory(inventory(true, true));
        assert!(settings.ready_cleanup.is_some());
        assert!(!settings.execute_after_scan);
    }
    #[test]
    fn refresh_drops_missing_selection_and_cancels_empty_confirmation() {
        let mut settings = StorageSettings {
            selected: BTreeSet::from(["test".into()]),
            inventory: Some(inventory(true, true)),
            confirmation: Some(request()),
            ..Default::default()
        };
        settings.apply_inventory(Inventory::default());
        assert!(settings.selected.is_empty() && settings.confirmation.is_none());
        assert!(settings.ready_cleanup.is_none());
    }
}
