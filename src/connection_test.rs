use std::time::{Duration, Instant};

use crate::config::Config;

const CHECK_TIMEOUT: Duration = Duration::from_secs(8);
const QUIC_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// juicehost binds QUIC on public_port + 1 (default 6402 -> 6403).
const DEFAULT_QUIC_PORT: u16 = 6403;
/// Proxied setups sometimes expose QUIC over UDP 443.
const ALT_QUIC_PORT: u16 = 443;

pub struct TestResult {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    pub latency_ms: Option<u64>,
}

impl TestResult {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            ok: false,
            detail: String::new(),
            latency_ms: None,
        }
    }
}

/// Run every connectivity check in parallel, then correlate the transfer
/// results with the active firewall state.
pub async fn run_all(config: &Config) -> Vec<TestResult> {
    let (api, ws, juicehost, quic, daemon) = tokio::join!(
        check_juiceback_api(config),
        check_device_ws(config),
        check_juicehost_http(config),
        check_quic_udp(config),
        check_daemon(config),
    );
    let results = vec![api, ws, juicehost, quic, daemon];
    let mut results = results;
    results.push(check_firewall(&results));
    results
}

/// Correlate transfer checks with the firewall state so we only blame the
/// firewall when an actual transfer probe also failed.
fn check_firewall(results: &[TestResult]) -> TestResult {
    let mut r = TestResult::new("Firewall");
    let info = crate::firewall::detect();

    let transfer_failed = results
        .iter()
        .any(|t| t.name == "Juicehost QUIC (UDP)" && !t.ok)
        || results
            .iter()
            .any(|t| t.name == "Juicehost (HTTP)" && !t.ok);

    if info.active {
        if transfer_failed {
            r.detail = format!(
                "{} - transfer checks failed, may be blocked",
                info.description
            );
        } else {
            r.ok = true;
            r.detail = format!("{} - transfer checks reach server", info.description);
        }
    } else if info.description.is_empty() {
        r.ok = true;
        r.detail = "no active firewall detected".into();
    } else {
        r.ok = true;
        r.detail = info.description;
    }
    r
}

/// Compact multi-line summary, one line per transport.
pub fn format_results(results: &[TestResult]) -> String {
    results
        .iter()
        .map(|r| {
            let icon = if r.ok { "\u{2713}" } else { "\u{2717}" };
            let mut line = format!("{icon} {}", r.name);
            if let Some(ms) = r.latency_ms {
                line.push_str(&format!(" ({ms}ms)"));
            }
            if !r.detail.is_empty() {
                line.push_str(&format!(" - {}", r.detail));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn check_juiceback_api(config: &Config) -> TestResult {
    let mut r = TestResult::new("Juicebox API (HTTP)");
    let Some(instance) = instance(config) else {
        r.detail = "juicebox instance not configured".into();
        return r;
    };

    let url = format!("{instance}/api/config");
    let client = http_client();
    let start = Instant::now();
    match client.get(&url).send().await {
        Ok(resp) => {
            r.latency_ms = Some(start.elapsed().as_millis() as u64);
            if resp.status().is_success() {
                r.ok = true;
            }
            r.detail = format!("HTTP {}", resp.status());
        }
        Err(e) => r.detail = format!("{e}"),
    }
    r
}

async fn check_device_ws(config: &Config) -> TestResult {
    let mut r = TestResult::new("Device WebSocket (WS)");
    let Some(instance) = instance(config) else {
        r.detail = "juicebox instance not configured".into();
        return r;
    };

    let token = crate::token::load_token().unwrap_or_default();
    let url = crate::device_ws::build_ws_url(&instance, &token);
    let start = Instant::now();

    match tokio::time::timeout(CHECK_TIMEOUT, tokio_tungstenite::connect_async(&url)).await {
        Ok(Ok(_)) => {
            r.ok = true;
            r.latency_ms = Some(start.elapsed().as_millis() as u64);
            r.detail = "connected".into();
        }
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(_))) => {
            // Endpoint is alive and processed the upgrade; it just wants a valid token.
            r.ok = true;
            r.latency_ms = Some(start.elapsed().as_millis() as u64);
            r.detail = "endpoint responds (auth required)".into();
        }
        Ok(Err(e)) => r.detail = format!("{e}"),
        Err(_) => r.detail = "timed out".into(),
    }
    r
}

async fn check_juicehost_http(config: &Config) -> TestResult {
    let mut r = TestResult::new("Juicehost (HTTP)");
    let Some(url) = juicehost_url(config).await else {
        r.detail = "juicehost not configured".into();
        return r;
    };

    let client = http_client();
    let start = Instant::now();
    match client.get(&url).send().await {
        Ok(resp) => {
            r.latency_ms = Some(start.elapsed().as_millis() as u64);
            if resp.status().is_success() {
                r.ok = true;
            }
            r.detail = format!("HTTP {}", resp.status());
        }
        Err(e) => r.detail = format!("{e}"),
    }
    r
}

async fn check_quic_udp(config: &Config) -> TestResult {
    let mut r = TestResult::new("Juicehost QUIC (UDP)");
    let Some(host) = juicehost_host(config).await else {
        r.detail = "juicehost not configured".into();
        return r;
    };

    let mut detail = String::from("no QUIC ports responded");
    for port in [DEFAULT_QUIC_PORT, ALT_QUIC_PORT] {
        match probe_quic(&host, port).await {
            QuicOutcome::Connected => {
                r.ok = true;
                r.detail = format!("QUIC handshake ok (udp://{host}:{port})");
                return r;
            }
            QuicOutcome::TlsReached => {
                r.ok = true;
                r.detail = format!("UDP reachable, TLS cert not trusted (udp://{host}:{port})");
                return r;
            }
            QuicOutcome::Closed => {
                detail = format!("UDP port closed (udp://{host}:{port})");
            }
            QuicOutcome::TimedOut => {
                detail = format!("UDP filtered or no QUIC server (udp://{host}:{port})");
            }
            QuicOutcome::Other(msg) => {
                detail = format!("{msg} (udp://{host}:{port})");
            }
        }
    }
    r.detail = detail;
    r
}

async fn check_daemon(config: &Config) -> TestResult {
    let mut r = TestResult::new("Local daemon (IPC WS)");
    let scheme = if config.use_tls { "wss" } else { "ws" };
    let url = format!("{scheme}://{}", config.daemon_address);
    let start = Instant::now();

    match tokio::time::timeout(CHECK_TIMEOUT, tokio_tungstenite::connect_async(&url)).await {
        Ok(Ok(_)) => {
            r.ok = true;
            r.latency_ms = Some(start.elapsed().as_millis() as u64);
            r.detail = "connected".into();
        }
        Ok(Err(e)) => r.detail = format!("{e}"),
        Err(_) => r.detail = "timed out (daemon not running?)".into(),
    }
    r
}

/// Resolve the juicehost HTTP base URL: explicit storage_host wins, otherwise
/// ask juiceback's config endpoint for the advertised juicehost_url.
async fn juicehost_url(config: &Config) -> Option<String> {
    if let Some(host) = config.storage_host.as_ref() {
        return Some(host.trim_end_matches('/').to_string());
    }

    let instance = instance(config)?;
    let client = http_client();
    let resp = client
        .get(format!("{instance}/api/config"))
        .send()
        .await
        .ok()?;
    let cfg: serde_json::Value = resp.json().await.ok()?;
    cfg.get("juicehost_url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string())
}

async fn juicehost_host(config: &Config) -> Option<String> {
    let url = juicehost_url(config).await?;
    let host = url.split("://").nth(1).or_else(|| url.split('/').next())?;
    let host = host.split('/').next()?;
    Some(host.split(':').next().unwrap_or(host).to_string())
}

fn instance(config: &Config) -> Option<String> {
    let instance = config.juicebox_instance.trim_end_matches('/').to_string();
    if instance.is_empty() {
        None
    } else {
        Some(instance)
    }
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(CHECK_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Attempt a real QUIC connection to the juicehost and classify what happened.
async fn probe_quic(host: &str, port: u16) -> QuicOutcome {
    let addr = match tokio::net::lookup_host((host, port)).await {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => return QuicOutcome::Other("DNS lookup failed".into()),
        },
        Err(e) => return QuicOutcome::Other(format!("DNS lookup failed: {e}")),
    };

    let client = match juicepipe::transport::QuicClient::new(
        juicepipe::transport::TransportConfig::default(),
    ) {
        Ok(c) => c,
        Err(e) => return QuicOutcome::Other(format!("endpoint init failed: {e}")),
    };

    match tokio::time::timeout(QUIC_CHECK_TIMEOUT, client.connect(addr, host)).await {
        Ok(Ok(_)) => QuicOutcome::Connected,
        Ok(Err(e)) => classify_quic_error(&e),
        Err(_) => QuicOutcome::TimedOut,
    }
}

fn classify_quic_error(e: &dyn std::fmt::Display) -> QuicOutcome {
    let msg = e.to_string().to_lowercase();
    if msg.contains("certificate")
        || msg.contains("unknownissuer")
        || msg.contains("invalidcertificate")
        || msg.contains("handshake")
        || msg.contains("crypto")
        || msg.contains("tls")
    {
        QuicOutcome::TlsReached
    } else if msg.contains("timed out") || msg.contains("timeout") {
        QuicOutcome::TimedOut
    } else if msg.contains("refused")
        || msg.contains("unreachable")
        || msg.contains("connectionreset")
        || msg.contains("no route")
    {
        QuicOutcome::Closed
    } else {
        QuicOutcome::Other(e.to_string())
    }
}

enum QuicOutcome {
    Connected,
    TlsReached,
    TimedOut,
    Closed,
    Other(String),
}
