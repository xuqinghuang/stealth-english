'use strict';
// 生成应用图标（蓝色圆角方块 + 白色 E），纯 Node 实现 PNG 与 ICO
// 用法：npm run icon  →  写进 src-tauri/icons/（窗口图标、托盘图标、打包图标都从这里取）

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { spawnSync } = require('child_process');

// ---------- CRC32 ----------
const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = (c & 1) ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1);
    t[n] = c >>> 0;
  }
  return t;
})();
function crc32(buf) {
  let c = 0xFFFFFFFF;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xFF] ^ (c >>> 8);
  return (c ^ 0xFFFFFFFF) >>> 0;
}

// ---------- PNG ----------
// PNG 的 length 与 CRC 都是大端（网络字节序），写成 LE 的话解码器会拿到一个天文数字的长度，
// 表现为 "unexpected end of file"——这个坑踩过一次，别改回去
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

function encodePNG(width, height, rgba) {
  const sig = Buffer.from([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;   // bit depth
  ihdr[9] = 6;   // RGBA
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0; // filter none
    rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  return Buffer.concat([
    sig,
    chunk('IHDR', ihdr),
    chunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0))
  ]);
}

// ---------- 画图标：圆角渐变方块 + 块状 E ----------
function drawIcon(size) {
  const rgba = Buffer.alloc(size * size * 4);
  const r = size * 0.18;                       // 圆角半径
  const set = (x, y, cr, cg, cb, ca) => {
    const i = (y * size + x) * 4;
    rgba[i] = cr; rgba[i + 1] = cg; rgba[i + 2] = cb; rgba[i + 3] = ca;
  };
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      // 圆角判断
      let inside = true;
      const corners = [[r, r], [size - r, r], [r, size - r], [size - r, size - r]];
      for (const [cx, cy] of corners) {
        const dx = (x < cx === (cx < size / 2)) ? 0 : x - cx;
        const dy = (y < cy === (cy < size / 2)) ? 0 : y - cy;
        if (dx !== 0 && dy !== 0 && dx * dx + dy * dy > r * r) { inside = false; break; }
      }
      if (!inside) { set(x, y, 0, 0, 0, 0); continue; }
      // 对角渐变 #3b82f6 → #1d4ed8
      const t = (x + y) / (2 * size);
      const cr = Math.round(0x3b + (0x1d - 0x3b) * t);
      const cg = Math.round(0x82 + (0x4e - 0x82) * t);
      const cb = Math.round(0xf6 + (0xd8 - 0xf6) * t);
      // 块状 E
      const pad = size * 0.25, w = size * 0.085;
      const x0 = pad, x1 = pad + w;
      const top0 = size * 0.26, top1 = top0 + w;
      const mid0 = size * 0.465, mid1 = mid0 + w * 0.86;
      const bot0 = size * 0.67, bot1 = bot0 + w;
      const right = size - pad;
      let white = false;
      if (x >= x0 && x <= right && y >= top0 && y <= bot1) {
        if (x <= x1) white = true;                                  // 竖笔
        else if (y >= top0 && y <= top1) white = true;              // 上横
        else if (y >= mid0 && y <= mid1 && x <= right - w * 0.4) white = true;  // 中横（略短）
        else if (y >= bot0 && y <= bot1) white = true;              // 下横
      }
      if (white) set(x, y, 255, 255, 255, 255);
      else set(x, y, cr, cg, cb, 255);
    }
  }
  return rgba;
}

// ---------- 输出：只画一张大图，多尺寸和 ICO 交给 tauri icon ----------
// 自己拼 ICO 全是坑（字节序、256 要写成 0），官方 CLI 用 image crate 生成，tauri-build 一定认
const icons = path.join(__dirname, '..', 'src-tauri', 'icons');
fs.mkdirSync(icons, { recursive: true });

const source = path.join(icons, '_source.png');
fs.writeFileSync(source, encodePNG(1024, 1024, drawIcon(1024)));

const rel = (p) => path.relative(process.cwd(), p);
const r = spawnSync('tauri', ['icon', rel(source), '-o', rel(icons)], { stdio: 'inherit', shell: true });
if (r.status !== 0) process.exit(r.status || 1);

// tauri icon 一次生成全套：iOS 的 AppIcon-*、Android 的 android/ + ios/、Windows Store 的
// Square*Logo / StoreLogo、macOS 的 icon.icns。本项目是 Windows 桌面专用，只留真正被引用的三个：
//   tauri.conf.json 的 bundle.icon → 32x32.png / 128x128.png / icon.ico
//   lib.rs 的托盘图标（include_bytes!）→ 32x32.png
// 用白名单而不是黑名单，是因为黑名单漏一个就会长期留在仓库里：原来按 /^AppIcon-|^mipmap-/ 匹配，
// 但 CLI 把它们包在 android/ 与 ios/ 两个目录里，顶层正则永远匹配不到 —— 40 多个图标就这么躺了下来。
// 要新增尺寸（例如给 bundle.icon 加 128x128@2x.png），这里也必须同步加一行。
const KEEP = new Set(['32x32.png', '128x128.png', 'icon.ico']);
for (const f of fs.readdirSync(icons)) {
  if (!KEEP.has(f)) fs.rmSync(path.join(icons, f), { recursive: true, force: true });
}
// 源图 _source.png 不在 KEEP 里，上面这一轮已经一并清掉了
console.log('图标已生成 → src-tauri/icons/（只保留 32x32.png / 128x128.png / icon.ico）');
