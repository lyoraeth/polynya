# ビルドと開発への参加

[English](../../CONTRIBUTING.md) · [Русский](../ru/contributing.md) · [简体中文](../zh/contributing.md) · **日本語**

Polynya の開発に関心を持っていただきありがとうございます。このファイルでは、環境の準備、コアとモジュールのビルド、テストの実行、変更の提案の方法を説明します。

---

## 1. ツールと依存関係

プロジェクトは外部の UI フレームワークを使わない純粋な Rust で書かれています（使っているのは `windows` 0.62、`serde` / `serde_json`、`toml` のクレートと、ビルド時の `embed-resource` だけです）。

ビルドに必要なもの：
* **Windows 11**（x64 または ARM64）
* **Rust**：バージョンは `rust-toolchain.toml` で固定されており、`rustup` が自動でインストールします。GNU ツールチェーンと **MinGW-w64**、MSVC のどちらでも構いません（通常のコンパイルに完全な Windows SDK は不要です）。リリースは MSVC でビルドしています
* **Inno Setup 6.5+**：インストーラーのビルドにだけ必要です（`winget install JRSoftware.InnoSetup`）

---

## 2. ビルド

プロジェクト全体は 1 つの Cargo ワークスペースです（コアの `polynya`、プロトコルのクレート `proto`、`modules/{agents,notify,picker,start}` にある 4 つの内蔵モジュール）。

リリース プロファイルですべてをまとめてビルドします：
```powershell
cargo build --release --workspace
```

ビルド後、`target/release/` に次のファイルができます：
* `polynya.exe`：アイランドのコアと CLI クライアント
* `polynya-agents.exe`、`polynya-notify.exe`、`polynya-picker.exe`：内蔵モジュールの実行ファイル
* `polynya-start.exe`：独自のスタート メニューのモジュール。Node.js の `modules/devtools` と同じくリポジトリ（`modules/start`）にありますが、どちらもまだリリースには含まれていません

*バージョン番号について：* `build.rs` は `git` からバージョンを自動的に算出し（`vX.Y.Z` タグ上では `X.Y.Z`、タグの間では `X.Y.Z+コミット数.ハッシュ`、最初のタグより前では `1.0.0-dev.N+ハッシュ`）、実行ファイルの `VERSIONINFO` に埋め込みます。

---

## 3. デバッグ用のローカル配置

実行時、`polynya.exe` は `%LOCALAPPDATA%\Polynya\` を自分の作業ディレクトリとして使います。新しいビルドをローカルで試すには：

1. 実行中のアイランドを `polynya quit` で停止します（常駐モジュールも一緒に終了します）。
2. ビルドした `polynya.exe` を `%LOCALAPPDATA%\Polynya\` に、モジュールの実行ファイル（`module.toml` と一緒に）を `%LOCALAPPDATA%\Polynya\modules\<id>\` にコピーします。
3. **重要：** `polynya.exe` は `explorer.exe` か、通常の（昇格していない！）ターミナルから起動してください。IDE やターミナルが管理者として動いていると、アイランドの子プロセスが昇格したトークンを引き継ぎ、UIPI のため Everything との通信が壊れます。

### デバッグ時のログ
コアとすべてのモジュールの共通ログは `%LOCALAPPDATA%\Polynya\logs\polynya.log` に書き込まれます。詳細な出力（カタログ走査の所要時間、バナーの判断、タスクバー取り込みのパラメーター）を有効にするには、`%APPDATA%\Polynya\config.ini` に次の行を追加します：
```ini
log = debug
```

---

## 4. テスト

プロジェクトには 85 個の自動テストがあり、通知のキューとシリーズのロジック、マニフェストの解析、`proto` のシリアライズ、APCA/OKLCH の色の計算、AI エージェントの設定のマージ、電卓のパーサー、CLI をカバーしています。

すべての単体テストを実行するには：
```powershell
cargo test --release --workspace
```

実際の Windows 環境との統合（GSMTC のメディア セッションのサムネイル、システムの位置情報など）を確認するテストは、既定では無視されています：
```powershell
cargo test --release --workspace -- --ignored
```

Pull Request を送る前に、`--workspace` のすべてのテストが通ることを確認してください。

### コード スタイル
* **フォーマット：** `cargo fmt --all`（設定は `rustfmt.toml`）。`rustfmt` はコメントを折り返さないので、コメントは手作業で 100 桁程度に収めてください。
* **コメント：** 英語で書き、コードの言い換えではなく*なぜ*そうしたのかを説明します。モジュールは `//!` で始め、公開アイテムは `///` で説明します。
* **`unsafe`：** すべてのブロックに、本当の根拠（ハンドルを誰が所有しているか、バッファーがいつまで生きているか）を書いた `// SAFETY:` コメントを付けます。ワークスペース全体で `clippy::undocumented_unsafe_blocks` lint が有効なので、`cargo clippy --workspace` で新しい警告が出ないようにしてください。
* **ファイルのヘッダー：** すべてのソース ファイルは `Copyright` と `SPDX-License-Identifier` の 2 行で始めます（`GPL-3.0-or-later`、`proto` クレートは `MIT`）。

---

## 5. 補助スクリプト（`tools/`）

通常の開発ではアセットを作り直す必要はありません（完成したファイルはリポジトリにあり、コンパイル時に埋め込まれます）。ただし、ロゴや通知パッケージのマニフェストを変更した場合は次を使います：
* **`tools/package.ps1`** は、`notify` モジュール用の署名なしスパース `.msix` パッケージを再ビルドします（軽量な `makeappx` を NuGet パッケージ `Microsoft.Windows.SDK.BuildTools` から自動でダウンロードします）。**重要：** パッケージのマニフェストを変更したら、マニフェスト内のバージョンと `src/package.rs` の `VERSION` 定数の両方を必ず上げてください。そうしないと、コアは起動時に更新に気付きません。
* **`tools/release.ps1`** は、1 つのアーキテクチャ向けのリリース ファイルを `target/dist` にビルドします。インストーラー（`installer/build.ps1` が Everything のインストーラーをダウンロードして SHA-256 を確認し、`installer/polynya.iss` をコンパイル）、ポータブル版のアーカイブ、そのためのモジュールのアーカイブです。`Release` ワークフローは `vX.Y.Z` タグで両アーキテクチャ向けに実行し、リリースの下書きを作ります。
* **`tools/logo.py` と `tools/ascii.py`** は、元の `assets/logo/mark.svg` から `.ico`、粒状感付きのラスターとベクターのロゴ、カラーの ASCII レイアウト `assets/logo/mark-ascii.json` を生成します。

---

## 6. ローカライズと文言のルール（`i18n`）

インターフェイスは 4 つの言語に対応しています：ロシア語（`ru`）、英語（`en`）、中国語（`zh`）、日本語（`ja`）。コアの文字列は `src/i18n.rs` に、モジュールの文字列はそれぞれのコードと `module.toml` にあります。文字列を追加するときは、プロジェクト共通のルールに従ってください：

1. **大文字・小文字：** 独立した見出しや設定項目の名前は大文字で始めます。「対象 + 状態」の形の複合的なステータスは、対象の後を小文字にします（例：`"Caps Lock on"`、`"#ff0000 copied"`）。
2. **三点リーダー：** 継続中の処理と入力のプレースホルダーには 1 文字の `…` を使います（`"思考中…"`、`"都市名を入力…"`）。対象を伴うエージェントの動作の文字列には付けません（`"Editing main.rs"`）。
3. **口調と用語：** あくまで中立的に、「あなた」や一人称を使いません。システムのコマンドには、各言語での Windows 自身の正確な表現を使います（*"Завершение работы"* / *"Shut down"* / *"关机"* / *"シャットダウン"*）。確認の文字列（`confirm`）は短い疑問文にします。

ドキュメントも同じ 4 つの言語で管理します。英語が主たる版（`README.md`、`docs/*.md`）で、翻訳は `README.<言語>.md` と `docs/<言語>/` にあります。法的な文書（`LICENSE`、`TRADEMARKS.md`）は英語版のみです。

---

## 7. コントリビューターのライセンス条件

このリポジトリに Pull Request を送ることで、あなたのコードが次の条件で配布されることに同意したものとみなされます：
* コアと内蔵モジュールの一部としては **[LICENSE](../../LICENSE)**（GNU GPLv3 + 第 7 条の追加条件）
* `proto` クレートとプロトコルのドキュメントの一部としては **[proto/LICENSE](../../proto/LICENSE)**（MIT License）
