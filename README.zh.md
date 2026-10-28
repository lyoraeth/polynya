<div align="center">
  <img src="assets/logo/mark.svg" width="96" height="96" alt="Polynya Logo">
  <h1>Polynya</h1>
  <p><b>住在 Windows 11 任务栏里的轻量启动器</b></p>
  <p>
    启动器 · 时间与天气 · 带可视化的播放器 · 通知堆栈 · 任意语言的模块
  </p>
  <p>
    <a href="README.md">English</a> · <a href="README.ru.md">Русский</a> · <b>简体中文</b> · <a href="README.ja.md">日本語</a>
  </p>
  <p>
    <a href="docs/zh/guide.md">文档</a> ·
    <a href="docs/zh/module-api.md">模块 API</a> ·
    <a href="https://github.com/lyoraeth/polynya/releases">下载</a>
  </p>
</div>

---

<!-- TODO: main GIF/screenshot of the taskbar with an open dropdown -->
<!-- <p align="center"><img src="assets/preview.gif" width="100%" alt="Polynya Preview"></p> -->

**Polynya** 是一个快速启动器，它直接住在 Windows 11 任务栏的空闲区域里，把任务栏变成一块“活”的信息面板。

**Raycast**、**PowerToys Run** 这类传统启动器只在被调用时才工作：按下快捷键、完成操作、关闭，其余时间对你来说它们并不存在。Polynya 同样可以用 `Alt+Space` 启动程序、搜索文件、计算和调用工具，但它还在屏幕上拥有一个固定位置。因此信息会自己呈现，无需额外按键：时间和天气、带实时均衡器的播放器，以及来自后台命令和模块的通知始终在眼前，而周围的图标和托盘照常可以点击。

与各种小组件和 **Windows 版灵动岛（Dynamic Island）移植**不同，它不是一个悬浮在屏幕上、风格格格不入的独立窗口。Polynya 融入系统本身：实时采集任务栏的颜色、边缘、描边和颗粒质感，跟随主题、强调色和壁纸变化，下拉面板从任务栏中平滑“流出”，并过渡到系统的 Mica 材质。

它使用原生 **Rust** 编写，自带轻量渲染器：内部没有浏览器引擎（Electron / WebView），也没有沉重的 UI 框架。空闲时占用 **0% CPU**、约 **30 MB 内存**，在全屏游戏和视频中会完全休眠。

---

## 功能一览

* **搜索启动器（`Alt+Space` 或点击放大镜）。** 启动程序和内置功能（取色器、设置、模块命令）的入口。对传统程序和应用商店应用进行模糊搜索，通过 **Everything** 即时搜索文件，计算器（三角函数、带容差的比较如 `0.1 + 0.2 == 0.3`、字符串运算）以及货币换算（`100 usd to eur`、`100$`）。自带完整支持输入法（IME）和鼠标选择的输入框，结果中可直接右键打开操作菜单；模块功能显示为“模块 › 功能”，快捷键显示在单独的窄列中；被截断的行会在提示中显示全文。

* **播放器与可视化。** 适用于任何 Windows 媒体会话（Spotify、播放 YouTube 的浏览器、桌面播放器）。五段频谱通过 WASAPI process loopback *只监听播放器进程本身*的音频，所以 Telegram 的提示音或后台运行的游戏不会让它跳动。

* **时间与天气。** 数字滚动切换的时钟，悬停显示日出日落，通过 Open-Meteo 获取逐小时与三天预报（无需 API 密钥）。城市通过 Windows 定位确定，也可以在设置中手动输入，立即生效。

* **独立的通知与事件堆栈。** 一个支持“系列”合并、可折叠为 “+N” 堆叠的共享队列，把来自模块和后台进程的事件直接显示在任务栏中：
  * **终端中的长时间命令：** 当你在其他窗口时，若 Windows Terminal / VS Code 中的后台构建或命令结束（例如 `cargo build · 48 秒`），就会通知你；

  * **来自脚本和模块的自定义推送：** 在控制台中用一条 `polynya push` 命令，或由任何外部模块通过 JSON 管道发送。

* **无障碍。** 每个控件都有名称：讲述人、NVDA 和 JAWS 通过 UI Automation 朗读“岛”、搜索结果和模块界面，新通知也会被朗读出来。只有图标的按钮和被宽度截断的文本都有提示。

* **全屏时保持安静。** 在游戏、视频或演示中，任务栏里的一切都会隐藏并休眠：不绘制帧，也不分析音频。

---

## 模块

启动器、时间与天气、播放器和通知堆栈之外的一切都是模块：独立的进程，由“岛”为它绘制界面、绑定快捷键并传递选中的文本。Polynya 自带几个模块，你自己的模块也用同样的方式编写。

模块是 `%LOCALAPPDATA%\Polynya\modules\<id>\` 中带有 `module.toml` 清单的文件夹。
* **无需处理布局：** 模块只提供数据（通过会话的命名管道或 `stdin/stdout` 传递 JSON），“岛”会以与任务栏一致的风格绘制卡片、列表、开关、输入框和动画。
* **任意技术栈、任意许可证：** 可以用 Rust、Python、Go、C#、JS 或任何其他语言编写。协议规范和 `proto` crate 采用 **MIT** 许可证，因此你可以以任何许可证发布自己的模块，包括闭源和商业许可。
* **一个模块，一个界面：** `[main]` 把模块加入搜索，`[[functions]]` 把其菜单中的项以相同名称放入搜索并绑定全局快捷键，例如 `Ctrl+Alt+T` 上的“翻译 › 翻译所选内容”。用快捷键运行的功能可以拿到当前程序中选中的文本。
* **脚本直接运行：** `main.js`、`main.py` 或 `main.ps1` 通过已安装的 Node.js、Python 或 PowerShell 运行，无需构建；错误会以卡片形式显示并写入日志。

完整的协议、清单和界面行类型说明见 **[模块 API](docs/zh/module-api.md)**，用 Node.js 和 Python 编写的完整模块见 **[示例](docs/zh/module-examples.md)**。

### 随附模块

* **AI 代理（Claude Code、Codex、Cursor）。** 显示代理正在思考、编辑文件、等待输入或已完成任务。启用模块时自动注册钩子，无需手动折腾配置。
* **取色器。** 从启动器打开一个不冻结屏幕的实时放大镜，复制十六进制颜色值，并把颜色展开为 11 级 **OKLCH** 色阶（类似 Tailwind 色阶）。
* **Windows 通知（实验性，默认关闭）。** 用从任务栏弹出的紧凑面板替代标准的通知横幅（需要一次性通过 UAC 安装软件包并修改注册表中的 `ShowBanner`，因为 Windows 没有关闭其他应用横幅的官方 API）。

* **开发中：自定义“开始”菜单、开发工具和剪贴板历史。** 尚未随版本提供，将在后续版本中加入。自定义“开始”菜单在单独按下 `Win` 时打开一个快捷菜单，包含系统操作和“关于”页面，标准快捷键照常可用。开发工具用快捷键转换选中的值：`16px` 转 `1rem`、HEX/RGB/HSL/OKLCH 颜色互转、流式 `clamp()`、Unix 时间、JSON、JWT、Base64 和命名风格。剪贴板历史会保存复制过的内容，并能从“岛”中再次粘贴。

---

## 与同类工具对比

Raycast 和 PowerToys 这样的启动器遵循“需要时按快捷键调用”的模式。Polynya 完成启动器的工作，同时提供一块常驻屏幕、看起来就像 Windows 一部分的信息面板。

| 功能 | Polynya | Raycast for Windows | PowerToys Run / Command Palette |
| :--- | :--- | :--- | :--- |
| **定位** | 启动器 + “活”任务栏（始终在屏幕上） | 快捷键启动器 | 快捷键启动器（CmdPal 是 Run 的继任者） |
| **无需调用即可看到** | **是：** 时间、天气、播放器、通知、代理 | 否 | 否 |
| **外观** | 采集任务栏颜色、边缘和颗粒 + 过渡到 Mica | Raycast 风格的独立窗口 | WinUI 风格窗口 |
| **技术栈** | 原生 Rust，自研 Direct2D / DComp 渲染 | 原生 Raycast 客户端 | .NET / WinUI |
| **空闲资源占用** | **0% CPU，约 30 MB 内存** | *未测量* | *未测量* |
| **应用与文件搜索** | 是（文件通过 Everything） | 是（自有搜索） | 是 |
| **计算器与货币** | 是（函数、比较、字符串、汇率） | 是 | 是（货币通过插件） |
| **播放器与可视化** | **是**（只监听播放器进程的声音） | 否 | 否 |
| **通知堆栈与 CLI（`push`）** | **是**（Windows、终端、你的脚本） | 否 | 否 |
| **AI 代理状态** | **是**（Claude Code、Codex、Cursor，无需配置） | 否 | 否 |
| **OKLCH 取色器** | **是** | 否 | 否（取色器是套件中的独立工具） |
| **剪贴板历史、片段、窗口管理** | 否 | **是** | 否（PowerToys 中的独立工具） |
| **AI 聊天** | 否 | **是**（付费） | 否 |
| **模块（扩展）** | 任意语言（通过管道 / stdio 传 JSON，脚本直接运行），界面由“岛”绘制；模块功能支持全局快捷键和所选内容 | JS/TS 商店（部分仅限 macOS） | C# / .NET 扩展 |
| **价格与源码** | **免费、开源**（GPLv3 / MIT） | 免费；Pro 每月 $8 起，闭源 | **免费、开源**（MIT） |

> *注：竞品的功能和价格信息截至 2026 年年中，可能会变化；具体条款请以其官网为准。*

---

## 性能

测试机：Intel Core i5-10300H（4 核 8 线程），Intel UHD Graphics 核显，1920x1080，Windows 11（26100）。空闲时主线程只是在 `GetMessage` 中休眠，长曲名的滚动由 DWM 通过合成动画完成。

| 场景 | 平均 CPU | 峰值（1 秒窗口） | 内存（活动工作集） |
| :--- | :--- | :--- | :--- |
| **空闲（屏幕上显示时钟、天气、卡片）** | **0.00%** | - | **26 MB**（核心）/ **约 30 MB**（含所有模块） |
| **播放音乐（可视化运行中）** | **0.52%** | - | 约 30 MB |
| **搜索已打开（光标闪烁）** | **0.16%** | 0.58% | 约 35 MB |
| **打开过所有类型的下拉面板之后** | **0.00%**（空闲时） | 1.34 - 1.72%（动画期间） | **约 39 MB**（核心 + 所有常驻模块） |

---

## 安装与运行

**系统要求：** 仅支持 **Windows 11**（x64 或 ARM64）。有意不支持 Windows 10：这个工具的核心依赖新的任务栏和 Mica 材质。

1. 从 **[Releases](https://github.com/lyoraeth/polynya/releases)** 下载对应架构（`x64` 或 `arm64`）的最新版本：
   * **安装程序（`Polynya-<版本>-<arch>-setup.exe`）**：推荐方式。为当前用户安装，不需要管理员权限，可以选择要安装的模块；如果还没有 Everything，可以一并安装它用于文件搜索。之后运行新版本的安装程序即可原地更新，设置会保留。
   * **便携版（`…-portable.zip`）**：解压到任意位置并运行 `polynya.exe`，设置和日志都保存在同一文件夹中。模块单独放在 `…-modules.zip` 中，解压到同一文件夹即可。不包含 Everything。
2. “岛”会自动嵌入任务栏，托盘中会出现用于打开设置的图标。

卸载请前往 **设置 → 应用 → 已安装的应用**：Polynya 会移除自启动、AI 代理钩子和通知包，并询问是否一并删除设置和日志。

> **“Windows 已保护你的电脑”：** 目前构建尚未签名，SmartScreen 可能会拦截新版本的首次运行。点击 **更多信息 → 仍要运行** 即可。

> **关于管理员权限：** 请**以普通用户身份、不要以管理员身份**运行 Polynya。它不需要提升权限，提升反而有害：由于 Windows 的权限隔离（UIPI），以管理员身份运行的进程无法正常与 Everything 搜索通信。只有安装 Everything 和启用实验性的 Windows 系统通知截获模块时才会请求 UAC。

---

## 命令行（CLI）

可以直接在终端和脚本中向堆栈推送通知或控制“岛”：

```powershell
# 在任务栏中显示一个卡片（默认停留 10 秒）
polynya push "deploy done" "prod · 42 s" --icon ok --key deploy

# 用相同的 key 更新卡片（无需排队，直接排到第一位）
polynya push "tests failed" "error in auth" --icon fail --key deploy

# 按 key 移除通知
polynya dismiss deploy

# 重新采集任务栏颜色（在 Windhawk 等补丁之后很有用）
polynya refresh

# 修改模块代码后重启模块进程
polynya modules reload
```

---

## 文档

* **[用户指南](docs/zh/guide.md)**：“岛”的各个区块、搜索、播放器和天气如何工作，所有 `config.ini` 键和 CLI。
* **[通知与 AI 代理](docs/zh/notifications.md)**：队列与系列规则、Claude Code / Codex / Cursor 的自动钩子，以及 Windows 通知截获的工作原理。
* **[已知限制与常见问题](docs/zh/troubleshooting.md)**：边界情况（Windhawk、VPN 下的 Spotify 封面、Telegram Desktop 等）。
* **[模块 API](docs/zh/module-api.md)** 与 **[示例](docs/zh/module-examples.md)**：`module.toml` 规范、协议、快捷键与所选内容，以及从零开始的模块。
* **[架构](docs/zh/architecture.md)**：为什么用 DirectComposition 而不是交换链、如何采集任务栏颜色、OKLCH/APCA 的数学原理，以及解决过的各种棘手问题。
* **[构建与参与开发](docs/zh/contributing.md)**：工具链、测试、重新打包通知软件包以及本地化规则。

---

## 联系方式

* **问题与想法：** [GitHub Discussions](https://github.com/lyoraeth/polynya/discussions)
* **Bug：** [GitHub Issues](https://github.com/lyoraeth/polynya/issues)
* **Telegram：** [@lyoraeth_art](https://t.me/lyoraeth_art)
* **Discord：** `@lyoraeth`

---

## 开发方式

本项目在开发中使用了生成式 AI。
Polynya 采用 [agentic engineering](https://simonwillison.net/2026/may/6/vibe-coding-and-agentic-engineering) 的方式开发：
代码和文档由我与 [Claude Code](https://claude.com/claude-code)（Anthropic）共同编写。
创意、产品与界面决策、架构选择和视觉设计由我负责，Claude 按照我的指示编写实现。
为了让这些决策达到此类产品所需的水平，我亲自研读了 Rust 文档、Windows 开发者论坛以及同类项目的源代码；
Claude 的实现未能通过检验的部分，由我手动重写。

每一处改动在进入项目之前，都会在真实的 Windows 11 任务栏上实际验证；每次发布前还会在本地进行测试并检查边界情况。
如果遇到异常行为，请在 [issues](https://github.com/lyoraeth/polynya/issues) 或通过私信（联系方式见上方）描述问题，并附上日志（`%LOCALAPPDATA%\Polynya\logs\polynya.log`）。

我并非无所不知，但我在努力变得更好，也乐于学习。如果你发现了架构或语法上的错误，或代码中潜在的危险之处，
请在 [issues](https://github.com/lyoraeth/polynya/issues) 或 [讨论区](https://github.com/lyoraeth/polynya/discussions) 留言：问题在哪里、为什么不好、怎样做更好。非常感谢。

---

## 许可证与商标

* **Polynya 核心与内置模块**以 **[GNU GPLv3](LICENSE)** 发布，并附有第 7 条下的附加条款（在代码和“关于”窗口中保留署名、禁止把修改后的构建冒充为官方发行版、分支必须改名）。
* **交互协议与 `proto` crate** 单独以 **[MIT](proto/LICENSE)** 许可证授权，第三方模块可以按任何条款分发。
* **“Polynya”** 名称和标志归 Danil Klimov（`lyoraeth`）所有。可以做什么、不可以做什么，请见 **[TRADEMARKS.md](TRADEMARKS.md)**（仅有英文版，以英文版为准）。
