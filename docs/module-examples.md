# Module examples

**English** · [Русский](ru/module-examples.md) · [简体中文](zh/module-examples.md) · [日本語](ja/module-examples.md)

Four complete modules, from the smallest one up. Each fits in one file and needs no packages: copy the folder into `%LOCALAPPDATA%\Polynya\modules\` (in a portable copy, into `modules\` next to `polynya.exe`) and open search. Every field is described in the **[Module API](module-api.md)**.

---

## 1. A command that does its work and closes (Node.js)

Search gets a "Copy UUID" row. On Enter, the module puts a new UUID on the clipboard, shows a chip and closes search.

`modules\uuid\module.toml`:
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

`modules\uuid\main.js`:
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

## 2. Selection on a hotkey (Python)

Select text in any app and press `Ctrl+Alt+S`: the module offers the text as a slug, in `snake_case` and in upper case. Enter copies the chosen variant and returns focus to the app, so `Ctrl+V` pastes it in place.

`modules\slug\module.toml`:
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

`modules\slug\main.py`:
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

Every `invoke` carries the clipboard too, so "copy, then the hotkey" works as well; run from search, `selection` is empty and the module starts from the clipboard. `[main]` takes a hotkey like any function. For a module with an interface of its own, its items go into `[[functions]]`: see the built-in devtools, where "Devtools › Convert value" is the same item as in its menu.

---

## 3. A screen that recalculates as you type (Node.js)

A text field and a result that the module redraws with `update` after each Enter. The manifest is the same as in example 1, with its own `id` and `name`. `update` keeps the selection and the text being edited on their rows by `id`, so the screen doesn't jump.

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

To go one level deeper, send `push` instead of `update`: the new screen slides in with a "back" button. The core handles "back" itself, so give rows on different screens different ids.

---

## 4. A resident module with chips (Python)

`resident = true`: the process starts with the island and keeps running. This module reminds you to take a break every 50 minutes. The chip stays until it is closed, and a module chip has the same fields as any other.

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

## A chip from any script, without a module

A one-off notification from a build script or a scheduled task needs no module folder:

```powershell
polynya push "Backup finished" "12 GB, 4 min" --icon ok --ttl 10
```

To control every chip field, write the JSON straight into the island's named pipe:

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

One connection carries one message. If the island isn't running, opening the pipe fails at once.

---

## Debugging

* `stderr` goes to the log `%LOCALAPPDATA%\Polynya\logs\polynya.log`: a `print(..., file=sys.stderr)` or `console.error` is visible there along with the core's messages about the module.
* `polynya modules reload` restarts module processes after a code change.
* A module can be run by hand: `node main.js run` and type JSON lines like `{"op":"invoke","command":"main","source":"search","clipboard":"hello"}` to see its answers.
