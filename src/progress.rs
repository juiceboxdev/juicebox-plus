#[cfg(target_os = "linux")]
use notify_rust::{Hint, Notification, NotificationHandle};

/// Progress tracker that shoves a progress bar into desktop notifications.
/// tested on plasma, gnome; if your DE ignores it (e.g, my hyprland config (end4) ), rip.
pub struct ProgressTracker {
    #[cfg(target_os = "linux")]
    handle: Option<NotificationHandle>,
    total: u64,
    last_pct: u32,
}

impl ProgressTracker {
    /// Start a new progress notification.
    pub fn start(title: &str, total: u64) -> Self {
        #[cfg(target_os = "linux")]
        let handle = Notification::new()
            .appname("juicebox-plus")
            .summary("juicebox-plus")
            .body(title)
            .hint(Hint::CustomInt("value".into(), 0))
            .show()
            .ok();

        #[cfg(target_os = "linux")]
        if handle.is_none() {
            tracing::warn!("Desktop notifications is unavailable, progress will be logged only");
        }

        Self {
            #[cfg(target_os = "linux")]
            handle,
            total,
            last_pct: 0,
        }
    }

    /// Update progress. throttled to 1% granularity so we don't ddos the notif server.
    pub fn update(&mut self, bytes_sent: u64) {
        if self.total == 0 {
            return;
        }
        let pct = ((bytes_sent as f64 / self.total as f64) * 100.0).min(100.0) as u32;
        if pct == self.last_pct {
            return;
        }
        self.last_pct = pct;

        #[cfg(target_os = "linux")]
        if let Some(ref mut handle) = self.handle {
            let mb_done = bytes_sent / 1_048_576;
            let mb_total = self.total / 1_048_576;

            handle
                .body(&format!("{mb_done} MB / {mb_total} MB ({pct}%)"))
                .hint(Hint::CustomInt("value".into(), pct as i32));

            if let Err(e) = handle.update() {
                tracing::warn!("Failed to update progress notification: {e}");
            }
        } else {
            tracing::debug!("Upload progress: {pct}% ({bytes_sent}/{})", self.total);
        }

        #[cfg(not(target_os = "linux"))]
        tracing::debug!("Upload progress: {pct}% ({bytes_sent}/{})", self.total);
    }

    /// Done sets 100% swaps to completion message.
    pub fn finish(&mut self, message: &str) {
        // Ensure bar reaches 100%.
        self.update(self.total);

        #[cfg(target_os = "linux")]
        if let Some(ref mut handle) = self.handle {
            handle
                .summary("juicebox-plus")
                .body(message)
                .hint(Hint::CustomInt("value".into(), 100));

            if let Err(e) = handle.update() {
                tracing::warn!("Failed to update completion notification: {e}");
            }
        } else {
            tracing::info!("{message}");
        }

        #[cfg(not(target_os = "linux"))]
        tracing::info!("{message}");
    }
}
