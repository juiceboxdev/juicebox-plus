use futures_util::{SinkExt, StreamExt};
use juicepipe::core::protocol::ipc::{HelloMessage, JobPayload, TelemetryMessage, WelcomeMessage};
use tao::event_loop::EventLoopProxy;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::config::Config;
use crate::progress::ProgressTracker;
use crate::state::TrayAction;

/// Connected IPC client for communicating with the juicepipe daemon.
pub struct IpcClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl IpcClient {
    /// Connect to the daemon.
    pub async fn connect(addr: Option<&str>, use_tls: bool) -> Result<Self, IpcError> {
        let addr = addr.unwrap_or("127.0.0.1:22100");
        let scheme = if use_tls { "wss" } else { "ws" };
        let url = format!("{scheme}://{addr}");

        let (ws_stream, _response) = connect_async(&url)
            .await
            .map_err(|e| IpcError::ConnectionFailed(format!("Failed to connect to daemon: {e}")))?;

        tracing::info!("Connected to daemon at {addr}");

        Ok(Self { ws: ws_stream })
    }

    /// Send HELLO so the daemon knows who we are.
    pub async fn handshake(
        &mut self,
        origin: &str,
        token: &str,
    ) -> Result<WelcomeMessage, IpcError> {
        let hello = HelloMessage::new(origin, token, &generate_nonce());
        let hello_json = serde_json::to_value(&hello)
            .map_err(|e| IpcError::ProtocolError(format!("Serialization failed: {e}")))?;

        self.send_json(&hello_json).await?;

        let response = self.receive_json().await?;
        let welcome: WelcomeMessage = serde_json::from_value(response)
            .map_err(|e| IpcError::ProtocolError(format!("Invalid welcome response: {e}")))?;

        Ok(welcome)
    }

    /// Tell the daemon to start an upload.
    pub async fn start_job(&mut self, payload: &JobPayload) -> Result<(), IpcError> {
        let msg = serde_json::json!({
            "action": "START_JOB",
            "payload": payload,
        });

        self.send_json(&msg).await
    }

    /// Pull telemetry from the daemon.
    pub async fn receive_telemetry(&mut self) -> Result<TelemetryMessage, IpcError> {
        let data = self.receive_json().await?;
        let telemetry: TelemetryMessage = serde_json::from_value(data)
            .map_err(|e| IpcError::ProtocolError(format!("Invalid telemetry: {e}")))?;
        Ok(telemetry)
    }

    async fn send_json(&mut self, value: &serde_json::Value) -> Result<(), IpcError> {
        let text = serde_json::to_string(value)
            .map_err(|e| IpcError::ProtocolError(format!("Serialization failed: {e}")))?;

        self.ws
            .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
            .await
            .map_err(|e| IpcError::ProtocolError(format!("Send failed: {e}")))?;

        Ok(())
    }

    async fn receive_json(&mut self) -> Result<serde_json::Value, IpcError> {
        let msg = self
            .ws
            .next()
            .await
            .ok_or(IpcError::ConnectionClosed)?
            .map_err(|e| IpcError::ProtocolError(format!("Receive failed: {e}")))?;

        match msg {
            tokio_tungstenite::tungstenite::Message::Text(text) => serde_json::from_str(&text)
                .map_err(|e| IpcError::ProtocolError(format!("JSON parse failed: {e}"))),
            tokio_tungstenite::tungstenite::Message::Close(_) => Err(IpcError::ConnectionClosed),
            _ => Err(IpcError::ProtocolError(
                "Unexpected message type".to_string(),
            )),
        }
    }
}

/// Connect and upload. two birds one stone.
pub async fn connect_and_upload(
    file_path: &str,
    _proxy: EventLoopProxy<TrayAction>,
    config: &Config,
) -> Result<(), IpcError> {
    let mut client = IpcClient::connect(Some(&config.daemon_address), config.use_tls).await?;

    let token = crate::token::load_token().unwrap_or_default();
    let welcome = client.handshake("juicebox-plus", &token).await?;
    tracing::info!("Daemon: {} ({})", welcome.daemon_version, welcome.status);

    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(|e| IpcError::ProtocolError(format!("Failed to stat file: {e}")))?;

    let payload = JobPayload {
        job_id: format!("jp_{}", uuid_simple()),
        file_path: file_path.to_string(),
        file_size_bytes: metadata.len(),
        chunk_size_bytes_hint: config.chunk_size_bytes,
        candidate_endpoints: vec![config.juicebox_instance.clone()],
        auth_bearer: token,
        fec_mode: if config.fec_enabled {
            "adaptive"
        } else {
            "disabled"
        }
        .to_string(),
        compression_policy: config.compression_mode.clone(),
    };

    let file_name = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload");
    let total_bytes = metadata.len();

    let mut progress = ProgressTracker::start(&format!("Uploading {file_name}..."), total_bytes);

    tracing::info!("Starting upload ({} bytes)", total_bytes);
    client.start_job(&payload).await?;

    loop {
        match client.receive_telemetry().await {
            Ok(telemetry) => {
                let pct = if telemetry.total_bytes > 0 {
                    telemetry.bytes_completed as f64 / telemetry.total_bytes as f64 * 100.0
                } else {
                    0.0
                };
                // COMPLETE FUCKING BULLSHIT this spams the logs.
                tracing::info!(
                    "Progress: {}/{} bytes ({pct:.1}%)",
                    telemetry.bytes_completed,
                    telemetry.total_bytes,
                );

                progress.update(telemetry.bytes_completed);

                if telemetry.bytes_completed >= telemetry.total_bytes && telemetry.total_bytes > 0 {
                    let mb = total_bytes / 1_048_576;
                    progress.finish(&format!("Upload complete: {file_name} ({mb} MB)"));
                    return Ok(());
                }
            }
            Err(IpcError::ConnectionClosed) => {
                tracing::warn!("Daemon closed connection during upload");
                progress.finish("Upload interrupted - connection lost");
                return Err(IpcError::ConnectionClosed);
            }
            Err(e) => {
                tracing::error!("Telemetry error: {e}");
                progress.finish(&format!("Upload failed: {e}"));
                return Err(e);
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Connection closed")]
    ConnectionClosed,
    #[error("Protocol error: {0}")]
    ProtocolError(String),
}

/// Generate a short random nonce for the HELLO handshake.
fn generate_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{t:x}")
}

/// Generate a unique-ish job ID. not a real uuid, don't @ me.
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{:08x}-{:04x}-{:04x}",
        t.as_secs(),
        t.subsec_millis(),
        rand_u16()
    )
}

fn rand_u16() -> u16 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut h = s.build_hasher();
    h.write_u64(0xDEAD_BEEF);
    h.finish() as u16
}

/// Push a file straight to juicehost with a JWT ticket, then tell juiceback we're done.
/// throws up a progress dialog so you know it's working.
pub async fn ultrafast_upload(
    file_path: &str,
    file_id: &str,
    _filename: &str,
    _mime_type: &str,
    expected_size: u64,
    ticket: &str,
    juicehost_url: &str,
    juiceback_url: &str,
    _proxy: EventLoopProxy<TrayAction>,
) -> Result<String, IpcError> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/internal/file/upload/{}",
        juicehost_url.trim_end_matches('/'),
        file_id
    );

    let metadata = tokio::fs::metadata(file_path)
        .await
        .map_err(|e| IpcError::ProtocolError(format!("Failed to stat file: {e}")))?;

    if expected_size > 0 && metadata.len() != expected_size {
        tracing::warn!(
            "File size mismatch: expected {expected_size}, got {}",
            metadata.len()
        );
    }

    let total_bytes = metadata.len();
    let display_name = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(_filename);

    let mut progress = ProgressTracker::start(&format!("Uploading {display_name}..."), total_bytes);

    // Pre-flight: check file size against juicehost's limit.
    let config_url = format!("{}/api/config", juicehost_url.trim_end_matches('/'));
    if let Ok(resp) = client.get(&config_url).send().await {
        if let Ok(cfg) = resp.json::<serde_json::Value>().await {
            if let Some(max_size) = cfg.get("max_file_size_bytes").and_then(|v| v.as_u64()) {
                if total_bytes > max_size {
                    let file_mb = total_bytes / 1_048_576;
                    let max_mb = max_size / 1_048_576;
                    progress.finish(&format!(
                        "File too large: {file_mb} MB exceeds server limit of {max_mb} MB"
                    ));
                    return Err(IpcError::ConnectionFailed(format!(
                        "File too large ({file_mb} MB). Server limit is {max_mb} MB. \
                         Increase MAX_FILE_SIZE_MB on your juicehost instance."
                    )));
                }
            }
        }
    }

    tracing::info!("Uploading to juicehost: {url} ({total_bytes} bytes)");

    // Build a streaming body that reads the file in chunks and reports progress.
    type StreamError = Box<dyn std::error::Error + Send + Sync>;
    const CHUNK_SIZE: usize = 64 * 1024; // 64 KB chunks
    let file = tokio::fs::File::open(file_path)
        .await
        .map_err(|e| IpcError::ProtocolError(format!("Failed to open file: {e}")))?;

    let progress = std::sync::Arc::new(std::sync::Mutex::new(progress));

    let progress_clone = progress.clone();
    let stream = futures_util::stream::unfold(
        (file, 0u64, total_bytes, progress_clone),
        |(mut file, sent, total, prog)| async move {
            if sent >= total {
                return None;
            }
            let to_read = CHUNK_SIZE.min((total - sent) as usize);
            let mut buf = vec![0u8; to_read];
            use tokio::io::AsyncReadExt;
            match file.read_exact(&mut buf).await {
                Ok(_) => {
                    let new_sent = sent + buf.len() as u64;
                    if let Ok(mut p) = prog.lock() {
                        p.update(new_sent);
                    }
                    Some((
                        Ok::<_, StreamError>(bytes::Bytes::from(buf)),
                        (file, new_sent, total, prog),
                    ))
                }
                Err(e) => Some((Err(e.into()), (file, sent, total, prog))),
            }
        },
    );

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {ticket}"))
        .header("Content-Type", _mime_type)
        .header("X-Juicebox-File-Name", _filename)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .map_err(|e| IpcError::ConnectionFailed(format!("Upload request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        let user_msg = if status.as_u16() == 413 {
            let file_mb = total_bytes / 1_048_576;
            format!("File too large ({file_mb} MB). Increase MAX_FILE_SIZE_MB on your juicehost instance.")
        } else {
            format!("Upload failed: HTTP {status}")
        };
        if let Ok(mut p) = progress.lock() {
            p.finish(&user_msg);
        }
        return Err(IpcError::ConnectionFailed(format!(
            "{user_msg} Server response: {text}"
        )));
    }

    tracing::info!("Upload successful to juicehost");
    let mb = total_bytes / 1_048_576;
    if let Ok(mut p) = progress.lock() {
        p.finish(&format!("Upload complete: {display_name} ({mb} MB)"));
    }

    // Mark file as complete on juiceback
    let complete_url = format!(
        "{}/upload/ultrafast/complete",
        juiceback_url.trim_end_matches('/')
    );
    let complete_body = serde_json::json!({
        "file_id": file_id,
        "filename": _filename,
        "mime_type": _mime_type,
        "file_size": total_bytes,
        "ticket": ticket,
    });

    // use the device JWT for auth because the ticket JWT goes in the body
    let device_token = crate::token::load_token().unwrap_or_default();

    let complete_response = client
        .post(&complete_url)
        .header("Authorization", format!("Bearer {device_token}"))
        .json(&complete_body)
        .send()
        .await
        .map_err(|e| {
            tracing::warn!("Failed to notify juiceback of completion: {e}");
            IpcError::ConnectionFailed(format!(
                "Upload succeeded but failed to notify juiceback: {e}"
            ))
        })?;

    let complete_status = complete_response.status();
    if !complete_status.is_success() {
        let text = complete_response.text().await.unwrap_or_default();
        tracing::warn!("juiceback complete returned HTTP {complete_status}: {text}");
        return Err(IpcError::ConnectionFailed(format!(
            "Upload succeeded but juiceback refused to mark it complete (HTTP {complete_status}): {text}"
        )));
    }

    tracing::info!("Marked file as complete on juiceback");

    Ok(String::new())
}
