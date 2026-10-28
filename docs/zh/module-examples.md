# 模块示例

[English](../module-examples.md) · [Русский](../ru/module-examples.md) · **简体中文** · [日本語](../ja/module-examples.md)

四个完整的模块，从最小的开始。每个都只有一个文件，不需要任何包：把文件夹复制到 `%LOCALAPPDATA%\Polynya\modules\`（便携版复制到 `polynya.exe` 旁边的 `modules\`），然后打开搜索即可。所有字段的说明见 **[模块 API](module-api.md)**。

---

## 1. 完成工作后自行关闭的命令（Node.js）

搜索中会出现一行“Copy UUID”。按 Enter 后，模块把一个新的 UUID 放入剪贴板，显示一张卡片，然后关闭搜索。

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

## 2. 用快捷键处理所选内容（Python）

在任意程序中选中文本，按 `Ctrl+Alt+S`：模块会给出这段文本的 slug、`snake_case` 和大写形式。按 Enter 复制所选的结果，并把焦点还给原程序，`Ctrl+V` 即可原地粘贴。

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

每次 `invoke` 都会附带剪贴板文本，所以“先复制，再按快捷键”也同样可用；从搜索中运行时 `selection` 为空，模块从剪贴板开始。`[main]` 也可以像功能一样绑定快捷键。有自己界面的模块，其界面中的项放在 `[[functions]]` 中：参见内置的 devtools，其中“开发工具 › 转换值”与其菜单中的项是同一个。

---

## 3. 随输入重新计算的界面（Node.js）

一个输入框和一个结果，模块在每次按 Enter 后通过 `update` 重绘结果。清单与示例 1 相同，只需换成自己的 `id` 和 `name`。`update` 会按 `id` 把选中项和正在编辑的文本保留在原来的行上，所以界面不会跳动。

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

要进入下一层，就用 `push` 代替 `update`：新界面会带着“返回”按钮从侧面滑入。“返回”由核心自己处理，所以不同界面上的行请使用不同的 id。

---

## 4. 带卡片的常驻模块（Python）

`resident = true`：进程随“岛”一起启动并持续运行。这个模块每 50 分钟提醒你休息一下。卡片会一直显示到被关闭为止，模块的卡片与其他卡片的字段完全相同。

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

## 不写模块，从任意脚本发送卡片

来自构建脚本或计划任务的一次性通知不需要模块文件夹：

```powershell
polynya push "Backup finished" "12 GB, 4 min" --icon ok --ttl 10
```

要控制卡片的所有字段，可以把 JSON 直接写入“岛”的命名管道：

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

一次连接只传一条消息。如果“岛”没有运行，打开管道会立即失败。

---

## 调试

* `stderr` 会写入日志 `%LOCALAPPDATA%\Polynya\logs\polynya.log`：`print(..., file=sys.stderr)` 或 `console.error` 的输出会和核心关于该模块的消息一起出现在那里。
* 修改代码后，`polynya modules reload` 会重启模块进程。
* 也可以手动运行模块：执行 `node main.js run`，然后输入 `{"op":"invoke","command":"main","source":"search","clipboard":"hello"}` 这样的 JSON 行，查看它的回复。
