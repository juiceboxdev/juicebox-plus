use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use notify_rust::Notification;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};

use crate::config::Config;
use crate::ipc;
use crate::pairing;
use crate::tray;

pub const SETTING_LABELS: &[(&str, &str)] = &[
    ("juicebox_instance", "Juicebox Instance"),
    ("storage_host", "Juicehost Instance"),
    ("upload_ttl_hours", "Retention time"),
    ("daemon_address", "Daemon Address"),
    ("chunk_size_bytes", "Chunk Size (bytes)"),
    ("pairing_timeout_secs", "Pairing Timeout (secs)"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DaemonStatus {
    Disconnected,
    Connecting,
    Connected,
    Uploading {
        job_id: String,
        bytes_completed: u64,
        total_bytes: u64,
    },
    Paused {
        job_id: String,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TrayAction {
    IncomingFile(String),
    PauseResume,
    ShowStatus,
    PairDevice,
    PairingComplete(String),
    ToggleTls,
    ToggleFec,
    SetCompressionMode(String),
    Setting(String),
    SetLogLevel(String),
    DeviceConnected,
    DeviceDisconnected,
    UploadFile,
    PasteUpload,
    Quit,
}

pub struct App {
    status: DaemonStatus,
    tray_icon: Option<tray_icon::TrayIcon>,
    proxy: EventLoopProxy<TrayAction>,
    config: Arc<Mutex<Config>>,
    device_ws_handle: Option<crate::device_ws::DeviceWsHandle>,
}

impl App {
    pub fn new(proxy: EventLoopProxy<TrayAction>, config: Arc<Mutex<Config>>) -> Self {
        Self {
            status: DaemonStatus::Disconnected,
            tray_icon: None,
            proxy,
            config,
            device_ws_handle: None,
        }
    }

    pub fn build_tray(&mut self, _event_loop: &EventLoopWindowTarget<TrayAction>) {
        let proxy = self.proxy.clone();
        let status = self.status.clone();
        self.tray_icon = Some(tray::create_tray_icon(proxy, &self.config, &status));
        tracing::info!("System tray icon created");
    }

    pub fn set_device_ws_handle(&mut self, handle: crate::device_ws::DeviceWsHandle) {
        self.device_ws_handle = Some(handle);
    }

    pub fn handle_event(
        &mut self,
        event: &tao::event::Event<TrayAction>,
        _elwt: &EventLoopWindowTarget<TrayAction>,
    ) {
        if let tao::event::Event::UserEvent(action) = event {
            self.handle_action(action.clone());
        }
    }

    fn handle_action(&mut self, action: TrayAction) {
        match action {
            TrayAction::IncomingFile(path) => {
                tracing::info!("File received from site: {path}");
                self.status = DaemonStatus::Connecting;

                let proxy = self.proxy.clone();
                let config = Arc::clone(&self.config);
                std::thread::spawn(move || {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::error!("Failed to create tokio runtime: {e}");
                            return;
                        }
                    };
                    rt.block_on(async {
                        let cfg = config.lock().unwrap().clone();
                        let _ = ipc::connect_and_upload(&path, proxy, &cfg).await;
                    });
                });
            }
            TrayAction::PauseResume => {
                tracing::info!("Pause/Resume toggled");
                if let DaemonStatus::Uploading { ref job_id, .. } = self.status {
                    let job_id = job_id.clone();
                    self.status = DaemonStatus::Paused { job_id };
                    notify("juicebox-plus", "Upload paused");
                } else if let DaemonStatus::Paused { ref job_id } = self.status {
                    let job_id = job_id.clone();
                    self.status = DaemonStatus::Uploading {
                        job_id,
                        bytes_completed: 0,
                        total_bytes: 0,
                    };
                    notify("juicebox-plus", "Upload resumed");
                }
                self.rebuild_tray();
            }
            TrayAction::ShowStatus => {
                let msg = match &self.status {
                    DaemonStatus::Disconnected => "Daemon: Disconnected".to_string(),
                    DaemonStatus::Connecting => "Daemon: Connecting...".to_string(),
                    DaemonStatus::Connected => "Daemon: Connected, idle".to_string(),
                    DaemonStatus::Uploading {
                        job_id,
                        bytes_completed,
                        total_bytes,
                    } => {
                        let pct = if *total_bytes > 0 {
                            (*bytes_completed as f64 / *total_bytes as f64 * 100.0) as u32
                        } else {
                            0
                        };
                        format!("Uploading {job_id}: {pct}%")
                    }
                    DaemonStatus::Paused { job_id } => {
                        format!("Paused: {job_id}")
                    }
                };

                notify("juicebox-plus", &msg);
            }
            TrayAction::PairDevice => {
                tracing::info!("Pair device requested");
                let config = Arc::clone(&self.config);
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let cfg = config.lock().unwrap().clone();
                    pairing::prompt_and_validate(Arc::new(cfg), Some(proxy));
                });
            }
            TrayAction::PairingComplete(device_name) => {
                tracing::info!("Pairing complete, spawning device WS client");
                self.rebuild_tray();
                let config = Arc::clone(&self.config);
                let proxy = self.proxy.clone();
                let _handle = crate::device_ws::spawn_device_ws(config, proxy);
                self.device_ws_handle = Some(_handle);
                if let Some(ref icon) = self.tray_icon {
                    let _ = icon.set_tooltip(Some(&format!(
                        "juicebox-plus - Paired as '{device_name}'"
                    )));
                }
                notify("juicebox-plus", &format!("Connected as '{device_name}'"));
            }
            TrayAction::ToggleTls => {
                let mut cfg = self.config.lock().unwrap();
                cfg.use_tls = !cfg.use_tls;
                let val = cfg.use_tls;
                let _ = cfg.save();
                drop(cfg);
                tracing::info!("TLS toggled to {val}");
                notify(
                    "juicebox-plus",
                    &format!("TLS {}", if val { "enabled" } else { "disabled" }),
                );
            }
            TrayAction::ToggleFec => {
                let mut cfg = self.config.lock().unwrap();
                cfg.fec_enabled = !cfg.fec_enabled;
                let val = cfg.fec_enabled;
                let _ = cfg.save();
                drop(cfg);
                tracing::info!("FEC toggled to {val}");
                notify(
                    "juicebox-plus",
                    &format!("FEC {}", if val { "enabled" } else { "disabled" }),
                );
            }
            TrayAction::SetCompressionMode(mode) => {
                let mut cfg = self.config.lock().unwrap();
                cfg.compression_mode = mode.clone();
                let _ = cfg.save();
                drop(cfg);
                tracing::info!("Compression mode set to {mode}");
                notify("juicebox-plus", &format!("Compression: {mode}"));
                self.rebuild_tray();
            }
            TrayAction::SetLogLevel(level) => {
                let mut cfg = self.config.lock().unwrap();
                cfg.log_level = level.clone();
                let _ = cfg.save();
                drop(cfg);
                tracing::info!("Log level set to {level}");
                notify("juicebox-plus", &format!("Log level: {level}"));
                self.rebuild_tray();
            }
            TrayAction::Setting(key) => {
                let label = SETTING_LABELS
                    .iter()
                    .find(|(k, _)| *k == key.as_str())
                    .map(|(_, l)| *l)
                    .unwrap_or(&key);

                if key == "upload_ttl_hours" {
                    let config = Arc::clone(&self.config);
                    std::thread::spawn(move || {
                        let rt = match tokio::runtime::Runtime::new() {
                            Ok(rt) => rt,
                            Err(e) => {
                                tracing::error!("Failed to create tokio runtime: {e}");
                                return;
                            }
                        };
                        rt.block_on(async {
                            let instance = config.lock().unwrap().juicebox_instance.clone();
                            let instance = instance.trim_end_matches('/').to_string();
                            let api_url = format!("{instance}/api/config");

                            let mut allowed_ttls: Vec<f64> = Vec::new();
                            let mut fetched = false;

                            if let Ok(resp) = reqwest::get(&api_url).await {
                                if resp.status().is_success() {
                                    if let Ok(cfg) = resp.json::<serde_json::Value>().await {
                                        if let Some(arr) = cfg.get("allowed_ttl_hours").and_then(|v| v.as_array()) {
                                            allowed_ttls = arr.iter().filter_map(|v| v.as_f64()).collect();
                                            fetched = true;
                                        }
                                    }
                                }
                            }

                            if !fetched {
                                allowed_ttls = vec![1.0, 2.0, 6.0, 12.0, 24.0, 48.0, 72.0, 168.0];
                            }

                            let current = config.lock().unwrap().upload_ttl_hours;
                            let current_label = current
                                .map(|v| crate::dialogs::format_duration(v))
                                .unwrap_or_else(|| "default".to_string());

                            let mut options: Vec<String> = allowed_ttls
                                .iter()
                                .map(|h| crate::dialogs::format_duration(*h))
                                .collect();
                            options.push("default".to_string());

                            let selected = crate::dialogs::select_list(
                                "Retention time",
                                "Choose retention time:",
                                &options,
                                Some(&current_label),
                            );

                            if let Some(new_val) = selected {
                                let value = if new_val == "default" {
                                    "default".to_string()
                                } else {
                                    crate::dialogs::parse_duration(&new_val)
                                        .map(|v| v.to_string())
                                        .unwrap_or(new_val)
                                };
                                let current_text = current
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "default".to_string());
                                if value != current_text {
                                    let mut cfg = config.lock().unwrap();
                                    if cfg.update("upload_ttl_hours", &value).is_ok() {
                                        notify("juicebox-plus", &format!("Retention time updated to {value}"));
                                    }
                                }
                            }
                        });
                    });
                } else if key == "storage_host" {
                    let config = Arc::clone(&self.config);
                    std::thread::spawn(move || {
                        let rt = match tokio::runtime::Runtime::new() {
                            Ok(rt) => rt,
                            Err(e) => {
                                tracing::error!("Failed to create tokio runtime: {e}");
                                return;
                            }
                        };
                        rt.block_on(async {
                            let instance = config.lock().unwrap().juicebox_instance.clone();
                            let instance = instance.trim_end_matches('/').to_string();
                            let api_url = format!("{instance}/api/config");

                            let mut default_host = String::new();

                            if let Ok(resp) = reqwest::get(&api_url).await {
                                if resp.status().is_success() {
                                    if let Ok(cfg) = resp.json::<serde_json::Value>().await {
                                        if let Some(url) = cfg.get("juicehost_url").and_then(|v| v.as_str()) {
                                            default_host = url.to_string();
                                        }
                                    }
                                }
                            }

                            let current = config.lock().unwrap().current_value("storage_host");
                            let default_text = if default_host.is_empty() { current.clone() } else { default_host };

                            if let Some(new_val) = crate::dialogs::input_dialog(
                                "Juicehost Instance",
                                &format!("Enter Juicehost URL (leave empty to use server default)\n\nCurrent: {current}\nServer default: {default_text}"),
                                &default_text,
                            ) {
                                if new_val != current {
                                    let mut cfg = config.lock().unwrap();
                                    if cfg.update("storage_host", &new_val).is_ok() {
                                        notify("juicebox-plus", &format!("Juicehost Instance updated"));
                                    }
                                }
                            }
                        });
                    });
                } else {
                    let current = self.config.lock().unwrap().current_value(&key);

                    if let Some(new_val) = crate::dialogs::input_dialog(
                        "juicebox-plus Settings",
                        &format!("{label}\n\nCurrent: {current}"),
                        &current,
                    ) {
                        if new_val != current {
                            match self.config.lock().unwrap().update(&key, &new_val) {
                                Ok(()) => {
                                    notify("juicebox-plus", &format!("{label} updated to {new_val}"))
                                }
                                Err(e) => {
                                    notify("juicebox-plus", &format!("Failed to update {label}: {e}"))
                                }
                            }
                        }
                    }
                }
            }
            TrayAction::UploadFile => {
                tracing::info!("Upload file requested");
                let config = Arc::clone(&self.config);
                if !config.lock().unwrap().paired {
                    notify("juicebox-plus", "Pair a device first to upload files");
                    return;
                }
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let files = crate::dialogs::open_files_dialog(
                        "Select files to upload",
                    );
                    let files = match files {
                        Some(f) if !f.is_empty() => f,
                        _ => {
                            tracing::info!("File selection cancelled");
                            return;
                        }
                    };
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::error!("Failed to create tokio runtime: {e}");
                            return;
                        }
                    };
                    rt.block_on(async {
                        let ttl = config.lock().unwrap().upload_ttl_hours;
                        match ipc::app_upload_many(&files, ttl).await {
                            Ok(urls) => {
                                let all = urls.join("\n");
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    let _ = cb.set_text(all.clone());
                                }
                                let count = urls.len();
                                let msg = if count == 1 {
                                    format!("Upload complete!\nLink copied to clipboard")
                                } else {
                                    format!("{count} files uploaded!\nLinks copied to clipboard")
                                };
                                let _ = proxy.send_event(TrayAction::ShowStatus);
                                notify("juicebox-plus", &msg);
                            }
                            Err(e) => {
                                tracing::error!("Upload failed: {e}");
                                notify("juicebox-plus", &format!("Upload failed: {e}"));
                            }
                        }
                    });
                });
            }
            TrayAction::PasteUpload => {
                tracing::info!("Paste upload requested");
                let config = Arc::clone(&self.config);
                if !config.lock().unwrap().paired {
                    notify("juicebox-plus", "Pair a device first to upload files");
                    return;
                }
                let _proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let mut cb = match arboard::Clipboard::new() {
                        Ok(c) => c,
                        Err(e) => {
                            notify("juicebox-plus", &format!("Clipboard error: {e}"));
                            return;
                        }
                    };

                    let temp_path = match cb.get_image() {
                        Ok(img) => {
                            let path = std::env::temp_dir().join(format!(
                                "juicebox-paste-{}.png",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                            ));
                            let rgba = img.bytes.into_owned();
                            let w = img.width as u32;
                            let h = img.height as u32;
                            if let Some(img_buffer) = image::RgbaImage::from_raw(w, h, rgba) {
                                let _ = img_buffer.save(&path);
                            }
                            path
                        }
                        Err(_) => {
                            match cb.get_text() {
                                Ok(text) => {
                                    let text = text.trim().to_string();
                                    // Check if it's a file:// URI or a direct file path
                                    let path = if let Some(stripped) = text.strip_prefix("file://") {
                                        std::path::PathBuf::from(stripped)
                                    } else {
                                        std::path::PathBuf::from(&text)
                                    };
                                    if path.exists() && path.is_file() {
                                        path
                                    } else {
                                        // Save text as .txt temp file
                                        let path = std::env::temp_dir().join(format!(
                                            "juicebox-paste-{}.txt",
                                            std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis()
                                        ));
                                        let _ = std::fs::write(&path, &text);
                                        path
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Clipboard image+text both failed: {e}");
                                    notify("juicebox-plus", "No image or text found in clipboard. Try copying a file path (Ctrl+C on a file) or an image.");
                                    return;
                                }
                            }
                        }
                    };

                    let path_str = temp_path.to_string_lossy().to_string();
                    let is_temp = temp_path.starts_with(std::env::temp_dir());

                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::error!("Failed to create tokio runtime: {e}");
                            if is_temp { let _ = std::fs::remove_file(&temp_path); }
                            return;
                        }
                    };
                    rt.block_on(async {
                        let ttl = config.lock().unwrap().upload_ttl_hours;
                        match ipc::app_upload(&path_str, ttl).await {
                            Ok(url) => {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    let _ = cb.set_text(url.clone());
                                }
                                notify("juicebox-plus", "Upload complete!\nLink copied to clipboard");
                            }
                            Err(e) => {
                                tracing::error!("Upload failed: {e}");
                                notify("juicebox-plus", &format!("Upload failed: {e}"));
                            }
                        }
                    });

                    if is_temp {
                        let _ = std::fs::remove_file(&temp_path);
                    }
                });
            }
            TrayAction::DeviceConnected => {
                tracing::info!("Device WS connected");
                self.status = DaemonStatus::Connected;
                if let Some(ref icon) = self.tray_icon {
                    let _ = icon.set_tooltip(Some("juicebox-plus - Device connected"));
                }
                notify("juicebox-plus", "Connected to juicebox");
            }
            TrayAction::DeviceDisconnected => {
                tracing::info!("Device WS disconnected");
                self.status = DaemonStatus::Disconnected;
                if let Some(ref icon) = self.tray_icon {
                    let _ = icon.set_tooltip(Some("juicebox-plus - Juicepipe Upload Daemon"));
                }
                // Rebuild tray to disable upload items if unpaired
                let paired = self.config.lock().unwrap().paired;
                if !paired {
                    self.rebuild_tray();
                }
            }
            TrayAction::Quit => {
                tracing::info!("Quit requested");
                if let Some(icon) = self.tray_icon.take() {
                    drop(icon);
                }
                std::process::exit(0);
            }
        }
    }

    fn rebuild_tray(&mut self) {
        let proxy = self.proxy.clone();
        let config = Arc::clone(&self.config);
        let status = self.status.clone();
        let menu = tray::build_menu(proxy, &config, &status);
        if let Some(ref icon) = self.tray_icon {
            icon.set_menu(Some(Box::new(menu)));
        }
    }
}

#[cfg(target_os = "linux")]
pub fn notify(summary: &str, body: &str) {
    let _ = Notification::new()
        .summary(summary)
        .body(body)
        .appname("juicebox-plus")
        .show();
}

#[cfg(target_os = "windows")]
pub fn notify(_summary: &str, body: &str) {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let toast_xml = format!(
        r#"<toast>
            <visual><binding template="ToastGeneric">
                <text>juicebox-plus</text>
                <text>{body}</text>
            </binding></visual>
        </toast>"#
    );
    if let Ok(doc) = XmlDocument::new() {
        if doc.LoadXml(&HSTRING::from(&toast_xml)).is_ok() {
            if let Ok(n) = ToastNotification::CreateToastNotification(&doc) {
                if let Ok(nt) = ToastNotificationManager::CreateToastNotifierWithId(
                    &HSTRING::from("juicebox-plus"),
                ) {
                    let _ = nt.Show(&n);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub fn notify(_summary: &str, body: &str) {
    use mac_notification_sys::{send_notification, Notification};

    let _ = send_notification(
        "juicebox-plus",
        None,
        body,
        Some(Notification::new().asynchronous(true)),
    );
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn notify(_summary: &str, body: &str) {
    tracing::info!("{body}");
}
