//! Firewall inspection on all platforms plus outbound-rule management on
//! Windows (the app runs as a standard user there, so adding a rule needs a
//! UAC-elevated `netsh`).
//!
//! `detect()` reports whether a firewall that could restrict the transfer is
//! active; callers correlate that with failed transfer probes before blaming
//! the firewall, to avoid false positives.

use std::process::Command;

#[cfg(target_os = "windows")]
use windows::core::{w, PCWSTR};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HINSTANCE;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::ShellExecuteW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// What the firewall state probe found.
#[derive(Debug, Default)]
pub struct FirewallInfo {
    /// True when a firewall is active that could block outbound transfers.
    pub active: bool,
    /// Human-readable summary of what was detected.
    pub description: String,
}

/// Probe the platform firewall state. Never requires elevation; commands that
/// need root are simply not detected.
pub fn detect() -> FirewallInfo {
    #[cfg(target_os = "windows")]
    let info = detect_windows();
    #[cfg(target_os = "linux")]
    let info = detect_linux();
    #[cfg(target_os = "macos")]
    let info = detect_macos();
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    let info = FirewallInfo::default();
    info
}

/// Hint to append to a failed-transfer message. Only fires for network-level
/// failures while a restrictive firewall is active.
pub fn transfer_block_hint(is_network_error: bool) -> Option<String> {
    if !is_network_error {
        return None;
    }
    let info = detect();
    if info.active {
        Some(format!(
            "Possible firewall block: {} - transfer may be blocked",
            info.description
        ))
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn detect_linux() -> FirewallInfo {
    let mut info = FirewallInfo::default();
    let mut parts: Vec<&str> = Vec::new();

    if let Ok(out) = run_cmd("ufw", &["status"]) {
        if out.contains("Status: active") {
            info.active = true;
            parts.push("ufw active");
        }
    }

    if let Ok(out) = run_cmd("firewall-cmd", &["--state"]) {
        if out.to_lowercase().contains("running") {
            info.active = true;
            parts.push("firewalld running");
        }
    }

    if let Ok(out) = run_cmd("nft", &["list", "ruleset"]) {
        if out.contains("policy drop") {
            info.active = true;
            parts.push("nftables drop policy");
        }
    }

    if !parts.is_empty() {
        info.description = parts.join(", ");
    }
    info
}

#[cfg(target_os = "macos")]
fn detect_macos() -> FirewallInfo {
    let mut info = FirewallInfo::default();
    let mut parts: Vec<&str> = Vec::new();

    if let Ok(out) = run_cmd(
        "/usr/libexec/ApplicationFirewall/socketfilterfw",
        &["--getglobalstate"],
    ) {
        if out.contains("State = 1") {
            info.active = true;
            parts.push("Application Firewall enabled");
        }
    }

    if let Ok(out) = run_cmd("pfctl", &["-s", "info"]) {
        if out.contains("Status: Enabled") {
            info.active = true;
            parts.push("pf enabled");
        }
    }

    if !parts.is_empty() {
        info.description = parts.join(", ");
    }
    info
}

#[cfg(target_os = "windows")]
fn detect_windows() -> FirewallInfo {
    let mut info = FirewallInfo::default();
    if rule_exists() {
        info.description = "Windows Firewall rule present".into();
    } else {
        info.active = true;
        info.description = "Windows Firewall: no juicebox-plus rule".into();
    }
    info
}

/// Run a command and return combined stdout+stderr on success.
#[cfg(not(target_os = "windows"))]
fn run_cmd(program: &str, args: &[&str]) -> Result<String, ()> {
    let out = Command::new(program).args(args).output().map_err(|_| ())?;
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

#[cfg(target_os = "windows")]
pub const RULE_NAME: &str = "Juicebox Plus";

/// Build the netsh add-rule command line. The program-based outbound allow
/// covers both TCP (HTTP uploads to juiceback) and UDP (QUIC transfers to
/// juicehost) on every profile.
#[cfg(target_os = "windows")]
fn netsh_add_args(exe: &str) -> String {
    format!(
        "advfirewall firewall add rule name=\"{RULE_NAME}\" dir=out program=\"{exe}\" action=allow profile=any"
    )
}

/// Query the firewall without elevation. Returns true when a rule matching
/// our name exists. The rule name is data, so it shows up in the listing
/// regardless of the system locale.
#[cfg(target_os = "windows")]
pub fn rule_exists() -> bool {
    let Ok(out) = Command::new("netsh")
        .arg("advfirewall")
        .arg("firewall")
        .arg("show")
        .arg("rule")
        .arg(format!("name=\"{RULE_NAME}\""))
        .output()
    else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    text.contains(RULE_NAME) && !text.contains("No rules match")
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
pub enum AddOutcome {
    AlreadyPresent,
    Added,
    Failed(String),
}

/// Add the outbound firewall rule, prompting for elevation (UAC) since the
/// app normally runs as a standard user.
#[cfg(target_os = "windows")]
pub fn add_rule() -> AddOutcome {
    if rule_exists() {
        return AddOutcome::AlreadyPresent;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(e) => return AddOutcome::Failed(format!("cannot resolve exe path: {e}")),
    };

    let params = netsh_add_args(&exe);
    let (wide, pcwstr) = to_pcwstr(&params);
    let _ = wide;

    let result: HINSTANCE =
        unsafe { ShellExecuteW(None, w!("runas"), w!("netsh"), pcwstr, None, SW_HIDE) };
    if result.0 as isize > 32 {
        AddOutcome::Added
    } else {
        AddOutcome::Failed(format!("ShellExecuteW returned {}", result.0 as isize))
    }
}

/// Attempt to add the rule at startup; log the outcome without blocking the
/// event loop (ShellExecuteW does not wait for the elevated process).
#[cfg(target_os = "windows")]
pub fn ensure_rule() {
    match add_rule() {
        AddOutcome::AlreadyPresent => tracing::debug!("Windows firewall rule already present"),
        AddOutcome::Added => tracing::info!("Windows firewall rule requested via UAC"),
        AddOutcome::Failed(e) => tracing::warn!("Could not add Windows firewall rule: {e}"),
    }
}

/// Borrow a null-terminated UTF-16 copy of `s` as a PCWSTR. The returned
/// Vec must outlive the PCWSTR.
#[cfg(target_os = "windows")]
fn to_pcwstr(s: &str) -> (Vec<u16>, PCWSTR) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    wide.push(0);
    let ptr = PCWSTR(wide.as_ptr());
    (wide, ptr)
}
