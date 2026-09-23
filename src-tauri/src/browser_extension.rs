use super::*;

pub(crate) fn copy_directory(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|error| format!("无法创建目录 {}：{error}", target.display()))?;
    for entry in std::fs::read_dir(source)
        .map_err(|error| format!("无法读取目录 {}：{error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("无法读取扩展文件：{error}"))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("无法检查扩展文件：{error}"))?;
        if file_type.is_dir() {
            copy_directory(&source_path, &target_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&source_path, &target_path).map_err(|error| {
                format!(
                    "无法复制扩展文件 {} 到 {}：{error}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(crate) fn build_debug_browser_host(project_root: &Path, debug_host: &Path) -> Result<(), String> {
    let output = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(project_root.join("src-tauri/Cargo.toml"))
        .arg("--bin")
        .arg("designbridge-browser-host")
        .current_dir(project_root)
        .output()
        .map_err(|error| format!("无法构建浏览器通信程序：{error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "无法构建浏览器通信程序".to_string()
        } else {
            detail
        });
    }
    if !debug_host.is_file() {
        return Err("浏览器通信程序构建完成但未找到运行文件".to_string());
    }
    Ok(())
}

pub(crate) fn browser_extension_sources(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "无法确定 DesignBridge 项目目录".to_string())?;
    let resource_root = app
        .path()
        .resource_dir()
        .map_err(|error| format!("无法确定客户端资源目录：{error}"))?
        .join("designbridge-installer");
    let project_extension = project_root.join("browser-extension");
    let bundled_extension = resource_root.join("browser-extension");
    let extension_source =
        if cfg!(debug_assertions) && project_extension.join("manifest.json").is_file() {
            project_extension
        } else if bundled_extension.join("manifest.json").is_file() {
            bundled_extension
        } else {
            project_extension
        };
    if !extension_source.join("manifest.json").is_file() {
        return Err("客户端中缺少浏览器扩展文件，请重新安装最新版客户端".to_string());
    }

    let debug_host = project_root
        .join("src-tauri/target/debug")
        .join(browser_host_binary_name());
    if cfg!(debug_assertions) {
        build_debug_browser_host(&project_root, &debug_host)?;
        return Ok((extension_source, debug_host));
    }

    let bundled_host = resource_root
        .join("browser-host/bin")
        .join(browser_host_binary_name());
    if bundled_host.is_file() {
        return Ok((extension_source, bundled_host));
    }

    build_debug_browser_host(&project_root, &debug_host)?;
    Ok((extension_source, debug_host))
}

pub(crate) fn browser_host_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "designbridge-browser-host.exe"
    } else {
        "designbridge-browser-host"
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn install_browser_native_manifests(host_path: &Path) -> Result<Vec<String>, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法确定当前用户目录".to_string())?;
    #[cfg(target_os = "macos")]
    let browser_roots = [
        ("Google Chrome", "Library/Application Support/Google/Chrome"),
        (
            "Microsoft Edge",
            "Library/Application Support/Microsoft Edge",
        ),
        (
            "Brave",
            "Library/Application Support/BraveSoftware/Brave-Browser",
        ),
        ("Chromium", "Library/Application Support/Chromium"),
    ];
    #[cfg(target_os = "linux")]
    let browser_roots = [
        ("Google Chrome", ".config/google-chrome"),
        ("Microsoft Edge", ".config/microsoft-edge"),
        ("Brave", ".config/BraveSoftware/Brave-Browser"),
        ("Chromium", ".config/chromium"),
    ];
    let manifest = serde_json::to_vec_pretty(&serde_json::json!({
        "name": BROWSER_NATIVE_HOST_NAME,
        "description": "DesignBridge browser extension native host",
        "path": host_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{BROWSER_EXTENSION_ID}/")],
    }))
    .map_err(|error| format!("无法生成浏览器通信配置：{error}"))?;
    let mut configured = Vec::new();
    for (browser, relative_root) in browser_roots {
        let directory = home.join(relative_root).join("NativeMessagingHosts");
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("无法创建 {browser} 配置目录：{error}"))?;
        std::fs::write(
            directory.join(format!("{BROWSER_NATIVE_HOST_NAME}.json")),
            &manifest,
        )
        .map_err(|error| format!("无法写入 {browser} 通信配置：{error}"))?;
        configured.push(browser.to_string());
    }
    Ok(configured)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn install_browser_native_manifests(_host_path: &Path) -> Result<Vec<String>, String> {
    Err("当前版本的浏览器扩展安装仅支持 macOS 和 Linux".to_string())
}

#[tauri::command]
pub(crate) fn install_browser_extension(
    app: tauri::AppHandle,
) -> Result<BrowserExtensionInstallResult, String> {
    let (extension_source, host_source) = browser_extension_sources(&app)?;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?;
    let extension_target = app_data.join("browser-extension");
    let host_target = app_data
        .join("browser-host")
        .join(browser_host_binary_name());
    if extension_target.exists() {
        std::fs::remove_dir_all(&extension_target)
            .map_err(|error| format!("无法更新旧版浏览器扩展：{error}"))?;
    }
    copy_directory(&extension_source, &extension_target)?;
    if let Some(parent) = host_target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建浏览器通信目录：{error}"))?;
    }
    let host_temp = host_target.with_file_name(format!(
        ".{}.tmp-{}",
        browser_host_binary_name(),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&host_temp);
    std::fs::copy(&host_source, &host_temp)
        .map_err(|error| format!("无法安装浏览器通信程序：{error}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(&host_temp, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("无法设置浏览器通信程序权限：{error}"))?;
    #[cfg(windows)]
    if host_target.exists() {
        std::fs::remove_file(&host_target)
            .map_err(|error| format!("无法替换旧版浏览器通信程序：{error}"))?;
    }
    if let Err(error) = std::fs::rename(&host_temp, &host_target) {
        let _ = std::fs::remove_file(&host_temp);
        return Err(format!("无法启用浏览器通信程序：{error}"));
    }
    let configured_browsers = install_browser_native_manifests(&host_target)?;

    Ok(BrowserExtensionInstallResult {
        extension_path: extension_target.to_string_lossy().into_owned(),
        extension_id: BROWSER_EXTENSION_ID.to_string(),
        configured_browsers,
    })
}

pub(crate) fn browser_extension_manager_target(browser: &str) -> Result<(&'static str, &'static str), String> {
    match browser {
        "chrome" => Ok(("Google Chrome", "chrome://extensions/")),
        "edge" => Ok(("Microsoft Edge", "edge://extensions/")),
        "brave" => Ok(("Brave Browser", "brave://extensions/")),
        "chromium" => Ok(("Chromium", "chrome://extensions/")),
        _ => Err("不支持的浏览器".to_string()),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn browser_profile_root(browser: &str) -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "无法确定当前用户目录".to_string())?;
    #[cfg(target_os = "macos")]
    let relative = match browser {
        "chrome" => "Library/Application Support/Google/Chrome",
        "edge" => "Library/Application Support/Microsoft Edge",
        "brave" => "Library/Application Support/BraveSoftware/Brave-Browser",
        "chromium" => "Library/Application Support/Chromium",
        _ => return Err("不支持的浏览器".to_string()),
    };
    #[cfg(target_os = "linux")]
    let relative = match browser {
        "chrome" => ".config/google-chrome",
        "edge" => ".config/microsoft-edge",
        "brave" => ".config/BraveSoftware/Brave-Browser",
        "chromium" => ".config/chromium",
        _ => return Err("不支持的浏览器".to_string()),
    };
    Ok(home.join(relative))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn extension_setting_state(settings: &serde_json::Value) -> (bool, bool) {
    let state_enabled = settings.get("state").and_then(|state| state.as_i64()) != Some(0);
    let has_disable_reason = settings
        .get("disable_reasons")
        .and_then(|reasons| reasons.as_array())
        .map(|reasons| !reasons.is_empty())
        .unwrap_or(false);
    let enabled = state_enabled && !has_disable_reason;
    let auto_capture_ready = enabled
        && settings
            .get("active_permissions")
            .and_then(|permissions| permissions.get("scriptable_host"))
            .and_then(|hosts| hosts.as_array())
            .map(|hosts| {
                hosts.iter().any(|host| {
                    host.as_str()
                        .map(|host| host.contains("lanhuapp.com") || host.contains("lanhu.com"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
    (enabled, auto_capture_ready)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn extension_profile_state(profile_root: &Path) -> (bool, bool, bool) {
    let mut profile_dirs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(profile_root) {
        for entry in entries.flatten() {
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                profile_dirs.push(entry.path());
            }
        }
    }

    let mut installed = false;
    let mut enabled = false;
    let mut auto_capture_ready = false;
    for profile_dir in profile_dirs {
        for file_name in ["Preferences", "Secure Preferences"] {
            let Ok(bytes) = std::fs::read(profile_dir.join(file_name)) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let Some(settings) = value
                .get("extensions")
                .and_then(|extensions| extensions.get("settings"))
                .and_then(|settings| settings.get(BROWSER_EXTENSION_ID))
            else {
                continue;
            };

            installed = true;
            let (setting_enabled, setting_auto_capture_ready) = extension_setting_state(settings);
            enabled |= setting_enabled;
            auto_capture_ready |= setting_auto_capture_ready;
        }
    }
    (installed, enabled, auto_capture_ready)
}

pub(crate) fn browser_extension_status_value(
    browser: &str,
    runtime: &CaptureRuntime,
) -> Result<BrowserExtensionStatus, String> {
    let (browser_name, _) = browser_extension_manager_target(browser)?;
    let heartbeat = runtime
        .inner
        .lock()
        .ok()
        .and_then(|inner| inner.browser_heartbeats.get(browser).cloned())
        .filter(|heartbeat| heartbeat.received_at.elapsed() <= BROWSER_HEARTBEAT_TIMEOUT);
    let connected = heartbeat.is_some();
    let extension_version = heartbeat.map(|heartbeat| heartbeat.extension_version);
    let version_current = extension_version.as_deref() == Some(BROWSER_EXTENSION_VERSION);

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let (installed, enabled, auto_capture_ready, native_host_installed) = {
        let profile_root = browser_profile_root(browser)?;
        let (installed, enabled, auto_capture_ready) = extension_profile_state(&profile_root);
        let native_host_installed = profile_root
            .join("NativeMessagingHosts")
            .join(format!("{BROWSER_NATIVE_HOST_NAME}.json"))
            .is_file();
        (
            installed,
            enabled,
            auto_capture_ready,
            native_host_installed,
        )
    };

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let (installed, enabled, auto_capture_ready, native_host_installed) =
        (false, false, false, false);

    Ok(BrowserExtensionStatus {
        browser: browser_name.to_string(),
        installed: installed || connected,
        enabled: enabled || connected,
        auto_capture_ready: auto_capture_ready || connected,
        native_host_installed: native_host_installed || connected,
        connected,
        version_current,
        extension_version,
    })
}

#[tauri::command]
pub(crate) fn browser_extension_status(
    browser: String,
    runtime: tauri::State<'_, CaptureRuntime>,
) -> Result<BrowserExtensionStatus, String> {
    browser_extension_status_value(&browser, &runtime)
}

pub(crate) fn open_browser_url(browser: &str, url: &str) -> Result<(), String> {
    let (application, _) = browser_extension_manager_target(browser)?;

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/bin/open")
            .arg("-a")
            .arg(application)
            .arg(url)
            .output()
            .map_err(|error| format!("无法打开 {application}：{error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("无法打开 {application}，请确认浏览器已经安装")
        } else {
            detail
        });
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Stdio;

        let candidates: &[&str] = match browser {
            "chrome" => &["google-chrome", "google-chrome-stable"],
            "edge" => &["microsoft-edge", "microsoft-edge-stable"],
            "brave" => &["brave-browser", "brave"],
            "chromium" => &["chromium", "chromium-browser"],
            _ => &[],
        };
        for executable in candidates {
            if Command::new(executable)
                .arg(url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok()
            {
                return Ok(());
            }
        }
        return Err(format!("无法打开 {application}，请确认浏览器已经安装"));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (application, url);
        Err("当前版本暂不支持自动打开系统浏览器".to_string())
    }
}

#[tauri::command]
pub(crate) fn open_browser_extension_manager(browser: String) -> Result<(), String> {
    let (_, url) = browser_extension_manager_target(&browser)?;
    open_browser_url(&browser, url)
}

pub(crate) fn browser_capture_url(mut url: Url, capture_id: &str) -> Url {
    let retained = url
        .query_pairs()
        .filter(|(key, _)| key != "designbridge_capture")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    url.query_pairs_mut()
        .extend_pairs(retained)
        .append_pair("designbridge_capture", capture_id);
    url
}

#[tauri::command]
pub(crate) fn start_browser_extension_capture(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, CaptureRuntime>,
    url: String,
    browser: String,
) -> Result<String, String> {
    let parsed_url = lanhu_url(&url)?;
    let route = lanhu_route(&parsed_url);
    if route.project_id.is_none() || route.image_id.is_none() {
        return Err("请粘贴具体的蓝湖设计稿链接".to_string());
    }

    let status = browser_extension_status_value(&browser, &runtime)?;
    if !status.installed || !status.enabled {
        return Err(format!(
            "未检测到 {} 中已启用的 DesignBridge 扩展，请先点击“安装浏览器扩展”",
            status.browser
        ));
    }
    if !status.auto_capture_ready {
        return Err(format!(
            "{} 中仍是旧版 DesignBridge 扩展，请在扩展管理页点击“重新加载”",
            status.browser
        ));
    }
    if !status.native_host_installed {
        return Err("浏览器通信程序尚未安装，请重新点击“安装浏览器扩展”".to_string());
    }
    if !status.connected {
        return Err(format!(
            "{} 中的 DesignBridge 扩展未连接当前客户端，请重新加载扩展后重试",
            status.browser
        ));
    }
    if !status.version_current {
        return Err(format!(
            "浏览器扩展版本不是 {BROWSER_EXTENSION_VERSION}，请更新并重新加载扩展"
        ));
    }

    let capture_id = capture_id();
    reserve_capture(&runtime, &capture_id, url.trim())?;
    let target_url = browser_capture_url(parsed_url, &capture_id);
    if let Err(error) = open_browser_url(&browser, target_url.as_str()) {
        take_source(&runtime, &capture_id);
        return Err(error);
    }

    emit_progress(
        &app,
        &capture_id,
        "authorize",
        "已打开系统浏览器，等待扩展读取登录状态…",
        8,
    );

    let timeout_app = app.clone();
    let timeout_capture_id = capture_id.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        if expire_pending_browser_capture(
            &timeout_app.state::<CaptureRuntime>(),
            &timeout_capture_id,
        ) {
            emit_failure(
                &timeout_app,
                &timeout_capture_id,
                "浏览器扩展未响应，请确认扩展已刷新且当前浏览器已登录蓝湖",
            );
        }
    });

    Ok(capture_id)
}
