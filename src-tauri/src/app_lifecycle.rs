use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    ActivationPolicy, Manager,
};
use tauri_plugin_opener::OpenerExt;

pub fn configure(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItem::with_id(app, "show", "打开主界面", true, None::<&str>)?;
    let website_item = MenuItem::with_id(app, "website", "打开官方网站", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, Some("CmdOrCtrl+Q"))?;
    let tray_menu = Menu::with_items(app, &[&show_item, &website_item, &separator, &quit_item])?;
    let app_handle = app.clone();
    let mut tray = TrayIconBuilder::new().menu(&tray_menu).tooltip("DesignBridge");
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "website" => {
                let _ = app.opener().open_url(
                    "https://github.com/molast/designbridge",
                    None::<&str>,
                );
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |_tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(&app_handle);
            }
        })
        .build(app)?;

    if let Some(window) = app.get_webview_window("main") {
        let window_for_close = window.clone();
        let app_for_close = app.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = app_for_close.set_activation_policy(ActivationPolicy::Accessory);
                let _ = window_for_close.hide();
            }
        });
    }
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
