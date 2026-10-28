# 通知、AI 代理与终端

[English](../notifications.md) · [Русский](../ru/notifications.md) · **简体中文** · [日本語](../ja/notifications.md)

Polynya 为所有来源提供一个共享的通知堆栈：核心的内置指示、AI 代理、后台终端命令、外部脚本（`polynya push`）、第三方模块，以及 Windows 自身的系统通知。

---

## 1. 队列、系列与 “+N” 堆叠

“岛”的右侧区域遵循严格的规则，避免频繁的状态更新遮住新通知或造成画面闪烁：

* **第一位与最短停留（3 秒）。** 任何新通知（或用相同 `key` 对已有通知的更新）都会尝试占据第一位，也就是最靠近“岛”主区块的位置。每条占据第一位的通知会保持**至少 3 秒**。如果在此期间来自*其他*来源的通知到达，它会以不可见的方式排队等候。
* **智能系列（`source`）。** 来自同一来源的通知（`source`；对 Windows 通知来说是“应用 + 标题”，即聊天软件中的某个具体会话）在队列中不会相互等待：
  * 同一系列的新消息会立即排到第一位，之前那条立刻移入堆栈。
  * 如果该来源的通知已经在不可见的等待队列中，新通知会取代它在队列中的位置，旧通知直接进入堆栈。
  * 如果已有其他来源在等待，第一位卡片的频繁更新（例如 AI 代理从“读取中”迅速切换到“编辑中”）**不会延长**其 3 秒停留时间。
* **公平的存活时间（`ttl_ms`）。** 卡片消失的倒计时只在通知真正到达第一位时才开始。鼠标悬停在卡片上或其下拉面板打开时，计时器暂停；光标离开后，计时器以完整时长重新开始。
* **卡片、展开视图与 “+N” 堆叠：**
  * 平时卡片显示图标和简短标题。悬停时详细内容以滚动字幕显示（多行文本用 ` · ` 连接）。
  * 点击卡片会打开下拉面板，显示完整内容、来源、时间和关闭按钮。
  * 放不进任务栏空闲宽度的卡片会折叠成一个带图标圆点的紧凑计数器 **“+N”**。悬停在 “+N” 上会滚动显示被隐藏通知的标题，点击则打开完整列表。

---

## 2. AI 代理（Claude Code、Codex、Cursor）

内置的 `agents` 模块把本地 AI 代理的工作状态直接显示在任务栏中。当你切换到浏览器或其他窗口时，也能看到代理在做什么：*“思考中…”*、*“编辑中 main.rs”*、*“等待中…”*、*“完成”* 或 *“错误”*。代理工作期间，卡片图标是动态的（Claude 的专属字形，或以 60° 为步进转动的 Polynya 标志）。

### 无需手动配置的原理
启用 `agents` 模块时，核心会调用一次 `polynya-agents.exe register`（禁用时调用 `unregister`）。模块会小心地把自己的钩子写入代理的配置文件，**不会覆盖你已有的设置，并保持键的顺序**：

1. **Claude Code（`~/.claude/settings.json`）：** 为 `UserPromptSubmit`、`PreToolUse`、`PostToolUse`、`Notification`、`Stop` 和 `SessionEnd` 事件注册钩子。程序路径使用正斜杠（`/`）写入，因为 Claude Code 通过 bash 环境调用钩子。
2. **Codex（`~/.codex/config.toml`）：** 在 `notify` 字段中注册处理程序。
3. **Cursor（`~/.cursor/hooks.json`）：** 为 `beforeSubmitPrompt`、`afterFileEdit`、`afterShellExecution`、`stop` 和 `sessionEnd` 注册钩子。

`polynya-agents.exe` 本身不会常驻内存：代理在事件发生时调用它，程序在几毫秒内把一条 JSON 消息发到“岛”的管道后立即退出（如果“岛”没有运行，它会马上退出，并且总是返回成功代码：模块的错误不应妨碍代理工作）。

---

## 3. 终端中的长时间命令与锁定键

* **终端中的长时间命令。** 如果你在 **Windows Terminal** 或 **VS Code** 集成终端中启动了耗时的构建或脚本，然后切换到其他窗口，命令结束时“岛”会显示一个卡片，包含命令名、运行时长和结果（对勾或叉号），例如 `cargo build · 48 秒`。
  * 无需为 PowerShell / bash / zsh 安装插件：焦点离开终端窗口时，会通过 ConPTY 子进程树自动开始跟踪（`RegisterWaitForSingleObject`）。像 `cargo build && cargo test` 这样的命令链只会产生一条最终通知。
  * 如果命令结束时你正看着终端窗口本身，则不会通知。
  * 阈值可以在菜单中设置（关 / 10 / 30 / 60 秒），或通过 `config.ini` 中的 `terminal_notify` 键设置（默认 10 秒）。
* **Caps Lock 与 Num Lock。** 通过底层的 Raw Input 跟踪切换（不使用会拖慢系统输入的全局键盘钩子），并显示一个简短的状态卡片。

---

## 4. Windows 系统通知（实验性 `notify` 模块）

`notify` 模块通过 `UserNotificationListener` 截获所有应用的系统通知，把它们加入“岛”的共享堆栈（最多同时 5 条系统通知，卡片存活 10 秒），并用从任务栏弹出的紧凑下拉面板（显示 5 秒）代替标准的笨重 Windows 横幅来展示新通知。

该模块有意标记为**实验性**并默认关闭，原因是 Windows 11 本身的两个根本限制：

### 限制 1：软件包标识（为什么安装时需要 UAC）
在 Windows API 中，只有拥有**软件包标识（Package Identity，MSIX）**的进程才能使用 `UserNotificationListener`。
* 为了不让用户购买代码签名证书或开启开发者模式，Polynya 使用一个未签名的 **稀疏软件包（sparse package）`Polynya.Notify`**（约 25 KB，内置于核心中），它指向外部文件夹中的模块程序。
* 在 Windows 11 中，注册带可执行文件的未签名软件包需要一次性的管理员确认（否则系统返回错误 `0x80073D2B`）。
* 在设置中点击 **“安装”**（或运行 `polynya notify-setup`）时，UAC 只用于注册软件包，“岛”本身继续以普通用户身份运行。禁用模块时，软件包会自动移除，无需管理员权限。

### 限制 2：抑制重复的 Windows 横幅（`ShowBanner`）
公开的 WinRT API 中没有文档化的方法可以“隐藏其他应用的弹出横幅，但把通知保留在通知中心”（`SuppressPopup` 只对自己的通知有效，`RemoveNotification` 会从所有地方删除通知）。如果什么都不做，每条消息都会弹出两次：先是标准的 Windows 窗口，紧接着是“岛”的下拉面板。

解决方式：
* 模块在用户注册表项 `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\<AUMID>` 中针对特定应用设置 `ShowBanner = 0`。这与 Windows 设置中的“显示通知横幅”开关完全相同。通知提示音保留，消息本身也仍留在通知中心。
* **只会修改确实发来过通知的应用**，其原始值会被备份（`%APPDATA%\Polynya\banners.txt` 和 `banners-restore.reg`）。如果你之前已在 Windows 中为某个应用关闭了横幅，模块不会动它（只显示一个安静的卡片，不弹出面板）。如果你在模块运行期间于 Windows 设置中重新为某个应用打开横幅，模块会尊重你的选择并放开该应用。
* **4 层保护与自动回滚：**
  1. 模块正常关闭或在设置中被禁用时，所有 `ShowBanner` 值会立即恢复原样。
  2. 模块会监视核心进程：如果 `polynya.exe` 关闭，模块会自行回滚注册表并退出。
  3. 如果模块本身崩溃，回滚由核心执行。
  4. **以防突然断电或两个进程都被任务管理器强行结束：** 会在 `HKCU\...\RunOnce\PolynyaBanners` 中写入一条对备份执行 `reg import` 的命令，下次登录时 Windows 本身就会恢复所有横幅，即使不启动 Polynya。
* 如果你不希望模块修改 `ShowBanner`，请在设置中关闭 **“隐藏系统横幅”** 开关（`config.ini` 中的 `notify_banners = system`）：标准的 Windows 通知会保持原样，“岛”只会在任务栏中以紧凑卡片的形式同步显示它们。
