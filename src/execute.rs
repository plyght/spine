use crate::detect::{DetectedManager, ManagerStatus};
use anyhow::Result;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub async fn execute_manager_workflow(manager: &mut DetectedManager) -> Result<()> {
    let config = &manager.config;

    // Refresh repositories
    if let Some(refresh_cmd) = &config.refresh {
        manager.status = ManagerStatus::Running("Refreshing".to_string(), String::new());
        match execute_command_with_timeout(
            refresh_cmd,
            config.requires_sudo,
            Duration::from_secs(300),
        )
        .await
        {
            Ok(result) if result.success => {
                let logs = format!("Refresh completed:\n{}\n{}", result.stdout, result.stderr);
                manager.status = ManagerStatus::Running("Refreshing".to_string(), logs);
            }
            Ok(result) => {
                manager.status = ManagerStatus::Failed(format!(
                    "Refresh failed: {}\nStdout: {}\nStderr: {}",
                    "Command failed", result.stdout, result.stderr
                ));
                return Ok(());
            }
            Err(e) => {
                manager.status = ManagerStatus::Failed(format!("Refresh error: {}", e));
                return Ok(());
            }
        }
    }

    // Self-update
    if let Some(self_update_cmd) = &config.self_update {
        manager.status = ManagerStatus::Running("Self-updating".to_string(), String::new());
        match execute_command_with_timeout(
            self_update_cmd,
            config.requires_sudo,
            Duration::from_secs(600),
        )
        .await
        {
            Ok(result) if result.success => {
                let logs = format!(
                    "Self-update completed:\n{}\n{}",
                    result.stdout, result.stderr
                );
                manager.status = ManagerStatus::Running("Self-updating".to_string(), logs);
            }
            Ok(result) => {
                manager.status = ManagerStatus::Failed(format!(
                    "Self-update failed: {}\nStdout: {}\nStderr: {}",
                    "Command failed", result.stdout, result.stderr
                ));
                return Ok(());
            }
            Err(e) => {
                manager.status = ManagerStatus::Failed(format!("Self-update error: {}", e));
                return Ok(());
            }
        }
    }

    // Upgrade all packages
    manager.status = ManagerStatus::Running("Upgrading".to_string(), String::new());
    match execute_command_with_timeout(
        &config.upgrade_all,
        config.requires_sudo,
        Duration::from_secs(3600),
    )
    .await
    {
        Ok(result) if result.success => {
            let logs = format!("Upgrade completed:\n{}\n{}", result.stdout, result.stderr);
            manager.status = ManagerStatus::Running("Upgrading".to_string(), logs);
        }
        Ok(result) => {
            manager.status = ManagerStatus::Failed(format!(
                "Upgrade failed: {}\nStdout: {}\nStderr: {}",
                "Command failed", result.stdout, result.stderr
            ));
            return Ok(());
        }
        Err(e) => {
            manager.status = ManagerStatus::Failed(format!("Upgrade error: {}", e));
            return Ok(());
        }
    }

    // Cleanup
    if let Some(cleanup_cmd) = &config.cleanup {
        manager.status = ManagerStatus::Running("Cleaning".to_string(), String::new());
        match execute_command_with_timeout(
            cleanup_cmd,
            config.requires_sudo,
            Duration::from_secs(300),
        )
        .await
        {
            Ok(result) if result.success => {
                let logs = format!("Cleanup completed:\n{}\n{}", result.stdout, result.stderr);
                manager.status = ManagerStatus::Running("Cleaning".to_string(), logs);
            }
            Ok(result) => {
                manager.status = ManagerStatus::Failed(format!(
                    "Cleanup failed: {}\nStdout: {}\nStderr: {}",
                    "Command failed", result.stdout, result.stderr
                ));
                return Ok(());
            }
            Err(e) => {
                manager.status = ManagerStatus::Failed(format!("Cleanup error: {}", e));
                return Ok(());
            }
        }
    }

    manager.status = ManagerStatus::Success;
    Ok(())
}

async fn execute_command_with_timeout(
    command: &str,
    requires_sudo: bool,
    timeout: Duration,
) -> Result<ExecutionResult> {
    let mut cmd = build_command(command, requires_sudo)?;

    let output = tokio::time::timeout(timeout, cmd.output()).await??;

    Ok(ExecutionResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn build_command(command: &str, requires_sudo: bool) -> Result<Command> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        anyhow::bail!("Empty command");
    }

    let mut cmd = if requires_sudo {
        // Check if sudo is available
        if which::which("sudo").is_err() {
            anyhow::bail!("sudo is required but not available");
        }

        let mut c = Command::new("sudo");
        c.arg("-n"); // Non-interactive mode
        c.args(&parts);
        c
    } else {
        let mut c = Command::new(parts[0]);
        if parts.len() > 1 {
            c.args(&parts[1..]);
        }
        c
    };

    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    Ok(cmd)
}

pub async fn check_sudo_availability() -> bool {
    if which::which("sudo").is_err() {
        return false;
    }

    // Test if we can run sudo without password prompt
    match Command::new("sudo")
        .args(["-n", "true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}
