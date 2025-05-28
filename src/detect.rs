use crate::config::{Config, ManagerConfig};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct DetectedManager {
    pub name: String,
    pub config: ManagerConfig,
    pub status: ManagerStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ManagerStatus {
    Pending,
    Running(String, String), // (operation_name, logs)
    Success(String),         // (final_logs)
    Failed(String),
}

pub async fn detect_package_managers(config: &Config) -> Result<Vec<DetectedManager>> {
    let mut detected = Vec::new();

    for (name, manager_config) in &config.managers {
        if is_manager_available(&manager_config.check_command).await? {
            detected.push(DetectedManager {
                name: name.clone(),
                config: manager_config.clone(),
                status: ManagerStatus::Pending,
            });
        }
    }

    Ok(detected)
}

async fn is_manager_available(check_command: &str) -> Result<bool> {
    let parts: Vec<&str> = check_command.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(false);
    }

    let command = parts[0];
    Ok(which::which(command).is_ok())
}
