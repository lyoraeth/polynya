# Known limitations, edge cases and FAQ

**English** · [Русский](ru/troubleshooting.md) · [简体中文](zh/troubleshooting.md) · [日本語](ja/troubleshooting.md)

Answers to common questions, quirks of third-party software and technical limitations of Windows 11 itself that are worth knowing about.

---

## 1. Startup and system integration

### Windows says "Windows protected your PC" when I run the installer
The builds are not code-signed yet, and SmartScreen warns about any new unsigned file until it gathers enough downloads. Click **More info → Run anyway**. Each release's files are built from the tagged sources by GitHub Actions in this repository.

### Why shouldn't Polynya run as administrator?
The main app (`polynya.exe`) must run as a regular user. Windows has User Interface Privilege Isolation (**UIPI**): an elevated island loses normal communication with regular-level processes (in particular, the Everything file search and some user events stop working), and every program you open through the launcher inherits the admin token.

### I use Windhawk (or another taskbar tweaker), and the island's colors don't match the taskbar
The island recaptures the taskbar's texture and color only on system events (theme, wallpaper or accent change, `explorer.exe` restart), so that it never runs screen capture on a timer. Mods like Windhawk recolor the taskbar directly in memory without sending Windows notifications.
* **Fix:** right-click the Polynya tray icon and choose **"Refresh look"**, or run `polynya refresh` in a terminal.

### How does the island behave with a vertical or auto-hiding taskbar?
* **Bottom or top taskbar:** both are fully supported.
* **Vertical taskbar (at the side of the screen):** in the current version the island hides automatically, and modules tied to a horizontal taskbar don't start. Vertical layout support is planned.
* **Auto-hiding taskbar:** hiding the island together with the sliding taskbar is not implemented yet.
* **Multiple monitors:** the island lives on the primary taskbar's monitor.

### After connecting a display with another scale, the taskbar outline got thicker
Explorer can keep a thicker taskbar outline after a display with a different scale was connected (a TV, a projector, a remote screen) and disconnected, until Explorer restarts. The island measures the outline thickness on every capture and continues it under its dropdowns, so no seam appears; the thicker outline itself is Windows behaviour.
* **Fix:** restart Explorer (Task Manager → Windows Explorer → Restart) or sign out and back in. The island rebinds to the new taskbar by itself.

### Why is text antialiased in grayscale instead of ClearType?
The island renders through a transparent DirectComposition surface. In Direct2D / Windows, ClearType subpixel antialiasing is technically impossible on a transparent background with an alpha channel (it needs an opaque backdrop of a known color when the glyph is rasterized), so grayscale antialiasing is used, and for Chinese and Japanese pixel-grid snapping (`GDI_NATURAL`) is enabled as well.

---

## 2. Search, weather and player

### Search finds no files or folders (only programs)
For instant file search without heavy indexing of its own, Polynya uses the IPC of an installed **[Everything](https://www.voidtools.com/)** (`WM_COPYDATA`).
* If you installed Polynya with the installer and unchecked Everything (or downloaded the portable version), make sure Everything is installed and running in the background. Without it the launcher offers the standard Explorer search ("Search Windows").

### The weather doesn't show up
If Windows location services are off **and** no city is entered in the island's settings, the utility doesn't know which coordinates to query Open-Meteo for (the log gets a `location unavailable and no city set` entry).
* **Fix:** open "Settings" in the launcher -> "Weather", turn off "Detect from location" and type your city.

### With a VPN, Spotify updates the track in the player with up to 30 seconds of delay and no cover
This is how the Spotify client itself behaves on Windows: if its access to the cover CDN (`i.scdn.co`) is blocked or slow, Spotify doesn't publish updated track metadata to the Windows media session (GSMTC) until its internal timeout (~30 seconds). Polynya rereads covers with delays (after 0.4 / 1.2 / 2.5 s) for slow connections, but it can't speed up Spotify itself when the CDN is unreachable.

---

## 3. Notifications and messengers

### I enabled the experimental Windows notifications module, but Telegram Desktop messages don't appear
The official **Telegram Desktop** client draws its own custom notification windows by default, bypassing the Windows notification center.
* **Fix:** open *Telegram Settings -> Notifications and Sounds* and enable **"Use Windows notifications"** (some forks like AyuGram already do this out of the box).

### Why does a system notification dropdown have no avatar or reply buttons, and why does a click open the app rather than the specific chat?
These are limitations of the public `UserNotificationListener` API for third-party processes in Windows:
* The system gives a listener only the toast's text fields and the app id (`AUMID`). It doesn't provide attached images/avatars, interactive action buttons, or the internal launch arguments the app uses to jump to a specific conversation. So a click on the dropdown launches or brings forward the app itself through `shell:AppsFolder\<AUMID>`.

### Why did the very first notification from a brand-new program pop up once as a standard Windows banner?
Windows creates the app's `Notifications\Settings\<AUMID>` registry key only when that program shows its very first notification. Once the first notification arrives, the `notify` module remembers the app in `banners-known.txt` and turns off its duplicate banner in advance on every later start.

### I confirmed UAC with a different (admin) account while installing the notifications module, and the module doesn't start
The `Polynya.Notify` system package is registered in the profile of the Windows user whose credentials were entered in the UAC window. If you use a standard account and entered the password of another local administrator in UAC, the package is installed into that administrator's profile.

### Why doesn't the island show even important alarms and priority calls in Do Not Disturb (Focus Assist)?
When Do Not Disturb is on in Windows (`ToastNotificationManager.NotificationMode != Unrestricted`), and also during presentations or fullscreen games, the island stops popping up dropdowns completely. Windows itself can let through notifications from the user's list of priority apps in this mode, but that list isn't published through a public API, so the island stays quiet for everyone.
