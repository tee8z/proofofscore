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
}

impl Default for UISettings {
    fn default() -> Self {
        UISettings {
            remote_url: String::from("http://127.0.0.1:8900"),
            ui_dir: String::from("./crates/public_ui"),
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
            private_key_file: String::from("./creds/private.key"),
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
