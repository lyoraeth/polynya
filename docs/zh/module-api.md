# Polynya 模块 API

[English](../module-api.md) · [Русский](../ru/module-api.md) · **简体中文** · [日本語](../ja/module-api.md)

Polynya 模块是一个可以用任何语言编写的独立程序：Node.js 或 Python 脚本、`.exe`、PowerShell 脚本都可以。它放在自己的文件夹里，旁边是清单文件 `module.toml`，并通过 `stdin`/`stdout` 以 JSON 行与核心通信。

界面由核心绘制。模块只发送数据：由行组成的界面、通知卡片、要复制的文本。“岛”会按任务栏的风格渲染这些内容，并负责主题、动画、键盘导航和 IME 输入。模块不需要 UI 框架，也不需要知道任务栏的位置和颜色。即使模块崩溃，“岛”也会继续运行，并显示发生了什么。

本页是参考文档。从零开始的完整模块请看 **[示例](module-examples.md)**。

---

## 1. 文件夹与安装

```text
%LOCALAPPDATA%\Polynya\modules\<id>\
├── module.toml
└── main.js            （或 main.py、tool.exe 等）
```

便携版中，模块文件夹是 `polynya.exe` 旁边的 `modules\<id>\`，日志也在那里的 `logs\` 中。

* **安装：**把文件夹放进 `modules\`。下次打开搜索时核心就会识别它，无需重启。修改代码后要重启正在运行的模块进程，请执行 `polynya modules reload`。
* **启用与停用：**在“岛”的设置中的“模块”部分操作，立即生效。
* **删除：**删除该文件夹即可。
* **问题**（清单损坏、找不到解释器、崩溃）会以卡片形式显示在任务栏中，并写入日志 `%LOCALAPPDATA%\Polynya\logs\polynya.log`。

---

## 2. 清单：`module.toml`

```toml
api = 1                      # 协议版本；高于核心所支持的版本会被拒绝
id = "devtools"              # 唯一 id：拉丁字母、数字、- 和 _
exe = "main.js"              # 要运行的内容，见“模块如何启动”
args = ["--fast"]            # 可选：放在核心自身参数之前的参数
kind = "search"              # 可选：search | zone | other
name.en = "Devtools"         # 模块名称，按语言区分（回退到 en）
name.zh = "开发工具"

resident = false             # 从“岛”启动时起就保持进程运行
hooks = false                # 启用 / 停用时调用 `exe register` / `exe unregister`
taskbar = false              # 仅在任务栏水平放置时运行
experimental = false         # 默认关闭，直到用户启用

[main]                       # 可选：模块在搜索中的入口；打开其主界面
hint.zh = "Web 工具"
keywords = ["devtools", "px", "rem"]
icon = "m18 16 4-4-4-4 | m6 8-4 4 4 4 | m14.5 4-5 16"

[[functions]]                # 可选：主界面中的功能，可出现在搜索中并绑定快捷键
id = "convert"               # 模块在 `invoke` 中收到的值
title.en = "Convert value"   # 与模块菜单中该项的名称完全一致
title.zh = "转换值"
hint.zh = "单位、颜色"
keywords = ["convert", "px", "rem"]
icon = "M8 3 4 7l4 4 | M4 7h16 | m16 21 4-4-4-4 | M20 17H4"
hotkey = "Ctrl+Alt+D"        # 可选：全局快捷键
selection = true             # 可选：按快捷键时发送所选文本
search = true                # 可选：false 表示不在搜索中显示（仅快捷键和菜单）
```

**多语言字符串**（`name`、`title`、`hint`）可以是普通字符串，也可以是按语言区分的表：`ru`、`en`、`zh`、`ja`。缺少某种语言时回退到 `en`。

### 主界面及其功能

模块是一个整体，只有一个界面。搜索、快捷键和菜单只是进入它的不同入口，而不是彼此独立的功能：

* **`[main]`** 以模块的 `name` 把模块本身加入搜索。选中后运行 `main`，模块打开自己的主界面。没有界面的模块（只有钩子或卡片）不需要 `[main]`。
* **`[[functions]]`** 是主界面中值得直接访问的项。**每个功能都必须在模块界面中有对应项**，且 `title` 相同：搜索中显示为“开发工具 › 转换值”，即通往菜单中同一项的路径，模块名称由核心自动加上。功能不能是界面中没有的东西。核心无法检查这一点，因此这是 API 的规则。
* **在菜单中**，功能所在的行带有 `"command": "convert"`（见[界面](#5-界面)）。核心会在该行上显示用户设置的功能快捷键，选择该行时发送 `"source": "menu"` 的 `invoke`。无论从哪里调用，功能都以同样的方式到达，模块根据 `source` 决定从什么开始。

**搜索匹配由核心完成。**核心会同时用所有语言把搜索框中的文本与名称（功能还包括“模块名称 + 名称”）和 `keywords` 进行匹配，在用户选中之前不会启动模块进程。

**快捷键**（`[main]` 或功能上）写成修饰键加一个键的形式，不区分大小写：`Ctrl+Alt+D`、`Ctrl+Shift+F2`、`Win+Alt+Space`。至少需要 `Ctrl`、`Alt`、`Shift`、`Win` 中的一个。键可以是字母、数字、`F1`–`F24`、`Space`、`Enter`、`Tab`、`Insert`、`Home`、`End`、`PageUp`、`PageDown`，或 `` ` - = , . / ; `` 中的一个。被其他程序占用的快捷键会被跳过，并在日志中记一行。用户可以在 `config.ini` 中改写或关闭任意快捷键：

```ini
hotkey.devtools.convert = Ctrl+Shift+Q
hotkey.devtools.convert =
```

第一行指定 `Ctrl+Shift+Q`；第二行的值为空，表示关闭该快捷键。`[main]` 的键为 `hotkey.<模块>.main`。

**图标**是 24×24 网格上的 SVG 路径，采用 Lucide 风格，以单笔描边绘制。多个子路径用 `|` 分隔。支持的命令为 `M L H V C S Q T A Z`，大小写均可。

有错误的清单会被跳过，原因写入日志并显示为卡片，例如 `devtools: manifest: id = "a b": use only latin letters, digits, - and _`。没有 `title` 的功能同样算作错误。

---

## 3. 模块如何启动

核心以模块文件夹为工作目录运行 `exe [args] run`：

| `exe` | 实际运行的内容 |
| :--- | :--- |
| `main.js`、`.mjs`、`.cjs` | `node main.js run` |
| `main.py`、`.pyw` | `py -3 main.py run`（没有 `py` 启动器时使用 `python`） |
| `main.ps1` | `powershell -NoProfile -ExecutionPolicy Bypass -File main.ps1 run` |
| 文件夹中的文件 | 该文件 |
| 其他 | `PATH` 中的程序（`deno`、`bun` 等）；把脚本加到 `args` 中 |

**进程何时运行。**普通模块在第一次执行命令时启动，之后保持运行，因此后续命令会立即响应。`resident = true` 的模块随“岛”一起启动。当模块被停用、清单发生变化或“岛”退出时，进程会被停止。

**如何退出。**`stdin` 上的 EOF 表示核心已退出或停止了该模块，模块也应退出。3 秒后仍在运行的进程会被核心强制结束。

**环境变量：**

| 变量 | 值 |
| :--- | :--- |
| `POLYNYA_MODULE` | 模块 id |
| `POLYNYA_LANG` | 界面语言：`ru`、`en`、`zh`、`ja` |
| `POLYNYA_API` | 核心的协议版本 |
| `POLYNYA_VERSION` | Polynya 的版本 |

核心还会设置 `PYTHONUNBUFFERED`、`PYTHONIOENCODING=utf-8` 和 `PYTHONUTF8`，Python 模块无需自行设置。

**输出。**写入 `stderr` 的每一行都会进入 Polynya 日志。进程崩溃（退出码非零）时，用户会看到一张带有 `stderr` 最后一行的卡片。运行至少 10 秒后崩溃的 `resident` 模块会被重启。`stdout` 中不是有效消息的行会连同原因写入日志，读取继续进行。

**`hooks = true`：**启用模块时，核心调用一次 `exe register`，停用时调用 `exe unregister`。模块通过这两个调用把自己的钩子写入其他程序的配置（内置的 `agents` 模块就是这样工作的）。

---

## 4. 协议

每条消息都是**一行 JSON**，以 `\n` 结尾，使用 UTF-8 编码。`op` 字段表示消息类型，未知字段会被忽略。每写一行后都要刷新 `stdout`，否则消息会卡在缓冲区里。

### 来自核心：`stdin`

| 消息 | 何时到达 |
| :--- | :--- |
| `{"op":"invoke","command":"convert","source":"hotkey","query":"","selection":"font-size: 16px","clipboard":"#fff"}` | 运行 `main` 或某个功能。`source` 表示来源：`menu`（带 `command` 的行）、`search` 或 `hotkey`。`query` 是搜索框中的文本。`selection` 是当前程序中的所选内容，仅在用快捷键运行 `selection = true` 的功能时提供，没有选中内容时为空。`clipboard` 始终是剪贴板文本。从什么开始由模块决定。 |
| `{"op":"select","id":"r1"}` | 选择了 `item`、`toggle` 或 `button` 行（Enter 或点击）。带 `command` 的 `item` 会改为发送 `invoke`。 |
| `{"op":"submit","id":"query","text":"16px 24px"}` | 在 `input` 行中按下了 Enter。 |
| `{"op":"closed"}` | 模块的下拉面板已关闭：按了 Esc、点击了外部或打开了其他面板。 |

### 发给核心：`stdout`

| 消息 | 作用 |
| :--- | :--- |
| `{"op":"open","screen":{...}}` | 用该界面打开模块的下拉面板。在搜索中执行命令后，它会代替结果列表成为搜索的一页；其他情况下在搜索区块下方打开。带上 `"at":[left,right]`（屏幕像素）时，改为在任务栏的该区段下方打开。 |
| `{"op":"push","screen":{...}}` | 在已打开的下拉面板中进入下一个界面，并带有“返回”按钮。 |
| `{"op":"update","screen":{...}}` | 原地替换当前界面：选中项和正在输入的文本会按 `id` 留在原来的行上。适用于随输入变化的结果或不断跳动的数值。 |
| `{"op":"close"}` | 关闭下拉面板。如果该面板是在响应一个命令，焦点会回到发起命令的窗口，用户可以立即粘贴。 |
| `{"op":"copy","text":"1rem"}` | 把文本放入剪贴板。 |
| `{"op":"chip",...}`、`{"op":"dismiss",...}` | 通知卡片，见[卡片](#6-卡片)。 |

**核心自己处理的部分：**方向键导航、悬停、长行滚动、“返回”（不会通知模块，所以不同界面上的行 id 要保持唯一）、Esc、点击外部时关闭、被宽度截断的行在提示中显示全文，以及屏幕阅读器：名称、角色和状态都来自行本身，模块无需为此额外做任何事。

**从搜索或快捷键发送 `invoke` 之后**，核心最多等待一秒模块的回复，然后才关闭搜索。只完成工作（复制、显示卡片）的模块用 `close` 回复即可。从菜单调用时，功能通常用 `push` 回复：带“返回”的下一个界面。

---

## 5. 界面

```json
{"title": "流式尺寸", "rows": [
  {"type": "input", "id": "sizes", "text": "16px 24px", "placeholder": "两个尺寸"},
  {"type": "divider"},
  {"type": "item", "id": "r1", "text": "clamp(1rem, 0.8333rem + 0.7407vw, 1.5rem)", "hint": "clamp()"}
]}
```

`title` 是可选的。有标题或可以返回上一级的界面会有一个带“返回”按钮的标题栏。

| `type` | 字段 | 说明 |
| :--- | :--- | :--- |
| `item` | `id`、`text`、`hint?`、`command?`、`keys?`、`confirm?` | 可选择的行；`hint` 以较淡的颜色显示在右侧。`command` 让这一行成为模块的功能：核心在窄列中显示该功能的快捷键，并用 `"source": "menu"` 的 `invoke` 代替 `select`。`keys` 用于显示不属于模块的快捷键（例如 `Win+L`）。带有 `confirm`（例如 `"现在重启？"`）时，第一次按下会显示该问题和一条逐渐缩短的进度条，只有在 3 秒内再按一次才会生效。 |
| `toggle` | `id`、`text`、`on` | 带开关的行。按下时发送 `select`，模块用带有新 `on` 值的 `update` 回复。 |
| `input` | `id`、`text`、`placeholder?` | 文本输入框。按下后开始编辑，光标位于鼠标所在处；拖动可选择，双击选择一个词，长文本会滚动，支持剪贴板、`Ctrl+Z` 和 IME。Enter 发送 `submit`，Esc 取消。`text` 是当前值。 |
| `button` | `id`、`text`、`plain?` | 占满整行宽度的按钮。`plain: true` 时不绘制背景。 |
| `text` | `text` | 较淡的段落；`\n` 换行。 |
| `divider` | | 一条细线。 |
| `section` | `title` | 一组行上方的小标题；`title` 为空时作为间距。 |
| `header` | `title`、`subtitle` | 大标题，下方带说明文字。 |
| `pair` | `label`、`value` | 左边是标签，右边是值。 |
| `art` | `lines`、`colors?` | 居中的等宽字符画。`colors[行][字符]` 为每个字符指定 `#rrggbb`，空字符串表示使用文字颜色。 |

任何行中的长文本都会按宽度截断，选中的行会以滚动字幕显示全文。

---

## 6. 卡片

卡片是任务栏右侧区域中的通知。模块把它写入自己的 `stdout`。其他任何程序可以把它发送到命名管道 `\\.\pipe\Polynya-<SessionId>`（每次连接一条 JSON 消息，最大 64 KB），或者在命令行中使用 `polynya push`。

```json
{"op": "chip", "key": "build", "title": "构建完成", "body": "48 秒", "icon": "ok", "ttl_ms": 5000}
```

| 字段 | 默认值 | 说明 |
| :--- | :--- | :--- |
| `key` | *必填* | 卡片 id。相同 `key` 的卡片会原地替换旧卡片。 |
| `title` | *必填* | 卡片上显示的简短文本。 |
| `body` | `""` | 详细信息：悬停时滚动显示，展开卡片时显示全文。 |
| `meta` | `""` | 展开后卡片底部的小字：来源、时间。 |
| `icon` | `"info"` | `ok`、`fail`、`info`、`key`、`claude`、`agent`，或彩色圆点 `color:#rrggbb`。 |
| `ttl_ms` | `null` | 存活时间（毫秒），从卡片排到第一位时开始计算。`null` 表示一直保留，直到被关闭或替换。 |
| `animated` | `false` | 动画图标：表示工作正在进行。 |
| `source` | `key` | 系列：同一来源的卡片在队列中不会相互等待。 |
| `popup` | `false` | 卡片排到第一位时，以紧凑的下拉面板展开。 |
| `swatches` | `[]` | 展开卡片中显示的 `#rrggbb` 颜色；点击可复制。 |
| `mark` | `null` | `swatches` 中高亮颜色的索引。 |

`{"op": "dismiss", "key": "build"}` 用于移除卡片。队列与系列的规则见 **[通知](notifications.md)**。

---

## 7. Rust：`proto` crate

仓库中的 `proto` crate 为所有消息（`ModIn`、`ModOut`、`Screen`、`Row`、`Msg`）提供了 `serde` 类型，以及几个辅助函数：

* `proto::send(&msg)` 把消息发送到管道，管道繁忙时会短暂重试。如果“岛”没有运行，会立即返回错误。
* `proto::language()` 返回界面语言：依次查找 `POLYNYA_LANG`、`config.ini`、Windows 语言。
* `proto::info!`、`warn!`、`error!` 和 `debug!` 向共享日志写入一行。每行都通过一次系统调用写入，因此不同进程的行不会交错。

---

## 8. 许可

Polynya 核心采用 **GPLv3** 许可，而协议规范和 `proto` crate 采用 **[MIT](../../proto/LICENSE)** 许可。你可以用任何语言编写模块，并以任何许可发布，包括闭源和商业许可。唯一的限制是品牌：可以写“适用于 Polynya 的模块”，但不能把你的构建或插件说成是 Polynya 官方产品（见 **[TRADEMARKS.md](../../TRADEMARKS.md)**）。
