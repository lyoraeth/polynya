# Polynya module API

**English** · [Русский](ru/module-api.md) · [简体中文](zh/module-api.md) · [日本語](ja/module-api.md)

A Polynya module is a separate program in any language: a Node.js or Python script, an `.exe`, a PowerShell script. It lives in its own folder next to a `module.toml` manifest and talks to the core in JSON lines over `stdin`/`stdout`.

The core draws the interface. A module sends data: a screen of rows, a notification chip, text to copy. The island renders it in the taskbar's style, with its theme, animations, keyboard navigation and IME input. A module needs no UI framework and knows nothing about the taskbar's position or colors. If a module crashes, the island keeps running and shows what happened.

This page is the reference. For working modules from scratch, see the **[examples](module-examples.md)**.

---

## 1. Folder and installation

```text
%LOCALAPPDATA%\Polynya\modules\<id>\
├── module.toml
└── main.js            (or main.py, tool.exe, ...)
```

In a portable copy the folder is `modules\<id>\` next to `polynya.exe`, and the log is in `logs\` there.

* **Install:** put the folder into `modules\`. The core picks it up the next time search opens, with no restart. To restart running module processes after changing the code, run `polynya modules reload`.
* **Turn on and off:** in the island's settings under "Modules". The change applies at once.
* **Remove:** delete the folder.
* **Problems** (a broken manifest, a missing interpreter, a crash) show up as a chip in the taskbar and in the log `%LOCALAPPDATA%\Polynya\logs\polynya.log`.

---

## 2. The manifest: `module.toml`

```toml
api = 1                      # protocol version; a newer one than the core knows is refused
id = "devtools"              # unique id: Latin letters, digits, - and _
exe = "main.js"              # what to run, see "How the module is started"
args = ["--fast"]            # optional: arguments placed before the core's own
kind = "search"              # optional: search | zone | other
name.en = "Devtools"         # the module's name, by language (falls back to en)
name.ru = "Девтулз"

resident = false             # keep the process running from the island's start
hooks = false                # call `exe register` / `exe unregister` on enable / disable
taskbar = false              # run only while the taskbar is horizontal
experimental = false         # off until the user enables it

[main]                       # optional: the module in search; opens its main interface
hint.en = "Web utilities"
keywords = ["devtools", "px", "rem"]
icon = "m18 16 4-4-4-4 | m6 8-4 4 4 4 | m14.5 4-5 16"

[[functions]]                # optional: functions of the main interface, in search and on hotkeys
id = "convert"               # what the module receives in `invoke`
title.en = "Convert value"   # exactly as the item is called in the module's menu
title.ru = "Преобразовать значение"
hint.en = "Units, colors"
keywords = ["convert", "px", "rem"]
icon = "M8 3 4 7l4 4 | M4 7h16 | m16 21 4-4-4-4 | M20 17H4"
hotkey = "Ctrl+Alt+D"        # optional: a global hotkey
selection = true             # optional: on the hotkey, send the selected text
search = true                # optional: false keeps it out of search (hotkey and menu only)
```

**Localized strings** (`name`, `title`, `hint`) are either a plain string or a table by language: `ru`, `en`, `zh`, `ja`. A missing language falls back to `en`.

### The main interface and its functions

A module is one thing with one interface. Search, hotkeys and the menu are different ways into it, never separate features:

* **`[main]`** puts the module itself into search, under its `name`. Choosing it runs `main`, and the module opens its main interface. A module without an interface (hooks, chips only) has no `[main]`.
* **`[[functions]]`** are items of that interface that are also worth reaching directly. **Every function must have its counterpart in the module's interface**, under the same `title`: search shows it as "Devtools › Convert value", the path to the same item in the menu, and the core adds the module name itself. A function cannot be something the interface does not have. The core cannot check this, so it is a rule of the API.
* **In the menu**, the row of a function carries `"command": "convert"` (see [Screens](#5-screens)). The core shows the function's hotkey on that row, as the user set it, and choosing the row sends `invoke` with `"source": "menu"`. A function arrives the same way from all three places, and the module decides what to start from by `source`.

**Search matching is done by the core.** It matches the search line against the title (for a function also "module name + title") and `keywords` in all languages at once, and the module process is not started until something is chosen.

**Hotkeys** (on `[main]` or a function) are written as modifiers plus a key, case-insensitive: `Ctrl+Alt+D`, `Ctrl+Shift+F2`, `Win+Alt+Space`. At least one of `Ctrl`, `Alt`, `Shift`, `Win` is required. A key is a letter, a digit, `F1`–`F24`, `Space`, `Enter`, `Tab`, `Insert`, `Home`, `End`, `PageUp`, `PageDown` or one of `` ` - = , . / ; ``. A hotkey taken by another app is skipped with a line in the log. The user can override or turn off any hotkey in `config.ini`:

```ini
hotkey.devtools.convert = Ctrl+Shift+Q
hotkey.devtools.convert =
```

The first line assigns `Ctrl+Shift+Q`; the second, with an empty value, turns the hotkey off. For `[main]` the key is `hotkey.<module>.main`.

**The icon** is an SVG path on a 24×24 grid, Lucide style, drawn as one stroke. Separate subpaths go with `|`. The supported commands are `M L H V C S Q T A Z`, in both cases.

A manifest with an error is skipped, and the reason goes to the log and a chip, e.g. `devtools: manifest: id = "a b": use only latin letters, digits, - and _`. A function without a `title` is an error too.

---

## 3. How the module is started

The core runs `exe [args] run` with the module folder as the working directory:

| `exe` | What runs |
| :--- | :--- |
| `main.js`, `.mjs`, `.cjs` | `node main.js run` |
| `main.py`, `.pyw` | `py -3 main.py run` (or `python` without the `py` launcher) |
| `main.ps1` | `powershell -NoProfile -ExecutionPolicy Bypass -File main.ps1 run` |
| a file in the folder | that file |
| anything else | a program from `PATH` (`deno`, `bun`, ...); add the script to `args` |

**When the process runs.** A regular module starts on the first command and stays alive, so later commands answer instantly. A `resident = true` module starts together with the island. The process is stopped when the module is turned off, its manifest changes, or the island exits.

**How to exit.** EOF on `stdin` means the core has exited or stopped the module, and it should exit too. The core kills a process that is still alive 3 seconds later.

**Environment variables:**

| Variable | Value |
| :--- | :--- |
| `POLYNYA_MODULE` | the module id |
| `POLYNYA_LANG` | the interface language: `ru`, `en`, `zh`, `ja` |
| `POLYNYA_API` | the protocol version of the core |
| `POLYNYA_VERSION` | the version of Polynya |

The core also sets `PYTHONUNBUFFERED`, `PYTHONIOENCODING=utf-8` and `PYTHONUTF8` so that a Python module does not have to.

**Output.** Every line written to `stderr` goes to the Polynya log. When the process crashes (a non-zero exit code), the user gets a chip with the last `stderr` line. A `resident` module that crashed after running for at least 10 seconds is restarted. Lines on `stdout` that are not valid messages go to the log with the reason, and reading continues.

**`hooks = true`:** when the module is turned on, the core calls `exe register` once, and `exe unregister` when it is turned off. The module uses these calls to add its hooks to other programs' configs (the built-in `agents` module works this way).

---

## 4. The protocol

Each message is **one line of JSON** ending with `\n`, encoded in UTF-8. The `op` field says what the message is, and unknown fields are ignored. Flush `stdout` after every line, or the message gets stuck in the buffer.

### From the core: `stdin`

| Message | When |
| :--- | :--- |
| `{"op":"invoke","command":"convert","source":"hotkey","query":"","selection":"font-size: 16px","clipboard":"#fff"}` | Run `main` or a function. `source` is where from: `menu` (a row with `command`), `search` or `hotkey`. `query` is the text in the search line. `selection` is the selection in the active app, only for a `selection = true` function run by its hotkey, and empty if nothing was selected. `clipboard` is the clipboard text, always. What to start from is the module's call. |
| `{"op":"select","id":"r1"}` | An `item`, `toggle` or `button` row was chosen (Enter or a click). An `item` with `command` sends `invoke` instead. |
| `{"op":"submit","id":"query","text":"16px 24px"}` | Enter in an `input` row. |
| `{"op":"closed"}` | The module's dropdown closed: Esc, a click outside or another panel opening. |

### To the core: `stdout`

| Message | What it does |
| :--- | :--- |
| `{"op":"open","screen":{...}}` | Opens the module's dropdown with this screen. After a command from search, it becomes a page of search in place of the result list. Otherwise it opens under the search block. With `"at":[left,right]` (screen pixels), it opens under that span of the taskbar instead. |
| `{"op":"push","screen":{...}}` | The next screen in the open dropdown, with a "back" button. |
| `{"op":"update","screen":{...}}` | Replaces the current screen in place: the selection and the text being typed stay on their rows by `id`. Use it for results that change while the user types, or a ticking value. |
| `{"op":"close"}` | Closes the dropdown. If the dropdown answered a command, focus returns to the window the command came from, so the user can paste right away. |
| `{"op":"copy","text":"1rem"}` | Puts text on the clipboard. |
| `{"op":"chip",...}`, `{"op":"dismiss",...}` | A notification chip, see [Chips](#6-chips). |

**What the core does by itself:** arrow navigation, hover, scrolling long rows, "back" (the module is not told about it, so make row ids unique across screens), Esc, closing on a click outside, tooltips with the whole text of rows cut off by the width, and screen readers: names, roles and states come from the rows themselves, so a module needs nothing extra for them.

**After an `invoke`** from search or a hotkey, the core waits up to a second for the module's answer before it closes search. A module that only does its work (copies, shows a chip) answers with `close`. From the menu, a function usually answers with `push`: the next screen with "back".

---

## 5. Screens

```json
{"title": "Fluid size", "rows": [
  {"type": "input", "id": "sizes", "text": "16px 24px", "placeholder": "Two sizes"},
  {"type": "divider"},
  {"type": "item", "id": "r1", "text": "clamp(1rem, 0.8333rem + 0.7407vw, 1.5rem)", "hint": "clamp()"}
]}
```

`title` is optional. A screen with a title, or one that has somewhere to go back to, gets a title bar with a "back" button.

| `type` | Fields | What it is |
| :--- | :--- | :--- |
| `item` | `id`, `text`, `hint?`, `command?`, `keys?`, `confirm?` | A selectable row; `hint` is muted on the right. `command` makes the row a function of the module: the core shows the function's hotkey in a narrow column and sends `invoke` with `"source": "menu"` instead of `select`. `keys` is a shortcut to show that is not the module's own (e.g. `Win+L`). With `confirm` (e.g. `"Restart now?"`), the first press shows that question with a shrinking bar, and only a second press within 3 seconds acts. |
| `toggle` | `id`, `text`, `on` | A row with a switch. A press sends `select`, and the module answers with `update` and the new `on`. |
| `input` | `id`, `text`, `placeholder?` | A text field. A press starts editing with the caret under the cursor; a drag selects, a double click selects a word, long text scrolls, and the clipboard, `Ctrl+Z` and IME work. Enter sends `submit`, Esc cancels. `text` is the current value. |
| `button` | `id`, `text`, `plain?` | A full-width button. `plain: true` draws it without a background. |
| `text` | `text` | A muted paragraph; `\n` breaks lines. |
| `divider` | | A thin line. |
| `section` | `title` | A small caption above a group of rows; an empty `title` is a gap. |
| `header` | `title`, `subtitle` | A large title with a caption below it. |
| `pair` | `label`, `value` | A label on the left and a value on the right. |
| `art` | `lines`, `colors?` | Monospace text art, centered. `colors[row][char]` is `#rrggbb` per character, and an empty string means the text color. |

Long text in any row is trimmed to fit, and the selected row scrolls it as a marquee.

---

## 6. Chips

A chip is a notification in the right zone of the taskbar. A module writes it to its `stdout`. Any other program sends it to the named pipe `\\.\pipe\Polynya-<SessionId>`, one JSON message of up to 64 KB per connection, or with `polynya push` from the command line.

```json
{"op": "chip", "key": "build", "title": "Build finished", "body": "48 s", "icon": "ok", "ttl_ms": 5000}
```

| Field | Default | Description |
| :--- | :--- | :--- |
| `key` | *required* | Chip id. A chip with the same `key` replaces the old one in place. |
| `title` | *required* | Short text on the chip. |
| `body` | `""` | Details: a marquee on hover and full text when the chip is expanded. |
| `meta` | `""` | Small caption at the bottom of the expanded chip: source, time. |
| `icon` | `"info"` | `ok`, `fail`, `info`, `key`, `claude`, `agent`, or a color circle `color:#rrggbb`. |
| `ttl_ms` | `null` | Lifetime in ms, counted from the moment the chip gets first place. `null` keeps the chip until it is closed or replaced. |
| `animated` | `false` | An animated icon: the work is in progress. |
| `source` | `key` | A series: chips from one source don't wait for each other in the queue. |
| `popup` | `false` | Expand the chip as a compact dropdown when it gets first place. |
| `swatches` | `[]` | `#rrggbb` colors drawn in the expanded chip; a click copies one. |
| `mark` | `null` | Index of the highlighted color in `swatches`. |

`{"op": "dismiss", "key": "build"}` removes a chip. The queue and series rules are described in **[Notifications](notifications.md)**.

---

## 7. For Rust: the `proto` crate

The `proto` crate in the repository has `serde` types for every message (`ModIn`, `ModOut`, `Screen`, `Row`, `Msg`) and a few helpers:

* `proto::send(&msg)` sends a message to the pipe and retries briefly while the pipe is busy. It returns an error right away if the island is not running.
* `proto::language()` returns the interface language: `POLYNYA_LANG`, then `config.ini`, then the Windows language.
* `proto::info!`, `warn!`, `error!` and `debug!` write a line to the shared log. Each line is written with a single system call, so lines from different processes never interleave.

---

## 8. Licensing

The Polynya core is licensed under **GPLv3**, while the protocol specification and the `proto` crate are under **[MIT](../proto/LICENSE)**. You can write modules in any language and publish them under any license, including closed and commercial ones. The only limit is the brand: "a module for Polynya" is fine, but presenting your build or plugin as an official Polynya product is not (see **[TRADEMARKS.md](../TRADEMARKS.md)**).
