# Polynya architecture and internals

**English** · [Русский](ru/architecture.md) · [简体中文](zh/architecture.md) · [日本語](ja/architecture.md)

This document describes the project's architecture and implementation: how rendering works without frameworks, why the window uses a DirectComposition surface instead of a regular swap chain, how exactly the taskbar's color and texture are captured, and which non-trivial Windows problems had to be worked around.

---

## 1. Multi-process layout and threads

Polynya is split into an independent core and isolated module processes; a crashing module never takes the island down:

| Process | Lifetime | Link to the core |
| :--- | :--- | :--- |
| **`polynya.exe`** (core) | Always | — |
| **`polynya-start.exe`** (Start, in development) | Resident module (`resident`) | `stdin` / `stdout` (JSON lines) |
| **`polynya-notify.exe`** (Windows notifications) | Resident, activated through the sparse package | Named pipe (`proto::send`) |
| **`polynya-agents.exe`** (AI agent hooks) | Started on a hook event, exits right away | Named pipe |
| **`polynya-picker.exe`** (color picker) | Started by its search command, exits after the pick | `stdin` / `stdout`, chips over the named pipe |
| **`devtools`** (`main.js` on Node.js, in development) | Started by its first command or hotkey, keeps running | `stdin` / `stdout` (JSON lines) |
| **`polynya.exe push/...`** (CLI client) | One message, then exit | Named pipe |

### Threads inside the core (`polynya.exe`)
* **UI thread:** the window, the `GetMessage` loop, timers and all Direct2D / DirectComposition rendering.
* **`ipc`:** the named pipe server `\\.\pipe\Polynya-<SessionId>`.
* **`media`:** the system media session subscription (GSMTC) and rereading late covers.
* **`audio`:** capturing the player process's audio through WASAPI process loopback and computing the spectrum.
* **`apps` and `icons`:** the background `shell:AppsFolder` walk and shell icon extraction (`IShellItemImageFactory`).
* **Mouse hook:** started in a separate thread *only while a dropdown is open* (a global hook on the UI thread would add micro-lag to the cursor across the whole system).
* **`module-<id>`:** reading a module process's `stdout` and `stderr` and tracking its exit.
* **Weather and Everything IPC:** background requests to Open-Meteo / Nominatim and `WM_COPYDATA` exchange with Everything.

---

## 2. Window and rendering (Direct2D + DirectComposition)

* **A window without a redirection bitmap:** created with `WS_EX_NOREDIRECTIONBITMAP | TOPMOST | TOOLWINDOW | NOACTIVATE`. The taskbar itself (`Shell_TrayWnd`) is made the window's owner: in Win32 an owned window is always drawn above its owner, so the taskbar never covers the island when it raises itself (after an Explorer restart the binding is restored on `TaskbarCreated`).
* **Why a DirectComposition surface rather than a DXGI swap chain:** on Intel integrated graphics the very first swap chain of any size reserves ~50 MB of internal driver memory. Switching to a plain DirectComposition surface (`IDCompositionSurface`) brought the core's idle memory down from 67 MB to **17-26 MB**.
* **Rendering on the integrated GPU:** on systems with two GPUs the core deliberately picks the integrated adapter. The Windows system compositor (DWM) runs there and draws the desktop, so textures don't travel between GPUs, and the discrete GPU isn't woken for a UI utility and can stay in deep sleep.
* **Click-through and the window region (`SetWindowRgn`):** the window spans the whole taskbar width, so without limits it would intercept every click on the system tray and program icons. To let empty space pass the mouse through, the window gets a shaped mask (`SetWindowRgn`) exactly along the visible blocks. When a dropdown opens, the mask is expanded in advance (before the frame's `Commit`) and straight to the panel's final size, with room for the shadow and corners; otherwise during the spring animation the edges would hit a region that hasn't grown yet and get clipped. Mouse hit testing inside the window still uses the dropdown's exact concave outline.
* **Rendering only on change:** at rest the main thread fully sleeps in `GetMessage` (0.00% CPU). Expand animations run at the monitor's refresh rate (`Commit` + `DwmFlush`), while background animations (visualizer, marquees) are capped by a timer at exactly **30 fps** on any monitor (on 144 Hz, waiting on `DwmFlush` without a timer would give 72 extra fps).
* **Hardware marquee via DWM:** a long track title is drawn once onto a separate composition surface (`scroller.rs`), and the Windows system compositor does the scrolling through `IDCompositionAnimation` with nearest-neighbor interpolation (the letters move by whole pixels and don't blur). CPU load while scrolling dropped from 0.36% to **0.03%**. Since two surfaces of one device can't be drawn at the same time (`SURFACE_BEING_RENDERED`), the scroller's layers are drawn strictly after the main frame is closed and sent in one shared `Commit`.
* **Shader prewarm at startup:** the first use of Direct2D effects (`Turbulence`, `DisplacementMap`, `Shadow`) requires shader compilation (~290 ms), which would freeze the first dropdown open. So at startup the core runs these effects idly on a 16x16 buffer, and the first real open takes **2.1 ms**.

---

## 3. Adapting to the taskbar, and the color model

### How the taskbar look is captured (`look.rs`)
Capturing the taskbar window through Windows Graphics Capture (WGC) doesn't work: it lacks the DWM blur backdrop. So a `BitBlt` is taken of the screen strip where the taskbar lies. For the duration of the capture (23–46 ms) the island's own window is marked with `WDA_EXCLUDEFROMCAPTURE` so it doesn't photograph itself.

From the capture the core extracts:
1. Whether the taskbar theme is light or dark, and the system accent;
2. **The edge row** (pixel colors along the taskbar edge per column, with a 48 px median filter and 8 px smoothing, so icons or the Start button don't leave color blotches at the seam);
3. **The outline row** (a thin line along the taskbar edge, if the lightness difference is ≥ 0.02);
4. **The grain (noise)** of the taskbar texture.

There is no timer polling: a recapture is triggered only by events (coalesced with a 1 s debounce): system start (plus follow-up captures after 2 / 5 / 15 / 30 s, since Explorer draws the backdrop with a delay), a wallpaper change, a theme change (`ImmersiveColorSet`), an accent change (`WM_DWMCOLORIZATIONCOLORCHANGED`), high contrast, an Explorer restart (`TaskbarCreated`), unlock and resume from sleep.

### OKLCH color space and APCA contrast (`tone.rs`)
* **The color rule:** everything permanently visible on screen (time and search plates, buttons, enabled toggles) is painted in **the taskbar's own tone**, not the system accent (the accent often clashes with the wallpaper). The system accent is used only to highlight a row at the moment of user action (hover or arrow selection).
* **Plates (`tone::plate`):** take the taskbar's hue in **OKLCH** space, while the lightness is chosen to hit a target perceptual **APCA** contrast (`Lc 60` for primary text, `Lc 45` for dimmed text) at chroma `≤ 0.06` and 20–45% opacity.

### Dropdowns: the seam with the taskbar and the transition into Mica
* **Seam geometry (`shape.rs`):** a dropdown grows out of the taskbar through concave fillets snapped to whole physical screen pixels (at fractional coordinates the edges flickered during animation).
* **A seamless outline:** the taskbar's native 1-pixel outline under an open dropdown is erased and replaced by the dropdown's contour with straight horizontal "tails" of 2 DIPs on each side (otherwise a smoothed Direct2D arc can't be joined to a pixel line without a dark dot at the tip).
* **The transition into Mica:** right at the seam the dropdown's background equals the taskbar edge color, and over 56 DIPs it smoothly turns into **Mica** (the current desktop wallpaper, blurred once at 1/8 of the screen resolution and tinted with the taskbar color up to a factor of 0.65 based on the OKLab lightness difference). On top lies a vignette with a turbulent edge (`Turbulence` + `DisplacementMap`) and a 128x128 grain tile. Mica and the shading are merged into one layer, so two separate alpha masks don't produce a light fringe from double antialiasing.

---

## 4. Non-trivial engineering decisions

### Text and fonts
* **An embedded font with no RAM cost:** a subset of the Inter font (180 KB instead of 410 KB) is embedded right into the `.exe`. DirectWrite's standard `CreateInMemoryFontFileReference` copies the font buffer into the process's private heap; instead, a custom loader passes an owner object, so DirectWrite reads glyphs directly from the memory-mapped `.exe` image on disk (0 bytes in the private working set).
* **Glyph overhang and tabular figures:** text width accounts for overhang metrics (so the date doesn't run into slanted or bold digits), and monospaced digits are measured strictly after applying the OpenType `tnum` feature. When minutes roll, the whole string is laid out at once and clipped per character cell, so the digits don't double.

### Input, focus and key interception
* **Launcher and focus:** at rest the window has `WS_EX_NOACTIVATE` plus `MA_NOACTIVATE` on click, so a click on the player doesn't steal focus from the user's active window. When search or a menu with input opens, `WS_EX_NOACTIVATE` is temporarily removed, and on `Esc` focus returns to the window it came from.
* **A lone `Alt` in search:** in Win32, pressing `Alt` puts a window into system menu mode, and the next letters arrive as `WM_SYSCHAR` with an error beep. The launcher swallows `SC_KEYMENU`, and `WM_SYSCHAR` without `Alt` held is handled as regular input.
* **Inheriting the IME mode:** so that a Chinese/Japanese layout doesn't reset to Latin every time search opens, the IME open status and conversion mode are copied from the window the user came from (read through that window's IME window with `WM_IME_CONTROL`).
* **A lone `Win` for the custom Start (in development):** the module intercepts only the *release* of `Win`, and only if no other key was pressed between the press and the release. To keep Windows from opening the standard Start, a press of an empty mask key `0xE8` is synthesized before the `Win` release (a trick from AutoHotkey: the system sees a shortcut and stays silent). And so that the Start button's system tooltip (an `explorer.exe` XAML popup) doesn't cover our menu, the module watches for topmost Explorer windows appearing over the button and hides them temporarily.

### Audio, terminal and color picker
* **Finding the process that actually plays sound:** in modern browsers a child audio process outputs the sound. The core takes the `AppUserModelId` from the GSMTC media session, finds a window with that id (or matches the `.exe` path), takes its PID and starts WASAPI process loopback on **the whole child process tree**, with no hardcoded lists like `"spotify.exe"`.
* **Tracking commands in ConPTY:** Windows Terminal and VS Code run through the ConPTY pseudo-console, so the regular console events don't come from there. The island starts tracking at the moment the user leaves the terminal window: it finds the shells in the terminal's process tree and their child tasks and waits for them to exit through `RegisterWaitForSingleObject`.
* **A live color picker without clicking through:** in Windows a fully transparent window (`alpha = 0`) lets mouse clicks through, so the picker's screen catcher is filled with alpha `1/255`, and the loupe itself is excluded from capture with `WDA_EXCLUDEFROMCAPTURE` + `CAPTUREBLT` so it doesn't magnify itself. Chroma outside the sRGB gamut is clipped by a binary search when generating the OKLCH palette.

---

## 5. Source map

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
