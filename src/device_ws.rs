use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::config::Config;
use crate::state::TrayAction;

type WsWriter = futures_util::stream::SplitSink<
    WebSocketStream<MaybeTlsStream<TcpStream>>,
    tokio_tungstenite::tungstenite::Message,
>;

pub struct DeviceWsHandle {
    #[allow(dead_code)]
    pub shutdown: mpsc::Sender<()>,
}

pub fn spawn_device_ws(
    config: Arc<Mutex<Config>>,
    proxy: tao::event_loop::EventLoopProxy<TrayAction>,
) -> DeviceWsHandle {
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut backoff_secs: u64 = 0;

            loop {
                let (instance, token) = {
                    let cfg = config.lock().unwrap();
                    if !cfg.paired || cfg.device_id.is_none() {
                        return;
                    }
                    (cfg.juicebox_instance.clone(), crate::token::load_token())
                };

                let token = match token {
                    Some(t) => t,
                    None => {
                        tracing::warn!("No device token found, cannot connect");
                        return;
                    }
                };

                let ws_url = build_ws_url(&instance, &token);
                tracing::info!("Connecting to device WebSocket: {}", ws_url);

                let connect_result = tokio_tungstenite::connect_async(&ws_url).await;

                match connect_result {
                    Ok((ws_stream, _response)) => {
                        backoff_secs = 0;
                        tracing::info!("Device WebSocket connected");
                        let _ = proxy.send_event(TrayAction::DeviceConnected);

                        let (mut write, mut read) = ws_stream.split();
                        let mut heartbeat_interval = tokio::time::interval_at(
                            tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                            std::time::Duration::from_secs(30),
                        );
                        let mut auth_ok = false;

                        loop {
                            tokio::select! {
                                _ = shutdown_rx.recv() => {
                                    tracing::info!("Device WS shutting down");
                                    return;
                                }
                                _ = heartbeat_interval.tick() => {
                                    if !auth_ok { break; }
                                    let msg = serde_json::json!({"type": "heartbeat"});
                                    if write.send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string().into())).await.is_err() {
                                        tracing::warn!("Failed to send heartbeat");
                                        break;
                                    }
                                }
                                msg = read.next() => {
                                    match msg {
                                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                                                if val.get("type").and_then(|t| t.as_str()) == Some("auth_ok") {
                                                    auth_ok = true;
                                                }
                                                handle_message(&val, &proxy, &instance, &mut write).await;
                                            }
                                        }
                                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => {
                                            tracing::warn!("Device WebSocket closed");
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }

                        // WS connected but auth_ok never arrived. The server accepted
                        // the upgrade (token was valid at upgrade time), so this is a
                        // server-side hiccup and not a pair desync so just reconnect
                        if !auth_ok {
                            tracing::warn!("Device not authenticated within timeout, will reconnect");
                        }

                        let _ = proxy.send_event(TrayAction::DeviceDisconnected);
                    }
                    Err(e) => {
                        tracing::error!("Device WebSocket connection failed: {e}");
                        let _ = proxy.send_event(TrayAction::DeviceDisconnected);

                        // Fast path: classify the tungstenite error directly.
                        // A 401/403 HTTP response means the server explicitly
                        // token was rejected which means the device is unpaired
                        if let Some(true) = classify_ws_error(&e) {
                            tracing::warn!(
                                "Server rejected authentication (401/403), marking as unpaired"
                            );
                            {
                                let mut cfg = config.lock().unwrap();
                                cfg.paired = false;
                                let _ = cfg.save();
                            }
                            return;
                        }

                        // Slow path: error is ambiguous (network error, TLS, etc.).
                        // Perform an explicit handshake to verify pairing status.
                        tracing::info!("Performing pairing handshake to check device status...");
                        match check_device_paired(&instance, &token).await {
                            Some(false) => {
                                tracing::warn!("Handshake confirmed device is unpaired");
                                {
                                    let mut cfg = config.lock().unwrap();
                                    cfg.paired = false;
                                    let _ = cfg.save();
                                }
                                return;
                            }
                            Some(true) => {
                                tracing::info!(
                                    "Handshake confirmed device is still paired, will retry"
                                );
                            }
                            None => {
                                tracing::warn!("Server unreachable during handshake, will retry");
                            }
                        }

                        // Exponential backoff: 5s, 10s, 20s, 40s, capped at 60s
                        backoff_secs = (backoff_secs.max(5) * 2).min(60);
                    }
                }

                // Wait before reconnecting, but check shutdown
                tracing::info!("Reconnecting in {backoff_secs}s...");
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Device WS shutting down during reconnect");
                        return;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)) => {}
                }
            }
        });
    });

    DeviceWsHandle {
        shutdown: shutdown_tx,
    }
}

async fn handle_message(
    msg: &serde_json::Value,
    proxy: &tao::event_loop::EventLoopProxy<TrayAction>,
    juiceback_instance: &str,
    write: &mut WsWriter,
) -> bool {
    let msg_type = msg.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match msg_type {
        "auth_ok" => {
            tracing::info!("Device authenticated");
        }
        "heartbeat_ack" => {
            tracing::trace!("Heartbeat acknowledged");
        }
        "ping" => {
            tracing::info!("Ping received, responding");
            let pong = serde_json::json!({"type": "ping_response"});
            if let Err(e) = write
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    pong.to_string().into(),
                ))
                .await
            {
                tracing::warn!("Failed to send ping_response: {e}");
            }
        }
        "upload_request" => {
            let file_id = msg.get("file_id").and_then(|v| v.as_str()).unwrap_or("");
            let filename = msg
                .get("filename")
                .and_then(|v| v.as_str())
                .unwrap_or("upload");
            let file_size = msg.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0);
            let ticket = msg.get("ticket").and_then(|v| v.as_str()).unwrap_or("");
            let juicest_url = msg
                .get("juicehost_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            tracing::info!("Upload request received: file_id={file_id} filename={filename}");

            // Open file picker to select the local file
            let file_path = match crate::dialogs::open_file_dialog("Select file to upload", filename) {
                Some(path) => path,
                None => {
                    tracing::info!("File selection cancelled by user");
                    let cancel_msg = serde_json::json!({
                        "type": "upload_cancelled",
                        "file_id": file_id,
                    });
                    if let Err(e) = write
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            cancel_msg.to_string().into(),
                        ))
                        .await
                    {
                        tracing::warn!("Failed to send upload_cancelled: {e}");
                    }
                    return true;
                }
            };

            let proxy_clone = proxy.clone();
            let file_id_owned = file_id.to_string();
            // Use the real filename from the selected file, not the placeholder from the WS message.
            let real_name = std::path::Path::new(&file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(filename);
            let filename_owned = real_name.to_string();
            // Guess the MIME from the actual picked file, not the placeholder
            // the browser sent in the reserve request.
            let mime_type_owned = crate::ipc::guess_mime(real_name);
            let ticket_owned = ticket.to_string();
            let juicest_url_owned = juicest_url.to_string();
            let juiceback_url_owned = juiceback_instance.to_string();

            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::error!("Failed to create tokio runtime: {e}");
                        return;
                    }
                };
                rt.block_on(async {
                    let result = crate::ipc::ultrafast_upload(
                        &file_path,
                        &file_id_owned,
                        &filename_owned,
                        &mime_type_owned,
                        file_size,
                        &ticket_owned,
                        &juicest_url_owned,
                        &juiceback_url_owned,
                        proxy_clone.clone(),
                    )
                    .await;

                    match result {
                        Ok(url) => {
                            tracing::info!("UltraFast upload complete: {url}");
                        }
                        Err(e) => {
                            tracing::error!("UltraFast upload failed: {e}");
                        }
                    }
                });
            });
        }
        _ => {
            tracing::debug!("Unknown message type: {msg_type}");
        }
    }
    true
}

/// Figure out if the WS error means we're unpaired or just offline.
/// Some(true) = unpaired, Some(false) = server goofed, None = network ded.
fn classify_ws_error(err: &tokio_tungstenite::tungstenite::Error) -> Option<bool> {
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            let status = response.status().as_u16();
            if status == 401 || status == 403 {
                Some(true)
            } else {
                Some(false)
            }
        }
        _ => None,
    }
}

/// Check with the server if we're still paired. simple as.
async fn check_device_paired(instance: &str, token: &str) -> Option<bool> {
    let url = format!("{}/api/device/status", instance.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    match client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status == 401 || status == 403 {
                Some(false) // unpaired
            } else if status == 200 {
                Some(true) // still paired
            } else {
                // server gave us something unexpected so just assume we're still paired
                Some(true)
            }
        }
        Err(_) => None, // Server unreachable
    }
}

pub(crate) fn build_ws_url(instance: &str, token: &str) -> String {
    let base = instance.trim_end_matches('/');
    let base = if base.starts_with("https") {
        base.replacen("https", "wss", 1)
    } else if base.starts_with("http") {
        base.replacen("http", "ws", 1)
    } else {
        format!("ws://{base}")
    };
    format!("{base}/api/device/ws?token={token}")
}
