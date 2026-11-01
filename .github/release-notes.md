Polynya 1.1.0 adds native Kimi Code support, image rendering in the module API, and bundled legal notices across all distribution packages.

### What's new

- **Kimi Code in AI agents.** The module now tracks Kimi Code alongside Claude Code, Codex, and Cursor: thinking process, tool calls, permission waits, task completion, errors, and interruptions. Hooks are registered automatically and cleaned up whenever the module is disabled.
- **Images in modules (API v2).** Modules can now attach raster images and SVG documents via absolute file paths or Base64 payloads. The island renders thumbnails and full-width visuals with preserved aspect ratios, including media inside expanded notifications. With `preview: true`, clicking an image opens the original file in the default Windows viewer. Existing `api = 1` modules remain fully supported.
- **Start Menu shortcut.** The installer now registers Polynya in the Start Menu and within `shell:AppsFolder`, making it indexable by standard Windows Search and discoverable within Polynya's own launcher.
- **Legal documentation in every package.** The installer presents the GPL license and bundles `LICENSE.txt` along with the generated `NOTICE.txt` into the application directory next to `polynya.exe`. Both files are also included directly in the portable archives.

### Which file to download

| File | Target audience |
| :--- | :--- |
| `Polynya-1.1.0-x64-setup.exe` | Recommended for most users. Standard installer running without elevated admin rights, featuring module selection and an optional Everything file search component. |
| `Polynya-1.1.0-arm64-setup.exe` | The same installer package built natively for Windows on ARM (Snapdragon and compatible hardware). |
| `…-portable.zip` + `…-modules.zip` | Portable version: extract both archives into the same folder and launch `polynya.exe`. Leaves no traces in the system registry or AppData. |

Requires Windows 11. Windows 10 is not supported.

### Bundled modules

- **AI agents**: Real-time status tracking for Claude Code, Codex, Cursor, and Kimi Code directly on the taskbar (hooks configure automatically).
- **Color picker**: Live screen loupe with integrated OKLCH color space palette.
- **Windows notifications** (experimental, disabled by default): System notifications rendered directly inside the island instead of standard toast banners. Requires an initial one-time UAC prompt during setup.

Custom Start, web development devtools, and clipboard history are actively in development and are scheduled for future updates.

### Known issues

- Binaries are not yet digitally signed with an EV/OV certificate. Windows SmartScreen may trigger a warning ("Windows protected your PC"): click **More info → Run anyway**.
- The island automatically hides on vertical taskbar layouts; auto-hiding taskbars are not yet dynamically synchronized with island visibility.

Documentation: [user guide](https://github.com/lyoraeth/polynya/blob/main/docs/guide.md) · [limitations and FAQ](https://github.com/lyoraeth/polynya/blob/main/docs/troubleshooting.md) · [module API](https://github.com/lyoraeth/polynya/blob/main/docs/module-api.md) · [module examples](https://github.com/lyoraeth/polynya/blob/main/docs/module-examples.md) · [на русском](https://github.com/lyoraeth/polynya/blob/main/README.ru.md)
