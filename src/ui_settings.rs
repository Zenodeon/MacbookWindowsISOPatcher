use super::*;
use macbook_iso_patcher::cleanup::{self, CleanupReport, CleanupRequest};

fn storage_paint_clip(
    bounds: egui::Rect,
    parent: egui::Rect,
    visuals: &egui::Visuals,
) -> egui::Rect {
    let margin = [&visuals.widgets.hovered, &visuals.widgets.active]
        .iter()
        .map(|v| v.expansion + v.bg_stroke.width)
        .fold(visuals.clip_rect_margin, f32::max);
    bounds.expand(margin).intersect(parent)
}

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
    #[cfg(test)]
    modal_rect: Option<egui::Rect>,
    #[cfg(test)]
    footer_rect: Option<egui::Rect>,
    pub needs_scan: bool,
    pub open: bool,
    pub builds_page: bool,
    pub scanning: bool,
    pub deleting: bool,
    pub moving: bool,
    pub move_progress: Option<macbook_iso_patcher::relocation::Progress>,
    pub recovery_failed: bool,
    pub inventory: Option<cleanup::Inventory>,
    pub navigation_epoch: u64,
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
    pub fn with_recovery_failure(recovery_failed: bool) -> Self {
        Self {
            recovery_failed,
            ..Default::default()
        }
    }
    pub fn navigate_builds(&mut self, builds: bool) {
        if self.deleting || self.moving {
            return;
        }
        if builds && !self.builds_page {
            self.navigation_epoch = self.navigation_epoch.wrapping_add(1);
        }
        self.builds_page = builds;
        self.cancel_confirmation();
    }
    pub fn cancel_confirmation(&mut self) {
        self.confirmation = None;
        self.confirmation_ready = false;
        self.execute_after_scan = false;
        self.ready_cleanup = None;
    }
    pub fn back(&mut self) {
        if self.deleting || self.moving {
            return;
        }
        if self.confirmation.is_some() {
            self.cancel_confirmation();
        } else {
            self.navigate_builds(false);
        }
    }
    pub fn busy(&self) -> bool {
        self.scanning || self.deleting || self.moving
    }
    pub fn apply_inventory(&mut self, inventory: cleanup::Inventory) {
        let previous = self
            .confirmation
            .as_ref()
            .and_then(|r| confirmation_manifest(self.inventory.as_ref()?, r));
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
    fn storage_workflow_busy(&self) -> bool {
        self.storage.recovery_failed
            || self.busy
            || self.scanning
            || self.dismounting
            || self.installing_adk
            || self.drivers.running
            || self.closing
    }
    pub(super) fn storage_read_only(&self) -> bool {
        self.page != Page::Model || self.storage_workflow_busy()
    }
    pub(super) fn storage_blocked(&self) -> bool {
        self.storage_read_only() || self.storage.busy()
    }
    pub(super) fn storage_move_gate(&self) -> macbook_iso_patcher::relocation::Gate {
        macbook_iso_patcher::relocation::Gate {
            model_selection: self.page == Page::Model,
            before_build: matches!(
                self.page,
                Page::Model | Page::Iso | Page::Editions | Page::Review
            ),
            idle: !self.storage_workflow_busy() && !self.storage.busy(),
        }
    }
    pub(super) fn storage_move_request(
        &self,
        kind: macbook_iso_patcher::relocation::Kind,
        destination: PathBuf,
    ) -> anyhow::Result<macbook_iso_patcher::relocation::Request> {
        use macbook_iso_patcher::relocation::{self, Kind};
        let gate = self.storage_move_gate();
        gate.check(kind)?;
        let (source, other_root) = match kind {
            Kind::Cache => (&self.settings.cache, &self.settings.work),
            Kind::Workspace => (&self.settings.work, &self.settings.cache),
        };
        let mut protected_sources = self
            .inventory
            .as_ref()
            .map(|i| vec![i.iso.clone()])
            .unwrap_or_default();
        if !self.iso.trim().is_empty() {
            protected_sources.push(PathBuf::from(self.iso.trim()));
        }
        Ok(relocation::Request {
            kind,
            source: source.trim().into(),
            destination,
            other_root: other_root.trim().into(),
            protected_sources,
            settings_path: self.settings_file.clone(),
            settings: serde_json::to_value(&self.settings)?,
            history_root: cleanup::history_dir(),
            gate,
        })
    }
    pub(super) fn start_storage_move(
        &mut self,
        kind: macbook_iso_patcher::relocation::Kind,
        ctx: &egui::Context,
    ) {
        use macbook_iso_patcher::relocation;
        if let Err(error) = self.storage_move_gate().check(kind) {
            self.storage.error = Some(error.to_string());
            return;
        }
        let Some(destination) = rfd::FileDialog::new()
            .set_title("Choose an empty destination folder")
            .pick_folder()
        else {
            return;
        };
        let request = match self.storage_move_request(kind, destination) {
            Ok(request) => request,
            Err(error) => {
                self.storage.error = Some(error.to_string());
                return;
            }
        };
        self.storage.open = true;
        self.storage.navigate_builds(false);
        self.storage.cancel_confirmation();
        self.storage.moving = true;
        self.storage.move_progress = None;
        self.storage.error = None;
        self.storage.notice = None;
        self.storage.last_request = None;
        self.storage.report = None;
        let tx = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let progress = |progress| {
                let _ = tx.send(Message::StorageMoveProgress(progress));
                ctx.request_repaint();
            };
            let result = relocation::execute(request, &progress).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Message::StorageMoved(kind, result));
            ctx.request_repaint();
        });
    }
    pub(super) fn workspace_field(&mut self, ui: &mut egui::Ui) {
        use macbook_iso_patcher::relocation::Kind;
        ui.label("WorkingSpace");
        ui.add(
            egui::Label::new(platform::display_path(std::path::Path::new(
                self.settings.work.trim(),
            )))
            .wrap()
            .selectable(true),
        );
        if ui
            .add_enabled(
                self.storage_move_gate().check(Kind::Workspace).is_ok(),
                egui::Button::new("Change WorkingSpace"),
            )
            .on_disabled_hover_text(Kind::Workspace.locked_reason())
            .clicked()
        {
            self.start_storage_move(Kind::Workspace, ui.ctx());
        }
    }
    fn storage_move_controls(
        &mut self,
        ui: &mut egui::Ui,
        kind: macbook_iso_patcher::relocation::Kind,
    ) -> bool {
        use macbook_iso_patcher::relocation::Kind;
        let (path, label) = match kind {
            Kind::Cache => (&self.settings.cache, "Move cache folder"),
            Kind::Workspace => (&self.settings.work, "Move WorkingSpace folder"),
        };
        let path = PathBuf::from(path);
        if path.is_dir() {
            if storage_path_link(ui, &path, !self.storage.moving, "Open folder").clicked() {
                self.open_storage_location(&path, false);
            }
        } else {
            ui.add(
                egui::Label::new(platform::display_path(&path))
                    .wrap()
                    .selectable(true),
            );
        }
        let mut ancestor = path.as_path();
        while !ancestor.exists() {
            let Some(parent) = ancestor.parent() else {
                break;
            };
            ancestor = parent;
        }
        ui.horizontal_wrapped(|ui| {
            if let Ok(bytes) = fs2::available_space(ancestor) {
                ui.label(format!("{} free", cleanup::format_bytes(bytes)));
                ui.add_space(8.0);
            }
            ui.add_enabled(
                self.storage_move_gate().check(kind).is_ok(),
                egui::Button::new(label),
            )
            .on_disabled_hover_text(kind.locked_reason())
            .clicked()
        })
        .inner
    }
    pub(super) fn scan_storage(&mut self, ctx: &egui::Context) {
        if self.storage.busy() || self.storage.recovery_failed {
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
        let mut navigate = None;
        let mut open_location = None;
        let mut move_folder = None;
        let frame = egui::Frame::new()
            .fill(colors.surface)
            .corner_radius(16)
            .stroke(egui::Stroke::new(
                1.0_f32,
                ctx.style().visuals.widgets.noninteractive.bg_stroke.color,
            ))
            .inner_margin(22.0);
        let modal=egui::Modal::new(egui::Id::new("storage-settings")).frame(frame).show(ctx,|ui|{
            let width=(ctx.content_rect().width()-88.0).clamp(240.0,760.0);
            let height=(ctx.content_rect().height()-100.0).clamp(220.0,560.0);
            // Only this allocation determines the modal size. Children are
            // independent bounded UIs: their content cannot enlarge the frame.
            let (shell_rect,_)=ui.allocate_exact_size(egui::vec2(width,height),egui::Sense::hover());
            let footer_rect=egui::Rect::from_min_max(egui::pos2(shell_rect.left(),shell_rect.bottom()-38.0),shell_rect.max);
            let body_rect=egui::Rect::from_min_max(shell_rect.min,egui::pos2(shell_rect.right(),footer_rect.top()-14.0));
            let mut body=ui.new_child(egui::UiBuilder::new().max_rect(body_rect).layout(egui::Layout::top_down(egui::Align::Min)));
            body.set_clip_rect(storage_paint_clip(body_rect,ui.clip_rect(),ui.visuals()));
            let confirmation_kind=match self.storage.confirmation {None=>0,Some(CleanupRequest::Cache{..})=>1,Some(CleanupRequest::Builds{..})=>2,Some(CleanupRequest::Isos{..})=>3};
            let scroll=egui::ScrollArea::vertical().id_salt(("storage-page",self.storage.builds_page,confirmation_kind,self.storage.navigation_epoch)).max_height(body_rect.height()).auto_shrink([false,false]);
            #[cfg(debug_assertions)]
            let scroll=if std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_some() && self.screenshot_frames==1 {
                std::env::var("MACBOOK_PATCHER_PREVIEW_SCROLL").ok().and_then(|v|v.parse::<f32>().ok()).map(|offset|scroll.clone().vertical_scroll_offset(offset)).unwrap_or(scroll)
            }else{scroll};
            scroll.show(&mut body,|ui|{
            if let Some(request)=self.storage.confirmation.clone() {
                let cache=matches!(request,CleanupRequest::Cache{..});
                let iso_only=matches!(request,CleanupRequest::Isos{..});
                let multiple=matches!(&request,CleanupRequest::Builds{ids,..} if ids.len()>1);
                ui.label(RichText::new(if cache {"Clear cache?"}else if iso_only{"Delete ISO?"}else if multiple{"Delete builds?"}else{"Delete build?"}).size(23.0).strong());
                ui.add_space(8.0);
                if !self.storage.confirmation_ready || self.storage.scanning {
                    ui.horizontal(|ui|{ui.spinner();ui.label("Checking which files are still available…");});
                    return;
                }
                ui.scope(|ui|{
                if let Some(notice)=&self.storage.notice{ui.label(RichText::new(notice).color(colors.muted));ui.add_space(8.0);}
                ui.label(if cache {"Cached drivers and setup downloads will need downloading again."}else if iso_only{"Only the generated ISO listed below will be deleted."}else{"Only the listed build files and folders will be deleted. Unverified output files are kept."});
                ui.label(RichText::new("Deletion is permanent. Original source ISOs are kept.").color(colors.warning));
                let mut bytes=0;
                ui.add_space(16.0);
                ui.scope(|ui|{
                    if let Some(inventory)=&self.storage.inventory {
                        match &request {
                            CleanupRequest::Cache{root,..}=>{
                                bytes=inventory.cache.size.bytes;
                                egui::Frame::new().fill(colors.source).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                                    ui.set_min_width(ui.available_width());
                                    ui.label(RichText::new("Cache location").strong());
                                    ui.add(egui::Label::new(platform::display_path(root)).wrap().selectable(true));
                                    ui.add_space(12.0);
                                    let files=egui::CollapsingHeader::new(format!("Files to delete ({})",inventory.cache.size.files)).id_salt("cache-confirm-files");
                                    #[cfg(debug_assertions)]
                                    let files=files.default_open(std::env::var("MACBOOK_PATCHER_PREVIEW").as_deref()==Ok("settings-live-cache"));
                                    files.show(ui,|ui|{
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
                ui.add_space(12.0);ui.label(format!("{}: {}",if cache{"Cache size"}else if iso_only{"ISO size"}else{"Build size"},cleanup::format_bytes(bytes)));
                if !cache {ui.label(RichText::new(if iso_only{"Windows may ask for administrator permission to delete the ISO."}else{"Windows may ask for administrator permission to clean image mounts."}).size(13.0).color(colors.muted));}
                });
                return;
            }
            ui.horizontal(|ui|{ui.label(RichText::new(if self.storage.builds_page{"Builds"}else{"Settings"}).size(25.0).strong());ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{refresh=ui.add_enabled(!self.storage.busy(),egui::Button::new("Refresh")).clicked();});});
            ui.label(RichText::new(if self.storage.builds_page{"Open build locations or delete files from each card."}else{"Manage cached downloads and generated ISOs."}).size(13.0).color(colors.muted));
            if self.storage_read_only(){ui.add_space(6.0);ui.label(RichText::new("Cache moves and cleanup require idle model selection. WorkingSpace can be changed while setup is idle.").size(13.0).color(colors.muted));}
            ui.add_space(14.0);
            self.storage_status(ui,ctx,blocked,&mut retry,&mut refresh);
            if let Some(p)=&self.storage.move_progress {
                ui.label(format!("{} · {} / {} files · {} / {}",p.phase,p.files,p.total_files,cleanup::format_bytes(p.bytes),cleanup::format_bytes(p.total_bytes)));
                ui.add(egui::ProgressBar::new(if p.total_bytes>0{p.bytes as f32/p.total_bytes as f32}else{0.0}).desired_height(18.0).animate(true));
                ui.add_space(8.0);
            }
            ui.scope(|ui|{
                if !self.storage.builds_page {
                egui::Frame::new().fill(colors.source).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("Cache").size(18.0).strong());
                    if let Some(inventory)=&self.storage.inventory {
                        ui.label(format!("{} cached files",inventory.cache.size.files));
                        ui.horizontal_wrapped(|ui|{ui.label(format!("Cache size: {}",cleanup::format_bytes(inventory.cache.size.bytes)));if ui.add_enabled(!blocked && !inventory.cache.paths.is_empty(),egui::Button::new("Clear cache")).clicked(){confirm=Some(CleanupRequest::Cache{root:PathBuf::from(self.settings.cache.trim()),protected:self.inventory.as_ref().map(|i|vec![i.iso.clone()]).unwrap_or_default()});}});
                    }else{ui.label("Scanning cached downloads…");}
                    if self.storage_move_controls(ui,macbook_iso_patcher::relocation::Kind::Cache){move_folder=Some(macbook_iso_patcher::relocation::Kind::Cache);}
                });
                ui.add_space(16.0);
                egui::Frame::new().fill(colors.canvas).corner_radius(12).inner_margin(16.0).show(ui,|ui|{
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("WorkingSpace & builds").size(18.0).strong());
                    if self.storage_move_controls(ui,macbook_iso_patcher::relocation::Kind::Workspace){move_folder=Some(macbook_iso_patcher::relocation::Kind::Workspace);}
                    if let Some(inventory)=&self.storage.inventory {
                        ui.label(format!("{} workspaces",inventory.workspace_count()));
                        ui.label(format!("{}: {}",inventory.space_label(),cleanup::format_bytes(inventory.total_bytes())));
                    }else{ui.label("Finding saved workspaces…");}
                    ui.label(RichText::new("Generated ISOs and associated workspaces.").size(13.0).color(colors.muted));
                    ui.add_space(8.0);
                    if ui.button("Manage builds").clicked(){navigate=Some(true);refresh=true;}
                });
                } else {
                if let Some(inventory)=&self.storage.inventory {
                    ui.label(format!("{} workspaces · {}: {}",inventory.workspace_count(),inventory.space_label(),cleanup::format_bytes(inventory.total_bytes())));
                    ui.add_space(8.0);
                }
                if let Some(inventory)=&self.storage.inventory {
                    if inventory.builds.is_empty(){ui.label("No saved builds yet. Failed or interrupted builds will appear here too.");}
                    for view in &inventory.builds {
                        let r=&view.record;
                        let frame=egui::Frame::new().fill(colors.canvas).corner_radius(12).inner_margin(13.0)
                            .stroke(egui::Stroke::new(1.0_f32,ctx.style().visuals.widgets.noninteractive.bg_stroke.color));
                        frame.show(ui,|ui|{
                            ui.set_min_width(ui.available_width());
                            ui.horizontal_top(|ui|{
                            let summary_width=(ui.available_width()-190.0).max(100.0);
                            ui.allocate_ui_with_layout(egui::vec2(summary_width,0.0),egui::Layout::top_down(egui::Align::Min),|ui|{
                            ui.label(RichText::new(view.title()).strong());
                            let model=MODEL_PROFILES.iter().find(|m|m.id==r.model).map(|m|m.name).unwrap_or(&r.model);
                            ui.label(RichText::new(format!("{model} · {}",r.editions.join(", "))).size(13.0));
                            ui.label(RichText::new(format!("{} · {} · {}",cleanup::format_date(r.created),view.status(),cleanup::format_bytes(view.size.bytes))).size(12.0).color(if view.complete(){colors.success}else{colors.warning}));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min),|ui|{
                                if ui.add_enabled(!blocked && view.can_delete(),egui::Button::new("Delete")).on_hover_text("Delete the remaining build files, including any verified ISO and workspace").clicked(){confirm=Some(CleanupRequest::Builds{ids:vec![r.id.clone()],protected:vec![]});}
                                if view.can_delete_iso() && ui.add_enabled(!blocked,egui::Button::new("Delete ISO")).on_hover_text("Delete only the generated ISO; keep the workspace").clicked(){confirm=Some(CleanupRequest::Isos{ids:vec![r.id.clone()],protected:vec![]});}
                            });
                            });
                            if let Some(problem)=&view.problem{ui.label(RichText::new(problem).size(12.0).color(colors.error));}
                            if view.artifacts().any(|a|a.available()) {
                            let paths=egui::CollapsingHeader::new("Paths").id_salt((&r.id,"paths"));
                            #[cfg(debug_assertions)]
                            let paths=paths.default_open(inventory.builds.first().is_some_and(|v|v.record.id==r.id) && std::env::var("MACBOOK_PATCHER_PREVIEW").is_ok_and(|v|v=="settings-builds" || v.starts_with("settings-live")));
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
            });
            let mut footer_ui=ui.new_child(egui::UiBuilder::new().max_rect(footer_rect).layout(egui::Layout::top_down(egui::Align::Min)));
            footer_ui.set_clip_rect(storage_paint_clip(footer_rect,ui.clip_rect(),ui.visuals()));
            let footer=footer_ui.horizontal(|ui|{
                if self.storage.confirmation.is_some(){cancel_confirmation=ui.add_enabled(!self.storage.deleting,egui::Button::new("Cancel")).clicked();}
                else if self.storage.builds_page && ui.add_enabled(!self.storage.deleting,egui::Button::new("Back to settings")).clicked(){navigate=Some(false);}
                if self.storage.moving{ui.spinner();ui.label("Moving folder…");}else if self.storage.deleting{ui.spinner();ui.label("Cleaning storage…");}else if self.storage.scanning && self.storage.confirmation.is_none(){ui.spinner();ui.label("Checking files…");}
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                    close=ui.add_enabled(!self.storage.deleting && !self.storage.moving,egui::Button::new("Close")).clicked();
                    if let Some(request)=&self.storage.confirmation && Self::primary(ui,if matches!(request,CleanupRequest::Cache{..}){"Clear cache"}else{"Delete permanently"},!blocked && self.storage.confirmation_ready){execute=Some(request.clone());}
                });
            });
            #[cfg(test)]
            {self.storage.footer_rect=Some(footer.response.rect);}
            #[cfg(not(test))]
            let _=footer;
        });
        #[cfg(test)]
        {
            self.storage.modal_rect = Some(modal.response.rect);
        }
        if let Some(kind) = move_folder {
            self.start_storage_move(kind, ctx);
        }
        if let Some(builds) = navigate {
            self.storage.navigate_builds(builds);
            ctx.request_repaint();
        }
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
            self.storage.back();
            ctx.request_repaint();
        } else if (close || modal.should_close()) && !self.storage.deleting && !self.storage.moving
        {
            self.storage.open = false;
            self.storage.cancel_confirmation();
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
    #[test]
    fn working_space_moves_use_setup_stages_and_idle_state() {
        use macbook_iso_patcher::relocation::Kind;
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        for page in [
            Page::Model,
            Page::Iso,
            Page::Editions,
            Page::Review,
            Page::Build,
            Page::Complete,
        ] {
            app.page = page;
            assert_eq!(
                app.storage_move_gate().check(Kind::Workspace).is_ok(),
                !matches!(page, Page::Build | Page::Complete)
            );
            assert_eq!(
                app.storage_move_gate().check(Kind::Cache).is_ok(),
                page == Page::Model
            );
        }
        app.page = Page::Review;
        app.donation = true;
        assert!(app.storage_move_gate().check(Kind::Workspace).is_ok());
        for state in 0..10 {
            app.busy = state == 0;
            app.scanning = state == 1;
            app.dismounting = state == 2;
            app.installing_adk = state == 3;
            app.drivers.running = state == 4;
            app.storage.scanning = state == 5;
            app.storage.deleting = state == 6;
            app.storage.moving = state == 7;
            app.closing = state == 8;
            app.storage.recovery_failed = state == 9;
            assert!(app.storage_move_gate().check(Kind::Workspace).is_err());
            assert!(
                app.storage_move_request(Kind::Workspace, "never-used".into())
                    .is_err()
            );
        }
        app.storage.recovery_failed = false;
        app.page = Page::Build;
        app.last_build_error = Some("Stopped build".into());
        assert!(app.storage_move_gate().check(Kind::Workspace).is_err());
        app.page = Page::Review;
        assert!(app.storage_move_gate().check(Kind::Workspace).is_ok());
        assert!(app.storage_move_gate().check(Kind::Cache).is_err());
        app.storage.moving = true;
        app.review_build(&ctx);
        assert!(app.error.is_none());
    }
    #[test]
    fn working_space_move_from_review_keeps_setup_and_driver_readiness() {
        use macbook_iso_patcher::relocation::{self, Kind};
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        let root = std::env::temp_dir().join(format!(
            "working-space-review-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(root.join("work")).unwrap();
        std::fs::write(root.join("work/retained.txt"), b"retained working files").unwrap();
        std::fs::write(root.join("source.iso"), b"selected source").unwrap();
        app.page = Page::Review;
        app.model_selected = Some(0);
        app.donation = true;
        app.settings.work = root.join("work").display().to_string();
        app.settings.cache = root.join("cache").display().to_string();
        app.settings_file = root.join("local/settings.json");
        app.iso = root.join("source.iso").display().to_string();
        app.output = root.join("patched.iso").display().to_string();
        app.inventory = Some(macbook_iso_patcher::domain::Inventory {
            iso: root.join("source.iso"),
            iso_size: 15,
            iso_modified: 0,
            install_file: "sources/install.wim".into(),
            setup_image: None,
            images: vec![],
        });
        app.selected.insert(6);
        app.drivers.ready = true;
        app.drivers.requested_model = Some(MODEL_PROFILES[0].id);
        app.drivers.key = Some((MODEL_PROFILES[0].id, root.join("cache")));
        app.drivers.id = 7;
        assert!(app.drivers_ready());
        let inventory = serde_json::to_value(&app.inventory).unwrap();
        let iso = app.iso.clone();
        let output = app.output.clone();
        let selected = app.selected.clone();
        let destination = root.join("new-work");
        let mut request = app
            .storage_move_request(Kind::Workspace, destination.clone())
            .unwrap();
        request.history_root = root.join("history");
        app.storage.moving = true;
        let outcome = relocation::execute(request, &|_| {}).unwrap();
        app.sender
            .send(Message::StorageMoved(Kind::Workspace, Ok(outcome)))
            .unwrap();
        app.consume();
        assert_eq!(app.page, Page::Review);
        assert!(app.donation);
        assert_eq!(app.model_selected, Some(0));
        assert_eq!(app.iso, iso);
        assert_eq!(app.output, output);
        assert_eq!(app.selected, selected);
        assert_eq!(serde_json::to_value(&app.inventory).unwrap(), inventory);
        assert!(app.drivers_ready());
        assert_eq!(app.drivers.id, 7);
        assert!(!app.storage.moving);
        assert_eq!(
            app.request().unwrap().workspace.canonicalize().unwrap(),
            destination.canonicalize().unwrap()
        );
        assert_eq!(
            std::fs::read(destination.join("retained.txt")).unwrap(),
            b"retained working files"
        );
        let saved: Settings = serde_json::from_slice(
            &relocation::load_settings(&app.settings_file, &root.join("missing-legacy.json"))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.work, app.settings.work);
        assert_eq!(saved.cache, app.settings.cache);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn settings_become_read_only_after_continue_not_model_selection() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        app.model_selected = Some(0);
        assert!(!app.storage_read_only());
        assert!(!app.storage_blocked());
        for page in [Page::Iso, Page::Editions, Page::Review, Page::Build] {
            app.page = page;
            assert!(app.storage_read_only());
            app.start_cleanup(
                CleanupRequest::Cache {
                    root: PathBuf::from("never-used"),
                    protected: vec![],
                },
                &ctx,
            );
            assert!(!app.storage.deleting);
            app.start_storage_move(macbook_iso_patcher::relocation::Kind::Cache, &ctx);
            assert!(!app.storage.moving);
        }
        app.page = Page::Model;
        assert!(!app.storage_read_only());
        app.drivers.running = true;
        assert!(app.storage_read_only());
    }
    #[test]
    fn moving_blocks_work_and_idle_model_selection_unlocks_moves() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        app.model_selected = Some(0);
        app.storage.moving = true;
        assert!(app.storage.busy());
        assert!(app.storage_blocked());
        app.scan(&ctx);
        app.install_adk(&ctx);
        app.begin(&ctx);
        assert!(!app.scanning && !app.installing_adk && !app.busy);
        app.storage.moving = false;
        for state in 0..7 {
            app.busy = state == 0;
            app.scanning = state == 1;
            app.dismounting = state == 2;
            app.installing_adk = state == 3;
            app.drivers.running = state == 4;
            app.storage.scanning = state == 5;
            app.storage.deleting = state == 6;
            assert!(app.storage_blocked());
        }
        app.storage.deleting = false;
        assert!(!app.storage_blocked());
        app.page = Page::Iso;
        assert!(app.storage_blocked());
        app.page = Page::Model;
        assert!(!app.storage_blocked());
    }
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
    fn refresh_cancels_empty_confirmation() {
        let mut settings = StorageSettings {
            inventory: Some(inventory(true, true)),
            confirmation: Some(request()),
            ..Default::default()
        };
        settings.apply_inventory(Inventory::default());
        assert!(settings.confirmation.is_none());
        assert!(settings.ready_cleanup.is_none());
    }
    #[test]
    fn back_from_confirmation_cancels_pending_cleanup_and_keeps_builds_page() {
        let mut settings = StorageSettings {
            builds_page: true,
            confirmation: Some(request()),
            execute_after_scan: true,
            ready_cleanup: Some(request()),
            ..Default::default()
        };
        settings.back();
        assert!(settings.builds_page && settings.confirmation.is_none());
        assert!(!settings.execute_after_scan && settings.ready_cleanup.is_none());
        settings.apply_inventory(inventory(true, true));
        assert!(settings.ready_cleanup.is_none());
        settings.back();
        assert!(!settings.builds_page);
    }
    #[test]
    fn entering_builds_resets_list_once_and_cancel_keeps_its_scroll_identity() {
        let mut settings = StorageSettings::default();
        settings.navigate_builds(true);
        let epoch = settings.navigation_epoch;
        settings.confirmation = Some(request());
        settings.back();
        assert_eq!(settings.navigation_epoch, epoch);
        settings.back();
        settings.navigate_builds(true);
        assert_ne!(settings.navigation_epoch, epoch);
    }
    #[test]
    fn busy_cleanup_cannot_navigate_away() {
        let mut settings = StorageSettings {
            builds_page: true,
            deleting: true,
            ..Default::default()
        };
        settings.back();
        settings.navigate_builds(false);
        assert!(settings.builds_page);
    }
    fn render_popup(app: &mut PatcherApp, ctx: &egui::Context, size: egui::Vec2) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ctx| app.storage_popup(ctx),
        );
    }
    #[test]
    fn popup_and_footer_do_not_jump_between_pages_or_during_checks() {
        for size in [
            egui::vec2(1240.0, 700.0),
            egui::vec2(800.0, 520.0),
            egui::vec2(640.0, 400.0),
        ] {
            let ctx = egui::Context::default();
            let mut app = PatcherApp::new_with_context(&ctx);
            app.storage = StorageSettings {
                open: true,
                inventory: Some(inventory(true, true)),
                ..Default::default()
            };
            render_popup(&mut app, &ctx, size);
            render_popup(&mut app, &ctx, size);
            let baseline = app.storage.modal_rect.unwrap();
            let footer = app.storage.footer_rect.unwrap();
            app.storage.navigate_builds(true);
            for phase in 0..9 {
                match phase {
                    0 => {}
                    1 => {
                        app.storage.confirmation = Some(request());
                        app.storage.confirmation_ready = false;
                        app.storage.scanning = true;
                    }
                    2 => {
                        app.storage.scanning = false;
                        app.storage.confirmation_ready = true;
                    }
                    3 => {
                        app.storage.back();
                    }
                    4 => {
                        app.storage.back();
                    }
                    5 => {
                        app.storage.confirmation = Some(CleanupRequest::Cache {
                            root: std::env::temp_dir().join("Long cache directory ".repeat(100)),
                            protected: vec![],
                        });
                        app.storage.confirmation_ready = false;
                        app.storage.scanning = true;
                    }
                    6 => {
                        app.storage.scanning = false;
                        app.storage.confirmation_ready = true;
                        app.storage.notice = Some("The available cache files changed. ".repeat(30));
                    }
                    7 => {
                        app.storage.back();
                        app.storage.notice = None;
                        app.storage.error = Some("A directory could not be read. ".repeat(300));
                    }
                    8 => {
                        app.storage.error = None;
                        app.storage.navigate_builds(true);
                        app.storage.inventory.as_mut().unwrap().builds[0]
                            .record
                            .editions = vec!["An unusually long edition name ".repeat(100)];
                    }
                    _ => unreachable!(),
                }
                render_popup(&mut app, &ctx, size);
                let actual = app.storage.modal_rect.unwrap();
                let actual_footer = app.storage.footer_rect.unwrap();
                assert!(
                    (actual.min - baseline.min).length() < 1.0
                        && (actual.max - baseline.max).length() < 1.0,
                    "phase {phase}, screen {size:?}: {actual:?} vs {baseline:?}"
                );
                assert!(
                    (actual_footer.top() - footer.top()).abs() < 1.0,
                    "footer shifted at phase {phase}: {actual_footer:?} vs {footer:?}"
                );
                assert!(actual.bottom() <= size.y && actual.right() <= size.x);
            }
        }
    }
    #[test]
    fn escape_from_confirmation_cancels_a_just_finished_preflight() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        app.storage = StorageSettings {
            open: true,
            builds_page: true,
            inventory: Some(inventory(true, true)),
            confirmation: Some(request()),
            confirmation_ready: true,
            ..Default::default()
        };
        let size = egui::vec2(1240.0, 700.0);
        render_popup(&mut app, &ctx, size);
        render_popup(&mut app, &ctx, size);
        app.storage.ready_cleanup = Some(request());
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| app.storage_popup(ctx),
        );
        assert!(app.storage.open && app.storage.builds_page);
        assert!(app.storage.confirmation.is_none() && app.storage.ready_cleanup.is_none());
    }
    #[test]
    fn cancel_click_in_fixed_footer_returns_to_same_popup() {
        let ctx = egui::Context::default();
        let mut app = PatcherApp::new_with_context(&ctx);
        app.storage = StorageSettings {
            open: true,
            builds_page: true,
            inventory: Some(inventory(true, true)),
            confirmation: Some(request()),
            confirmation_ready: true,
            ..Default::default()
        };
        let size = egui::vec2(1240.0, 700.0);
        render_popup(&mut app, &ctx, size);
        render_popup(&mut app, &ctx, size);
        let before = app.storage.modal_rect.unwrap();
        let position = app.storage.footer_rect.unwrap().left_center() + egui::vec2(24.0, 0.0);
        for pressed in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events: vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| app.storage_popup(ctx),
            );
        }
        assert!(app.storage.open && app.storage.builds_page && app.storage.confirmation.is_none());
        render_popup(&mut app, &ctx, size);
        assert_eq!(app.storage.modal_rect.unwrap(), before);
    }
    #[test]
    fn hover_outlines_at_body_and_footer_edges_are_not_clipped() {
        for theme in [appearance::ThemeMode::Light, appearance::ThemeMode::Dark] {
            for footer in [false, true] {
                let ctx = egui::Context::default();
                let mut app = PatcherApp::new_with_context(&ctx);
                theme.apply(&ctx);
                app.storage = StorageSettings {
                    open: true,
                    inventory: Some(inventory(true, true)),
                    ..Default::default()
                };
                let size = egui::vec2(1240.0, 700.0);
                render_popup(&mut app, &ctx, size);
                render_popup(&mut app, &ctx, size);
                let bounds = app.storage.modal_rect.unwrap();
                let bar = app.storage.footer_rect.unwrap();
                let position = if footer {
                    bar.right_center() - egui::vec2(32.0, 0.0)
                } else {
                    egui::pos2(bar.right() - 40.0, bounds.top() + 40.0)
                };
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events: vec![egui::Event::PointerMoved(position)],
                        ..Default::default()
                    },
                    |ctx| app.storage_popup(ctx),
                );
                let mut found = false;
                for shape in output.shapes {
                    if let egui::Shape::Rect(rect) = &shape.shape
                        && rect.rect.contains(position)
                        && rect.rect.width() < 160.0
                        && rect.rect.height() < 60.0
                        && rect.stroke.width > 0.0
                    {
                        found = true;
                        assert!(
                            shape
                                .clip_rect
                                .contains_rect(shape.shape.visual_bounding_rect()),
                            "{theme:?}, footer={footer}: outline {:?} exceeds clip {:?}",
                            shape.shape.visual_bounding_rect(),
                            shape.clip_rect
                        );
                    }
                }
                assert!(
                    found,
                    "Hovered button outline not found: {theme:?}, footer={footer}"
                );
                assert_eq!(app.storage.modal_rect.unwrap(), bounds);
            }
        }
    }
}
