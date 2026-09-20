'use strict';
// 生成 Tauri 实际加载的前端目录 web/：src/ 原样复制 + core/ 的 CommonJS 包装 + 注入 IPC 桥接
// core/*.js 是 CommonJS（node 里 npm test 直接 require），浏览器不能直接加载，所以包装一层
// 改完 src/ 或 bridge/ 后必须重跑本脚本，否则 Tauri 里跑的还是旧界面
// 用法：node tools/sync-web.js

const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
const WEB = path.join(ROOT, 'web');
const CORE = ['align', 'split', 'importer'];   // 按依赖顺序：importer 需要前两个

function copy(src, dest) {
  if (fs.statSync(src).isDirectory()) {
    fs.mkdirSync(dest, { recursive: true });
    for (const f of fs.readdirSync(src)) copy(path.join(src, f), path.join(dest, f));
  } else {
    fs.copyFileSync(src, dest);
  }
}

function mkdir(p) { fs.mkdirSync(p, { recursive: true }); }

// 页面级 meta CSP 会让 Tauri 注入的 IPC 脚本被挡，CSP 改由 tauri.conf.json 下发
function stripCspMeta(html) {
  return html.replace(/\s*<meta http-equiv="Content-Security-Policy"[^>]*>/i, '');
}

// 把 CommonJS 文件包成能直接 <script> 加载的形式，require('./x') 走 window.__core
function wrapCore(name) {
  const src = fs.readFileSync(path.join(ROOT, 'core', name + '.js'), 'utf8');
  return `(function () {
  window.__core = window.__core || {};
  var module = { exports: {} };
  var exports = module.exports;
  function require(id) { return window.__core[String(id).replace(/^\\.\\//, '')]; }
${src}
  window.__core[${JSON.stringify(name)}] = module.exports;
})();
`;
}

const PAGES = {
  'manager/index.html': ['../bridge/api.js', '../core/align.js', '../core/split.js', '../core/importer.js'],
  'widget/index.html': ['../bridge/api.js']
};

function injectScripts(rel, html, scripts) {
  const block = scripts.map(s => `<script src="${s}"></script>`).join('\n') + '\n';
  const m = html.match(/<script src="[^"]*\.js"><\/script>\s*<\/body>/);
  if (!m) throw new Error(`${rel}: 找不到注入点（页尾的 <script> + </body>）`);
  return html.replace(m[0], block + m[0]);
}

console.log('清理 web/…');
fs.rmSync(WEB, { recursive: true, force: true });
mkdir(WEB);

console.log('复制 src/ → web/…');
copy(path.join(ROOT, 'src'), WEB);

mkdir(path.join(WEB, 'core'));
for (const name of CORE) {
  fs.writeFileSync(path.join(WEB, 'core', name + '.js'), wrapCore(name));
}

mkdir(path.join(WEB, 'bridge'));
copy(path.join(ROOT, 'bridge'), path.join(WEB, 'bridge'));

for (const [rel, scripts] of Object.entries(PAGES)) {
  const file = path.join(WEB, rel);
  const html = injectScripts(rel, stripCspMeta(fs.readFileSync(file, 'utf8')), scripts);
  fs.writeFileSync(file, html);
}

console.log('完成 → web/');
