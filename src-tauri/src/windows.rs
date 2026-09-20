// 窗口层：悬浮窗（透明/置顶/不占任务栏）+ 资料库窗口
// 窗口控制：建悬浮窗 / 改尺寸 / 显示隐藏 / 拖动落盘

use crate::state;
use serde_json::{json, Value};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

pub const WIDGET: &str = "widget";
pub const MANAGER: &str = "manager";

// 悬浮窗尺寸常量（逻辑像素）
const MIN_W: f64 = 220.0;
const MAX_W: f64 = 1400.0;
const MIN_H: f64 = 44.0;
const MAX_H: f64 = 900.0;
const DOCK_RIGHT: f64 = 16.0;
const DOCK_BOTTOM: f64 = 48.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// 窗口层独有的进程状态（书 / 配置那些在 state::App 里）
pub struct Ui {
    /// 最近一次程序化设置的边界，用来区分「自己改尺寸」和「用户拖动」
    pub last_bounds: Mutex<Option<Bounds>>,
    /// 首次自适应完成前先不显示，避免默认尺寸的闪现
    pub pending_show: Mutex<bool>,
    /// 拖拽防抖：轮询线程只在 seq 变化时落盘，等价于 600ms 防抖
    pub move_seq: AtomicU64,
    pub move_saved: AtomicU64,
    /// 悬浮窗当前是否可见。用 SW_SHOWNOACTIVATE 显示会绕过 tao 的内部状态，
    /// 所以不能问 win.is_visible()，只能自己记
    pub visible: AtomicBool,
}

impl Ui {
    pub fn new() -> Self {
        Ui {
            last_bounds: Mutex::new(None),
            pending_show: Mutex::new(false),
            move_seq: AtomicU64::new(0),
            move_saved: AtomicU64::new(0),
            visible: AtomicBool::new(false),
        }
    }
}

// ---------- 原生调用：整体不透明度 + 不抢焦点地显示 ----------
// Tauri 2.11 既没有 Window::set_opacity 也没有 showInactive，只能自己调 user32；
// 这两个恰好就是 Windows 原生的做法（SetLayeredWindowAttributes / SW_SHOWNOACTIVATE）。
#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn GetWindowLongW(hwnd: *mut c_void, index: i32) -> i32;
        fn SetWindowLongW(hwnd: *mut c_void, index: i32, value: i32) -> i32;
        fn SetLayeredWindowAttributes(hwnd: *mut c_void, key: u32, alpha: u8, flags: u32) -> i32;
        fn SetWindowPos(
            hwnd: *mut c_void,
            after: *mut c_void,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
        fn ShowWindow(hwnd: *mut c_void, cmd: i32) -> i32;
    }

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_LAYERED: i32 = 0x0008_0000;
    const LWA_ALPHA: u32 = 0x0000_0002;
    const SW_SHOWNOACTIVATE: i32 = 4;
    const SW_HIDE: i32 = 0;

    // 改完扩展位必须重刷一次框架，否则新样式要等下一次重画才生效（MSDN 的硬性要求）
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    const SWP_FLUSH: u32 =
        SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;

    /// opacity 取 0.35~1；1 时撤掉 LAYERED 扩展位，恢复普通窗口
    pub fn set_alpha(hwnd: *mut c_void, opacity: f64) {
        let v = opacity.clamp(0.05, 1.0);
        let want_layered = v < 1.0;
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            if want_layered != (style & WS_EX_LAYERED != 0) {
                let next = if want_layered {
                    style | WS_EX_LAYERED
                } else {
                    style & !WS_EX_LAYERED
                };
                SetWindowLongW(hwnd, GWL_EXSTYLE, next);
                SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_FLUSH);
            }
            if want_layered {
                SetLayeredWindowAttributes(hwnd, 0, (v * 255.0).round() as u8, LWA_ALPHA);
            }
        }
    }

    pub fn show_no_activate(hwnd: *mut c_void) {
        unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    }

    /// 隐藏也必须走原生 API：窗口是用 ShowWindow 显示的，tao 的 WindowFlags 里 VISIBLE 还停在
    /// false（窗口创建时就是隐藏的），它的 apply_diff 发现 flag 没变化会直接 return，
    /// 于是 win.hide() 是空操作——配置写了「已隐藏」，窗口却还挂在屏幕上。
    pub fn hide(hwnd: *mut c_void) {
        unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
}

#[cfg(not(windows))]
mod win32 {
    use std::ffi::c_void;
    pub fn set_alpha(_hwnd: *mut c_void, _opacity: f64) {}
    pub fn show_no_activate(_hwnd: *mut c_void) {}
    pub fn hide(_hwnd: *mut c_void) {}
}

fn hwnd_of(win: &WebviewWindow) -> Option<*mut c_void> {
    win.hwnd().ok().map(|h| h.0 as *mut c_void)
}

// ---------- 读配置的小工具 ----------
fn cfg_num(app: &AppHandle, key: &str, default: f64) -> f64 {
    state::read(app, |st| st.config.get(key).and_then(Value::as_f64).unwrap_or(default))
}
fn cfg_bool(app: &AppHandle, key: &str, default: bool) -> bool {
    state::read(app, |st| st.config.get(key).and_then(Value::as_bool).unwrap_or(default))
}
fn cfg_str(app: &AppHandle, key: &str) -> Option<String> {
    state::read(app, |st| st.config.get(key).and_then(Value::as_str).map(String::from))
}

fn clamp_w(v: f64) -> f64 {
    v.max(MIN_W).min(MAX_W).round()
}
fn clamp_h(v: f64) -> f64 {
    v.max(MIN_H).min(MAX_H).round()
}

/// 主屏工作区（扣掉任务栏），逻辑像素的 x / y / w / h
fn work_area(app: &AppHandle) -> (f64, f64, f64, f64) {
    match app.primary_monitor().ok().flatten() {
        Some(m) => {
            let sf = m.scale_factor();
            let wa = m.work_area();
            (
                wa.position.x as f64 / sf,
                wa.position.y as f64 / sf,
                wa.size.width as f64 / sf,
                wa.size.height as f64 / sf,
            )
        }
        None => (0.0, 0.0, 1920.0, 1080.0),
    }
}

fn initial_size(app: &AppHandle) -> (f64, f64) {
    if cfg_str(app, "widgetSizeMode").as_deref() == Some("manual") {
        let size = state::read(app, |st| st.config.get("widgetSize").cloned());
        if let Some(size) = size {
            let w = size.get("w").and_then(Value::as_f64).unwrap_or(MIN_W);
            let h = size.get("h").and_then(Value::as_f64).unwrap_or(MIN_H);
            return (clamp_w(w), clamp_h(h));
        }
    }
    // auto：先占位，渲染层量完内容马上收敛
    (clamp_w(cfg_num(app, "widgetMaxWidth", 520.0)), 90.0)
}

fn initial_pos(app: &AppHandle, w: f64, h: f64) -> (f64, f64) {
    let saved = state::read(app, |st| st.config.get("widgetPos").cloned());
    if let Some(pos) = saved {
        if let (Some(x), Some(y)) = (pos.get("x").and_then(Value::as_f64), pos.get("y").and_then(Value::as_f64)) {
            return (x, y);
        }
    }
    let (ax, ay, aw, ah) = work_area(app);
    (ax + aw - w - DOCK_RIGHT, ay + ah - h - DOCK_BOTTOM)
}

// ---------- 悬浮窗 ----------
pub fn create_widget(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let (w, h) = initial_size(app);
    let (x, y) = initial_pos(app, w, h);
    let win = WebviewWindowBuilder::new(app, WIDGET, WebviewUrl::App("widget/index.html".into()))
        .inner_size(w, h)
        .position(x, y)
        .title("英语悄悄学")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;

    {
        let ui = app.state::<Ui>();
        *ui.last_bounds.lock().unwrap() = Some(Bounds {
            x: x.round() as i32,
            y: y.round() as i32,
            w: w.round() as u32,
            h: h.round() as u32,
        });
    }
    let handle = app.clone();
    win.on_window_event(move |event| on_widget_event(&handle, event));
    Ok(win)
}

fn on_widget_event(app: &AppHandle, event: &WindowEvent) {
    // 位置记忆：只记真正被拖走的情况，自己改窗口尺寸不算
    let WindowEvent::Moved(pos) = event else { return };
    let win = match widget(app) {
        Some(win) => win,
        None => return,
    };
    let sf = win.scale_factor().unwrap_or(1.0);
    let x = (pos.x as f64 / sf).round() as i32;
    let y = (pos.y as f64 / sf).round() as i32;
    {
        let ui = app.state::<Ui>();
        let guard = ui.last_bounds.lock().unwrap();
        if let Some(last) = *guard {
            // 系统会把坐标对齐到物理像素，差 1~2px 不算拖动
            if (last.x - x).abs() <= 2 && (last.y - y).abs() <= 2 {
                return;
            }
        }
    }
    state::write(app, |st| {
        st.config["widgetPos"] = json!({ "x": x, "y": y });
        st.config["widgetDragged"] = Value::Bool(true);
    });
    app.state::<Ui>().move_seq.fetch_add(1, Ordering::SeqCst);
}

/// 拖一下会连发上百个 Moved，逐个写文件太浪费，交给这个轮询线程收尾
pub fn spawn_move_flusher(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(300));
        let ui = handle.state::<Ui>();
        let seq = ui.move_seq.load(Ordering::SeqCst);
        if seq == ui.move_saved.load(Ordering::SeqCst) {
            continue;
        }
        state::write(&handle, |st| st.save_config());
        ui.move_saved.store(seq, Ordering::SeqCst);
    });
}

/// 渲染层量出内容尺寸后由这里落窗：内容尺寸 ≠ 窗口尺寸时窗口会裁掉文字
pub fn set_widget_size(app: &AppHandle, width: f64, height: f64, anchor: Option<&str>) {
    let Some(win) = widget(app) else { return };
    let (w, h) = (clamp_w(width), clamp_h(height));
    let sf = win.scale_factor().unwrap_or(1.0);
    if let Ok(size) = win.inner_size() {
        if (w * sf).round() as u32 == size.width && (h * sf).round() as u32 == size.height {
            return;
        }
    }
    let (mut x, mut y) = match win.outer_position() {
        Ok(p) => {
            let p = p.to_logical::<f64>(sf);
            (p.x, p.y)
        }
        Err(_) => (0.0, 0.0),
    };
    // 未拖动过 = 停靠右下角，改宽高时右下角不动；手柄拖拽（anchor='topleft'）时左上角不动
    if anchor != Some("topleft") && !cfg_bool(app, "widgetDragged", false) {
        let (ax, ay, aw, ah) = work_area(app);
        x = ax + aw - w - DOCK_RIGHT;
        y = ay + ah - h - DOCK_BOTTOM;
    } else if anchor == Some("bottom") {
        // 查词卡从原文上方弹开：底边钉住、窗口向上生长，原文一行都不会动。
        // 只有拖过窗口才需要处理 —— 没拖过时走上面的右下角停靠，底边本来就是固定的。
        // 反过来若按默认来做，窗口顶边不动、向下长，卡片出现就会把句子整段顶下去。
        if let Ok(size) = win.outer_size() {
            let old_h = size.to_logical::<f64>(sf).height;
            if old_h > 0.0 {
                let (_, ay, _, _) = work_area(app);
                // 顶到屏幕上沿就不许再往上了，否则卡片被裁到屏幕外等于查了个寂寞
                y = (y + old_h - h).max(ay);
            }
        }
    }
    {
        let ui = app.state::<Ui>();
        *ui.last_bounds.lock().unwrap() = Some(Bounds {
            x: x.round() as i32,
            y: y.round() as i32,
            w: w.round() as u32,
            h: h.round() as u32,
        });
    }
    let _ = win.set_size(tauri::LogicalSize::new(w, h));
    let _ = win.set_position(tauri::LogicalPosition::new(x, y));
}

/// 渲染层报回内容尺寸后才真正显示，避免以占位尺寸闪一下
pub fn reveal_widget(app: &AppHandle) {
    {
        let ui = app.state::<Ui>();
        let mut pending = ui.pending_show.lock().unwrap();
        if !*pending {
            return;
        }
        *pending = false;
    }
    if cfg_bool(app, "widgetVisible", true) {
        show_widget(app);
    }
}

pub fn show_widget(app: &AppHandle) {
    let Some(win) = widget(app) else { return };
    clamp_into_screen(app);
    state::write(app, |st| {
        st.config["widgetVisible"] = Value::Bool(true);
        st.save_config();
    });
    match hwnd_of(&win) {
        Some(hwnd) => win32::show_no_activate(hwnd),
        None => {
            let _ = win.unminimize();
            let _ = win.show();
        }
    }
    app.state::<Ui>().visible.store(true, Ordering::SeqCst);
    // 以隐藏状态创建的窗口会带着 WS_EX_APPWINDOW，任务栏上会多出一个按钮；显示后补一次
    if let Err(e) = win.set_skip_taskbar(true) {
        log::warn!("set_skip_taskbar 失败：{e}");
    }
}

pub fn hide_widget(app: &AppHandle) {
    state::write(app, |st| {
        st.config["widgetVisible"] = Value::Bool(false);
        st.save_config();
    });
    state::send_cmd(app, "stop");
    app.state::<Ui>().visible.store(false, Ordering::SeqCst);
    if let Some(win) = widget(app) {
        match hwnd_of(&win) {
            Some(hwnd) => win32::hide(hwnd),
            None => {
                let _ = win.hide();
            }
        }
    }
}

pub fn toggle_widget(app: &AppHandle) -> bool {
    if widget(app).is_none() {
        return false;
    }
    if app.state::<Ui>().visible.load(Ordering::SeqCst) {
        hide_widget(app);
        false
    } else {
        show_widget(app);
        true
    }
}

/// 拔过副屏 / 改过分辨率后窗口可能跑到屏幕外，唤回时先拉回可见区域
fn clamp_into_screen(app: &AppHandle) {
    let Some(win) = widget(app) else { return };
    let sf = win.scale_factor().unwrap_or(1.0);
    let (ax, ay, aw, ah) = work_area(app);
    let p = match win.outer_position() {
        Ok(p) => p.to_logical::<f64>(sf),
        Err(_) => return,
    };
    let x = p.x.max(ax).min(ax + aw - 80.0);
    let y = p.y.max(ay).min(ay + ah - 40.0);
    if (x - p.x).abs() > 0.5 || (y - p.y).abs() > 0.5 {
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    }
}

pub fn widget(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(WIDGET)
}

/// 切到「固定尺寸」时把当前窗口尺寸记下来，避免一切换就跳成内容大小
pub fn current_widget_size(app: &AppHandle) -> Option<(u32, u32)> {
    let win = widget(app)?;
    let size = win.inner_size().ok()?;
    let sf = win.scale_factor().unwrap_or(1.0);
    Some((
        (size.width as f64 / sf).round() as u32,
        (size.height as f64 / sf).round() as u32,
    ))
}

// ---------- 资料库窗口 ----------
pub fn show_manager(app: &AppHandle) {
    if app.get_webview_window(MANAGER).is_none() {
        create_manager(app).ok();
    }
    if let Some(win) = app.get_webview_window(MANAGER) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        apply_manager_opacity(app);
        // show() 改 VISIBLE 标志 → tao 的 apply_diff 用自己缓存的 ex-style 覆盖 GWL_EXSTYLE，
        // 把刚设好的 WS_EX_LAYERED 抹掉，窗口就变全不透明。而且那次覆盖是 PostMessage 异步执行的，
        // 所以这里再排一条主线程任务，顺序排在它后面，保证最终生效的是我们的 alpha。
        let late = app.clone();
        let _ = app.run_on_main_thread(move || apply_manager_opacity(&late));
    }
    state::push_manager(app);
}

fn create_manager(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let win = WebviewWindowBuilder::new(app, MANAGER, WebviewUrl::App("manager/index.html".into()))
        .inner_size(880.0, 640.0)
        .title("英语悄悄学 · 资料库")
        .resizable(true)
        .visible(false)
        .build()?;
    let handle = app.clone();
    win.on_window_event(move |event| match event {
        // 关闭 = 收进托盘，不退出
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            if let Some(win) = handle.get_webview_window(MANAGER) {
                let _ = win.hide();
            }
        }
        // 最大化 / 还原 / 系统改尺寸都会再走一次 tao 的 apply_diff，同样抹掉 LAYERED，补回来
        WindowEvent::Resized(_) => apply_manager_opacity(&handle),
        _ => {}
    });
    Ok(win)
}

pub fn apply_manager_opacity(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(MANAGER) {
        let v = cfg_num(app, "managerOpacity", 0.92);
        let opacity = if v.is_finite() { v.clamp(0.35, 1.0) } else { 1.0 };
        if let Some(hwnd) = hwnd_of(&win) {
            win32::set_alpha(hwnd, opacity);
        }
    }
}

// ---------- 命令 ----------
#[tauri::command]
pub fn widget_resize(app: AppHandle, width: Option<f64>, height: Option<f64>, anchor: Option<String>) {
    if let (Some(w), Some(h)) = (width, height) {
        set_widget_size(&app, w, h, anchor.as_deref());
        reveal_widget(&app);
    }
}

#[tauri::command]
pub fn widget_toggle(app: AppHandle) -> Value {
    json!({ "visible": toggle_widget(&app) })
}

#[tauri::command]
pub fn widget_hide(app: AppHandle) {
    hide_widget(&app);
}

#[tauri::command]
pub fn widget_drag(app: AppHandle) {
    if let Some(win) = widget(&app) {
        let _ = win.start_dragging();
    }
}

// ---------- 供 state.rs 回调 ----------
pub fn mark_pending_show(app: &AppHandle, value: bool) {
    let ui = app.state::<Ui>();
    *ui.pending_show.lock().unwrap() = value;
}
