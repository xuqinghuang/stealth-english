// 本地 JSON 存储：data/config.json 与 data/books/<id>/，格式与前端约定一致
// 便携模式：exe 旁边放 data/；开发编译时指向仓库的 data/，免得每次 clean 丢书库
// 只有前端直接调的三个是 command，其余给 state.rs 当内部函数用

use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn data_root() -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data")
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("data")))
            .unwrap_or_else(|| PathBuf::from("data"))
    }
}

fn read_json(path: &Path) -> Option<Value> {
    fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// books/ 不存在时后面所有写入都会失败
pub fn ensure_root(root: &Path) -> Result<(), String> {
    fs::create_dir_all(root.join("books")).map_err(err)
}

// 先写 .tmp 再改名，避免写一半被强杀留下半个坏文件
pub fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(err)?;
    }
    let tmp = path.with_file_name(format!("{}.tmp", path.file_name().and_then(|s| s.to_str()).unwrap_or("data.json")));
    fs::write(&tmp, serde_json::to_string_pretty(value).map_err(err)?).map_err(err)?;
    fs::rename(&tmp, path).map_err(err)
}

fn meta_path(root: &Path, id: &str) -> PathBuf {
    root.join("books").join(id).join("meta.json")
}

fn cards_path(root: &Path, id: &str) -> PathBuf {
    root.join("books").join(id).join("sentences.json")
}

pub fn list_books_at(root: &Path) -> Value {
    let mut out: Vec<(i64, Value)> = Vec::new();
    if let Ok(entries) = fs::read_dir(root.join("books")) {
        for entry in entries.flatten() {
            let Some(meta) = read_json(&entry.path().join("meta.json")) else { continue };
            if let Value::Object(mut map) = meta {
                let created = map.get("createdAt").and_then(Value::as_i64).unwrap_or(0);
                if let Some(id) = entry.file_name().to_str() {
                    map.insert("id".into(), Value::String(id.to_string()));
                }
                out.push((created, Value::Object(map)));
            }
        }
    }
    // 资料库按导入先后排，按 createdAt 升序
    out.sort_by_key(|(created, _)| *created);
    Value::Array(out.into_iter().map(|(_, meta)| meta).collect())
}

pub fn read_book_at(root: &Path, id: &str) -> Option<Value> {
    let meta = read_json(&meta_path(root, id))?;
    let cards = read_json(&cards_path(root, id)).unwrap_or_else(|| Value::Array(Vec::new()));
    let mut book = Map::new();
    book.insert("meta".into(), meta);
    book.insert("cards".into(), cards);
    Some(Value::Object(book))
}

pub fn save_meta(root: &Path, id: &str, meta: &Value) -> Result<(), String> {
    write_json(&meta_path(root, id), meta)
}

pub fn save_cards(root: &Path, id: &str, cards: &Value) -> Result<(), String> {
    write_json(&cards_path(root, id), cards)
}

pub fn delete_book_at(root: &Path, id: &str) -> Result<(), String> {
    let dir = root.join("books").join(id);
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(err)?;
    }
    Ok(())
}

// config.json 缺失或半损坏时用默认值兜底
pub fn deep_merge(base: &Value, patch: &Value) -> Value {
    if let (Value::Object(b), Value::Object(p)) = (base, patch) {
        let mut out = b.clone();
        for (k, v) in p {
            let merged = match out.get(k) {
                Some(bv) if bv.is_object() && v.is_object() => deep_merge(bv, v),
                _ => v.clone(),
            };
            out.insert(k.clone(), merged);
        }
        Value::Object(out)
    } else {
        patch.clone()
    }
}

pub fn load_config_at(root: &Path, defaults: &Value) -> Value {
    let saved = read_json(&root.join("config.json")).unwrap_or_else(|| Value::Object(Map::new()));
    deep_merge(defaults, &saved)
}

pub fn save_config_at(root: &Path, cfg: &Value) -> Result<(), String> {
    write_json(&root.join("config.json"), cfg)
}

pub fn new_book_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    let (millis, nanos) = match since {
        Ok(d) => (d.as_millis() as u64, d.subsec_nanos() as u64),
        Err(_) => (0, 0),
    };
    // 同一毫秒内连续导入两本书也不能撞车，所以再叠一个进程内计数器
    let salt = (nanos + COUNTER.fetch_add(1, Ordering::Relaxed)) % 1_679_616;   // 36^4
    format!("b_{}_{:0>4}", base36(millis), base36(salt))
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 { return "0".into(); }
    let mut s = String::new();
    while n > 0 {
        s.insert(0, DIGITS[(n % 36) as usize] as char);
        n /= 36;
    }
    s
}

#[tauri::command]
pub fn list_books() -> Value {
    list_books_at(&data_root())
}

#[tauri::command]
pub fn read_book(id: &str) -> Value {
    read_book_at(&data_root(), id).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("ew-store-{}-{}", name, nanos))
    }

    // 老版本（Electron 时期）留下的书库必须能原样读出，否则用户要重导一遍
    #[test]
    fn reads_existing_books_untouched() {
        let root = data_root();
        let books = list_books_at(&root);
        let arr = books.as_array().expect("list_books 应返回数组");
        assert!(!arr.is_empty(), "开发模式下应能读到仓库 data/books 里的书");
        for book in arr {
            assert!(book.get("id").and_then(Value::as_str).is_some(), "每本书都要带 id");
            assert!(book.get("title").is_some(), "每本书都要带 title");
        }
        let first = arr[0].get("id").unwrap().as_str().unwrap();
        let book = read_book_at(&root, first).expect("read_book_at 应能读出书");
        let cards = book["cards"].as_array().unwrap();
        assert_eq!(cards.len() as i64, book["meta"]["count"].as_i64().unwrap(), "cards 数要与 meta.count 一致");
        assert!(!cards[0]["en"].as_str().unwrap().is_empty(), "卡片要有英文原句");
    }

    #[test]
    fn write_list_read_delete_round_trip() {
        let root = temp_root("book");
        let id = new_book_id();
        let meta = serde_json::json!({ "title": "测试", "count": 2, "createdAt": 100, "progress": { "index": 1 } });
        let cards = serde_json::json!([
            { "en": "Hello there.", "cn": "你好。" },
            { "en": "Nice to meet you.", "cn": "很高兴见到你。" }
        ]);
        save_meta(&root, &id, &meta).unwrap();
        save_cards(&root, &id, &cards).unwrap();

        let books = list_books_at(&root);
        assert_eq!(books.as_array().unwrap().len(), 1);
        assert_eq!(books[0]["id"], id);
        assert_eq!(read_book_at(&root, &id).unwrap()["cards"][1]["cn"], "很高兴见到你。");

        delete_book_at(&root, &id).unwrap();
        assert!(list_books_at(&root).as_array().unwrap().is_empty());
        assert!(read_book_at(&root, &id).is_none());
        fs::remove_dir_all(root).ok();
    }

    // 书 id 不能撞车，格式沿用老版本（b_时间戳base36_随机）
    #[test]
    fn book_ids_look_unique() {
        let a = new_book_id();
        let b = new_book_id();
        assert!(a.starts_with("b_") && a.matches('_').count() == 2, "格式应为 b_<时间>_<随机>，实际 {a}");
        assert_ne!(a, b, "连续两次不能生成同一个 id");
    }

    #[test]
    fn config_merge_keeps_defaults_for_new_keys() {
        let root = temp_root("cfg");
        let defaults = serde_json::json!({
            "opacity": 0.9, "font": 15, "tts": { "rate": 0.95, "readCn": false, "enVoice": "" },
            "hotkeys": { "toggle": "Alt+H" }, "brandNew": { "deep": true }
        });
        assert_eq!(load_config_at(&root, &defaults), defaults, "没有 config.json 时全用默认值");

        // 老版本 config.json：缺 brandNew，且只改过 opacity / font / tts.rate
        save_config_at(&root, &serde_json::json!({ "opacity": 0.4, "tts": { "rate": 1.2 }, "font": 18 })).unwrap();
        let merged = load_config_at(&root, &defaults);
        assert_eq!(merged["opacity"], 0.4, "用户改过的值要覆盖默认");
        assert_eq!(merged["font"], 18);
        assert_eq!(merged["tts"]["rate"], 1.2, "嵌套里用户改过的要覆盖");
        assert_eq!(merged["tts"]["readCn"], false, "嵌套里用户没提的键要保留默认");
        assert_eq!(merged["hotkeys"]["toggle"], "Alt+H", "用户没动过的整组保留默认");
        assert_eq!(merged["brandNew"]["deep"], true, "新版本加的键不能因为老配置而丢");
        fs::remove_dir_all(root).ok();
    }

    // 写入不能留下 .tmp 残档，否则崩溃后 data/ 会越长越脏
    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let root = temp_root("atomic");
        let path = meta_path(&root, "b_test_0002");
        write_json(&path, &serde_json::json!({ "title": "x" })).unwrap();
        assert!(path.exists());
        assert!(!path.with_file_name("meta.json.tmp").exists());
        fs::remove_dir_all(root).ok();
    }
}
