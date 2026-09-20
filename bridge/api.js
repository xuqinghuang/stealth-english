'use strict';
// window.api：页面与 Rust 后端之间的唯一入口（invoke 调用 + 事件监听）
// 页面只依赖这一层接口，后端怎么实现、怎么改，src/ 下的代码都不用动

(function () {
  const T = window.__TAURI__;
  if (!T || !T.core) {
    document.body && (document.body.textContent = '未找到 Tauri 运行时：请通过 tauri dev / 打包后的 exe 启动');
    return;
  }
  const invoke = T.core.invoke;
  const listen = T.event.listen;

  // 事件是异步注册的，启动时的首帧状态由 getState() 兜底，这里只接后续推送
  function on(event, cb) {
    listen(event, (e) => { try { cb(e.payload); } catch (err) { console.error(err); } });
  }

  function core() {
    const imp = window.__core && window.__core.importer;
    if (!imp) throw new Error('导入模块还没加载完成，请稍后再试');
    return imp;
  }

  window.api = {
    // ---------- 悬浮窗 ----------
    getState: () => invoke('state_get'),
    onState: (cb) => on('state', cb),
    onCmd: (cb) => on('cmd', cb),
    setIndex: (payload) => invoke('index_set', { index: payload.index, delta: payload.delta }),
    resizeWidget: (size) => invoke('widget_resize', {
      width: size.width, height: size.height, anchor: size.anchor
    }),
    widgetToggle: () => invoke('widget_toggle'),
    widgetHide: () => invoke('widget_hide'),
    setConfig: (patch) => invoke('config_set', { patch }),
    // 窗口拖动：Electron 时代用 CSS -webkit-app-region，但 wry 会给 WebView2 打开非客户区
    // 支持，那条属性会把整窗变成"标题栏"并吞掉鼠标移动事件（:hover 永不更新）。
    // 所以样式表里一条 app-region 都不能留，拖动一律走这里主动触发。
    startDrag: () => invoke('widget_drag'),

    // ---------- 资料库 ----------
    listBooks: () => invoke('list_books'),
    bookCards: (id) => invoke('read_book', { id }),
    openBook: (id) => invoke('book_open', { id }),
    deleteBook: (id) => invoke('book_delete', { id }),
    renameBook: (id, title) => invoke('book_rename', { id, title }),
    onStateSync: (cb) => on('state-sync', cb),

    // ---------- 导入：解码在 Rust，切分对齐仍在网页里跑同一份 core/ ----------
    readFile: () => invoke('pick_txt'),
    importPreview: (params) => core().preview(params),
    importCommit: (params) => {
      const book = core().buildBook(params);
      return invoke('book_add', { meta: book.meta, cards: book.cards })
        .then((r) => ({ ok: r.ok, id: r.id, warnings: book.warnings, count: book.meta.count }));
    },

    // ---------- 神经语音（Edge 免费端点，Rust 侧合成 + 缓存） ----------
    ttsVoices: () => invoke('tts_voices'),
    ttsSpeak: (p) => invoke('tts_speak', { text: p.text, preset: p.preset, rate: p.rate }),
    ttsCacheSize: () => invoke('tts_cache_size'),
    ttsCacheClear: () => invoke('tts_cache_clear'),

    // ---------- 查词（有道 jsonapi，取词和缓存在 Rust 侧） ----------
    dictLookup: (word) => invoke('dict_lookup', { word }),
    dictCacheSize: () => invoke('dict_cache_size'),
    dictCacheClear: () => invoke('dict_cache_clear'),

    // ---------- 设置 ----------
    setHotkey: (action, value) => invoke('hotkey_set', { action, value })
  };

  if (!document.getElementById('widget')) return;
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const zone = e.target.closest('#widget');
    // .w-word / .w-dict 也要排除：点单词查义时若顺带触发了拖动，窗口会跟着鼠标跑
    if (!zone || e.target.closest('.w-app, .w-ctrl, #wGrip, .w-word, .w-dict, button, input, select, textarea')) return;
    e.preventDefault();
    window.api.startDrag();
  });
})();
