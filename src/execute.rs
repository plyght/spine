use crate::detect::{DetectedManager, ManagerStatus};
use anyhow::Result;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub async fn execute_manager_workflow(manager_ref: Arc<Mutex<DetectedManager>>) -> Result<()> {
    let config = {
        let manager = manager_ref.lock().unwrap();
        manager.config.clone()
    };

    let mut accumulated_logs = String::new();

    // Refresh repositories
    if let Some(refresh_cmd) = &config.refresh {
        accumulated_logs.push_str("=== REFRESHING REPOSITORIES ===\n");
        {
            let mut manager = manager_ref.lock().unwrap();
            manager.status =
                ManagerStatus::Running("Refreshing".to_string(), accumulated_logs.clone());
        }

        match execute_command_with_logs(
            refresh_cmd,
            config.requires_sudo,
            Duration::from_secs(300),
            manager_ref.clone(),
            "Refreshing".to_string(),
            &mut accumulated_logs,
        )
        .await
        {
            Ok(true) => {
                accumulated_logs.push_str("\n✓ Refresh completed\n\n");
            }
            Ok(false) => {
                let mut manager = manager_ref.lock().unwrap();
                manager.status = ManagerStatus::Failed(format!(
                    "Refresh command failed\n\nLogs:\n{}",
                    accumulated_logs
                ));
                return Ok(());
            }
            Err(e) => {
                let mut manager = manager_ref.lock().unwrap();
                manager.status = ManagerStatus::Failed(format!(
                    "Refresh error: {}\n\nLogs:\n{}",
                    e, accumulated_logs
                ));
                return Ok(());
            }
        }
    }

    // Self-update
    if let Some(self_update_cmd) = &config.self_update {
        accumulated_logs.push_str("=== SELF-UPDATE ===\n");
        {
            let mut manager = manager_ref.lock().unwrap();
            manager.status =
                ManagerStatus::Running("Self-updating".to_string(), accumulated_logs.clone());
        }

        match execute_command_with_logs(
            self_update_cmd,
            config.requires_sudo,
            Duration::from_secs(600),
            manager_ref.clone(),
            "Self-updating".to_string(),
            &mut accumulated_logs,
        )
        .await
        {
            Ok(true) => {
                accumulated_logs.push_str("\n✓ Self-update completed\n\n");
            }
            Ok(false) => {
                let mut manager = manager_ref.lock().unwrap();
                manager.status = ManagerStatus::Failed(format!(
                    "Self-update command failed\n\nLogs:\n{}",
                    accumulated_logs
                ));
                return Ok(());
            }
            Err(e) => {
                let mut manager = manager_ref.lock().unwrap();
                manager.status = ManagerStatus::Failed(format!(
                    "Self-update error: {}\n\nLogs:\n{}",
                    e, accumulated_logs
                ));
                return Ok(());
            }
        }
    }

    // Upgrade all packages
    accumulated_logs.push_str("=== UPGRADING PACKAGES ===\n");
    {
        let mut manager = manager_ref.lock().unwrap();
        manager.status = ManagerStatus::Running("Upgrading".to_string(), accumulated_logs.clone());
    }

    match execute_command_with_logs(
        &config.upgrade_all,
        config.requires_sudo,
        Duration::from_secs(3600),
        manager_ref.clone(),
        "Upgrading".to_string(),
        &mut accumulated_logs,
    )
    .await
    {
        Ok(true) => {
            accumulated_logs.push_str("\n✓ Upgrade completed\n\n");
        }
        Ok(false) => {
            let mut manager = manager_ref.lock().unwrap();
            manager.status = ManagerStatus::Failed(format!(
                "Upgrade command failed\n\nLogs:\n{}",
                accumulated_logs
            ));
            return Ok(());
        }
        Err(e) => {
            let mut manager = manager_ref.lock().unwrap();
            manager.status = ManagerStatus::Failed(format!(
                "Upgrade error: {}\n\nLogs:\n{}",
                e, accumulated_logs
            ));
            return Ok(());
        }
    }

    // Cleanup
    if let Some(cleanup_cmd) = &config.cleanup {
        accumulated_logs.push_str("=== CLEANUP ===\n");
        {
            let mut manager = manager_ref.lock().unwrap();
            manager.status =
                ManagerStatus::Running("Cleaning".to_string(), accumulated_logs.clone());
        }

        match execute_command_with_logs(
            cleanup_cmd,
            config.requires_sudo,
            Duration::from_secs(300),
            manager_ref.clone(),
            "Cleaning".to_string(),
            &mut accumulated_logs,
        )
        .await
        {
            Ok(true) => {
                accumulated_logs.push_str("\n✓ Cleanup completed\n\n");
            }
            Ok(false) => {
                let mut manager = manager_ref.lock().unwrap();
                manager.status = ManagerStatus::Failed(format!(
                    "Cleanup command failed\n\nLogs:\n{}",
                    accumulated_logs
                ));
                return Ok(());
            }
            Err(e) => {
                let mut manager = manager_ref.lock().unwrap();
                manager.status = ManagerStatus::Failed(format!(
                    "Cleanup error: {}\n\nLogs:\n{}",
                    e, accumulated_logs
                ));
                return Ok(());
            }
        }
    }

    // Set final success status with complete logs
    {
        let mut manager = manager_ref.lock().unwrap();
        manager.status = ManagerStatus::Success(accumulated_logs);
    }
    Ok(())
}

// Wrapper function for backwards compatibility with non-TUI usage
pub async fn execute_manager_workflow_simple(manager: &mut DetectedManager) -> Result<()> {
    let manager_ref = Arc::new(Mutex::new(manager.clone()));
    execute_manager_workflow(manager_ref.clone()).await?;

    // Copy the updated state back
    let updated_manager = manager_ref.lock().unwrap();
    *manager = updated_manager.clone();

    Ok(())
}

async fn execute_command_with_logs(
    command: &str,
    requires_sudo: bool,
    timeout: Duration,
    manager_ref: Arc<Mutex<DetectedManager>>,
    operation: String,
    accumulated_logs: &mut String,
) -> Result<bool> {
    let mut cmd = build_command(command, requires_sudo)?;

    let mut child = cmd.spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to get stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to get stderr"))?;

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    let timeout_future = tokio::time::sleep(timeout);
    tokio::pin!(timeout_future);

    loop {
        tokio::select! {
            () = &mut timeout_future => {
                let _ = child.kill().await;
                accumulated_logs.push_str("\nERROR: Command timed out\n");
                let mut manager = manager_ref.lock().unwrap();
                manager.status = ManagerStatus::Failed(format!("Command timed out\n\nLogs:\n{}", accumulated_logs));
                return Err(anyhow::anyhow!("Command timed out"));
            }

            stdout_line = stdout_reader.next_line() => {
                match stdout_line {
                    Ok(Some(line)) => {
                        accumulated_logs.push_str(&line);
                        accumulated_logs.push('\n');

                        let mut manager = manager_ref.lock().unwrap();
                        manager.status = ManagerStatus::Running(operation.clone(), accumulated_logs.clone());
                    }
                    Ok(None) => {
                        // stdout closed
                    }
                    Err(e) => {
                        accumulated_logs.push_str(&format!("ERROR reading stdout: {}\n", e));
                        return Err(anyhow::anyhow!("Error reading stdout: {}", e));
                    }
                }
            }

            stderr_line = stderr_reader.next_line() => {
                match stderr_line {
                    Ok(Some(line)) => {
                        accumulated_logs.push_str("STDERR: ");
                        accumulated_logs.push_str(&line);
                        accumulated_logs.push('\n');

                        let mut manager = manager_ref.lock().unwrap();
                        manager.status = ManagerStatus::Running(operation.clone(), accumulated_logs.clone());
                    }
                    Ok(None) => {
                        // stderr closed
                    }
                    Err(e) => {
                        accumulated_logs.push_str(&format!("ERROR reading stderr: {}\n", e));
                        return Err(anyhow::anyhow!("Error reading stderr: {}", e));
                    }
                }
            }

            status = child.wait() => {
                match status {
                    Ok(exit_status) => {
                        let success = exit_status.success();
                        if !success {
                            accumulated_logs.push_str(&format!("\nCommand exited with code: {}\n", exit_status.code().unwrap_or(-1)));
                        }
                        return Ok(success);
                    }
                    Err(e) => {
                        accumulated_logs.push_str(&format!("ERROR waiting for command: {}\n", e));
                        return Err(anyhow::anyhow!("Error waiting for command: {}", e));
                    }
                }
            }
        }
    }
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
