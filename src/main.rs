use anyhow::Result;
use clap::{Parser, Subcommand};

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
        #[arg(short, long, help = "Selective mode - wait for user to select which managers to update")]
        selective: bool,
    },
    #[command(about = "List detected package managers")]
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Upgrade { selective } => {
            upgrade(selective).await?;
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

async fn upgrade(selective: bool) -> Result<()> {
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

    // Run the TUI workflow
    match tui::run_tui(managers, config, selective).await {
        Ok(()) => {
            println!("Upgrade process completed.");
        }
        Err(e) => {
            eprintln!("Error during upgrade process: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}
