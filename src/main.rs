use anyhow::Result;
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::io;

use crate::detect::{DetectedManager, ManagerStatus};
use crate::execute::execute_manager_workflow_simple;

mod config;
mod detect;
mod execute;
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
    },
    #[command(about = "List detected package managers")]
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Upgrade { selective, no_tui } => {
            upgrade(selective, no_tui).await?;
        }
        Commands::List => {
            list_managers().await?;
        }
    }

    Ok(())
}

async fn list_managers() -> Result<()> {
    let config = match config::load_config().await {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading configuration: {}", e);
            eprintln!("Please ensure backbone.toml is available in the current directory or installed with the binary.");
            std::process::exit(1);
        }
    };

    let managers = match detect::detect_package_managers(&config).await {
        Ok(managers) => managers,
        Err(e) => {
            eprintln!("Error detecting package managers: {}", e);
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

async fn upgrade(selective: bool, no_tui: bool) -> Result<()> {
    // Load configuration with error handling
    let config = match config::load_config().await {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading configuration: {}", e);
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
            eprintln!("Error detecting package managers: {}", e);
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
    if no_tui {
        match run_spinner_upgrade(managers, selective).await {
            Ok(()) => {
                println!("Upgrade process completed.");
            }
            Err(e) => {
                eprintln!("Error during upgrade process: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        match tui::run_tui(managers, config, selective).await {
            Ok(()) => {
                println!("Upgrade process completed.");
            }
            Err(e) => {
                eprintln!("Error during upgrade process: {}", e);
                std::process::exit(1);
            }
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
    println!("  Total Managers:    {}", total);
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
                println!("    └─ Error: {}", err);
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
