// Draws the launcher's icon, `crates/mvs-app/res/launcher.ico`, and its large master, `res/launcher.png`: MVS over
// CLASSIC in the launcher's wordmark (sky.rs `wordmark`: Barlow Black Italic in its gold with the brown ledge, the red
// CLASSIC tag, here straight and as wide as MVS) on a rounded tile of the launcher's night sky, with its haze, a few
// stars, a top sheen and a thin light edge. At 24 px and below the tile holds MVS alone, larger.
//
//   node tools/dev/launcher-icon.js            then open the printed address in a browser
//
// The drawing runs in the browser's canvas (its text and gradients); the page sends each size back, this script
// writes the .ico (PNG images) and the master, and ends. build.rs embeds the .ico in mvs-launcher.exe.
'use strict';
const http = require('http');
const fs = require('fs');
const path = require('path');

const repo = path.resolve(__dirname, '../..');
const fonts = path.join(repo, 'crates/mvs-ui/launcher/fonts');
const res = path.join(repo, 'crates/mvs-app/res');
const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256];
const MASTER = 1024;
const PORT = Number(process.argv[2]) || 8934;

const b64 = f => fs.readFileSync(path.join(fonts, f)).toString('base64');

// The drawing, on a 256-unit square scaled to `n` pixels.
const draw = String.raw`
function rr(g, x, y, w, h, r) { g.beginPath(); g.roundRect(x, y, w, h, r); }
function grad(g, y0, y1, stops) { const l = g.createLinearGradient(0, y0, 0, y1); stops.forEach(([o, c]) => l.addColorStop(o, c)); return l; }
function draw(n) {
  const c = document.createElement('canvas'); c.width = c.height = n;
  const g = c.getContext('2d'); g.scale(n / 256, n / 256);
  const small = n <= 24;
  // the tile: the launcher's sky (sky.rs), its haze and stars, a sheen over the top
  g.save(); rr(g, 6, 6, 244, 244, 46); g.clip();
  g.fillStyle = grad(g, 6, 250, [[0, '#071240'], [0.4, '#081a55'], [0.78, '#12406f'], [1, '#1b5470']]); g.fillRect(0, 0, 256, 256);
  const haze = (x, y, r, col) => { const h = g.createRadialGradient(x, y, 0, x, y, r); h.addColorStop(0, col); h.addColorStop(1, 'rgba(0,0,0,0)'); g.fillStyle = h; g.fillRect(0, 0, 256, 256); };
  haze(40, 230, 150, 'rgba(110,97,165,0.55)');
  haze(230, 40, 140, 'rgba(38,90,121,0.7)');
  if (!small) {
    for (const [x, y, r, a] of [[36, 40, 2.2, 0.9], [62, 26, 1.4, 0.6], [214, 214, 1.6, 0.6], [222, 30, 1.2, 0.5], [30, 120, 1.2, 0.45], [228, 128, 1.6, 0.7], [190, 22, 1.1, 0.5]]) {
      const s = g.createRadialGradient(x, y, 0, x, y, r * 3); s.addColorStop(0, 'rgba(223,232,255,' + a + ')'); s.addColorStop(1, 'rgba(190,210,255,0)');
      g.fillStyle = s; g.fillRect(x - r * 3, y - r * 3, r * 6, r * 6);
    }
  }
  g.fillStyle = grad(g, 6, 110, [[0, 'rgba(255,255,255,0.10)'], [1, 'rgba(255,255,255,0)']]); g.fillRect(0, 0, 256, 110);
  g.restore();
  rr(g, 6.5, 6.5, 243, 243, 46); g.lineWidth = 1.5; g.strokeStyle = 'rgba(150,190,240,0.28)'; g.stroke();

  // MVS: the wordmark's shadow, brown ledge and gold, and a fine light along the letters' tops
  const size = small ? 150 : 124, base = small ? 182 : 140, len = small ? 214 : 196;
  g.font = size + 'px MvsWord';
  const m = g.measureText('MVS'); const sx = len / m.width;
  const top = -m.actualBoundingBoxAscent, bot = m.actualBoundingBoxDescent;
  const word = (dy, fill, blur) => {
    g.save(); g.translate(128 - len / 2, base + dy); g.scale(sx, 1);
    if (blur) { g.shadowColor = 'rgba(3,10,40,0.6)'; g.shadowBlur = blur * n / 256; g.shadowOffsetY = 8 * n / 256; }
    g.fillStyle = fill; g.fillText('MVS', 0, 0); g.restore();
  };
  word(0, 'rgba(3,10,40,0.01)', 18);
  word(size * 0.035, '#964716');
  word(0, grad(g, top, bot, [[0.06, '#ffeca8'], [0.40, '#f9c850'], [0.70, '#f0a030'], [0.96, '#e27f2c']]));
  g.save(); g.translate(128 - len / 2, base); g.scale(sx, 1); g.beginPath(); g.rect(0, top, m.width, (bot - top) * 0.18); g.clip();
  g.fillStyle = 'rgba(255,255,255,0.35)'; g.fillText('MVS', 0, 0); g.restore();
  if (small) return c;

  // CLASSIC: the launcher's red tag (its gradient, dark ledge, shadow and top light), straight and as wide as MVS
  const px = 40, sp = 0.14 * px, w = len, h = px * 1.22;
  g.font = px + 'px MvsTag'; g.letterSpacing = sp + 'px';
  g.save(); g.translate(128, 186);
  g.shadowColor = 'rgba(3,10,40,0.45)'; g.shadowBlur = 20 * n / 256; g.shadowOffsetY = 10 * n / 256;
  g.fillStyle = '#6c0f1c'; rr(g, -w / 2, -h / 2 + 3, w, h, 3); g.fill();
  g.shadowColor = 'transparent';
  g.fillStyle = grad(g, -h / 2, h / 2, [[0, '#e64f5b'], [0.55, '#c92a3b'], [1, '#a91e31']]); rr(g, -w / 2, -h / 2, w, h, 3); g.fill();
  g.fillStyle = 'rgba(255,255,255,0.4)'; g.fillRect(-w / 2 + 2, -h / 2, w - 4, 1);
  g.fillStyle = '#fff'; g.textBaseline = 'middle'; g.textAlign = 'center';
  g.fillText('CLASSIC', sp / 2, 1.5);
  g.restore();
  return c;
}`;

const page = `<!doctype html><html><head><meta charset="utf-8"><title>launcher icon</title>
<style>@font-face { font-family: MvsWord; src: url('data:font/ttf;base64,${b64('Barlow-BlackItalic.ttf')}'); }
@font-face { font-family: MvsTag; src: url('data:font/ttf;base64,${b64('BarlowCondensed-ExtraBoldItalic.ttf')}'); }
body { background:#888; font:14px sans-serif; } canvas { margin:4px; vertical-align:bottom; }</style></head>
<body><p id="state">drawing...</p><script>${draw}
(async () => {
  await Promise.all([document.fonts.load('124px MvsWord'), document.fonts.load('40px MvsTag')]);
  for (const n of ${JSON.stringify([...ICO_SIZES, MASTER])}) {
    const c = draw(n); if (n <= 256) document.body.append(c);
    const blob = await new Promise(r => c.toBlob(r, 'image/png'));
    await fetch('/size/' + n, { method: 'POST', body: blob });
  }
  await fetch('/done', { method: 'POST' });
  document.getElementById('state').textContent = 'written: res/launcher.ico and res/launcher.png';
  document.title = 'done';
})();
</script></body></html>`;

// An .ico of PNG images, smallest first (a 256 size is written as 0 in the directory).
function ico(pngs) {
  const head = Buffer.alloc(6 + 16 * pngs.length);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(pngs.length, 4);
  let at = head.length;
  pngs.forEach(([n, png], i) => {
    const e = 6 + 16 * i;
    head[e] = head[e + 1] = n >= 256 ? 0 : n;
    head.writeUInt16LE(1, e + 4);
    head.writeUInt16LE(32, e + 6);
    head.writeUInt32LE(png.length, e + 8);
    head.writeUInt32LE(at, e + 12);
    at += png.length;
  });
  return Buffer.concat([head, ...pngs.map(p => p[1])]);
}

const got = new Map();
const server = http.createServer((req, out) => {
  const size = /^\/size\/(\d+)$/.exec(req.url);
  if (req.method === 'POST' && size) {
    const parts = [];
    req.on('data', d => parts.push(d));
    req.on('end', () => { got.set(Number(size[1]), Buffer.concat(parts)); out.end('ok'); });
    return;
  }
  if (req.method === 'POST' && req.url === '/done') {
    const missing = [...ICO_SIZES, MASTER].filter(n => !got.has(n));
    if (missing.length) throw new Error(`no image for ${missing.join(', ')}`);
    fs.writeFileSync(path.join(res, 'launcher.ico'), ico(ICO_SIZES.map(n => [n, got.get(n)])));
    fs.writeFileSync(path.join(res, 'launcher.png'), got.get(MASTER));
    console.log('written: crates/mvs-app/res/launcher.ico and launcher.png');
    out.end('ok', () => server.close());
    return;
  }
  out.setHeader('content-type', 'text/html');
  out.end(page);
});
server.listen(PORT, '127.0.0.1', () => console.log(`open http://127.0.0.1:${PORT}/ in a browser`));
