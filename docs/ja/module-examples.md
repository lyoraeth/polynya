# モジュールのサンプル

[English](../module-examples.md) · [Русский](../ru/module-examples.md) · [简体中文](../zh/module-examples.md) · **日本語**

小さいものから順に、完成したモジュールを 4 つ紹介します。どれも 1 ファイルで完結し、パッケージは不要です。フォルダーを `%LOCALAPPDATA%\Polynya\modules\`（ポータブル版では `polynya.exe` の隣の `modules\`）にコピーして検索を開くだけで動きます。各フィールドの説明は **[モジュール API](module-api.md)** にあります。

---

## 1. 処理を済ませて閉じるコマンド（Node.js）

検索に「Copy UUID」の行が追加されます。Enter を押すと、モジュールは新しい UUID をクリップボードに入れ、チップを表示して検索を閉じます。

`modules\uuid\module.toml`：
```toml
api = 1
id = "uuid"
exe = "main.js"

name.en = "Copy UUID"
name.ru = "Скопировать UUID"

[main]
hint.en = "A new UUID v4 to the clipboard"
keywords = ["uuid", "guid"]
icon = "M4 7h16 | M4 12h16 | M4 17h10"
```

`modules\uuid\main.js`：
```js
const readline = require('node:readline');
const crypto = require('node:crypto');

const send = (msg) => process.stdout.write(JSON.stringify(msg) + '\n');

readline.createInterface({ input: process.stdin })
  .on('line', (line) => {
    const msg = JSON.parse(line);
    if (msg.op === 'invoke') {
      const id = crypto.randomUUID();
      send({ op: 'copy', text: id });
      send({ op: 'chip', key: 'uuid', icon: 'ok', title: 'UUID copied', body: id, ttl_ms: 3000 });
      send({ op: 'close' });
    }
  })
  // EOF on stdin: the island has exited or turned the module off.
  .on('close', () => process.exit(0));
```

---

## 2. ホットキーで選択範囲を扱う（Python）

任意のアプリでテキストを選択して `Ctrl+Alt+S` を押すと、モジュールがそのテキストを slug、`snake_case`、大文字に変換した候補を出します。Enter で選んだ候補がコピーされ、フォーカスが元のアプリに戻るので、そのまま `Ctrl+V` で貼り付けられます。

`modules\slug\module.toml`：
```toml
api = 1
id = "slug"
exe = "main.py"

name.en = "Slugify"

[main]
keywords = ["slug", "case"]
icon = "M4 12h16"
hotkey = "Ctrl+Alt+S"
selection = true
```

`modules\slug\main.py`：
```python
import json
import re
import sys

def send(msg):
    print(json.dumps(msg, ensure_ascii=False), flush=True)

variants = {}

for line in sys.stdin:
    msg = json.loads(line)
    if msg["op"] == "invoke":
        # By the hotkey: the selection, or what was copied if nothing is selected.
        text = msg["selection"] or msg["clipboard"]
        words = re.findall(r"\w+", text.lower())
        variants = {
            "slug": "-".join(words),
            "snake": "_".join(words),
            "upper": text.upper(),
        }
        rows = [{"type": "item", "id": k, "text": v, "hint": k} for k, v in variants.items()]
        send({"op": "open", "screen": {"rows": rows}})
    elif msg["op"] == "select":
        send({"op": "copy", "text": variants[msg["id"]]})
        send({"op": "close"})
```

どの `invoke` にもクリップボードのテキストが付くので、「コピーしてからホットキー」でも動きます。検索から実行した場合は `selection` が空になり、モジュールはクリップボードから始めます。`[main]` にも機能と同じようにホットキーを付けられます。独自の画面を持つモジュールでは、画面の項目を `[[functions]]` に書きます。内蔵の devtools を参照してください。「開発ツール › 値を変換」はそのメニューの項目と同じものです。

---

## 3. 入力に合わせて再計算する画面（Node.js）

入力欄と結果があり、Enter を押すたびにモジュールが `update` で結果を描き直します。マニフェストはサンプル 1 と同じで、`id` と `name` だけを変えます。`update` は選択中の行と編集中のテキストを `id` で同じ行に保つので、画面が跳ねません。

```js
const readline = require('node:readline');
const send = (msg) => process.stdout.write(JSON.stringify(msg) + '\n');

function screen(text) {
  const rows = [{ type: 'input', id: 'text', text, placeholder: 'Type some text' }];
  if (text) {
    rows.push(
      { type: 'divider' },
      { type: 'pair', label: 'Characters', value: String([...text].length) },
      { type: 'pair', label: 'Words', value: String(text.split(/\s+/).filter(Boolean).length) },
      { type: 'item', id: 'copy', text: 'Copy in upper case' },
    );
  }
  return { title: 'Counter', rows };
}

let current = '';
readline.createInterface({ input: process.stdin })
  .on('line', (line) => {
    const msg = JSON.parse(line);
    if (msg.op === 'invoke') send({ op: 'open', screen: screen((current = msg.clipboard)) });
    if (msg.op === 'submit') send({ op: 'update', screen: screen((current = msg.text)) });
    if (msg.op === 'select' && msg.id === 'copy') {
      send({ op: 'copy', text: current.toUpperCase() });
      send({ op: 'close' });
    }
  })
  .on('close', () => process.exit(0));
```

さらに 1 階層深く進むには、`update` の代わりに `push` を送ります。新しい画面が「戻る」ボタン付きで横からスライドしてきます。「戻る」はコアが処理するので、画面ごとに行の id を変えてください。

---

## 4. チップを使う常駐モジュール（Python）

`resident = true`：プロセスはアイランドと一緒に起動し、動き続けます。このモジュールは 50 分ごとに休憩を促します。チップは閉じられるまで残り、モジュールのチップにもほかのチップと同じフィールドがあります。

```toml
api = 1
id = "breaks"
exe = "main.py"
kind = "zone"
name.en = "Break reminder"
resident = true
```

```python
import json
import sys
import threading
import time

def send(msg):
    print(json.dumps(msg, ensure_ascii=False), flush=True)

def remind():
    while True:
        time.sleep(50 * 60)
        send({"op": "chip", "key": "breaks", "icon": "info", "title": "Time for a break", "ttl_ms": None})

threading.Thread(target=remind, daemon=True).start()
# stdin ends when the island exits or turns the module off.
for _ in sys.stdin:
    pass
```

---

## モジュールなしで任意のスクリプトからチップを送る

ビルドスクリプトやタスク スケジューラからの単発の通知には、モジュール フォルダーは要りません。

```powershell
polynya push "Backup finished" "12 GB, 4 min" --icon ok --ttl 10
```

チップのすべてのフィールドを指定したい場合は、アイランドの名前付きパイプに JSON を直接書き込みます。

```python
import ctypes, json

def session_id():
    sid = ctypes.c_ulong()
    k32 = ctypes.windll.kernel32
    k32.ProcessIdToSessionId(k32.GetCurrentProcessId(), ctypes.byref(sid))
    return sid.value

def chip(**msg):
    with open(rf"\\.\pipe\Polynya-{session_id()}", "wb", buffering=0) as pipe:
        pipe.write(json.dumps({"op": "chip", **msg}).encode())

chip(key="backup", title="Backing up…", icon="agent", animated=True)
# ...later, the same key updates the chip in place:
chip(key="backup", title="Backup finished", icon="ok", ttl_ms=5000)
```

1 回の接続で送れるメッセージは 1 つです。アイランドが起動していなければ、パイプを開く時点ですぐに失敗します。

---

## デバッグ

* `stderr` はログ `%LOCALAPPDATA%\Polynya\logs\polynya.log` に書き込まれます。`print(..., file=sys.stderr)` や `console.error` の出力は、そのモジュールに関するコアのメッセージと一緒にそこで確認できます。
* コードを変更したら、`polynya modules reload` でモジュールのプロセスを再起動します。
* モジュールは手動でも実行できます。`node main.js run` を実行し、`{"op":"invoke","command":"main","source":"search","clipboard":"hello"}` のような JSON 行を入力すると応答を確認できます。
