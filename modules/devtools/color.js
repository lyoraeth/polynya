// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

// CSS colors: parsing HEX, rgb(), hsl() and oklch(), and printing them back in every format.
// A color is { r, g, b, a } with sRGB channels in 0..1.

'use strict';

const NUM = String.raw`[-+]?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?`;
// Any color the module understands, for finding them inside a CSS line.
const COLOR_RE = new RegExp(
  String.raw`#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{3,4})\b|\b(?:rgba?|hsla?|oklch)\(\s*${NUM}(?:%|deg)?(?:\s*[,\s]\s*${NUM}%?){2}(?:\s*[,/]\s*${NUM}%?)?\s*\)`,
  'gi',
);

const clamp01 = (x) => Math.min(1, Math.max(0, x));

/** A number with at most `digits` decimals and no trailing zeros. */
function fmt(x, digits) {
  const s = (+x.toFixed(digits)).toString();
  return s === '-0' ? '0' : s;
}

/** Alpha from `0.5` or `50%`. */
function alpha(s) {
  if (s === undefined) return 1;
  return clamp01(s.endsWith('%') ? parseFloat(s) / 100 : parseFloat(s));
}

/** Parses one CSS color; `null` if it is not one. */
function parse(text) {
  const s = text.trim().toLowerCase();
  let m = /^#([0-9a-f]{3,8})$/.exec(s);
  if (m) {
    let h = m[1];
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join('');
    if (h.length !== 6 && h.length !== 8) return null;
    const n = (i) => parseInt(h.slice(i, i + 2), 16) / 255;
    return { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? n(6) : 1 };
  }
  m = /^(rgba?|hsla?|oklch)\((.*)\)$/.exec(s);
  if (!m) return null;
  // Both the legacy `rgb(1, 2, 3, 0.5)` and the modern `rgb(1 2 3 / 50%)` syntax.
  const parts = m[2].split(/\s*[,/]\s*|\s+/).filter(Boolean);
  if (parts.length < 3 || parts.length > 4 || parts.some((p) => !new RegExp(`^${NUM}(%|deg)?$`).test(p))) return null;
  const [x, y, z, w] = parts;
  const a = alpha(w);
  switch (m[1]) {
    case 'rgb':
    case 'rgba': {
      const ch = (p) => clamp01(p.endsWith('%') ? parseFloat(p) / 100 : parseFloat(p) / 255);
      return { r: ch(x), g: ch(y), b: ch(z), a };
    }
    case 'hsl':
    case 'hsla':
      return { ...fromHsl(parseFloat(x), parseFloat(y) / 100, parseFloat(z) / 100), a };
    default: {
      const l = x.endsWith('%') ? parseFloat(x) / 100 : parseFloat(x);
      // Chroma as a percentage is relative to 0.4, as in CSS Color 4.
      const c = y.endsWith('%') ? (parseFloat(y) / 100) * 0.4 : parseFloat(y);
      return { ...fromOklch(l, c, parseFloat(z)), a };
    }
  }
}

function fromHsl(h, s, l) {
  h = ((h % 360) + 360) % 360;
  const k = (n) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n) => l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
  return { r: clamp01(f(0)), g: clamp01(f(8)), b: clamp01(f(4)) };
}

function toHsl({ r, g, b }) {
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return { h: 0, s: 0, l };
  const s = d / (1 - Math.abs(2 * l - 1));
  let h;
  if (max === r) h = ((g - b) / d) % 6;
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  return { h: (h * 60 + 360) % 360, s, l };
}

const toLinear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const toGamma = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

// OKLab by Björn Ottosson: https://bottosson.github.io/posts/oklab/
function toOklch({ r, g, b }) {
  const [lr, lg, lb] = [r, g, b].map(toLinear);
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  const c = Math.hypot(A, B);
  // Grays have no hue; 0 instead of noise from rounding.
  const h = c < 1e-4 ? 0 : ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360;
  return { l: L, c, h };
}

/** OKLCH to sRGB; colors out of the sRGB gamut are clipped. */
function fromOklch(L, c, h) {
  const A = c * Math.cos((h * Math.PI) / 180);
  const B = c * Math.sin((h * Math.PI) / 180);
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  const lin = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
  const [r, g, b] = lin.map((x) => clamp01(toGamma(clamp01(x))));
  return { r, g, b };
}

const byte = (x) => Math.round(clamp01(x) * 255);
const alphaSuffix = (a, sep) => (a < 1 ? `${sep}${fmt(a, 3)}` : '');

function hex(c) {
  const h = (x) => byte(x).toString(16).padStart(2, '0');
  return `#${h(c.r)}${h(c.g)}${h(c.b)}${c.a < 1 ? h(c.a) : ''}`;
}

/** Every format a color can be printed in: [label, text]. */
function formats(c) {
  const { h, s, l } = toHsl(c);
  const ok = toOklch(c);
  return [
    ['HEX', hex(c)],
    ['RGB', `rgb(${byte(c.r)} ${byte(c.g)} ${byte(c.b)}${alphaSuffix(c.a, ' / ')})`],
    ['HSL', `hsl(${fmt(h, 1)} ${fmt(s * 100, 1)}% ${fmt(l * 100, 1)}%${alphaSuffix(c.a, ' / ')})`],
    ['OKLCH', `oklch(${fmt(ok.l * 100, 2)}% ${fmt(ok.c, 4)} ${fmt(ok.h, 2)}${alphaSuffix(c.a, ' / ')})`],
    ['rgba()', `rgba(${byte(c.r)}, ${byte(c.g)}, ${byte(c.b)}, ${fmt(c.a, 3)})`],
  ];
}

/** Which format a color was written in, to leave it out of the conversions. */
function kind(text) {
  const s = text.trim().toLowerCase();
  if (s.startsWith('#')) return 'HEX';
  if (s.startsWith('rgba')) return 'rgba()';
  if (s.startsWith('rgb')) return 'RGB';
  if (s.startsWith('hsl')) return 'HSL';
  return 'OKLCH';
}

module.exports = { COLOR_RE, parse, formats, kind, hex, fmt };
