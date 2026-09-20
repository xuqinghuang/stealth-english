// 全局快捷键：注册、热更新、收集注册失败的清单
// 语义要点：注册失败 = 被别的程序占用，配置要能回滚

use crate::{state, windows};
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

const ACTIONS: [&str; 4] = ["toggle", "prev", "next", "play"];

fn label(action: &str) -> String {
    match action {
        "toggle" => "显示/隐藏".into(),
        "prev" => "上一句".into(),
        "next" => "下一句".into(),
        "play" => "播放/暂停".into(),
        other => other.into(),
    }
}

fn fire(app: &AppHandle, action: &str) {
    log::info!("快捷键 {action}");
    match action {
        "toggle" => {
            windows::toggle_widget(app);
        }
        "next" => {
            state::write(app, |st| st.step(1));
            state::push(app, "nav");
        }
        "prev" => {
            state::write(app, |st| st.step(-1));
            state::push(app, "nav");
        }
        "play" => state::send_cmd(app, "playpause"),
        _ => {}
    }
}

fn handler(action: &str) -> impl Fn(&AppHandle, &Shortcut, ShortcutEvent) + Send + Sync + 'static {
    let action = action.to_string();
    move |app: &AppHandle, _shortcut: &Shortcut, event: ShortcutEvent| {
        if event.state == ShortcutState::Pressed {
            fire(app, &action);
        }
    }
}

fn accel_of(app: &AppHandle, action: &str) -> Option<String> {
    state::read(app, |st| {
        st.config
            .get("hotkeys")
            .and_then(|h| h.get(action))
            .and_then(Value::as_str)
            .map(String::from)
    })
}

/// 全部重注册，返回注册不上的清单（先 unregisterAll 再逐个 register）
pub fn register_all(app: &AppHandle) -> Vec<String> {
    let _ = app.global_shortcut().unregister_all();
    let mut fails = Vec::new();
    for action in ACTIONS {
        let accel = match accel_of(app, action) {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };
        if let Err(e) = app.global_shortcut().on_shortcut(accel.as_str(), handler(action)) {
            log::warn!("快捷键 {accel} 注册失败：{e}");
            fails.push(format!("{accel}（{}）", label(action)));
        }
    }
    fails
}

#[tauri::command]
pub fn hotkey_set(app: AppHandle, action: String, value: String) -> Result<Value, String> {
    let old = accel_of(&app, &action);
    if old.as_deref() == Some(value.as_str()) {
        return Ok(json!({ "ok": true }));
    }
    // 先注册新的，成功了才撤旧的：中间不会出现两个都不能用的空档
    if let Err(e) = app.global_shortcut().on_shortcut(value.as_str(), handler(&action)) {
        log::warn!("快捷键 {value} 注册失败：{e}");
        return Ok(json!({ "ok": false, "error": format!("「{value}」不可用或已被其他程序占用") }));
    }
    if let Some(old) = old {
        let _ = app.global_shortcut().unregister(old.as_str());
    }
    state::write(&app, |st| {
        st.config["hotkeys"][action.as_str()] = json!(value);
        st.save_config();
    });
    Ok(json!({ "ok": true }))
}
