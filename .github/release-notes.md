The first public release.

Polynya is a lightweight launcher for Windows 11 that doesn't hide until you press a hotkey: it lives right in the taskbar. `Alt+Space` opens search for apps and files, a calculator and a currency converter. Time, weather, a player with a visualizer and notifications are always in sight, and the island follows the taskbar's color and edge, the theme, the accent and the wallpaper. Written in Rust: 0% CPU and about 30 MB of memory at rest.

Everything else is modules. A module is a separate process in any language: it hands over data, and the island draws its interface, puts its functions on global hotkeys and passes it the selected text. The API is open and documented.

### Which file to download

| File | For whom |
| :--- | :--- |
| `Polynya-1.0.0-x64-setup.exe` | Most people. An installer without admin rights, with a choice of modules; it can also install Everything for file search. |
| `Polynya-1.0.0-arm64-setup.exe` | The same for Windows on ARM (Snapdragon and others). |
| `…-portable.zip` + `…-modules.zip` | The portable version: unpack both archives into one folder and run `polynya.exe`. Nothing is installed into the system. |

Requires Windows 11. Windows 10 is not supported.

### Polynya

- Launcher: apps, files via Everything, calculator, currencies, module commands.
- Time and weather with an hourly and three-day forecast.
- Player with a visualizer for any Windows media session.
- Notification stack: from modules, from long terminal commands and from your own scripts via `polynya push`.
- Interface in English, Russian, Chinese and Japanese.

### Bundled modules

- **AI agents**: the status of Claude Code, Codex and Cursor in the taskbar; the hooks are set up automatically.
- **Color picker**: a live loupe and an OKLCH palette.
- **Windows notifications** (experimental, off by default): system notifications in the island instead of the standard banners. Installing it asks for UAC once.

### Modules in development

- Custom Start, devtools for web development, clipboard history.

### Known issues

- The builds are not code-signed yet. Windows may show "Windows protected your PC": click **More info → Run anyway**.
- With a vertical taskbar the island hides; with an auto-hiding taskbar it doesn't hide along with it yet.

Documentation: [user guide](https://github.com/lyoraeth/polynya/blob/main/docs/guide.md) · [limitations and FAQ](https://github.com/lyoraeth/polynya/blob/main/docs/troubleshooting.md) · [module API](https://github.com/lyoraeth/polynya/blob/main/docs/module-api.md) · [на русском](https://github.com/lyoraeth/polynya/blob/main/README.ru.md)
