Polynya 1.2.0 adds background release checks, a configurable launcher hotkey, and installation through Scoop and WinGet.

## What's new

* **Release checks in the background and on demand.** Polynya checks GitHub Releases after startup and then once a day. A newer stable version is announced once in the notification stack. In **Settings → About**, the update button shows the available version and opens its release page; you can also start a check manually. The checker announces releases and opens their pages; it does not replace application files automatically.
* **Choose the launcher hotkey by pressing it.** Open **Settings → Launcher hotkey**, click the current binding and press your preferred combination. **Esc** cancels recording; **Delete** disables the hotkey. `Alt+Space` remains the default, and the launcher can still be opened with the magnifier. The binding is also stored as `hotkey.launcher` in `config.ini`.
* **Scoop and WinGet distribution.** Stable release publication now generates package manifests from the tagged sources and the final release files. Everything is declared as a required dependency. Scoop preserves settings, logs and custom modules across updates and reinstalls, and switches the core and bundled modules together on rollback. WinGet catalog availability follows Microsoft's manifest validation.
* **English, Russian, Chinese and Japanese.** The new update states, hotkey recorder and error messages are localized in all four interface languages.

## Installation and updates

**WinGet:**

```powershell
winget install --id lyoraeth.Polynya --exact --source winget
```

Update an existing installation with `winget upgrade --id lyoraeth.Polynya --exact`.

**Scoop:**

```powershell
scoop bucket add extras
scoop bucket add polynya https://github.com/lyoraeth/polynya
scoop install polynya/polynya
```

Start and configure Everything for file search. To update Polynya, run `polynya quit`, then `scoop update polynya`, and launch Polynya again. The experimental notification identity package is kept during ordinary Scoop updates and removed on uninstall.

**Direct downloads:** choose the files for your architecture from the assets below.

| File | For whom |
| :--- | :--- |
| **`Polynya-1.2.0-x64-setup.exe`** | **Most people.** Installs for the current user, lets you choose modules and can install Everything for file search. Run a newer installer to update in place while keeping your settings. |
| **`Polynya-1.2.0-arm64-setup.exe`** | The same installer built natively for Windows on ARM. |
| **`…-portable.zip` + `…-modules.zip`** | Extract both archives for the same architecture into one folder and run `polynya.exe`. Settings and logs stay there. To update, close Polynya and extract the new archives over the existing files, keeping your `data` and `logs` folders. |

## Packaging fixes

* Silent installation no longer launches Polynya or its bundled Everything and notification-package installers. Package managers handle their dependencies.
* Silent uninstall keeps settings and logs without asking a question. Interactive uninstall still offers the choice to delete them.
* Scoop lifecycle failures stop the operation instead of reporting success with incomplete files or autostart state. User data and third-party module directories are preserved; conflicting paths are reported explicitly.

> **Requirements & SmartScreen:** Windows 11, build 22000 or newer, on x64 or ARM64. Builds are not code-signed yet; if Windows shows *"Windows protected your PC"*, choose **More info → Run anyway**. Everything and the optional experimental notification-package setup may request UAC. Run Polynya itself as a regular user.

**Documentation:** [User guide](https://github.com/lyoraeth/polynya/blob/main/docs/guide.md) · [Limitations and FAQ](https://github.com/lyoraeth/polynya/blob/main/docs/troubleshooting.md) · [Module API](https://github.com/lyoraeth/polynya/blob/main/docs/module-api.md) · [На русском](https://github.com/lyoraeth/polynya/blob/main/README.ru.md)

**Full changelog:** https://github.com/lyoraeth/polynya/compare/v1.1.1...v1.2.0
