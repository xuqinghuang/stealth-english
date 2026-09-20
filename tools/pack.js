'use strict';
// 便携版打包：免安装文件夹，双击 exe 就能跑（不是安装包）
// 用法：node tools/pack.js          （完整：生成前端 → cargo build --release → 组装）
//       node tools/pack.js --skip-build   （只重新组装，沿用已编好的 exe）

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const ROOT = path.join(__dirname, '..');
const RELEASE = path.join(ROOT, 'src-tauri', 'target', 'release');
const EXE = path.join(RELEASE, 'stealth-english.exe');
const OUT = path.join(ROOT, 'dist', 'stealth-english');
const NAME = 'stealth-english.exe';

function rm(p) { fs.rmSync(p, { recursive: true, force: true }); }

function run(cmd, args, cwd) {
  console.log(`> ${cmd} ${args.join(' ')}   (cwd: ${path.relative(ROOT, cwd) || '.'})`);
  const r = spawnSync(cmd, args, { cwd, stdio: 'inherit', shell: true });
  if (r.status !== 0) process.exit(r.status || 1);
}

const skipBuild = process.argv.includes('--skip-build');

if (!skipBuild) {
  run('node', [path.join('tools', 'sync-web.js')], ROOT);
  run('cargo', ['build', '--release'], path.join(ROOT, 'src-tauri'));
}

if (!fs.existsSync(EXE)) {
  console.error(`找不到 ${path.relative(ROOT, EXE)}，请先不带 --skip-build 跑一次`);
  process.exit(1);
}

console.log('清理旧输出…');
rm(OUT);
fs.mkdirSync(OUT, { recursive: true });

console.log('复制运行库…');
// Tauri 的 exe 正常是自包含的；万一 cargo 旁落出了 dll，一起带上，免得在别人机器上闪退。
// 但要排除 app_lib.dll —— 那是 Cargo.toml 里 [lib] crate-type 中 cdylib 的产物（给移动端用的），
// 和 exe 是各自独立编译出来的，PE 导入表里 exe 并不引用它。带进包只会让人以为还缺别的 dll。
for (const f of fs.readdirSync(RELEASE)) {
  const low = f.toLowerCase();
  if (low.endsWith('.dll') && low !== 'app_lib.dll') fs.copyFileSync(path.join(RELEASE, f), path.join(OUT, f));
}

fs.copyFileSync(EXE, path.join(OUT, NAME));

const mb = (n) => (n / 1024 / 1024).toFixed(1) + ' MB';
const size = fs.statSync(path.join(OUT, NAME)).size;
let total = 0;
for (const f of fs.readdirSync(OUT)) total += fs.statSync(path.join(OUT, f)).size;

console.log(`完成 → dist/stealth-english/`);
console.log(`  ${NAME}  ${mb(size)}   整个文件夹 ${mb(total)}`);
console.log('  双击 exe 即可运行；数据写在旁边的 data/ 里（首次启动自动生成内置示例书）');
