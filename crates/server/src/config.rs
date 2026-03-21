use anyhow::anyhow;
use clap::Parser;
use fern::colors::{Color, ColoredLevelConfig};
use log::LevelFilter;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
};
use time::{format_description::well_known::Iso8601, OffsetDateTime};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to Settings.toml file holding configuration options
    #[arg(short, long)]
    pub config: Option<String>,

    /// Log level to run with the service (default: info)
    #[arg(short, long)]
    pub level: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    pub config: Option<String>,
    pub level: Option<String>,
    pub db_settings: DBSettings,
    pub api_settings: APISettings,
    pub ui_settings: UISettings,
    #[serde(default)]
    pub ln_settings: LnSettings,
    #[serde(default)]
    pub competition_settings: CompetitionSettings,
    #[serde(default)]
    pub bot_detection: BotDetectionSettings,
    #[serde(default)]
    pub admin: AdminSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DBSettings {
    pub data_folder: String,
    pub migrations_folder: String,
}

impl Default for DBSettings {
    fn default() -> Self {
        DBSettings {
            data_folder: String::from("./data"),
            migrations_folder: String::from("./crates/server/migrations"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UISettings {
    pub remote_url: String,
    pub ui_dir: String,
    /// Directory containing bundled static assets (JS/CSS).
    #[serde(default = "default_static_dir")]
    pub static_dir: String,
    /// Default Nostr relays for NIP-07 extension auth.
    /// Users can override these in the UI.
    #[serde(default = "default_relays")]
    pub default_relays: Vec<String>,
}

fn default_static_dir() -> String {
    String::from("./static")
}

fn default_relays() -> Vec<String> {
    vec![
        "wss://relay.damus.io".to_string(),
        "wss://relay.primal.net".to_string(),
    ]
}

impl Default for UISettings {
    fn default() -> Self {
        UISettings {
            remote_url: String::from("http://127.0.0.1:8900"),
            ui_dir: String::from("./ui"),
            static_dir: default_static_dir(),
            default_relays: default_relays(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct APISettings {
    pub domain: String,
    pub port: String,
    /// Nostr private key used to sign game data and verify users
    pub private_key_file: String,
    pub voltage_api_key: String,
    pub voltage_api_url: String,
    pub voltage_org_id: String,
    pub voltage_env_id: String,
    pub voltage_wallet_id: String,
}

impl Default for APISettings {
    fn default() -> Self {
        APISettings {
            domain: String::from("127.0.0.1"),
            port: String::from("8900"),
            private_key_file: String::from("./creds/private.pem"),
            voltage_api_key: String::from(""),
            voltage_api_url: String::from("https://voltageapi.com/v1/"),
            voltage_org_id: String::from(""),
            voltage_env_id: String::from(""),
            voltage_wallet_id: String::from(""),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LnSettings {
    /// Lightning provider: "voltage" or "lnd"
    pub provider: String,
    /// LND REST API base URL (used when provider = "lnd")
    pub lnd_base_url: Option<String>,
    /// Path to the LND admin macaroon file (used when provider = "lnd")
    pub lnd_macaroon_path: Option<String>,
    /// Path to the LND TLS certificate for self-signed certs (used when provider = "lnd")
    pub lnd_tls_cert_path: Option<String>,
}

impl Default for LnSettings {
    fn default() -> Self {
        LnSettings {
            provider: "voltage".to_string(),
            lnd_base_url: None,
            lnd_macaroon_path: None,
            lnd_tls_cert_path: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompetitionSettings {
    /// When the competition window opens (HH:MM in UTC, e.g. "00:00")
    pub start_time: String,
    /// How long the competition runs after start_time.
    /// Format: seconds (u64). e.g. 86400 = 24h, 3600 = 1h, 300 = 5min.
    pub duration_secs: u64,
    /// Entry fee in sats
    pub entry_fee_sats: i64,
    /// Number of game sessions granted per payment
    #[serde(default = "default_plays_per_payment")]
    pub plays_per_payment: i32,
    /// How long granted plays remain valid (in minutes). 0 = never expire.
    #[serde(default = "default_plays_ttl_minutes")]
    pub plays_ttl_minutes: i64,
    /// Prize pool percentage (0-100) — remainder goes to server
    pub prize_pool_pct: u8,
}

impl CompetitionSettings {
    /// Parse "HH:MM" into (hour, minute)
    fn parse_time(time_str: &str) -> (u8, u8) {
        let parts: Vec<&str> = time_str.split(':').collect();
        let hour = parts.first().and_then(|h| h.parse().ok()).unwrap_or(0);
        let minute = parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);
        (hour, minute)
    }

    pub fn start_hour_minute(&self) -> (u8, u8) {
        Self::parse_time(&self.start_time)
    }

    /// Compute the end (hour, minute) by adding duration_secs to start_time.
    pub fn end_hour_minute(&self) -> (u8, u8) {
        let (sh, sm) = self.start_hour_minute();
        let start_mins = sh as u64 * 60 + sm as u64;
        let end_mins = (start_mins + self.duration_secs / 60) % (24 * 60);
        ((end_mins / 60) as u8, (end_mins % 60) as u8)
    }

    /// Human-readable duration string (e.g. "24h", "1h30m", "5m")
    pub fn duration_display(&self) -> String {
        let hours = self.duration_secs / 3600;
        let mins = (self.duration_secs % 3600) / 60;
        let secs = self.duration_secs % 60;
        if hours > 0 && mins > 0 {
            format!("{}h{}m", hours, mins)
        } else if hours > 0 {
            format!("{}h", hours)
        } else if mins > 0 && secs > 0 {
            format!("{}m{}s", mins, secs)
        } else if mins > 0 {
            format!("{}m", mins)
        } else {
            format!("{}s", secs)
        }
    }
}

fn default_plays_per_payment() -> i32 {
    5
}

fn default_plays_ttl_minutes() -> i64 {
    60
}

impl Default for CompetitionSettings {
    fn default() -> Self {
        CompetitionSettings {
            start_time: "00:00".to_string(),
            duration_secs: 86400, // 24 hours
            entry_fee_sats: 1000,
            plays_per_payment: 5,
            plays_ttl_minutes: 60,
            prize_pool_pct: 80,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BotDetectionSettings {
    pub enabled: bool,
    /// Max distinct accounts allowed from one IP in a rolling hour
    pub max_accounts_per_ip_per_hour: u32,
    /// Max sessions from one IP in a rolling hour
    pub max_sessions_per_ip_per_hour: u32,
    /// Minimum frame timing variance in microseconds^2.
    /// Human play: 5000+. Perfect setInterval: <1000.
    pub min_timing_variance_us2: u64,
    /// Max mean timing offset in microseconds (positive = slower than real-time).
    /// Catches slow-motion cheating.
    pub max_mean_offset_us: i64,
}

impl Default for BotDetectionSettings {
    fn default() -> Self {
        BotDetectionSettings {
            enabled: true,
            max_accounts_per_ip_per_hour: 5,
            max_sessions_per_ip_per_hour: 20,
            min_timing_variance_us2: 1000,
            max_mean_offset_us: 50000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdminSettings {
    /// CIDR subnet(s) allowed to access /admin (e.g. "10.100.0.0/24", "127.0.0.1/32")
    pub allowed_subnets: Vec<String>,
}

impl Default for AdminSettings {
    fn default() -> Self {
        AdminSettings {
            allowed_subnets: vec![
                "10.100.0.0/24".to_string(),
                "127.0.0.1/32".to_string(),
                "::1/128".to_string(),
            ],
        }
    }
}

pub fn get_settings() -> Result<Settings, anyhow::Error> {
    let cli = Cli::parse();

    let mut settings = if let Some(config_path) = cli.config {
        let path = PathBuf::from(config_path);

        let absolute_path = if path.is_absolute() {
            path
        } else {
            env::current_dir()?.join(path)
        };

        let file_settings = match File::open(absolute_path) {
            Ok(mut file) => {
                let mut content = String::new();
                file.read_to_string(&mut content)
                    .map_err(|e| anyhow!("Failed to read config: {}", e))?;
                toml::from_str(&content)
                    .map_err(|e| anyhow!("Failed to map config to settings: {}", e))?
            }
            Err(err) => return Err(anyhow!("Failed to find file: {}", err)),
        };
        file_settings
    } else {
        let default_path = PathBuf::from("./config/local.toml");
        match File::open(&default_path) {
            Ok(mut file) => {
                let mut content = String::new();
                file.read_to_string(&mut content)
                    .map_err(|e| anyhow!("Failed to read default config: {}", e))?;
                toml::from_str(&content)
                    .map_err(|e| anyhow!("Failed to parse default config: {}", e))?
            }
            Err(_) => {
                // Create default settings
                let default_settings = Settings::default();

                // Create config directory if it doesn't exist
                fs::create_dir_all("./config")
                    .map_err(|e| anyhow!("Failed to create config directory: {}", e))?;

                let toml_content = toml::to_string(&default_settings)
                    .map_err(|e| anyhow!("Failed to serialize default settings: {}", e))?;

                let mut file = fs::File::create(&default_path)
                    .map_err(|e| anyhow!("Failed to create config file: {}", e))?;
                file.write_all(toml_content.as_bytes())
                    .map_err(|e| anyhow!("Failed to write default config: {}", e))?;

                default_settings
            }
        }
    };

    if let Some(cli_level) = cli.level {
        settings.level = Some(cli_level);
    }

    Ok(settings)
}

pub fn setup_logger(level: Option<String>) -> Result<(), fern::InitError> {
    let rust_log = get_log_level(level);
    let colors = ColoredLevelConfig::new()
        .trace(Color::White)
        .debug(Color::Cyan)
        .info(Color::Blue)
        .warn(Color::Yellow)
        .error(Color::Magenta);

    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{} {}] {}: {}",
                OffsetDateTime::now_utc().format(&Iso8601::DEFAULT).unwrap(),
                colors.color(record.level()),
                record.target(),
                message
            ));
        })
        .level(rust_log)
        .filter(|metadata| !metadata.target().starts_with("hyper"))
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}

pub fn get_log_level(level: Option<String>) -> LevelFilter {
    if let Some(level) = &level {
        match level.as_ref() {
            "trace" => LevelFilter::Trace,
            "debug" => LevelFilter::Debug,
            "info" => LevelFilter::Info,
            "warn" => LevelFilter::Warn,
            "error" => LevelFilter::Error,
            _ => LevelFilter::Info,
        }
    } else {
        let rust_log = env::var("RUST_LOG").unwrap_or_else(|_| String::from(""));
        match rust_log.to_lowercase().as_str() {
            "trace" => LevelFilter::Trace,
            "debug" => LevelFilter::Debug,
            "info" => LevelFilter::Info,
            "warn" => LevelFilter::Warn,
            "error" => LevelFilter::Error,
            _ => LevelFilter::Info,
        }
    }
}
