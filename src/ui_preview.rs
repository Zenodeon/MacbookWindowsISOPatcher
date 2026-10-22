//! Debug-only, screenshot-only fixtures. Never present in release builds.
use super::*;

impl PatcherApp {
    pub(super) fn configure_preview(&mut self) {
        if std::env::var_os("MACBOOK_PATCHER_SCREENSHOT").is_none() {
            return;
        }
        let Ok(view) = std::env::var("MACBOOK_PATCHER_PREVIEW") else {
            return;
        };
        self.window_fitted = true; // Keep the explicitly requested screenshot viewport size.
        if view.starts_with("output-conflict") {
            self.close_ready = true;
            self.model_selected = Some(0);
            self.page = Page::Review;
            let path =
                std::path::absolute(".cache/output-preview/Windows10_A1706_Pro.iso").unwrap();
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"Disposable screenshot placeholder").unwrap();
            self.output_conflict = Some(output_choices::OutputDialog {
                path: path.clone(),
                retry: BuildRetry::Review,
                candidate: Some(macbook_iso_patcher::cleanup::OutputCandidate {
                    path,
                    size: 5_200_000_000,
                    fingerprint: None,
                    id: Some("preview-only".into()),
                    reason: None,
                }),
                checking: view.ends_with("busy"),
                confirming: view.ends_with("confirm") || view.ends_with("busy"),
                problem: None,
                copied: false,
            });
            return;
        }
        if view.starts_with("settings-live") {
            if view.ends_with("-limited") {
                self.page = Page::Iso;
                self.model_selected = Some(0);
            }
            let view = view.strip_suffix("-limited").unwrap_or(&view);
            self.storage.open = true;
            self.storage.builds_page =
                view != "settings-live-summary" && view != "settings-live-cache";
            self.settings_file = std::env::temp_dir().join("macbook-patcher-preview-settings.json");
            match macbook_iso_patcher::cleanup::inventory(
                &PathBuf::from(self.settings.cache.trim()),
                &PathBuf::from(self.settings.work.trim()),
            ) {
                Ok(inventory) => {
                    if view == "settings-live-confirm" || view == "settings-live-checking" {
                        self.storage.confirmation =
                            Some(macbook_iso_patcher::cleanup::CleanupRequest::Workspaces {
                                ids: inventory
                                    .builds
                                    .iter()
                                    .take(1)
                                    .map(|v| v.record.id.clone())
                                    .collect(),
                                protected: vec![],
                            });
                    }
                    if view == "settings-live-cache" {
                        self.storage.confirmation =
                            Some(macbook_iso_patcher::cleanup::CleanupRequest::Cache {
                                root: PathBuf::from(self.settings.cache.trim()),
                                protected: vec![],
                            });
                    }
                    self.storage.apply_inventory(inventory);
                    if view == "settings-live-checking" {
                        self.storage.scanning = true;
                        self.storage.confirmation_ready = false;
                        self.close_ready = true;
                    }
                }
                Err(e) => self.storage.error = Some(format!("{e:#}")),
            }
            if std::env::var_os("MACBOOK_PATCHER_PREVIEW_SMALL").is_some() {
                self.window_fitted = true;
            }
            return;
        }
        if view.starts_with("settings") {
            use macbook_iso_patcher::cleanup::*;
            self.storage.open = true;
            self.storage.builds_page = view == "settings-builds"
                || view == "settings-iso-confirm"
                || view == "settings-build-confirm";
            self.settings_file = std::env::temp_dir().join("macbook-patcher-preview-settings.json");
            let mut inventory = macbook_iso_patcher::cleanup::Inventory::default();
            if view != "settings-empty" && view != "settings-scanning" {
                inventory.cache = CacheInventory {
                    paths: vec![
                        PathBuf::from(r"C:\Downloads\Patcher cache\win64_15.45.5174.exe"),
                        PathBuf::from(r"C:\Downloads\Patcher cache\BootCampESD-041-88734.pkg"),
                    ],
                    size: StorageSize {
                        files: 12,
                        bytes: 1320 * 1024 * 1024,
                    },
                };
                for (index, name, state) in [
                    (0, "Windows10_A1706_Pro.iso", BuildState::Complete),
                    (2, "Windows10_A1707_Pro.iso", BuildState::Failed),
                ] {
                    let id = uuid::Uuid::new_v4().to_string();
                    let root = std::path::absolute(
                        PathBuf::from(".cache").join(format!("storage-preview-{id}")),
                    )
                    .unwrap();
                    let workspace = root.join("workspaces");
                    let job = workspace.join(format!("build-{id}"));
                    std::fs::create_dir_all(&job).unwrap();
                    std::fs::write(job.join("build.log"), b"Disposable screenshot fixture")
                        .unwrap();
                    let output = root.join(name);
                    if state == BuildState::Complete {
                        std::fs::write(&output, b"Disposable screenshot ISO placeholder").unwrap();
                    }
                    let record = BuildRecord {
                        id: id.clone(),
                        job,
                        workspace,
                        source: root.join("original.iso"),
                        output: output.clone(),
                        partial: None,
                        connection_log: None,
                        model: MODEL_PROFILES[index].id.into(),
                        editions: vec!["Windows 10 Pro".into()],
                        created: 1792440000,
                        state: state.clone(),
                        published: if state == BuildState::Complete {
                            Some(fingerprint(&output).unwrap())
                        } else {
                            None
                        },
                    };
                    inventory.builds.push(inspect_build(record));
                }
            }
            if view == "settings-confirm" {
                self.storage.confirmation = Some(CleanupRequest::Workspaces {
                    ids: inventory
                        .builds
                        .iter()
                        .take(1)
                        .map(|v| v.record.id.clone())
                        .collect(),
                    protected: vec![],
                });
            }
            if view == "settings-build-confirm" {
                self.storage.confirmation = Some(CleanupRequest::Workspaces {
                    ids: inventory
                        .builds
                        .iter()
                        .take(1)
                        .map(|v| v.record.id.clone())
                        .collect(),
                    protected: vec![],
                });
            }
            if view == "settings-iso-confirm" {
                self.storage.confirmation = Some(CleanupRequest::Isos {
                    ids: inventory
                        .builds
                        .iter()
                        .take(1)
                        .map(|v| v.record.id.clone())
                        .collect(),
                    protected: vec![],
                });
            }
            if view == "settings-cache-confirm" {
                self.storage.confirmation = Some(CleanupRequest::Cache {
                    root: self.settings.cache.clone().into(),
                    protected: vec![],
                });
            }
            self.storage.confirmation_ready = self.storage.confirmation.is_some();
            if view == "settings-error" {
                let record = inventory.builds[0].record.clone();
                std::fs::write(&record.output, b"Changed disposable output").unwrap();
                inventory.builds[0] = inspect_build(record);
                self.storage.builds_page = true;
            }
            if view == "settings-busy" {
                self.storage.deleting = true;
                self.close_ready = true;
            }
            if view == "settings-moving" {
                self.storage.moving = true;
                self.close_ready = true;
                self.storage.move_progress = Some(macbook_iso_patcher::relocation::Progress {
                    files: 8,
                    total_files: 20,
                    bytes: 350_000_000,
                    total_bytes: 800_000_000,
                    phase: "Copying files".into(),
                });
            }
            if view == "settings-limited" {
                self.page = Page::Iso;
                self.model_selected = Some(0);
            }
            if view == "settings-workingspace" {
                self.page = Page::Review;
                self.model_selected = Some(0);
            }
            if view == "settings-workingspace-donation" {
                self.page = Page::Review;
                self.model_selected = Some(0);
                self.donation = true;
            }
            if view == "settings-build-locked" {
                self.page = Page::Build;
                self.busy = true;
                self.close_ready = true;
            }
            if view == "settings-scanning" {
                self.storage.scanning = true;
                self.close_ready = true;
            } else {
                self.storage.apply_inventory(inventory);
            }
            if std::env::var_os("MACBOOK_PATCHER_PREVIEW_SMALL").is_some() {
                self.window_fitted = true;
            }
            return;
        }
        let (view, model_index) = match view.as_str() {
            "models-download" => ("models", 0),
            "iso-download" | "iso-driver-ready" | "iso-driver-error" => ("iso", 0),
            "models-unverified" => ("models", 1),
            "models-dual" => ("models", 2),
            "iso-unverified" => ("iso", 1),
            "editions-unverified" => ("editions", 1),
            "review-unverified" => ("review-ready", 1),
            "workspace-error" | "generic-error" | "worker-error" => ("review-ready", 0),
            "build-active"
            | "build-download"
            | "build-download-unknown"
            | "build-preparing"
            | "build-log"
            | "build-failed"
            | "build-failure-popup"
            | "complete" => ("review-ready", 0),
            "complete-2017" | "complete-2017-usb" | "review-2017" => ("review-ready", 3),
            "complete-unverified" => ("review-ready", 1),
            other if other.starts_with("complete-usb-") => ("review-ready", 0),
            "review-unverified-download" => ("review-ready", 1),
            other => (other, 0),
        };
        let driver_preview = std::env::var("MACBOOK_PATCHER_PREVIEW").unwrap_or_default();
        if driver_preview.ends_with("-download") || driver_preview.contains("-driver-") {
            self.model_selected = Some(model_index);
            self.drivers.requested_model = Some(MODEL_PROFILES[model_index].id);
            self.drivers.key = Some((
                MODEL_PROFILES[model_index].id,
                self.settings.cache.trim().into(),
            ));
            self.drivers.stage = "Downloading drivers".into();
            self.drivers.ready = driver_preview.ends_with("-ready");
            if driver_preview.ends_with("-error") {
                self.drivers.error = Some(
                    "The connection was interrupted. Retry to download the remaining files.".into(),
                );
            } else if !self.drivers.ready {
                self.drivers.progress = Some(DownloadProgress::new(
                    "BootCampESD-041-88734.pkg".into(),
                    106_000_000,
                    Some(353_187_226),
                    Some(12_300_000),
                ));
            }
        }
        if view == "mounted-close" {
            // Used only by the opt-in Windows lifecycle test with its own data ISO.
            let iso = PathBuf::from(
                std::env::var_os("MACBOOK_PATCHER_PREVIEW_ISO").expect("Lifecycle test ISO"),
            );
            self.iso_mount =
                Some(platform::MountedIso::open(&iso).expect("Mount lifecycle test ISO"));
            self.iso = iso.display().to_string();
            self.model_selected = Some(0);
            self.page = Page::Iso;
            return;
        }
        if view == "models" {
            if model_index != 0 {
                self.model_selected = Some(model_index);
            }
            return;
        }
        self.model_selected = Some(model_index);
        if view == "iso" {
            self.page = Page::Iso;
            self.iso = "C:\\Preview\\Windows10_English_x64.iso".into();
            return;
        }
        if !["editions", "review-ready", "review-missing", "dismount"].contains(&view) {
            return;
        }
        self.iso = "C:\\Preview\\Windows10_English_x64.iso".into();
        self.output = format!(
            "C:\\Preview\\Win10_{}_Patched.iso",
            MODEL_PROFILES[model_index].id.replace('-', "_")
        );
        self.settings_file = std::env::temp_dir().join("macbook-patcher-preview-settings.json");
        self.inventory = Some(Inventory {
            iso: self.iso.clone().into(),
            iso_size: 5 * GIB,
            iso_modified: 0,
            install_file: "sources/install.wim".into(),
            setup_image: None,
            images: [
                ("Windows 10 Pro", "Professional"),
                ("Windows 10 Home", "Core"),
                ("Windows 10 Education", "Education"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (name, edition))| Image {
                index: i as u32 + 1,
                name: name.into(),
                edition: edition.into(),
                arch: 9,
                build: 19045,
                revision: Some(2965),
                major: 10,
                minor: 0,
                language: "en-US".into(),
                installation_type: "Client".into(),
                size: 14 * GIB,
            })
            .collect(),
        });
        if let Some(inv) = &mut self.inventory {
            inv.setup_image = inv.images.first().cloned().map(|mut image| {
                image.index = 2;
                image.build = 19041;
                image
            });
        }
        self.selected.insert(1);
        self.page = if view.starts_with("review") {
            Page::Review
        } else {
            Page::Editions
        };
        if view == "review-ready" {
            self.drivers.requested_model = Some(MODEL_PROFILES[model_index].id);
            self.drivers.key = Some((
                MODEL_PROFILES[model_index].id,
                self.settings.cache.trim().into(),
            ));
            self.drivers.ready = !driver_preview.ends_with("-download");
            // Existing fixture paths drive the UI's file-presence state only.
            // The fake ISO cannot pass build preflight; release has no fixtures.
            let existing = std::env::current_exe().unwrap().display().to_string();
            self.settings.oscdimg = existing.clone();
            self.settings.seven_zip = existing;
        } else if view == "review-missing" {
            self.settings.oscdimg.clear();
            self.settings.seven_zip.clear();
        }
        if matches!(
            driver_preview.as_str(),
            "build-active"
                | "build-download"
                | "build-download-unknown"
                | "build-preparing"
                | "build-log"
                | "build-failed"
                | "build-failure-popup"
        ) {
            self.page = Page::Build;
            self.busy = true;
            self.close_ready = true; // Fixture has no worker to wait for.
            self.stage = if driver_preview == "build-preparing" {
                "Preparing your build"
            } else {
                "Servicing Windows"
            }
            .into();
            self.detail = "Adding the selected graphics and Boot Camp drivers.".into();
            self.build_progress.apply(&Event::OverallProgress {
                completed: 6,
                total: 12,
            });
            self.build_progress.apply(&Event::OperationProgress {
                operation: "Importing Intel graphics driver".into(),
                percent: Some(47.0),
            });
            if driver_preview.starts_with("build-download") {
                self.build_progress.operation = None;
                self.stage = "Downloading drivers".into();
                self.detail = "BootCampESD-041-88734.pkg".into();
                self.download = Some(DownloadProgress::new(
                    self.detail.clone(),
                    106_000_000,
                    (driver_preview != "build-download-unknown").then_some(353_187_226),
                    Some(12_300_000),
                ));
            }
            if matches!(
                driver_preview.as_str(),
                "build-log" | "build-failed" | "build-failure-popup"
            ) {
                self.logs = vec![
                    "[worker +0.12s] authenticating worker: read timeout: writer=Some(3s), reader=Some(3s)".into(),
                    "[worker +0.13s] sending build request: read timeout: writer=None, reader=None".into(),
                    "Checking Windows media".into(),
                    "Copying Windows media from C:\\Preview\\Windows10_English_x64.iso into the retained build workspace before exporting the selected Windows 10 Pro image.".into(),
                    "Deployment Image Servicing and Management tool\nVersion: 10.0.26100.5074\nImage Version: 10.0.19041.2006".into(),
                    "Adding signed Intel Iris 550 graphics driver".into(),
                    "The operation completed successfully.".into(),
                ];
                self.logs
                    .extend((1..=30).map(|n| format!("[DISM] Servicing selected image: {n}.0%")));
            }
            if driver_preview.starts_with("build-fail") {
                let _=self.sender.send(Message::Build(Event::Failed("Not enough disk space while servicing the image. The build workspace was retained.".into())));
                self.consume();
                if driver_preview == "build-failed" {
                    self.error = None;
                }
            }
        }
        if driver_preview.starts_with("complete") {
            let job = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(".cache")
                .join("completion-preview")
                .join(format!("build-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&job).unwrap();
            let iso = job.join("Win10_MacBook_Preview.iso");
            std::fs::write(&iso, b"Debug completion screenshot fixture; not bootable.").unwrap();
            let log = job.join("build.log");
            self.logs = vec![
                "Debug completion-page fixture".into(),
                "ISO publication completed; build logs retained.".into(),
            ];
            std::fs::write(&log, self.logs.join("\n")).unwrap();
            self.page = Page::Build;
            self.build_progress.apply(&Event::OverallProgress {
                completed: 11,
                total: 12,
            });
            let _ = self
                .sender
                .send(Message::Build(Event::BuildFiles(BuildFiles { job, log })));
            let _ = self.sender.send(Message::Build(Event::Finished(iso)));
            self.consume();
            self.close_ready = true;
            if driver_preview.starts_with("complete-usb-") || driver_preview == "complete-2017-usb"
            {
                let status = match driver_preview.as_str() {
                    "complete-usb-preparing" => complete::UsbPreparationStatus::Preparing,
                    "complete-usb-failed" => complete::UsbPreparationStatus::Failed,
                    _ => complete::UsbPreparationStatus::Prepared,
                };
                self.preparing_usb = status == complete::UsbPreparationStatus::Preparing;
                self.usb_logging = Some(complete::LoggingUsb {
                    path: PathBuf::from(
                        r"\\?\D:\Selected USB preview\Windows setup support\Long folder name to check wrapping on smaller windows",
                    ),
                    status,
                });
            }
        }
        self.confirm_dismount = view == "dismount";
        if driver_preview == "workspace-error" {
            self.show_build_error(
                "WorkingSpace needs at least 55.4 GB free".into(),
                BuildRetry::Review,
            );
        } else if driver_preview == "worker-error" {
            self.show_error("Lost connection to the build worker. Check the retained build workspace before retrying: A non-blocking socket operation could not be completed immediately. (os error 10035)");
        } else if driver_preview == "generic-error" {
            self.show_error(
                "The selected ISO could not be read. Choose an existing Windows ISO and try again.",
            );
        }
    }
}
