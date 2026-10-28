# Security policy

[Русский](docs/ru/security.md) · [简体中文](docs/zh/security.md) · [日本語](docs/ja/security.md)

## Supported versions

Security fixes are released for the current minor branch of the `1.x` release.

| Version | Supported |
| :--- | :--- |
| `1.0.x` | Yes |

---

## Reporting a vulnerability

Please **do not report vulnerabilities in public GitHub Issues**.

If you found a security problem (for example, privilege escalation, command injection through hooks, or crossing the session boundary in IPC), contact the author directly:
* **Telegram:** [@lyoraeth_art](https://t.me/lyoraeth_art)
* **Discord:** `@lyoraeth`
* Or use the private **Report a vulnerability** form on the *Security* tab of this GitHub repository.

Include the steps to reproduce and the build version (`polynya help`).

---

## Polynya's security model (architecture notes)

To avoid false alarms during an audit, here are the project's isolation mechanisms and how it works with the system:

1. **No admin rights.** The main `polynya.exe` process and the built-in modules are designed to run with regular user rights only.
2. **IPC isolation between users.** The control pipe `\\.\pipe\Polynya-<SessionId>` is tied to the current user's Windows session id. With Fast User Switching, processes from another user's session never meet your pipe. A single pipe message is strictly limited to 64 KB.
3. **Why `polynya notify-setup` asks for UAC.** The only place elevation is requested is the one-time registration of the unsigned `Polynya.Notify` sparse package when you enable the experimental Windows notifications module yourself (a Windows 11 requirement for accessing `UserNotificationListener` without a paid certificate; the error without admin rights is `0x80073D2B`). After that the module process itself starts as a regular user through `IApplicationActivationManager`.
4. **Registry changes by the `notify` module.** To suppress duplicate toasts the module changes only the per-user `ShowBanner` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\<AUMID>` (no `HKLM` access), and creates a safety rollback entry in `HKCU\...\RunOnce\PolynyaBanners` in case the process ends abnormally.
