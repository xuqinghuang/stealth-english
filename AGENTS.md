# AGENTS.md

给在这个仓库里干活的 AI 编码代理看。人的设计文档是 `设计方案.md`（含功能清单、决策过程、踩坑记录），这里只写**动手前必须知道的事**。

## 项目

办公室隐身英语句子练习工具（Windows 桌面应用）。悬浮窗伪装成工作提醒 / 系统通知，在工位角落播放英文句子；资料库窗口负责导入 txt、练习、改设置；老板键一键隐藏。

- **技术栈**：Tauri 2（Rust）+ 原生 HTML/CSS/JS。没有框架、没有打包器、没有前端运行时依赖，`package.json` 里只有 `@tauri-apps/cli` 一个 devDependency。
- **只有 Windows 一个目标平台**，依赖 WebView2、DWM 与 Win32（分层窗口、置顶、鼠标穿透）。
- **命名**：仓库名 / npm 包名 / `tauri.conf.json` 的 productName / Cargo 包名统一用 `stealth-english`；**对用户显示的名字仍是中文「英语悄悄学」**（托盘提示、资料库窗口标题），两者刻意分开。
- **交付形态**：便携版文件夹 `dist/stealth-english/`，双击 `stealth-english.exe` 运行，数据写在 exe 旁边的 `data/`。

## 命令

| 命令 | 作用 |
|---|---|
| `npm run dev` | 起开发窗口（先自动跑 `web`，再 `cargo build` 并运行） |
| `npm run web` | **把 `src/` + `core/` + `bridge/` 生成为 `web/`** |
| `npm test` | 跑 `core/test.js`（27 项，纯 JS 逻辑，不需要 Rust） |
| `npm run pack` | 完整打包 → `dist/stealth-english/`（web → `cargo build --release` → 组装） |
| `npm run pack -- --skip-build` | 只重新组装，复用 `src-tauri/target/release/stealth-english.exe` |
| `npm run icon` | 重画图标并调 `tauri icon` 生成到 `src-tauri/icons/`（脚本只保留被引用的 3 个文件） |
| `npm run tauri -- <args>` | 透传 Tauri CLI 原生命令（`info` / `icon` / `build`…） |

**两条硬规则**

1. **改了 `src/` 或 `bridge/` 就必须 `npm run web`**。`npm run dev` 会自动跑，手工验证和打包前别忘了。
2. **前端资源是编译期嵌进二进制的**（`tauri.conf.json` 没设 `devUrl`，`tauri-codegen` 走 `EmbeddedAssets`）。所以改完前端要**重跑 `npm run dev`**（`web/` 变化会自动触发重编，不必手动 `cargo build`）；打包版必须重新 `npm run pack`，只替换 `dist/` 里的 exe 是没用的。

## 目录

| 路径 | 说明 |
|---|---|
| `src/widget/` | 悬浮窗页面（`index.html` + `widget.js`） |
| `src/manager/` | 资料库页面（`index.html` + `manager.js`，四个 tab：我的资料库 / 导入文章 / 练习 / 设置） |
| `bridge/api.js` | `window.api` —— 页面访问后端的唯一入口（`invoke` + 事件监听 + 拖拽） |
| `core/` | 纯 JS 业务逻辑（`align.js` 对齐、`split.js` 分句、`importer.js` 组卡），CommonJS |
| `core/test.js` | `npm test` 的入口 |
| `tools/sync-web.js` | 生成 `web/`（复制 `src/`、包装 `core/`、注入 bridge、剥掉页面 CSP meta） |
| `tools/pack.js` | 便携版打包 |
| `tools/gen-icon.js` | 画 1024 源图并调 `tauri icon` |
| `src-tauri/src/lib.rs` | 装配：插件、窗口、托盘、快捷键、命令注册 |
| `src-tauri/src/windows.rs` | 两个窗口的创建与行为，含 Win32 层（分层窗口 / 不透明度 / 穿透） |
| `src-tauri/src/state.rs` | 内存状态、事件推送、业务命令 |
| `src-tauri/src/store.rs` | `data/` 下的 JSON 读写、数据根目录解析 |
| `src-tauri/src/tts.rs` | Edge 神经语音合成 + mp3 缓存 |
| `src-tauri/src/dict.rs` | 点词查义：有道 jsonapi 取词 + 字段校验 + 裁剪 + 词典缓存 |
| `src-tauri/src/textfile.rs` | txt 导入：对话框 + 编码识别 |
| `src-tauri/src/hotkeys.rs` | 全局快捷键 |
| `src-tauri/default_config.json` | 配置默认值（Rust 用 `include_str!` 编译进来做兜底） |
| `src-tauri/demo_book.json` | 内置的 288 句示例书（**全部自编**，不含任何教材原文；按 72 个语法点 × 4 句排，从 be 动词一路到被动语态）。改内容必须同时把 `state.rs` 的 `DEMO_VERSION` 加一，否则老装机不会再播种 |
| `src-tauri/tauri.conf.json` | 窗口与构建配置（`frontendDist=../web`、CSP、bundle 图标） |
| `src-tauri/capabilities/` | 权限配置，两个窗口只授 `core:default`，业务命令由自身 `invoke_handler` 提供 |
| `src-tauri/icons/` | 应用图标，**只有 3 个文件**：`32x32.png`（托盘图标）、`128x128.png`、`icon.ico`（exe 资源）。iOS / Android / UWP 那一整套已清掉，`gen-icon.js` 用白名单保证不会再生成 |
| `web/` | **生成物**，别手改；Tauri 实际加载的前端 |
| `data/` | 运行时数据（dev 编译指向仓库根 `data/`） |
| `dist/` | 打包产物 |
| `design/` | **设计原型的收纳目录，整个被 `.gitignore` 排除**（不进仓库、不参与构建）：`mockup.html` 早期外观原型（**已停止维护**，里面还有 EPUB / 粘贴导入等后来砍掉的按钮，别当成需求）、`mockup-dict.html` 点词查义原型（功能已落地，见 `src/widget/`）、`voice-samples/` 音色试听样例 |
| `设计方案.md` | 人的设计文档 |

## 架构

**两个窗口、一份状态**

| 窗口 | 标签 | 页面 | 形态 |
|---|---|---|---|
| 悬浮窗 | `widget` | `src/widget/` | 无边框 + 透明 + 置顶 + 不占任务栏 |
| 资料库 | `manager` | `src/manager/` | 普通窗口，关闭 = 收进托盘 |

- 状态唯一来源是 Rust 的 `state::App`（`config` / `book` / `index`）。前端**只发 patch**（`config_set`、`index_set`…），改完由 Rust 推给两个窗口，页面不自己算状态。
- 事件（Rust → 页面）：`state` → 悬浮窗、`state-sync` → 资料库、`cmd` → 悬浮窗（`playpause` / `stop`）。启动首帧由 `api.getState()` 兜底，别假设事件一定先到。
- 22 个命令在 `lib.rs` 的 `generate_handler!` 里注册：state 7（`state_get` / `index_set` / `config_set` / `book_open` / `book_delete` / `book_rename` / `book_add`）、store 2（`list_books` / `read_book`）、textfile 1（`pick_txt`）、dict 3（`dict_lookup` / `dict_cache_size` / `dict_cache_clear`）、tts 4（`tts_speak` / `tts_voices` / `tts_cache_size` / `tts_cache_clear`）、hotkeys 1（`hotkey_set`）、windows 4（`widget_resize` / `widget_toggle` / `widget_hide` / `widget_drag`）。
- **打开资料库、退出程序只走托盘**（`lib.rs` 里直接调 `show_manager` / `app.exit`），没有对应的前端命令。曾经有过 `manager_show` / `app_quit` 两个命令和 `api.js` 里的桥接，但没有任何页面调用过，已删除 —— 前端不需要它们。

**导入流程**（切分对齐刻意留在 JS 侧）
`pick_txt` 只负责弹对话框 + 编码识别（UTF-8 / GBK / GB18030 / UTF-16）→ 文本回页面 → 页面跑 `core/importer.js`（内部用 `align.js` / `split.js`）→ 预览确认 → `book_add` 落盘。同一份 `core/` 既被 `npm test` 直接 `require`，也被包装后在浏览器里跑。

**语音**
`tts_speak` 在 Rust 侧用 WebSocket 连微软 Edge 免费朗读端点，结果缓存成 mp3（`data/cache/tts/<hash>.mp3`，key = 音色 + 语速 + 文本），以 base64 回传，页面用 `<audio>` 播。任何失败（没网 / 403 / 空音频 / 25 秒超时）都静默回退系统语音 `speechSynthesis`，不打断自动播放。

**查词（点单词看释义）**
悬浮窗把整句切成 `.w-word` 可点单词，点击 → `dict_lookup` → Rust 请求有道非官方接口 `dict.youdao.com/jsonapi` → 裁剪 → 写 `data/cache/dict.json`。卡片排在英文句**上方**，靠窗口向上生长显示（**窗口画不到自己外面，做绝对定位浮层会被裁掉**）。

- **「向上」要在 Rust 侧显式锚定**：`widget_resize` 的 `anchor` 除 `topleft` 外，卡片开合时前端报 `bottom`（底边钉住，`y = y_old + old_h - new_h`）。用户拖过窗口后默认是左上角固定、向下长，卡片一出现会把原文整段顶下去；没拖过时走右下角停靠，底边本来就固定，不用管。`bottom` 分支会把 `y` 钳在工作区上沿，避免卡片被顶出屏幕。
- 卡片头部常驻关闭按钮（`✕`，`data-act="close"`），「查询中…」和失败态也带着。

- **成功判据是「解析出字段」，不是 HTTP 状态码**。取词必须带浏览器 UA + `Referer: https://dict.youdao.com/`，**少任一个都会返回 200 但 body 为 0 字节**——静默失败。
- **词性不在任何字段里**，而是释义字符串的前缀（`"v. 耳语，低语…"`）；`trs[].pos` 恒为 `null`。没有标准词性前缀的义项（`【名】 (X) 某人名`）直接丢弃。
- 释义路径固定是 `ec.word[0].trs[][tr][0].l.i[0]`；`ec` 键不存在就是没收录这个词。附加字段：`prototype` 给词形还原（sitting→sit / was→be / children→child），`wfs` 给词形变化表。
- 卡片只放「单词 + 英/美音标 + 词性 + 释义」，**不带例句、不带来源与耗时**；只有失败态才显示原因 + `重试` / `复制单词`。
- 缓存存的是**裁剪后**的结果（每词 150~250 字节），不是接口原响应——原响应从 49 KB（tree）到 **517 KB**（run）不等，直接存会撑出几百 MB。

**数据目录**（`store.rs::data_root`：dev 编译 → 仓库根 `data/`，发布版 → exe 旁边 `data/`）

```
data/config.json                 全部设置（单一 config；前端 patch，Rust deep_merge）
data/books/<id>/meta.json        书名、进度等
data/books/<id>/sentences.json   句子卡片
data/cache/tts/*.mp3             神经语音缓存（设置页可查大小、清理）
data/cache/dict.json             词典缓存：查过的词（已裁剪，不是接口原文）
data/logs/app.log                日志
```

改 JSON 结构时必须保证**旧书库仍能读**——用户的 `data/` 是长期资产。

## 不要做的事

- **不要手改 `web/`**：生成物，`npm run web` 会整个删掉重建。
- **样式表里禁止出现 `app-region` / `-webkit-app-region`**。wry 会给 WebView2 打开非客户区支持，该属性会把整块元素变成"标题栏"：鼠标移动事件不再派发给页面，**CSS `:hover` 卡在最后一次状态永不更新**（悬停才出现的元素一旦出现就再不消失），而标了 `no-drag` 的子元素照常能点，症状极具迷惑性。窗口拖动一律走 `bridge/api.js` 的 `mousedown` → `widget_drag`。
- **不要把 `src-tauri/` 改名**：Tauri CLI 的约定目录，改名后 `tauri dev/build` 找不到配置（`--config` 是合并而非替换路径）。
- **不要引入框架、打包器或新的前端依赖**，也不要给 `core/` 加 Node API 依赖：`core/*.js` 是 CommonJS，既要能在 Node 里 `require`，也要能被 `sync-web.js` 包一层后在浏览器跑。
- **页面里不要直接 `fetch` 外网**：WebView2 有跨域限制、CSP 也拦，需要联网的功能放 Rust 侧。
- **CSP 只在 `tauri.conf.json` 维护**：页面内的 `<meta http-equiv="Content-Security-Policy">` 会被 `sync-web.js` 剥掉（它会挡掉 Tauri 注入的 IPC 脚本）。加新资源类型（如音频 blob）记得同步改 `csp`。
- **新增配置项要同时写进 `src-tauri/default_config.json`**，否则老用户升级后拿到 `undefined`。

## 已知的坑（都踩过，细节见 `设计方案.md` 第 7 节）

1. **前端资源是编译期嵌入的**（见"两条硬规则"第 2 条）：改完 `src/` 没重编，看到的还是旧界面。
2. **`-webkit-app-region` 会让 `:hover` 卡死**（见"不要做的事"）。
3. **tao 缓存的窗口标志会和实际情况脱节**：① 资料库窗口的不透明度会被抹掉 —— `apply_diff` 在窗口标志变化时（`show()` 就会触发）用自己缓存的 ex-style 覆盖整个 `GWL_EXSTYLE`，刚设的 `WS_EX_LAYERED` 位就没了；tao 的窗口操作还是 `PostMessageW` **异步**投递，"在 `show()` 之后再设一次"也不保险，必须用 `app.run_on_main_thread()` 排到同队列之后；改完扩展位要补 `SetWindowPos(SWP_FRAMECHANGED)`。② 悬浮窗用原生 `ShowWindow(SW_SHOWNOACTIVATE)` 显示（为了不抢焦点），tao 的 `VISIBLE` 标志一直停在 false，`apply_diff` 见 flag 无变化**直接 return**，所以 **`win.hide()` 是空操作**，隐藏也得走原生 `ShowWindow(SW_HIDE)`。另外「隐藏悬浮窗」不能跨会话持久化（`lib.rs` 启动时无条件重置 `widgetVisible`），否则用户下次启动就再也看不到窗口。
4. **Edge 朗读端点**：HTTP POST 已废弃，必须 WebSocket；握手不带 `Origin` + Edge UA 会 403；`Sec-MS-GEC` 的十六进制**必须大写**；音频帧是 `Path:audio\r\n` + mp3，切片要跳过 12 字节协议头。
5. **图标**：`tools/gen-icon.js` 自己编 PNG（chunk 的 length 与 CRC 必须**大端**，写错过一次导致启动即 panic）；ICO 交给官方 `tauri icon` 生成，别自己拼。
6. **有道 `jsonapi` 有两层时效风险**：① 缺 UA 或 Referer 时返回 **200 + 空 body**，属于静默失败（详见"架构 → 查词"）；② 有道已经给网页版接口 `jsonapi_s` 加过 `md5` 签名了（固定密钥就写在前端 JS 里），移动端这个 `jsonapi` 暂时还没加 —— 哪天跟上，要补的签名逻辑在 `dict.rs::fetch`。它是个非官方逆向接口，别赌它不变；正解是把它当成"一个可插拔的源"，失败就降级。
7. 本机环境：`cargo` 常不在 PATH（在 `$HOME/.cargo/bin`，需要时 `export PATH="$HOME/.cargo/bin:$PATH"`）；PowerShell 的 `Remove-Item` 偶尔静默删不掉文件，`[System.IO.File]::Delete()` 更可靠。

## 验证

- **逻辑改动**：`npm test`（27 项，全绿才算过）。
- **Rust 改动**：`cargo test --lib`（`dict` / `store` / `textfile` 三个模块有单测）。动了联网逻辑再加跑一次 `cargo test --lib -- --ignored --nocapture`，它会真连有道接口（`dict::tests::live_roundtrip`）——**只跑离线单测发现不了"200 + 空 body"这类问题**，请求头带没带对只有真连一次才知道。
- **界面 / 窗口改动**：`npm run web` 后 `npm run dev` 实机看一眼。透明、置顶、鼠标穿透、不透明度、托盘、快捷键这些只能人工验证。
- **快捷键会真的注册到系统**（默认 `Alt+H` 显示隐藏、`Alt+←/→` 上下一句、`Alt+P` 播放暂停）。`hotkey_set` 被占用时只警告不崩，调完记得确认注册成功（启动日志里有失败清单）。

## 参考资料

- `设计方案.md`：功能清单、导入规则、语音方案、里程碑、风险与踩坑 —— **改需求前先读它**。
- `design/mockup.html` / `design/mockup-dict.html`：早期原型（外观 / 词卡），**都已停止维护**，只作设计演进的参考。
- 仓库已初始化 git（分支 `main`，远程 `xuqinghuang/stealth-english`），`.gitignore` 忽略 `node_modules/`、`src-tauri/target/`、`web/`、`dist/`、`data/`、`.workbuddy/`，以及整个 `design/`（原型与音色样例的收纳目录）。
- **不要用 `git rm -r` 删目录**：本机的 git（2.55.0.windows.5）会连带把**祖先目录**一起删掉——删 `src-tauri/icons/android` 时 `src-tauri/` 和 `tools/` 整个消失了（见 `设计方案.md` 第 7 节）。删目录请用文件系统操作（`fs.rmSync`）+ `git add -A`；删单文件用 `git rm <file>` 是安全的。同理，`rebase` / `filter-branch` 这类历史重写也不要在这个仓库用，要改历史就重建。
