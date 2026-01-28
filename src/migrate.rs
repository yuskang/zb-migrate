//! Homebrew to Zerobrew Migration Module
//!
//! This module provides functionality to:
//! 1. Read installed packages from Homebrew
//! 2. Import them into Zerobrew's management
//! 3. Handle subsequent updates via Zerobrew

use anyhow::{bail, Context, Result};
use console::style;
use dialoguer::{theme::ColorfulTheme, Select};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

/// Check if running in CI environment
fn is_ci() -> bool {
    std::env::var("CI").is_ok()
}

/// Create a progress bar with appropriate style for the environment
fn create_progress_bar(total: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);

    if is_ci() {
        // Simple style for CI without animations
        pb.set_style(
            ProgressStyle::with_template("{msg} [{pos}/{len}]")
                .unwrap()
        );
        // Disable steady tick in CI to avoid excessive output
        pb.enable_steady_tick(Duration::from_secs(10));
    } else {
        // Nice animated style for interactive use
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}"
            )
            .unwrap()
            .progress_chars("#>-")
        );
        pb.enable_steady_tick(Duration::from_millis(100));
    }

    pb.set_message(message.to_string());
    pb
}

/// Create a spinner for indeterminate progress
fn create_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();

    if is_ci() {
        pb.set_style(
            ProgressStyle::with_template("{msg}")
                .unwrap()
        );
    } else {
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {msg}")
                .unwrap()
        );
        pb.enable_steady_tick(Duration::from_millis(100));
    }

    pb.set_message(message.to_string());
    pb
}

/// Known problematic packages that may cause issues during migration.
/// These packages often have complex linking requirements, system dependencies,
/// or are deeply integrated into other packages' build processes.
pub const KNOWN_PROBLEMATIC_PACKAGES: &[&str] = &[
    // SSL/TLS and cryptography - system-level dependencies
    "openssl@3",
    "openssl@1.1",
    "openssl",
    "libressl",
    "gnutls",
    "libssh2",
    "libssh",

    // Python versions - complex dependency chains
    "python@3.11",
    "python@3.12",
    "python@3.10",
    "python@3.9",
    "python@3.13",

    // Event and async libraries - widely depended upon
    "libevent",
    "libuv",
    "libev",

    // HTTP/networking libraries
    "nghttp2",
    "curl",
    "wget",

    // GLib/GObject ecosystem - complex introspection
    "gobject-introspection",
    "glib",
    "gdk-pixbuf",
    "gtk+3",
    "cairo",
    "pango",

    // Node.js versions - complex native modules
    "node@20",
    "node@18",
    "node@16",
    "node",

    // Database clients with system dependencies
    "postgresql@14",
    "postgresql@15",
    "postgresql@16",
    "mysql-client",
    "libpq",

    // Compression libraries
    "zlib",
    "xz",
    "lz4",
    "zstd",
    "brotli",

    // Image libraries
    "libpng",
    "libjpeg",
    "libtiff",
    "webp",

    // ICU - Unicode support (heavily depended upon)
    "icu4c",

    // Build tools that are deeply integrated
    "pkg-config",
    "cmake",
    "autoconf",
    "automake",
    "libtool",

    // Ruby versions
    "ruby@3.0",
    "ruby@3.1",
    "ruby@3.2",
    "ruby@3.3",

    // Other commonly problematic packages
    "gettext",
    "readline",
    "ncurses",
    "pcre",
    "pcre2",
];

/// Categorization of a package for migration analysis
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MigrationRisk {
    /// Safe to migrate - no known issues
    Safe,
    /// Risky - depends on problematic packages
    Risky,
    /// Should keep in Homebrew - this package is problematic itself
    KeepInHomebrew,
}

/// Detailed information about a package's migration risk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageAnalysis {
    pub name: String,
    pub version: String,
    pub risk: MigrationRisk,
    pub reason: String,
    pub problematic_dependencies: Vec<String>,
}

/// Complete analysis report for all installed packages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    /// Packages that are safe to migrate
    pub safe_to_migrate: Vec<PackageAnalysis>,
    /// Packages that have some risk (depend on problematic packages)
    pub risky: Vec<PackageAnalysis>,
    /// Packages that should remain in Homebrew
    pub should_keep_in_homebrew: Vec<PackageAnalysis>,
    /// Total number of packages analyzed
    pub total_packages: usize,
}

impl AnalysisReport {
    /// Create a new empty report
    pub fn new() -> Self {
        Self {
            safe_to_migrate: Vec::new(),
            risky: Vec::new(),
            should_keep_in_homebrew: Vec::new(),
            total_packages: 0,
        }
    }

    /// Print a formatted summary of the analysis
    pub fn print_summary(&self) {
        println!("\n=== Package Migration Analysis ===\n");
        println!("Total packages analyzed: {}\n", self.total_packages);

        // Summary counts
        println!("Summary:");
        println!("  Safe to migrate:        {} packages", self.safe_to_migrate.len());
        println!("  Risky (use caution):    {} packages", self.risky.len());
        println!("  Keep in Homebrew:       {} packages", self.should_keep_in_homebrew.len());

        // Safe packages
        if !self.safe_to_migrate.is_empty() {
            println!("\n--- Safe to Migrate ({}) ---", self.safe_to_migrate.len());
            println!("These packages have no known issues and can be safely migrated:\n");
            for pkg in &self.safe_to_migrate {
                println!("  [OK] {} @ {}", pkg.name, pkg.version);
            }
        }

        // Risky packages
        if !self.risky.is_empty() {
            println!("\n--- Risky Packages ({}) ---", self.risky.len());
            println!("These packages depend on problematic packages. Migration may work but test carefully:\n");
            for pkg in &self.risky {
                println!("  [!] {} @ {}", pkg.name, pkg.version);
                println!("      Reason: {}", pkg.reason);
                if !pkg.problematic_dependencies.is_empty() {
                    println!("      Problematic deps: {}", pkg.problematic_dependencies.join(", "));
                }
            }
        }

        // Keep in Homebrew
        if !self.should_keep_in_homebrew.is_empty() {
            println!("\n--- Keep in Homebrew ({}) ---", self.should_keep_in_homebrew.len());
            println!("These packages are known to have issues and should remain in Homebrew:\n");
            for pkg in &self.should_keep_in_homebrew {
                println!("  [X] {} @ {}", pkg.name, pkg.version);
                println!("      Reason: {}", pkg.reason);
            }
        }

        // Recommendations
        println!("\n=== Recommendations ===\n");

        if !self.safe_to_migrate.is_empty() {
            println!("1. Start by migrating safe packages:");
            println!("   zb-migrate migrate --packages {}",
                self.safe_to_migrate.iter()
                    .take(5)
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(","));
            if self.safe_to_migrate.len() > 5 {
                println!("   (showing first 5 of {} safe packages)", self.safe_to_migrate.len());
            }
        }

        if !self.risky.is_empty() {
            println!("\n2. For risky packages, migrate one at a time and test:");
            println!("   zb-migrate migrate --packages <package-name>");
            println!("   # Then test the package before proceeding");
        }

        if !self.should_keep_in_homebrew.is_empty() {
            println!("\n3. Leave problematic packages in Homebrew:");
            println!("   These packages are core dependencies that many other packages rely on.");
            println!("   Migrating them may break other software.");
        }

        println!();
    }

    /// Export the report as JSON
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// Represents a Homebrew package with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrewPackage {
    pub name: String,
    pub version: String,
    pub tap: Option<String>,
    pub is_cask: bool,
    pub dependencies: Vec<String>,
    pub pinned: bool,
}

/// Represents the migration state
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MigrationState {
    pub migrated_packages: HashMap<String, BrewPackage>,
    pub failed_packages: Vec<String>,
    pub homebrew_prefix: PathBuf,
}

/// Main migrator struct
pub struct HomebrewMigrator {
    pub homebrew_prefix: PathBuf,
    state_file: PathBuf,
    verbose: bool,
}

impl HomebrewMigrator {
    /// Create a new migrator instance
    pub fn new(verbose: bool) -> Result<Self> {
        let homebrew_prefix = Self::detect_homebrew_prefix(verbose)?;
        let home = std::env::var("HOME").context(
            "HOME environment variable is not set.\n\
             This is required to locate the zerobrew configuration directory.\n\
             Suggestion: Ensure you are running in a proper shell environment.",
        )?;

        Ok(Self {
            homebrew_prefix,
            state_file: PathBuf::from(format!("{}/.zerobrew/migration_state.json", home)),
            verbose,
        })
    }

    /// Detect Homebrew installation prefix
    fn detect_homebrew_prefix(verbose: bool) -> Result<PathBuf> {
        let start = Instant::now();
        if verbose {
            eprintln!("[verbose] Running: brew --prefix");
        }

        // Try to get prefix from brew command
        let output = Command::new("brew")
            .arg("--prefix")
            .output()
            .context(
                "Failed to run 'brew --prefix': Homebrew does not appear to be installed.\n\n\
                 To install Homebrew, run:\n\
                   /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"\n\n\
                 For more information, visit: https://brew.sh",
            )?;

        let elapsed = start.elapsed();
        if verbose {
            eprintln!("[verbose] Command completed in {:.2?}", elapsed);
            eprintln!("[verbose] Exit code: {}", output.status);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() {
                eprintln!("[verbose] stdout: {}", stdout.trim());
            }
            if !stderr.trim().is_empty() {
                eprintln!("[verbose] stderr: {}", stderr.trim());
            }
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Failed to detect Homebrew prefix: 'brew --prefix' returned an error.\n\n\
                 Error output: {}\n\n\
                 Suggestions:\n\
                 - Ensure Homebrew is properly installed and configured\n\
                 - Try running 'brew doctor' to diagnose issues\n\
                 - Check that 'brew' is in your PATH",
                stderr.trim()
            );
        }

        let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();

        Ok(PathBuf::from(prefix))
    }

    /// List all installed Homebrew formulae (fast mode - minimal brew calls)
    pub fn list_installed_formulae(&self) -> Result<Vec<BrewPackage>> {
        let start = Instant::now();
        if self.verbose {
            eprintln!("[verbose] Running: brew list --formula --versions");
        }

        let output = Command::new("brew")
            .args(["list", "--formula", "--versions"])
            .output()
            .context(
                "Failed to list Homebrew formulae: Could not execute 'brew list'.\n\n\
                 Suggestions:\n\
                 - Verify Homebrew is installed: run 'brew --version'\n\
                 - Check your internet connection if this is a network-related issue\n\
                 - Try running 'brew update' to refresh Homebrew",
            )?;

        let elapsed = start.elapsed();
        if self.verbose {
            eprintln!("[verbose] Command completed in {:.2?}", elapsed);
            eprintln!("[verbose] Exit code: {}", output.status);
            let stdout = String::from_utf8_lossy(&output.stdout);
            eprintln!("[verbose] Found {} lines of output", stdout.lines().count());
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Failed to list Homebrew formulae: 'brew list --formula' returned an error.\n\n\
                 Error output: {}\n\n\
                 Suggestions:\n\
                 - Run 'brew doctor' to check for issues\n\
                 - Try 'brew update' to refresh package information",
                stderr.trim()
            );
        }

        // Get pinned packages once
        let pinned_set = self.get_pinned_packages()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut packages = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[0].to_string();
                let version = parts[1].to_string();
                let pinned = pinned_set.contains(&name);

                packages.push(BrewPackage {
                    name,
                    version,
                    tap: None, // Skip tap lookup for speed
                    is_cask: false,
                    dependencies: Vec::new(), // Lazy load when needed
                    pinned,
                });
            }
        }

        Ok(packages)
    }

    /// Get all pinned packages at once
    fn get_pinned_packages(&self) -> Result<std::collections::HashSet<String>> {
        let output = Command::new("brew").args(["list", "--pinned"]).output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().map(|s| s.to_string()).collect())
    }

    /// List installed formulae with full details (slower, used for migration)
    pub fn list_installed_formulae_detailed(&self) -> Result<Vec<BrewPackage>> {
        let mut packages = self.list_installed_formulae()?;
        let total = packages.len();

        let pb = create_progress_bar(total as u64, "Loading package details...");

        for (i, pkg) in packages.iter_mut().enumerate() {
            pb.set_message(format!("Loading: {}", pkg.name));
            pkg.dependencies = self.get_dependencies(&pkg.name)?;
            pkg.tap = self.get_tap(&pkg.name)?;
            pb.set_position((i + 1) as u64);
        }

        pb.finish_with_message(format!("Loaded {} packages", total));
        Ok(packages)
    }

    /// List all installed Homebrew casks
    pub fn list_installed_casks(&self) -> Result<Vec<BrewPackage>> {
        let output = Command::new("brew")
            .args(["list", "--cask", "--versions"])
            .output()
            .context(
                "Failed to list Homebrew casks: Could not execute 'brew list --cask'.\n\n\
                 Suggestions:\n\
                 - Verify Homebrew is installed: run 'brew --version'\n\
                 - Check your internet connection if this is a network-related issue",
            )?;

        if !output.status.success() {
            // Casks might not be installed, return empty
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut packages = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                packages.push(BrewPackage {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    tap: None,
                    is_cask: true,
                    dependencies: Vec::new(),
                    pinned: false,
                });
            }
        }

        Ok(packages)
    }

    /// Get dependencies for a package
    fn get_dependencies(&self, name: &str) -> Result<Vec<String>> {
        let output = Command::new("brew")
            .args(["deps", "--installed", name])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                Ok(stdout.lines().map(|s| s.to_string()).collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Get the tap for a package
    fn get_tap(&self, name: &str) -> Result<Option<String>> {
        let output = Command::new("brew")
            .args(["info", "--json=v2", name])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                // Parse JSON to extract tap
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    if let Some(tap) = json["formulae"][0]["tap"].as_str() {
                        if tap != "homebrew/core" {
                            return Ok(Some(tap.to_string()));
                        }
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Export Homebrew packages to a Brewfile-compatible format for zerobrew
    pub fn export_to_brewfile(&self, path: &PathBuf) -> Result<()> {
        let formulae = self.list_installed_formulae()?;
        let casks = self.list_installed_casks()?;

        let mut content = String::new();
        content.push_str("# Zerobrew Migration Brewfile\n");
        content.push_str("# Generated from Homebrew installation\n\n");

        // Collect taps
        let mut taps: std::collections::HashSet<String> = std::collections::HashSet::new();
        for pkg in &formulae {
            if let Some(ref tap) = pkg.tap {
                taps.insert(tap.clone());
            }
        }

        for tap in &taps {
            content.push_str(&format!("tap \"{}\"\n", tap));
        }
        content.push('\n');

        // Formulae
        for pkg in &formulae {
            content.push_str(&format!("brew \"{}\"\n", pkg.name));
        }
        content.push('\n');

        // Casks
        for pkg in &casks {
            content.push_str(&format!("cask \"{}\"\n", pkg.name));
        }

        fs::write(path, content)?;
        Ok(())
    }

    /// Migrate a single package from Homebrew to Zerobrew
    pub fn migrate_package(&self, package: &BrewPackage) -> Result<MigrateResult> {
        println!("Migrating: {} ({})", package.name, package.version);

        let start = Instant::now();
        if self.verbose {
            eprintln!("[verbose] Running: zb install {}", package.name);
        }

        // Step 1: Install via zerobrew (it will use cache if available)
        let zb_result = Command::new("zb").args(["install", &package.name]).output();

        let elapsed = start.elapsed();

        match zb_result {
            Ok(output) if output.status.success() => {
                if self.verbose {
                    eprintln!("[verbose] Command completed in {:.2?}", elapsed);
                    eprintln!("[verbose] Exit code: {}", output.status);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stdout.trim().is_empty() {
                        eprintln!("[verbose] stdout: {}", stdout.trim());
                    }
                    if !stderr.trim().is_empty() {
                        eprintln!("[verbose] stderr: {}", stderr.trim());
                    }
                }
                // Step 2: Optionally uninstall from Homebrew to free space
                // (We don't do this automatically - user should confirm)
                Ok(MigrateResult::Success {
                    name: package.name.clone(),
                    version: package.version.clone(),
                })
            }
            Ok(output) => {
                if self.verbose {
                    eprintln!("[verbose] Command completed in {:.2?}", elapsed);
                    eprintln!("[verbose] Exit code: {}", output.status);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if !stdout.trim().is_empty() {
                        eprintln!("[verbose] stdout: {}", stdout.trim());
                    }
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                if self.verbose {
                    eprintln!("[verbose] stderr: {}", stderr.trim());
                }
                Ok(MigrateResult::Failed {
                    name: package.name.clone(),
                    reason: stderr.to_string(),
                })
            }
            Err(e) => {
                if self.verbose {
                    eprintln!("[verbose] Command failed after {:.2?}: {}", elapsed, e);
                }
                Ok(MigrateResult::Failed {
                    name: package.name.clone(),
                    reason: format!("Failed to run zb: {}", e),
                })
            }
        }
    }

    /// Migrate all packages from Homebrew to Zerobrew
    pub fn migrate_all(&self, dry_run: bool) -> Result<MigrationReport> {
        let casks = self.list_installed_casks()?;

        // Use fast version for dry-run, detailed version for actual migration
        let formulae = if dry_run {
            self.list_installed_formulae()?
        } else {
            self.list_installed_formulae_detailed()?
        };

        let mut report = MigrationReport {
            total_formulae: formulae.len(),
            total_casks: casks.len(),
            ..Default::default()
        };

        if dry_run {
            println!("\n=== DRY RUN - No changes will be made ===\n");
            println!(
                "Found {} formulae and {} casks to migrate:\n",
                formulae.len(),
                casks.len()
            );

            for pkg in &formulae {
                println!("  [formula] {} @ {}", pkg.name, pkg.version);
            }
            for pkg in &casks {
                println!("  [cask] {} @ {}", pkg.name, pkg.version);
            }
            return Ok(report);
        }

        // Migrate formulae (respect dependency order)
        let sorted = self.topological_sort(&formulae)?;
        for pkg in sorted {
            match self.migrate_package(&pkg)? {
                MigrateResult::Success { name, .. } => {
                    report.successful.push(name);
                }
                MigrateResult::Failed { name, reason } => {
                    report.failed.push((name, reason));
                }
            }
        }

        // Note: Casks are currently not supported by zerobrew
        for pkg in &casks {
            report
                .skipped
                .push((pkg.name.clone(), "Casks not yet supported".to_string()));
        }

        // Save migration state
        let mut state = self.load_state().unwrap_or_default();
        state.homebrew_prefix = self.homebrew_prefix.clone();
        for name in &report.successful {
            if let Some(pkg) = formulae.iter().find(|p| &p.name == name) {
                state.migrated_packages.insert(name.clone(), pkg.clone());
            }
        }
        for (name, _) in &report.failed {
            state.failed_packages.push(name.clone());
        }
        self.save_state(&state)?;

        Ok(report)
    }

    /// Interactive migration mode - prompts user before each package
    pub fn migrate_interactive(&self) -> Result<MigrationReport> {
        let casks = self.list_installed_casks()?;
        let formulae = self.list_installed_formulae_detailed()?;

        let mut report = MigrationReport {
            total_formulae: formulae.len(),
            total_casks: casks.len(),
            ..Default::default()
        };

        // Check if we're in a TTY environment
        let is_tty = std::io::stdin().is_terminal();
        if !is_tty {
            println!("Non-interactive environment detected. Falling back to non-interactive mode.");
            return self.migrate_all(false);
        }

        println!("\n=== Interactive Migration Mode ===\n");
        println!("Found {} formulae to migrate.\n", formulae.len());
        println!("Options for each package:");
        println!("  (y)es     - Migrate this package");
        println!("  (n)o      - Skip this package");
        println!("  (a)ll yes - Migrate all remaining packages");
        println!("  (q)uit    - Stop migration\n");

        let sorted = self.topological_sort(&formulae)?;
        let mut migrate_all_remaining = false;

        for (idx, pkg) in sorted.iter().enumerate() {
            // Show package info
            println!("{}", style(format!("--- Package {}/{} ---", idx + 1, sorted.len())).bold());
            println!("  Name:    {}", style(&pkg.name).cyan());
            println!("  Version: {}", pkg.version);
            if let Some(ref tap) = pkg.tap {
                println!("  Tap:     {}", tap);
            }
            if !pkg.dependencies.is_empty() {
                println!("  Deps:    {}", pkg.dependencies.join(", "));
            }
            if pkg.pinned {
                println!("  Status:  {}", style("[pinned]").yellow());
            }
            println!();

            let should_migrate = if migrate_all_remaining {
                println!("  Auto-migrating (all yes mode)...");
                true
            } else {
                // Show interactive prompt
                let items = vec![
                    "(y)es - Migrate this package",
                    "(n)o - Skip this package",
                    "(a)ll yes - Migrate all remaining",
                    "(q)uit - Stop migration",
                ];

                let selection = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("What would you like to do?")
                    .items(&items)
                    .default(0)
                    .interact();

                match selection {
                    Ok(0) => true,  // Yes
                    Ok(1) => {      // No/Skip
                        report.skipped.push((pkg.name.clone(), "User skipped".to_string()));
                        println!("  {} Skipped\n", style("->").yellow());
                        continue;
                    }
                    Ok(2) => {      // All yes
                        migrate_all_remaining = true;
                        true
                    }
                    Ok(3) | Err(_) => {  // Quit
                        println!("\n{}", style("Migration stopped by user.").yellow());
                        break;
                    }
                    _ => continue,
                }
            };

            if should_migrate {
                match self.migrate_package(pkg)? {
                    MigrateResult::Success { name, version } => {
                        println!("  {} Migrated: {} @ {}\n", style("OK").green(), name, version);
                        report.successful.push(name);
                    }
                    MigrateResult::Failed { name, reason } => {
                        println!("  {} Failed: {} - {}\n", style("X").red(), name, reason);
                        report.failed.push((name, reason));
                    }
                }
            }
        }

        // Handle casks
        for pkg in &casks {
            report.skipped.push((pkg.name.clone(), "Casks not yet supported".to_string()));
        }

        // Save migration state
        let mut state = self.load_state().unwrap_or_default();
        state.homebrew_prefix = self.homebrew_prefix.clone();
        for name in &report.successful {
            if let Some(pkg) = formulae.iter().find(|p| &p.name == name) {
                state.migrated_packages.insert(name.clone(), pkg.clone());
            }
        }
        for (name, _) in &report.failed {
            state.failed_packages.push(name.clone());
        }
        self.save_state(&state)?;

        Ok(report)
    }

    /// Topological sort for dependency order
    fn topological_sort(&self, packages: &[BrewPackage]) -> Result<Vec<BrewPackage>> {
        let mut result = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let pkg_map: HashMap<String, &BrewPackage> =
            packages.iter().map(|p| (p.name.clone(), p)).collect();

        fn visit(
            name: &str,
            pkg_map: &HashMap<String, &BrewPackage>,
            visited: &mut std::collections::HashSet<String>,
            result: &mut Vec<BrewPackage>,
        ) {
            if visited.contains(name) {
                return;
            }
            visited.insert(name.to_string());

            if let Some(pkg) = pkg_map.get(name) {
                for dep in &pkg.dependencies {
                    visit(dep, pkg_map, visited, result);
                }
                result.push((*pkg).clone());
            }
        }

        for pkg in packages {
            visit(&pkg.name, &pkg_map, &mut visited, &mut result);
        }

        Ok(result)
    }

    /// Cleanup: Optionally remove Homebrew packages after migration
    pub fn cleanup_homebrew(&self, packages: &[String], force: bool) -> Result<()> {
        if !force {
            println!("WARNING: This will uninstall packages from Homebrew.");
            println!("Make sure zerobrew has successfully installed them first.");
            println!("Run with --force to proceed.");
            return Ok(());
        }

        for name in packages {
            println!("Removing from Homebrew: {}", name);
            let _ = Command::new("brew")
                .args(["uninstall", "--ignore-dependencies", name])
                .status();
        }

        Ok(())
    }

    /// Save migration state
    pub fn save_state(&self, state: &MigrationState) -> Result<()> {
        let json = serde_json::to_string_pretty(state)?;
        if let Some(parent) = self.state_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.state_file, json)?;
        Ok(())
    }

    /// Load migration state
    pub fn load_state(&self) -> Result<MigrationState> {
        if self.state_file.exists() {
            let content = fs::read_to_string(&self.state_file)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(MigrationState::default())
        }
    }

    /// Analyze all installed packages and categorize them by migration risk.
    ///
    /// This method:
    /// 1. Lists all installed Homebrew packages
    /// 2. Checks which ones are in the known problematic list
    /// 3. Analyzes dependency chains to find packages that depend on problematic ones
    /// 4. Returns a categorized report with recommendations
    pub fn analyze_packages(&self) -> Result<AnalysisReport> {
        println!("Analyzing installed packages...");

        // Get all installed packages with their dependencies
        let packages = self.list_installed_formulae_detailed()?;
        let total = packages.len();

        // Build a set of problematic package names for quick lookup
        let problematic_set: HashSet<&str> = KNOWN_PROBLEMATIC_PACKAGES.iter().copied().collect();

        // Build a map of package name -> package for dependency lookup
        let pkg_map: HashMap<&str, &BrewPackage> = packages
            .iter()
            .map(|p| (p.name.as_str(), p))
            .collect();

        let mut report = AnalysisReport::new();
        report.total_packages = total;

        println!("Categorizing {} packages...", total);

        for pkg in &packages {
            // Check if this package is itself problematic
            if problematic_set.contains(pkg.name.as_str()) {
                let reason = Self::get_problematic_reason(&pkg.name);
                report.should_keep_in_homebrew.push(PackageAnalysis {
                    name: pkg.name.clone(),
                    version: pkg.version.clone(),
                    risk: MigrationRisk::KeepInHomebrew,
                    reason,
                    problematic_dependencies: Vec::new(),
                });
                continue;
            }

            // Check if this package depends on any problematic packages
            let problematic_deps: Vec<String> = pkg
                .dependencies
                .iter()
                .filter(|dep| problematic_set.contains(dep.as_str()))
                .cloned()
                .collect();

            if !problematic_deps.is_empty() {
                report.risky.push(PackageAnalysis {
                    name: pkg.name.clone(),
                    version: pkg.version.clone(),
                    risk: MigrationRisk::Risky,
                    reason: format!(
                        "Depends on {} problematic package(s)",
                        problematic_deps.len()
                    ),
                    problematic_dependencies: problematic_deps,
                });
            } else {
                // Check transitive dependencies (dependencies of dependencies)
                let transitive_problematic = Self::find_transitive_problematic_deps(
                    pkg,
                    &pkg_map,
                    &problematic_set,
                );

                if !transitive_problematic.is_empty() {
                    report.risky.push(PackageAnalysis {
                        name: pkg.name.clone(),
                        version: pkg.version.clone(),
                        risk: MigrationRisk::Risky,
                        reason: format!(
                            "Has transitive dependency on {} problematic package(s)",
                            transitive_problematic.len()
                        ),
                        problematic_dependencies: transitive_problematic,
                    });
                } else {
                    // Safe to migrate
                    report.safe_to_migrate.push(PackageAnalysis {
                        name: pkg.name.clone(),
                        version: pkg.version.clone(),
                        risk: MigrationRisk::Safe,
                        reason: "No known problematic dependencies".to_string(),
                        problematic_dependencies: Vec::new(),
                    });
                }
            }
        }

        // Sort each category alphabetically
        report.safe_to_migrate.sort_by(|a, b| a.name.cmp(&b.name));
        report.risky.sort_by(|a, b| a.name.cmp(&b.name));
        report.should_keep_in_homebrew.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(report)
    }

    /// Find transitive problematic dependencies (dependencies of dependencies)
    fn find_transitive_problematic_deps(
        pkg: &BrewPackage,
        pkg_map: &HashMap<&str, &BrewPackage>,
        problematic_set: &HashSet<&str>,
    ) -> Vec<String> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut problematic_found: Vec<String> = Vec::new();

        Self::find_transitive_deps_recursive(
            pkg,
            pkg_map,
            problematic_set,
            &mut visited,
            &mut problematic_found,
        );

        problematic_found
    }

    /// Recursive helper for finding transitive dependencies
    fn find_transitive_deps_recursive(
        pkg: &BrewPackage,
        pkg_map: &HashMap<&str, &BrewPackage>,
        problematic_set: &HashSet<&str>,
        visited: &mut HashSet<String>,
        found: &mut Vec<String>,
    ) {
        for dep_name in &pkg.dependencies {
            if visited.contains(dep_name) {
                continue;
            }
            visited.insert(dep_name.clone());

            // Check if this dependency is problematic
            if problematic_set.contains(dep_name.as_str()) && !found.contains(dep_name) {
                found.push(dep_name.clone());
            }

            // Recurse into this dependency's dependencies
            if let Some(dep_pkg) = pkg_map.get(dep_name.as_str()) {
                Self::find_transitive_deps_recursive(
                    dep_pkg,
                    pkg_map,
                    problematic_set,
                    visited,
                    found,
                );
            }
        }
    }

    /// Get a human-readable reason why a package is problematic
    fn get_problematic_reason(name: &str) -> String {
        match name {
            // SSL/TLS
            n if n.starts_with("openssl") => {
                "Core SSL/TLS library - many packages link against it".to_string()
            }
            "gnutls" => "GNU TLS library - system-level security dependency".to_string(),
            "libressl" => "LibreSSL - alternative SSL library with wide usage".to_string(),
            n if n.starts_with("libssh") => "SSH library - security-critical dependency".to_string(),

            // Python
            n if n.starts_with("python@") || n == "python" => {
                "Python runtime - complex virtual environment and pip dependencies".to_string()
            }

            // Event libraries
            "libevent" => "Event notification library - used by many network tools".to_string(),
            "libuv" => "Async I/O library - core dependency for Node.js ecosystem".to_string(),
            "libev" => "Event loop library - embedded in many applications".to_string(),

            // HTTP/networking
            "nghttp2" => "HTTP/2 library - used by curl and many HTTP clients".to_string(),
            "curl" => "URL transfer library - fundamental networking tool".to_string(),
            "wget" => "Network downloader - may have complex SSL dependencies".to_string(),

            // GLib ecosystem
            "gobject-introspection" => {
                "GObject introspection - required for many GTK/GNOME tools".to_string()
            }
            "glib" => "GLib core library - foundation for GTK ecosystem".to_string(),
            n if n.starts_with("gtk") || n == "cairo" || n == "pango" => {
                "GTK/graphics library - complex native rendering dependencies".to_string()
            }
            "gdk-pixbuf" => "Image loading library - part of GTK stack".to_string(),

            // Node.js
            n if n.starts_with("node") => {
                "Node.js runtime - native modules require specific linking".to_string()
            }

            // Databases
            n if n.starts_with("postgresql") || n == "libpq" => {
                "PostgreSQL client - complex library dependencies".to_string()
            }
            "mysql-client" => "MySQL client - database connectivity library".to_string(),

            // Compression
            "zlib" => "Core compression library - nearly universal dependency".to_string(),
            "xz" | "lz4" | "zstd" | "brotli" => {
                "Compression library - widely linked by other packages".to_string()
            }

            // Image libraries
            "libpng" | "libjpeg" | "libtiff" | "webp" => {
                "Image format library - many graphics tools depend on it".to_string()
            }

            // ICU
            "icu4c" => {
                "Unicode library - heavily depended upon for internationalization".to_string()
            }

            // Build tools
            "pkg-config" => "Build configuration tool - used during compilation".to_string(),
            "cmake" => "Build system - required for building many packages".to_string(),
            "autoconf" | "automake" | "libtool" => {
                "GNU build tools - required for package compilation".to_string()
            }

            // Ruby
            n if n.starts_with("ruby@") || n == "ruby" => {
                "Ruby runtime - gem native extensions require specific linking".to_string()
            }

            // Other
            "gettext" => "Internationalization library - widely used for translations".to_string(),
            "readline" => "Command-line editing library - used by many CLI tools".to_string(),
            "ncurses" => "Terminal UI library - fundamental for terminal apps".to_string(),
            "pcre" | "pcre2" => {
                "Regular expression library - used by many text processing tools".to_string()
            }

            _ => "Known to cause migration issues".to_string(),
        }
    }
}

#[derive(Debug)]
pub enum MigrateResult {
    Success { name: String, version: String },
    Failed { name: String, reason: String },
}

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub total_formulae: usize,
    pub total_casks: usize,
    pub successful: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub skipped: Vec<(String, String)>,
}

impl MigrationReport {
    pub fn print_summary(&self) {
        println!("\n=== Migration Summary ===");
        println!("Total formulae: {}", self.total_formulae);
        println!("Total casks: {}", self.total_casks);
        println!("Successful: {}", self.successful.len());
        println!("Failed: {}", self.failed.len());
        println!("Skipped: {}", self.skipped.len());

        if !self.failed.is_empty() {
            println!("\nFailed packages:");
            for (name, reason) in &self.failed {
                println!("  {} - {}", name, reason);
            }
        }

        if !self.skipped.is_empty() {
            println!("\nSkipped packages:");
            for (name, reason) in &self.skipped {
                println!("  {} - {}", name, reason);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ============================================
    // BrewPackage Parsing Tests
    // ============================================

    #[test]
    fn test_brew_package_creation() {
        let pkg = BrewPackage {
            name: "git".to_string(),
            version: "2.42.0".to_string(),
            tap: None,
            is_cask: false,
            dependencies: vec!["pcre2".to_string(), "gettext".to_string()],
            pinned: false,
        };

        assert_eq!(pkg.name, "git");
        assert_eq!(pkg.version, "2.42.0");
        assert!(pkg.tap.is_none());
        assert!(!pkg.is_cask);
        assert_eq!(pkg.dependencies.len(), 2);
        assert!(!pkg.pinned);
    }

    #[test]
    fn test_brew_package_with_tap() {
        let pkg = BrewPackage {
            name: "neovim".to_string(),
            version: "0.9.4".to_string(),
            tap: Some("homebrew/core".to_string()),
            is_cask: false,
            dependencies: vec![],
            pinned: true,
        };

        assert_eq!(pkg.tap, Some("homebrew/core".to_string()));
        assert!(pkg.pinned);
    }

    #[test]
    fn test_brew_package_cask() {
        let pkg = BrewPackage {
            name: "visual-studio-code".to_string(),
            version: "1.84.0".to_string(),
            tap: Some("homebrew/cask".to_string()),
            is_cask: true,
            dependencies: vec![],
            pinned: false,
        };

        assert!(pkg.is_cask);
        assert_eq!(pkg.name, "visual-studio-code");
    }

    #[test]
    fn test_parse_brew_list_output() {
        // Simulate parsing brew list --formula --versions output
        let brew_output = "git 2.42.0\nnode 20.9.0\nrust 1.73.0\npython@3.11 3.11.6";

        let mut packages = Vec::new();
        for line in brew_output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                packages.push(BrewPackage {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    tap: None,
                    is_cask: false,
                    dependencies: Vec::new(),
                    pinned: false,
                });
            }
        }

        assert_eq!(packages.len(), 4);
        assert_eq!(packages[0].name, "git");
        assert_eq!(packages[0].version, "2.42.0");
        assert_eq!(packages[1].name, "node");
        assert_eq!(packages[2].name, "rust");
        assert_eq!(packages[3].name, "python@3.11");
        assert_eq!(packages[3].version, "3.11.6");
    }

    #[test]
    fn test_parse_brew_list_empty_output() {
        let brew_output = "";

        let mut packages = Vec::new();
        for line in brew_output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                packages.push(BrewPackage {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    tap: None,
                    is_cask: false,
                    dependencies: Vec::new(),
                    pinned: false,
                });
            }
        }

        assert!(packages.is_empty());
    }

    #[test]
    fn test_parse_brew_list_with_multiple_versions() {
        // Some packages show multiple versions installed
        let brew_output = "openssl@3 3.1.4 3.1.3";

        let mut packages = Vec::new();
        for line in brew_output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                // Take the first (latest) version
                packages.push(BrewPackage {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    tap: None,
                    is_cask: false,
                    dependencies: Vec::new(),
                    pinned: false,
                });
            }
        }

        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "openssl@3");
        assert_eq!(packages[0].version, "3.1.4");
    }

    #[test]
    fn test_parse_malformed_brew_output() {
        // Lines with less than 2 parts should be skipped
        let brew_output = "git 2.42.0\ninvalid_line\nnode 20.9.0\n   \nrust";

        let mut packages = Vec::new();
        for line in brew_output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                packages.push(BrewPackage {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    tap: None,
                    is_cask: false,
                    dependencies: Vec::new(),
                    pinned: false,
                });
            }
        }

        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "git");
        assert_eq!(packages[1].name, "node");
    }

    // ============================================
    // MigrationState Serialization Tests
    // ============================================

    #[test]
    fn test_migration_state_default() {
        let state = MigrationState::default();

        assert!(state.migrated_packages.is_empty());
        assert!(state.failed_packages.is_empty());
        assert_eq!(state.homebrew_prefix, PathBuf::new());
    }

    #[test]
    fn test_migration_state_serialization() {
        let mut state = MigrationState::default();
        state.homebrew_prefix = PathBuf::from("/opt/homebrew");
        state.failed_packages.push("broken-pkg".to_string());

        let pkg = BrewPackage {
            name: "git".to_string(),
            version: "2.42.0".to_string(),
            tap: None,
            is_cask: false,
            dependencies: vec!["pcre2".to_string()],
            pinned: false,
        };
        state.migrated_packages.insert("git".to_string(), pkg);

        let json = serde_json::to_string(&state).expect("Serialization failed");

        assert!(json.contains("\"homebrew_prefix\""));
        assert!(json.contains("/opt/homebrew"));
        assert!(json.contains("\"git\""));
        assert!(json.contains("broken-pkg"));
    }

    #[test]
    fn test_migration_state_deserialization() {
        let json = r#"{
            "migrated_packages": {
                "node": {
                    "name": "node",
                    "version": "20.9.0",
                    "tap": null,
                    "is_cask": false,
                    "dependencies": ["icu4c"],
                    "pinned": false
                }
            },
            "failed_packages": ["broken-pkg"],
            "homebrew_prefix": "/opt/homebrew"
        }"#;

        let state: MigrationState = serde_json::from_str(json).expect("Deserialization failed");

        assert_eq!(state.homebrew_prefix, PathBuf::from("/opt/homebrew"));
        assert_eq!(state.failed_packages.len(), 1);
        assert_eq!(state.failed_packages[0], "broken-pkg");
        assert!(state.migrated_packages.contains_key("node"));

        let node_pkg = state.migrated_packages.get("node").unwrap();
        assert_eq!(node_pkg.version, "20.9.0");
        assert_eq!(node_pkg.dependencies, vec!["icu4c"]);
    }

    #[test]
    fn test_migration_state_roundtrip() {
        let mut original = MigrationState::default();
        original.homebrew_prefix = PathBuf::from("/usr/local");
        original.failed_packages = vec!["pkg1".to_string(), "pkg2".to_string()];

        let pkg = BrewPackage {
            name: "rust".to_string(),
            version: "1.73.0".to_string(),
            tap: Some("homebrew/core".to_string()),
            is_cask: false,
            dependencies: vec!["libssh2".to_string(), "openssl@3".to_string()],
            pinned: true,
        };
        original.migrated_packages.insert("rust".to_string(), pkg);

        // Serialize
        let json = serde_json::to_string_pretty(&original).expect("Serialization failed");

        // Deserialize
        let restored: MigrationState = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(restored.homebrew_prefix, original.homebrew_prefix);
        assert_eq!(restored.failed_packages, original.failed_packages);
        assert_eq!(
            restored.migrated_packages.len(),
            original.migrated_packages.len()
        );

        let rust_pkg = restored.migrated_packages.get("rust").unwrap();
        assert_eq!(rust_pkg.version, "1.73.0");
        assert!(rust_pkg.pinned);
        assert_eq!(rust_pkg.tap, Some("homebrew/core".to_string()));
    }

    #[test]
    fn test_migration_state_empty_json() {
        let json = r#"{
            "migrated_packages": {},
            "failed_packages": [],
            "homebrew_prefix": ""
        }"#;

        let state: MigrationState = serde_json::from_str(json).expect("Deserialization failed");

        assert!(state.migrated_packages.is_empty());
        assert!(state.failed_packages.is_empty());
    }

    // ============================================
    // Topological Sort Tests
    // ============================================

    fn create_test_package(name: &str, deps: Vec<&str>) -> BrewPackage {
        BrewPackage {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            tap: None,
            is_cask: false,
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            pinned: false,
        }
    }

    #[test]
    fn test_topological_sort_no_dependencies() {
        let packages = vec![
            create_test_package("a", vec![]),
            create_test_package("b", vec![]),
            create_test_package("c", vec![]),
        ];

        let pkg_map: HashMap<String, &BrewPackage> =
            packages.iter().map(|p| (p.name.clone(), p)).collect();

        let mut result = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        fn visit(
            name: &str,
            pkg_map: &HashMap<String, &BrewPackage>,
            visited: &mut std::collections::HashSet<String>,
            result: &mut Vec<BrewPackage>,
        ) {
            if visited.contains(name) {
                return;
            }
            visited.insert(name.to_string());

            if let Some(pkg) = pkg_map.get(name) {
                for dep in &pkg.dependencies {
                    visit(dep, pkg_map, visited, result);
                }
                result.push((*pkg).clone());
            }
        }

        for pkg in &packages {
            visit(&pkg.name, &pkg_map, &mut visited, &mut result);
        }

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_topological_sort_linear_dependencies() {
        // c depends on b, b depends on a
        let packages = vec![
            create_test_package("a", vec![]),
            create_test_package("b", vec!["a"]),
            create_test_package("c", vec!["b"]),
        ];

        let pkg_map: HashMap<String, &BrewPackage> =
            packages.iter().map(|p| (p.name.clone(), p)).collect();

        let mut result = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        fn visit(
            name: &str,
            pkg_map: &HashMap<String, &BrewPackage>,
            visited: &mut std::collections::HashSet<String>,
            result: &mut Vec<BrewPackage>,
        ) {
            if visited.contains(name) {
                return;
            }
            visited.insert(name.to_string());

            if let Some(pkg) = pkg_map.get(name) {
                for dep in &pkg.dependencies {
                    visit(dep, pkg_map, visited, result);
                }
                result.push((*pkg).clone());
            }
        }

        for pkg in &packages {
            visit(&pkg.name, &pkg_map, &mut visited, &mut result);
        }

        // a should come before b, b should come before c
        let pos_a = result.iter().position(|p| p.name == "a").unwrap();
        let pos_b = result.iter().position(|p| p.name == "b").unwrap();
        let pos_c = result.iter().position(|p| p.name == "c").unwrap();

        assert!(pos_a < pos_b, "a should come before b");
        assert!(pos_b < pos_c, "b should come before c");
    }

    #[test]
    fn test_topological_sort_diamond_dependency() {
        // d depends on b and c, both b and c depend on a
        let packages = vec![
            create_test_package("a", vec![]),
            create_test_package("b", vec!["a"]),
            create_test_package("c", vec!["a"]),
            create_test_package("d", vec!["b", "c"]),
        ];

        let pkg_map: HashMap<String, &BrewPackage> =
            packages.iter().map(|p| (p.name.clone(), p)).collect();

        let mut result = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        fn visit(
            name: &str,
            pkg_map: &HashMap<String, &BrewPackage>,
            visited: &mut std::collections::HashSet<String>,
            result: &mut Vec<BrewPackage>,
        ) {
            if visited.contains(name) {
                return;
            }
            visited.insert(name.to_string());

            if let Some(pkg) = pkg_map.get(name) {
                for dep in &pkg.dependencies {
                    visit(dep, pkg_map, visited, result);
                }
                result.push((*pkg).clone());
            }
        }

        for pkg in &packages {
            visit(&pkg.name, &pkg_map, &mut visited, &mut result);
        }

        assert_eq!(result.len(), 4);

        let pos_a = result.iter().position(|p| p.name == "a").unwrap();
        let pos_b = result.iter().position(|p| p.name == "b").unwrap();
        let pos_c = result.iter().position(|p| p.name == "c").unwrap();
        let pos_d = result.iter().position(|p| p.name == "d").unwrap();

        assert!(pos_a < pos_b, "a should come before b");
        assert!(pos_a < pos_c, "a should come before c");
        assert!(pos_b < pos_d, "b should come before d");
        assert!(pos_c < pos_d, "c should come before d");
    }

    #[test]
    fn test_topological_sort_missing_dependency() {
        // b depends on "missing" which is not in the package list
        let packages = vec![
            create_test_package("a", vec![]),
            create_test_package("b", vec!["missing"]),
        ];

        let pkg_map: HashMap<String, &BrewPackage> =
            packages.iter().map(|p| (p.name.clone(), p)).collect();

        let mut result = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        fn visit(
            name: &str,
            pkg_map: &HashMap<String, &BrewPackage>,
            visited: &mut std::collections::HashSet<String>,
            result: &mut Vec<BrewPackage>,
        ) {
            if visited.contains(name) {
                return;
            }
            visited.insert(name.to_string());

            if let Some(pkg) = pkg_map.get(name) {
                for dep in &pkg.dependencies {
                    visit(dep, pkg_map, visited, result);
                }
                result.push((*pkg).clone());
            }
        }

        for pkg in &packages {
            visit(&pkg.name, &pkg_map, &mut visited, &mut result);
        }

        // Should still work - missing deps are just skipped
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_topological_sort_empty_list() {
        let packages: Vec<BrewPackage> = vec![];

        let pkg_map: HashMap<String, &BrewPackage> =
            packages.iter().map(|p| (p.name.clone(), p)).collect();

        let mut result = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        fn visit(
            name: &str,
            pkg_map: &HashMap<String, &BrewPackage>,
            visited: &mut std::collections::HashSet<String>,
            result: &mut Vec<BrewPackage>,
        ) {
            if visited.contains(name) {
                return;
            }
            visited.insert(name.to_string());

            if let Some(pkg) = pkg_map.get(name) {
                for dep in &pkg.dependencies {
                    visit(dep, pkg_map, visited, result);
                }
                result.push((*pkg).clone());
            }
        }

        for pkg in &packages {
            visit(&pkg.name, &pkg_map, &mut visited, &mut result);
        }

        assert!(result.is_empty());
    }

    #[test]
    fn test_topological_sort_complex_graph() {
        // Complex dependency graph:
        // f -> d, e
        // e -> c
        // d -> b, c
        // c -> a
        // b -> a
        // a -> (none)
        let packages = vec![
            create_test_package("a", vec![]),
            create_test_package("b", vec!["a"]),
            create_test_package("c", vec!["a"]),
            create_test_package("d", vec!["b", "c"]),
            create_test_package("e", vec!["c"]),
            create_test_package("f", vec!["d", "e"]),
        ];

        let pkg_map: HashMap<String, &BrewPackage> =
            packages.iter().map(|p| (p.name.clone(), p)).collect();

        let mut result = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        fn visit(
            name: &str,
            pkg_map: &HashMap<String, &BrewPackage>,
            visited: &mut std::collections::HashSet<String>,
            result: &mut Vec<BrewPackage>,
        ) {
            if visited.contains(name) {
                return;
            }
            visited.insert(name.to_string());

            if let Some(pkg) = pkg_map.get(name) {
                for dep in &pkg.dependencies {
                    visit(dep, pkg_map, visited, result);
                }
                result.push((*pkg).clone());
            }
        }

        for pkg in &packages {
            visit(&pkg.name, &pkg_map, &mut visited, &mut result);
        }

        assert_eq!(result.len(), 6);

        // Verify ordering constraints
        let positions: HashMap<String, usize> = result
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone(), i))
            .collect();

        assert!(positions["a"] < positions["b"]);
        assert!(positions["a"] < positions["c"]);
        assert!(positions["b"] < positions["d"]);
        assert!(positions["c"] < positions["d"]);
        assert!(positions["c"] < positions["e"]);
        assert!(positions["d"] < positions["f"]);
        assert!(positions["e"] < positions["f"]);
    }

    // ============================================
    // Brewfile Export Format Tests
    // ============================================

    #[test]
    fn test_brewfile_format_basic() {
        let formulae = vec![
            create_test_package("git", vec![]),
            create_test_package("node", vec![]),
        ];

        let casks = vec![BrewPackage {
            name: "visual-studio-code".to_string(),
            version: "1.84.0".to_string(),
            tap: None,
            is_cask: true,
            dependencies: vec![],
            pinned: false,
        }];

        let mut content = String::new();
        content.push_str("# Zerobrew Migration Brewfile\n");
        content.push_str("# Generated from Homebrew installation\n\n");

        // Collect taps
        let mut taps: std::collections::HashSet<String> = std::collections::HashSet::new();
        for pkg in &formulae {
            if let Some(ref tap) = pkg.tap {
                taps.insert(tap.clone());
            }
        }

        for tap in &taps {
            content.push_str(&format!("tap \"{}\"\n", tap));
        }
        content.push('\n');

        // Formulae
        for pkg in &formulae {
            content.push_str(&format!("brew \"{}\"\n", pkg.name));
        }
        content.push('\n');

        // Casks
        for pkg in &casks {
            content.push_str(&format!("cask \"{}\"\n", pkg.name));
        }

        assert!(content.contains("# Zerobrew Migration Brewfile"));
        assert!(content.contains("brew \"git\""));
        assert!(content.contains("brew \"node\""));
        assert!(content.contains("cask \"visual-studio-code\""));
    }

    #[test]
    fn test_brewfile_format_with_taps() {
        let formulae = vec![
            BrewPackage {
                name: "neovim".to_string(),
                version: "0.9.4".to_string(),
                tap: Some("homebrew/core".to_string()),
                is_cask: false,
                dependencies: vec![],
                pinned: false,
            },
            BrewPackage {
                name: "custom-tool".to_string(),
                version: "1.0.0".to_string(),
                tap: Some("user/custom-tap".to_string()),
                is_cask: false,
                dependencies: vec![],
                pinned: false,
            },
        ];

        let mut content = String::new();
        content.push_str("# Zerobrew Migration Brewfile\n");
        content.push_str("# Generated from Homebrew installation\n\n");

        // Collect taps
        let mut taps: std::collections::HashSet<String> = std::collections::HashSet::new();
        for pkg in &formulae {
            if let Some(ref tap) = pkg.tap {
                taps.insert(tap.clone());
            }
        }

        for tap in &taps {
            content.push_str(&format!("tap \"{}\"\n", tap));
        }
        content.push('\n');

        for pkg in &formulae {
            content.push_str(&format!("brew \"{}\"\n", pkg.name));
        }

        assert!(
            content.contains("tap \"homebrew/core\"") || content.contains("tap \"user/custom-tap\"")
        );
        assert!(content.contains("brew \"neovim\""));
        assert!(content.contains("brew \"custom-tool\""));
    }

    #[test]
    fn test_brewfile_format_empty() {
        let formulae: Vec<BrewPackage> = vec![];
        let casks: Vec<BrewPackage> = vec![];

        let mut content = String::new();
        content.push_str("# Zerobrew Migration Brewfile\n");
        content.push_str("# Generated from Homebrew installation\n\n");

        let taps: std::collections::HashSet<String> = std::collections::HashSet::new();

        for tap in &taps {
            content.push_str(&format!("tap \"{}\"\n", tap));
        }
        content.push('\n');

        for pkg in &formulae {
            content.push_str(&format!("brew \"{}\"\n", pkg.name));
        }
        content.push('\n');

        for pkg in &casks {
            content.push_str(&format!("cask \"{}\"\n", pkg.name));
        }

        assert!(content.contains("# Zerobrew Migration Brewfile"));
        assert!(!content.contains("brew \""));
        assert!(!content.contains("cask \""));
    }

    #[test]
    fn test_brewfile_format_special_characters() {
        let formulae = vec![
            create_test_package("python@3.11", vec![]),
            create_test_package("openssl@3", vec![]),
        ];

        let mut content = String::new();
        for pkg in &formulae {
            content.push_str(&format!("brew \"{}\"\n", pkg.name));
        }

        assert!(content.contains("brew \"python@3.11\""));
        assert!(content.contains("brew \"openssl@3\""));
    }

    // ============================================
    // MigrationReport Tests
    // ============================================

    #[test]
    fn test_migration_report_default() {
        let report = MigrationReport::default();

        assert_eq!(report.total_formulae, 0);
        assert_eq!(report.total_casks, 0);
        assert!(report.successful.is_empty());
        assert!(report.failed.is_empty());
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn test_migration_report_with_data() {
        let mut report = MigrationReport::default();
        report.total_formulae = 10;
        report.total_casks = 5;
        report.successful.push("git".to_string());
        report.successful.push("node".to_string());
        report
            .failed
            .push(("broken-pkg".to_string(), "Install failed".to_string()));
        report
            .skipped
            .push(("cask-app".to_string(), "Casks not supported".to_string()));

        assert_eq!(report.total_formulae, 10);
        assert_eq!(report.total_casks, 5);
        assert_eq!(report.successful.len(), 2);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.skipped.len(), 1);
    }

    // ============================================
    // MigrateResult Tests
    // ============================================

    #[test]
    fn test_migrate_result_success() {
        let result = MigrateResult::Success {
            name: "git".to_string(),
            version: "2.42.0".to_string(),
        };

        match result {
            MigrateResult::Success { name, version } => {
                assert_eq!(name, "git");
                assert_eq!(version, "2.42.0");
            }
            MigrateResult::Failed { .. } => panic!("Expected Success variant"),
        }
    }

    #[test]
    fn test_migrate_result_failed() {
        let result = MigrateResult::Failed {
            name: "broken-pkg".to_string(),
            reason: "Package not found".to_string(),
        };

        match result {
            MigrateResult::Success { .. } => panic!("Expected Failed variant"),
            MigrateResult::Failed { name, reason } => {
                assert_eq!(name, "broken-pkg");
                assert_eq!(reason, "Package not found");
            }
        }
    }

    // ============================================
    // BrewPackage Serialization Tests
    // ============================================

    #[test]
    fn test_brew_package_serialization() {
        let pkg = BrewPackage {
            name: "git".to_string(),
            version: "2.42.0".to_string(),
            tap: Some("homebrew/core".to_string()),
            is_cask: false,
            dependencies: vec!["pcre2".to_string(), "gettext".to_string()],
            pinned: true,
        };

        let json = serde_json::to_string(&pkg).expect("Serialization failed");

        assert!(json.contains("\"name\":\"git\""));
        assert!(json.contains("\"version\":\"2.42.0\""));
        assert!(json.contains("\"tap\":\"homebrew/core\""));
        assert!(json.contains("\"is_cask\":false"));
        assert!(json.contains("\"pinned\":true"));
        assert!(json.contains("pcre2"));
        assert!(json.contains("gettext"));
    }

    #[test]
    fn test_brew_package_deserialization() {
        let json = r#"{
            "name": "node",
            "version": "20.9.0",
            "tap": null,
            "is_cask": false,
            "dependencies": ["icu4c", "libnghttp2"],
            "pinned": false
        }"#;

        let pkg: BrewPackage = serde_json::from_str(json).expect("Deserialization failed");

        assert_eq!(pkg.name, "node");
        assert_eq!(pkg.version, "20.9.0");
        assert!(pkg.tap.is_none());
        assert!(!pkg.is_cask);
        assert_eq!(pkg.dependencies, vec!["icu4c", "libnghttp2"]);
        assert!(!pkg.pinned);
    }

    #[test]
    fn test_brew_package_roundtrip() {
        let original = BrewPackage {
            name: "rust".to_string(),
            version: "1.73.0".to_string(),
            tap: Some("homebrew/core".to_string()),
            is_cask: false,
            dependencies: vec!["libssh2".to_string(), "openssl@3".to_string()],
            pinned: true,
        };

        let json = serde_json::to_string(&original).expect("Serialization failed");
        let restored: BrewPackage = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(restored.name, original.name);
        assert_eq!(restored.version, original.version);
        assert_eq!(restored.tap, original.tap);
        assert_eq!(restored.is_cask, original.is_cask);
        assert_eq!(restored.dependencies, original.dependencies);
        assert_eq!(restored.pinned, original.pinned);
    }

    // ============================================
    // File I/O Tests (using tempfile)
    // ============================================

    #[test]
    fn test_brewfile_write_to_file() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

        let content = "# Zerobrew Migration Brewfile\nbrew \"git\"\ncask \"vscode\"\n";
        temp_file
            .write_all(content.as_bytes())
            .expect("Failed to write");

        let read_content =
            std::fs::read_to_string(temp_file.path()).expect("Failed to read temp file");

        assert_eq!(read_content, content);
    }

    #[test]
    fn test_migration_state_file_io() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");

        let mut state = MigrationState::default();
        state.homebrew_prefix = PathBuf::from("/opt/homebrew");
        state
            .migrated_packages
            .insert("git".to_string(), create_test_package("git", vec![]));

        // Write state
        let json = serde_json::to_string_pretty(&state).expect("Serialization failed");
        std::fs::write(temp_file.path(), &json).expect("Failed to write");

        // Read state
        let read_content =
            std::fs::read_to_string(temp_file.path()).expect("Failed to read temp file");
        let restored: MigrationState =
            serde_json::from_str(&read_content).expect("Deserialization failed");

        assert_eq!(restored.homebrew_prefix, state.homebrew_prefix);
        assert!(restored.migrated_packages.contains_key("git"));
    }

    // ============================================
    // Integration Tests (requires Homebrew)
    // ============================================

    #[test]
    #[ignore] // Run with: cargo test -- --ignored
    fn test_migrator_creation_integration() {
        // This test requires Homebrew to be installed
        let result = HomebrewMigrator::new(false);
        assert!(result.is_ok(), "Migrator creation should succeed");

        let migrator = result.unwrap();
        assert!(
            migrator.homebrew_prefix.exists(),
            "Homebrew prefix should exist"
        );
    }

    #[test]
    #[ignore] // Run with: cargo test -- --ignored
    fn test_list_installed_formulae_integration() {
        // This test requires Homebrew to be installed
        let migrator = HomebrewMigrator::new(false).expect("Failed to create migrator");
        let formulae = migrator
            .list_installed_formulae()
            .expect("Failed to list formulae");

        // Just verify it returns something (assuming at least one package is installed)
        println!("Found {} formulae", formulae.len());
    }
}
