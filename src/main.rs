//! Zerobrew Migration CLI
//!
//! A tool to migrate from Homebrew to Zerobrew

mod migrate;

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::{style, set_colors_enabled};
use std::path::PathBuf;

use migrate::HomebrewMigrator;

#[derive(Parser)]
#[command(name = "zb-migrate")]
#[command(about = "Migrate from Homebrew to Zerobrew", long_about = None)]
struct Cli {
    /// Enable verbose output (show commands, output, and timing)
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

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

        /// Interactive mode - prompt before each package migration
        #[arg(short, long)]
        interactive: bool,
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

    /// Analyze packages and categorize by migration risk
    Analyze {
        /// Output as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle --no-color flag
    if cli.no_color {
        set_colors_enabled(false);
    }

    let migrator = HomebrewMigrator::new(cli.verbose)?;

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
                println!("{} {}",
                    style("📦 Homebrew Formulae").cyan().bold(),
                    style(format!("({})", formulae.len())).dim()
                );
                println!("{}", style("─".repeat(50)).dim());
                for pkg in &formulae {
                    let pinned = if pkg.pinned {
                        format!(" {}", style("[pinned]").yellow())
                    } else {
                        String::new()
                    };
                    let tap = pkg
                        .tap
                        .as_ref()
                        .map(|t| format!(" {}", style(format!("({})", t)).dim()))
                        .unwrap_or_default();
                    println!("  {:<28} {}{}{}",
                        style(&pkg.name).white().bold(),
                        style(&pkg.version).dim(),
                        tap,
                        pinned
                    );
                }

                if casks {
                    let cask_list = migrator.list_installed_casks()?;
                    println!("\n{} {}",
                        style("🖥️  Homebrew Casks").cyan().bold(),
                        style(format!("({})", cask_list.len())).dim()
                    );
                    println!("{}", style("─".repeat(50)).dim());
                    for pkg in &cask_list {
                        println!("  {:<28} {}",
                            style(&pkg.name).white().bold(),
                            style(&pkg.version).dim()
                        );
                    }
                }
            }
        }

        Commands::Export { output } => {
            println!("{} Exporting to {}...",
                style("→").cyan().bold(),
                style(output.display()).white()
            );
            migrator.export_to_brewfile(&output)?;
            println!("{} Brewfile created at {}",
                style("✓").green().bold(),
                style(output.display()).white().bold()
            );
        }

        Commands::Migrate { dry_run, packages, interactive } => {
            if let Some(pkg_names) = packages {
                // Migrate specific packages
                let all_formulae = migrator.list_installed_formulae()?;
                for name in pkg_names {
                    if let Some(pkg) = all_formulae.iter().find(|p| p.name == name) {
                        if dry_run {
                            println!("{} Would migrate: {} {}",
                                style("[DRY RUN]").yellow().bold(),
                                style(&pkg.name).white().bold(),
                                style(&pkg.version).dim()
                            );
                        } else {
                            let result = migrator.migrate_package(pkg)?;
                            match result {
                                migrate::MigrateResult::Success { name, version } => {
                                    println!("{} {} {} migrated successfully",
                                        style("✓").green().bold(),
                                        style(&name).white().bold(),
                                        style(&version).dim()
                                    );
                                }
                                migrate::MigrateResult::Failed { name, reason } => {
                                    println!("{} {} failed: {}",
                                        style("✗").red().bold(),
                                        style(&name).white().bold(),
                                        style(&reason).dim()
                                    );
                                }
                            }
                        }
                    } else {
                        println!("{} Package not found: {}",
                            style("✗").red().bold(),
                            style(&name).yellow()
                        );
                    }
                }
            } else if interactive && !dry_run {
                // Interactive migration mode
                let report = migrator.migrate_interactive()?;
                report.print_summary();
            } else {
                // Migrate all
                let report = migrator.migrate_all(dry_run)?;
                if !dry_run {
                    report.print_summary();
                }
            }
        }

        Commands::Outdated => {
            println!("{} Zerobrew does not currently support checking for updates.\n",
                style("ℹ").cyan().bold()
            );
            println!("To check for updates on packages still in Homebrew:");
            println!("  {}", style("brew outdated").white().bold());
            println!("\nTo update a Zerobrew package, reinstall it:");
            println!("  {}", style("zb uninstall <package>").white().bold());
            println!("  {}", style("zb install <package>").white().bold());
        }

        Commands::Upgrade => {
            println!("{} Zerobrew does not currently support bulk upgrades.\n",
                style("ℹ").cyan().bold()
            );
            println!("To upgrade packages still in Homebrew:");
            println!("  {}", style("brew upgrade").white().bold());
            println!("\nTo upgrade a Zerobrew package, reinstall it:");
            println!("  {}", style("zb uninstall <package>").white().bold());
            println!("  {}", style("zb install <package>").white().bold());
            println!("\nTo list installed Zerobrew packages:");
            println!("  {}", style("zb list").white().bold());
        }

        Commands::Cleanup { force } => {
            let state = migrator.load_state()?;
            let packages: Vec<String> = state.migrated_packages.keys().cloned().collect();

            if packages.is_empty() {
                println!("{} No migrated packages to clean up.",
                    style("ℹ").cyan().bold()
                );
            } else {
                migrator.cleanup_homebrew(&packages, force)?;
            }
        }

        Commands::Status => {
            let state = migrator.load_state()?;
            println!("{}", style("╭─ Migration Status ─────────────────────╮").cyan());
            println!("{}  {} Migrated:  {} packages              {}",
                style("│").cyan(),
                style("✓").green().bold(),
                style(state.migrated_packages.len()).white().bold(),
                style("│").cyan()
            );
            println!("{}  {} Failed:    {} packages              {}",
                style("│").cyan(),
                style("✗").red().bold(),
                style(state.failed_packages.len()).white().bold(),
                style("│").cyan()
            );
            println!("{}", style("╰────────────────────────────────────────╯").cyan());

            if !state.migrated_packages.is_empty() {
                println!("\n{}", style("Migrated:").green().bold());
                for (name, pkg) in &state.migrated_packages {
                    println!("  {:<28} {}",
                        style(name).white().bold(),
                        style(&pkg.version).dim()
                    );
                }
            }

            if !state.failed_packages.is_empty() {
                println!("\n{}", style("Failed:").red().bold());
                for name in &state.failed_packages {
                    println!("  {}", style(name).red());
                }
            }
        }

        Commands::Analyze { json } => {
            let report = migrator.analyze_packages()?;
            if json {
                println!("{}", report.to_json()?);
            } else {
                report.print_summary();
            }
        }
    }

    Ok(())
}
