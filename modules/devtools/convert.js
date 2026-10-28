// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

// Quick conversion: guesses what a piece of text is and offers what it can be turned into.
// Values inside a CSS line are converted in place, so `padding: 16px 24px` becomes
// `padding: 1rem 1.5rem`, ready to paste back.

'use strict';

const color = require('./color');
const { t, lang } = require('./i18n');

const { fmt } = color;
const LEN = String.raw`-?(?:\d+\.?\d*|\.\d+)`;
const LEN_RE = new RegExp(String.raw`(${LEN})(px|rem|em|pt)(?![\w%-])`, 'gi');
// `16px 24px`, `16 24`, `16px, 24px`, `16px → 24px`, optionally after `font-size:`.
const PAIR_RE = new RegExp(String.raw`^(\s*[\w-]+\s*:\s*)?(${LEN})(px|rem|em|pt)?\s*(?:,|→|->|\.\.|\s)\s*(${LEN})(px|rem|em|pt)?(\s*;?\s*)$`, 'i');
const MAX_RESULTS = 12;

/** A length in px. */
function toPx(n, unit, s) {
  switch (unit) {
    case 'rem':
    case 'em':
      return n * s.base;
    case 'pt':
      return n / 0.75;
    default:
      return n;
  }
}

function fromPx(px, unit, s) {
  switch (unit) {
    case 'rem':
    case 'em':
      return px / s.base;
    case 'pt':
      return px * 0.75;
    default:
      return px;
  }
}

/** `clamp()` that grows from `minPx` to `maxPx` between the viewport widths in the settings. */
function clamp(minPx, maxPx, s) {
  const slope = (maxPx - minPx) / (s.vmax - s.vmin);
  const intercept = minPx - slope * s.vmin;
  const vw = fmt(Math.abs(slope * 100), s.digits);
  const rem = (px) => `${fmt(px / s.base, s.digits)}rem`;
  const preferred = slope === 0 ? rem(minPx) : `${rem(intercept)} ${slope < 0 ? '-' : '+'} ${vw}vw`;
  return `clamp(${rem(Math.min(minPx, maxPx))}, ${preferred}, ${rem(Math.max(minPx, maxPx))})`;
}

/** Converts every CSS length in `text` to `unit`; `null` if nothing changes. */
function lengths(text, unit, s) {
  let changed = false;
  const out = text.replace(LEN_RE, (all, n, from) => {
    from = from.toLowerCase();
    if (from === unit || (unit === 'em' && from === 'rem') || (unit === 'rem' && from === 'em')) return all;
    changed = true;
    return fmt(fromPx(toPx(parseFloat(n), from, s), unit, s), s.digits) + unit;
  });
  return changed ? out : null;
}

function lengthCandidates(text, s, out) {
  const units = new Set([...text.matchAll(LEN_RE)].map((m) => m[2].toLowerCase()));
  if (units.size === 0) {
    // A bare number: either way round.
    const m = /^\s*(-?(?:\d+\.?\d*|\.\d+))\s*$/.exec(text);
    // Long numbers are timestamps or ids rather than sizes.
    if (m && Math.abs(parseFloat(m[1])) < 100000) {
      const n = parseFloat(m[1]);
      out.push({ label: 'px → rem', value: `${fmt(n / s.base, s.digits)}rem` });
      out.push({ label: 'rem → px', value: `${fmt(n * s.base, s.digits)}px` });
    }
    return;
  }
  const targets = [];
  if (units.has('px')) targets.push('rem', 'em', 'pt');
  if (units.has('rem') || units.has('em')) targets.push('px', 'pt');
  if (units.has('pt')) targets.push('px', 'rem');
  const from = [...units].join(', ');
  for (const unit of new Set(targets)) {
    const value = lengths(text, unit, s);
    if (value) out.push({ label: `${from} → ${unit}`, value });
  }
}

function clampCandidate(text, s, out) {
  const m = PAIR_RE.exec(text);
  if (!m) return;
  const [, prop = '', a, ua = 'px', b, ub = 'px', tail = ''] = m;
  const min = toPx(parseFloat(a), ua.toLowerCase(), s);
  const max = toPx(parseFloat(b), ub.toLowerCase(), s);
  if (min === max) return;
  out.unshift({ label: t.fluid, value: `${prop}${clamp(min, max, s)}${tail}` });
}

function colorCandidates(text, out) {
  const found = [...text.matchAll(color.COLOR_RE)].map((m) => [m[0], color.parse(m[0])]).filter(([, c]) => c);
  if (found.length === 0) return;
  const icon = `color:${color.hex({ ...found[0][1], a: 1 })}`;
  const kinds = new Set(found.map(([src]) => color.kind(src)));
  for (const [label] of color.formats(found[0][1])) {
    if (kinds.size === 1 && kinds.has(label)) continue;
    const value = text.replace(color.COLOR_RE, (src) => {
      const c = color.parse(src);
      return c ? color.formats(c).find(([l]) => l === label)[1] : src;
    });
    out.push({ label, value, icon });
  }
}

function timeCandidates(text, out) {
  const s = text.trim();
  let ms = null;
  let given = null;
  if (/^\d{10}(\.\d+)?$/.test(s)) [ms, given] = [parseFloat(s) * 1000, 's'];
  else if (/^\d{13}$/.test(s)) [ms, given] = [parseInt(s, 10), 'ms'];
  else if (/^\d{4}-\d{2}-\d{2}([t ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?)?(z|[+-]\d{2}:?\d{2})?$/i.test(s)) ms = Date.parse(s);
  if (ms === null || Number.isNaN(ms)) return;
  const d = new Date(ms);
  out.push({ label: t.utc, value: d.toISOString() });
  out.push({ label: t.local, value: d.toLocaleString(lang) });
  if (given !== 's') out.push({ label: t.unixS, value: String(Math.floor(ms / 1000)) });
  if (given !== 'ms') out.push({ label: t.unixMs, value: String(Math.round(ms)) });
}

/** Text from Base64 (or base64url) if it decodes to readable UTF-8. */
function fromBase64(s) {
  if (s.length < 4 || !/^[A-Za-z0-9+/_-]+={0,2}$/.test(s) || (/[+/=]/.test(s) && s.length % 4 !== 0)) return null;
  try {
    const text = new TextDecoder('utf-8', { fatal: true }).decode(Buffer.from(s, /[-_]/.test(s) ? 'base64url' : 'base64'));
    // eslint-disable-next-line no-control-regex
    return text && !/[\u0000-\u0008\u000e-\u001f�]/.test(text) ? text : null;
  } catch {
    return null;
  }
}

function jwtCandidates(text, out) {
  const m = /^\s*(eyJ[\w-]*)\.(eyJ[\w-]*)\.[\w-]*\s*$/.exec(text);
  if (!m) return false;
  const part = (p) => {
    try {
      return JSON.parse(Buffer.from(p, 'base64url').toString('utf8'));
    } catch {
      return null;
    }
  };
  const [header, payload] = [part(m[1]), part(m[2])];
  if (!payload) return false;
  const exp = typeof payload.exp === 'number' ? t.expires(new Date(payload.exp * 1000).toLocaleString(lang)) : '';
  out.push({ label: exp ? `${t.jwtPayload} · ${exp}` : t.jwtPayload, value: JSON.stringify(payload, null, 2) });
  if (header) out.push({ label: t.jwtHeader, value: JSON.stringify(header, null, 2) });
  return true;
}

function jsonCandidates(text, out) {
  const s = text.trim();
  if (!/^[[{"]/.test(s)) return;
  let v;
  try {
    v = JSON.parse(s);
  } catch {
    return;
  }
  if (typeof v === 'string') {
    out.push({ label: t.jsonUnstr, value: v });
    return;
  }
  out.push({ label: t.jsonPretty, value: JSON.stringify(v, null, 2) });
  out.push({ label: t.jsonMin, value: JSON.stringify(v) });
}

/** Words of an identifier or a short phrase: `fontSize`, `font-size`, `Font size`. */
function words(text) {
  const s = text.trim();
  // Names start with a letter; `16px 24px` is sizes, not words.
  if (s.length > 64 || !/^\p{L}[\p{L}\d_\- ]*$/u.test(s)) return null;
  const w = s
    .replace(/(\p{Ll}|\d)(\p{Lu})/gu, '$1 $2')
    .replace(/(\p{Lu})(\p{Lu}\p{Ll})/gu, '$1 $2')
    .split(/[\s_-]+/)
    .filter(Boolean)
    .map((x) => x.toLowerCase());
  return w.length >= 2 && w.length <= 8 ? w : null;
}

function caseCandidates(text, out) {
  const w = words(text);
  if (!w) return;
  const cap = (x) => x.charAt(0).toUpperCase() + x.slice(1);
  out.push({ label: 'camelCase', value: w[0] + w.slice(1).map(cap).join('') });
  out.push({ label: 'PascalCase', value: w.map(cap).join('') });
  out.push({ label: 'snake_case', value: w.join('_') });
  out.push({ label: 'kebab-case', value: w.join('-') });
  out.push({ label: 'CONSTANT_CASE', value: w.join('_').toUpperCase() });
  out.push({ label: t.words, value: cap(w.join(' ')) });
}

/** A CSS value or one declaration on one line (`16px 24px`, `color: #fff;`): colors and sizes in
 * it are converted in place. A paragraph, code or a document is not, even if it has a `#fff` in it. */
function isCss(s) {
  return !s.includes('\n') && s.length <= 200 && /^([\w-]+\s*:\s*)?[^:;{}]+;?$/.test(s);
}

/** What `text` can be turned into, by what it is: a color or a size gets conversions, a timestamp
 * gets dates, JSON gets formatting, a name gets cases. Only text that is none of these gets the
 * generic encodings. [{ label, value, icon? }], most likely first. */
function candidates(text, s) {
  const trimmed = text.trim();
  if (!trimmed) return [];
  const out = [];
  if (jwtCandidates(text, out)) return finish(text, out);
  timeCandidates(text, out);
  if (out.length) return finish(text, out);
  jsonCandidates(text, out);
  if (out.length) return finish(text, out);
  if (isCss(trimmed)) {
    colorCandidates(text, out);
    lengthCandidates(text, s, out);
    clampCandidate(text, s, out);
    if (out.length) return finish(text, out);
  }
  const decoded = fromBase64(trimmed);
  if (decoded) out.push({ label: t.b64dec, value: decoded });
  if (!/\s/.test(trimmed) && /%[0-9a-f]{2}/i.test(trimmed)) {
    try {
      out.push({ label: t.urlDec, value: decodeURIComponent(trimmed) });
    } catch {
      // Not valid percent-encoding after all.
    }
  }
  if (out.length) return finish(text, out);
  caseCandidates(text, out);
  if (out.length) return finish(text, out);
  // Plain text: only what fits any text.
  out.push({ label: t.b64enc, value: Buffer.from(text, 'utf8').toString('base64') });
  if (!text.includes('\n') && encodeURIComponent(text) !== text) out.push({ label: t.urlEnc, value: encodeURIComponent(text) });
  if (/["\\\n\t]/.test(text)) out.push({ label: t.jsonStr, value: JSON.stringify(text) });
  return finish(text, out);
}

/** Drops results equal to the input or to each other. */
function finish(text, out) {
  const seen = new Set([text]);
  return out.filter((c) => !seen.has(c.value) && seen.add(c.value)).slice(0, MAX_RESULTS);
}

module.exports = { candidates, clamp, toPx, LEN_RE };
