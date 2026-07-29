use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

const APP_NAME: &str = "juicebox-plus"; // if yall want a fork you can name it something else using this variable

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub daemon_address: String,
    pub juicebox_instance: String,
    pub use_tls: bool,

    pub chunk_size_bytes: u64,
    pub fec_enabled: bool,
    pub compression_mode: String,

    pub pairing_timeout_secs: u64,
    pub upload_ttl_hours: Option<f64>,

    pub paired: bool,
    pub device_id: Option<String>,

    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            daemon_address: "127.0.0.1:22100".into(),
            juicebox_instance: "https://box.juicey.dev/".into(),
            use_tls: false,
            chunk_size_bytes: 33_554_432,
            fec_enabled: true,
            compression_mode: "auto".into(),
            pairing_timeout_secs: 30,
            upload_ttl_hours: None,
            paired: false,
            device_id: None,
            log_level: "info".into(),
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join(APP_NAME).join("config.toml"))
    }

    pub fn last_modified() -> Option<SystemTime> {
        let path = Self::path()?;
        fs::metadata(path).ok().and_then(|m| m.modified().ok())
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            tracing::warn!("Could not determine config directory, using defaults");
            return Self::default();
        };

        match fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => {
                    tracing::info!("Loaded config from {}", path.display());
                    config
                }
                Err(e) => {
                    tracing::error!("Failed to parse config at {}: {e}", path.display());
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!("No config file found, using defaults");
                let config = Self::default();
                if let Err(e) = config.save() {
                    tracing::warn!("Failed to write default config: {e}");
                }
                config
            }
            Err(e) => {
                tracing::error!("Failed to read config at {}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let Some(path) = Self::path() else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let contents = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        {
            use std::fs::OpenOptions;
            use std::io::Write;
            #[cfg(unix)]
            use std::os::unix::fs::OpenOptionsExt;
            let mut opts = OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            opts.mode(0o600);
            let mut file = opts.open(&path)?;
            file.write_all(contents.as_bytes())?;
        }
        Ok(())
    }

    pub fn current_value(&self, setting: &str) -> String {
        match setting {
            "daemon_address" => self.daemon_address.clone(),
            "juicebox_instance" => self.juicebox_instance.clone(),
            "chunk_size_bytes" => self.chunk_size_bytes.to_string(),
            "pairing_timeout_secs" => self.pairing_timeout_secs.to_string(),
            "upload_ttl_hours" => self.upload_ttl_hours.map(|v| v.to_string()).unwrap_or_else(|| "default".to_string()),
            "paired" => self.paired.to_string(),
            "device_id" => self.device_id.clone().unwrap_or_default(),
            "log_level" => self.log_level.clone(),
            _ => String::new(),
        }
    }

    pub fn update(&mut self, setting: &str, value: &str) -> Result<(), String> {
        match setting {
            "daemon_address" => self.daemon_address = value.to_string(),
            "juicebox_instance" => self.juicebox_instance = value.to_string(),
            "chunk_size_bytes" => {
                self.chunk_size_bytes = value.parse().map_err(|_| "expected a number")?;
            }
            "pairing_timeout_secs" => {
                self.pairing_timeout_secs = value.parse().map_err(|_| "expected a number")?;
            }
            "upload_ttl_hours" => {
                if value.trim().is_empty() || value == "default" {
                    self.upload_ttl_hours = None;
                } else {
                    let hours: f64 = value.parse().map_err(|_| "expected a number")?;
                    self.upload_ttl_hours = Some(hours);
                }
            }
            "log_level" => self.log_level = value.to_string(),
            _ => return Err(format!("unknown setting: {setting}")),
        }
        self.save().map_err(|e| e.to_string())
    }
}
