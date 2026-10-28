// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

// Polynya module: small web development tools.
//
// `main` opens the menu of tools. `convert` is the converter from that menu, also run from search
// and by its hotkey (Ctrl+Alt+D): by the hotkey it takes the selection of the active app, and Enter
// copies the result and returns focus to the app. The protocol is JSON lines: commands from the
// core come on stdin, screens go to stdout.

'use strict';

const fs = require('node:fs');
const path = require('node:path');
const readline = require('node:readline');

const { candidates, clamp, toPx, LEN_RE } = require('./convert');
const tools = require('./tools');
const { t } = require('./i18n');

const SETTINGS_FILE = path.join(process.env.APPDATA || '.', 'Polynya', 'devtools.json');
const DEFAULTS = { base: 16, vmin: 360, vmax: 1440, digits: 4 };
const LIMITS = { base: [1, 100], vmin: [0, 10000], vmax: [1, 10000], digits: [0, 8] };

function loadSettings() {
  try {
    return { ...DEFAULTS, ...JSON.parse(fs.readFileSync(SETTINGS_FILE, 'utf8')) };
  } catch {
    return { ...DEFAULTS };
  }
}

let settings = loadSettings();
// Results on screen by row id: selecting a row copies its value.
let results = new Map();
let nextId = 0;
// Clipboard text from the last `invoke`, the starting value of the converter.
let clip = '';
let clampText = '16px 24px';

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + '\n');
}

/** One line of a value for a row; the core trims what does not fit. */
const preview = (s, n = 160) => s.replace(/\s+/g, ' ').trim().slice(0, n);

/** A row that copies `value` when chosen. */
function result(label, value, icon) {
  const id = `r${nextId++}`;
  results.set(id, { value, icon });
  return { type: 'item', id, text: preview(value), hint: label };
}

function convertScreen(text, title) {
  const rows = candidates(text, settings).map((c) => result(c.label, c.value, c.icon));
  if (rows.length === 0 && text.trim()) rows.push({ type: 'text', text: t.nothing });
  if (rows.length > 0) rows.push({ type: 'divider' });
  rows.push({ type: 'input', id: 'in:convert', text: preview(text, 2000), placeholder: t.edit });
  return { title, rows };
}

function clampScreen(text) {
  const rows = [{ type: 'input', id: 'in:clamp', text, placeholder: t.clampInput }];
  const sizes = [...text.matchAll(LEN_RE)].map((m) => toPx(parseFloat(m[1]), m[2].toLowerCase(), settings));
  // Bare numbers are px.
  if (sizes.length < 2) sizes.splice(0, sizes.length, ...(text.match(/-?\d*\.?\d+/g) || []).map(Number));
  if (sizes.length >= 2 && sizes[0] !== sizes[1]) {
    const [min, max] = sizes;
    const value = clamp(min, max, settings);
    rows.push({ type: 'divider' }, result('clamp()', value), result('font-size', `font-size: ${value};`));
    rows.push({ type: 'section', title: '' });
    const { vmin, vmax } = settings;
    for (const w of [vmin, Math.round((vmin + vmax) / 2), vmax]) {
      const px = min + ((max - min) * (w - vmin)) / (vmax - vmin);
      rows.push({ type: 'pair', label: t.clampAt(w), value: `${+px.toFixed(2)}px` });
    }
  }
  return { title: t.clamp, rows };
}

function listScreen(title, pairs, input) {
  const rows = input ? [input, { type: 'divider' }] : [];
  for (const [label, value] of pairs) rows.push(result(label, value));
  return { title, rows };
}

function settingsScreen() {
  const rows = [];
  for (const key of Object.keys(DEFAULTS)) {
    rows.push({ type: 'section', title: t[key] }, { type: 'input', id: `set:${key}`, text: String(settings[key]), placeholder: String(DEFAULTS[key]) });
  }
  return { title: t.settings, rows };
}

const TOOLS = [
  ['convert', t.convert, t.convertHint, () => convertScreen(clip, t.convert)],
  ['clamp', t.clamp, t.clampHint, () => clampScreen(clampText)],
  ['ids', t.ids, t.idsHint, () => listScreen(t.ids, tools.ids())],
  ['now', t.now, t.nowHint, () => listScreen(t.now, tools.now())],
  ['hash', t.hash, t.hashHint, () => hashScreen('')],
  ['lorem', t.lorem, t.loremHint, () => listScreen(t.lorem, tools.lorem())],
];

function hashScreen(text) {
  const input = { type: 'input', id: 'in:hash', text, placeholder: t.hashInput };
  return listScreen(t.hash, text ? tools.hashes(text) : [], input);
}

function toolsScreen() {
  // The converter row is the `convert` function: the core shows its hotkey and runs it with `invoke`.
  const rows = TOOLS.map(([id, text, hint]) => ({ type: 'item', id: `tool:${id}`, text, hint, ...(id === 'convert' && { command: 'convert' }) }));
  rows.push({ type: 'divider' }, { type: 'item', id: 'tool:settings', text: t.settings });
  return { rows };
}

function copy(value, icon) {
  send({ op: 'copy', text: value });
  send({ op: 'chip', key: 'devtools', icon: icon || 'ok', title: t.copied, body: preview(value), ttl_ms: 3000 });
  send({ op: 'close' });
}

function onInvoke({ command, source, selection = '', clipboard = '' }) {
  clip = clipboard;
  if (command !== 'convert') {
    results = new Map();
    return send({ op: 'open', screen: toolsScreen() });
  }
  // By the hotkey: what is selected, or what was copied; quick, without a title. From the menu: the
  // next screen; from search: a page of search. Both start from the clipboard.
  if (source === 'hotkey') return send({ op: 'open', screen: convertScreen(selection || clipboard) });
  send({ op: source === 'menu' ? 'push' : 'open', screen: convertScreen(clipboard, t.convert) });
}

function onSelect({ id }) {
  if (results.has(id)) {
    const { value, icon } = results.get(id);
    return copy(value, icon);
  }
  if (id === 'tool:settings') return send({ op: 'push', screen: settingsScreen() });
  const tool = TOOLS.find(([name]) => `tool:${name}` === id);
  if (tool) send({ op: 'push', screen: tool[3]() });
}

function onSubmit({ id, text }) {
  if (id === 'in:convert') return send({ op: 'update', screen: convertScreen(text, t.convert) });
  if (id === 'in:clamp') {
    clampText = text;
    return send({ op: 'update', screen: clampScreen(text) });
  }
  if (id === 'in:hash') return send({ op: 'update', screen: hashScreen(text) });
  if (id.startsWith('set:')) {
    const key = id.slice(4);
    const n = parseFloat(text);
    const [lo, hi] = LIMITS[key] || [];
    if (Number.isFinite(n) && n >= lo && n <= hi) {
      settings = { ...settings, [key]: key === 'digits' ? Math.round(n) : n };
      // A viewport range that ends before it starts would turn clamp() inside out.
      if (settings.vmax <= settings.vmin) settings.vmax = settings.vmin + 1;
      try {
        fs.mkdirSync(path.dirname(SETTINGS_FILE), { recursive: true });
        fs.writeFileSync(SETTINGS_FILE, JSON.stringify(settings, null, 2));
      } catch (e) {
        console.error(`settings: ${e.message}`);
      }
    }
    send({ op: 'update', screen: settingsScreen() });
  }
}

const HANDLERS = { invoke: onInvoke, select: onSelect, submit: onSubmit, closed: () => (results = new Map()) };

if (process.argv[2] === 'run') {
  const rl = readline.createInterface({ input: process.stdin });
  rl.on('line', (line) => {
    try {
      const msg = JSON.parse(line);
      HANDLERS[msg.op]?.(msg);
    } catch (e) {
      // stderr goes to the Polynya log.
      console.error(e.stack || String(e));
    }
  });
  // EOF: the core exited or stopped the module.
  rl.on('close', () => process.exit(0));
}
