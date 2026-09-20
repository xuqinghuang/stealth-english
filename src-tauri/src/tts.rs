// 神经语音：微软 Edge 浏览器同款的免费朗读端点（WebSocket + Sec-MS-GEC 鉴权）
// 这是一条逆向出来的非官方接口，微软随时可能改鉴权或直接下线，
// 所以这里所有失败都返回 Err，由调用方（前端）静默回退到系统语音，绝不阻塞学习流程。
//
// 协议要点（实测，2026-09）：
//   wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1
//   ?TrustedClientToken=...&Sec-MS-GEC=...&Sec-MS-GEC-Version=...&ConnectionId=...
//   握手必须带 Origin + User-Agent，否则 403
//   发两条文本帧：speech.config（申明输出格式）→ ssml（带 X-RequestId）
//   收若干二进制帧，每帧以 "Path:audio\r\n" 之后才是 mp3 数据；见到 Path:turn.end 结束

use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tungstenite::client::IntoClientRequest;
use tungstenite::{connect, Message};

const TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
const GEC_VERSION: &str = "1-143.0.3650.96";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";
const ORIGIN: &str = "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold";
const WSS: &str = "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1";
const FORMAT: &str = "audio-24khz-48kbitrate-mono-mp3";
const DELIM: &str = "\r\n\r\n";
/// 音频帧的头部标记，后面紧跟 mp3 二进制
const AUDIO_TAG: &[u8] = b"Path:audio\r\n";

// ---------- 内置音色：3 类 × 2 个 ----------
// 全部用原音色，不做任何 pitch 变调——变调会把声音弄成"捏着嗓子"的假童声。
// Edge 免费端点里英文真童声只有两个（美音 Ana / 英音 Maisie），男孩童声一个都没有
// （那是 Azure 付费层的东西），所以干脆没有"小男孩"这一类。
struct Preset {
    id: &'static str,
    group: &'static str,
    label: &'static str,
    voice: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset { id: "girl-ana", group: "小女孩", label: "Ana · 美音童声", voice: "en-US-AnaNeural" },
    Preset { id: "girl-maisie", group: "小女孩", label: "Maisie · 英音童声", voice: "en-GB-MaisieNeural" },
    Preset { id: "woman-ava", group: "女人", label: "Ava · 自然口语（推荐）", voice: "en-US-AvaNeural" },
    Preset { id: "woman-aria", group: "女人", label: "Aria · 清晰朗读腔", voice: "en-US-AriaNeural" },
    Preset { id: "man-andrew", group: "男人", label: "Andrew · 温暖自然", voice: "en-US-AndrewNeural" },
    Preset { id: "man-chris", group: "男人", label: "Christopher · 新闻播报", voice: "en-US-ChristopherNeural" },
];

/// 中文朗读走同一个端点，音色固定：Azure 里口碑最好的女声
static ZH_PRESET: Preset = Preset {
    id: "zh",
    group: "中文",
    label: "晓晓 · 中文",
    voice: "zh-CN-XiaoxiaoNeural",
};

fn preset_of(id: &str) -> Option<&'static Preset> {
    if id == "zh" {
        return Some(&ZH_PRESET);
    }
    PRESETS.iter().find(|p| p.id == id)
}

// ---------- 缓存 ----------
/// data/cache/tts/：一句一份 mp3，名字是「音色+语速+音调+文本」的哈希
pub fn cache_dir() -> PathBuf {
    crate::store::data_root().join("cache").join("tts")
}

fn cache_key(text: &str, voice: &str, rate_pct: i32) -> String {
    let src = format!("{voice}|{rate_pct}|{text}");
    let full = format!("{:x}", Sha256::digest(src.as_bytes()));
    full[..32].to_string()
}

fn request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let full = format!("{:x}", Sha256::digest(format!("ew-{nanos}").as_bytes()));
    full[..32].to_string()
}

fn connection_id() -> String {
    request_id()
}

/// Sec-MS-GEC：微软 2024 年起要求的鉴权
/// Windows 文件时间（100ns 为单位）取整到 5 分钟 + token，取 SHA-256 十六进制
fn sec_ms_gec() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 11_644_473_600;
    let rounded = secs - (secs % 300);
    let ticks = rounded * 10_000_000;
    // 必须大写：小写会被服务端拒（实测 403）
    format!("{:X}", Sha256::digest(format!("{ticks}{TOKEN}").as_bytes()))
}

fn synth_url() -> String {
    format!(
        "{WSS}?TrustedClientToken={TOKEN}&Sec-MS-GEC={}&Sec-MS-GEC-Version={GEC_VERSION}&ConnectionId={}",
        sec_ms_gec(),
        connection_id()
    )
}

fn ssml(text: &str, voice: &str, rate_pct: i32) -> String {
    let safe = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let lang = if voice.starts_with("zh") { "zh-CN" } else { "en-US" };
    format!(
        "<speak version=\"1.0\" xmlns=\"http://www.w3.org/2001/10/synthesis\" xml:lang=\"{lang}\">\
         <voice name=\"{voice}\"><prosody rate=\"{rate_pct}%\">{safe}</prosody></voice></speak>"
    )
}

fn locate(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 一次完整合成：握手 → 发配置 → 发文本 → 收音频直到 turn.end
fn synthesize(text: &str, voice: &str, rate_pct: i32) -> Result<Vec<u8>, String> {
    let mut request = synth_url()
        .into_client_request()
        .map_err(|e| format!("构造朗读请求失败：{e}"))?;
    let headers = request.headers_mut();
    headers.insert(
        "Origin",
        tungstenite::http::HeaderValue::from_static(ORIGIN),
    );
    headers.insert(
        "User-Agent",
        tungstenite::http::HeaderValue::from_static(UA),
    );

    let (mut socket, _) = connect(request).map_err(|e| format!("连接朗读服务失败：{e}"))?;

    let config = format!(
        "Content-Type:application/json; charset=utf-8\r\nPath:speech.config{DELIM}\
         {{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":\
         {{\"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"false\"}},\
         \"outputFormat\":\"{FORMAT}\"}}}}}}}}"
    );
    socket
        .send(Message::Text(config.into()))
        .map_err(|e| format!("发送配置失败：{e}"))?;

    let body = ssml(text, voice, rate_pct);
    let frame = format!(
        "X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nPath:ssml{DELIM}{body}",
        request_id()
    );
    socket
        .send(Message::Text(frame.into()))
        .map_err(|e| format!("发送文本失败：{e}"))?;

    let mut audio: Vec<u8> = Vec::new();
    while let Ok(message) = socket.read() {
        match message {
            Message::Binary(raw) => {
                let bytes: &[u8] = &raw;
                if locate(bytes, b"Path:turn.end").is_some() {
                    break;
                }
                if let Some(pos) = locate(bytes, AUDIO_TAG) {
                    // 必须跳过头本身，否则协议文本混进 mp3 开头，浏览器解码不出来 → 无声
                    audio.extend_from_slice(&bytes[pos + AUDIO_TAG.len()..]);
                }
            }
            Message::Text(raw) => {
                let text: &str = &raw;
                if text.contains("Path:turn.end") {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    let _ = socket.close(None);

    if audio.len() < 1024 {
        return Err("朗读服务没有返回音频".to_string());
    }
    if !looks_like_mp3(&audio) {
        // 微软改协议时最容易出的就是帧结构变了，宁可报错回退，也不能把垃圾写进缓存
        return Err("朗读服务返回的不是 mp3（协议可能已变更）".to_string());
    }
    Ok(audio)
}

/// mp3 要么以 ID3 标签开头，要么以 11 位同步字 0xFFEx 开头
fn looks_like_mp3(b: &[u8]) -> bool {
    if b.len() < 4 {
        return false;
    }
    if &b[..3] == b"ID3" {
        return true;
    }
    b[0] == 0xFF && (b[1] & 0xE0) == 0xE0
}

/// 底层调用是阻塞的、且没有自带超时，卡住会把整句朗读挂死，所以放到独立线程 + 超时回收
fn synthesize_capped(text: &str, voice: &str, rate_pct: i32) -> Result<Vec<u8>, String> {
    let owned = (text.to_string(), voice.to_string(), rate_pct);
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let r = synthesize(&owned.0, &owned.1, owned.2);
        let _ = tx.send(r);
    });
    match rx.recv_timeout(Duration::from_secs(25)) {
        Ok(r) => r,
        Err(_) => Err("朗读超时：网络不通或接口已变更".to_string()),
    }
}

// ---------- commands ----------
/// 语速 0.5~1.5（1.0 为正常）→ SSML 的百分比
fn rate_pct_of(rate: f64) -> i32 {
    let r = if rate <= 0.0 { 1.0 } else { rate };
    (((r - 1.0) * 100.0).round() as i32).clamp(-50, 50)
}

#[tauri::command]
pub fn tts_voices() -> Vec<serde_json::Value> {
    PRESETS
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "group": p.group, "label": p.label, "voice": p.voice
            })
        })
        .collect()
}

/// 合成一句：命中缓存直接返回，否则联网合成后落盘
/// 返回 base64 音频（页面直接喂给 <audio>，省掉 asset 协议那一套配置）
#[tauri::command]
pub async fn tts_speak(
    text: String,
    preset: String,
    rate: f64,
) -> Result<serde_json::Value, String> {
    let p = preset_of(&preset).ok_or_else(|| format!("未知音色：{preset}"))?;
    let rate_pct = rate_pct_of(rate);
    let dir = cache_dir();
    let path = dir.join(format!("{}.mp3", cache_key(&text, p.voice, rate_pct)));

    let bytes: Vec<u8> = if path.exists() {
        match fs::read(&path) {
            Ok(b) if b.len() >= 1024 => b,
            _ => fetch(&text, p, rate_pct).await?,
        }
    } else {
        let fresh = fetch(&text, p, rate_pct).await?;
        fs::create_dir_all(&dir).ok();
        fs::write(&path, &fresh).ok();
        fresh
    };

    use base64::Engine;
    Ok(serde_json::json!({
        "audio": base64::engine::general_purpose::STANDARD.encode(&bytes),
        "bytes": bytes.len()
    }))
}

async fn fetch(text: &str, p: &Preset, rate_pct: i32) -> Result<Vec<u8>, String> {
    let owned = text.to_string();
    let voice = p.voice.to_string();
    tauri::async_runtime::spawn_blocking(move || synthesize_capped(&owned, &voice, rate_pct))
        .await
        .map_err(|e| format!("朗读线程异常：{e}"))?
}

/// 缓存占用（字节）
#[tauri::command]
pub fn tts_cache_size() -> u64 {
    let dir = cache_dir();
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                }
            }
        }
    }
    total
}

#[tauri::command]
pub fn tts_cache_clear() -> u64 {
    let dir = cache_dir();
    let mut freed = 0u64;
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("mp3") {
                freed += entry.metadata().map(|m| m.len()).unwrap_or(0);
                fs::remove_file(&path).ok();
            }
        }
    }
    freed
}
