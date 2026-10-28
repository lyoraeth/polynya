// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

// Generators of the tools list: ids, the current time, hashes and placeholder text.

'use strict';

const crypto = require('node:crypto');
const { t, lang } = require('./i18n');

const NANOID = 'useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict';

function nanoid(size = 21) {
  const bytes = crypto.randomBytes(size);
  let id = '';
  for (const b of bytes) id += NANOID[b & 63];
  return id;
}

/** [label, value] */
function ids() {
  const uuid = crypto.randomUUID();
  return [
    ['UUID v4', uuid],
    ['UUID, no dashes', uuid.replaceAll('-', '')],
    ['nanoid', nanoid()],
    ['hex, 16', crypto.randomBytes(8).toString('hex')],
    ['hex, 32', crypto.randomBytes(16).toString('hex')],
    ['base64url, 32 bytes', crypto.randomBytes(32).toString('base64url')],
  ];
}

function now() {
  const d = new Date();
  return [
    [t.unixS, String(Math.floor(d.getTime() / 1000))],
    [t.unixMs, String(d.getTime())],
    [t.utc, d.toISOString()],
    [t.local, d.toLocaleString(lang)],
    ['RFC 7231', d.toUTCString()],
  ];
}

function hashes(text) {
  const h = (alg, enc = 'hex') => crypto.createHash(alg).update(text, 'utf8').digest(enc);
  return [
    ['SHA-256', h('sha256')],
    ['SHA-256, base64', h('sha256', 'base64')],
    ['SHA-1', h('sha1')],
    ['MD5', h('md5')],
  ];
}

const LOREM =
  'Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. ' +
  'Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. ' +
  'Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. ' +
  'Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.';

function lorem() {
  const sentence = LOREM.split('. ')[0] + '.';
  return [
    [t.sentence, sentence],
    [t.paragraph, LOREM],
    [t.paragraphs, [LOREM, LOREM, LOREM].join('\n\n')],
  ];
}

module.exports = { ids, now, hashes, lorem };
