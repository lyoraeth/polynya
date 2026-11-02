Polynya 1.1.1 unlocks monitor-refresh-rate animations, moves scrolling text into dedicated DirectComposition layers, and fixes disabling the AI agents module.

## What's new

* **Animations at the monitor's refresh rate.** Interactive controls are no longer capped at 30 fps and now run at your display's native refresh rate: toggle switches, page transitions in menus and settings, selection highlights, list scrolling and action menus in the launcher, and the confirmation progress bar.
* **Scrolling text on dedicated DirectComposition layers.** Long track titles over glass backgrounds, the selected row in the launcher, weather details on hover, notification text, and long menu labels no longer redraw the main island on every frame. Text is rasterized into a bitmap once (with gradient masks cached by width and color) and scrolled smoothly on its own transparent layer at the monitor's refresh rate; layers are removed from the visual tree as soon as they close. On opaque taskbar backgrounds, the player still uses the zero-CPU DWM composition animation. Usage at rest stays at 0% CPU, while scrolling both a player track and a long launcher result simultaneously at 75 Hz takes ~0.6% CPU and ~37 MB of memory.
* **Unified AI agent indicator.** All supported agents (Claude Code, Codex, Cursor, and Kimi Code) now share the same 8 Hz terminal-glyph animation instead of spinning the logo continuously, dropping an extra frame loop and keeping status chips consistent across tools.
* **Fixed width for the "+N" notification pile.** Hovering over the collapsed notification counter no longer expands it or starts a marquee, preventing the island from accidentally overlapping adjacent taskbar icons. Clicking "+N" still opens the full notification stack, and hover behavior on regular notification chips is unchanged.

## Fixes

* **Disabling the AI agents module.** Turning the module off in settings now immediately dismisses active status chips from the taskbar and blocks new events—even if an already running terminal or IDE still holds the hooks in memory until it restarts.
* **Cleaned up search and module lists.** Removed leftover test entries for image verification that stayed in the launcher and settings after the API v2 rollout in `1.1.0`.

---

## Which file to download

| File | For whom |
| :--- | :--- |
| **`Polynya-1.1.1-x64-setup.exe`** | **Most people.** Installs without admin rights, lets you pick modules and optionally installs Everything for file search. Updates in place and keeps your settings. |
| **`Polynya-1.1.1-arm64-setup.exe`** | The same installer built natively for Windows on ARM (Snapdragon and others). |
| **`…-portable.zip` + `…-modules.zip`** | **Portable version:** unpack both archives into the same folder and run `polynya.exe`. Settings and logs stay in that folder; nothing is written to AppData or the registry. |

> **Requirements & SmartScreen:** Windows 11 only (Windows 10 is not supported). Builds are not code-signed yet, so if Windows shows *"Windows protected your PC"* on first run, click **More info → Run anyway**.

**Documentation:** [User guide](https://github.com/lyoraeth/polynya/blob/main/docs/guide.md) · [Limitations and FAQ](https://github.com/lyoraeth/polynya/blob/main/docs/troubleshooting.md) · [Module API](https://github.com/lyoraeth/polynya/blob/main/docs/module-api.md) · [Module examples](https://github.com/lyoraeth/polynya/blob/main/docs/module-examples.md) · [На русском](https://github.com/lyoraeth/polynya/blob/main/README.ru.md)
