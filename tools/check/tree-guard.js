// Checks every tracked file against the kinds of file the repo holds and a size limit: the repo holds code, tools
// and docs, and a file of another kind or a large one is refused before it lands.
//   node tools/check/tree-guard.js
// Exit 0 when every file passes, 1 with one line per file that does not.
'use strict';
const { execFileSync } = require('child_process');
const fs = require('fs');

// File kinds by extension (lower case).
const KINDS = new Set([
  'rs', 'toml', 'lock', 'md', 'json', 'txt', 'js', 'ps1', 'bat', 'yml', 'yaml', 'rc', 'hlsl',
  // the Linux build script
  'sh',
  // the vendored FreeType sources
  'c', 'h', 'mk', 'cin', 'hin', 'dat',
  // the launcher's fonts and the app's icons
  'ttf', 'ico', 'png',
  // the README's screenshots
  'jpg',
  // the port's own shaders compiled for Linux, which has no D3DCompile
  'dxbc',
  // the Expert bot's trained network (our own weights) and the scripts that train it
  'net', 'py',
]);
// Files with no extension the repo holds, by name.
const NAMES = new Set(['LICENSE', 'NOTICE', 'README', 'Jamfile', 'commit-msg', '.gitignore', '.gitattributes']);
// Pictures only where the app and the README keep theirs.
const PICTURES = /^(docs\/images\/|crates\/[^/]+\/res\/)/;
const LIMIT = 1024 * 1024;
// Epic's EOS SDK runtimes, kept with Epic's permission (third_party/eos/README.md): the files past both rules.
const EOS_LIBS = new Set(['third_party/eos/EOSSDK-Win64-Shipping.dll', 'third_party/eos/libEOSSDK-Linux-Shipping.so']);

const files = execFileSync('git', ['ls-files', '-z'], { encoding: 'utf8' }).split('\0').filter(Boolean);
const bad = [];
for (const f of files) {
  if (EOS_LIBS.has(f)) continue;
  const base = f.split('/').pop();
  const dot = base.lastIndexOf('.');
  const ext = dot > 0 ? base.slice(dot + 1).toLowerCase() : '';
  if (!(KINDS.has(ext) || NAMES.has(base) || base.startsWith('LICENSE-'))) bad.push(`${f}: not a kind of file the repo holds`);
  else if ((ext === 'png' || ext === 'jpg') && !PICTURES.test(f)) bad.push(`${f}: a picture outside docs/images and the crates' res`);
  const size = fs.statSync(f).size;
  if (size > LIMIT) bad.push(`${f}: ${size} bytes, over the ${LIMIT} byte limit`);
}
for (const b of bad) console.log(b);
console.log(`${files.length} files, ${bad.length} refused`);
process.exit(bad.length ? 1 : 0);
