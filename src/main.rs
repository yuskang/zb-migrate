//! Zerobrew Migration CLI
//!
//! A tool to migrate from Homebrew to Zerobrew

mod migrate;

use clap::{Parser, Subcommand};
use anyhow::Result;
use std::path::PathBuf;

use migrate::HomebrewMigrator;

#[derive(Parser)]
#[command(name = "zb-migrate")]
#[command(about = "Migrate from Homebrew to Zerobrew", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all installed Homebrew packages
    List {
        /// Include casks in the listing
        #[arg(long)]
        casks: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Export Homebrew packages to a Brewfile
    Export {
        /// Output file path (default: ./Brewfile)
        #[arg(short, long, default_value = "Brewfile")]
        output: PathBuf,
    },

    /// Migrate packages from Homebrew to Zerobrew
    Migrate {
        /// Dry run - show what would be migrated without making changes
        #[arg(long)]
        dry_run: bool,

        /// Migrate only specific packages
        #[arg(short, long)]
        packages: Option<Vec<String>>,
    },

    /// Check for available updates
    Outdated,

    /// Update all packages via Zerobrew
    Upgrade,

    /// Cleanup Homebrew after successful migration
    Cleanup {
        /// Force cleanup without confirmation
        #[arg(long)]
        force: bool,
    },

    /// Show migration status
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let migrator = HomebrewMigrator::new()?;

    match cli.command {
        Commands::List { casks, json } => {
            let formulae = migrator.list_installed_formulae()?;

            if json {
                let mut all_packages = formulae.clone();
                if casks {
                    all_packages.extend(migrator.list_installed_casks()?);
                }
                println!("{}", serde_json::to_string_pretty(&all_packages)?);
            } else {
                println!("Installed Homebrew Formulae ({}):", formulae.len());
                println!("{:-<50}", "");
                for pkg in &formulae {
                    let pinned = if pkg.pinned { " [pinned]" } else { "" };
                    let tap = pkg.tap.as_ref().map(|t| format!(" ({})", t)).unwrap_or_default();
                    println!("  {} @ {}{}{}", pkg.name, pkg.version, tap, pinned);
                }

                if casks {
                    let cask_list = migrator.list_installed_casks()?;
                    println!("\nInstalled Homebrew Casks ({}):", cask_list.len());
                    println!("{:-<50}", "");
                    for pkg in &cask_list {
                        println!("  {} @ {}", pkg.name, pkg.version);
                    }
                }
            }
        }

        Commands::Export { output } => {
            println!("Exporting to {}...", output.display());
            migrator.export_to_brewfile(&output)?;
            println!("Done! Brewfile created at {}", output.display());
        }

        Commands::Migrate { dry_run, packages } => {
            if let Some(pkg_names) = packages {
                // Migrate specific packages
                let all_formulae = migrator.list_installed_formulae()?;
                for name in pkg_names {
                    if let Some(pkg) = all_formulae.iter().find(|p| p.name == name) {
                        if dry_run {
                            println!("[DRY RUN] Would migrate: {} @ {}", pkg.name, pkg.version);
                        } else {
                            let result = migrator.migrate_package(pkg)?;
                            match result {
                                migrate::MigrateResult::Success { name, version } => {
                                    println!("✓ Migrated: {} @ {}", name, version);
                                }
                                migrate::MigrateResult::Failed { name, reason } => {
                                    println!("✗ Failed: {} - {}", name, reason);
                                }
                            }
                        }
                    } else {
                        println!("Package not found: {}", name);
                    }
                }
            } else {
                // Migrate all
                let report = migrator.migrate_all(dry_run)?;
                if !dry_run {
                    report.print_summary();
                }
            }
        }

        Commands::Outdated => {
            println!("Checking for updates...\n");
            let updates = migrator.check_updates()?;

            if updates.is_empty() {
                println!("All packages are up to date.");
            } else {
                println!("Available updates:");
                for update in &updates {
                    println!("  {} {} -> {}", update.name, update.current_version, update.new_version);
                }
            }
        }

        Commands::Upgrade => {
            migrator.update_all()?;
            println!("Upgrade complete!");
        }

        Commands::Cleanup { force } => {
            let state = migrator.load_state()?;
            let packages: Vec<String> = state.migrated_packages.keys().cloned().collect();

            if packages.is_empty() {
                println!("No migrated packages to clean up.");
            } else {
                migrator.cleanup_homebrew(&packages, force)?;
            }
        }

        Commands::Status => {
            let state = migrator.load_state()?;
            println!("Migration Status");
            println!("{:-<50}", "");
            println!("Migrated packages: {}", state.migrated_packages.len());
            println!("Failed packages: {}", state.failed_packages.len());

            if !state.migrated_packages.is_empty() {
                println!("\nMigrated:");
                for (name, pkg) in &state.migrated_packages {
                    println!("  {} @ {}", name, pkg.version);
                }
            }

            if !state.failed_packages.is_empty() {
                println!("\nFailed:");
                for name in &state.failed_packages {
                    println!("  {}", name);
                }
            }
        }
    }

    Ok(())
}
