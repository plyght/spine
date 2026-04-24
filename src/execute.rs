use crate::detect::{DetectedManager, ManagerStatus};
use anyhow::Result;
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;

pub async fn execute_manager_workflow(manager_ref: Arc<Mutex<DetectedManager>>) -> Result<()> {
    let config = {
        let manager = manager_ref.lock().await;
        manager.config.clone()
    };

    let mut accumulated_logs = String::new();

    // Refresh repositories
    if let Some(refresh_cmd) = &config.refresh {
        accumulated_logs.push_str("=== REFRESHING REPOSITORIES ===\n");
        {
            let mut manager = manager_ref.lock().await;
            manager.status = ManagerStatus::Running("Refreshing".to_string());
            manager.logs = accumulated_logs.clone();
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
                let mut manager = manager_ref.lock().await;
                manager.status = ManagerStatus::Failed(format!(
                    "Refresh command failed\n\nLogs:\n{accumulated_logs}"
                ));
                return Ok(());
            }
            Err(e) => {
                let mut manager = manager_ref.lock().await;
                manager.status = ManagerStatus::Failed(format!(
                    "Refresh error: {e}\n\nLogs:\n{accumulated_logs}"
                ));
                return Ok(());
            }
        }
    }

    // Self-update
    if let Some(self_update_cmd) = &config.self_update {
        accumulated_logs.push_str("=== SELF-UPDATE ===\n");
        {
            let mut manager = manager_ref.lock().await;
            manager.status = ManagerStatus::Running("Self-updating".to_string());
            manager.logs = accumulated_logs.clone();
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
                let mut manager = manager_ref.lock().await;
                manager.status = ManagerStatus::Failed(format!(
                    "Self-update command failed\n\nLogs:\n{accumulated_logs}"
                ));
                return Ok(());
            }
            Err(e) => {
                let mut manager = manager_ref.lock().await;
                manager.status = ManagerStatus::Failed(format!(
                    "Self-update error: {e}\n\nLogs:\n{accumulated_logs}"
                ));
                return Ok(());
            }
        }
    }

    // Upgrade all packages
    accumulated_logs.push_str("=== UPGRADING PACKAGES ===\n");
    {
        let mut manager = manager_ref.lock().await;
        manager.status = ManagerStatus::Running("Upgrading".to_string());
        manager.logs = accumulated_logs.clone();
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
            let mut manager = manager_ref.lock().await;
            manager.status = ManagerStatus::Failed(format!(
                "Upgrade command failed\n\nLogs:\n{accumulated_logs}"
            ));
            return Ok(());
        }
        Err(e) => {
            let mut manager = manager_ref.lock().await;
            manager.status =
                ManagerStatus::Failed(format!("Upgrade error: {e}\n\nLogs:\n{accumulated_logs}"));
            return Ok(());
        }
    }

    // Cleanup
    if let Some(cleanup_cmd) = &config.cleanup {
        accumulated_logs.push_str("=== CLEANUP ===\n");
        {
            let mut manager = manager_ref.lock().await;
            manager.status = ManagerStatus::Running("Cleaning".to_string());
            manager.logs = accumulated_logs.clone();
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
                let mut manager = manager_ref.lock().await;
                manager.status = ManagerStatus::Failed(format!(
                    "Cleanup command failed\n\nLogs:\n{accumulated_logs}"
                ));
                return Ok(());
            }
            Err(e) => {
                let mut manager = manager_ref.lock().await;
                manager.status = ManagerStatus::Failed(format!(
                    "Cleanup error: {e}\n\nLogs:\n{accumulated_logs}"
                ));
                return Ok(());
            }
        }
    }

    // Set final success status with complete logs
    {
        let mut manager = manager_ref.lock().await;
        manager.status = ManagerStatus::Success;
        manager.logs = accumulated_logs;
    }
    Ok(())
}

// Wrapper function for backwards compatibility with non-TUI usage
pub async fn execute_manager_workflow_simple(manager: &mut DetectedManager) -> Result<()> {
    let manager_ref = Arc::new(Mutex::new(manager.clone()));
    execute_manager_workflow(manager_ref.clone()).await?;

    // Copy the updated state back
    let updated_manager = manager_ref.lock().await;
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

    let shared_logs = Arc::new(Mutex::new(accumulated_logs.clone()));
    let stdout_task = tokio::spawn(read_output_stream(
        stdout,
        None,
        operation.clone(),
        manager_ref.clone(),
        shared_logs.clone(),
    ));
    let stderr_task = tokio::spawn(read_output_stream(
        stderr,
        Some("STDERR: "),
        operation,
        manager_ref.clone(),
        shared_logs.clone(),
    ));

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            let mut logs = shared_logs.lock().await;
            logs.push_str(&format!("ERROR waiting for command: {e}\n"));
            *accumulated_logs = logs.clone();
            return Err(anyhow::anyhow!("Error waiting for command: {e}"));
        }
        Err(_) => {
            let _ = child.kill().await;
            let mut logs = shared_logs.lock().await;
            logs.push_str("\nERROR: Command timed out\n");
            *accumulated_logs = logs.clone();
            let mut manager = manager_ref.lock().await;
            manager.status =
                ManagerStatus::Failed(format!("Command timed out\n\nLogs:\n{accumulated_logs}"));
            return Err(anyhow::anyhow!("Command timed out"));
        }
    };

    stdout_task.await??;
    stderr_task.await??;

    let mut logs = shared_logs.lock().await;
    let success = status.success();
    if !success {
        logs.push_str(&format!(
            "\nCommand exited with code: {}\n",
            status.code().unwrap_or(-1)
        ));
    }
    *accumulated_logs = logs.clone();
    Ok(success)
}

async fn read_output_stream<T>(
    mut stream: T,
    prefix: Option<&'static str>,
    operation: String,
    manager_ref: Arc<Mutex<DetectedManager>>,
    shared_logs: Arc<Mutex<String>>,
) -> Result<()>
where
    T: tokio::io::AsyncRead + Unpin,
{
    let mut buffer = [0; 4096];

    loop {
        let bytes_read = stream.read(&mut buffer).await?;
        if bytes_read == 0 {
            return Ok(());
        }

        let chunk = String::from_utf8_lossy(&buffer[..bytes_read]);
        let mut logs = shared_logs.lock().await;
        if let Some(prefix) = prefix {
            if logs.ends_with('\n') || logs.is_empty() {
                logs.push_str(prefix);
            }
            for (index, part) in chunk.split_inclusive('\n').enumerate() {
                if index > 0 {
                    logs.push_str(prefix);
                }
                logs.push_str(part);
            }
        } else {
            logs.push_str(&chunk);
        }

        let mut manager = manager_ref.lock().await;
        manager.status = ManagerStatus::Running(operation.clone());
        manager.logs = logs.clone();
    }
}

fn build_command(command: &str, requires_sudo: bool) -> Result<Command> {
    if command.is_empty() {
        anyhow::bail!("Empty command");
    }

    let mut cmd = if requires_sudo {
        if which::which("sudo").is_err() {
            anyhow::bail!("sudo is required but not available");
        }
        let mut c = Command::new("sudo");
        c.arg("-n");
        c.arg("--preserve-env=HOME,PATH,USER,LOGNAME,SHELL,TERM");
        c.arg("sh");
        c.arg("-c");
        c.arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c");
        c.arg(command);
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

pub async fn prepare_sudo_session() -> Result<()> {
    if which::which("sudo").is_err() {
        anyhow::bail!("sudo is required but not available");
    }

    if check_sudo_availability().await {
        return Ok(());
    }

    println!("  \x1b[2msudo\x1b[0m administrator privileges needed; authenticating once before the TUI\n");

    let status = Command::new("sudo").arg("-v").status().await?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("sudo authentication failed");
    }
}

pub fn spawn_sudo_keepalive() -> Arc<AtomicBool> {
    let running = Arc::new(AtomicBool::new(true));
    let keepalive_running = running.clone();

    tokio::spawn(async move {
        while keepalive_running.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if !keepalive_running.load(Ordering::Relaxed) {
                break;
            }
            let _ = Command::new("sudo")
                .args(["-n", "-v"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
    });

    running
}

pub fn stop_sudo_keepalive(running: Arc<AtomicBool>) {
    running.store(false, Ordering::Relaxed);
}
