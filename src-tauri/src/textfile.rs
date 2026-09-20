// 选文件 + 解码：对应 core/importer.js 的 readText()，规则要逐条对齐
// 放在 Rust 侧做，是因为 WebView2 里没有 Node 的 Buffer，且这样能单测每条编码分支

use encoding_rs::{GB18030, UTF_16BE, UTF_16LE};
use serde_json::Value;
use std::fs;
use tauri_plugin_dialog::DialogExt;

pub fn decode_text(b: &[u8]) -> Result<String, String> {
    if b.is_empty() {
        return Err("文件是空的。".into());
    }
    let mut text = if b.starts_with(&[0xFF, 0xFE]) {
        UTF_16LE.decode_without_bom_handling(&b[2..]).0.into_owned()
    } else if b.starts_with(&[0xFE, 0xFF]) {
        UTF_16BE.decode_without_bom_handling(&b[2..]).0.into_owned()
    } else if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        utf8_strict(&b[3..])?
    } else {
        // 严格 UTF-8 成功则直接采用；失败时按中文 Windows 文本尝试 GB18030
        utf8_strict(b).unwrap_or_else(|_| GB18030.decode_without_bom_handling(b).0.into_owned())
    };
    if text.starts_with('\u{FEFF}') {
        text.remove(0);
    }
    if text.contains('\u{0}') {
        return Err("文件包含无效的二进制内容，无法作为文本导入。".into());
    }
    let bad = text.matches('\u{FFFD}').count();
    if bad > 10 && bad as f64 / text.chars().count().max(1) as f64 > 0.002 {
        return Err("文件里出现大量乱码，请检查文件编码后再导入。".into());
    }
    if text.trim().is_empty() {
        return Err("文件是空的。".into());
    }
    Ok(text)
}

fn utf8_strict(b: &[u8]) -> Result<String, String> {
    std::str::from_utf8(b)
        .map(|s| s.to_string())
        .map_err(|_| "无法识别文件编码，请另存为 UTF-8 后再导入。".to_string())
}

// 前端 window.api.readFile() 走这里：弹原生对话框 → 读字节 → 解码成字符串
#[tauri::command]
pub fn pick_txt(app: tauri::AppHandle) -> Result<Value, String> {
    let picked = app
        .dialog()
        .file()
        .set_title("选择文本文件（支持 UTF-8 / GBK / UTF-16）")
        .add_filter("文本文档", &["txt", "md"])
        .blocking_pick_file();
    let Some(picked) = picked else {
        return Ok(serde_json::json!({ "canceled": true }));
    };
    let path = picked.into_path().map_err(|_| "选中的路径不是本地文件。".to_string())?;
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("未命名").to_string();
    let bytes = fs::read(&path).map_err(crate::store::err)?;
    let text = decode_text(&bytes)?;
    Ok(serde_json::json!({ "name": name, "text": text }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_utf8() {
        let bytes = "Once upon a time.\n从前有个人。".as_bytes().to_vec();
        assert_eq!(decode_text(&bytes).unwrap(), "Once upon a time.\n从前有个人。");
    }

    #[test]
    fn utf8_with_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend("Hello.".as_bytes());
        assert_eq!(decode_text(&bytes).unwrap(), "Hello.");
    }

    // 中文 txt 最常见的坑：GBK/GB18030，严格 UTF-8 解会失败，必须退到 GB18030
    #[test]
    fn gb18030_fallback_for_chinese_txt() {
        let (gbk, _, _) = encoding_rs::GBK.encode("你好，世界！这是中文课文。");
        let out = decode_text(&gbk).unwrap();
        assert_eq!(out, "你好，世界！这是中文课文。");
    }

    #[test]
    fn utf16_le_and_be() {
        let mut le = vec![0xFF, 0xFE];
        le.extend("Hello 你好".encode_utf16().flat_map(|c| c.to_le_bytes()));
        assert_eq!(decode_text(&le).unwrap(), "Hello 你好");

        let mut be = vec![0xFE, 0xFF];
        be.extend("Hello 你好".encode_utf16().flat_map(|c| c.to_be_bytes()));
        assert_eq!(decode_text(&be).unwrap(), "Hello 你好");
    }

    #[test]
    fn rejects_binary_empty_and_garbage() {
        let mut nul = "Hello.".as_bytes().to_vec();
        nul.push(0);
        nul.extend("world".as_bytes());
        assert_eq!(decode_text(&nul).unwrap_err(), "文件包含无效的二进制内容，无法作为文本导入。");

        assert_eq!(decode_text(&[]).unwrap_err(), "文件是空的。");
        assert_eq!(decode_text("   \n ".as_bytes()).unwrap_err(), "文件是空的。");

        // 大段非法字节：解出来的替换符超过阈值，直接让用户另存为 UTF-8，而不是导入一堆乱码
        // 从 1 开始：含 0x00 会先被上面的二进制检查拦掉，测不到这个阈值
        let garbage: Vec<u8> = (1..=0xFFu8).cycle().take(40000).collect();
        let err = decode_text(&garbage).unwrap_err();
        assert!(err.contains("乱码") || err.contains("编码"), "实际：{err}");
    }
}
