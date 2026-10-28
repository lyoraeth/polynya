# User guide

**English** · [Русский](ru/guide.md) · [简体中文](zh/guide.md) · [日本語](ja/guide.md)

This document covers everyday use of Polynya: how the layout in the taskbar works, what the search launcher can do, how the player and weather are set up, how to install, update and remove it, where files live, and which keys exist in `config.ini` and on the command line.

---

## 1. How the island lives in the taskbar

* **Clickable tray and icons.** The utility's window spans the whole width of the primary monitor's taskbar, but it takes the mouse *only where something is drawn* (the island's blocks, notification chips, an open dropdown). Everything else is click-through, so the system tray and the icons of running programs work as usual.
* **Windows icon alignment (`TaskbarAl`).** If the Windows 11 taskbar icons are **centered**, the island automatically takes the free left part of the taskbar (up to the Start button) and grows to the right. If the icons are **left-aligned**, the island sits exactly in the center of the taskbar.
* **Bottom or top taskbar.** Both are supported. With a bottom taskbar the dropdowns grow upward out of it (a mirrored shape). With a vertical taskbar (at the side of the screen) the island hides.
* **Multiple monitors.** The island lives on the monitor of the primary taskbar and moves with it automatically if you change the main display or the resolution.
* **Fullscreen mode.** When a fullscreen game, video or presentation is open on the monitor, the island hides and fully stops drawing frames and analyzing audio. Calling search with `Alt+Space` still works on top of the fullscreen window.

### Block layout
The segment order depends on where the system icons sit in the taskbar:

* **Centered island** (when the Windows icons are left-aligned):
  1. **Player and visualizer** (grows to the left);
  2. **Time and weather** (stays exactly in the center, on a plate);
  3. **Search button** (a magnifier on a plate, grows to the right);
  4. **Right notification zone** (chips and the "+N" pile, grow to the right).
  On hover a segment widens to the right without shifting the central time block.
* **Left island** (when the Windows icons are centered in the taskbar):
  The island takes the free left part up to the Start button and grows from left to right:
  1. **Time and weather** (on a plate);
  2. **Search button** (on a plate);
  3. **Player and visualizer**;
  4. **Notification zone** (chips and the "+N" pile).

---

## 2. Time and weather

### Clock
* Shows the current time and date; when the minute changes, the changing digits roll smoothly.
* **Hover:** shows sunrise and sunset times.
* **Click:** opens the standard Windows notification center and calendar.

### Weather
* Data comes from **Open-Meteo** (free, no API keys).
* **City detection:** by default it uses Windows location (`Geolocator`) with reverse geocoding through **Nominatim** (OpenStreetMap); in this mode a navigation icon is shown next to the city name. Or you can turn off detection in the settings and type any city by hand (applied on the fly, no restart).
* **Hover:** shows the "Feels like" temperature, wind speed and humidity.
* **Click:** opens a dropdown with an hourly forecast for the next hours and a 3-day forecast.

---

## 3. Player and visualizer

* **Supported players:** any app that publishes a Windows media session (GSMTC): Spotify, browsers (YouTube, Yandex Music, SoundCloud, etc.), Telegram, classic players. If several players are open, the one that is playing right now wins.
* **Marquee:** long track titles scroll smoothly, driven by the DWM system compositor (with almost no CPU load).
* **Hover and click:** on hover, playback buttons (previous / pause / next), a progress bar and timing appear. A click opens a dropdown with a large cover over its blurred copy and seeking by clicking the bar.
* **Isolated visualizer:** 5 spectrum bands listen to the audio of *the specific player process* (including browser child processes) through WASAPI process loopback. Game sounds or messenger notifications don't affect the equalizer. The bands show dynamics rather than raw loudness (how far a band rises above its average background over the last 1.5 seconds).

---

## 4. Search launcher (`Alt+Space` or a click on the magnifier)

The launcher covers launching software, calling built-in commands, file search and quick calculations. The program catalog is prewarmed in the background 10 seconds after startup, so search opens instantly.

### What you can search and do
* **Apps and module commands.** Searches all classic and Store programs (`shell:AppsFolder`) with fuzzy input (e.g. `vsc` finds *Visual Studio Code*). Built-in commands ("Settings", "Color picker" and third-party module commands) are matched by title and keywords in all supported languages at once.
* **Recents.** On an empty query it shows recently launched programs and commands mixed together. While typing, frequent items get a small score bonus (on a tie a module command always ranks above a regular program).
* **File search (via Everything).** If Everything is installed and running, files and folders appear in the results once the query is a few characters long. Without Everything a "Search Windows" row is offered at the bottom.
* **Calculator.** Computes as you type:
  * Arithmetic, parentheses, constants (`pi`), roots (`√16`), trigonometry in degrees (`sin(30)`);
  * Comparisons with floating-point tolerance (`0.1 + 0.2 == 0.3` -> `true`);
  * String operations (`"ha" * 3` -> `hahaha`).
  * `Enter` copies the answer to the clipboard. The last calculation is kept for 10 minutes after the window closes, so you can reopen search and type `+ 15` right away.
* **Currency converter.** Understands natural queries and currency codes: `100$`, `100 usd to eur`, `10 долларов в рубли`. If no target currency is given (`100$`), it converts to the home currency of your Windows region. Rates come from `open.er-api.com`; the rate date is shown in the hint on the right.

### Context menu and input
* **Right-click action menu:** right-clicking a result row smoothly expands a submenu right inside the dropdown: *Open*, *Run as administrator*, *Show in Explorer*, *Copy path*, *Remove from recent*, *Uninstall* (a second right click collapses it).
* **A full input field:** mouse and keyboard selection, `Ctrl+A/C/X/V`, multi-level `Ctrl+Z` undo, word jumps and deletion (`Ctrl+Arrows`, `Ctrl+Backspace`), and Asian input methods (**IME**), keeping the input mode of the window you called search from.

---

## 5. Modules

Everything beyond the launcher, time and weather, player and notification stack is done by modules. They are turned on and off in the settings (section 6); writing your own is covered in the [Module API](module-api.md).

### AI agents (`agents` module)
Shows the status of Claude Code, Codex and Cursor in the notification stack: the agent is thinking, editing a file, waiting for input or done. The hooks are written into the agents' configs when the module is turned on and removed when it is turned off. Details: [Notifications, AI agents and the terminal](notifications.md).

### Color picker (`picker` module)
Called with the **"Color picker"** command from the launcher.
* Opens a live screen loupe without freezing the picture on the monitor (it can even pick colors from the island itself).
* A click copies the color's HEX code to the clipboard and shows a chip in the taskbar.
* Clicking the picker's chip expands a palette of **11 shades of that color in OKLCH space** (like a Tailwind scale, with your original color placed unchanged on the nearest step). Clicking any swatch copies its HEX.

### Windows notifications (`notify` module, experimental)
Off by default. Shows Windows system notifications in the island instead of the standard banners. Turning it on asks for UAC once, to register the module's package. Details and limitations: [Notifications, AI agents and the terminal](notifications.md).

### Devtools (`devtools` module)
> **In development:** not shipped in the current release yet; it will come in a later one.

Small tools for web development. The module runs on **Node.js**, which must be installed and in `PATH`.
* **Converting the selection (`Ctrl+Alt+D`).** Select a value in any app (or copy it) and press the hotkey. A list of conversions appears; Enter copies the chosen one, shows a chip and returns focus to the app, so `Ctrl+V` pastes it in place. Values inside a CSS line are converted in place: `padding: 16px 24px` becomes `padding: 1rem 1.5rem`.
  * Sizes: `px`, `rem`, `em`, `pt`; two sizes (`16px 24px`) also give a fluid `clamp()`.
  * Colors: HEX, RGB, HSL, OKLCH.
  * Unix time (seconds and milliseconds) and ISO 8601 dates.
  * JSON (formatted and minified), JWT (header and payload), Base64, URL encoding, name casing (`camelCase`, `snake_case`, `kebab-case` and more).
* **The *Devtools* command in search** opens the list of tools: the converter, fluid size with a preview at three screen widths, ID generation (UUID, nanoid, hex), the current time, hashes (SHA-256, SHA-1, MD5) and placeholder text.
* **Settings** (in the same list): the root font size for `rem`, the screen width range for `clamp()` and the number of decimal places.
* The hotkey can be changed or turned off in `config.ini`: `hotkey.devtools.convert = Ctrl+Shift+Q`.

### Custom Start (`start` module)
> **In development:** not shipped in the current release yet; it will come in a later one.

Replaces the bulky standard menu on a lone `Win` press, `Ctrl+Esc` or a left click on the Start button (horizontal taskbar only).
* All system shortcuts (`Win+E`, `Win+D`, `Win+Shift+S`, `Win+L`, etc.), Task Manager on `Ctrl+Shift+Esc` and the right-click power menu on the Start button (`Win+X`) work normally.
* Inside: *About*, *Settings*, *Task Manager*, *Sleep*, *Restart*, *Shut down*, *Lock*, *Sign out*.
* **Protection from accidental presses:** restart, shut down and sign out require a second click within 3 seconds to confirm (a question with a shrinking bar appears, no system dialogs).
* **"About" screen:** shows the colored ASCII Polynya mark, the Windows version, CPU model, RAM, graphics cards, free disk space and system uptime.

---

## 6. Settings, tray and refreshing the look

Open the settings with the **"Settings"** command in search or from the Polynya icon in the system tray.
* **Theme and weather city** apply instantly, on the fly.
* **Interface language, enabling/disabling modules and the notification banner mode** apply with the **"Apply"** button (the island restarts itself gently with the `--relaunch` flag). If you changed these and accidentally closed the window, a screen protecting the unsaved changes appears.
* **Refresh look (`polynya refresh`):** the island recaptures the taskbar colors by itself when the theme, wallpaper or accent changes or Explorer restarts. But if you use third-party tweakers like **Windhawk**, which recolor the taskbar bypassing Windows system events, click **"Refresh look"** in the tray (or run `polynya refresh`).

---

## 7. Config file (`config.ini`)

Settings are stored in `%APPDATA%\Polynya\config.ini`, in a portable copy in `data\config.ini` next to `polynya.exe` (created with comments on first launch). Format: `key = value`, comments start with `#`. You can open the file quickly with the **"Open config.ini"** button at the bottom of the settings.

| Key | Allowed values | Default | Description |
| :--- | :--- | :--- | :--- |
| `taskbar_height` | a number in DIPs (pixels at 100% scale) / empty | *from the system* | Manual override of the taskbar height (normally detected automatically). |
| `city` | a city name / empty | *by location* | City for the forecast (if empty, the Windows location is used). |
| `language` | `ru`, `en`, `zh`, `ja` / empty | *Windows language* | Language of the interface and built-in modules. |
| `terminal_notify` | seconds (`0` is off) | `10` | Time threshold for notifications about long terminal commands. |
| `modules_off` | space-separated `id`s | *empty* | Disabled regular modules (e.g. `agents picker`). |
| `modules_on` | space-separated `id`s | *empty* | Enabled experimental modules (e.g. `notify`). |
| `notify_banners` | `ours` / `system` | `ours` | Who draws popup banners while the `notify` module is on: `ours` (an island dropdown) or `system` (keep Windows banners, only chips in the island). |
| `theme` | `os`, `dark`, `light` | `os` | Controls only the color of text and icons (the background and plates always adapt to the actual taskbar color). |
| `font` | a family name / empty | `Inter` (embedded) | Custom interface font (if installed in the system; otherwise the embedded Inter, and if even that fails to load, Segoe UI Variable Text or Segoe UI). |
| `hotkey.<module>.<command>` | a hotkey like `Ctrl+Alt+D` / empty | *from the module* | Overrides a module command's hotkey; empty turns it off (e.g. `hotkey.translator.translate`). |
| `log` | `debug` / empty | *regular* | Enables verbose logging (including track titles, catalog walk timings and taskbar capture details). |

---

## 8. Command line reference (CLI)

All commands go through the main `polynya` binary:

```text
polynya
    Start the island in the taskbar.

polynya push <title> [details] [--key K] [--ttl SEC] [--icon ICON] [--meta TEXT]
    Send a notification (chip) to the island's stack.
    Options:
      --key K       Notification key. Calling again with the same key updates the existing chip.
      --ttl SEC     Lifetime in seconds (default 10). 0 means it stays until closed.
      --icon ICON   Chip icon: ok | fail | info | key | claude | agent | color:#rrggbb
      --meta TEXT   Small source/time caption at the bottom of the expanded dropdown.
    (Details from the CLI are cut to the first line and 160 characters.)

polynya dismiss <K>
    Remove the notification with key K from the stack.

polynya refresh
    Force a recapture of the taskbar colors, edge and noise.

polynya notify-setup
    Install the system package for the experimental Windows notifications module (asks for UAC)
    and start the module right away.

polynya modules reload
    Restart module processes, e.g. after changing a module's code.

polynya modules enable|disable <id>...
    Turn modules on or off; applies on the next start of the island.

polynya language ru|en|zh|ja
    Set the interface language; applies on the next start of the island.

polynya quit
    Close the island.

polynya uninstall [--purge]
    Close the island and undo what it registered in the system: autostart, AI agent hooks and
    the notifications package. --purge also deletes the settings and logs. The installer's
    uninstaller runs it; the files themselves are left in place.

polynya help
    Print the colored ASCII Polynya mark, the exact build version and the command reference.
```

---

## 9. Installing, updating and removing

* **Installer.** Installs for the current user into `%LOCALAPPDATA%\Polynya`, without admin rights. You pick the modules to install: AI agents and the color picker are ticked, Windows notifications are not (experimental). If Everything is not installed yet, the installer can install it for file search; that step asks for UAC. Autostart at sign-in is a checkbox too.
* **Updating.** Run the installer of a newer version: it closes the island, replaces the files and starts it again. Your settings, module switches and autostart stay as they are. Modules already installed are updated; the ones you did not install before can be added.
* **Removing.** Settings → Apps → Installed apps → Polynya. The uninstaller removes autostart, AI agent hooks and the notifications package and asks whether to delete the settings and logs too. Everything stays: it is a separate program.
* **Portable version.** `…-portable.zip` holds `polynya.exe`, a `portable` marker file and the license. Unpack it anywhere: the island runs right from that folder and keeps everything next to it (`data\`, `logs\`, `modules\`). Modules come separately in `…-modules.zip`: unpack it into the same folder. Autostart is off until you turn it on in the tray. Note that the AI agent hooks point to whichever island enabled them last, the portable or the installed one.
* **SmartScreen.** The builds are not code-signed yet, so Windows may show "Windows protected your PC" on the first run of a new version: click **More info → Run anyway**.

---

## 10. Where things live (files, logs and registry)

In a portable copy `%LOCALAPPDATA%\Polynya` and `%APPDATA%\Polynya` below are replaced by the folder with `polynya.exe`: `modules\` and `logs\` lie next to it, and the files from `%APPDATA%\Polynya` go to `data\`.

| Path / key | What is stored there |
| :--- | :--- |
| `%LOCALAPPDATA%\Polynya\polynya.exe` | The core executable. |
| `%LOCALAPPDATA%\Polynya\modules\<id>\` | Module folders (`module.toml` + the module's `.exe`). |
| `%LOCALAPPDATA%\Polynya\logs\polynya.log` | A single log for the core and modules (rotated at 1 MB into `polynya.old.log`, at most two files are kept). |
| `%APPDATA%\Polynya\config.ini` | User settings. |
| `%APPDATA%\Polynya\recent.txt` | Launcher launch history and frequency (`launches\ttime\tid`). |
| `%APPDATA%\Polynya\banners*.{txt,reg}` | A targeted backup of system banner settings for the `notify` module. |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` | The autostart entry at sign-in. |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce\PolynyaBanners` | A safety rollback of system banners via `reg import` if the process was killed abnormally. |
