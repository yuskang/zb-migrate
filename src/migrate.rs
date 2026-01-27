//! Homebrew to Zerobrew Migration Module
//!
//! This module provides functionality to:
//! 1. Read installed packages from Homebrew
//! 2. Import them into Zerobrew's management
//! 3. Handle subsequent updates via Zerobrew

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
}

impl HomebrewMigrator {
    /// Create a new migrator instance
    pub fn new() -> Result<Self> {
        let homebrew_prefix = Self::detect_homebrew_prefix()?;
        let home = std::env::var("HOME").context("HOME not set")?;

        Ok(Self {
            homebrew_prefix,
            state_file: PathBuf::from(format!("{}/.zerobrew/migration_state.json", home)),
        })
    }

    /// Detect Homebrew installation prefix
    fn detect_homebrew_prefix() -> Result<PathBuf> {
        // Try to get prefix from brew command
        let output = Command::new("brew")
            .arg("--prefix")
            .output()
            .context("Failed to run 'brew --prefix'. Is Homebrew installed?")?;

        if !output.status.success() {
            bail!("brew --prefix failed");
        }

        let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();

        Ok(PathBuf::from(prefix))
    }

    /// List all installed Homebrew formulae (fast mode - minimal brew calls)
    pub fn list_installed_formulae(&self) -> Result<Vec<BrewPackage>> {
        let output = Command::new("brew")
            .args(["list", "--formula", "--versions"])
            .output()
            .context("Failed to list Homebrew formulae")?;

        if !output.status.success() {
            bail!("brew list failed");
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

        println!("Loading package details...");
        for (i, pkg) in packages.iter_mut().enumerate() {
            if i % 10 == 0 {
                print!("\rProcessing {}/{}...", i + 1, total);
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            pkg.dependencies = self.get_dependencies(&pkg.name)?;
            pkg.tap = self.get_tap(&pkg.name)?;
        }
        println!("\rProcessed {} packages.      ", total);

        Ok(packages)
    }

    /// List all installed Homebrew casks
    pub fn list_installed_casks(&self) -> Result<Vec<BrewPackage>> {
        let output = Command::new("brew")
            .args(["list", "--cask", "--versions"])
            .output()
            .context("Failed to list Homebrew casks")?;

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

        // Step 1: Install via zerobrew (it will use cache if available)
        let zb_result = Command::new("zb").args(["install", &package.name]).output();

        match zb_result {
            Ok(output) if output.status.success() => {
                // Step 2: Optionally uninstall from Homebrew to free space
                // (We don't do this automatically - user should confirm)
                Ok(MigrateResult::Success {
                    name: package.name.clone(),
                    version: package.version.clone(),
                })
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Ok(MigrateResult::Failed {
                    name: package.name.clone(),
                    reason: stderr.to_string(),
                })
            }
            Err(e) => Ok(MigrateResult::Failed {
                name: package.name.clone(),
                reason: format!("Failed to run zb: {}", e),
            }),
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

    /// Check for updates and update via zerobrew
    pub fn check_updates(&self) -> Result<Vec<UpdateInfo>> {
        let output = Command::new("zb")
            .args(["outdated"])
            .output()
            .context("Failed to check for updates via zerobrew")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut updates = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                updates.push(UpdateInfo {
                    name: parts[0].to_string(),
                    current_version: parts[1].to_string(),
                    new_version: parts[2].to_string(),
                });
            }
        }

        Ok(updates)
    }

    /// Update all packages via zerobrew
    pub fn update_all(&self) -> Result<()> {
        println!("Updating all packages via zerobrew...");

        let status = Command::new("zb")
            .args(["upgrade"])
            .status()
            .context("Failed to run zb upgrade")?;

        if !status.success() {
            bail!("zb upgrade failed");
        }

        Ok(())
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

#[derive(Debug)]
pub struct UpdateInfo {
    pub name: String,
    pub current_version: String,
    pub new_version: String,
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

    #[test]
    fn test_migrator_creation() {
        // This will fail if Homebrew is not installed
        let result = HomebrewMigrator::new();
        // Just check it doesn't panic
        let _ = result;
    }
}
