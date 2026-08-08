#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod connection_test;
mod device_ws;
mod dialogs;
mod file_validation;
mod firewall;
mod ipc;
mod pairing;
mod progress;
mod state;
mod token;
mod tray;

use std::sync::{Arc, Mutex};

use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tracing_subscriber::EnvFilter;

#[cfg(target_os = "windows")]
fn register_aumid() {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::*;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from("juicebox-plus"));

        let mut hkey = HKEY::default();
        let status = RegCreateKeyW(
            HKEY_CURRENT_USER,
            &HSTRING::from("Software\\Classes\\AppUserModelId\\juicebox-plus"),
            &mut hkey,
        );
        if status.is_ok() {
            let wide: Vec<u16> = "Juicebox Plus\0".encode_utf16().collect();
            let bytes: Vec<u8> = wide
                .iter()
                .flat_map(|u| u.to_le_bytes())
                .collect();
            let _ = RegSetValueExW(
                hkey,
                &HSTRING::from("DisplayName"),
                None,
                REG_SZ,
                Some(&bytes),
            );
            let _ = RegCloseKey(hkey);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn register_aumid() {}

fn main() {
    register_aumid();
    #[cfg(target_os = "windows")]
    firewall::ensure_rule();
    let config = config::Config::load();
    let log_level = config.log_level.clone();
    let config = Arc::new(Mutex::new(config));

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level)),
        )
        .init();

    tracing::info!("juicebox-plus starting");

    let config_clone = Arc::clone(&config);
    std::thread::spawn(move || {
        let mut last_modified = config::Config::last_modified();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let current_modified = config::Config::last_modified();
            if current_modified != last_modified {
                last_modified = current_modified;
                let new_config = config::Config::load();
                *config_clone.lock().unwrap() = new_config;
                tracing::info!("Config reloaded");
            }
        }
    });

    let event_loop = EventLoopBuilder::<state::TrayAction>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let mut app = state::App::new(proxy.clone(), config.clone());
    app.build_tray(&event_loop);

    if config.lock().unwrap().paired {
        let handle = device_ws::spawn_device_ws(Arc::clone(&config), proxy.clone());
        app.set_device_ws_handle(handle);
        tracing::info!("Device WS client started (paired)");
    }

    event_loop.run(move |event, elwt, control_flow| {
        *control_flow = ControlFlow::Wait;

        app.handle_event(&event, elwt);
    });
}
