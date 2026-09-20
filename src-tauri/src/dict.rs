// 查词：有道词典 jsonapi（非官方接口）+ 本地缓存
//
// 接口性质与来源
//   `dict.youdao.com/jsonapi` 是有道自家产品内部在调的接口 —— 从它认的请求参数
//   （imei / mid / vendor / screen / keyfrom=mdict.<版本>.<平台>）看，是移动端在用。
//   有道从未公开文档化过它，社区的用法全部来自浏览器 DevTools 抓包。
//   有道正式的 API 是 openapi.youdao.com（智云平台）：要注册、按字符计费、
//   而且条款明文禁止缓存 —— 跟「点过的词存本地」这个设计天然冲突。
//
// 实测到的两个坑（2026-09，都是 curl 打出来的）
//   1. 缺 User-Agent 或缺 Referer 时，接口返回 **HTTP 200 但 body 为 0 字节**。
//      失败是静默的。所以「成功」的判据只能是解析出完整字段，绝不能看状态码 ——
//      否则接口哪天变了，用户看到的是空白卡片，日志里却全是 200。
//   2. 有道已经给网页版接口 `jsonapi_s` 加过 sign 签名了（md5 三步 + 一个写死在
//      前端 JS 里的固定密钥），移动端这个 `jsonapi` 暂时还没加。哪天跟上，
//      就要在这里补签名。届时故障表现是上面第 1 条那种空响应。
//
// 任何一次查不到都返回 Err，由前端降级，绝不阻塞阅读。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const REFERER: &str = "https://dict.youdao.com/";
const API: &str = "https://dict.youdao.com/jsonapi";
const TIMEOUT_SECS: u64 = 8;

/// 卡片放得下的量：词性最多 4 个，每个词性最多 3 条子义项，每条最多 30 字
const MAX_POS: usize = 4;
const MAX_MEANS_PER_POS: usize = 3;
const MAX_MEAN_CHARS: usize = 30;

#[derive(Serialize, Deserialize, Clone)]
pub struct Tr {
    pub pos: String,
    pub means: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    /// 被查的原词（小写）
    pub w: String,
    /// 美式音标
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub us: Option<String>,
    /// 英式音标
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uk: Option<String>,
    /// 词形还原后的原形：sitting → sit、was → be、children → child。有道在 prototype 里给
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proto: Option<String>,
    pub trs: Vec<Tr>,
}

// ---------- 缓存 ----------
/// data/cache/dict.json：一个词一条裁剪后的记录（约 150~250 字节）
///
/// 注意这里存的是**裁剪后**的结果，不是接口原始响应 —— 实测原始响应
/// 从 49 KB（tree）到 517 KB（run）不等，里面塞满了柯林斯、双语例句、
/// 同义词、词根词缀。5000 个词直接存原文就是 250 MB 级，正是要避开的东西。
pub fn cache_path() -> PathBuf {
    crate::store::data_root().join("cache").join("dict.json")
}

pub struct Cache(Mutex<HashMap<String, Entry>>);

impl Cache {
    /// 启动时读一次，读不出来就当空表（缓存坏了不该拦住程序启动）
    pub fn load() -> Self {
        let map = fs::read_to_string(cache_path())
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, Entry>>(&s).ok())
            .unwrap_or_default();
        Cache(Mutex::new(map))
    }

    fn get(&self, key: &str) -> Option<Entry> {
        self.0.lock().ok()?.get(key).cloned()
    }

    /// 写入并立刻落盘。词典表是 MB 级、点词是「人点一下」的频率，
    /// 全量重写比维护增量日志简单得多，也不会有半边写入的状态
    fn put(&self, key: &str, entry: Entry) {
        let Ok(mut map) = self.0.lock() else { return };
        map.insert(key.to_string(), entry);
        let path = cache_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).ok();
        }
        if let Ok(s) = serde_json::to_string(&*map) {
            fs::write(path, s).ok();
        }
    }

    fn stats(&self) -> (usize, u64) {
        let n = self.0.lock().map(|m| m.len()).unwrap_or(0);
        let bytes = fs::metadata(cache_path()).map(|m| m.len()).unwrap_or(0);
        (n, bytes)
    }

    fn clear(&self) -> u64 {
        let bytes = fs::metadata(cache_path()).map(|m| m.len()).unwrap_or(0);
        let Ok(mut map) = self.0.lock() else { return 0 };
        map.clear();
        fs::remove_file(cache_path()).ok();
        bytes
    }
}

// ---------- 取词 ----------
/// 只留字母，转小写。句子里点出来的词常带标点或首字母大写
fn normalize(word: &str) -> String {
    word.trim()
        .trim_matches(|c: char| !c.is_ascii_alphabetic())
        .to_lowercase()
}

async fn fetch(word: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("构造查词请求失败：{e}"))?;

    let resp = client
        .get(API)
        .query(&[("q", word)])
        // 这两个头缺一个，服务端就返回 200 + 空 body（实测）
        .header("User-Agent", UA)
        .header("Referer", REFERER)
        .send()
        .await
        .map_err(|e| format!("查词请求失败：{e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取查词响应失败：{e}"))?;

    if text.trim().is_empty() {
        return Err(format!(
            "查词服务返回了空响应（HTTP {status}）：接口可能已变更或需要新鉴权"
        ));
    }
    serde_json::from_str(&text).map_err(|_| "查词结果无法解析（接口结构可能已变更）".to_string())
}

/// 从原始响应里裁出卡片要用的字段。
/// 这里是唯一判定「查到了」的地方 —— 拿不到词条或没有一条可用释义就报错，
/// 坏数据绝不写进缓存
fn parse(word: &str, v: &Value) -> Result<Entry, String> {
    let head = v
        .get("ec")
        .and_then(|e| e.get("word"))
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or_else(|| "没有收录这个词".to_string())?;

    let mut trs: Vec<Tr> = Vec::new();
    if let Some(list) = head.get("trs").and_then(Value::as_array) {
        for item in list {
            // 形状固定是 trs[].tr[0].l.i[0]，中间三层都是包装壳
            let Some(line) = item
                .get("tr")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(|t| t.get("l"))
                .and_then(|l| l.get("i"))
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(Value::as_str)
            else {
                continue;
            };

            // 词性不在任何字段里，而是藏在释义字符串的前缀上（"v. 耳语，低语…"）。
            // 形如 "【名】 (Sitting) （美、加）西特（人名）" 这种专名义项没有标准
            // 词性前缀，split_pos 返回 None —— 正好把这类对阅读无用的条目滤掉
            let Some((pos, rest)) = split_pos(line) else { continue };
            let means = split_means(rest);
            if means.is_empty() {
                continue;
            }
            trs.push(Tr { pos: pos.to_string(), means });
            if trs.len() >= MAX_POS {
                break;
            }
        }
    }

    if trs.is_empty() {
        return Err("收录了词条但没有可用的释义".to_string());
    }

    Ok(Entry {
        w: word.to_string(),
        us: text_of(head, "usphone"),
        uk: text_of(head, "ukphone"),
        proto: text_of(head, "prototype"),
        trs,
    })
}

fn text_of(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// 拆出 "v." / "adj." / "int." 这类词性前缀，返回（词性, 剩余释义）
fn split_pos(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && i < 8 && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == 0 || i >= b.len() || b[i] != b'.' {
        return None;
    }
    let pos = &s[..=i];
    let rest = s[i + 1..].trim_start();
    if rest.is_empty() {
        None
    } else {
        Some((pos, rest))
    }
}

/// 按分号切子义项并截断。词典的单条释义常有 60+ 字，卡片放不下
fn split_means(s: &str) -> Vec<String> {
    s.split(['；', ';'])
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .take(MAX_MEANS_PER_POS)
        .map(|x| {
            if x.chars().count() > MAX_MEAN_CHARS {
                let cut: String = x.chars().take(MAX_MEAN_CHARS).collect();
                format!("{cut}…")
            } else {
                x.to_string()
            }
        })
        .collect()
}

// ---------- commands ----------
#[tauri::command]
pub async fn dict_lookup(
    word: String,
    cache: tauri::State<'_, Cache>,
) -> Result<Value, String> {
    let key = normalize(&word);
    if key.is_empty() {
        return Err("空词".to_string());
    }

    if let Some(entry) = cache.get(&key) {
        return Ok(json!({ "entry": entry, "cached": true }));
    }

    let raw = fetch(&key).await?;
    let entry = parse(&key, &raw)?;
    cache.put(&key, entry.clone());
    Ok(json!({ "entry": entry, "cached": false }))
}

/// 缓存条目数与占用字节（设置页显示用）
#[tauri::command]
pub fn dict_cache_size(cache: tauri::State<'_, Cache>) -> Value {
    let (entries, bytes) = cache.stats();
    json!({ "entries": entries, "bytes": bytes })
}

#[tauri::command]
pub fn dict_cache_clear(cache: tauri::State<'_, Cache>) -> Value {
    json!({ "freed": cache.clear() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实响应里 ec 子树（2026-09 抓取，只留解析用得到的部分）。
    /// 用真夹具而不是手写的结构，是为了让测试盯的是「接口实际长什么样」，
    /// 而不是「我以为它长什么样」—— 词性藏在释义前缀里这件事就是这么发现的
    const WHISPER: &str = r#"{"ec":{"exam_type":["高中","CET4","CET6","考研","IELTS","GRE"],"source":{"name":"有道词典","url":"https://dict.youdao.com"},"word":[{"usphone":"ˈwɪspər","ukphone":"ˈwɪspə(r)","ukspeech":"whisper&type=1","trs":[{"tr":[{"l":{"i":["v. 耳语，低语，小声说；（私下或秘密地）传说（某事），谣传；（树叶）发沙沙声，（风）发飒飒声，（水）发潺潺声"]}}]},{"tr":[{"l":{"i":["n. 耳语（声），低语（声），私语（声）；沙沙声，飒飒声，潺潺声；传闻，谣言；蛛丝马迹，暗示；少许，微量；少量信息"]}}]}],"wfs":[{"wf":{"name":"复数","value":"whispers"}}],"return-phrase":{"l":{"i":"whisper"}},"usspeech":"whisper&type=2"}]}}"#;

    const SITTING: &str = r#"{"ec":{"exam_type":["初中","高中"],"word":[{"usphone":"ˈsɪtɪŋ","ukphone":"ˈsɪtɪŋ","trs":[{"tr":[{"l":{"i":["n. 坐着（做某事）的一段时间；（一次）供人画像（照相）的时间；一批，一次（就餐时间）；（法庭的）开庭，（议会的）开会；孵卵"]}}]},{"tr":[{"l":{"i":["adj. 坐的，坐着做的；（母鸡，鸟）孵蛋的，伏窝的；现任的，在任期内的；易击中的；（动物）蹲着的，（鸟）停落的"]}}]},{"tr":[{"l":{"i":["v. 坐，坐下；使就座；坐落在，位于（sit 的现在分词形式）"]}}]},{"tr":[{"l":{"i":["【名】  (Sitting) （美、加）西特（人名）"]}}]}],"wfs":[{"wf":{"name":"复数","value":"sittings"}}],"prototype":"sit"}]}}"#;

    /// 查不到的词：顶层压根没有 ec 这个键，只剩 web_trans（网络释义）
    const NOT_FOUND: &str =
        r#"{"web_trans":{"web-translation":[{"key":"ASDFGHJKL"}]},"input":"asdfghjkl","le":"en","lang":"en"}"#;

    fn parse_str(word: &str, raw: &str) -> Result<Entry, String> {
        parse(word, &serde_json::from_str(raw).expect("夹具本身要是合法 JSON"))
    }

    #[test]
    fn parses_plain_word() {
        let e = parse_str("whisper", WHISPER).unwrap();
        assert_eq!(e.w, "whisper");
        assert_eq!(e.us.as_deref(), Some("ˈwɪspər"));
        assert_eq!(e.uk.as_deref(), Some("ˈwɪspə(r)"));
        assert!(e.proto.is_none());
        assert_eq!(e.trs.iter().map(|t| t.pos.as_str()).collect::<Vec<_>>(), ["v.", "n."]);
        // 释义要按分号切开：原句 60+ 字，卡片放不下
        assert_eq!(e.trs[0].means[0], "耳语，低语，小声说");
        assert_eq!(e.trs[0].means.len(), MAX_MEANS_PER_POS);
        assert!(e.trs[0].means.iter().all(|m| m.chars().count() <= MAX_MEAN_CHARS + 1));
    }

    #[test]
    fn drops_senses_without_pos_prefix() {
        let e = parse_str("sitting", SITTING).unwrap();
        // 有 4 条 trs，最后一条是「【名】 (Sitting) （美、加）西特（人名）」
        // —— 专名，没有标准词性前缀，应当被滤掉
        assert_eq!(e.trs.iter().map(|t| t.pos.as_str()).collect::<Vec<_>>(), ["n.", "adj.", "v."]);
        // prototype 给出词形还原，sitting → sit
        assert_eq!(e.proto.as_deref(), Some("sit"));
    }

    #[test]
    fn errors_on_missing_and_malformed() {
        assert!(parse_str("asdfghjkl", NOT_FOUND).is_err(), "没有 ec 键就该报错");
        assert!(parse_str("x", "{}").is_err());
        assert!(parse_str("x", r#"{"ec":{"word":[]}}"#).is_err());
        // 有词条但一条带词性的释义都没有 → 也算查不到，不能返回空卡
        assert!(parse_str(
            "x",
            r#"{"ec":{"word":[{"trs":[{"tr":[{"l":{"i":["【名】某某（人名）"]}}]}]}]}}"#
        )
        .is_err());
    }

    #[test]
    fn splits_pos_prefix() {
        assert_eq!(split_pos("v. 耳语"), Some(("v.", "耳语")));
        assert_eq!(split_pos("adj. 好的"), Some(("adj.", "好的")));
        assert_eq!(split_pos("int. 啊"), Some(("int.", "啊")));
        assert_eq!(split_pos("【名】 (X) 人名"), None);
        assert_eq!(split_pos("v."), None, "只有词性没有释义，不算一条");
        assert_eq!(split_pos("中文"), None);
        assert_eq!(split_pos(""), None);
    }

    #[test]
    fn clips_means() {
        assert_eq!(split_means("a；b;c；d"), ["a", "b", "c"]);
        assert!(split_means("；；").is_empty());
        let long = split_means(&"字".repeat(50));
        assert_eq!(long.len(), 1);
        assert_eq!(long[0].chars().count(), MAX_MEAN_CHARS + 1); // 截断后补一个省略号
    }

    #[test]
    fn normalizes_punctuation_and_case() {
        assert_eq!(normalize("  Whisper!  "), "whisper");
        assert_eq!(normalize("“Sitting”"), "sitting");
        assert_eq!(normalize("well-known"), "well-known");
        assert_eq!(normalize("..."), "");
    }

    /// 真接口端到端（要联网，默认跳过）：
    ///   cargo test --lib -- --ignored --nocapture
    /// 这条用来确认 UA / Referer 两个头确实带对了 —— 少任何一个都会拿到
    /// HTTP 200 的空 body，只看状态码是发现不了的
    #[test]
    #[ignore]
    fn live_roundtrip() {
        for word in ["whisper", "sitting", "epistemology"] {
            let raw = tauri::async_runtime::block_on(fetch(word)).unwrap_or_else(|e| panic!("{word}: {e}"));
            let e = parse(word, &raw).unwrap_or_else(|e| panic!("{word}: {e}"));
            println!("{word} | us={:?} uk={:?} proto={:?} | {} 个词性", e.us, e.uk, e.proto, e.trs.len());
            for t in &e.trs {
                println!("    {} {}", t.pos, t.means.join("；"));
            }
            assert!(!e.trs.is_empty());
        }
    }
}
