# Keyroost L10N v1.0.15

多语言硬件安全密钥管理工具
A multilingual hardware security key management tool

---

## 项目简介

Keyroost L10N 是 Keyroost 的多语言本地化分支，支持 FIDO2、OATH、OpenPGP、PIV 等安全密钥操作。项目采用分层架构设计，基于 Rust + egui/eframe 构建，提供完整的多语言界面支持。

Keyroost L10N is a multilingual localization branch of Keyroost, supporting FIDO2, OATH, OpenPGP, PIV and other security key operations. The project adopts a layered architecture, built with Rust + egui/eframe, providing complete multilingual interface support.

---

## 主要功能

- **FIDO2/CTAP2**: 通行密钥管理、指纹注册、安全策略配置
- **OATH**: TOTP/HOTP 凭证管理
- **OpenPGP**: 卡片信息、密钥管理、PIN 管理
- **PIV**: 插槽管理、证书操作、PIN/PUK 管理
- **设备缓存**: 自动保存和识别已连接的密钥
- **多语言支持**: 中文、英文，可扩展更多语言

---

## Features

- **FIDO2/CTAP2**: Passkey management, fingerprint enrollment, security policy configuration
- **OATH**: TOTP/HOTP credential management
- **OpenPGP**: Card information, key management, PIN management
- **PIV**: Slot management, certificate operations, PIN/PUK management
- **Device Caching**: Automatic save and recognition of connected keys
- **Multilingual Support**: Chinese, English, extensible to more languages

---

## v1.0.15 更新内容

### 多语言架构重构
进行了语言包语言加载的重构。从之前的添加语言需要根据源码重新翻译并编译，变成了只需要根据英文语言包的格式进行目标语言的翻译即可。

Refactored the language pack loading system. Previously, adding a new language required retranslating and recompiling from source code. Now, you only need to translate based on the English language pack format.

### 新增多语言适配
- 新增 `language/` 目录，包含 `en.json` 和 `zh-CN.json`
- 支持运行时语言切换，无需重新编译
- 添加新语言只需创建对应的 JSON 翻译文件

### Multilingual Adaptation
- Added `language/` directory with `en.json` and `zh-CN.json`
- Runtime language switching without recompilation
- Adding new languages only requires creating a corresponding JSON translation file

### 显示逻辑改进
- 刷新按钮现在会完整重载所有设备数据（名称、槽位、凭证等）
- 修复了多处硬编码中文字符串，统一使用翻译键
- 同步中英文语言包至 713 个翻译键

### Display Logic Improvements
- Refresh button now fully reloads all device data (names, slots, credentials, etc.)
- Fixed multiple hardcoded Chinese strings, unified to use translation keys
- Synchronized Chinese and English language packs to 713 translation keys

### 文件版本信息
- 文件说明: keyroost_l10n
- 文件版本: 1.0.15
- 产品名称: keyroost_l10n
- 产品版本: 1.0.15

### File Version Information
- File Description: keyroost_l10n
- File Version: 1.0.15
- Product Name: keyroost_l10n
- Product Version: 1.0.15

---

## 下载

从 GitHub Releases 下载最新版本：

- [keyroost_l10n-v1.0.15-x64.zip](https://github.com/Yvhany/keyroost_l10n/releases/download/v1.0.15/keyroost_l10n-v1.0.15-x64.zip) (Windows 64位)
- [keyroost_l10n-v1.0.15-x86.zip](https://github.com/Yvhany/keyroost_l10n/releases/download/v1.0.15/keyroost_l10n-v1.0.15-x86.zip) (Windows 32位)

## Download

Download the latest version from GitHub Releases:

- [keyroost_l10n-v1.0.15-x64.zip](https://github.com/Yvhany/keyroost_l10n/releases/download/v1.0.15/keyroost_l10n-v1.0.15-x64.zip) (Windows 64-bit)
- [keyroost_l10n-v1.0.15-x86.zip](https://github.com/Yvhany/keyroost_l10n/releases/download/v1.0.15/keyroost_l10n-v1.0.15-x86.zip) (Windows 32-bit)

---

## 添加新语言

只需在 `language/` 目录下创建新的 JSON 文件，参考 `en.json` 的格式进行翻译即可。

To add a new language, simply create a new JSON file in the `language/` directory, following the format of `en.json`.

### 示例 / Example

```json
{
  "language": "ja",
  "language_name": "日本語",
  "settings": "設定",
  "cancel": "キャンセル",
  "ok": "OK"
}
```

---

## 技术栈

- **语言**: Rust
- **GUI 框架**: egui/eframe
- **国际化**: JSON 语言包
- **构建系统**: Cargo

## Tech Stack

- **Language**: Rust
- **GUI Framework**: egui/eframe
- **Internationalization**: JSON language packs
- **Build System**: Cargo

---

## 构建

```bash
# 64位版本
cargo build --release --package keyroost --target x86_64-pc-windows-msvc

# 32位版本
cargo build --release --package keyroost --target i686-pc-windows-msvc
```

## Build

```bash
# 64-bit version
cargo build --release --package keyroost --target x86_64-pc-windows-msvc

# 32-bit version
cargo build --release --package keyroost --target i686-pc-windows-msvc
```

---

## 项目结构

```
keyroost_l10n/
├── language/           # 语言包目录
│   ├── en.json         # 英文翻译
│   └── zh-CN.json      # 中文翻译
├── cache/              # 设备缓存目录
├── config/             # 配置文件目录
├── crates/             # 源代码
│   └── keyroost/       # 主程序
└── keyroost_l10n.exe   # 可执行文件
```

## Project Structure

```
keyroost_l10n/
├── language/           # Language packs
│   ├── en.json         # English translations
│   └── zh-CN.json      # Chinese translations
├── cache/              # Device cache
├── config/             # Configuration files
├── crates/             # Source code
│   └── keyroost/       # Main application
└── keyroost_l10n.exe   # Executable
```

---

## 许可证

本项目基于 MIT 和 Apache-2.0 双重许可。

This project is licensed under MIT and Apache-2.0.

---

## 链接

- [GitHub 仓库](https://github.com/Yvhany/keyroost_l10n)
- [原始项目 Keyroost](https://github.com/framefilter/keyroost)

## Links

- [GitHub Repository](https://github.com/Yvhany/keyroost_l10n)
- [Original Project Keyroost](https://github.com/framefilter/keyroost)
