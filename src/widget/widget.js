'use strict';
// 悬浮窗逻辑：渲染主进程推送的状态，本地 TTS，自动播放

const $ = id => document.getElementById(id);
const widget = $('widget');
const synth = window.speechSynthesis;

let S = null;                 // 最近一次主进程推送
let playing = false;          // “播放中”模式（决定切句后是否自动朗读）
let apTimer = null;
let voices = [];

function loadVoices() { try { voices = synth.getVoices(); } catch (e) {} }
loadVoices();
if (synth) synth.onvoiceschanged = loadVoices;

// 兜底不提供音色选择：取第一个 lang 匹配的系统语音，机器上没有就交给浏览器默认
function pickVoice(langPrefix) {
  return voices.find(v => v.lang && v.lang.toLowerCase().startsWith(langPrefix)) || null;
}

// 正文颜色由用户选，桌面可能是白的也可能是黑的，所以按亮度自动配一圈对比描边
function applyFontColor(input) {
  const hex = /^#?[0-9a-f]{6}$/i.test(String(input || '').trim()) ? String(input).trim().replace('#', '') : 'ffffff';
  const rgb = [0, 2, 4].map(i => parseInt(hex.slice(i, i + 2), 16));
  const lum = (rgb[0] * 0.299 + rgb[1] * 0.587 + rgb[2] * 0.114) / 255;
  const halo = lum > 0.55
    ? '0 0 3px rgba(0,0,0,.92), 0 1px 2px rgba(0,0,0,.8)'
    : '0 0 3px rgba(255,255,255,.92), 0 1px 2px rgba(255,255,255,.8)';
  widget.style.setProperty('--wcolor', '#' + hex);
  widget.style.setProperty('--wcolor-2', `rgba(${rgb.join(',')},.68)`);
  widget.style.setProperty('--whalo', halo);
}

// ---------- 渲染 ----------
function render() {
  if (!S) return;
  const c = S.config;
  widget.className = 'mode-' + c.mode + ' cn-' + c.cnMode;
  widget.style.opacity = c.opacity;
  widget.style.fontSize = c.font + 'px';
  applyFontColor(c.fontColor);
  if ($('appTitle').textContent !== c.widgetTitle) $('appTitle').textContent = c.widgetTitle;

  if (S.card) {
    // 只在换句时重建句子 DOM：既避免词卡留在上一句的词上，也保住已点单词的高亮，
    // 顺带省掉「改个透明度就重建一遍所有 span」
    if (S.card.en !== lastEn) { lastEn = S.card.en; closeDict(); renderEn(S.card.en); }
    $('wCn').textContent = S.card.cn || '（这句没有中文翻译）';
    $('wProgress').textContent = (S.index + 1) + ' / ' + S.total;
    $('wBook').textContent = S.bookTitle || '';
    $('wEmpty').style.display = 'none';
    $('wEn').style.display = ''; $('wCn').style.display = '';
  } else {
    lastEn = null;
    closeDict();
    $('wEn').textContent = ''; $('wCn').textContent = '';
    $('wProgress').textContent = '';
    $('wBook').textContent = '';
    $('wEmpty').style.display = '';
  }
  $('btnCn').style.opacity = c.cnMode === 'always' ? 1 : (c.cnMode === 'hover' ? .6 : .35);
  scheduleFit();
}

// ---------- 尺寸：宽度按句子长度收缩（受上限约束），高度按内容收敛 ----------
const MIN_W = 220, MAX_W = 1400, MIN_H = 44, MAX_H = 900;
let lastW = 0, lastH = 0, fitRaf = 0, dragging = false;

function scheduleFit() {
  if (fitRaf) return;
  fitRaf = requestAnimationFrame(() => { fitRaf = 0; fitWindow(); });
}

function fitWindow() {
  if (!S || dragging) return;
  const c = S.config;
  if (c.widgetSizeMode === 'manual') {
    widget.style.width = '100%';
    lastW = lastH = 0;                       // 切回 auto 时保证重新报一次
    dictAnchorBottom = false;                // 手动尺寸不撑窗，锚点标记别留给下一次
    return;
  }
  // max-content 不受当前窗口宽度约束，量到的是「整句不换行需要的宽度」
  // 必须用 getBoundingClientRect：offsetWidth 会把 359.x 取整成 359，最后一个词就被挤到下一行
  widget.style.width = 'max-content';
  const want = Math.ceil(widget.getBoundingClientRect().width);
  const w = Math.max(MIN_W, Math.min(c.widgetMaxWidth || MAX_W, want));
  widget.style.width = w + 'px';             // 先按目标宽度排版，才能量到换行后的真实高度
  const h = Math.ceil(widget.getBoundingClientRect().height);
  // 精确比较：测量值由内容唯一决定，主进程遇到相同尺寸会直接返回，不存在来回抖动；
  // 用 >1 的阈值会吞掉 1px 的真实变化，窗口窄 1px 就够让最后一个词折行
  if (w !== lastW || h !== lastH) {
    lastW = w; lastH = h;
    const anchor = dictAnchorBottom ? 'bottom' : undefined;
    dictAnchorBottom = false;
    window.api.resizeWidget({ width: w, height: h, anchor });
  }
}

// 窗口宽度变了 → 换行数变了 → 高度得重量
window.addEventListener('resize', scheduleFit);

// ---------- 点词查义 ----------
// 句子里的每个单词都包成 .w-word，点一下联网查释义（结果缓存在 Rust 侧）。
// 词卡排在英文句**上方**、靠窗口向上生长显示出来 —— 窗口画不到自己外面，
// 做成绝对定位的浮层会被直接裁掉，所以这里不做浮层。
// 「向上」不是自动的：窗口被拖过之后默认左上角固定、向下长，卡片会把原文顶下去。
// 所以卡片开合引起的这次 resize 要报 anchor='bottom'（底边不动），原文才一动不动。
let lastEn = null;            // 上一句英文，用来判断换句时要不要收起词卡
let dictKey = null;           // 词卡当前显示的词（小写）；再点一次同词就收起
let dictSeq = 0;              // 连点多个词时只认最后一次结果，免得先返回的把后点的覆盖掉
let dictAnchorBottom = false; // 本次尺寸变化保持底边不动（只在卡片开合时置位，用完即清）

function esc(s) {
  return String(s == null ? '' : s).replace(/[&<>"]/g,
    c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
}

// 卡片顶部：单词 + 音标（可选）+ 右侧的朗读 / 关闭。
// 关闭按钮常驻，「查询中…」和失败态也带着 —— 否则卡片一弹出来想收都收不掉。
const headHTML = (word, rest) =>
  `<div class="wd-head"><span class="wd-word">${esc(word)}</span>${rest || ''}`
  + `<span class="wd-tools"><button data-act="say" title="朗读">🔊</button>`
  + `<button data-act="close" title="关闭">✕</button></span></div>`;

/// 把整句切成「文本节点 + 可点单词」，标点与空格原样保留
function renderEn(text) {
  const el = $('wEn');
  el.textContent = '';
  const re = /[A-Za-z][A-Za-z'’\-]*/g;
  let last = 0, m;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) el.appendChild(document.createTextNode(text.slice(last, m.index)));
    const span = document.createElement('span');
    span.className = 'w-word';
    span.dataset.w = m[0];
    span.textContent = m[0];
    el.appendChild(span);
    last = m.index + m[0].length;
  }
  if (last < text.length) el.appendChild(document.createTextNode(text.slice(last)));
}

function closeDict() {
  dictKey = null;
  dictSeq++;                                    // 作废在途请求，防止它回来又把卡打开
  const card = $('wDict');
  if (card) {
    // 只有卡片真在显示时才置锚点：本来就关着还置位，会让下一次「换句」也按底边锚定，
    // 拖过窗口的情况下那句原文会顺着往上跑
    if (card.style.display !== 'none') dictAnchorBottom = true;
    card.style.display = 'none';
    card.textContent = '';
  }
  document.querySelectorAll('.w-word.w-hit').forEach(n => n.classList.remove('w-hit'));
  scheduleFit();
}

function paintDict(inner) {
  const card = $('wDict');
  // 宽度上限跟着用户的设置走，否则注解一长就把窗口撑过他设的上限
  card.style.maxWidth = Math.min(420, (S && S.config.widgetMaxWidth) || 520) + 'px';
  card.innerHTML = inner;
  card.style.display = '';
  dictAnchorBottom = true;   // 开卡、以及「查询中…」换成结果，都按底边锚定
  scheduleFit();
}

function dictCardHTML(e) {
  const phon = [];
  if (e.uk) phon.push('英 /' + e.uk + '/');
  if (e.us && e.us !== e.uk) phon.push('美 /' + e.us + '/');
  const trs = (e.trs || []).map(t =>
    `<div class="wd-tr"><span class="wd-pos">${esc(t.pos)}</span>`
    + `<span class="wd-means">${(t.means || []).map(esc).join('；')}</span></div>`).join('');
  return headHTML(e.w || '', phon.length ? `<span class="wd-phon">${esc(phon.join('  '))}</span>` : '')
    + (e.proto && e.proto !== e.w ? `<div class="wd-proto">原形 ${esc(e.proto)}</div>` : '')
    + trs;
}

async function lookup(word, span) {
  const key = String(word).toLowerCase();
  if (dictKey === key) { closeDict(); return; }   // 点同一个词 = 收起

  document.querySelectorAll('.w-word.w-hit').forEach(n => n.classList.remove('w-hit'));
  span.classList.add('w-hit');
  dictKey = key;
  const seq = ++dictSeq;
  paintDict(headHTML(word, '<span class="wd-load">查询中…</span>'));

  try {
    const r = await window.api.dictLookup(word);
    if (seq !== dictSeq) return;
    paintDict(dictCardHTML(r.entry));
  } catch (err) {
    if (seq !== dictSeq) return;
    // 只有失败才把原因摆出来；成功什么都不提示
    paintDict(headHTML(word,
      `<span class="wd-fail"><span>${esc(String(err || '查不到释义'))}</span>`
      + `<button data-act="retry">重试</button><button data-act="copy">复制单词</button></span>`));
  }
}

$('wEn').addEventListener('click', (e) => {
  const span = e.target.closest('.w-word');
  if (!span) { if (dictKey) closeDict(); return; }
  lookup(span.dataset.w, span);
});

$('wDict').addEventListener('click', (e) => {
  const btn = e.target.closest('button[data-act]');
  if (!btn || !dictKey) return;
  const act = btn.dataset.act;
  if (act === 'close') {
    closeDict();
  } else if (act === 'retry') {
    const span = document.querySelector('.w-word.w-hit');
    if (span) { dictKey = null; lookup(span.dataset.w, span); }
  } else if (act === 'copy') {
    // 剪贴板在自定义协议下可能被拒，写不进去就算了，不打断阅读
    try { navigator.clipboard.writeText(dictKey); } catch (err) {}
  } else if (act === 'say') {
    speak(dictKey, 'en');
  }
});

// ---------- 朗读 ----------
// 神经语音走 <audio>（Rust 合成好给回 base64），系统语音走 speechSynthesis，两者互斥
let audioEl = null;

function stopAudio() {
  if (!audioEl) return;
  try { audioEl.pause(); } catch (e) {}
  audioEl.onended = audioEl.onerror = null;
  audioEl = null;
}

// 音频用 Blob URL 而不是 data: URI：后者要整段 base64 塞进 DOM 属性，长句会很卡
// 播放失败（解码失败 / 被策略拦下）会走 onfail，由调用方回退到系统语音
function playAudio(b64, onend, onfail) {
  stopAudio();
  let url = null;
  try {
    const bin = atob(b64);
    const buf = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i);
    url = URL.createObjectURL(new Blob([buf], { type: 'audio/mpeg' }));
  } catch (e) {
    onfail && onfail('音频解码失败');
    return;
  }
  const el = new Audio(url);
  audioEl = el;
  const cleanup = () => { if (url) { URL.revokeObjectURL(url); url = null; } };
  const done = () => { cleanup(); if (audioEl === el) audioEl = null; onend && onend(); };
  const fail = () => { cleanup(); if (audioEl === el) audioEl = null; onfail && onfail(); };
  el.onended = done;
  el.onerror = fail;
  el.play().catch(fail);
}

function stopAll() {
  playing = false;
  clearTimeout(apTimer); apTimer = null;
  try { synth && synth.cancel(); } catch (e) {}
  stopAudio();
  syncPlayIcon();
}

// 系统语音（神经语音不可用时的兜底）
function legacySpeak(text, langPrefix, onend) {
  if (!synth) { onend && onend(); return; }
  const u = new SpeechSynthesisUtterance(text);
  u.lang = langPrefix === 'en' ? 'en-US' : 'zh-CN';
  u.rate = (S && S.config.tts.rate) || 0.95;
  const v = pickVoice(langPrefix);
  if (v) u.voice = v;
  u.onend = () => onend && onend();
  u.onerror = () => onend && onend();
  synth.speak(u);
}

// 神经语音：合成失败（没网 / 接口变了）静默回退到系统语音，不打断自动播放链
function speak(text, langPrefix, onend) {
  const cfg = (S && S.config.tts) || {};
  const preset = langPrefix === 'en' ? cfg.voice : 'zh';
  if (cfg.engine !== 'edge' || !preset) { legacySpeak(text, langPrefix, onend); return; }
  window.api.ttsSpeak({ text, preset, rate: cfg.rate || 0.95 })
    .then(r => {
      if (!r || !r.audio) { legacySpeak(text, langPrefix, onend); return; }
      // 合成成功但播不出来（解码/自动播放策略）时同样回退，不能直接吞掉静音
      playAudio(r.audio, onend, () => legacySpeak(text, langPrefix, onend));
    })
    .catch(() => legacySpeak(text, langPrefix, onend));
}

function speakCurrent() {
  if (!S || !S.card) return;
  playing = true;
  syncPlayIcon();
  const cfg = S.config.tts;
  try { synth && synth.cancel(); } catch (e) {}
  stopAudio();
  speak(S.card.en, 'en', () => {
    if (!playing) return;
    if (cfg.readCn && S.card.cn) {
      speak(S.card.cn, 'zh', afterSpeech);
    } else afterSpeech();
  });
}

function afterSpeech() {
  if (!playing || !S) return;
  if (S.config.loop) {
    apTimer = setTimeout(() => { if (playing) speakCurrent(); }, S.config.autoplay.interval * 1000);
  } else if (S.config.autoplay.on) {
    apTimer = setTimeout(() => { if (playing) window.api.setIndex({ delta: 1 }); }, S.config.autoplay.interval * 1000);
  } else {
    playing = false; syncPlayIcon();
  }
}

function togglePlay() {
  if (playing) stopAll();
  else speakCurrent();
}

function syncPlayIcon() { $('btnPlay').textContent = playing ? '⏸' : '▶'; }

// ---------- 状态 / 命令 ----------
window.api.onState((s) => {
  const indexChanged = S && (s.index !== S.index || s.bookId !== (S.bookId ?? null));
  const hadCard = !!(S && S.card);
  S = s;
  render();
  // 主进程隐藏我们时先发了 stop 命令；这里只处理内容切换后的续播
  if (s.reason === 'stop') { stopAll(); return; }
  if (indexChanged && playing && hadCard && s.card) {
    clearTimeout(apTimer); apTimer = null;
    try { synth && synth.cancel(); } catch (e) {}
    stopAudio();
    speakCurrent();
  }
});

window.api.onCmd((cmd) => {
  if (cmd === 'playpause') { playing ? stopAll() : speakCurrent(); }
  else if (cmd === 'stop') stopAll();
});

// ---------- 按钮 ----------
$('btnPrev').addEventListener('click', () => { window.api.setIndex({ delta: -1 }); });
$('btnNext').addEventListener('click', () => { window.api.setIndex({ delta: 1 }); });
$('btnPlay').addEventListener('click', togglePlay);
$('btnHide').addEventListener('click', () => { stopAll(); window.api.widgetHide(); });
$('btnCn').addEventListener('click', () => {
  if (!S) return;
  const order = ['always', 'hover', 'off'];
  const next = order[(order.indexOf(S.config.cnMode) + 1) % 3];
  window.api.setConfig({ cnMode: next });
});

// 右下角手柄：拖出来的尺寸算「固定」，之后不再按句子伸缩
$('wGrip').addEventListener('mousedown', (e) => {
  e.preventDefault();
  e.stopPropagation();
  dragging = true;
  const sw = window.innerWidth, sh = window.innerHeight;
  const sx = e.screenX, sy = e.screenY;
  let size = { w: sw, h: sh };
  const onMove = (ev) => {
    size = {
      w: Math.round(Math.max(MIN_W, Math.min(MAX_W, sw + ev.screenX - sx))),
      h: Math.round(Math.max(MIN_H, Math.min(MAX_H, sh + ev.screenY - sy)))
    };
    window.api.resizeWidget({ width: size.w, height: size.h, anchor: 'topleft' });
  };
  const onUp = () => {
    window.removeEventListener('mousemove', onMove);
    window.removeEventListener('mouseup', onUp);
    dragging = false;
    window.api.setConfig({ widgetSizeMode: 'manual', widgetSize: size });
  };
  window.addEventListener('mousemove', onMove);
  window.addEventListener('mouseup', onUp);
});

// 伪装标题可编辑
$('appTitle').addEventListener('blur', () => {
  const t = $('appTitle').textContent.trim().slice(0, 30) || '工作提醒';
  $('appTitle').textContent = t;
  window.api.setConfig({ widgetTitle: t });
});
$('appTitle').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.preventDefault(); $('appTitle').blur(); }
});

// ---------- 本地快捷键（悬浮窗获得焦点时） ----------
document.addEventListener('keydown', (e) => {
  if (e.target.isContentEditable) return;
  // 带修饰键的组合交给全局快捷键：Alt+H 同时被这里和全局各 toggle 一次，两次相抵，按了像没反应
  if (e.altKey || e.ctrlKey || e.metaKey) return;
  if (e.key === 'Escape') { if (dictKey) closeDict(); return; }
  if (e.key === 'h' || e.key === 'H') { window.api.widgetToggle(); }
  else if (e.key === 'ArrowRight') { window.api.setIndex({ delta: 1 }); }
  else if (e.key === 'ArrowLeft') { window.api.setIndex({ delta: -1 }); }
  else if (e.key === ' ') { e.preventDefault(); togglePlay(); }
});

// ---------- 鼠标移开自动变淡 ----------
let fadeT = null;
widget.addEventListener('mouseenter', () => {
  clearTimeout(fadeT);
  if (S && S.config.autoFade) widget.style.opacity = S.config.opacity;
});
widget.addEventListener('mouseleave', () => {
  if (!S || !S.config.autoFade) return;
  clearTimeout(fadeT);
  fadeT = setTimeout(() => {
    widget.style.opacity = Math.max(0.12, S.config.opacity * 0.3);
  }, 2500);
});

// ---------- 时钟 ----------
function tick() {
  const d = new Date();
  $('wTime').textContent = ('0' + d.getHours()).slice(-2) + ':' + ('0' + d.getMinutes()).slice(-2);
}
setInterval(tick, 15000); tick();

// ---------- 启动 ----------
(async () => {
  S = await window.api.getState();
  render();
})();
