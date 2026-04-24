use anyhow::Result;
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::io;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::detect::{DetectedManager, ManagerStatus};
use crate::execute::{execute_manager_workflow, execute_manager_workflow_simple};

mod config;
mod detect;
mod execute;
mod notify;
mod tui;

#[derive(Parser)]
#[command(name = "spn")]
#[command(about = "A meta package manager for Unix-like systems")]
struct Cli {
    #[arg(
        short,
        long,
        global = true,
        help = "Selective mode - wait for user to select which managers to update"
    )]
    selective: bool,
    #[arg(
        short = 'n',
        long = "no-tui",
        global = true,
        help = "Non-TUI mode - use spinners instead of interactive interface"
    )]
    no_tui: bool,
    #[arg(long, global = true, help = "Send notification when upgrade completes")]
    notify: bool,
    #[arg(
        long,
        global = true,
        help = "Preview detected managers and commands without running anything"
    )]
    dry_run: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Upgrade all package managers")]
    Upgrade,
    #[command(about = "Upgrade all package managers without the TUI")]
    Cli,
    #[command(about = "List detected package managers")]
    List,
    #[command(about = "Check Spine configuration, detection, sudo, and terminal readiness")]
    Doctor,
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
        Some(Commands::Upgrade) | None => {
            upgrade(cli.selective, cli.no_tui, cli.notify, cli.dry_run).await?;
        }
        Some(Commands::Cli) => {
            upgrade(cli.selective, true, cli.notify, cli.dry_run).await?;
        }
        Some(Commands::List) => {
            list_managers().await?;
        }
        Some(Commands::Doctor) => {
            doctor().await?;
        }
        Some(Commands::Auto {
            enable,
            disable,
            status,
        }) => {
            manage_auto_update(enable, disable, status).await?;
        }
    }

    Ok(())
}

async fn list_managers() -> Result<()> {
    let (config, config_path) = match config::load_config_with_path().await {
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

    println!("Config: {}", config_path.display());
    println!("Detected {} package manager(s):", managers.len());
    for manager in &managers {
        println!("  ✓ {} ({})", manager.name, manager.config.name);
        println!("    Check command: {}", manager.config.check_command);
        println!("    Requires sudo: {}", manager.config.requires_sudo);
        println!();
    }

    Ok(())
}

async fn upgrade(
    selective: bool,
    no_tui: bool,
    notify_on_complete: bool,
    dry_run: bool,
) -> Result<()> {
    // Load configuration with error handling
    let (config, config_path) = match config::load_config_with_path().await {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading configuration: {e}");
            eprintln!("Please ensure backbone.toml is available in the current directory or installed with the binary.");
            std::process::exit(1);
        }
    };

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

    println!("{}", bold("spine"));
    println!("  {} {}", dim("config"), config_path.display());
    println!(
        "  {} {} managers  {}",
        dim("found"),
        managers.len(),
        dim(managers
            .iter()
            .map(|m| &m.name)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "))
    );

    if dry_run {
        print_dry_run(&managers);
        return Ok(());
    }

    // Check for sudo availability if any managers require it
    let needs_sudo = managers.iter().any(|m| m.config.requires_sudo);
    let sudo_keepalive = if needs_sudo {
        execute::prepare_sudo_session().await?;
        Some(execute::spawn_sudo_keepalive())
    } else {
        None
    };

    println!("  {}\n", dim("starting upgrades"));

    // Choose between TUI and non-TUI workflow
    let result = if no_tui {
        run_spinner_upgrade(managers, selective).await
    } else {
        tui::run_tui(managers, config, selective).await
    };

    if let Some(running) = sudo_keepalive {
        execute::stop_sudo_keepalive(running);
    }

    match result {
        Ok(()) => {
            println!("{}", dim("upgrade process completed"));
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

fn summarize_error(error: &str) -> String {
    let title = error.lines().next().unwrap_or("failed").trim();
    let tail = error
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && trimmed != "Logs:"
                && !trimmed.starts_with("===")
                && !trimmed.starts_with('✓')
        })
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| line.trim().strip_prefix("STDERR: ").unwrap_or(line.trim()))
        .collect::<Vec<_>>();

    if tail.is_empty() {
        title.to_string()
    } else {
        format!("{} — {}", title, tail.join(" / "))
    }
}

fn use_color() -> bool {
    std::io::IsTerminal::is_terminal(&io::stdout())
}

fn paint(text: impl std::fmt::Display, code: &str) -> String {
    if use_color() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn dim(text: impl std::fmt::Display) -> String {
    paint(text, "2")
}

fn green(text: impl std::fmt::Display) -> String {
    paint(text, "32")
}

fn red(text: impl std::fmt::Display) -> String {
    paint(text, "31")
}

fn yellow(text: impl std::fmt::Display) -> String {
    paint(text, "33")
}

fn bold(text: impl std::fmt::Display) -> String {
    paint(text, "1")
}

fn print_dry_run(managers: &[DetectedManager]) {
    println!("\nDry run: no commands will be executed.\n");
    for manager in managers {
        println!("{} ({})", manager.name, manager.config.name);
        println!("  Requires sudo: {}", manager.config.requires_sudo);
        if let Some(refresh) = &manager.config.refresh {
            println!("  Refresh:       {}", refresh);
        }
        if let Some(self_update) = &manager.config.self_update {
            println!("  Self-update:   {}", self_update);
        }
        println!("  Upgrade:       {}", manager.config.upgrade_all);
        if let Some(cleanup) = &manager.config.cleanup {
            println!("  Cleanup:       {}", cleanup);
        }
        println!();
    }
}

async fn doctor() -> Result<()> {
    println!("Spine Doctor\n");

    let config_paths = config::config_paths();
    println!("Config search order:");
    for path in &config_paths {
        let marker = if path.exists() { "✓" } else { " " };
        println!("  {marker} {}", path.display());
    }

    let (config, config_path) = match config::load_config_with_path().await {
        Ok(config) => config,
        Err(e) => {
            println!("\n✗ Configuration failed: {e}");
            return Ok(());
        }
    };

    println!("\nActive config: {}", config_path.display());
    println!("Configured managers: {}", config.managers.len());

    let managers = match detect::detect_package_managers(&config).await {
        Ok(managers) => managers,
        Err(e) => {
            println!("✗ Detection failed: {e}");
            return Ok(());
        }
    };

    println!("Detected managers: {}", managers.len());
    for manager in &managers {
        let sudo = if manager.config.requires_sudo {
            " sudo"
        } else {
            ""
        };
        println!("  ✓ {}{}", manager.name, sudo);
    }

    let needs_sudo = managers.iter().any(|m| m.config.requires_sudo);
    if needs_sudo {
        if which::which("sudo").is_err() {
            println!("✗ sudo required by detected managers but sudo is not installed");
        } else if execute::check_sudo_availability().await {
            println!("✓ sudo is available without prompting");
        } else {
            println!("! sudo is installed but will prompt before upgrades");
        }
    } else {
        println!("✓ No detected managers require sudo");
    }

    if std::io::IsTerminal::is_terminal(&io::stdout()) {
        println!("✓ stdout is a terminal");
    } else {
        println!("! stdout is not a terminal; use spn cli or spn --no-tui");
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
        let shared_managers: Vec<Arc<Mutex<DetectedManager>>> = managers
            .into_iter()
            .map(|m| Arc::new(Mutex::new(m)))
            .collect();
        let mut join_set = JoinSet::new();

        for manager_ref in &shared_managers {
            let manager_ref = manager_ref.clone();
            join_set.spawn(async move {
                let name = manager_ref.lock().await.name.clone();
                println!("Starting {name}");
                let result = execute_manager_workflow(manager_ref.clone()).await;
                let manager = manager_ref.lock().await;
                match &manager.status {
                    ManagerStatus::Success => println!("{} {}", green("✓"), manager.name),
                    ManagerStatus::Failed(err) => {
                        println!(
                            "{} {} {}",
                            red("✗"),
                            manager.name,
                            dim(summarize_error(err))
                        )
                    }
                    _ => println!("{} {}", yellow("?"), manager.name),
                }
                result
            });
        }

        while let Some(result) = join_set.join_next().await {
            result??;
        }

        managers = Vec::new();
        for manager_ref in shared_managers {
            managers.push(manager_ref.lock().await.clone());
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
    let incomplete = total - successful - failed;

    println!();
    println!("{}", bold("spine summary"));
    println!(
        "  {} {} total  {} {} passed  {} {} failed{}",
        dim("·"),
        total,
        green("✓"),
        successful,
        red("✗"),
        failed,
        if incomplete > 0 {
            format!("  {} {} incomplete", yellow("?"), incomplete)
        } else {
            String::new()
        }
    );
    println!();

    for manager in managers {
        match &manager.status {
            ManagerStatus::Success => {
                println!(
                    "  [{}] {:<18} {}",
                    green(" ok "),
                    manager.name,
                    dim("complete")
                );
            }
            ManagerStatus::Failed(err) => {
                println!(
                    "  [{}] {:<18} {}",
                    red("fail"),
                    manager.name,
                    summarize_error(err)
                );
            }
            _ => {
                println!(
                    "  [{}] {:<18} {}",
                    yellow("skip"),
                    manager.name,
                    dim("incomplete")
                );
            }
        }
    }

    println!();
    if failed > 0 {
        println!(
            "{} {} failed. Open the TUI detail view for full logs, or rerun a manager manually.",
            yellow("hint:"),
            failed
        );
    } else if successful > 0 {
        println!("{} all package managers completed", green("result:"));
    }
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
