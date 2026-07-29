mod config;
mod device_ws;
mod ipc;
mod pairing;
mod progress;
mod state;
mod token;
mod tray;

use std::sync::{Arc, Mutex};

use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tracing_subscriber::EnvFilter;

fn main() {
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
