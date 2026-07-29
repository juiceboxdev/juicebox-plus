use crate::config::Config;

fn is_valid_key_format(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.len() == 9
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1].is_ascii_alphanumeric()
        && bytes[2].is_ascii_alphanumeric()
        && bytes[3].is_ascii_alphanumeric()
        && bytes[4] == b'-'
        && bytes[5].is_ascii_alphanumeric()
        && bytes[6].is_ascii_alphanumeric()
        && bytes[7].is_ascii_alphanumeric()
        && bytes[8].is_ascii_alphanumeric()
}

#[cfg(target_os = "linux")]
mod platform {
    use std::collections::HashMap;
    use std::sync::Arc;

    use futures_util::StreamExt;
    use tokio::time::timeout;
    use zbus::connection::Connection;
    use zbus::match_rule::MatchRule;
    use zbus::message::Type;
    use zbus::proxy::Proxy;
    use zbus::zvariant::Value;
    use zbus::MessageStream;

    use crate::config::Config;

    const IFACE: &str = "org.freedesktop.Notifications";
    const PATH: &str = "/org/freedesktop/Notifications";

    pub fn prompt_and_validate(
        config: Arc<Config>,
        proxy: Option<tao::event_loop::EventLoopProxy<crate::state::TrayAction>>,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            match run_pairing_flow(&config).await {
                Ok(Some(key)) => super::validate_and_notify(&key, &config, proxy.as_ref()).await,
                Ok(None) => {
                    tracing::info!("Pairing cancelled or timed out");
                }
                Err(e) => {
                    tracing::error!("Pairing flow error: {e}");
                    #[cfg(target_os = "linux")]
                    let _ = notify_rust::Notification::new()
                        .summary("juicebox-plus")
                        .body("Pairing failed. Check logs.")
                        .appname("juicebox-plus")
                        .show();
                    #[cfg(not(target_os = "linux"))]
                    tracing::info!("Pairing failed. Check logs.");
                }
            }
        });
    }

    fn open_gui_dialog() -> Option<String> {
        tinyfiledialogs::input_box(
            "juicebox-plus Pair Device",
            "Enter pairing code (XXXX-XXXX):",
            "",
        )
    }

    async fn run_pairing_flow(
        config: &Config,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        let conn = Connection::session().await?;
        let proxy = Proxy::new(&conn, IFACE, PATH, IFACE).await?;

        let mut hints: HashMap<String, Value<'_>> = HashMap::new();
        hints.insert("has-inline-reply".into(), Value::Bool(true));
        hints.insert(
            "inline-reply-placeholder".into(),
            Value::Str("XXXX-XXXX".into()),
        );

        let notif_id: u32 = proxy
            .call_method(
                "Notify",
                &(
                    "juicebox-plus",
                    0u32,
                    "",
                    "Pair Device",
                    "Enter pairing code to authorize this device:",
                    vec!["gui", "GUI", "inline-reply", "Reply"],
                    hints,
                    -1i32,
                ),
            )
            .await?
            .body()
            .deserialize()?;

        tracing::info!("Pairing notification shown (id={notif_id})");

        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface(IFACE)?
            .build();

        let stream = MessageStream::for_match_rule(rule, &conn, Some(1)).await?;
        tokio::pin!(stream);

        let reply_timeout = std::time::Duration::from_secs(config.pairing_timeout_secs);
        match timeout(reply_timeout, collect_reply(&mut stream, notif_id)).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!("Pairing notification timed out");
                Ok(None)
            }
        }
    }

    async fn collect_reply(
        stream: &mut (impl futures_util::Stream<Item = Result<zbus::Message, zbus::Error>> + Unpin),
        notif_id: u32,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        while let Some(msg) = stream.next().await {
            let msg = msg?;

            let member = msg.header().member().map(|m| m.to_string());

            match member.as_deref() {
                Some("ActionInvoked") => {
                    if let Ok((id, action, reply)) =
                        msg.body().deserialize::<(u32, String, String)>()
                    {
                        if id == notif_id {
                            match action.as_str() {
                                "inline-reply" => return Ok(Some(reply)),
                                "__closed" => return Ok(None),
                                _ => {}
                            }
                        }
                    } else if let Ok((id, action)) = msg.body().deserialize::<(u32, String)>() {
                        if id == notif_id {
                            match action.as_str() {
                                "gui" => {
                                    let result = tokio::task::spawn_blocking(open_gui_dialog)
                                        .await
                                        .unwrap_or(None);
                                    return Ok(result);
                                }
                                "__closed" => return Ok(None),
                                _ => {}
                            }
                        }
                    }
                }
                Some("NotificationReplied") => {
                    if let Ok((id, reply)) = msg.body().deserialize::<(u32, String)>() {
                        if id == notif_id {
                            return Ok(Some(reply));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::{Arc, Mutex};

    use windows::core::{IInspectable, Interface, HSTRING};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{IPropertyValue, TypedEventHandler};
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastDismissedEventArgs, ToastNotification,
        ToastNotificationManager,
    };

    use crate::config::Config;

    pub fn prompt_and_validate(
        config: Arc<Config>,
        proxy: Option<tao::event_loop::EventLoopProxy<crate::state::TrayAction>>,
    ) {
        let toast_xml = r#"
        <toast>
            <visual>
                <binding template="ToastGeneric">
                    <text>juicebox-plus</text>
                    <text>Enter pairing code to authorize this device:</text>
                </binding>
            </visual>
            <actions>
                <input id="pairingCode" type="text" placeHolderContent="XXXX-XXXX"/>
                <action content="Pair" arguments="action=pair" inputId="pairingCode"/>
                <action content="GUI" arguments="action=gui"/>
                <action content="Cancel" arguments="action=cancel"/>
            </actions>
        </toast>
        "#;

        let xml_doc = match XmlDocument::new() {
            Ok(doc) => doc,
            Err(_) => return,
        };

        if xml_doc.LoadXml(&HSTRING::from(toast_xml)).is_err() {
            return;
        }

        let notification = match ToastNotification::CreateToastNotification(&xml_doc) {
            Ok(n) => n,
            Err(_) => return,
        };

        let reply: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let reply_activated = reply.clone();

        let activated_handler = TypedEventHandler::<ToastNotification, IInspectable>::new(
            move |_sender: ::windows::core::Ref<'_, ToastNotification>, args: ::windows::core::Ref<'_, IInspectable>| {
                if let Ok(args) = args.cast::<ToastActivatedEventArgs>() {
                    if let Ok(arguments) = args.Arguments() {
                        let arguments_str = arguments.to_string();
                        if arguments_str.contains("action=gui") {
                            let result = tinyfiledialogs::input_box(
                                "juicebox-plus Pair Device",
                                "Enter pairing code (XXXX-XXXX):",
                                "",
                            );
                            *reply_activated.lock().unwrap() = result;
                            return Ok(());
                        }
                    }
                    if let Ok(user_input) = args.UserInput() {
                        if let Ok(code) = user_input.Lookup(&HSTRING::from("pairingCode")) {
                            if let Ok(prop) = code.cast::<IPropertyValue>() {
                                if let Ok(hstr) = prop.GetString() {
                                    let s: String = hstr.to_string();
                                    if !s.trim().is_empty() {
                                        *reply_activated.lock().unwrap() = Some(s);
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            },
        );
        let _ = notification.Activated(&activated_handler);

        let dismissed_handler = TypedEventHandler::<ToastNotification, ToastDismissedEventArgs>::new(
            move |_sender: ::windows::core::Ref<'_, ToastNotification>, _args: ::windows::core::Ref<'_, ToastDismissedEventArgs>| {
                Ok(())
            },
        );
        let _ = notification.Dismissed(&dismissed_handler);

        let notifier = match ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(
            "juicebox-plus",
        )) {
            Ok(n) => n,
            Err(_) => return,
        };

        let _ = notifier.Show(&notification);

        std::mem::forget(notification);

        let timeout = std::time::Duration::from_secs(config.pairing_timeout_secs);
        let start = std::time::Instant::now();

        loop {
            let key = reply.lock().unwrap().take();
            if let Some(key) = key {
                let cfg = config.clone();
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async { super::validate_and_notify(&key, &cfg, proxy.as_ref()).await });
                return;
            }

            let file_key = read_pairing_key_from_file();
            if !file_key.is_empty() {
                let cfg = config.clone();
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    super::validate_and_notify(&file_key, &cfg, proxy.as_ref()).await
                });
                return;
            }

            if start.elapsed() >= timeout {
                tracing::warn!("Pairing notification timed out");
                return;
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    fn read_pairing_key_from_file() -> String {
        let path = std::env::temp_dir().join("juicebox-plus-pairing-key");
        match std::fs::read_to_string(&path) {
            Ok(key) => {
                let _ = std::fs::remove_file(&path);
                key.trim().to_string()
            }
            Err(_) => String::new(),
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::sync::Arc;

    use mac_notification_sys::{send_notification, MainButton, Notification, NotificationResponse};

    use crate::config::Config;

    pub fn prompt_and_validate(
        config: Arc<Config>,
        proxy: Option<tao::event_loop::EventLoopProxy<crate::state::TrayAction>>,
    ) {
        let response = send_notification(
            "juicebox-plus",
            None,
            "Enter pairing code to authorize this device:",
            Some(Notification::new().main_button(MainButton::Response("XXXX-XXXX"))),
        );

        match response {
            Ok(NotificationResponse::Reply(key)) => {
                let key = key.trim().to_string();
                if !key.is_empty() {
                    let cfg = config.clone();
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        super::validate_and_notify(&key, &cfg, proxy.as_ref()).await
                    });
                }
            }
            Ok(NotificationResponse::ActionButton(action)) => {
                tracing::info!("macOS notification action: {action}");
            }
            Ok(_) => {
                tracing::info!("Pairing cancelled");
            }
            Err(e) => {
                tracing::error!("macOS notification error: {e}");
            }
        }
    }
}

async fn validate_and_notify(
    key: &str,
    config: &Config,
    proxy: Option<&tao::event_loop::EventLoopProxy<crate::state::TrayAction>>,
) {
    if !is_valid_key_format(key) {
        notify_all("Invalid format. Code must be XXXX-XXXX.");
        return;
    }

    let code = key.to_uppercase();
    let instance = config.juicebox_instance.trim_end_matches('/').to_string();
    let hostname = gethostname::gethostname().to_string_lossy().to_string();

    let result = exchange_pairing_code(&instance, &code, &hostname).await;

    match result {
        Ok(PairingResult { device_id, token }) => {
            if let Err(e) = crate::token::save_token(&token) {
                tracing::error!("Failed to save token: {e}");
                notify_all(&format!("Paired but failed to save token: {e}"));
                return;
            }

            let mut cfg = config.clone();
            cfg.paired = true;
            cfg.device_id = Some(device_id);
            if let Err(e) = cfg.save() {
                tracing::warn!("Failed to save config: {e}");
            }

            tracing::info!("Device paired successfully as '{}'", hostname);
            notify_all("Device paired successfully!");

            if let Some(p) = proxy {
                let _ = p.send_event(crate::state::TrayAction::PairingComplete(hostname));
            }
        }
        Err(e) => {
            tracing::error!("Pairing failed: {e}");
            notify_all(&format!("Pairing failed: {e}"));
        }
    }
}

struct PairingResult {
    device_id: String,
    token: String,
}

async fn exchange_pairing_code(
    instance: &str,
    code: &str,
    device_name: &str,
) -> Result<PairingResult, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/pair/verify", instance))
        .json(&serde_json::json!({
            "code": code,
            "device_name": device_name,
        }))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    if resp.status().is_success() {
        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid response: {e}"))?;

        let token = data["token"].as_str().ok_or("Missing token in response")?;
        let device_id = data["device_id"]
            .as_str()
            .ok_or("Missing device_id in response")?;

        Ok(PairingResult {
            device_id: device_id.to_string(),
            token: token.to_string(),
        })
    } else if resp.status().as_u16() == 410 {
        Err("Code expired. Generate a new one.".into())
    } else if resp.status().as_u16() == 409 {
        Err("Code already used. Generate a new one.".into())
    } else if resp.status().as_u16() == 404 {
        Err("Invalid code. Check and try again.".into())
    } else {
        let status = resp.status();
        Err(format!("Pairing failed (HTTP {status})"))
    }
}

fn notify_all(body: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = notify_rust::Notification::new()
            .summary("juicebox-plus")
            .body(body)
            .appname("juicebox-plus")
            .show();
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
                    <text>{body}</text>
                </binding></visual>
            </toast>"#
        );
        if let Ok(doc) = XmlDocument::new() {
            if doc.LoadXml(&HSTRING::from(&toast_xml)).is_ok() {
                if let Ok(n) = ToastNotification::CreateToastNotification(&doc) {
                    if let Ok(nt) = ToastNotificationManager::CreateToastNotifierWithId(
                        &HSTRING::from("juicebox-plus"),
                    ) {
                        let _ = nt.Show(&n);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        use mac_notification_sys::{send_notification, Notification};

        let _ = send_notification(
            "juicebox-plus",
            None,
            body,
            Some(Notification::new().asynchronous(true)),
        );
    }
}

pub use platform::prompt_and_validate;
