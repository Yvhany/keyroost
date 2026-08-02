# keyroost_l10n

keyroost 的中文本地化版本，提供完整的中文界面支持。

## 功能特性

- **FIDO2/CTAP2** - 通行密钥管理
- **OATH** - 动态验证码 (TOTP/HOTP)
- **OpenPGP** - 智能卡加密和签名
- **PIV** - 个人身份验证
- **设备管理** - 历史设备缓存、自动识别
- **多语言支持** - 中文/英文界面切换

## 下载

### Release 文件
- [keyroost_l10n-v1.0.2-zh-CN_x64.zip](https://github.com/Yvhany/keyroost/releases/download/v1.0.2/keyroost_l10n-v1.0.2-zh-CN_x64.zip) - 64位版本
- [keyroost_l10n-v1.0.2-zh-CN_x86.zip](https://github.com/Yvhany/keyroost/releases/download/v1.0.2/keyroost_l10n-v1.0.2-zh-CN_x86.zip) - 32位版本

### Release 包内容
```
keyroost_l10n-v1.0.2-zh-CN_x64/
├── keyroost_l10n-v1.0.2-zh-CN_x64.exe
└── language/
    ├── en.json
    └── zh-CN.json
```

## 使用说明

1. 解压 zip 文件
2. 运行 `keyroost_l10n-v1.0.2-zh-CN_x64.exe` 或 `_x86.exe`
3. 语言会自动检测系统语言（优先中文）

## 功能说明

### 密钥保存
- 支持将当前已连接的密钥保存到本地
- 下次插入该密钥时自动识别并加载保存的信息
- 保存内容：密钥名称、序列号、备注信息

### 设备列表排序
- 已连接的设备始终排在列表最前面
- 未连接的设备（历史设备）按名称 A → Z 排序
- 排序规则：已连接 > 未连接（字母序）

### 设备状态显示
- 标题栏显示设备列表和已连接设备数量
- 格式："设备列表(X)  已连接:Y"

### 设备列表右键菜单
- 命名密钥
- 查看序列号
- 从历史记录中移除

### 设置页面
- 语言切换（中文/英文）
- 文字大小调整（5%步进）
- 深色模式切换
- 主题颜色选择

## 开发说明

### 编译环境
- Rust 1.85+
- Windows SDK
- 代码签名证书：yvhan_dev_RSA

### 编译命令
```bash
# 64位
cargo build --release --package keyroost --target x86_64-pc-windows-msvc

# 32位
cargo build --release --package keyroost --target i686-pc-windows-msvc

# 签名
signtool sign /a /fd SHA256 /sha1 <thumbprint> file.exe
```

### 翻译文件
语言包位于 `language/` 目录：
- `en.json` - 英文翻译
- `zh-CN.json` - 中文翻译
- `app.yml` - 翻译键定义

### 添加新语言
1. 在 `language/` 目录下创建新的 JSON 文件（如 `ja.json`）
2. 包含 `language` 和 `language_name` 字段
3. 添加所有翻译键值对

## 致谢

- 原始项目: [keyroost](https://github.com/framefilter/keyroost)
- 作者: framefilter

## 许可证

MIT License
