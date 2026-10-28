# Примеры модулей

[English](../module-examples.md) · **Русский** · [简体中文](../zh/module-examples.md) · [日本語](../ja/module-examples.md)

Четыре готовых модуля, от самого маленького. Каждый умещается в один файл и не требует пакетов: скопируйте папку в `%LOCALAPPDATA%\Polynya\modules\` (в портативной версии - в `modules\` рядом с `polynya.exe`) и откройте поиск. Все поля описаны в **[API модулей](module-api.md)**.

---

## 1. Команда, которая делает свое дело и закрывается (Node.js)

В поиске появляется строка "Copy UUID". По Enter модуль кладет новый UUID в буфер обмена, показывает чип и закрывает поиск.

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

## 2. Выделение по сочетанию клавиш (Python)

Выделите текст в любой программе и нажмите `Ctrl+Alt+S`: модуль предложит этот текст в виде slug, в `snake_case` и прописными. Enter копирует выбранный вариант и возвращает фокус в программу, так что `Ctrl+V` сразу вставит его на место.

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

В каждом `invoke` есть и текст из буфера, так что сценарий "скопировал, потом сочетание" тоже работает; из поиска `selection` пустой, и модуль начинает с буфера. Сочетание можно повесить и на `[main]`, как на любую функцию. У модуля со своим интерфейсом его пункты идут в `[[functions]]`: см. встроенный devtools, где "Девтулз › Преобразовать значение" - тот же пункт, что в его меню.

---

## 3. Экран, который пересчитывается по мере ввода (Node.js)

Поле ввода и результат, который модуль перерисовывает через `update` после каждого Enter. Манифест такой же, как в примере 1, со своими `id` и `name`. `update` оставляет выделение и редактируемый текст на своих строках по `id`, поэтому экран не прыгает.

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

Чтобы уйти на уровень глубже, отправьте `push` вместо `update`: новый экран въедет сбоку с кнопкой "назад". "Назад" ядро обрабатывает само, поэтому давайте строкам на разных экранах разные id.

---

## 4. Постоянный модуль с чипами (Python)

`resident = true`: процесс стартует вместе с островом и продолжает работать. Этот модуль раз в 50 минут напоминает сделать перерыв. Чип висит, пока его не закроют; у чипа модуля те же поля, что и у любого другого.

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

## Чип из любого скрипта, без модуля

Разовому уведомлению из скрипта сборки или задачи планировщика папка модуля не нужна:

```powershell
polynya push "Backup finished" "12 GB, 4 min" --icon ok --ttl 10
```

Чтобы управлять всеми полями чипа, пишите JSON прямо в именованный канал острова:

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

Одно подключение - одно сообщение. Если остров не запущен, открыть канал не получится сразу же.

---

## Отладка

* `stderr` попадает в лог `%LOCALAPPDATA%\Polynya\logs\polynya.log`: `print(..., file=sys.stderr)` или `console.error` видны там вместе с сообщениями ядра о модуле.
* `polynya modules reload` перезапускает процессы модулей после правки кода.
* Модуль можно запустить руками: `node main.js run`, а затем вводить JSON-строки вроде `{"op":"invoke","command":"main","source":"search","clipboard":"hello"}` и смотреть его ответы.
