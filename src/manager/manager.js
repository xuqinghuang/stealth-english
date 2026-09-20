'use strict';
// 资料库窗口逻辑：资料库 / 导入 / 练习 / 设置

const $ = id => document.getElementById(id);
const synth = window.speechSynthesis;

let cfg = null;                       // 主进程配置（缓存）
let books = [];                       // 资料库列表
let cur = { id: null, cards: [], index: 0, title: '' };

const UNIT_LABEL = { sentence: '单句', smart: '智能合并', paragraph: '整段' };
const RULE_LABEL = { A: '逐段交替', B: '两段式', C: '行内对照', D: '仅英文' };

function errMsg(err) {
  return String((err && err.message) || err)
    .replace(/^Error invoking remote method '[^']+':\s*/i, '')
    .replace(/^Error:\s*/i, '');
}

let toastT = null;
function toast(msg) {
  const t = $('toast');
  t.textContent = msg;
  t.classList.add('show');
  clearTimeout(toastT);
  toastT = setTimeout(() => t.classList.remove('show'), 2400);
}

/* ================= 标签页 ================= */
function switchTab(name) {
  document.querySelectorAll('.m-tabs button').forEach(b => b.classList.toggle('active', b.dataset.tab === name));
  document.querySelectorAll('.m-body .tab').forEach(t => t.classList.toggle('active', t.id === 'tab-' + name));
  if (name === 'lib') refreshBooks();
  if (name === 'practice') loadPractice().then(() => syncCursor('start'));
}
document.querySelectorAll('.m-tabs button').forEach(b => b.addEventListener('click', () => switchTab(b.dataset.tab)));
$('libImport').addEventListener('click', () => switchTab('import'));

/* ================= 资料库 ================= */
async function refreshBooks() {
  books = await window.api.listBooks();
  renderBooks();
  renderPracticeBookSelect();
}

function renderBooks() {
  const grid = $('libGrid');
  grid.innerHTML = '';
  if (!books.length) {
    grid.innerHTML = '<div class="empty-tip">书库是空的 —— 点右上角「＋ 导入文章」，把你手头的中英对照 TXT 导进来吧</div>';
    return;
  }
  const colors = ['linear-gradient(135deg,#2563eb,#60a5fa)', 'linear-gradient(135deg,#059669,#34d399)', 'linear-gradient(135deg,#d97706,#fbbf24)', 'linear-gradient(135deg,#7c3aed,#a78bfa)', 'linear-gradient(135deg,#db2777,#f472b6)'];
  books.forEach((b, i) => {
    const pct = b.count ? Math.round(((b.progress?.index || 0) + 1) / b.count * 100) : 0;
    const card = document.createElement('div');
    card.className = 'bcard';
    card.innerHTML = `
      <div class="bc-cover" style="background:${colors[i % colors.length]}">${esc(b.title.slice(0, 2))}</div>
      <div class="bc-info">
        <div class="bc-title"><b title="${esc(b.title)}">${esc(b.title)}</b><button class="btn mini" data-op="rename">✎</button></div>
        <span>${UNIT_LABEL[b.unit] || '单句'} · ${b.count} 句 · 学到第 ${(b.progress?.index || 0) + 1} 句</span>
        <div class="bc-bar"><i style="width:${pct}%"></i></div>
        <div class="bc-ops">
          <button class="btn mini primary" data-op="open">▶ 练习</button>
          <button class="btn mini danger" data-op="del">删除</button>
        </div>
      </div>`;
    card.querySelector('[data-op=open]').addEventListener('click', async () => {
      await window.api.openBook(b.id);
      await loadPractice();
      switchTab('practice');
    });
    card.querySelector('[data-op=del]').addEventListener('click', async () => {
      if (!confirm(`确定删除《${b.title}》吗？（含全部句子和进度）`)) return;
      await window.api.deleteBook(b.id);
      toast('已删除');
      refreshBooks();
    });
    card.querySelector('[data-op=rename]').addEventListener('click', () => {
      const bEl = card.querySelector('.bc-title b');
      const input = document.createElement('input');
      input.type = 'text';
      input.value = b.title;
      input.style.cssText = 'flex:1;min-width:0';
      bEl.replaceWith(input);
      input.focus(); input.select();
      const done = async () => {
        const t = input.value.trim();
        if (t && t !== b.title) { await window.api.renameBook(b.id, t); b.title = t; }
        refreshBooks();
      };
      input.addEventListener('blur', done);
      input.addEventListener('keydown', e => { if (e.key === 'Enter') input.blur(); });
    });
    grid.appendChild(card);
  });
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

/* ================= 导入 ================= */
const imp = { text: '', name: '' };

$('pickTxt').addEventListener('click', async () => {
  try {
    const r = await window.api.readFile();
    if (r.canceled) return;
    imp.text = r.text; imp.name = r.name;
    $('fileName').textContent = '已读取：' + r.name + '（' + Math.round(r.text.length / 1024) + ' KB）';
    if (!$('bookTitle').value) $('bookTitle').value = r.name.replace(/\.(txt|md)$/i, '');
  } catch (err) { showImportError(errMsg(err)); }
});

function importParams() {
  const rule = document.querySelector('input[name=align]:checked').value;
  const unit = document.querySelector('input[name=unit]:checked').value;
  return { text: imp.text, rule, unit, title: $('bookTitle').value.trim() };
}

function showImportError(msg) {
  $('previewArea').innerHTML = `<div class="i-err">⚠ ${esc(msg)}</div>`;
}

$('btnPreview').addEventListener('click', async () => {
  const p = importParams();
  if (!p.text.trim()) { showImportError('请先选择要导入的文件。'); return; }
  try {
    const pv = await window.api.importPreview(p);
    const rules = Object.entries(pv.ruleCount).map(([k, n]) => `${RULE_LABEL[k] || k}×${n} 段`).join('，');
    const warns = pv.warnings.length
      ? '<p class="i-note i-warn">⚠ ' + pv.warnings.map(esc).join('<br>⚠ ') + '</p>'
      : '';
    const rows = pv.sample.map((c, i) => `
      <div class="i-row"><span class="i-no">${i + 1}</span><div>
        <p class="i-en">${esc(c.en)}</p>
        <p class="i-cn">${c.cn ? esc(c.cn) : '<i style="color:#5c6570">（无中文）</i>'}</p>
      </div></div>`).join('');
    $('previewArea').innerHTML = `
      <div class="i-preview">
        <div class="i-stats" style="padding:8px 2px 2px">
          <span>识别：<b>${rules || '—'}</b></span>
          <span>英文段 <b>${pv.stats.enParas}</b></span>
          <span>中文段 <b>${pv.stats.cnParas}</b></span>
          <span>共 <b>${pv.stats.cards}</b> 张练习卡</span>
        </div>
        <div class="i-list">${rows}${warns}</div>
        <p class="i-note">以上是前 ${pv.sample.length} 句预览，列表可滚动。</p>
      </div>
      <button class="btn primary" id="btnCommit" style="margin-top:10px">确认导入（${pv.stats.cards} 句）</button>`;
    $('btnCommit').scrollIntoView({ block: 'nearest' });
    $('btnCommit').addEventListener('click', async () => {
      try {
        const r = await window.api.importCommit(p);
        toast(`导入成功：《${p.title || '未命名'}》共 ${r.count} 句`);
        $('previewArea').innerHTML = '';
        imp.text = ''; imp.name = '';
        $('fileName').textContent = '';
        await refreshBooks();
        switchTab('lib');
      } catch (err) { showImportError(errMsg(err)); }
    });
  } catch (err) { showImportError(errMsg(err)); }
});

/* ================= 练习 ================= */
function renderPracticeBookSelect() {
  const sel = $('practiceBook');
  sel.innerHTML = books.map(b => `<option value="${b.id}">${esc(b.title)}</option>`).join('');
  if (cur.id) sel.value = cur.id;
}

async function loadPractice() {
  if (!books.length) { await refreshBooks(); }
  if (!cur.id && books.length) {
    await window.api.openBook(books[0].id);
    cur.id = books[0].id;
  }
  if (!cur.id) return;
  const bk = await window.api.bookCards(cur.id);
  if (!bk) return;
  cur.cards = bk.cards;
  cur.title = bk.meta.title;
  cur.index = Math.min(bk.meta.progress?.index || 0, Math.max(0, bk.cards.length - 1));
  setBookMeta(bk);
  syncChips();
  renderAtCursor();
}

function setBookMeta(bk) {
  $('pMeta').textContent = `· ${bk.meta.count} 句 · ${UNIT_LABEL[bk.meta.unit] || '单句'}`;
}

$('practiceBook').addEventListener('change', async (e) => {
  const id = e.target.value;
  const r = await window.api.openBook(id);
  cur.id = id;
  cur.index = r.index || 0;
  const bk = await window.api.bookCards(id);
  if (bk) {
    cur.cards = bk.cards; cur.title = bk.meta.title;
    setBookMeta(bk);
  }
  renderAtCursor();
});

function syncChips() {
  if (!cfg) return;
  $('pLoopChk').checked = !!cfg.loop;
  $('pShuffleChk').checked = !!cfg.shuffle;
  $('pRate').value = cfg.tts.rate;
  $('rateVal').textContent = cfg.tts.rate.toFixed(2) + 'x';
}

let rowEls = [];
let curRow = null;
let rowBase = 0;                  // 本页第一句在全书中的序号
let page = 0;

const pageSize = () => (cfg && cfg.practicePageSize) || 20;
const pageCount = () => Math.max(1, Math.ceil(cur.cards.length / pageSize()));
const rowAt = (i) => rowEls[i - rowBase];

function renderAtCursor() {
  page = Math.floor(cur.index / pageSize());
  renderPractice(true);
}

function renderPractice(cursorIntoView) {
  const list = $('pList');
  curRow = null;
  if (!cur.cards.length) {
    list.innerHTML = '<div class="x-empty">这本书还没有可练习的句子</div>';
    rowEls = []; rowBase = 0;
    syncProgress();
    syncPager();
    return;
  }
  page = Math.max(0, Math.min(page, pageCount() - 1));
  rowBase = page * pageSize();
  const end = Math.min(cur.cards.length, rowBase + pageSize());
  let html = '';
  for (let i = rowBase; i < end; i++) {
    const c = cur.cards[i];
    html += `<div class="x-row" data-i="${i}" title="点击从这里开始练">`
      + `<b class="x-no">${i + 1}</b><div>`
      + `<p class="x-en">${esc(c.en)}</p>`
      + (c.cn ? `<p class="x-cn">${esc(c.cn)}</p>` : '')
      + `</div></div>`;
  }
  list.innerHTML = html;
  rowEls = list.querySelectorAll('.x-row');
  syncCursor(cursorIntoView ? 'start' : 'top');
  syncPager();
}

function syncPager() {
  const pages = pageCount();
  $('pPageInfo').textContent = cur.cards.length
    ? `第 ${page + 1} / ${pages} 页 · 共 ${cur.cards.length} 句`
    : '';
  $('pPagePrev').disabled = page <= 0;
  $('pPageNext').disabled = page >= pages - 1;
  $('pPageSize').value = String(pageSize());
}

// 播放进度走到别页时，列表跟着翻过去
function followIndex() {
  const want = Math.floor(cur.index / pageSize());
  if (want !== page) { page = want; renderPractice(true); }
  else syncCursor('nearest');
}

function syncCursor(mode) {
  const list = $('pList');
  const row = rowAt(cur.index);
  if (curRow && curRow !== row) curRow.classList.remove('cur');
  curRow = row || null;
  if (curRow) curRow.classList.add('cur');
  if (mode === 'top') list.scrollTop = 0;
  else if (mode && curRow) curRow.scrollIntoView({ block: mode });
  syncProgress();
}

function syncProgress() {
  const total = cur.cards.length;
  $('pBar').style.width = total ? ((cur.index + 1) / total * 100) + '%' : '0%';
  $('pPos').textContent = total ? `第 ${cur.index + 1} 句 / 共 ${total} 句` : '';
}

window.api.onStateSync(async (s) => {
  let changed = false;
  if (s.bookId && s.bookId !== cur.id) {
    cur.id = s.bookId;
    const bk = await window.api.bookCards(s.bookId);
    if (bk) {
      cur.cards = bk.cards; cur.title = bk.meta.title;
      renderPracticeBookSelect();
      $('practiceBook').value = s.bookId;
      setBookMeta(bk);
      changed = true;
    }
  }
  if (s.total !== cur.cards.length && s.total > 0 && cur.id) {
    const bk = await window.api.bookCards(cur.id);
    if (bk) { cur.cards = bk.cards; changed = true; }
  }
  cur.index = s.index;
  if (!document.getElementById('tab-practice').classList.contains('active')) return;
  if (changed) renderAtCursor(); else followIndex();
});

$('pPrev').addEventListener('click', () => window.api.setIndex({ delta: -1 }));
$('pNext').addEventListener('click', () => window.api.setIndex({ delta: 1 }));
$('pPagePrev').addEventListener('click', () => { if (page > 0) { page--; renderPractice(); } });
$('pPageNext').addEventListener('click', () => { if (page < pageCount() - 1) { page++; renderPractice(); } });
$('pPageSize').addEventListener('change', e => {
  cfg.practicePageSize = +e.target.value;
  window.api.setConfig({ practicePageSize: cfg.practicePageSize });
  renderAtCursor();
});
// ---------- 朗读：神经语音优先，失败回退系统语音 ----------
let audioEl = null;

function stopAudio() {
  if (!audioEl) return;
  try { audioEl.pause(); } catch (e) {}
  audioEl.onended = audioEl.onerror = null;
  audioEl = null;
}

function playAudio(b64, onfail) {
  stopAudio();
  let url = null;
  try {
    const bin = atob(b64);
    const buf = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i);
    url = URL.createObjectURL(new Blob([buf], { type: 'audio/mpeg' }));
  } catch (e) { onfail && onfail('音频解码失败'); return; }
  const el = new Audio(url);
  audioEl = el;
  const cleanup = () => { if (url) { URL.revokeObjectURL(url); url = null; } };
  const done = () => { cleanup(); if (audioEl === el) audioEl = null; };
  const fail = () => { cleanup(); if (audioEl === el) audioEl = null; onfail && onfail('音频播放失败'); };
  el.onended = done;
  el.onerror = fail;
  el.play().catch(fail);
}

function legacySpeak(text) {
  if (!synth) return;
  stopAudio();
  try { synth.cancel(); } catch (e) {}
  const u = new SpeechSynthesisUtterance(text);
  u.lang = 'en-US';
  u.rate = cfg ? cfg.tts.rate : 0.95;
  synth.speak(u);
}

// 失败原因要让用户看见：静默回退到系统语音时，机器上没有英文语音 → 表现就是"点了没声音"
function ttsFail(msg) {
  console.error('[tts]', msg);
  toast('神经语音不可用，已用系统语音：' + msg);
}

function speakEn(text) {
  const t = (cfg && cfg.tts) || {};
  if (t.engine !== 'edge') { legacySpeak(text); return; }
  stopAudio();
  window.api.ttsSpeak({ text, preset: t.voice || 'woman-ava', rate: t.rate || 0.95 })
    .then(r => {
      if (!r || !r.audio) { legacySpeak(text); return; }
      playAudio(r.audio, (m) => { legacySpeak(text); ttsFail(m); });
    })
    .catch(err => { legacySpeak(text); ttsFail(errMsg(err)); });
}

$('pPlay').addEventListener('click', () => {
  const card = cur.cards[cur.index];
  if (card) speakEn(card.en);
});
$('pList').addEventListener('click', (e) => {
  const row = e.target.closest('.x-row');
  if (!row) return;
  if ($('pMaskChk').checked && e.target.classList.contains('x-en')) {
    row.classList.toggle('reveal');
    return;
  }
  window.api.setIndex({ index: +row.dataset.i });
});
$('pCnChk').addEventListener('change', e => { $('pList').classList.toggle('nocn', !e.target.checked); });
$('pMaskChk').addEventListener('change', e => {
  const list = $('pList');
  list.classList.toggle('mask', e.target.checked);
  list.querySelectorAll('.x-row.reveal').forEach(r => r.classList.remove('reveal'));
});
$('pLoopChk').addEventListener('change', e => { cfg.loop = e.target.checked; window.api.setConfig({ loop: cfg.loop }); });
$('pShuffleChk').addEventListener('change', e => { cfg.shuffle = e.target.checked; window.api.setConfig({ shuffle: cfg.shuffle }); });
$('pRate').addEventListener('input', e => {
  cfg.tts.rate = +e.target.value;
  $('rateVal').textContent = cfg.tts.rate.toFixed(2) + 'x';
  saveConfigDebounced();
});

/* ================= 设置 ================= */
function bindSettings() {
  $('sMode').value = cfg.mode;
  $('sOpacity').value = Math.round(cfg.opacity * 100);
  $('sOpacityVal').textContent = Math.round(cfg.opacity * 100) + '%';
  $('sFont').value = cfg.font;
  $('sFontVal').textContent = cfg.font + 'px';
  $('sFontColor').value = cfg.fontColor || '#ffffff';
  $('sFontColorVal').textContent = $('sFontColor').value;
  $('sMaxW').value = cfg.widgetMaxWidth;
  $('sMaxWVal').textContent = cfg.widgetMaxWidth + 'px';
  $('sSizeMode').value = cfg.widgetSizeMode || 'auto';
  $('sMgrOpacity').value = Math.round((cfg.managerOpacity ?? 0.92) * 100);
  $('sMgrOpacityVal').textContent = $('sMgrOpacity').value + '%';
  $('sCnMode').value = cfg.cnMode;
  $('sAutoFade').checked = !!cfg.autoFade;
  $('sAutoplay').checked = !!cfg.autoplay.on;
  $('sInterval').value = cfg.autoplay.interval;
  $('sLoop').checked = !!cfg.loop;
  $('sShuffle').checked = !!cfg.shuffle;
  $('sReadCn').checked = !!cfg.tts.readCn;
  $('sRate').value = cfg.tts.rate;
  $('sRateVal').textContent = cfg.tts.rate.toFixed(2) + 'x';
  $('sEdgeTts').checked = cfg.tts.engine !== 'system';
  refreshCacheSize();
  refreshDictCache();
  $('hkToggle').value = cfg.hotkeys.toggle;
  $('hkPrev').value = cfg.hotkeys.prev;
  $('hkNext').value = cfg.hotkeys.next;
  $('hkPlay').value = cfg.hotkeys.play;
}

// 音色清单由 Rust 给（内置预设），按 小女孩 / 女人 / 男人 分组
async function renderVoicePresets() {
  const sel = $('sVoicePreset');
  if (!sel || !cfg) return;
  let list = [];
  try { list = await window.api.ttsVoices(); } catch (e) { list = []; }
  if (!list.length) {
    sel.innerHTML = '<option value="woman-ava">Ava · 自然口语（推荐）</option>';
  } else {
    const groups = {};
    list.forEach(v => { (groups[v.group] = groups[v.group] || []).push(v); });
    sel.innerHTML = Object.keys(groups).map(g =>
      `<optgroup label="${esc(g)}">` +
      groups[g].map(v => `<option value="${esc(v.id)}">${esc(v.label)}</option>`).join('') +
      '</optgroup>').join('');
  }
  sel.value = cfg.tts.voice || 'woman-ava';
  if (!sel.value) sel.value = 'woman-ava';
}

async function refreshCacheSize() {
  const el = $('sCacheVal');
  if (!el) return;
  let n = 0;
  try { n = await window.api.ttsCacheSize(); } catch (e) { n = 0; }
  el.textContent = n >= 1048576
    ? (n / 1048576).toFixed(1) + ' MB'
    : Math.max(0, Math.round(n / 1024)) + ' KB';
}

// 词典缓存：条目数是「查过的词」的个数，顺带也是将来做生词本的现成数据
async function refreshDictCache() {
  const el = $('sDictVal');
  if (!el) return;
  let r = { entries: 0, bytes: 0 };
  try { r = await window.api.dictCacheSize(); } catch (e) {}
  const size = (r.bytes || 0) >= 1048576
    ? ((r.bytes || 0) / 1048576).toFixed(1) + ' MB'
    : Math.max(0, Math.round((r.bytes || 0) / 1024)) + ' KB';
  el.textContent = (r.entries || 0) + ' 条 · ' + size;
}

let cfgSaveT = null;
function saveConfigDebounced() {
  clearTimeout(cfgSaveT);
  cfgSaveT = setTimeout(() => window.api.setConfig(cfgSnapshot()), 400);
}
// 不含 widgetSizeMode：悬浮窗拖手柄会改成 manual，这里的缓存可能是旧的，回写会冲掉用户的手动尺寸
function cfgSnapshot() {
  return {
    mode: cfg.mode, opacity: cfg.opacity, font: cfg.font, fontColor: cfg.fontColor, widgetMaxWidth: cfg.widgetMaxWidth,
    cnMode: cfg.cnMode, autoFade: cfg.autoFade,
    autoplay: cfg.autoplay, loop: cfg.loop, shuffle: cfg.shuffle, tts: cfg.tts
  };
}

$('sMode').addEventListener('change', e => { cfg.mode = e.target.value; saveConfigDebounced(); });
$('sOpacity').addEventListener('input', e => {
  cfg.opacity = e.target.value / 100;
  $('sOpacityVal').textContent = e.target.value + '%';
  saveConfigDebounced();
});
$('sFont').addEventListener('input', e => { cfg.font = +e.target.value; $('sFontVal').textContent = cfg.font + 'px'; saveConfigDebounced(); });
$('sFontColor').addEventListener('input', e => { cfg.fontColor = e.target.value; $('sFontColorVal').textContent = e.target.value; saveConfigDebounced(); });
$('sMaxW').addEventListener('input', e => { cfg.widgetMaxWidth = +e.target.value; $('sMaxWVal').textContent = e.target.value + 'px'; saveConfigDebounced(); });
$('sSizeMode').addEventListener('change', e => { window.api.setConfig({ widgetSizeMode: e.target.value }); });
$('sMgrOpacity').addEventListener('input', e => {
  $('sMgrOpacityVal').textContent = e.target.value + '%';
  window.api.setConfig({ managerOpacity: e.target.value / 100 });
});
$('sCnMode').addEventListener('change', e => { cfg.cnMode = e.target.value; saveConfigDebounced(); });
$('sAutoFade').addEventListener('change', e => { cfg.autoFade = e.target.checked; saveConfigDebounced(); });
$('sAutoplay').addEventListener('change', e => { cfg.autoplay.on = e.target.checked; saveConfigDebounced(); });
$('sInterval').addEventListener('change', e => {
  cfg.autoplay.interval = Math.min(60, Math.max(3, +e.target.value || 8));
  e.target.value = cfg.autoplay.interval;
  saveConfigDebounced();
});
$('sLoop').addEventListener('change', e => { cfg.loop = e.target.checked; $('pLoopChk').checked = cfg.loop; saveConfigDebounced(); });
$('sShuffle').addEventListener('change', e => { cfg.shuffle = e.target.checked; $('pShuffleChk').checked = cfg.shuffle; saveConfigDebounced(); });
$('sReadCn').addEventListener('change', e => { cfg.tts.readCn = e.target.checked; saveConfigDebounced(); });
$('sRate').addEventListener('input', e => {
  cfg.tts.rate = +e.target.value;
  $('sRateVal').textContent = cfg.tts.rate.toFixed(2) + 'x';
  $('pRate').value = cfg.tts.rate; $('rateVal').textContent = cfg.tts.rate.toFixed(2) + 'x';
  saveConfigDebounced();
});
$('sEdgeTts').addEventListener('change', e => {
  cfg.tts.engine = e.target.checked ? 'edge' : 'system';
  saveConfigDebounced();
});
$('sVoicePreset').addEventListener('change', e => { cfg.tts.voice = e.target.value; saveConfigDebounced(); });
$('sVoiceTry').addEventListener('click', () => {
  speakEn('Good morning. This is how I sound when I read your sentences.');
});
$('sCacheClear').addEventListener('click', async () => {
  try { await window.api.ttsCacheClear(); } catch (e) {}
  refreshCacheSize();
  toast('语音缓存已清理');
});
$('sDictClear').addEventListener('click', async () => {
  try { await window.api.dictCacheClear(); } catch (e) {}
  refreshDictCache();
  toast('词典缓存已清除');
});

/* ---------- 快捷键捕获 ---------- */
const HK_ACTIONS = { hkToggle: 'toggle', hkPrev: 'prev', hkNext: 'next', hkPlay: 'play' };
for (const [inputId, action] of Object.entries(HK_ACTIONS)) {
  const input = $(inputId);
  input.addEventListener('focus', () => { input.value = '按下组合键…'; });
  input.addEventListener('keydown', async (e) => {
    e.preventDefault(); e.stopPropagation();
    if (e.key === 'Escape') { input.value = cfg.hotkeys[action]; input.blur(); return; }
    if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return;
    const parts = [];
    if (e.ctrlKey) parts.push('Control');
    if (e.altKey) parts.push('Alt');
    if (e.shiftKey) parts.push('Shift');
    if (e.metaKey) parts.push('Super');
    if (!parts.length) { input.value = '需带 Alt 或 Ctrl'; return; }
    let key = e.key;
    if (key.length === 1) key = key.toUpperCase();
    else if (/^(Arrow|Page|Home|End|Insert|Delete|F\d+)$/.test(key)) {
      key = key.replace('Arrow', '');
    } else { input.value = '这个键不能用作快捷键'; return; }
    const accel = [...parts, key].join('+');
    const r = await window.api.setHotkey(action, accel);
    if (r.ok) { cfg.hotkeys[action] = accel; input.value = accel; toast('快捷键已更新：' + accel); }
    else { input.value = cfg.hotkeys[action]; toast(r.error || '设置失败'); }
    input.blur();
  });
  input.addEventListener('blur', () => { if (input.value !== cfg.hotkeys[action]) input.value = cfg.hotkeys[action]; });
}

/* ================= 启动 ================= */
(async () => {
  const s = await window.api.getState();
  cfg = s.config;
  bindSettings();
  await renderVoicePresets();
  await refreshBooks();
  await loadPractice();
})();
