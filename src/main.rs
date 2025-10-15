use anyhow::Result;
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::io;

use crate::detect::{DetectedManager, ManagerStatus};
use crate::execute::execute_manager_workflow_simple;

mod config;
mod detect;
mod execute;
mod notify;
mod tui;

#[derive(Parser)]
#[command(name = "spn")]
#[command(about = "A meta package manager for Unix-like systems")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Upgrade all package managers")]
    Upgrade {
        #[arg(
            short,
            long,
            help = "Selective mode - wait for user to select which managers to update"
        )]
        selective: bool,
        #[arg(
            short = 'n',
            long = "no-tui",
            help = "Non-TUI mode - use spinners instead of interactive interface"
        )]
        no_tui: bool,
        #[arg(long, help = "Send notification when upgrade completes")]
        notify: bool,
    },
    #[command(about = "List detected package managers")]
    List,
    #[command(about = "Enable or disable automatic background updates")]
    Auto {
        #[arg(long, help = "Enable automatic updates")]
        enable: bool,
        #[arg(long, help = "Disable automatic updates")]
        disable: bool,
        #[arg(long, help = "Show current auto-update status")]
        status: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Upgrade {
            selective,
            no_tui,
            notify,
        } => {
            upgrade(selective, no_tui, notify).await?;
        }
        Commands::List => {
            list_managers().await?;
        }
        Commands::Auto {
            enable,
            disable,
            status,
        } => {
            manage_auto_update(enable, disable, status).await?;
        }
    }

    Ok(())
}

async fn list_managers() -> Result<()> {
    let config = match config::load_config().await {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading configuration: {e}");
            eprintln!("Please ensure backbone.toml is available in the current directory or installed with the binary.");
            std::process::exit(1);
        }
    };

    let managers = match detect::detect_package_managers(&config).await {
        Ok(managers) => managers,
        Err(e) => {
            eprintln!("Error detecting package managers: {e}");
            std::process::exit(1);
        }
    };

    if managers.is_empty() {
        println!("No package managers detected on this system.");
        println!(
            "Spine checked for: {}",
            config
                .managers
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Ok(());
    }

    println!("Detected {} package manager(s):", managers.len());
    for manager in &managers {
        println!("  ✓ {} ({})", manager.name, manager.config.name);
        println!("    Check command: {}", manager.config.check_command);
        println!("    Requires sudo: {}", manager.config.requires_sudo);
        println!();
    }

    Ok(())
}

async fn upgrade(selective: bool, no_tui: bool, notify_on_complete: bool) -> Result<()> {
    // Load configuration with error handling
    let config = match config::load_config().await {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading configuration: {e}");
            eprintln!("Please ensure backbone.toml is available in the current directory or installed with the binary.");
            std::process::exit(1);
        }
    };

    // Check for sudo availability if any managers require it
    let requires_sudo = config.managers.values().any(|m| m.requires_sudo);
    if requires_sudo {
        match execute::check_sudo_availability().await {
            true => {}
            false => {
                eprintln!("Warning: Some package managers require sudo access.");
                eprintln!("Please ensure you have the necessary privileges or run with sudo.");
                eprintln!("Continuing anyway - some operations may fail...\n");
            }
        }
    }

    // Detect available package managers
    let managers = match detect::detect_package_managers(&config).await {
        Ok(managers) => managers,
        Err(e) => {
            eprintln!("Error detecting package managers: {e}");
            std::process::exit(1);
        }
    };

    if managers.is_empty() {
        println!("No package managers detected on this system.");
        println!(
            "Spine checked for: {}",
            config
                .managers
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Ok(());
    }

    println!(
        "Detected {} package manager(s): {}",
        managers.len(),
        managers
            .iter()
            .map(|m| &m.name)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("Starting upgrade process...\n");

    // Choose between TUI and non-TUI workflow
    let result = if no_tui {
        run_spinner_upgrade(managers, selective).await
    } else {
        tui::run_tui(managers, config, selective).await
    };

    match result {
        Ok(()) => {
            println!("Upgrade process completed.");
            if notify_on_complete {
                let _ = notify::send_notification(
                    "Spine Update Complete",
                    "All package managers have been updated successfully.",
                );
            }
        }
        Err(e) => {
            eprintln!("Error during upgrade process: {e}");
            if notify_on_complete {
                let _ = notify::send_notification(
                    "Spine Update Failed",
                    "Package manager updates encountered errors.",
                );
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn run_spinner_upgrade(mut managers: Vec<DetectedManager>, selective: bool) -> Result<()> {
    println!("Running package manager upgrades...\n");

    if selective {
        // In selective mode, prompt for each manager
        let mut i = 0;
        while i < managers.len() {
            println!("Run upgrade for {} (y/N)?", managers[i].name);
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes" {
                run_manager_with_spinner(&mut managers[i]).await?;
            } else {
                println!("Skipping {}\n", managers[i].name);
            }
            i += 1;
        }
    } else {
        // Run all managers sequentially
        for manager in managers.iter_mut() {
            run_manager_with_spinner(manager).await?;
        }
    }

    // Print summary using the same function as TUI
    print_spinner_summary(&managers);

    Ok(())
}

async fn run_manager_with_spinner(manager: &mut DetectedManager) -> Result<()> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
            .template("{spinner:.green} {msg}")?,
    );

    pb.set_message(format!("Starting {}", manager.name));
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    // Execute the manager workflow
    let result = execute_manager_workflow_simple(manager).await;

    pb.finish_with_message(match &manager.status {
        ManagerStatus::Success => format!("✓ {} completed successfully", manager.name),
        ManagerStatus::Failed(err) => format!("✗ {} failed: {}", manager.name, err),
        _ => format!("? {} finished with unknown status", manager.name),
    });

    println!();

    result
}

fn print_spinner_summary(managers: &[DetectedManager]) {
    let total = managers.len();
    let successful = managers
        .iter()
        .filter(|m| matches!(m.status, ManagerStatus::Success))
        .count();
    let failed = managers
        .iter()
        .filter(|m| matches!(m.status, ManagerStatus::Failed(_)))
        .count();

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("                           SPINE UPGRADE SUMMARY");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("\nOverall Results:");
    println!("  Total Managers:    {total}");
    println!(
        "  ✓ Successful:      {} ({:.1}%)",
        successful,
        (successful as f32 / total as f32) * 100.0
    );
    println!(
        "  ✗ Failed:          {} ({:.1}%)",
        failed,
        (failed as f32 / total as f32) * 100.0
    );

    println!("\nDetailed Results:");
    for manager in managers {
        match &manager.status {
            ManagerStatus::Success => {
                println!("  ✓ {:<20} Success", manager.name);
            }
            ManagerStatus::Failed(err) => {
                println!("  ✗ {:<20} Failed", manager.name);
                println!("    └─ Error: {err}");
            }
            _ => {
                println!("  ? {:<20} Incomplete", manager.name);
            }
        }
    }

    if failed > 0 {
        println!("\n⚠️  Some package managers failed to upgrade completely.");
        println!("   Check the error details above and consider running 'spn upgrade' again.");
        println!("   You may also need to run the failed managers manually with sudo privileges.");
    } else if successful > 0 {
        println!("\n🎉 All package managers upgraded successfully!");
        println!("   Your system is now up to date.");
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

async fn manage_auto_update(enable: bool, disable: bool, status_only: bool) -> Result<()> {
    let config = config::load_config().await?;

    if status_only {
        print_auto_update_status(&config);
        return Ok(());
    }

    if !enable && !disable {
        print_auto_update_status(&config);
        eprintln!("\nUse --enable or --disable to change settings");
        eprintln!("Edit ~/.config/spine/backbone.toml to configure schedule");
        return Ok(());
    }

    if enable {
        enable_auto_update(&config).await?;
    } else if disable {
        disable_auto_update().await?;
    }

    Ok(())
}

fn print_auto_update_status(config: &config::Config) {
    println!("Auto-Update Status:");
    println!(
        "  Enabled:      {}",
        if config.auto_update.enabled {
            "✓ Yes"
        } else {
            "✗ No"
        }
    );
    println!("  Schedule:     {}", config.auto_update.schedule);

    if config.auto_update.schedule == "daily" {
        println!("  Time:         {}", config.auto_update.time);
    } else {
        println!("  Day:          {}", config.auto_update.day);
        println!("  Time:         18:00");
    }

    println!(
        "  Notifications: {}",
        if config.auto_update.notify {
            "✓ Enabled"
        } else {
            "✗ Disabled"
        }
    );
    println!(
        "  Mode:         {}",
        if config.auto_update.no_tui {
            "Background"
        } else {
            "Interactive"
        }
    );
}

async fn enable_auto_update(config: &config::Config) -> Result<()> {
    let binary_path = std::env::current_exe()?;

    if config.auto_update.schedule == "daily" {
        setup_daily_auto_update(
            &config.auto_update.time,
            &binary_path,
            config.auto_update.notify,
        )?;
        println!(
            "✓ Enabled automatic daily updates at {}",
            config.auto_update.time
        );
    } else {
        setup_weekly_auto_update(
            &config.auto_update.day,
            &binary_path,
            config.auto_update.notify,
        )?;
        println!(
            "✓ Enabled automatic weekly updates on {}",
            config.auto_update.day
        );
    }

    println!("\nUpdates will run in the background.");
    if config.auto_update.notify {
        println!("You'll receive a notification when complete.");
    }

    Ok(())
}

async fn disable_auto_update() -> Result<()> {
    remove_auto_update_schedule()?;
    println!("✓ Disabled automatic updates");
    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_daily_auto_update(time: &str, binary_path: &std::path::Path, notify: bool) -> Result<()> {
    use std::env;
    use std::fs;

    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid time format. Use HH:MM (e.g., 18:00)");
    }

    let hour = parts[0];
    let minute = parts[1];

    let notify_flag = if notify { " --notify" } else { "" };
    let binary_path_str = binary_path.to_string_lossy();

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.spine.auto-update</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary_path_str}</string>
        <string>upgrade</string>
        <string>--no-tui</string>{notify_flag}
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Hour</key>
        <integer>{hour}</integer>
        <key>Minute</key>
        <integer>{minute}</integer>
    </dict>
    <key>StandardOutPath</key>
    <string>/tmp/spine-auto-update.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/spine-auto-update-error.log</string>
</dict>
</plist>"#
    );

    let home = env::var("HOME")?;
    let plist_path = format!("{home}/Library/LaunchAgents/com.spine.auto-update.plist");
    fs::write(&plist_path, plist_content)?;

    std::process::Command::new("launchctl")
        .args(["load", "-w", &plist_path])
        .output()?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn setup_daily_auto_update(time: &str, binary_path: &std::path::Path, notify: bool) -> Result<()> {
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid time format. Use HH:MM (e.g., 18:00)");
    }

    let hour = parts[0];
    let minute = parts[1];

    let notify_flag = if notify { " --notify" } else { "" };
    let binary_path_str = binary_path.to_string_lossy();

    let cron_entry = format!(
        "{minute} {hour} * * * {binary_path_str} upgrade --no-tui{notify_flag} >> /tmp/spine-auto-update.log 2>&1\n"
    );

    let output = std::process::Command::new("crontab").arg("-l").output();

    let mut current_crontab = if output.is_ok() {
        String::from_utf8_lossy(&output.unwrap().stdout).to_string()
    } else {
        String::new()
    };

    current_crontab = current_crontab
        .lines()
        .filter(|line| !line.contains("spine") && !line.contains("spn"))
        .collect::<Vec<_>>()
        .join("\n");

    if !current_crontab.is_empty() && !current_crontab.ends_with('\n') {
        current_crontab.push('\n');
    }
    current_crontab.push_str(&cron_entry);

    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(current_crontab.as_bytes())?;
    child.wait()?;

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn setup_daily_auto_update(
    _time: &str,
    _binary_path: &std::path::Path,
    _notify: bool,
) -> Result<()> {
    anyhow::bail!("Auto-update is only supported on macOS and Linux")
}

#[cfg(target_os = "macos")]
fn setup_weekly_auto_update(day: &str, binary_path: &std::path::Path, notify: bool) -> Result<()> {
    let weekday = match day.to_lowercase().as_str() {
        "monday" => 1,
        "tuesday" => 2,
        "wednesday" => 3,
        "thursday" => 4,
        "friday" => 5,
        "saturday" => 6,
        "sunday" => 7,
        _ => anyhow::bail!(
            "Invalid day. Use: monday, tuesday, wednesday, thursday, friday, saturday, sunday"
        ),
    };

    let notify_flag = if notify { " --notify" } else { "" };
    let binary_path_str = binary_path.to_string_lossy();

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.spine.auto-update</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary_path_str}</string>
        <string>upgrade</string>
        <string>--no-tui</string>{notify_flag}
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Weekday</key>
        <integer>{weekday}</integer>
        <key>Hour</key>
        <integer>18</integer>
        <key>Minute</key>
        <integer>0</integer>
    </dict>
    <key>StandardOutPath</key>
    <string>/tmp/spine-auto-update.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/spine-auto-update-error.log</string>
</dict>
</plist>"#
    );

    use std::env;
    use std::fs;
    let home = env::var("HOME")?;
    let plist_path = format!("{home}/Library/LaunchAgents/com.spine.auto-update.plist");
    fs::write(&plist_path, plist_content)?;

    std::process::Command::new("launchctl")
        .args(["load", "-w", &plist_path])
        .output()?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn setup_weekly_auto_update(day: &str, binary_path: &std::path::Path, notify: bool) -> Result<()> {
    let weekday = match day.to_lowercase().as_str() {
        "monday" => "1",
        "tuesday" => "2",
        "wednesday" => "3",
        "thursday" => "4",
        "friday" => "5",
        "saturday" => "6",
        "sunday" => "0",
        _ => anyhow::bail!(
            "Invalid day. Use: monday, tuesday, wednesday, thursday, friday, saturday, sunday"
        ),
    };

    let notify_flag = if notify { " --notify" } else { "" };
    let binary_path_str = binary_path.to_string_lossy();

    let cron_entry = format!(
        "0 18 * * {weekday} {binary_path_str} upgrade --no-tui{notify_flag} >> /tmp/spine-auto-update.log 2>&1\n"
    );

    let output = std::process::Command::new("crontab").arg("-l").output();

    let mut current_crontab = if output.is_ok() {
        String::from_utf8_lossy(&output.unwrap().stdout).to_string()
    } else {
        String::new()
    };

    current_crontab = current_crontab
        .lines()
        .filter(|line| !line.contains("spine") && !line.contains("spn"))
        .collect::<Vec<_>>()
        .join("\n");

    if !current_crontab.is_empty() && !current_crontab.ends_with('\n') {
        current_crontab.push('\n');
    }
    current_crontab.push_str(&cron_entry);

    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(current_crontab.as_bytes())?;
    child.wait()?;

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn setup_weekly_auto_update(
    _day: &str,
    _binary_path: &std::path::Path,
    _notify: bool,
) -> Result<()> {
    anyhow::bail!("Auto-update is only supported on macOS and Linux")
}

#[cfg(target_os = "macos")]
fn remove_auto_update_schedule() -> Result<()> {
    use std::env;
    let home = env::var("HOME")?;

    let plist_path = format!("{home}/Library/LaunchAgents/com.spine.auto-update.plist");

    if std::path::Path::new(&plist_path).exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path])
            .output();
        let _ = std::fs::remove_file(&plist_path);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_auto_update_schedule() -> Result<()> {
    let output = std::process::Command::new("crontab").arg("-l").output();

    if output.is_ok() {
        let current_crontab = String::from_utf8_lossy(&output.unwrap().stdout);
        let filtered: String = current_crontab
            .lines()
            .filter(|line| !line.contains("spine") && !line.contains("spn"))
            .collect::<Vec<_>>()
            .join("\n");

        let mut child = std::process::Command::new("crontab")
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .spawn()?;

        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(filtered.as_bytes())?;
        child.wait()?;
    }

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remove_auto_update_schedule() -> Result<()> {
    anyhow::bail!("Auto-update is only supported on macOS and Linux")
}
