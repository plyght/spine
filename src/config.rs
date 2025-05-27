use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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

fn get_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // XDG config directory (~/.config/spine/backbone.toml) - FIRST priority
    if let Some(config_dir) = dirs::config_dir() {
        paths.push(config_dir.join("spine").join("backbone.toml"));
    }

    // Current directory
    if let Ok(current_dir) = std::env::current_dir() {
        paths.push(current_dir.join("backbone.toml"));
    }

    // Home directory (~/.spine/backbone.toml)
    if let Some(home_dir) = dirs::home_dir() {
        paths.push(home_dir.join(".spine").join("backbone.toml"));
    }

    // Binary directory
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            paths.push(parent.join("backbone.toml"));
        }
    }

    // System directories
    paths.push(PathBuf::from("/etc/spine/backbone.toml"));
    paths.push(PathBuf::from("/usr/local/etc/spine/backbone.toml"));

    paths
}

async fn create_default_config() -> Result<PathBuf> {
    let default_config = include_str!("../backbone.toml");

    // Always try XDG config directory first (default on all systems)
    if let Some(config_dir) = dirs::config_dir() {
        let spine_config_dir = config_dir.join("spine");
        let config_path = spine_config_dir.join("backbone.toml");

        match tokio::fs::create_dir_all(&spine_config_dir).await {
            Ok(_) => {
                tokio::fs::write(&config_path, default_config).await?;
                return Ok(config_path);
            }
            Err(_) => {
                // Continue to fallback if XDG fails
            }
        }
    }

    // Fallback to home directory
    if let Some(home_dir) = dirs::home_dir() {
        let spine_home_dir = home_dir.join(".spine");
        let config_path = spine_home_dir.join("backbone.toml");
        tokio::fs::create_dir_all(&spine_home_dir).await?;
        tokio::fs::write(&config_path, default_config).await?;
        return Ok(config_path);
    }

    anyhow::bail!("Unable to create config directory in any standard location");
}

pub async fn load_config() -> Result<Config> {
    let possible_paths = get_config_paths();

    for path in &possible_paths {
        if path.exists() {
            let content = tokio::fs::read_to_string(&path).await?;
            let config: Config = toml::from_str(&content)?;
            return Ok(config);
        }
    }

    // No config found, create a default one
    let created_path = create_default_config().await?;
    let content = tokio::fs::read_to_string(&created_path).await?;
    let config: Config = toml::from_str(&content)?;

    eprintln!(
        "Created default configuration at: {}",
        created_path.display()
    );
    Ok(config)
}
