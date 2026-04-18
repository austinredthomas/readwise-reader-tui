use anyhow::Result;
use clap::Parser;
use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub token: String,
    pub default_location: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            default_location: "new".to_string(),
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Readwise API Token
    #[arg(short, long, env = "READWISE_TOKEN")]
    token: Option<String>,

    /// Default location to load (new, later, archive, feed)
    #[arg(short, long)]
    location: Option<String>,

    /// Path to config file
    #[arg(short, long)]
    config: Option<PathBuf>,
}

pub fn load_config() -> Result<AppConfig> {
    let args = Args::parse();

    // Determine config file path
    let config_path = args.config.unwrap_or_else(|| {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("readwise-reader-tui");
        path.push("config.toml");
        path
    });

    let figment = Figment::new()
        .merge(Toml::file(&config_path))
        .merge(Env::prefixed("READWISE_"));

    // Manual merge of CLI args if present
    let mut config: AppConfig = figment.extract().unwrap_or_default();

    if let Some(token) = args.token {
        config.token = token;
    }
    if let Some(location) = args.location {
        config.default_location = location;
    }

    if config.token.is_empty() {
        anyhow::bail!("Readwise API token is missing. Please provide it via READWISE_TOKEN env var, --token flag, or in config.toml");
    }

    Ok(config)
}
