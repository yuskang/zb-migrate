# zb-migrate

繁體中文 | [English](README.md)

將 Homebrew 套件遷移到 [Zerobrew](https://github.com/lucasgelfond/zerobrew) 的命令列工具。

## 功能特色

- 列出所有已安裝的 Homebrew 套件（formulae 和 casks）
- 匯出為 Brewfile 格式
- 按依賴順序遷移套件到 Zerobrew
- 追蹤遷移狀態
- 透過 Zerobrew 處理後續更新
- 遷移成功後清理 Homebrew

## 安裝方式

### 一鍵安裝

```bash
curl -fsSL https://raw.githubusercontent.com/yuskang/zb-migrate/main/install.sh | bash
```

### 從原始碼編譯

```bash
git clone https://github.com/yuskang/zb-migrate.git
cd zb-migrate
cargo build --release
cp target/release/zb-migrate /usr/local/bin/
```

### 透過 Cargo 安裝

```bash
cargo install --git https://github.com/yuskang/zb-migrate.git
```

## 前置需求

- 已安裝 [Homebrew](https://brew.sh/)
- 已安裝 [Zerobrew](https://github.com/lucasgelfond/zerobrew)
- Rust 工具鏈（從原始碼編譯時需要）

## 使用方式

### 列出已安裝套件

```bash
# 僅列出 formulae
zb-migrate list

# 包含 casks
zb-migrate list --casks

# 以 JSON 格式輸出
zb-migrate list --json
```

### 匯出 Brewfile

```bash
zb-migrate export -o ~/Brewfile.zerobrew
```

### 遷移套件

```bash
# 預覽遷移（建議先執行）
zb-migrate migrate --dry-run

# 執行遷移
zb-migrate migrate

# 僅遷移特定套件
zb-migrate migrate -p git -p node
```

### 檢查可用更新

```bash
zb-migrate outdated
```

### 更新所有套件

```bash
zb-migrate upgrade
```

### 查看遷移狀態

```bash
zb-migrate status
```

### 清理 Homebrew

確認一切正常後：

```bash
# 預覽
zb-migrate cleanup

# 執行清理
zb-migrate cleanup --force
```

## 已知限制

### Zerobrew 的限制

由於 Zerobrew 的架構設計，部分套件可能無法遷移：

| 問題 | 受影響的套件 | 解決方案 |
|------|-------------|----------|
| **連結衝突** | `openssl@3`、`python@3.x` 及其依賴套件 | 保留在 Homebrew |
| **不支援 Casks** | 所有 GUI 應用程式（`.app`） | 繼續使用 `brew install --cask` |
| **第三方 Tap 套件** | 部分非官方 tap 的套件 | 可能需要手動處理 |

### 經常失敗的套件

以下套件通常有連結衝突，建議保留在 Homebrew：

- `openssl@3` - 核心 SSL 函式庫，許多套件依賴它
- `python@3.x` - Python 直譯器
- `libevent`、`gnutls`、`nghttp2` - 網路函式庫
- `gobject-introspection` - GLib 內省工具
- `node@xx` - Node.js 版本

### 建議的共存策略

| 管理工具 | 套件類型 |
|---------|---------|
| **Zerobrew** | 大多數 CLI 工具、公用程式 |
| **Homebrew** | OpenSSL 相關套件、Casks、有問題的套件 |

### 更新策略

```bash
# Zerobrew 管理的套件
zb upgrade

# Homebrew 管理的套件
brew upgrade
```

## 運作原理

1. **讀取套件清單**：使用 `brew list --formula --versions` 取得已安裝套件
2. **解析依賴關係**：拓撲排序以確保正確的安裝順序
3. **執行遷移**：透過 `zb install` 逐一安裝
4. **追蹤狀態**：將遷移狀態儲存到 `~/.zerobrew/migration_state.json`
5. **管理更新**：後續透過 `zb upgrade` 統一更新

## 遷移狀態檔案

遷移狀態儲存於 `~/.zerobrew/migration_state.json`：

```json
{
  "migrated_packages": {
    "git": { "name": "git", "version": "2.43.0", ... }
  },
  "failed_packages": ["openssl@3"],
  "homebrew_prefix": "/opt/homebrew"
}
```

## 疑難排解

### 連結衝突錯誤

如果看到類似這樣的錯誤：
```
error: link conflict at '/opt/zerobrew/prefix/bin/xxx'
```

表示該套件有檔案衝突，請保留在 Homebrew：
```bash
brew install <套件名稱>
```

### 遷移後找不到指令

確保 Zerobrew 的 bin 目錄在 PATH 中：
```bash
export PATH="/opt/zerobrew/prefix/bin:$PATH"
```

將上述內容加入 `~/.zshrc` 或 `~/.bashrc`。

## 貢獻

歡迎貢獻！請隨時提交 Pull Request。

## 授權

MIT 授權 - 詳見 [LICENSE](LICENSE)。

## 相關專案

- [Zerobrew](https://github.com/lucasgelfond/zerobrew) - 快速的 Homebrew 替代方案
- [Homebrew](https://brew.sh/) - macOS 的套件管理工具
