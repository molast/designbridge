use super::*;

#[tauri::command]
pub(crate) async fn list_saved_captures(app: tauri::AppHandle) -> Result<Vec<CaptureResult>, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?
        .join("captures");
    let mut captures = Vec::new();
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(captures),
        Err(error) => return Err(format!("无法读取抓取历史：{error}")),
    };

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("无法读取抓取历史：{error}"))?
    {
        let metadata_path: PathBuf = entry.path().join("capture.json");
        let Ok(bytes) = tokio::fs::read(metadata_path).await else {
            continue;
        };
        if let Ok(mut capture) = serde_json::from_slice::<CaptureResult>(&bytes) {
            normalize_legacy_layer_frames(&mut capture.layers);
            normalize_legacy_layer_radius(&mut capture.layers);
            // Older captures treated metadata-only sibling pages as failures.
            // Recompute from the actual per-page error field when loading them.
            capture.failed_count = failed_design_count(&capture.designs);
            captures.push(capture);
        }
    }

    captures.sort_by_key(|capture| Reverse(capture.captured_at));
    captures.truncate(30);
    Ok(captures)
}

#[tauri::command]
pub(crate) async fn check_for_update() -> Result<AppUpdateInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent(format!("DesignBridge/{APP_VERSION}"))
        .build()
        .map_err(|error| format!("无法创建更新检查客户端：{error}"))?;
    let release_body = client
        .get("https://api.github.com/repos/molast/designbridge/releases/latest")
        .send()
        .await
        .map_err(|error| format!("无法检查更新：{error}"))?
        .error_for_status()
        .map_err(|error| format!("无法读取最新版本：{error}"))?
        .text()
        .await
        .map_err(|error| format!("无法读取最新版本内容：{error}"))?;
    let release = serde_json::from_str::<GithubRelease>(&release_body)
        .map_err(|error| format!("无法解析最新版本：{error}"))?;
    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let available = !release.draft
        && !release.prerelease
        && version_tuple(&latest_version) > version_tuple(APP_VERSION);
    Ok(AppUpdateInfo {
        available,
        current_version: APP_VERSION.to_string(),
        latest_version,
        release_url: release.html_url,
        release_name: release.name.unwrap_or_default(),
    })
}

pub(crate) fn valid_capture_id(capture_id: &str) -> bool {
    !capture_id.is_empty()
        && capture_id.len() <= 80
        && capture_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

#[tauri::command]
pub(crate) async fn export_slice_variants(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, CaptureRuntime>,
    request: ExportSliceRequest,
) -> Result<ExportSliceResult, String> {
    if !valid_capture_id(&request.capture_id) {
        return Err("无效的抓取记录 ID".to_string());
    }
    let format = request.format.to_ascii_lowercase();
    if !matches!(format.as_str(), "png" | "jpg" | "webp") {
        return Err("仅支持 PNG、JPG 和 WEBP 格式".to_string());
    }
    let platform = request.platform.to_ascii_lowercase();
    if !matches!(platform.as_str(), "android" | "ios") {
        return Err("仅支持 Android 和 iOS 平台".to_string());
    }

    let mut seen_scales = HashSet::new();
    let targets = request
        .scales
        .iter()
        .filter(|scale| seen_scales.insert((*scale).clone()))
        .map(|scale| {
            export_target(&platform, scale)
                .ok_or_else(|| format!("不支持的 {platform} 切图倍率：{scale}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if targets.is_empty() {
        return Err("请至少选择一个切图倍率".to_string());
    }

    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?
        .join("captures");
    let _file_guard = runtime.file_ops.lock().await;
    let mut entries = tokio::fs::read_dir(&root)
        .await
        .map_err(|error| format!("无法读取抓取历史：{error}"))?;
    let mut saved_capture = None;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("无法读取抓取历史：{error}"))?
    {
        if !entry
            .file_type()
            .await
            .map_err(|error| format!("无法检查抓取记录：{error}"))?
            .is_dir()
        {
            continue;
        }
        let directory = entry.path();
        let Ok(bytes) = tokio::fs::read(directory.join("capture.json")).await else {
            continue;
        };
        let Ok(capture) = serde_json::from_slice::<CaptureResult>(&bytes) else {
            continue;
        };
        if capture.capture_id == request.capture_id {
            saved_capture = Some((directory, capture));
            break;
        }
    }
    let (capture_dir, capture) = saved_capture.ok_or_else(|| "抓取记录不存在".to_string())?;
    let slice = capture
        .slices
        .iter()
        .find(|slice| slice.id == request.slice_id)
        .ok_or_else(|| "切图不存在或尚未下载完成".to_string())?;
    let source_path = PathBuf::from(
        slice
            .local_path
            .as_deref()
            .ok_or_else(|| "切图尚未下载完成".to_string())?,
    );
    if !source_path.starts_with(&capture_dir) {
        return Err("切图文件路径无效".to_string());
    }
    let bytes = tokio::fs::read(&source_path)
        .await
        .map_err(|error| format!("无法读取切图文件：{error}"))?;
    let source_image =
        image::load_from_memory(&bytes).map_err(|error| format!("无法解码切图文件：{error}"))?;

    let layer_frame = capture
        .layers
        .iter()
        .find(|layer| layer.id == request.layer_id)
        .and_then(|layer| layer.frame.as_ref());
    let source_scale = if slice.output_scale.is_finite() && slice.output_scale > 0.0 {
        slice.output_scale
    } else {
        3.0
    };
    let logical_width = layer_frame
        .map(|frame| frame.width)
        .filter(|width| width.is_finite() && *width > 0.0)
        .or_else(|| slice.width.map(|width| width / source_scale))
        .ok_or_else(|| "切图宽度无效".to_string())?;
    let logical_height = layer_frame
        .map(|frame| frame.height)
        .filter(|height| height.is_finite() && *height > 0.0)
        .or_else(|| slice.height.map(|height| height / source_scale))
        .ok_or_else(|| "切图高度无效".to_string())?;

    let output_root = app
        .path()
        .download_dir()
        .map_err(|error| format!("无法确定下载目录：{error}"))?;
    let base_name = sanitize_filename(&slice.name);
    let direct_file = targets.len() == 1;
    let mut files = Vec::with_capacity(targets.len());
    for target in targets {
        let width = export_dimension(logical_width, target.factor)?;
        let height = export_dimension(logical_height, target.factor)?;
        let resized = source_image.resize_exact(width, height, FilterType::Lanczos3);
        let encoded = encode_export_image(&resized, &format)?;
        let target_dir = if direct_file || target.directory.is_empty() {
            output_root.clone()
        } else {
            output_root.join(target.directory)
        };
        tokio::fs::create_dir_all(&target_dir)
            .await
            .map_err(|error| format!("无法创建切图导出目录：{error}"))?;
        let file_path = target_dir.join(format!("{base_name}{}.{}", target.suffix, format));
        tokio::fs::write(&file_path, encoded)
            .await
            .map_err(|error| format!("无法写入切图：{error}"))?;
        files.push(ExportedSliceFile {
            label: target.label.to_string(),
            path: file_path.to_string_lossy().into_owned(),
            width,
            height,
        });
    }

    Ok(ExportSliceResult {
        output_dir: output_root.to_string_lossy().into_owned(),
        files,
    })
}

#[tauri::command]
pub(crate) async fn delete_saved_capture(app: tauri::AppHandle, capture_id: String) -> Result<(), String> {
    if !valid_capture_id(&capture_id) {
        return Err("无效的抓取记录 ID".to_string());
    }
    let runtime = app.state::<CaptureRuntime>();
    mark_capture_cancelled(&runtime, &capture_id);

    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?
        .join("captures");
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("抓取记录不存在".to_string())
        }
        Err(error) => return Err(format!("无法读取抓取历史：{error}")),
    };

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("无法读取抓取历史：{error}"))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|error| format!("无法检查抓取记录：{error}"))?;
        if !file_type.is_dir() {
            continue;
        }

        let directory = entry.path();
        let Ok(bytes) = tokio::fs::read(directory.join("capture.json")).await else {
            continue;
        };
        let Ok(capture) = serde_json::from_slice::<CaptureResult>(&bytes) else {
            continue;
        };
        if capture.capture_id != capture_id {
            continue;
        }

        let _file_guard = runtime.file_ops.lock().await;
        tokio::fs::remove_dir_all(directory)
            .await
            .map_err(|error| format!("无法删除抓取记录：{error}"))?;
        return Ok(());
    }

    Err("抓取记录不存在".to_string())
}

#[tauri::command]
pub(crate) async fn delete_saved_designs(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, CaptureRuntime>,
    request: DeleteSavedDesignsRequest,
) -> Result<usize, String> {
    if request.project_id.is_empty() || request.design_ids.is_empty() || request.design_ids.len() > 500 {
        return Err("无效的页面删除请求".to_string());
    }
    let design_ids = request.design_ids.into_iter().collect::<HashSet<_>>();
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?
        .join("captures");
    let _file_guard = runtime.file_ops.lock().await;
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("无法读取抓取历史：{error}")),
    };
    let mut deleted = 0usize;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("无法读取抓取历史：{error}"))?
    {
        if !entry.file_type().await.map_err(|error| format!("无法检查抓取记录：{error}"))?.is_dir() {
            continue;
        }
        let directory = entry.path();
        let metadata_path = directory.join("capture.json");
        let Ok(bytes) = tokio::fs::read(&metadata_path).await else {
            continue;
        };
        let Ok(mut capture) = serde_json::from_slice::<CaptureResult>(&bytes) else {
            continue;
        };
        if capture.project_id != request.project_id
            || !capture.designs.iter().any(|design| design_ids.contains(&design.id))
        {
            continue;
        }

        for design in capture.designs.iter().filter(|design| design_ids.contains(&design.id)) {
            if let Some(path) = design.local_path.as_deref().map(PathBuf::from).filter(|path| path.starts_with(&directory)) {
                let _ = tokio::fs::remove_file(path).await;
            }
            deleted += 1;
        }

        let source_design_id = Url::parse(&capture.source_url)
            .ok()
            .and_then(|url| lanhu_route(&url).image_id);
        let owns_deleted_resources = source_design_id
            .as_ref()
            .map(|id| design_ids.contains(id))
            .unwrap_or(false);
        if owns_deleted_resources {
            for slice in &capture.slices {
                if let Some(path) = slice.local_path.as_deref().map(PathBuf::from).filter(|path| path.starts_with(&directory)) {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
            let exports = directory.join("exports");
            if tokio::fs::try_exists(&exports).await.unwrap_or(false) {
                let _ = tokio::fs::remove_dir_all(exports).await;
            }
            capture.slices.clear();
            capture.layers.clear();
            capture.slice_downloaded_count = 0;
            capture.slice_failed_count = 0;
            capture.slice_total_count = 0;
            capture.slices_complete = true;
        }

        capture.designs.retain(|design| !design_ids.contains(&design.id));
        if request.delete_capture && owns_deleted_resources {
            tokio::fs::remove_dir_all(&directory)
                .await
                .map_err(|error| format!("无法删除抓取记录：{error}"))?;
            continue;
        }
        if capture.designs.is_empty() {
            tokio::fs::remove_dir_all(&directory)
                .await
                .map_err(|error| format!("无法删除页面资源：{error}"))?;
            continue;
        }
        capture.downloaded_count = capture.designs.iter().filter(|design| design.local_path.is_some()).count();
        capture.failed_count = capture.designs.iter().filter(|design| design.error.is_some()).count();
        let metadata = serde_json::to_vec_pretty(&capture)
            .map_err(|error| format!("无法生成项目元数据：{error}"))?;
        tokio::fs::write(metadata_path, metadata)
            .await
            .map_err(|error| format!("无法更新项目元数据：{error}"))?;
    }
    Ok(deleted)
}

#[tauri::command]
pub(crate) async fn clear_saved_design_cache(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, CaptureRuntime>,
) -> Result<usize, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?
        .join("captures");
    let _file_guard = runtime.file_ops.lock().await;
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("无法读取抓取历史：{error}")),
    };
    let mut cleared_pages = 0usize;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| format!("无法读取抓取历史：{error}"))?
    {
        if !entry
            .file_type()
            .await
            .map_err(|error| format!("无法检查抓取记录：{error}"))?
            .is_dir()
        {
            continue;
        }
        let directory = entry.path();
        let metadata_path = directory.join("capture.json");
        let Ok(bytes) = tokio::fs::read(&metadata_path).await else {
            continue;
        };
        let Ok(mut capture) = serde_json::from_slice::<CaptureResult>(&bytes) else {
            continue;
        };

        let mut resources = tokio::fs::read_dir(&directory)
            .await
            .map_err(|error| format!("无法读取缓存目录：{error}"))?;
        while let Some(resource) = resources
            .next_entry()
            .await
            .map_err(|error| format!("无法读取缓存资源：{error}"))?
        {
            if resource.file_name() == "capture.json" {
                continue;
            }
            let file_type = resource
                .file_type()
                .await
                .map_err(|error| format!("无法检查缓存资源：{error}"))?;
            if file_type.is_dir() {
                tokio::fs::remove_dir_all(resource.path())
                    .await
                    .map_err(|error| format!("无法删除缓存目录：{error}"))?;
            } else {
                tokio::fs::remove_file(resource.path())
                    .await
                    .map_err(|error| format!("无法删除缓存文件：{error}"))?;
            }
        }

        cleared_pages += capture
            .designs
            .iter()
            .filter(|design| design.local_path.is_some())
            .count();
        for design in &mut capture.designs {
            design.local_path = None;
            design.error = None;
        }
        capture.downloaded_count = 0;
        capture.failed_count = 0;
        capture.slices.clear();
        capture.layers.clear();
        capture.slice_downloaded_count = 0;
        capture.slice_failed_count = 0;
        capture.slice_total_count = 0;
        capture.slices_complete = true;
        let metadata = serde_json::to_vec_pretty(&capture)
            .map_err(|error| format!("无法生成项目元数据：{error}"))?;
        tokio::fs::write(metadata_path, metadata)
            .await
            .map_err(|error| format!("无法更新项目元数据：{error}"))?;
    }
    Ok(cleared_pages)
}
