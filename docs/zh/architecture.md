# Polynya 的架构与内部实现

[English](../architecture.md) · [Русский](../ru/architecture.md) · **简体中文** · [日本語](../ja/architecture.md)

本文介绍项目的架构与实现：不依赖框架的渲染如何工作、为什么窗口使用 DirectComposition 表面而不是普通的交换链、任务栏的颜色和纹理究竟如何采集，以及绕过了哪些不简单的 Windows 问题。

---

## 1. 多进程结构与线程

Polynya 分为独立的核心和隔离的模块进程，任何模块崩溃都不会拖垮“岛”：

| 进程 | 生命周期 | 与核心的通信 |
| :--- | :--- | :--- |
| **`polynya.exe`**（核心） | 常驻 | — |
| **`polynya-start.exe`**（“开始”菜单，开发中） | 常驻模块（`resident`） | `stdin` / `stdout`（JSON 行） |
| **`polynya-notify.exe`**（Windows 通知） | 常驻，通过稀疏软件包激活 | 命名管道（`proto::send`） |
| **`polynya-agents.exe`**（AI 代理钩子） | 由钩子事件启动，随即退出 | 命名管道 |
| **`polynya-picker.exe`**（取色器） | 由自己的搜索命令启动，取色后退出 | `stdin` / `stdout`，卡片通过命名管道发送 |
| **`devtools`**（Node.js 上的 `main.js`，开发中） | 由第一次命令或快捷键启动，之后持续运行 | `stdin` / `stdout`（JSON 行） |
| **`polynya.exe push/...`**（CLI 客户端） | 发送一条消息后退出 | 命名管道 |

### 核心（`polynya.exe`）内部的线程
* **UI 线程：** 窗口、`GetMessage` 消息循环、定时器，以及全部 Direct2D / DirectComposition 渲染。
* **`ipc`：** 命名管道 `\\.\pipe\Polynya-<SessionId>` 的服务端。
* **`media`：** 订阅系统媒体会话（GSMTC）并补读迟到的封面。
* **`audio`：** 通过 WASAPI process loopback 采集播放器进程的音频并计算频谱。
* **`apps` 与 `icons`：** 在后台遍历 `shell:AppsFolder` 并提取外壳图标（`IShellItemImageFactory`）。
* **鼠标钩子：** *仅在下拉面板打开期间*在单独线程中启用（放在 UI 线程上的全局钩子会让整个系统的光标出现微小延迟）。
* **`module-<id>`：** 读取模块进程的 `stdout` 和 `stderr`，并跟踪其退出。
* **天气与 Everything IPC：** 在后台请求 Open-Meteo / Nominatim，并通过 `WM_COPYDATA` 与 Everything 交换数据。

---

## 2. 窗口与渲染（Direct2D + DirectComposition）

* **没有重定向位图的窗口：** 以 `WS_EX_NOREDIRECTIONBITMAP | TOPMOST | TOOLWINDOW | NOACTIVATE` 样式创建。窗口的所有者被设为任务栏本身（`Shell_TrayWnd`）：在 Win32 中，被拥有的窗口总是绘制在所有者之上，因此任务栏自行置顶时永远不会盖住“岛”（资源管理器重启后，通过 `TaskbarCreated` 恢复绑定）。
* **为什么用 DirectComposition 表面而不是 DXGI 交换链：** 在 Intel 核显上，第一个交换链无论大小都会占用约 50 MB 的驱动内部内存。改用纯 DirectComposition 表面（`IDCompositionSurface`）后，核心空闲时的内存从 67 MB 降到 **17-26 MB**。
* **在核显上渲染：** 在双显卡系统中，核心有意选择集成显卡。Windows 系统合成器（DWM）在那里运行并绘制桌面，因此纹理不必在显卡之间来回传输，独立显卡也不会为了一个界面工具被唤醒，可以保持深度休眠。
* **点击穿透与窗口区域（`SetWindowRgn`）：** 窗口横跨整个任务栏宽度，若不加限制，它会拦截对系统托盘和程序图标的所有点击。为了让空白处让鼠标穿过，窗口设置了一个精确沿可见区块轮廓的形状遮罩（`SetWindowRgn`）。打开下拉面板时，遮罩会提前（在该帧 `Commit` 之前）直接扩展到面板的最终大小，并为阴影和圆角留出余量；否则在弹簧动画期间，画面边缘会撞上尚未长大的区域而被裁切。窗口内部的鼠标命中测试则仍按下拉面板精确的凹形轮廓计算。
* **只在变化时渲染：** 空闲时主线程完全休眠在 `GetMessage` 中（0.00% CPU）。展开动画以显示器刷新率运行（`Commit` + `DwmFlush`），而背景动画（可视化、滚动字幕）在任何显示器上都由定时器限制为正好 **30 fps**（在 144 Hz 显示器上，不加定时器地等待 `DwmFlush` 会多出 72 fps）。
* **由 DWM 实现的硬件滚动字幕：** 长曲名只绘制一次到单独的合成表面上（`scroller.rs`），滚动本身由 Windows 系统合成器通过最近邻插值的 `IDCompositionAnimation` 完成（文字按整像素移动，不会模糊）。滚动时的 CPU 占用从 0.36% 降到 **0.03%**。由于同一设备的两个表面不能同时绘制（`SURFACE_BEING_RENDERED`），滚动字幕的图层严格在主帧结束后绘制，并通过同一个 `Commit` 提交。
* **启动时预热着色器：** 首次使用 Direct2D 效果（`Turbulence`、`DisplacementMap`、`Shadow`）需要编译着色器（约 290 ms），这会让第一次打开下拉面板时卡顿。因此核心在启动时用 16x16 的缓冲区空跑这些效果，第一次真正打开只需 **2.1 ms**。

---

## 3. 适配任务栏与颜色模型

### 如何采集任务栏外观（`look.rs`）
通过 Windows Graphics Capture（WGC）捕获任务栏窗口行不通：其中没有 DWM 的模糊背景。因此改为对任务栏所在的屏幕条带执行 `BitBlt`。在截图期间（23–46 ms），“岛”自身的窗口会被标记为 `WDA_EXCLUDEFROMCAPTURE`，以免拍到自己。

从截图中提取：
1. 任务栏主题是浅色还是深色，以及系统强调色；
2. **边缘行**（逐列取任务栏边缘的像素颜色，使用 48 px 中值滤波和 8 px 平滑，避免图标或“开始”按钮在接缝处留下色斑）；
3. **描边行**（沿任务栏边缘的细线，前提是亮度差 ≥ 0.02）；
4. 任务栏纹理的**颗粒（噪点）**。

没有任何定时轮询：只在事件发生时重新采集（以 1 秒去抖合并）：系统启动（外加 2 / 5 / 15 / 30 秒后的补充采集，因为资源管理器会延迟绘制背景）、壁纸变化、主题变化（`ImmersiveColorSet`）、强调色变化（`WM_DWMCOLORIZATIONCOLORCHANGED`）、高对比度、资源管理器重启（`TaskbarCreated`）、解锁以及从睡眠中唤醒。

### OKLCH 色彩空间与 APCA 对比度（`tone.rs`）
* **颜色规则：** 屏幕上常驻可见的一切（时间和搜索的底板、按钮、已开启的开关）都使用**任务栏自身的色调**，而不是系统强调色（强调色常常与壁纸冲突）。系统强调色只用于在用户操作时（悬停或方向键选择）高亮某一行。
* **底板（`tone::plate`）：** 在 **OKLCH** 空间中取任务栏的色相，亮度则按目标感知对比度 **APCA**（主文字 `Lc 60`、次要文字 `Lc 45`）选取，彩度 `≤ 0.06`，不透明度 20–45%。

### 下拉面板：与任务栏的接缝以及向 Mica 的过渡
* **接缝几何（`shape.rs`）：** 下拉面板通过对齐到整数物理像素的凹形圆角从任务栏中长出（在小数坐标上，边缘会在动画中闪烁）。
* **无缝的描边：** 任务栏原生的 1 像素描边在打开的下拉面板下方被擦除，换成下拉面板的轮廓，两侧各带 2 DIP 的水平直线“尾巴”（否则平滑的 Direct2D 圆弧无法与像素线无缝衔接，尖端会出现暗点）。
* **向 Mica 过渡：** 在接缝处，下拉面板的背景等于任务栏边缘颜色，在 56 DIP 内平滑过渡到 **Mica**（当前桌面壁纸，以屏幕 1/8 的分辨率模糊一次，并按 OKLab 亮度差以最高 0.65 的系数染上任务栏颜色）。上面叠加带湍流边缘的暗角（`Turbulence` + `DisplacementMap`）和 128x128 的颗粒平铺。Mica 与阴影合并为同一图层，避免两个独立的 Alpha 遮罩因双重抗锯齿产生浅色描边。

---

## 4. 不简单的工程决策

### 文字与字体
* **不占内存的内置字体：** Inter 字体的子集（180 KB 而非 410 KB）直接嵌入 `.exe`。DirectWrite 标准的 `CreateInMemoryFontFileReference` 会把字体缓冲区复制到进程的私有堆中；这里改用自定义加载器并传入所有者对象，让 DirectWrite 直接从磁盘上映射到内存的 `.exe` 映像中读取字形（私有工作集中占用 0 字节）。
* **字形悬垂与等宽数字：** 文字宽度考虑了悬垂度量（避免日期压到倾斜或粗体数字上），等宽数字严格在应用 OpenType `tnum` 特性之后测量。分钟滚动时整行一次排版，再按字符单元裁剪，避免数字重影。

### 输入、焦点与按键截获
* **启动器与焦点：** 平时窗口带有 `WS_EX_NOACTIVATE`，点击时返回 `MA_NOACTIVATE`，因此点击播放器不会夺走用户当前窗口的焦点。打开搜索或带输入的菜单时，临时去掉 `WS_EX_NOACTIVATE`，按 `Esc` 后焦点返回原来的窗口。
* **搜索中单独按下 `Alt`：** 在 Win32 中按下 `Alt` 会让窗口进入系统菜单模式，之后的字母会以 `WM_SYSCHAR` 到达并伴随错误提示音。启动器会吞掉 `SC_KEYMENU`，未按住 `Alt` 的 `WM_SYSCHAR` 按普通输入处理。
* **继承输入法状态：** 为了避免每次打开搜索时中文/日文输入法都被重置为英文，会从用户来源的窗口复制输入法的开启状态和转换模式（通过该窗口的输入法窗口用 `WM_IME_CONTROL` 读取）。
* **自定义“开始”菜单的单独 `Win` 键（开发中）：** 模块只截获 `Win` 的*松开*，且仅当按下与松开之间没有按过其他键时。为了不让 Windows 打开标准“开始”菜单，在松开 `Win` 之前会合成一个空的掩码键 `0xE8`（来自 AutoHotkey 的技巧：系统会认为这是一个组合键，从而保持沉默）。为了不让“开始”按钮的系统提示（`explorer.exe` 的 XAML 弹窗）盖住我们的菜单，模块会监视出现在按钮上方的资源管理器置顶窗口并临时隐藏它们。

### 音频、终端与取色器
* **找到真正发声的进程：** 现代浏览器由一个子音频进程输出声音。核心从 GSMTC 媒体会话中取得 `AppUserModelId`，找到具有该标识的窗口（或匹配 `.exe` 路径），取其 PID，并对**整个子进程树**启动 WASAPI process loopback，无需硬编码 `"spotify.exe"` 之类的列表。
* **跟踪 ConPTY 中的命令：** Windows Terminal 和 VS Code 通过 ConPTY 伪控制台运行，因此普通的控制台事件不会从那里发出。“岛”会在用户离开终端窗口时开始跟踪：在终端的进程树中找到 shell 及其子任务，并通过 `RegisterWaitForSingleObject` 等待它们退出。
* **不会点击穿透的实时取色器：** 在 Windows 中，完全透明的窗口（`alpha = 0`）会让鼠标点击穿过，因此取色器的屏幕捕获层以 `1/255` 的 Alpha 填充，而放大镜本身通过 `WDA_EXCLUDEFROMCAPTURE` + `CAPTUREBLT` 排除在捕获之外，避免放大自己。生成 OKLCH 色阶时，超出 sRGB 色域的彩度通过二分查找裁剪。

---

## 5. 源码地图

```text
src/
  main.rs        window, message loop, timers, focus, wiring of subsystems
  island/        drawing and behavior of the island
    mod.rs       layout, dropdowns, hit testing, notification popups
    kit.rs       brushes, theme, text (layout cache), Frame, marquee
    tone.rs      OKLab/OKLCH color math, APCA contrast, plates, vivid
    zone.rs      right notification zone: stack, 3 s queue, series, "+N" pile
    logo.rs      the Polynya mark, the Claude glyph, notification and app icons
    scroller.rs  a separate DComp surface for the hardware marquee
    segments/    capsule segments: player, time, weather
    panels/      dropdown screens: player, weather, notification, pile, launcher, modules
  launcher/      search: apps, files (Everything), calculator, currencies, fuzzy matching, recents
  look.rs        taskbar capture (BitBlt + edge median filter + noise)
  taskbar.rs     taskbar geometry, orientation and tracking
  wallpaper.rs   wallpaper change tracking and the Mica backdrop
  media.rs       GSMTC media sessions
  audio.rs       WASAPI process loopback and the 5-band spectrum
  weather.rs     Open-Meteo client, system geolocation and Nominatim reverse geocoding
  terminal.rs    tracking long commands in the terminal process tree
  locks.rs       Caps/Num Lock indication via Raw Input
  ipc.rs         named pipe server
  modules.rs     module manager: module.toml parsing, launching, stdin/stdout, packaged modules
  package.rs     installing and checking the Polynya.Notify sparse package
  settings.rs    built-in settings screens
  config.rs      reading and writing config.ini
  i18n.rs        interface localization tables (ru, en, zh, ja)
  install.rs     moving the binary and its modules to %LOCALAPPDATA% (not in a portable copy)
  autostart.rs   managing the autostart registry key
  gfx.rs         D3D11 / D2D1 / DComp setup, effects and shader prewarm
  shape.rs       geometry of the concave flow and the outline tails
  spring.rs      spring animation physics
  icons.rs       built-in vector icons and the SVG path parser for Direct2D
  cli.rs         command line parsing (push, dismiss, refresh, quit, uninstall, etc.)
proto/           shared protocol crate (MIT): message types, pipe client, atomic logger
modules/         sources of the built-in modules: agents, notify, picker, start
tools/           package build script (package.ps1) and logo generators (logo.py, ascii.py)
build.rs         VERSIONINFO generation from git describe and embedding resources into the exe
```
