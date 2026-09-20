// 应用装配入口：装插件、建窗口、建托盘、注册快捷键

mod dict;
mod hotkeys;
mod state;
mod store;
mod textfile;
mod tts;
mod windows;

use serde_json::Value;
use std::time::Duration;
use tauri::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

type Check = tauri::menu::CheckMenuItem<tauri::Wry>;

/// 托盘里「开机自启」的勾选态要能改，所以把条目存进 managed state
struct TrayMenu {
    autostart: Check,
}

const TRAY_ICON: &[u8] = include_bytes!("../icons/32x32.png");

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // 单实例必须最先注册：第二次双击 exe 时把资料库叫到前台，而不是再开一份托盘
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            windows::show_manager(app);
        }));
    }

    let log_dir = store::data_root().join("logs");
    builder
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Folder {
                        path: log_dir,
                        file_name: Some("app".into()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                ])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(Default::default(), None))
        .invoke_handler(tauri::generate_handler![
            state::state_get,
            state::index_set,
            state::config_set,
            state::book_open,
            state::book_delete,
            state::book_rename,
            state::book_add,
            store::list_books,
            store::read_book,
            textfile::pick_txt,
            dict::dict_lookup,
            dict::dict_cache_size,
            dict::dict_cache_clear,
            tts::tts_speak,
            tts::tts_voices,
            tts::tts_cache_size,
            tts::tts_cache_clear,
            hotkeys::hotkey_set,
            windows::widget_resize,
            windows::widget_toggle,
            windows::widget_hide,
            windows::widget_drag
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let root = store::data_root();
            let root_str = root.display().to_string();
            store::ensure_root(&root)?;

            let mut app_state = state::App::boot(root);
            app_state.seed_demo_book();
            let current = app_state
                .config
                .get("currentBookId")
                .and_then(Value::as_str)
                .map(String::from);
            let loaded = match current {
                Some(id) => app_state.load_book(&id, false),
                None => false,
            };
            if !loaded {
                app_state.open_first_book();
            }
            // 「隐藏悬浮窗」只对当前会话有效：启动一律显示。
            // 隐藏状态一旦落盘，下次启动窗口就完全没有踪影，而找回入口只有托盘图标和全局快捷键，
            // 用户未必知道，结果就是「程序在跑，但桌面上什么都看不到」。
            app_state.config["widgetVisible"] = Value::Bool(true);
            app_state.save_config();
            app.manage(state::Hub::new(app_state));
            app.manage(windows::Ui::new());
            app.manage(dict::Cache::load());

            windows::create_widget(&handle)?;
            create_tray(&handle)?;
            let fails = hotkeys::register_all(&handle);
            windows::spawn_move_flusher(&handle);

            // 先等渲染层量完内容尺寸再显示，避免以占位尺寸闪一下；1200ms 是兜底
            windows::mark_pending_show(&handle, true);
            let late = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1200));
                windows::reveal_widget(&late);
            });
            log::info!("启动完成 root={root_str} 快捷键失败={}", fails.len());
            warn_hotkeys(&handle, fails);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("启动失败：先跑 npm run web 生成 web/")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                state::flush(app);
            }
        });
}

fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let toggle = MenuItem::with_id(app, "toggle", "显示 / 隐藏悬浮窗", true, None::<MenuId>)?;
    let open = MenuItem::with_id(app, "open", "打开资料库", true, None::<MenuId>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let autostart = Check::with_id(app, "autostart", "开机自启", true, enabled, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<MenuId>)?;
    let menu = Menu::with_items(app, &[&toggle, &open, &sep, &autostart, &quit])?;
    let icon = tauri::image::Image::from_bytes(TRAY_ICON)?;

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("英语悄悄学")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            // 左键单击 = 显示/隐藏，右键才出菜单
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                windows::toggle_widget(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => {
                windows::toggle_widget(app);
            }
            "open" => windows::show_manager(app),
            "autostart" => {
                let al = app.autolaunch();
                let now = al.is_enabled().unwrap_or(false);
                let done = if now { al.disable() } else { al.enable() };
                if done.is_ok() {
                    if let Some(items) = app.try_state::<TrayMenu>() {
                        let _ = items.autostart.set_checked(!now);
                    }
                }
            }
            "quit" => {
                state::flush(app);
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    app.manage(TrayMenu { autostart });
    Ok(())
}

/// 快捷键被占用时提示用户：只警告，不阻塞启动
fn warn_hotkeys(app: &AppHandle, fails: Vec<String>) {
    if fails.is_empty() {
        return;
    }
    let msg = format!(
        "部分全局快捷键被其他程序占用：\n{}\n\n可在「资料库 → 设置」里更换。",
        fails.join("\n")
    );
    app.dialog()
        .message(msg)
        .title("英语悄悄学")
        .kind(MessageDialogKind::Warning)
        .show(|_| {});
}
