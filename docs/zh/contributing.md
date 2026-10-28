# 构建与参与开发

[English](../../CONTRIBUTING.md) · [Русский](../ru/contributing.md) · **简体中文** · [日本語](../ja/contributing.md)

感谢你对 Polynya 开发的兴趣！本文介绍如何搭建环境、构建核心和模块、运行测试以及提交你的修改。

---

## 1. 工具与依赖

项目使用纯 Rust 编写，不依赖外部 UI 框架（只使用 `windows` 0.62、`serde` / `serde_json`、`toml` crate，以及构建时的 `embed-resource`）。

构建需要：
* **Windows 11**（x64 或 ARM64）；
* **Rust**：版本固定在 `rust-toolchain.toml` 中，`rustup` 会自动安装。GNU 工具链加 **MinGW-w64** 或 MSVC 都可以（普通编译不需要完整的 Windows SDK）；发布版本使用 MSVC 构建。
* **Inno Setup 6.5+**：仅用于构建安装程序（`winget install JRSoftware.InnoSetup`）。

---

## 2. 构建

整个项目是一个 Cargo 工作区（核心 `polynya`、协议 crate `proto` 以及位于 `modules/{agents,notify,picker,start}` 的 4 个内置模块）。

以 release 配置一次性构建全部内容：
```powershell
cargo build --release --workspace
```

构建完成后，`target/release/` 中会出现：
* `polynya.exe`：“岛”的核心和 CLI 客户端；
* `polynya-agents.exe`、`polynya-notify.exe`、`polynya-picker.exe`：内置模块的程序；
* `polynya-start.exe`：自定义“开始”菜单模块。它和 Node.js 编写的 `modules/devtools` 一样位于仓库中（`modules/start`），但两者都暂未随版本发布。

*关于版本号：* `build.rs` 会自动从 `git` 计算版本号（在 `vX.Y.Z` 标签上为 `X.Y.Z`，标签之间为 `X.Y.Z+提交数.哈希`，第一个标签之前为 `1.0.0-dev.N+哈希`），并写入可执行文件的 `VERSIONINFO`。

---

## 3. 本地部署与调试

运行时 `polynya.exe` 以 `%LOCALAPPDATA%\Polynya\` 作为自己的工作目录。在本地测试新构建：

1. 用 `polynya quit` 停止正在运行的“岛”（常驻模块会随之退出）。
2. 把构建出的 `polynya.exe` 复制到 `%LOCALAPPDATA%\Polynya\`，把模块程序（连同它们的 `module.toml`）复制到 `%LOCALAPPDATA%\Polynya\modules\<id>\`。
3. **重要：** 请从 `explorer.exe` 或普通（未提升权限！）的终端启动 `polynya.exe`。如果你的 IDE 或终端以管理员身份运行，“岛”的子进程会继承提升后的令牌，由于 UIPI 会破坏与 Everything 的通信。

### 调试时的日志
核心和所有模块的共用日志写入 `%LOCALAPPDATA%\Polynya\logs\polynya.log`。要启用详细输出（目录遍历耗时、横幅处理决策、任务栏采集参数），请在 `%APPDATA%\Polynya\config.ini` 中添加一行：
```ini
log = debug
```

---

## 4. 测试

项目有 85 个自动化测试，覆盖通知队列与系列逻辑、清单解析、`proto` 序列化、APCA/OKLCH 色彩计算、AI 代理配置合并、计算器解析器和 CLI。

运行全部单元测试：
```powershell
cargo test --release --workspace
```

用于检查与真实 Windows 环境集成的测试（GSMTC 媒体会话缩略图、系统定位等）默认被忽略：
```powershell
cargo test --release --workspace -- --ignored
```

提交 Pull Request 之前，请确认 `--workspace` 中的所有测试都通过。

### 代码风格
* **格式化：** `cargo fmt --all`（配置见 `rustfmt.toml`）。`rustfmt` 不会重排注释，请手动把注释控制在约 100 列以内。
* **注释：** 使用英文；解释*为什么*，而不是复述代码。模块以 `//!` 开头，公开项用 `///` 说明。
* **`unsafe`：** 每个代码块都要有带真实理由的 `// SAFETY:` 注释（句柄归谁所有、缓冲区存活多久）。整个工作区启用了 `clippy::undocumented_unsafe_blocks` lint：`cargo clippy --workspace` 不应产生新的警告。
* **文件头：** 每个源文件以 `Copyright` 和 `SPDX-License-Identifier` 两行开头（`GPL-3.0-or-later`，`proto` crate 为 `MIT`）。

---

## 5. 辅助脚本（`tools/`）

日常开发中无需重新生成资源（现成的文件已在仓库中并在编译时嵌入），但如果你修改了标志或通知软件包的清单：
* **`tools/package.ps1`** 为 `notify` 模块重新构建未签名的稀疏 `.msix` 软件包（它会自行从 NuGet 包 `Microsoft.Windows.SDK.BuildTools` 下载轻量的 `makeappx`）。**重要：** 每次修改软件包清单时，务必同时提升清单中的版本号和 `src/package.rs` 中 `VERSION` 常量的值，这样核心在启动时才能发现更新。
* **`tools/release.ps1`** 为一种架构构建发布文件，输出到 `target/dist`：安装程序（通过 `installer/build.ps1` 编译 `installer/polynya.iss`，该脚本会下载 Everything 安装程序并校验其 SHA-256）、便携版压缩包及其模块压缩包。`Release` 工作流在打上 `vX.Y.Z` 标签时为两种架构运行它，并创建发布草稿。
* **`tools/logo.py` 与 `tools/ascii.py`** 从源文件 `assets/logo/mark.svg` 生成 `.ico`、带颗粒的位图和矢量标志，以及彩色 ASCII 布局 `assets/logo/mark-ascii.json`。

---

## 6. 本地化与文案规则（`i18n`）

界面支持 4 种语言：俄语（`ru`）、英语（`en`）、中文（`zh`）和日语（`ja`）。核心的字符串位于 `src/i18n.rs`，模块的字符串位于各自的代码和 `module.toml` 中。添加字符串时请遵循项目的统一规则：

1. **大小写：** 独立的标题和设置项名称首字母大写；“对象 + 状态”形式的复合状态中，对象之后的部分小写（例如 `"Caps Lock on"`、`"#ff0000 copied"`）。
2. **省略号：** 持续进行的过程和输入占位符统一使用单个字符 `…`（`"思考中…"`、`"输入城市…"`）；带对象的代理操作字符串不加省略号（`"Editing main.rs"`）。
3. **语气与术语：** 严格保持中性，不使用“你”和第一人称。系统命令使用 Windows 在相应语言中的准确措辞（*"Завершение работы"* / *"Shut down"* / *"关机"* / *"シャットダウン"*）。确认字符串（`confirm`）只写成简短的问句。

文档也使用这四种语言：英文为主版本（`README.md`、`docs/*.md`），翻译位于 `README.<语言>.md` 和 `docs/<语言>/`。法律文本（`LICENSE`、`TRADEMARKS.md`）只有英文版。

---

## 7. 贡献者许可条款

向本仓库提交 Pull Request，即表示你同意你的代码：
* 作为核心和内置模块的一部分时，按 **[LICENSE](../../LICENSE)**（GNU GPLv3 + 第 7 条附加条款）分发；
* 作为 `proto` crate 和协议文档的一部分时，按 **[proto/LICENSE](../../proto/LICENSE)**（MIT 许可证）分发。
