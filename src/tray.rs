use std::sync::{Arc, Mutex};

use tao::event_loop::EventLoopProxy;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIconBuilder};

use crate::config::Config;
use crate::state::{TrayAction, SETTING_LABELS};

const COMPRESSION_MODES: &[(&str, &str)] =
    &[("auto", "Auto"), ("lz4", "LZ4"), ("disabled", "Disabled")];

const LOG_LEVELS: &[(&str, &str)] = &[
    ("error", "Error"),
    ("warn", "Warn"),
    ("info", "Info"),
    ("debug", "Debug"),
    ("trace", "Trace"),
];

fn load_icon() -> Icon {
    let bytes = include_bytes!("../assets/logo.png");
    let img = image::load_from_memory(bytes).expect("Failed to decode embedded logo.png");
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Icon::from_rgba(rgba.into_raw(), w, h).expect("Failed to create tray icon")
}

pub fn build_menu(
    proxy: EventLoopProxy<TrayAction>,
    config: &Arc<Mutex<Config>>,
) -> Menu {
    let cfg = config.lock().unwrap();
    let use_tls = cfg.use_tls;
    let fec_enabled = cfg.fec_enabled;
    let current_compression = cfg.compression_mode.clone();
    let current_log_level = cfg.log_level.clone();
    let paired = cfg.paired;
    let device_name = cfg.device_id.as_ref().map(|_| "Paired".to_string());
    drop(cfg);

    let menu = Menu::new();

    let pause_item = MenuItem::new("Pause", true, None);
    let status_item = MenuItem::new("Status", true, None);
    let upload_item = MenuItem::new("Upload File...", true, None);
    let paste_item = MenuItem::new("Upload Paste...", true, None);

    if !paired {
        upload_item.set_enabled(false);
        paste_item.set_enabled(false);
    }

    let pair_label = match (&paired, &device_name) {
        (true, _) => "\u{2611} Paired",
        (false, _) => "\u{2612} Pair Device...",
    };
    let pair_item = MenuItem::new(pair_label, true, None);

    let quit_item = MenuItem::new("Quit", true, None);

    let settings_menu = Submenu::new("Settings", true);
    let tls_item = CheckMenuItem::new("Use TLS", true, use_tls, None);
    let fec_item = CheckMenuItem::new("Use FEC", true, fec_enabled, None);

    let compression_menu = Submenu::new("Compression Mode", true);
    let mut compression_ids = Vec::new();
    for (mode, label) in COMPRESSION_MODES {
        let item = CheckMenuItem::new(*label, true, current_compression == *mode, None);
        compression_ids.push((item.id().clone(), mode.to_string()));
        compression_menu.append(&item).unwrap();
    }

    let log_level_menu = Submenu::new("Log Level", true);
    let mut log_level_ids = Vec::new();
    for (level, label) in LOG_LEVELS {
        let item = CheckMenuItem::new(*label, true, current_log_level == *level, None);
        log_level_ids.push((item.id().clone(), level.to_string()));
        log_level_menu.append(&item).unwrap();
    }

    let mut setting_ids = Vec::new();
    for (key, label) in SETTING_LABELS {
        if *key == "log_level" {
            continue;
        }
        let item = MenuItem::new(*label, true, None);
        setting_ids.push((item.id().clone(), key.to_string()));
        settings_menu.append(&item).unwrap();
    }

    settings_menu.append(&PredefinedMenuItem::separator()).unwrap();
    settings_menu.append(&tls_item).unwrap();
    settings_menu.append(&fec_item).unwrap();
    settings_menu.append(&compression_menu).unwrap();
    settings_menu.append(&log_level_menu).unwrap();

    let pause_id = pause_item.id().clone();
    let status_id = status_item.id().clone();
    let upload_id = upload_item.id().clone();
    let paste_id = paste_item.id().clone();
    let pair_id = pair_item.id().clone();
    let quit_id = quit_item.id().clone();
    let tls_id = tls_item.id().clone();
    let fec_id = fec_item.id().clone();

    menu.append(&pause_item).unwrap();
    menu.append(&status_item).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&upload_item).unwrap();
    menu.append(&paste_item).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&pair_item).unwrap();
    menu.append(&settings_menu).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&quit_item).unwrap();

    let p1 = proxy.clone();
    let p2 = proxy.clone();
    let p3 = proxy.clone();
    let p4 = proxy.clone();
    let p5 = proxy.clone();
    let p6 = proxy.clone();
    let p7 = proxy.clone();
    let p8 = proxy.clone();
    let p9 = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == pause_id {
            let _ = p1.send_event(TrayAction::PauseResume);
        } else if event.id == status_id {
            let _ = p2.send_event(TrayAction::ShowStatus);
        } else if event.id == upload_id {
            let _ = p3.send_event(TrayAction::UploadFile);
        } else if event.id == paste_id {
            let _ = p4.send_event(TrayAction::PasteUpload);
        } else if event.id == pair_id {
            let _ = p5.send_event(TrayAction::PairDevice);
        } else if event.id == quit_id {
            let _ = p6.send_event(TrayAction::Quit);
        } else if event.id == tls_id {
            let _ = p7.send_event(TrayAction::ToggleTls);
        } else if event.id == fec_id {
            let _ = p8.send_event(TrayAction::ToggleFec);
        } else if let Some((_, mode)) = compression_ids.iter().find(|(id, _)| *id == event.id) {
            let _ = p7.send_event(TrayAction::SetCompressionMode(mode.clone()));
        } else if let Some((_, level)) = log_level_ids.iter().find(|(id, _)| *id == event.id) {
            let _ = p9.send_event(TrayAction::SetLogLevel(level.clone()));
        } else if let Some((_, key)) = setting_ids.iter().find(|(id, _)| *id == event.id) {
            let _ = p1.send_event(TrayAction::Setting(key.clone()));
        }
    }));

    menu
}

pub fn create_tray_icon(
    proxy: EventLoopProxy<TrayAction>,
    config: &Arc<Mutex<Config>>,
) -> tray_icon::TrayIcon {
    let icon = load_icon();
    let cfg = config.lock().unwrap();
    let paired = cfg.paired;
    drop(cfg);

    let menu = build_menu(proxy.clone(), config);

    let tooltip = if paired {
        "juicebox-plus \u{2014} Paired"
    } else {
        "juicebox-plus \u{2014} Not Paired"
    };

    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip)
        .with_icon(icon)
        .build()
        .expect("Failed to build tray icon")
}
