<div align="center">
  <img src="assets/logo/mark.svg" width="96" height="96" alt="Polynya Logo">
  <h1>Polynya</h1>
  <p><b>A lightweight launcher that lives in the Windows 11 taskbar</b></p>
  <p>
    Launcher · Time and weather · Player with visualizer · Notification stack · Modules in any language
  </p>
  <p>
    <b>English</b> · <a href="README.ru.md">Русский</a> · <a href="README.zh.md">简体中文</a> · <a href="README.ja.md">日本語</a>
  </p>
  <p>
    <a href="docs/guide.md">Documentation</a> ·
    <a href="docs/module-api.md">Module API</a> ·
    <a href="https://github.com/lyoraeth/polynya/releases">Download</a>
  </p>
</div>

---

<!-- TODO: main GIF/screenshot of the taskbar with an open dropdown -->
<!-- <p align="center"><img src="assets/preview.gif" width="100%" alt="Polynya Preview"></p> -->

**Polynya** is a fast launcher that lives right in the free space of the Windows 11 taskbar and turns it into a living info panel.

Classic launchers like **Raycast** or **PowerToys Run** work on demand only: press the hotkey, do the thing, close it, and the rest of the time they don't exist for you. Polynya has `Alt+Space` for launching programs, searching files, calculations and utilities, but it also has a permanent place on screen. So its output works by itself, with no extra keystrokes: time and weather, a player with a live equalizer and notifications from background commands and modules are always in sight, while the icons and the tray around them stay clickable as usual.

And unlike the many widgets and **Dynamic Island ports for Windows**, it is not a separate floating window with its own foreign style on top of the screen. Polynya blends into the system: it captures your taskbar's color, edge, outline and grain on the fly, follows the theme, accent and wallpaper, and its dropdowns flow smoothly out of the taskbar into the system Mica material.

It is written in native **Rust** with its own lightweight renderer: no browser engines (Electron / WebView) and no heavy UI frameworks inside. At rest it uses **0% CPU** and about **30 MB of memory**, and it falls fully asleep in fullscreen games and videos.

---

## What's inside

* **Search launcher (`Alt+Space` or a click on the magnifier).** The entry point for launching programs and built-in features (color picker, settings, module commands). Fuzzy search over classic and Store apps, instant file search via **Everything**, a calculator (trigonometry, tolerant comparisons like `0.1 + 0.2 == 0.3`, string operations) and a currency converter (`100 usd to eur`, `100$`). Its own input field with full IME support and mouse selection, a right-click action menu right in the results, module functions shown as "Module › Function" with their hotkeys in a narrow column, and the full text of a cut-off row in a tooltip.

* **Player and visualizer.** Works with any Windows media session (Spotify, browsers with YouTube, desktop players). The five-band spectrum listens to the audio of *the player process only* via WASAPI process loopback, so sounds from Telegram notifications or a game running in the background don't move it.

* **Time and weather.** A clock with rolling digits, sunrise and sunset on hover, an hourly and three-day forecast from Open-Meteo (no API keys). The city comes from Windows location or is entered by hand in the settings, applied on the fly.

* **A self-contained notification and event stack.** A shared queue with series and a collapsed "+N" pile shows events from modules and background processes right in the taskbar:
  * **Long terminal commands:** notifies when a background build or command in Windows Terminal / VS Code finishes (e.g. `cargo build · 48 s`) while you were in another window;

  * **Your own pushes from scripts and modules:** one `polynya push` command from the console, or the JSON pipe from any external module.

* **Accessible.** Every control has a name: Narrator, NVDA and JAWS read the island, search results and module screens through UI Automation, and new notifications are read out. Buttons that show only an icon and text cut off by the width get tooltips.

* **Silence in fullscreen.** In games, videos or presentations everything in the taskbar hides and sleeps: no frames are drawn and no audio is analyzed.

---

## Modules

Everything beyond the launcher, time and weather, player and notification stack is a module: a separate process that gets its interface, hotkeys and the selected text from the island. A few modules ship with Polynya, and your own are written the same way.

A module is a folder in `%LOCALAPPDATA%\Polynya\modules\<id>\` with a `module.toml` manifest.
* **No layout work:** a module just hands over data (JSON over the session's named pipe or `stdin/stdout`), and the island draws chips, lists, toggles, input fields and animations in the same style as the taskbar.
* **Any stack, any license:** write in Rust, Python, Go, C#, JS or anything else. The protocol specification and the `proto` crate are **MIT**-licensed, so you can publish your modules under any license, including closed and commercial ones.
* **One module, one interface:** `[main]` puts the module into search, and `[[functions]]` bring items of its menu into search and onto global hotkeys under the same names, like "Translator › Translate selection" on `Ctrl+Alt+T`. A function run by its hotkey can get the text selected in the active app.
* **Scripts as is:** `main.js`, `main.py` or `main.ps1` runs through the installed Node.js, Python or PowerShell, with no build step; errors show up as a chip and in the log.

The full protocol, manifest and screen row types are described in the **[Module API](docs/module-api.md)**, with complete modules in Node.js and Python in the **[examples](docs/module-examples.md)**.

### Bundled modules

* **AI agents (Claude Code, Codex, Cursor).** Shows when an agent is thinking, editing a file, waiting for input or done with a task. Hooks are registered automatically when the module is enabled, no manual config fiddling.
* **Color picker.** The launcher opens a live loupe without freezing the screen; it copies the hex value and expands the color into 11 **OKLCH** shades (like a Tailwind scale).
* **Windows notifications (experimental, off by default).** Replaces standard toasts with a compact dropdown from the taskbar (requires a one-time package install with UAC and a `ShowBanner` registry change, because Windows has no official API to turn off other apps' banners).

* **In development: custom Start, devtools and clipboard history.** Not shipped yet, coming in later releases. Custom Start opens a quick menu with system actions and an "About" screen on a lone `Win` press, with all the standard shortcuts working as usual. Devtools convert the selected value on a hotkey: `16px` to `1rem`, colors between HEX/RGB/HSL/OKLCH, a fluid `clamp()`, Unix time, JSON, JWT, Base64 and name casing. Clipboard history keeps what you copied and pastes it back from the island.

---

## Compared to alternatives

Launchers like Raycast and PowerToys work as "call it with a hotkey when needed". Polynya covers the launcher's job and also gives you a permanent info panel that is already on screen and looks like part of Windows itself.

| Feature | Polynya | Raycast for Windows | PowerToys Run / Command Palette |
| :--- | :--- | :--- | :--- |
| **What it is** | Launcher + living taskbar (always on screen) | Hotkey launcher | Hotkey launcher (CmdPal succeeds Run) |
| **Visible without invoking** | **Yes:** time, weather, player, notifications, agents | No | No |
| **Look** | Captures the taskbar's color, edge and grain + flows into Mica | Its own Raycast-style window | WinUI-style window |
| **Stack** | Native Rust, own Direct2D / DComp renderer | Native Raycast client | .NET / WinUI |
| **Resources at rest** | **0% CPU, ~30 MB RAM** | *not measured* | *not measured* |
| **App and file search** | Yes (files via Everything) | Yes (own search) | Yes |
| **Calculator and currencies** | Yes (functions, comparisons, strings, exchange rates) | Yes | Yes (currencies via plugins) |
| **Player and visualizer** | **Yes** (listens to the player process only) | No | No |
| **Notification stack and CLI (`push`)** | **Yes** (Windows, terminal, your scripts) | No | No |
| **AI agent status** | **Yes** (Claude Code, Codex, Cursor, no setup) | No | No |
| **OKLCH color picker** | **Yes** | No | No (the picker is a separate utility in the suite) |
| **Clipboard history, snippets, window management** | No | **Yes** | No (separate PowerToys utilities) |
| **AI chat** | No | **Yes** (paid) | No |
| **Modules (extensions)** | Any language (JSON over a pipe / stdio, scripts run as is), the island draws the UI; global hotkeys and the selection for module functions | JS/TS store (partly macOS-only) | C# / .NET extensions |
| **Price and source** | **Free, open source** (GPLv3 / MIT) | Free; Pro from $8/mo, closed source | **Free, open source** (MIT) |

> *Note: competitor features and prices are as of mid-2026 and may change; check their official sites for exact terms.*

---

## Performance

Test machine: Intel Core i5-10300H (4/8), integrated Intel UHD Graphics, 1920x1080, Windows 11 (26100). At rest the main thread just sleeps in `GetMessage`, and DWM itself scrolls long track titles through a composition animation.

| Scenario | Average CPU | Peak (1 s window) | Memory (active working set) |
| :--- | :--- | :--- | :--- |
| **Idle (clock, weather, chips on screen)** | **0.00%** | - | **26 MB** (core) / **~30 MB** (with all modules) |
| **Music playing (visualizer running)** | **0.52%** | - | ~30 MB |
| **Search open (caret blinking)** | **0.16%** | 0.58% | ~35 MB |
| **After opening every kind of dropdown** | **0.00%** (at rest) | 1.34 - 1.72% (during animations) | **~39 MB** (core + all resident modules) |

---

## Installation

**Requirements:** **Windows 11** only (x64 or ARM64). Windows 10 is intentionally unsupported: the whole point of the utility depends on the new taskbar and the Mica material.

1. Download the latest build for your architecture (`x64` or `arm64`) from **[Releases](https://github.com/lyoraeth/polynya/releases)**:
   * **Installer (`Polynya-<version>-<arch>-setup.exe`)**: the main option. It installs for the current user without admin rights, lets you pick the modules and, if Everything is not there yet, installs it for file search. Running a newer installer later updates Polynya in place and keeps your settings.
   * **Portable (`…-portable.zip`)**: unpack it anywhere and run `polynya.exe`; settings and logs stay in the same folder. Modules come separately in `…-modules.zip`: unpack it into the same folder. Everything is not included.
2. The island embeds itself into the taskbar, and a tray icon gives access to the settings.

To uninstall, use **Settings → Apps → Installed apps**: Polynya removes its autostart, agent hooks and notifications package, and asks whether to delete the settings and logs too.

> **"Windows protected your PC":** the builds are not code-signed yet, so SmartScreen may stop the first run of a new version. Click **More info → Run anyway**.

> **About admin rights:** run Polynya **as a regular user, without admin rights**. The island doesn't need elevation, and it even hurts: because of Windows privilege isolation (UIPI), an elevated process can't talk to the Everything search properly. UAC is requested only to install Everything and to enable the experimental Windows notification capture module.

---

## Command line (CLI)

You can push notifications to the stack or poke the island right from the terminal and your scripts:

```powershell
# Show a chip in the taskbar (stays 10 s by default)
polynya push "deploy done" "prod · 42 s" --icon ok --key deploy

# Update the chip with the same key (it goes first without waiting in the queue)
polynya push "tests failed" "error in auth" --icon fail --key deploy

# Remove a notification by key
polynya dismiss deploy

# Recapture the taskbar colors (useful after patches like Windhawk)
polynya refresh

# Restart module processes after changing a module's code
polynya modules reload
```

---

## Documentation

* **[User guide](docs/guide.md)**: how the island's blocks, search, player and weather work, every `config.ini` key and the CLI.
* **[Notifications and AI agents](docs/notifications.md)**: queue and series rules, automatic hooks for Claude Code / Codex / Cursor, and how Windows toast capture works.
* **[Known limitations and FAQ](docs/troubleshooting.md)**: edge cases (Windhawk, Spotify covers behind a VPN, Telegram Desktop and more).
* **[Module API](docs/module-api.md)** and **[examples](docs/module-examples.md)**: the `module.toml` spec, the protocol, hotkeys and the selection, and modules from scratch.
* **[Architecture](docs/architecture.md)**: why DirectComposition instead of a swap chain, how the taskbar color is captured, the OKLCH/APCA math and the tricky problems solved along the way.
* **[Building and contributing](CONTRIBUTING.md)**: toolchain, tests, rebuilding the notification package and localization rules.

---

## Contact

* **Questions and ideas:** [GitHub Discussions](https://github.com/lyoraeth/polynya/discussions)
* **Bugs:** [GitHub Issues](https://github.com/lyoraeth/polynya/issues)
* **Telegram:** [@lyoraeth_art](https://t.me/lyoraeth_art)
* **Discord:** `@lyoraeth`

---

## How it's made

Generative AI was used to build this project.
Polynya is made through [agentic engineering](https://simonwillison.net/2026/may/6/vibe-coding-and-agentic-engineering):
I write the code and documentation together with [Claude Code](https://claude.com/claude-code) (Anthropic).
The idea, product and interface decisions, the architecture choices and the visual design are mine;
Claude writes the implementation under my direction. To make those decisions at the level a product
like this needs, I worked through the Rust documentation, the Windows developer forums and the source
code of other projects in a similar niche myself, and rewrote by hand the parts of the code where
Claude's version didn't hold up.

Every change is tested live on a real Windows 11 taskbar before it gets into the project, and before
each release the tests and edge cases are checked locally. If you run into unexpected behavior, please
describe it in [issues](https://github.com/lyoraeth/polynya/issues) or in a private message (contacts above) and attach the log (`%LOCALAPPDATA%\Polynya\logs\polynya.log`).

I don't know everything, but I try to get better and I'm glad to learn. If you find an architectural
or code-level mistake, or a potentially dangerous spot in the code, please write in [issues](https://github.com/lyoraeth/polynya/issues)
or [discussions](https://github.com/lyoraeth/polynya/discussions): where the problem is, why it's bad and how to do it better.
I'll be really grateful.

---

## License and trademarks

* **The Polynya core and built-in modules** are released under **[GNU GPLv3](LICENSE)** with additional terms under Section 7 (keeping attribution in the code and the "About" window, a ban on passing off modified builds as an official release, and a mandatory rename for forks).
* **The interaction protocol and the `proto` crate** are licensed separately under **[MIT](proto/LICENSE)**, so third-party modules can be distributed on any terms.
* The **"Polynya"** name and logo belong to Danil Klimov (`lyoraeth`). What you can and cannot do with them is described in **[TRADEMARKS.md](TRADEMARKS.md)**.
