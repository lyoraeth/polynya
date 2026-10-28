# 安全策略

[English](../../SECURITY.md) · [Русский](../ru/security.md) · **简体中文** · [日本語](../ja/security.md)

## 受支持的版本

安全修复针对当前的 `1.x` 次版本分支发布。

| 版本 | 支持 |
| :--- | :--- |
| `1.0.x` | 是 |

---

## 报告漏洞

请**不要在公开的 GitHub Issues 中报告漏洞**。

如果你发现了安全问题（例如权限提升、通过钩子注入命令，或 IPC 越出会话边界），请直接联系作者：
* **Telegram：** [@lyoraeth_art](https://t.me/lyoraeth_art)
* **Discord：** `@lyoraeth`
* 或使用本 GitHub 仓库 *Security* 选项卡中的私密表单 **Report a vulnerability**。

请附上复现步骤和构建版本（`polynya help`）。

---

## Polynya 的安全模型（架构说明）

为避免审计时的误报，下面说明项目的隔离机制以及它与系统交互的方式：

1. **无需管理员权限。** 主进程 `polynya.exe` 和内置模块被设计为只以普通用户权限运行。
2. **用户之间的 IPC 隔离。** 控制管道 `\\.\pipe\Polynya-<SessionId>` 绑定到当前用户的 Windows 会话 ID。使用快速用户切换时，其他用户会话中的进程不会碰到你的管道。单条管道消息的大小严格限制为 64 KB。
3. **为什么 `polynya notify-setup` 会请求 UAC。** 唯一请求提升权限的地方，是你手动启用实验性 Windows 系统通知模块时，对未签名稀疏软件包 `Polynya.Notify` 的一次性注册（这是 Windows 11 在没有付费证书的情况下访问 `UserNotificationListener` 的要求；没有管理员权限时的错误代码为 `0x80073D2B`）。此后模块进程本身通过 `IApplicationActivationManager` 以普通用户身份启动。
4. **`notify` 模块对注册表的修改。** 为了抑制重复的通知，模块只修改 `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\<AUMID>` 下的每用户 `ShowBanner` 值（不访问 `HKLM`），并在 `HKCU\...\RunOnce\PolynyaBanners` 中创建一个保险回滚项，以防进程异常终止。
