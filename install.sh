#!/bin/bash
# zb-migrate installer
# https://github.com/georgekang/zb-migrate

set -e

# Configuration
REPO="yuskang/zb-migrate"
BINARY_NAME="zb-migrate"
INSTALL_DIR="/usr/local/bin"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Detect architecture
detect_arch() {
    local arch=$(uname -m)
    case $arch in
        x86_64)  echo "x86_64-apple-darwin" ;;
        arm64)   echo "aarch64-apple-darwin" ;;
        aarch64) echo "aarch64-apple-darwin" ;;
        *)
            echo "unsupported"
            ;;
    esac
}

# Detect OS
detect_os() {
    local os=$(uname -s)
    case $os in
        Darwin) echo "macos" ;;
        Linux)  echo "linux" ;;
        *)      echo "unsupported" ;;
    esac
}

# Language selection / 語言選擇
select_language() {
    echo ""
    echo "Please select language / 請選擇語言:"
    echo "  1) English"
    echo "  2) 繁體中文"
    echo ""
    read -p "Enter choice [1-2] / 輸入選項 [1-2]: " lang_choice

    case $lang_choice in
        2) LANG="zh" ;;
        *) LANG="en" ;;
    esac
}

# Messages
msg() {
    local key=$1
    case $LANG in
        zh)
            case $key in
                "welcome") echo "歡迎使用 zb-migrate 安裝程式" ;;
                "arch_detect") echo "偵測到架構: $2" ;;
                "arch_intel") echo "Intel Mac (x86_64)" ;;
                "arch_arm") echo "Apple Silicon Mac (arm64)" ;;
                "arch_unsupported") echo "不支援的架構: $2" ;;
                "checking_deps") echo "正在檢查依賴項..." ;;
                "rust_not_found") echo "未找到 Rust。正在安裝 Rust..." ;;
                "brew_not_found") echo "未找到 Homebrew。請先安裝 Homebrew: https://brew.sh" ;;
                "zb_not_found") echo "未找到 Zerobrew。建議先安裝: https://github.com/lucasgelfond/zerobrew" ;;
                "downloading") echo "正在下載預編譯二進制檔..." ;;
                "download_failed") echo "下載失敗，改為從源碼編譯..." ;;
                "cloning") echo "正在複製儲存庫..." ;;
                "building") echo "正在編譯（這可能需要一些時間）..." ;;
                "installing") echo "正在安裝..." ;;
                "success") echo "安裝完成！" ;;
                "usage") echo "使用方式:" ;;
                "usage_list") echo "  zb-migrate list              # 列出 Homebrew 套件" ;;
                "usage_dry") echo "  zb-migrate migrate --dry-run # 預覽遷移" ;;
                "usage_migrate") echo "  zb-migrate migrate           # 執行遷移" ;;
                "usage_help") echo "  zb-migrate --help            # 查看更多指令" ;;
                "cleanup") echo "正在清理暫存檔案..." ;;
                "error") echo "安裝過程中發生錯誤" ;;
                "docs") echo "文件: https://github.com/$REPO" ;;
                "installed_to") echo "已安裝到: $2" ;;
                "add_path") echo "請將以下路徑加入 PATH: $2" ;;
            esac
            ;;
        *)
            case $key in
                "welcome") echo "Welcome to zb-migrate installer" ;;
                "arch_detect") echo "Detected architecture: $2" ;;
                "arch_intel") echo "Intel Mac (x86_64)" ;;
                "arch_arm") echo "Apple Silicon Mac (arm64)" ;;
                "arch_unsupported") echo "Unsupported architecture: $2" ;;
                "checking_deps") echo "Checking dependencies..." ;;
                "rust_not_found") echo "Rust not found. Installing Rust..." ;;
                "brew_not_found") echo "Homebrew not found. Please install first: https://brew.sh" ;;
                "zb_not_found") echo "Zerobrew not found. Recommended to install: https://github.com/lucasgelfond/zerobrew" ;;
                "downloading") echo "Downloading pre-built binary..." ;;
                "download_failed") echo "Download failed, falling back to source build..." ;;
                "cloning") echo "Cloning repository..." ;;
                "building") echo "Building (this may take a while)..." ;;
                "installing") echo "Installing..." ;;
                "success") echo "Installation complete!" ;;
                "usage") echo "Usage:" ;;
                "usage_list") echo "  zb-migrate list              # List Homebrew packages" ;;
                "usage_dry") echo "  zb-migrate migrate --dry-run # Preview migration" ;;
                "usage_migrate") echo "  zb-migrate migrate           # Execute migration" ;;
                "usage_help") echo "  zb-migrate --help            # See more commands" ;;
                "cleanup") echo "Cleaning up temporary files..." ;;
                "error") echo "An error occurred during installation" ;;
                "docs") echo "Documentation: https://github.com/$REPO" ;;
                "installed_to") echo "Installed to: $2" ;;
                "add_path") echo "Please add to PATH: $2" ;;
            esac
            ;;
    esac
}

# Print colored message
print_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
print_success() { echo -e "${GREEN}[OK]${NC} $1"; }
print_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
print_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Check command exists
check_command() {
    command -v "$1" >/dev/null 2>&1
}

# Try to download pre-built binary
try_download_binary() {
    local arch=$1
    local tmp_dir=$2

    print_info "$(msg downloading)"

    # Get latest release URL
    local release_url="https://github.com/$REPO/releases/latest/download/${BINARY_NAME}-${arch}.tar.gz"

    if curl -fsSL "$release_url" -o "$tmp_dir/binary.tar.gz" 2>/dev/null; then
        tar -xzf "$tmp_dir/binary.tar.gz" -C "$tmp_dir" 2>/dev/null
        if [ -f "$tmp_dir/$BINARY_NAME" ]; then
            chmod +x "$tmp_dir/$BINARY_NAME"
            return 0
        fi
    fi

    return 1
}

# Build from source
build_from_source() {
    local tmp_dir=$1

    # Check/Install Rust
    if ! check_command cargo; then
        print_warn "$(msg rust_not_found)"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
    print_success "Rust ✓"

    # Clone repository
    print_info "$(msg cloning)"
    git clone --depth 1 "https://github.com/$REPO.git" "$tmp_dir/zb-migrate" 2>/dev/null || {
        print_error "$(msg error)"
        exit 1
    }

    cd "$tmp_dir/zb-migrate"

    # Build
    print_info "$(msg building)"
    cargo build --release --quiet

    cp target/release/$BINARY_NAME "$tmp_dir/$BINARY_NAME"
}

# Install binary
install_binary() {
    local tmp_dir=$1
    local binary_path="$tmp_dir/$BINARY_NAME"

    print_info "$(msg installing)"

    # Try /usr/local/bin first (requires sudo)
    if [ -w "$INSTALL_DIR" ] || sudo -n true 2>/dev/null; then
        sudo cp "$binary_path" "$INSTALL_DIR/" 2>/dev/null && {
            print_success "$(msg installed_to "$INSTALL_DIR/$BINARY_NAME")"
            return 0
        }
    fi

    # Fallback to ~/.local/bin
    local local_bin="$HOME/.local/bin"
    mkdir -p "$local_bin"
    cp "$binary_path" "$local_bin/"
    chmod +x "$local_bin/$BINARY_NAME"

    print_success "$(msg installed_to "$local_bin/$BINARY_NAME")"

    # Check if in PATH
    if [[ ":$PATH:" != *":$local_bin:"* ]]; then
        print_warn "$(msg add_path "$local_bin")"
        echo ""
        echo "  # Add to ~/.zshrc or ~/.bashrc:"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
}

# Main installation
main() {
    select_language

    echo ""
    echo "========================================"
    print_info "$(msg welcome)"
    echo "========================================"
    echo ""

    # Detect architecture
    local arch=$(detect_arch)
    local os=$(detect_os)

    if [ "$os" != "macos" ]; then
        print_error "$(msg arch_unsupported "$os")"
        exit 1
    fi

    case $arch in
        x86_64-apple-darwin)
            print_info "$(msg arch_detect "$(msg arch_intel)")"
            ;;
        aarch64-apple-darwin)
            print_info "$(msg arch_detect "$(msg arch_arm)")"
            ;;
        *)
            print_error "$(msg arch_unsupported "$arch")"
            exit 1
            ;;
    esac

    # Check dependencies
    print_info "$(msg checking_deps)"

    # Check Homebrew
    if ! check_command brew; then
        print_error "$(msg brew_not_found)"
        exit 1
    fi
    print_success "Homebrew ✓"

    # Check Zerobrew (warning only)
    if ! check_command zb; then
        print_warn "$(msg zb_not_found)"
    else
        print_success "Zerobrew ✓"
    fi

    echo ""

    # Create temp directory
    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    # Try pre-built binary first, fallback to source build
    if ! try_download_binary "$arch" "$TEMP_DIR"; then
        print_warn "$(msg download_failed)"
        build_from_source "$TEMP_DIR"
    fi

    # Install
    install_binary "$TEMP_DIR"

    echo ""
    echo "========================================"
    print_success "$(msg success)"
    echo "========================================"
    echo ""

    # Print usage
    msg "usage"
    echo ""
    msg "usage_list"
    msg "usage_dry"
    msg "usage_migrate"
    msg "usage_help"
    echo ""
    msg "docs"
    echo ""
}

# Run
main "$@"
