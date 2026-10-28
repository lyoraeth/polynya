# Notifications, AI agents and the terminal

**English** · [Русский](ru/notifications.md) · [简体中文](zh/notifications.md) · [日本語](ja/notifications.md)

Polynya has one shared notification stack for all sources: built-in core indicators, AI agents, background terminal commands, external scripts (`polynya push`), third-party modules and Windows' own system notifications.

---

## 1. The queue, series and the "+N" pile

The island's right zone follows strict rules so that frequent status updates don't cover new notifications or make the screen flicker:

* **First place and minimum hold (3 seconds).** Any new notification (or an update of an existing one with the same `key`) tries to take first place, closest to the island's main block. Every notification that takes first place holds it for **at least 3 seconds**. If a notification from a *different* source arrives during that time, it waits its turn invisibly.
* **Smart series (`source`).** Notifications from the same source (`source`; for Windows toasts it is "app + title", i.e. a specific messenger conversation) don't wait for each other in the queue:
  * A new message from the same series takes first place at once, and the previous one moves into the stack immediately.
  * If a notification from this source was already waiting in the invisible queue, the new one takes its place in the queue, and the old one goes straight to the stack.
  * Frequent updates of the first chip (e.g. an AI agent quickly switching from "Reading a file" to "Editing") **do not extend** its 3-second hold if another source is already waiting.
* **Fair lifetime (`ttl_ms`).** The countdown until a chip disappears starts only when the notification actually reaches first place. While the mouse is over the chip or its dropdown is open, the timer is paused, and after the cursor leaves it restarts with the full duration.
* **Chip, expanded view and the "+N" pile:**
  * Normally a chip shows an icon and a short title. On hover the details scroll as a marquee (multi-line text is joined with ` · `).
  * A click on the chip opens a dropdown with the full text, source, time and a close button.
  * Chips that don't fit into the free width of the taskbar fold into a compact **"+N"** counter with icon circles. Hovering "+N" scrolls the titles of the hidden notifications, and a click opens the full list.

---

## 2. AI agents (Claude Code, Codex, Cursor)

The built-in `agents` module shows the status of local AI agents right in the taskbar. You see what the agent is doing while you switch to the browser or another window: *"Thinking…"*, *"Editing main.rs"*, *"Waiting…"*, *"Done"* or *"Error"*. While the agent works, the chip icon is animated (Claude's own glyph, or the Polynya mark turning in 60° steps).

### How it works without manual setup
When the `agents` module is enabled, the core calls `polynya-agents.exe register` once (and `unregister` when it is disabled). The module carefully adds its hooks to the agents' config files **without overwriting your existing settings and keeping the key order**:

1. **Claude Code (`~/.claude/settings.json`):** hooks for the `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Notification`, `Stop` and `SessionEnd` events. The binary path is written with forward slashes (`/`), because Claude Code runs hooks through a bash environment.
2. **Codex (`~/.codex/config.toml`):** a handler in the `notify` field.
3. **Cursor (`~/.cursor/hooks.json`):** hooks for `beforeSubmitPrompt`, `afterFileEdit`, `afterShellExecution`, `stop` and `sessionEnd`.

`polynya-agents.exe` itself does not stay in memory: the agent calls it on an event, the binary drops a JSON message into the island's pipe within a couple of milliseconds and exits (if the island is not running, it exits right away, always with a success code: a module error must not get in the agent's way).

---

## 3. Long terminal commands and Lock keys

* **Long terminal commands.** If you started a long build or script in **Windows Terminal** or the **VS Code** integrated terminal and switched to another window, the island shows a chip with the command name, run time and result (a check mark or a cross) when it finishes, e.g. `cargo build · 48 s`.
  * No plugins for PowerShell / bash / zsh are needed: tracking starts automatically when focus leaves the terminal window, through the ConPTY child process tree (`RegisterWaitForSingleObject`). A chain like `cargo build && cargo test` gives a single final notification.
  * If the command finished while you were looking at the terminal window itself, there is no notification.
  * The threshold is set in the menu (Off / 10 / 30 / 60 s) or with the `terminal_notify` key in `config.ini` (10 s by default).
* **Caps Lock and Num Lock.** Toggles are tracked through low-level Raw Input (without global keyboard hooks, which delay input system-wide) and show a short status chip.

---

## 4. Windows system notifications (experimental `notify` module)

The `notify` module captures all apps' system toasts through `UserNotificationListener`, adds them to the island's shared stack (up to 5 system notifications at once, a chip lives 10 s) and shows a new notification as a compact dropdown from the taskbar (for 5 s) instead of the standard bulky Windows banner.

The module is intentionally marked **experimental** and is off by default because of two fundamental limitations of Windows 11 itself:

### Limitation 1. Package identity (why UAC is needed at install)
In the Windows API, `UserNotificationListener` is available only to processes with **package identity** (MSIX).
* To avoid making users buy code-signing certificates or enable developer mode, Polynya uses an unsigned **sparse identity package `Polynya.Notify`** (~25 KB, embedded in the core) that points to the module binary in an external folder.
* On Windows 11, registering an unsigned package with an executable requires a one-time admin confirmation (without it the system returns error `0x80073D2B`).
* Pressing **"Install"** in the settings (or running `polynya notify-setup`) asks for UAC only to register the package; the island itself keeps running as a regular user. When the module is disabled, the package is removed automatically, without admin rights.

### Limitation 2. Suppressing duplicate Windows banners (`ShowBanner`)
The public WinRT API has no documented way to "hide another app's popup banner but keep the notification in the notification center" (`SuppressPopup` works only for your own toasts, and `RemoveNotification` removes the notification everywhere). Without doing anything, every message would pop up twice: first as the standard Windows window, and right away as an island dropdown.

How it is solved:
* The module sets `ShowBanner = 0` for specific apps under the user registry key `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\<AUMID>`. This is exactly the same "Show notification banners" toggle as in Windows Settings. The notification sound stays, and the message itself remains in the notification center.
* **Only apps that actually sent notifications are touched**, and their original values are backed up (`%APPDATA%\Polynya\banners.txt` and `banners-restore.reg`). If you had turned off banners for some app in Windows yourself, the module leaves it alone (it shows only a quiet chip without a dropdown). If you turn an app's banners back on in Windows Settings while the module runs, your choice is respected and the module lets that app go.
* **4 levels of protection and automatic rollback:**
  1. When the module closes normally or is disabled in the settings, all `ShowBanner` values are restored at once.
  2. The module watches the core process: if `polynya.exe` closes, the module rolls the registry back and exits.
  3. If the module itself crashes, the core performs the rollback.
  4. **In case of a sudden power loss or both processes being killed hard through Task Manager:** a `reg import` command for the backup is written to `HKCU\...\RunOnce\PolynyaBanners`, so at the next sign-in Windows itself restores all banners even without starting Polynya.
* If you don't want the module to change `ShowBanner`, turn off the **"Hide system banners"** toggle in the settings (`notify_banners = system` in `config.ini`): the standard Windows toasts stay in place, and the island just mirrors them as compact chips in the taskbar.
