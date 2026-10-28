# セキュリティ ポリシー

[English](../../SECURITY.md) · [Русский](../ru/security.md) · [简体中文](../zh/security.md) · **日本語**

## サポート対象のバージョン

セキュリティ修正は、`1.x` リリースの現在のマイナー ブランチに対して提供されます。

| バージョン | サポート |
| :--- | :--- |
| `1.0.x` | あり |

---

## 脆弱性の報告

脆弱性は**公開の GitHub Issues に投稿しないでください**。

セキュリティ上の問題（たとえば権限昇格、フック経由のコマンド インジェクション、IPC でのセッション境界の越境など）を見つけた場合は、作者に直接ご連絡ください。
* **Telegram：** [@lyoraeth_art](https://t.me/lyoraeth_art)
* **Discord：** `@lyoraeth`
* または、この GitHub リポジトリの *Security* タブにある非公開フォーム **Report a vulnerability** をご利用ください。

再現手順とビルドのバージョン（`polynya help`）を添えてください。

---

## Polynya のセキュリティ モデル（アーキテクチャの補足）

監査時の誤検知を避けるため、プロジェクトの分離の仕組みとシステムとの関わり方を説明します。

1. **管理者権限なしで動作。** メインの `polynya.exe` プロセスと内蔵モジュールは、通常ユーザーの権限だけで動くように設計されています。
2. **ユーザー間の IPC の分離。** 制御用の名前付きパイプ `\\.\pipe\Polynya-<SessionId>` は、現在のユーザーの Windows セッション ID に結び付いています。ユーザーの簡易切り替え（Fast User Switching）を使っても、他のユーザーのセッションのプロセスがあなたのパイプに触れることはありません。パイプのメッセージ 1 件のサイズは 64 KB に厳しく制限されています。
3. **`polynya notify-setup` が UAC を求める理由。** 権限の昇格を求めるのは、試験的な Windows システム通知モジュールを自分で有効にしたときの、署名なしスパース パッケージ `Polynya.Notify` の一度きりの登録だけです（有料の証明書なしで `UserNotificationListener` にアクセスするための Windows 11 の要件です。管理者権限がない場合のエラー コードは `0x80073D2B`）。その後、モジュールのプロセス自体は `IApplicationActivationManager` を通じて通常ユーザーとして起動します。
4. **`notify` モジュールによるレジストリの変更。** 重複するトーストを抑えるため、モジュールが変更するのは `HKCU\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\<AUMID>` にあるユーザーごとの `ShowBanner` の値だけで（`HKLM` にはアクセスしません）、プロセスが異常終了した場合に備えて `HKCU\...\RunOnce\PolynyaBanners` に保険のロールバック エントリを作成します。
