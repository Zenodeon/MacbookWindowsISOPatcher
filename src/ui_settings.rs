use super::*;
use macbook_iso_patcher::cleanup::{self, CleanupReport, CleanupRequest};

#[derive(Default)]
pub(super) struct StorageSettings {
    pub needs_scan: bool,
    pub open: bool,
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
            CleanupRequest::Builds { ids, protected } => {
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
                CleanupRequest::Builds { .. } => ipc::cleanup_elevated(request.clone()),
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
            ui.set_width((ctx.content_rect().width()-88.0).clamp(240.0,if cache_confirmation{560.0}else{760.0}));
            if let Some(request)=self.storage.confirmation.clone() {
                let cache=matches!(request,CleanupRequest::Cache{..});
                ui.label(RichText::new(if cache {"Clear cache?"}else{"Delete selected builds?"}).size(23.0).strong());
                ui.add_space(8.0);
                ui.label(if cache {"Cached drivers and setup downloads will need downloading again."}else{"Recorded generated ISOs, selected workspaces and build logs will be deleted. Unverified output files are kept."});
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
                            CleanupRequest::Builds{ids,..}=>{for view in &inventory.builds {if ids.contains(&view.record.id){bytes+=view.size.bytes;ui.label(RichText::new(view.record.output.file_name().unwrap_or_default().to_string_lossy()).strong());if view.record.published.is_some(){ui.label(platform::display_path(&view.record.output));}ui.label(platform::display_path(&view.record.job));if let Some(path)=&view.record.partial{ui.label(platform::display_path(path));}
if let Some(path)=&view.record.connection_log{ui.label(platform::display_path(path));}ui.add_space(6.0);}}}
                        }
                    }
                });
                ui.add_space(12.0);ui.label(format!("Estimated space to reclaim: {}",cleanup::format_bytes(bytes)));
                if !cache {ui.label(RichText::new("Windows may ask for administrator permission to clean image mounts.").size(13.0).color(colors.muted));}
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                    if Self::primary(ui,if cache{"Clear cache"}else{"Delete permanently"},!blocked){execute=Some(request.clone());}
                    cancel_confirmation=ui.button("Cancel").clicked();
                });
                return;
            }
            ui.horizontal(|ui|{ui.label(RichText::new("Settings").size(25.0).strong());ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{refresh=ui.add_enabled(!self.storage.busy(),egui::Button::new("Refresh")).clicked();});});
            ui.label(RichText::new("Manage cached downloads and generated ISOs.").size(13.0).color(colors.muted));
            ui.add_space(14.0);
            let status_start=ui.cursor().top();
            self.storage_status(ui,ctx,blocked,&mut retry,&mut refresh);
            let status_height=ui.cursor().top()-status_start;
            egui::ScrollArea::vertical().id_salt("storage-content").max_height((ctx.content_rect().height()-245.0-status_height).max(60.0)).show(ui,|ui|{
                egui::Frame::new().fill(colors.source).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("Cache").size(18.0).strong());
                    if let Some(inventory)=&self.storage.inventory {
                        ui.horizontal_wrapped(|ui|{ui.label(format!("{} cached files · {}",inventory.cache.size.files,cleanup::format_bytes(inventory.cache.size.bytes)));if ui.add_enabled(!blocked && !inventory.cache.paths.is_empty(),egui::Button::new("Clear cache")).clicked(){confirm=Some(CleanupRequest::Cache{root:PathBuf::from(self.settings.cache.trim()),protected:self.inventory.as_ref().map(|i|vec![i.iso.clone()]).unwrap_or_default()});}});
                    }else{ui.label("Scanning cached downloads…");}
                    ui.label(RichText::new(platform::display_path(&PathBuf::from(self.settings.cache.trim()))).size(12.0).color(colors.muted));
                });
                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui|{
                    ui.label(RichText::new("Builds").size(18.0).strong());
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
                        let row=frame.show(ui,|ui|{
                            ui.set_min_width(ui.available_width());
                            ui.label(RichText::new(r.output.file_name().unwrap_or_default().to_string_lossy()).strong());
                            let model=MODEL_PROFILES.iter().find(|m|m.id==r.model).map(|m|m.name).unwrap_or(&r.model);
                            ui.label(RichText::new(format!("{model} · {}",r.editions.join(", "))).size(13.0));
                            let state=if r.published.is_some() && !view.output_present{"ISO missing · workspace only"}else if r.published.is_none(){match r.state { cleanup::BuildState::Failed=>"Failed · workspace only",cleanup::BuildState::Cancelled=>"Cancelled · workspace only",cleanup::BuildState::Building=>"Interrupted / in progress · workspace only",_=>"Incomplete / unknown · workspace only"}}else{r.state.label()};
                            ui.label(RichText::new(format!("{} · {state} · {}",cleanup::format_date(r.created),cleanup::format_bytes(view.size.bytes))).size(12.0).color(if r.state==cleanup::BuildState::Complete{colors.success}else{colors.warning}));
                            if let Some(problem)=&view.problem{ui.label(RichText::new(problem).size(12.0).color(colors.error));}
                            egui::CollapsingHeader::new("Paths").id_salt((&r.id,"paths")).show(ui,|ui|{ui.label(format!("ISO: {}",platform::display_path(&r.output)));ui.label(format!("Workspace: {}",platform::display_path(&r.job)));if r.published.is_none(){ui.label("No verified generated ISO is recorded; only its workspace will be removed.");}});
                        });
                        if row.response.interact(egui::Sense::click()).on_hover_text("Select this build").clicked() && !blocked && view.problem.is_none(){if selected{self.storage.selected.remove(&r.id);}else{self.storage.selected.insert(r.id.clone());}}
                        ui.add_space(7.0);
                    }
                }else{ui.label("Finding saved builds…");}
            });
            ui.add_space(14.0);
            ui.horizontal(|ui|{
                if self.storage.deleting{ui.spinner();ui.label("Cleaning storage… Approve Windows if prompted.");}else if self.storage.scanning{ui.spinner();ui.label("Scanning storage…");}else if blocked{ui.label(RichText::new("Finish the current operation before deleting files.").size(12.0).color(colors.muted));}
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
