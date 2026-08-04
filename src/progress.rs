#[cfg(target_os = "linux")]
use notify_rust::{Hint, Notification, NotificationHandle};

/// Progress tracker that shoves a progress bar into desktop notifications.
/// tested on plasma, gnome; if your DE ignores it (e.g, my hyprland config (end4) ), rip.
pub struct ProgressTracker {
    #[cfg(target_os = "linux")]
    handle: Option<NotificationHandle>,
    #[cfg(target_os = "windows")]
    tag: String,
    total: u64,
    last_pct: u32,
}

#[cfg(target_os = "windows")]
fn show_progress_toast(tag: &str, body: &str, pct: u32) {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let toast_xml = format!(
        r#"<toast>
            <visual><binding template="ToastGeneric">
                <text>juicebox-plus</text>
                <text>{body}</text>
                <progress value="{pct}" status="{pct}%" />
            </binding></visual>
        </toast>"#
    );
    if let Ok(doc) = XmlDocument::new() {
        if doc.LoadXml(&HSTRING::from(&toast_xml)).is_ok() {
            if let Ok(n) = ToastNotification::CreateToastNotification(&doc) {
                n.SetTag(&HSTRING::from(tag)).ok();
                if let Ok(nt) = ToastNotificationManager::CreateToastNotifierWithId(
                    &HSTRING::from("juicebox-plus"),
                ) {
                    let _ = nt.Show(&n);
                }
            }
        }
    }
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

        #[cfg(target_os = "windows")]
        let tag = format!("juicebox-plus-progress-{}", std::process::id());

        #[cfg(target_os = "windows")]
        show_progress_toast(&tag, title, 0);

        Self {
            #[cfg(target_os = "linux")]
            handle,
            #[cfg(target_os = "windows")]
            tag,
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

        #[cfg(target_os = "windows")]
        {
            let mb_done = bytes_sent / 1_048_576;
            let mb_total = self.total / 1_048_576;
            show_progress_toast(
                &self.tag,
                &format!("{mb_done} MB / {mb_total} MB ({pct}%)"),
                pct,
            );
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
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

        #[cfg(target_os = "windows")]
        {
            use windows::core::HSTRING;
            use windows::Data::Xml::Dom::XmlDocument;
            use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

            let toast_xml = format!(
                r#"<toast>
                    <visual><binding template="ToastGeneric">
                        <text>juicebox-plus</text>
                        <text>{message}</text>
                    </binding></visual>
                </toast>"#
            );
            if let Ok(doc) = XmlDocument::new() {
                if doc.LoadXml(&HSTRING::from(&toast_xml)).is_ok() {
                    if let Ok(n) = ToastNotification::CreateToastNotification(&doc) {
                        n.SetTag(&HSTRING::from(&self.tag)).ok();
                        if let Ok(nt) = ToastNotificationManager::CreateToastNotifierWithId(
                            &HSTRING::from("juicebox-plus"),
                        ) {
                            let _ = nt.Show(&n);
                        }
                    }
                }
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        tracing::info!("{message}");
    }
}
