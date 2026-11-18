#!/usr/bin/env node
// Compares the pictures of two folders (two runs of the exe's pictures, or of the oracle's `npm run shots`), file by
// file, sub-folders included, or two PNG files (an exe picture against the oracle's).
//
//   node tools/check/png-diff.js <dirA | a.png> <dirB | b.png> [--mask x,y,w,h ...] [--all]
//
// A pixel counts when one of its channels is off by more than 1 (a rounding step of the page's last pass is not a
// difference). --mask leaves a rectangle out, as often as it is given: the page's status line with its frame rate
// is `--mask 600,0,680,24` at 1280x720. One line per picture that differs, is another size or is in one folder
// only (--all: every picture), with its counted pixels and the mean |RGB| difference / 255 over the unmasked pixels
// (the unit of the light trial's numbers), then the total line.
// For two runs to be comparable at all, the page's Math.random must be the same sequence: `npm run shots -- --seed 1`.
// Exit code: 0 no counted pixel and the same files, 1 otherwise.
// (its own PNG reader, node's zlib only: RGBA 8 bits a channel out, of every colour type and bit depth, tRNS
// included, not interlaced)
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

function readPng(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47 || buf.readUInt32BE(4) !== 0x0d0a1a0a) throw new Error('not a PNG');
  let width = 0, height = 0, depth = 0, type = 0, palette = null, trns = null;
  const idat = [];
  for (let o = 8; o < buf.length; ) {
    const len = buf.readUInt32BE(o), kind = buf.toString('latin1', o + 4, o + 8), body = buf.subarray(o + 8, o + 8 + len);
    if (kind === 'IHDR') {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      depth = body[8];
      type = body[9];
      if (body[12] !== 0) throw new Error('an interlaced PNG');
    } else if (kind === 'PLTE') palette = body;
    else if (kind === 'tRNS') trns = body;
    else if (kind === 'IDAT') idat.push(body);
    else if (kind === 'IEND') break;
    o += 12 + len;
  }
  const channels = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 }[type];
  if (!channels) throw new Error('PNG colour type ' + type);
  const bpp = Math.max(1, (channels * depth) >> 3);
  const stride = (width * channels * depth + 7) >> 3;
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const rows = Buffer.alloc(stride * height);
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < height; y++) {
    const f = raw[y * (stride + 1)], src = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)), row = rows.subarray(y * stride, (y + 1) * stride);
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? row[i - bpp] : 0, b = prev[i], c = i >= bpp ? prev[i - bpp] : 0;
      let v = src[i];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      } else if (f !== 0) throw new Error('PNG filter ' + f);
      row[i] = v & 255;
    }
    prev = row;
  }
  // sample n of a row: its value at the bit depth, then on 0..255
  const max = (1 << depth) - 1;
  const sample = (row, n) => {
    if (depth === 16) return row.readUInt16BE(n * 2);
    if (depth === 8) return row[n];
    const bit = n * depth;
    return (row[bit >> 3] >> (8 - depth - (bit & 7))) & max;
  };
  const to8 = (v) => (depth === 8 ? v : Math.round((v / max) * 255));
  const key = trns && type !== 3 ? [0, 1, 2].map((k) => (k * 2 < trns.length ? trns.readUInt16BE(k * 2) : -1)) : null;
  const data = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y++) {
    const row = rows.subarray(y * stride, (y + 1) * stride);
    for (let x = 0; x < width; x++) {
      const o = (y * width + x) * 4;
      let r, g, b, al = 255;
      if (type === 3) {
        const i = sample(row, x);
        [r, g, b] = [palette[i * 3], palette[i * 3 + 1], palette[i * 3 + 2]];
        if (trns && i < trns.length) al = trns[i];
      } else {
        const s = (k) => sample(row, x * channels + k);
        if (type === 0 || type === 4) {
          const v = s(0);
          if (key && v === key[0]) al = 0;
          r = g = b = to8(v);
          if (type === 4) al = to8(s(1));
        } else {
          const v = [s(0), s(1), s(2)];
          if (key && v[0] === key[0] && v[1] === key[1] && v[2] === key[2]) al = 0;
          [r, g, b] = v.map(to8);
          if (type === 6) al = to8(s(3));
        }
      }
      data[o] = r;
      data[o + 1] = g;
      data[o + 2] = b;
      data[o + 3] = al;
    }
  }
  return { width, height, data };
}

const args = process.argv.slice(2);
const dirs = [];
const masks = [];
let all = false;
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--mask') {
    const m = (args[++i] ?? '').split(',').map(Number);
    if (m.length !== 4 || m.some((v) => !Number.isFinite(v))) throw new Error('--mask takes x,y,w,h');
    masks.push(m);
  } else if (args[i] === '--all') all = true;
  else if (args[i].startsWith('--')) throw new Error('unknown option ' + args[i]);
  else dirs.push(args[i]);
}
if (dirs.length !== 2) {
  console.error('usage: node tools/check/png-diff.js <dirA | a.png> <dirB | b.png> [--mask x,y,w,h ...] [--all]');
  process.exit(1);
}

const list = (dir, at = '') =>
  fs.readdirSync(path.join(dir, at), { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? list(dir, at + e.name + '/') : e.name.toLowerCase().endsWith('.png') ? [at + e.name] : []));
const files = dirs.every((d) => fs.statSync(d).isFile());
const at = (d, name) => (files ? d : path.join(d, name));
const [a, b] = files ? [new Set(['picture']), new Set(['picture'])] : dirs.map((d) => new Set(list(d)));
const names = [...new Set([...a, ...b])].sort();
const masked = (x, y) => masks.some(([mx, my, mw, mh]) => x >= mx && x < mx + mw && y >= my && y < my + mh);

let same = 0, differ = 0, lone = 0, pixels = 0;
for (const name of names) {
  if (!a.has(name) || !b.has(name)) {
    lone++;
    console.log(`${name}: only in ${a.has(name) ? dirs[0] : dirs[1]}`);
    continue;
  }
  const [p, q] = dirs.map((d) => readPng(fs.readFileSync(at(d, name))));
  if (p.width !== q.width || p.height !== q.height) {
    differ++;
    console.log(`${name}: ${p.width}x${p.height} and ${q.width}x${q.height}`);
    continue;
  }
  let n = 0, sum = 0, kept = 0;
  for (let i = 0; i < p.data.length; i += 4) {
    const at = i / 4;
    if (masks.length && masked(at % p.width, (at / p.width) | 0)) continue;
    const dr = Math.abs(p.data[i] - q.data[i]), dg = Math.abs(p.data[i + 1] - q.data[i + 1]), db = Math.abs(p.data[i + 2] - q.data[i + 2]);
    kept++;
    sum += (dr + dg + db) / 3;
    if (dr <= 1 && dg <= 1 && db <= 1 && Math.abs(p.data[i + 3] - q.data[i + 3]) <= 1) continue;
    n++;
  }
  pixels += n;
  if (n) differ++;
  else same++;
  if (n || all) console.log(`${name}: ${n} pixels, mean ${(kept ? sum / kept : 0).toFixed(3)} / 255`);
}
console.log(`total: ${names.length} pictures, ${same} the same, ${differ} differ (${pixels} pixels)` + (lone ? `, ${lone} in one folder only` : ''));
process.exit(differ || lone ? 1 : 0);
