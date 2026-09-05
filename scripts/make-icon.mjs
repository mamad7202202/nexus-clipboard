/**
 * Renders the Nexus app icon to a PNG without any image dependency.
 *
 * The mark: a rounded-square badge carrying a three-layer "stack", which reads
 * as clipboard history at 16 px as well as at 1024 px. Shapes are rasterised
 * from signed-distance functions so every edge is anti-aliased, and the PNG is
 * assembled by hand (zlib is the only thing we need from Node).
 */

import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

const SIZE = 1024;

// ---------------------------------------------------------------------------
// Geometry helpers (all in 0..1 space, scaled by SIZE)
// ---------------------------------------------------------------------------

/** Signed distance to a rounded rectangle centred at (cx, cy). */
function sdRoundRect(px, py, cx, cy, hw, hh, r) {
  const qx = Math.abs(px - cx) - (hw - r);
  const qy = Math.abs(py - cy) - (hh - r);
  const ax = Math.max(qx, 0);
  const ay = Math.max(qy, 0);
  return Math.hypot(ax, ay) + Math.min(Math.max(qx, qy), 0) - r;
}

/** Convert a signed distance to coverage, anti-aliased over ~1.5 px. */
function coverage(d, feather = 1.5) {
  return Math.min(1, Math.max(0, 0.5 - d / feather));
}

function mix(a, b, t) {
  return a + (b - a) * t;
}

/** Composite `src` (straight alpha) over `dst` (straight alpha), in place. */
function over(dst, i, r, g, b, a) {
  if (a <= 0) return;
  const da = dst[i + 3] / 255;
  const outA = a + da * (1 - a);
  if (outA <= 0) return;
  for (let c = 0; c < 3; c++) {
    const sc = [r, g, b][c];
    const dc = dst[i + c] / 255;
    dst[i + c] = Math.round(((sc * a + dc * da * (1 - a)) / outA) * 255);
  }
  dst[i + 3] = Math.round(outA * 255);
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

const pixels = new Uint8Array(SIZE * SIZE * 4); // RGBA, straight alpha

const S = SIZE;
const badge = { cx: S / 2, cy: S / 2, hw: S * 0.44, hh: S * 0.44, r: S * 0.225 };

// Gradient endpoints: deep indigo → violet, the app's accent family.
const TOP = [0x4f, 0x46, 0xe5];
const BOTTOM = [0x7c, 0x3a, 0xed];
const GLOW = [0xa7, 0x8b, 0xfa];

for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    const i = (y * S + x) * 4;
    const px = x + 0.5;
    const py = y + 0.5;

    const d = sdRoundRect(px, py, badge.cx, badge.cy, badge.hw, badge.hh, badge.r);
    const a = coverage(d, 2.0);
    if (a <= 0) continue;

    // Vertical gradient with a soft radial highlight in the upper-left.
    const t = y / S;
    let r = mix(TOP[0], BOTTOM[0], t);
    let g = mix(TOP[1], BOTTOM[1], t);
    let b = mix(TOP[2], BOTTOM[2], t);

    const hx = S * 0.32;
    const hy = S * 0.28;
    const hd = Math.hypot(px - hx, py - hy) / (S * 0.55);
    const glow = Math.max(0, 1 - hd) ** 2 * 0.45;
    r = mix(r, GLOW[0], glow);
    g = mix(g, GLOW[1], glow);
    b = mix(b, GLOW[2], glow);

    over(pixels, i, r / 255, g / 255, b / 255, a);
  }
}

// The stack: three rounded bars, back-to-front, each slightly inset and more
// opaque than the one behind it.
const bars = [
  { w: 0.34, h: 0.055, y: 0.355, alpha: 0.42 },
  { w: 0.42, h: 0.055, y: 0.475, alpha: 0.68 },
  { w: 0.50, h: 0.150, y: 0.625, alpha: 1.0 },
];

for (const bar of bars) {
  const hw = (S * bar.w) / 2;
  const hh = (S * bar.h) / 2;
  const cy = S * bar.y;
  const cx = S / 2;
  const r = Math.min(hw, hh) * 0.92;

  for (let y = Math.floor(cy - hh - 3); y <= Math.ceil(cy + hh + 3); y++) {
    if (y < 0 || y >= S) continue;
    for (let x = Math.floor(cx - hw - 3); x <= Math.ceil(cx + hw + 3); x++) {
      if (x < 0 || x >= S) continue;
      const d = sdRoundRect(x + 0.5, y + 0.5, cx, cy, hw, hh, r);
      const a = coverage(d, 2.0) * bar.alpha;
      if (a <= 0) continue;
      over(pixels, (y * S + x) * 4, 1, 1, 1, a);
    }
  }
}

// ---------------------------------------------------------------------------
// PNG encoding
// ---------------------------------------------------------------------------

function crc32(buf) {
  let c;
  const table = crc32.table || (crc32.table = (() => {
    const t = new Int32Array(256);
    for (let n = 0; n < 256; n++) {
      c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      t[n] = c;
    }
    return t;
  })());

  let crc = -1;
  for (let i = 0; i < buf.length; i++) crc = (crc >>> 8) ^ table[(crc ^ buf[i]) & 0xff];
  return (crc ^ -1) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function encodePng(rgba, size) {
  // Each scanline is prefixed with filter type 0 (None).
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    Buffer.from(rgba.buffer, y * size * 4, size * 4).copy(raw, y * (size * 4 + 1) + 1);
  }

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;  // bit depth
  ihdr[9] = 6;  // colour type: RGBA
  ihdr[10] = 0; // deflate
  ihdr[11] = 0; // adaptive filtering
  ihdr[12] = 0; // no interlace

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const out = process.argv[2] || "src-tauri/icons/source.png";
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, encodePng(pixels, SIZE));
console.log(`wrote ${out} (${SIZE}×${SIZE})`);
