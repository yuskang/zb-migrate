# zb-migrate

[繁體中文](README.zh-TW.md) | English

A CLI tool to migrate packages from Homebrew to [Zerobrew](https://github.com/lucasgelfond/zerobrew).

## Features

- List all installed Homebrew packages (formulae & casks)
- Export to Brewfile format
- Migrate packages to Zerobrew with dependency ordering
- Track migration status
- Handle subsequent updates via Zerobrew
- Clean up Homebrew after successful migration

## Installation

### One-liner Install

```bash
curl -fsSL https://raw.githubusercontent.com/yuskang/zb-migrate/main/install.sh | bash
```

### Build from Source

```bash
git clone https://github.com/yuskang/zb-migrate.git
cd zb-migrate
cargo build --release
cp target/release/zb-migrate /usr/local/bin/
```

### Install via Cargo

```bash
cargo install --git https://github.com/yuskang/zb-migrate.git
```

## Prerequisites

- [Homebrew](https://brew.sh/) installed
- [Zerobrew](https://github.com/lucasgelfond/zerobrew) installed
- Rust toolchain (for building from source)

## Usage

### List Installed Packages

```bash
# List formulae only
zb-migrate list

# Include casks
zb-migrate list --casks

# Output as JSON
zb-migrate list --json
```

### Export Brewfile

```bash
zb-migrate export -o ~/Brewfile.zerobrew
```

### Migrate Packages

```bash
# Preview migration (recommended first)
zb-migrate migrate --dry-run

# Execute migration
zb-migrate migrate

# Migrate specific packages only
zb-migrate migrate -p git -p node
```

### Check for Updates

```bash
zb-migrate outdated
```

### Upgrade All Packages

```bash
zb-migrate upgrade
```

### Check Migration Status

```bash
zb-migrate status
```

### Cleanup Homebrew

After confirming everything works:

```bash
# Preview
zb-migrate cleanup

# Execute cleanup
zb-migrate cleanup --force
```

## Known Limitations

### Zerobrew Limitations

Due to Zerobrew's architecture, some packages may fail to migrate:

| Issue | Affected Packages | Solution |
|-------|-------------------|----------|
| **Link conflicts** | `openssl@3`, `python@3.x`, and packages depending on them | Keep in Homebrew |
| **Casks not supported** | All GUI applications (`.app`) | Continue using `brew install --cask` |
| **Tap packages** | Some third-party taps | May require manual intervention |

### Packages That Typically Fail

These packages often have link conflicts and should remain in Homebrew:

- `openssl@3` - Core SSL library, many packages depend on it
- `python@3.x` - Python interpreters
- `libevent`, `gnutls`, `nghttp2` - Network libraries
- `gobject-introspection` - GLib introspection
- `node@xx` - Node.js versions

### Recommended Coexistence Strategy

| Manager | Package Type |
|---------|--------------|
| **Zerobrew** | Most CLI tools, utilities |
| **Homebrew** | OpenSSL-related packages, Casks, problematic packages |

### Update Strategy

```bash
# Zerobrew-managed packages
zb upgrade

# Homebrew-managed packages
brew upgrade
```

## How It Works

1. **Read packages**: Uses `brew list --formula --versions` to get installed packages
2. **Resolve dependencies**: Topologically sorts packages to ensure correct installation order
3. **Migrate**: Installs each package via `zb install`
4. **Track state**: Saves migration status to `~/.zerobrew/migration_state.json`
5. **Manage updates**: Uses `zb upgrade` for subsequent updates

## Migration State File

Migration state is stored at `~/.zerobrew/migration_state.json`:

```json
{
  "migrated_packages": {
    "git": { "name": "git", "version": "2.43.0", ... }
  },
  "failed_packages": ["openssl@3"],
  "homebrew_prefix": "/opt/homebrew"
}
```

## Troubleshooting

### Link Conflict Errors

If you see errors like:
```
error: link conflict at '/opt/zerobrew/prefix/bin/xxx'
```

This package has a file conflict. Keep it in Homebrew:
```bash
brew install <package-name>
```

### Command Not Found After Migration

Ensure Zerobrew's bin directory is in your PATH:
```bash
export PATH="/opt/zerobrew/prefix/bin:$PATH"
```

Add to your `~/.zshrc` or `~/.bashrc`.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Related Projects

- [Zerobrew](https://github.com/lucasgelfond/zerobrew) - Fast Homebrew alternative
- [Homebrew](https://brew.sh/) - The missing package manager for macOS
