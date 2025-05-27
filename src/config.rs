use std::collections::HashMap;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub managers: HashMap<String, ManagerConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManagerConfig {
    pub name: String,
    pub check_command: String,
    pub refresh: Option<String>,
    pub self_update: Option<String>,
    pub upgrade_all: String,
    pub cleanup: Option<String>,
    pub requires_sudo: bool,
}

pub async fn load_config() -> Result<Config> {
    // Try multiple possible locations for backbone.toml
    let possible_paths = vec![
        std::env::current_dir()?.join("backbone.toml"),
        std::env::current_exe()?.parent().unwrap().join("backbone.toml"),
        std::path::PathBuf::from("/etc/spine/backbone.toml"),
        std::path::PathBuf::from("/usr/local/etc/spine/backbone.toml"),
    ];
    
    for path in possible_paths {
        if path.exists() {
            let content = tokio::fs::read_to_string(&path).await?;
            let config: Config = toml::from_str(&content)?;
            return Ok(config);
        }
    }
    
    anyhow::bail!("backbone.toml configuration file not found. Please ensure it's in the current directory or installed with the binary.");
}