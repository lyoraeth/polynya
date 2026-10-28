# Polynya のアーキテクチャと内部構造

[English](../architecture.md) · [Русский](../ru/architecture.md) · [简体中文](../zh/architecture.md) · **日本語**

このドキュメントでは、プロジェクトのアーキテクチャと実装について説明します。フレームワークなしでのレンダリングの仕組み、ウィンドウに通常のスワップ チェーンではなく DirectComposition サーフェスを選んだ理由、タスクバーの色と質感を具体的にどう取り込んでいるか、そして回避しなければならなかった Windows の厄介な問題です。

---

## 1. マルチプロセス構成とスレッド

Polynya は独立したコアと、分離されたモジュールのプロセスに分かれています。どのモジュールがクラッシュしてもアイランドは落ちません。

| プロセス | 生存期間 | コアとの通信 |
| :--- | :--- | :--- |
| **`polynya.exe`**（コア） | 常駐 | — |
| **`polynya-start.exe`**（スタート メニュー、開発中） | 常駐モジュール（`resident`） | `stdin` / `stdout`（JSON の行） |
| **`polynya-notify.exe`**（Windows の通知） | 常駐、スパース パッケージ経由で起動 | 名前付きパイプ（`proto::send`） |
| **`polynya-agents.exe`**（AI エージェントのフック） | フックのイベントで起動し、すぐに終了 | 名前付きパイプ |
| **`polynya-picker.exe`**（カラーピッカー） | 検索のコマンドで起動し、色を選ぶと終了 | `stdin` / `stdout`、チップは名前付きパイプ |
| **`devtools`**（Node.js 上の `main.js`、開発中） | 最初のコマンドかホットキーで起動し、そのまま動き続ける | `stdin` / `stdout`（JSON の行） |
| **`polynya.exe push/...`**（CLI クライアント） | メッセージを 1 つ送って終了 | 名前付きパイプ |

### コア（`polynya.exe`）内のスレッド
* **UI スレッド：** ウィンドウ、`GetMessage` のメッセージ ループ、タイマー、そして Direct2D / DirectComposition のレンダリングすべて。
* **`ipc`：** 名前付きパイプ `\\.\pipe\Polynya-<SessionId>` のサーバー。
* **`media`：** システムのメディア セッション（GSMTC）の購読と、遅れて届くカバー画像の読み直し。
* **`audio`：** WASAPI process loopback によるプレーヤー プロセスの音声の取り込みとスペクトラムの計算。
* **`apps` と `icons`：** `shell:AppsFolder` のバックグラウンド走査と、シェル アイコンの取得（`IShellItemImageFactory`）。
* **マウス フック：** *ドロップダウンが開いている間だけ*別スレッドで立ち上げます（UI スレッドにグローバル フックを置くと、システム全体のカーソルにわずかな遅延が生じるため）。
* **`module-<id>`：** モジュール プロセスの `stdout` と `stderr` の読み取りと、終了の監視。
* **天気と Everything IPC：** Open-Meteo / Nominatim へのバックグラウンド リクエストと、Everything との `WM_COPYDATA` によるやり取り。

---

## 2. ウィンドウとレンダリング（Direct2D + DirectComposition）

* **リダイレクト ビットマップを持たないウィンドウ：** `WS_EX_NOREDIRECTIONBITMAP | TOPMOST | TOOLWINDOW | NOACTIVATE` で作成します。ウィンドウのオーナーにはタスクバー自体（`Shell_TrayWnd`）を指定します。Win32 では所有されたウィンドウは常にオーナーの上に描画されるため、タスクバーが自ら前面に出てもアイランドを覆うことはありません（エクスプローラーの再起動後は `TaskbarCreated` で関連付けを復元します）。
* **DXGI のスワップ チェーンではなく DirectComposition サーフェスを使う理由：** Intel の内蔵グラフィックスでは、最初のスワップ チェーンはサイズにかかわらず約 50 MB のドライバー内部メモリを確保します。純粋な DirectComposition サーフェス（`IDCompositionSurface`）に切り替えたことで、待機時のコアのメモリは 67 MB から **17〜26 MB** に下がりました。
* **内蔵 GPU でのレンダリング：** GPU が 2 つあるシステムでは、コアは意図的に内蔵アダプターを選びます。Windows のシステム コンポジター（DWM）もそこで動いてデスクトップを描いているため、テクスチャが GPU 間を行き来せず、UI ツールのために独立 GPU が起こされることもなく、深いスリープを保てます。
* **クリックの透過とウィンドウ リージョン（`SetWindowRgn`）：** ウィンドウはタスクバーの全幅に広がっているため、何もしなければシステム トレイやプログラムのアイコンへのクリックをすべて奪ってしまいます。空いている場所でマウスを通すために、見えているブロックの輪郭どおりの形のマスク（`SetWindowRgn`）をウィンドウに設定しています。ドロップダウンを開くときは、このマスクを前もって（そのフレームの `Commit` より前に）、影と角の丸みの余白込みでパネルの最終サイズまで一気に広げます。そうしないと、スプリング アニメーションの間に画像の端がまだ広がっていないリージョンにぶつかって切れてしまいます。ウィンドウ内のマウスのヒット テストは、ドロップダウンの正確なくぼんだ輪郭で計算します。
* **変化があるときだけ描画：** 待機中のメイン スレッドは `GetMessage` で完全に眠っています（CPU 0.00%）。展開アニメーションはモニターのリフレッシュ レートで動き（`Commit` + `DwmFlush`）、背景のアニメーション（ビジュアライザー、スクロール表示）はどのモニターでもタイマーでちょうど **30 fps** に制限しています（144 Hz では、タイマーなしで `DwmFlush` を待つと余分に 72 fps 出てしまいます）。
* **DWM によるハードウェア スクロール表示：** 長い曲名は別のコンポジション サーフェス（`scroller.rs`）に一度だけ描画し、スクロール自体は Windows のシステム コンポジターが最近傍補間の `IDCompositionAnimation` で行います（文字は整数ピクセル単位で動き、ぼやけません）。スクロール中の CPU 負荷は 0.36% から **0.03%** に下がりました。同じデバイスの 2 つのサーフェスは同時に描画できない（`SURFACE_BEING_RENDERED`）ため、スクロール表示のレイヤーはメインのフレームを閉じた後に描き、1 回の共通の `Commit` で送ります。
* **起動時のシェーダーの事前ウォームアップ：** Direct2D のエフェクト（`Turbulence`、`DisplacementMap`、`Shadow`）を初めて使うときはシェーダーのコンパイル（約 290 ms）が必要で、最初にドロップダウンを開くときに固まってしまいます。そこでコアは起動時に 16x16 のバッファーでこれらのエフェクトを空回しし、実際に初めて開くときは **2.1 ms** で済むようにしています。

---

## 3. タスクバーへの適応と色のモデル

### タスクバーの見た目の取り込み方（`look.rs`）
Windows Graphics Capture（WGC）でタスクバーのウィンドウをキャプチャする方法は使えません。DWM のぼかした下地が含まれないためです。そこで、タスクバーがある画面の帯を `BitBlt` で取り込みます。取り込みの間（23〜46 ms）は、アイランド自身のウィンドウに `WDA_EXCLUDEFROMCAPTURE` を付け、自分を写さないようにします。

取り込んだ画像から次の情報を取り出します。
1. タスクバーのテーマが明るいか暗いか、そしてシステムのアクセント カラー
2. **縁の行**（タスクバーの縁のピクセルの色を列ごとに、48 px のメディアン フィルターと 8 px の平滑化をかけて取得します。アイコンやスタート ボタンが継ぎ目に色むらを作らないようにするため）
3. **輪郭線の行**（タスクバーの縁に沿った細い線。明度差が 0.02 以上の場合）
4. タスクバーの質感の**粒状感（ノイズ）**

タイマーによるポーリングは一切ありません。取り込み直しはイベントでだけ起こります（1 秒のデバウンスでまとめます）：システムの起動（エクスプローラーが下地を遅れて描くため、2 / 5 / 15 / 30 秒後の追加の取り込みも）、壁紙の変更、テーマの変更（`ImmersiveColorSet`）、アクセント カラーの変更（`WM_DWMCOLORIZATIONCOLORCHANGED`）、ハイ コントラスト、エクスプローラーの再起動（`TaskbarCreated`）、ロック解除、スリープからの復帰。

### OKLCH 色空間と APCA コントラスト（`tone.rs`）
* **色のルール：** 画面に常に見えているもの（時刻と検索のプレート、ボタン、オンのトグル）はすべて、システムのアクセント カラーではなく**タスクバー自身の色合い**で塗ります（アクセント カラーは壁紙とぶつかることが多いため）。システムのアクセント カラーは、ユーザーが操作する瞬間（ホバーや矢印キーでの選択）に行を強調するときだけ使います。
* **プレート（`tone::plate`）：** **OKLCH** 空間でタスクバーの色相を取り、明度は目標とする知覚コントラスト **APCA**（主要な文字は `Lc 60`、控えめな文字は `Lc 45`）に合わせて選びます。彩度は `0.06 以下`、不透明度は 20〜45% です。

### ドロップダウン：タスクバーとの継ぎ目と Mica への移行
* **継ぎ目の形状（`shape.rs`）：** ドロップダウンは、物理ピクセルの整数座標にそろえたくぼんだ角の丸みを通ってタスクバーから伸びます（小数座標ではアニメーション中に縁がちらつきました）。
* **継ぎ目のない輪郭線：** 開いたドロップダウンの下にあるタスクバーのネイティブな 1 ピクセルの輪郭線は消し、両側に 2 DIP の水平な「しっぽ」を持つドロップダウンの輪郭に置き換えます（そうしないと、アンチエイリアスされた Direct2D の円弧をピクセルの線と先端に暗い点を残さずにつなげられません）。
* **Mica への移行：** 継ぎ目のすぐそばではドロップダウンの背景はタスクバーの縁の色と同じで、56 DIP かけて滑らかに **Mica**（現在のデスクトップの壁紙を画面の 1/8 の解像度で一度だけぼかし、OKLab の明度差に応じて最大 0.65 の係数でタスクバーの色に染めたもの）へ移ります。その上に、乱流状の縁を持つビネット（`Turbulence` + `DisplacementMap`）と 128x128 の粒状感のタイルを重ねます。Mica と陰影は 1 つのレイヤーにまとめており、2 つの別々のアルファ マスクが二重のアンチエイリアスで明るい縁取りを生まないようにしています。

---

## 4. 一筋縄ではいかなかった設計判断

### 文字とフォント
* **RAM を使わない内蔵フォント：** Inter フォントのサブセット（410 KB ではなく 180 KB）を `.exe` に直接埋め込んでいます。DirectWrite 標準の `CreateInMemoryFontFileReference` はフォントのバッファーをプロセスのプライベート ヒープにコピーしますが、代わりにオーナー オブジェクトを渡す独自のローダーを実装し、DirectWrite がディスク上のメモリ マップされた `.exe` イメージから直接グリフを読むようにしています（プライベート ワーキング セットは 0 バイト）。
* **グリフのはみ出しと等幅数字：** 文字の幅はオーバーハングのメトリクスを考慮して計算し（日付が斜体や太字の数字に重ならないように）、等幅数字は OpenType の `tnum` 機能を適用した後に厳密に測り直します。分が変わってスクロールするときは文字列全体をまとめてレイアウトし、文字のセルごとに切り抜くので、数字が二重に見えません。

### 入力、フォーカス、キーの乗っ取り
* **ランチャーとフォーカス：** 普段のウィンドウには `WS_EX_NOACTIVATE` を付け、クリックには `MA_NOACTIVATE` を返します。そのため、プレーヤーをクリックしてもユーザーのアクティブなウィンドウからフォーカスが奪われません。検索や入力のあるメニューを開くと `WS_EX_NOACTIVATE` を一時的に外し、`Esc` で元のウィンドウにフォーカスを戻します。
* **検索中の `Alt` 単独押し：** Win32 では `Alt` を押すとウィンドウがシステム メニュー モードに入り、続く文字がエラー音とともに `WM_SYSCHAR` として届きます。ランチャーでは `SC_KEYMENU` を握りつぶし、`Alt` を押していない状態の `WM_SYSCHAR` は通常の入力として扱います。
* **IME の状態の引き継ぎ：** 検索を開くたびに中国語や日本語の入力方式が英字に戻ってしまわないよう、ユーザーが来た元のウィンドウから IME のオン/オフ状態と変換モードをコピーします（そのウィンドウの IME ウィンドウから `WM_IME_CONTROL` で読み取ります）。
* **独自のスタート用の `Win` 単独押し（開発中）：** モジュールが乗っ取るのは `Win` を*離したとき*だけで、押してから離すまでの間に他のキーが押されていない場合に限ります。Windows 標準のスタート メニューを開かせないため、`Win` を離す前に空のマスク キー `0xE8` の押下を合成します（AutoHotkey 由来の手法で、システムはショートカットとみなして何もしません）。また、スタート ボタンのシステム ツールチップ（`explorer.exe` の XAML ポップアップ）がメニューを覆わないよう、ボタンの上に現れるエクスプローラーの最前面ウィンドウを監視して一時的に隠します。

### 音声、ターミナル、カラーピッカー
* **実際に音を出しているプロセスを見つける：** 最近のブラウザーでは子の音声プロセスが音を出しています。コアは GSMTC のメディア セッションから `AppUserModelId` を受け取り、その ID を持つウィンドウを探して（または `.exe` のパスで照合して）PID を取得し、**子プロセスのツリー全体**に対して WASAPI process loopback を開始します。`"spotify.exe"` のようなハードコードされたリストは使いません。
* **ConPTY でのコマンドの追跡：** Windows Terminal と VS Code は疑似コンソール ConPTY を通して動くため、通常のコンソール イベントはそこからは届きません。アイランドはユーザーがターミナルのウィンドウを離れた瞬間に追跡を始め、ターミナルのプロセス ツリーからシェルとその子タスクを見つけて、`RegisterWaitForSingleObject` で終了を待ちます。
* **クリックが突き抜けない動くカラーピッカー：** Windows では完全に透明なウィンドウ（`alpha = 0`）はマウスのクリックを透過させてしまうため、カラーピッカーの画面全体のキャッチャーはアルファ `1/255` で塗りつぶし、ルーペ自体は `WDA_EXCLUDEFROMCAPTURE` + `CAPTUREBLT` で取り込みから除外して、自分を拡大しないようにしています。OKLCH のパレットを生成するときは、sRGB の色域外の彩度を二分探索で切り詰めます。

---

## 5. ソース コードの地図

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
