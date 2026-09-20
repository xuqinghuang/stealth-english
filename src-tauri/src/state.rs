// 主进程状态：配置、当前书、进度、洗牌、广播
// 应用状态：载入书库、切句、组装并下发悬浮窗负载

use crate::{hotkeys, store, windows};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const DEMO_VERSION: i64 = 2;
const DEMO_BOOK: &str = include_str!("../demo_book.json");

pub struct App {
    pub root: PathBuf,
    pub config: Value,
    pub book_id: Option<String>,
    pub meta: Option<Value>,
    pub cards: Vec<Value>,
    pub index: usize,
    pub shuffle: Option<Vec<usize>>,
    pub shuffle_pos: usize,
}

pub struct Hub(Mutex<App>);

impl Hub {
    pub fn new(app: App) -> Hub {
        Hub(Mutex::new(app))
    }
    ///  panic 之后也要能继续用这把锁：一个渲染层的偶发错误不该让整个程序卡死
    fn lock(&self) -> std::sync::MutexGuard<'_, App> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn read<T: Sized>(app: &AppHandle, f: impl FnOnce(&App) -> T) -> T {
    let hub = app.state::<Hub>();
    let g = hub.lock();
    f(&g)
}

pub fn write<T: Sized>(app: &AppHandle, f: impl FnOnce(&mut App) -> T) -> T {
    let hub = app.state::<Hub>();
    let mut g = hub.lock();
    f(&mut g)
}

impl App {
    pub fn boot(root: PathBuf) -> App {
        let defaults: Value = serde_json::from_str(include_str!("../default_config.json"))
            .expect("default_config.json 坏了");
        let config = store::load_config_at(&root, &defaults);
        App {
            root,
            config,
            book_id: None,
            meta: None,
            cards: Vec::new(),
            index: 0,
            shuffle: None,
            shuffle_pos: 0,
        }
    }

    pub fn save_config(&self) {
        if let Err(e) = store::save_config_at(&self.root, &self.config) {
            log::error!("saveConfig {e}");
        }
    }

    fn flag(&self, key: &str) -> bool {
        self.config.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn payload(&self, reason: &str) -> Value {
        json!({
            "reason": reason,
            "config": self.config,
            "bookId": self.book_id,
            "bookTitle": self.meta.as_ref().and_then(|m| m.get("title")).cloned().unwrap_or_else(|| json!("")),
            "index": self.index,
            "total": self.cards.len(),
            "card": self.cards.get(self.index).cloned().unwrap_or(Value::Null),
        })
    }

    pub fn sync(&self) -> Value {
        json!({ "bookId": self.book_id, "index": self.index, "total": self.cards.len() })
    }

    pub fn load_book(&mut self, id: &str, keep_index: bool) -> bool {
        let Some(book) = store::read_book_at(&self.root, id) else { return false };
        let meta = book["meta"].clone();
        let cards = book["cards"].as_array().cloned().unwrap_or_default();
        let total = cards.len();
        if !keep_index {
            let saved = meta
                .get("progress")
                .and_then(|p| p.get("index"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            self.index = if total == 0 { 0 } else { (saved.max(0) as usize).min(total - 1) };
        }
        self.book_id = Some(id.to_string());
        self.meta = Some(meta);
        self.cards = cards;
        self.shuffle = None;
        self.shuffle_pos = 0;
        self.config["currentBookId"] = json!(id);
        self.save_config();
        if self.flag("shuffle") {
            self.reshuffle();
        }
        true
    }

    pub fn open_first_book(&mut self) -> bool {
        let id = store::list_books_at(&self.root)
            .get(0)
            .and_then(|b| b.get("id"))
            .and_then(Value::as_str)
            .map(String::from);
        match id {
            Some(id) => self.load_book(&id, false),
            None => {
                self.book_id = None;
                self.meta = None;
                self.cards.clear();
                self.index = 0;
                false
            }
        }
    }

    /// 随机顺序：切到随机时重排一次，切回顺序时清掉
    pub fn reshuffle(&mut self) {
        let total = self.cards.len();
        let mut order: Vec<usize> = (0..total).collect();
        let mut rng = Rng::new();
        for i in (1..total).rev() {
            let j = rng.below(i + 1);
            order.swap(i, j);
        }
        self.shuffle = Some(order);
        self.shuffle_pos = 0;
    }

    pub fn set_index(&mut self, i: i64) {
        let total = self.cards.len();
        if total == 0 {
            return;
        }
        self.index = (i.max(0) as usize).min(total - 1);
        self.save_progress();
    }

    pub fn step(&mut self, delta: i64) {
        let total = match self.cards.len() {
            0 => return,
            n => n as i64,
        };
        let order_len = self.shuffle.as_ref().map(Vec::len).unwrap_or(0) as i64;
        if self.flag("shuffle") && order_len == total {
            let order = self.shuffle.clone().unwrap_or_default();
            self.shuffle_pos = (((self.shuffle_pos as i64 + delta) % total + total) % total) as usize;
            self.index = order[self.shuffle_pos];
        } else {
            let next = ((self.index as i64 + delta) % total + total) % total;
            self.index = next as usize;
        }
        self.save_progress();
    }

    /// meta 很小，直接写不做防抖，崩了也不会丢进度
    pub fn save_progress(&self) {
        if let (Some(id), Some(meta)) = (&self.book_id, &self.meta) {
            let mut meta = meta.clone();
            meta["progress"] = json!({ "index": self.index });
            if let Err(e) = store::save_meta(&self.root, id, &meta) {
                log::error!("saveMeta {e}");
            }
        }
    }

    pub fn flush(&self) {
        self.save_progress();
        if let Some(id) = &self.book_id {
            if !self.cards.is_empty() {
                let snapshot = Value::Array(self.cards.clone());
                if let Err(e) = store::save_cards(&self.root, id, &snapshot) {
                    log::error!("saveCards {e}");
                }
            }
        }
        self.save_config();
    }
}

/// 不引 rand：Fisher–Yates 只需要一个够乱的 64 位序列
struct Rng(u64);
impl Rng {
    fn new() -> Rng {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x2545_F491_4F6C_DD1D);
        Rng(nanos | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        (self.next() % n as u64) as usize
    }
}

// ---------- 广播 ----------
pub fn push(app: &AppHandle, reason: &str) {
    let (payload, sync) = {
        let hub = app.state::<Hub>();
        let st = hub.lock();
        (st.payload(reason), st.sync())
    };
    let _ = app.emit_to(windows::WIDGET, "state", payload);
    let _ = app.emit_to(windows::MANAGER, "state-sync", sync);
}

/// 资料库刚打开时状态可能已经变了（比如悬浮窗切了句），补一次同步
pub fn push_manager(app: &AppHandle) {
    let sync = read(app, |st| st.sync());
    let _ = app.emit_to(windows::MANAGER, "state-sync", sync);
}

pub fn send_cmd(app: &AppHandle, cmd: &str) {
    let _ = app.emit_to(windows::WIDGET, "cmd", cmd.to_string());
}

pub fn flush(app: &AppHandle) {
    write(app, |st| st.flush());
}

// ---------- 首次运行：内置默认书（自编文本，不碰任何原书课文） ----------
impl App {
    pub fn seed_demo_book(&mut self) -> bool {
        let version = self.config.get("demoSeedVersion").and_then(Value::as_i64).unwrap_or(0);
        if version >= DEMO_VERSION {
            return false;
        }
        let demo: Value = match serde_json::from_str(DEMO_BOOK) {
            Ok(v) => v,
            Err(e) => {
                log::error!("demo_book.json 坏了：{e}");
                return false;
            }
        };
        let mut meta = demo["meta"].clone();
        meta["createdAt"] = json!(now_millis());
        meta["progress"] = json!({ "index": 0 });
        let id = store::new_book_id();
        let written = store::save_meta(&self.root, &id, &meta)
            .and_then(|_| store::save_cards(&self.root, &id, &demo["cards"]));
        if let Err(e) = written {
            log::error!("seedDemoBook {e}");
            return false;
        }
        self.config["demoSeedVersion"] = json!(DEMO_VERSION);
        self.save_config();
        true
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------- 命令 ----------
#[tauri::command]
pub fn state_get(app: AppHandle) -> Value {
    read(&app, |st| st.payload("get"))
}

#[tauri::command]
pub fn index_set(app: AppHandle, index: Option<i64>, delta: Option<i64>) -> Value {
    if let Some(delta) = delta {
        write(&app, |st| st.step(delta));
    } else if let Some(index) = index {
        write(&app, |st| st.set_index(index));
    }
    push(&app, "nav");
    json!({ "index": read(&app, |st| st.index) })
}

#[tauri::command]
pub fn config_set(app: AppHandle, patch: Value) -> Result<Value, String> {
    {
        let hub = app.state::<Hub>();
        let mut st = hub.lock();
        st.config = store::deep_merge(&st.config, &patch);
        // 切到「固定尺寸」时把当前窗口尺寸记下来，避免一切换就跳成内容大小
        if patch.get("widgetSizeMode").and_then(Value::as_str) == Some("manual")
            && patch.get("widgetSize").is_none()
        {
            if let Some((w, h)) = windows::current_widget_size(&app) {
                st.config["widgetSize"] = json!({ "w": w, "h": h });
            }
        }
        if patch.get("shuffle").is_some() {
            if st.flag("shuffle") {
                st.reshuffle();
            } else {
                st.shuffle = None;
                st.shuffle_pos = 0;
            }
        }
        st.save_config();
    }
    if patch.get("managerOpacity").is_some() {
        windows::apply_manager_opacity(&app);
    }
    if patch.get("hotkeys").is_some() {
        let fails = hotkeys::register_all(&app);
        if !fails.is_empty() {
            return Ok(json!({ "ok": false, "fails": fails }));
        }
    }
    push(&app, "config");
    Ok(json!({ "ok": true }))
}

#[tauri::command]
pub fn book_open(app: AppHandle, id: String) -> Value {
    let ok = write(&app, |st| st.load_book(&id, false));
    push(&app, "book");
    match ok {
        true => json!({ "ok": true, "index": read(&app, |st| st.index) }),
        false => json!({ "ok": false }),
    }
}

#[tauri::command]
pub fn book_delete(app: AppHandle, id: String) -> Result<Value, String> {
    store::delete_book_at(&store::data_root(), &id)?;
    let same = read(&app, |st| st.book_id.as_deref() == Some(id.as_str()));
    if same {
        write(&app, |st| {
            st.open_first_book();
        });
    }
    push(&app, "book");
    Ok(json!({ "ok": true }))
}

#[tauri::command]
pub fn book_rename(app: AppHandle, id: String, title: String) -> Result<Value, String> {
    let root = store::data_root();
    if let Some(book) = store::read_book_at(&root, &id) {
        let mut meta = book["meta"].clone();
        let cut: String = title.chars().take(60).collect();
        meta["title"] = json!(cut);
        store::save_meta(&root, &id, &meta)?;
        write(&app, |st| {
            if st.book_id.as_deref() == Some(id.as_str()) {
                st.meta = Some(meta.clone());
            }
        });
    }
    push(&app, "book");
    Ok(json!({ "ok": true }))
}

/// 导入入库：切分对齐仍在网页里跑（同一份 core/importer.js），这里只负责写盘 + 设为当前书
#[tauri::command]
pub fn book_add(app: AppHandle, meta: Value, cards: Value) -> Result<Value, String> {
    let root = store::data_root();
    let count = cards.as_array().map(Vec::len).unwrap_or(0);
    if count == 0 {
        return Err("没有解析出任何可练习的英文句子。".into());
    }
    let id = store::new_book_id();
    store::save_meta(&root, &id, &meta)?;
    store::save_cards(&root, &id, &cards)?;
    let ok = write(&app, |st| st.load_book(&id, false));
    push(&app, "book");
    Ok(json!({ "ok": ok, "id": id }))
}
