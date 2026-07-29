use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use notify_rust::Notification;
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};

use crate::config::Config;
use crate::ipc;
use crate::pairing;
use crate::tray;

pub const SETTING_LABELS: &[(&str, &str)] = &[
    ("daemon_address", "Daemon Address"),
    ("juicebox_instance", "Juicebox Instance"),
    ("chunk_size_bytes", "Chunk Size (bytes)"),
    ("pairing_timeout_secs", "Pairing Timeout (secs)"),
    ("log_level", "Log Level"),
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
    DeviceConnected,
    DeviceDisconnected,
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
        self.tray_icon = Some(tray::create_tray_icon(proxy, &self.config));
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
                let config = Arc::clone(&self.config);
                let proxy = self.proxy.clone();
                let _handle = crate::device_ws::spawn_device_ws(config, proxy);
                self.device_ws_handle = Some(_handle);
                if let Some(ref icon) = self.tray_icon {
                    let _ = icon.set_tooltip(Some(&format!(
                        "juicebox-plus \u{2014} Paired as '{device_name}'"
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
            }
            TrayAction::Setting(key) => {
                let label = SETTING_LABELS
                    .iter()
                    .find(|(k, _)| *k == key.as_str())
                    .map(|(_, l)| *l)
                    .unwrap_or(&key);

                let current = self.config.lock().unwrap().current_value(&key);

                if let Some(new_val) = tinyfiledialogs::input_box(
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
}

#[cfg(target_os = "linux")]
pub fn notify(summary: &str, body: &str) {
    let _ = Notification::new()
        .summary(summary)
        .body(body)
        .appname("juicebox-plus")
        .show();
}

#[cfg(not(target_os = "linux"))]
pub fn notify(_summary: &str, body: &str) {
    tracing::info!("{body}");
}
