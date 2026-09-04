use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use reqwest::{header, redirect::Policy, Client};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use url::{Host, Url};

pub mod design_data;

use design_data::*;

const TITLE_PREFIX: &str = "__DESIGNBRIDGE__";
const MAX_IMAGE_BYTES: u64 = 100 * 1024 * 1024;
static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct CaptureRuntime {
    inner: Mutex<CaptureRuntimeInner>,
    file_ops: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct CaptureRuntimeInner {
    active: HashSet<String>,
    cancelled: HashSet<String>,
    sources: HashMap<String, String>,
    chunks: HashMap<String, ChunkAccumulator>,
}

#[derive(Debug)]
struct ChunkAccumulator {
    parts: Vec<Option<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LanhuProjectPayload {
    resolved_url: String,
    team_id: String,
    project_id: String,
    project_name: String,
    designs: Vec<LanhuDesignPayload>,
    #[serde(default)]
    slices: Vec<LanhuSlicePayload>,
    #[serde(default)]
    layers: Vec<InspectableLayer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LanhuDesignPayload {
    id: String,
    name: String,
    width: Option<f64>,
    height: Option<f64>,
    #[serde(default)]
    coordinate_space: Option<DesignCoordinateSpace>,
    url: String,
    update_time: Option<String>,
    has_comment: Option<bool>,
    #[serde(default)]
    comments: Vec<DesignComment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LanhuSlicePayload {
    id: String,
    name: String,
    width: Option<f64>,
    height: Option<f64>,
    url: String,
    #[serde(default)]
    org_url: Option<String>,
    #[serde(default)]
    svg_url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureProgress {
    capture_id: String,
    stage: String,
    message: String,
    percent: u8,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureFailure {
    capture_id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportSliceRequest {
    capture_id: String,
    layer_id: String,
    slice_id: String,
    format: String,
    platform: String,
    scales: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportedSliceFile {
    label: String,
    path: String,
    width: u32,
    height: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportSliceResult {
    output_dir: String,
    files: Vec<ExportedSliceFile>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ExportTarget {
    label: &'static str,
    directory: &'static str,
    suffix: &'static str,
    factor: f64,
}

struct TitleMessage<'a> {
    capture_id: &'a str,
    kind: &'a str,
    index: usize,
    total: usize,
    data: &'a str,
}

fn parse_title_message(title: &str) -> Option<TitleMessage<'_>> {
    let mut parts = title.splitn(6, '|');
    if parts.next()? != TITLE_PREFIX {
        return None;
    }

    let capture_id = parts.next()?;
    let kind = parts.next()?;
    let index = parts.next()?.parse().ok()?;
    let total = parts.next()?.parse().ok()?;
    let data = parts.next()?;

    if total == 0 || total > 2_048 || index >= total || data.len() > 8_192 {
        return None;
    }

    Some(TitleMessage {
        capture_id,
        kind,
        index,
        total,
        data,
    })
}

fn lanhu_url(input: &str) -> Result<Url, String> {
    let url = Url::parse(input.trim()).map_err(|_| "请输入完整的蓝湖链接".to_string())?;
    if url.scheme() != "https" {
        return Err("仅支持 HTTPS 蓝湖链接".to_string());
    }

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let allowed = host == "lanhuapp.com"
        || host.ends_with(".lanhuapp.com")
        || host == "lanhu.com"
        || host.ends_with(".lanhu.com");
    if !allowed {
        return Err("当前仅支持 lanhuapp.com 的蓝湖链接".to_string());
    }

    Ok(url)
}

fn safe_asset_url(input: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|_| "资源地址无效".to_string())?;
    if url.scheme() != "https" {
        return Err("资源地址必须使用 HTTPS".to_string());
    }

    if !matches!(url.host(), Some(Host::Domain(_))) {
        return Err("不允许从 IP 地址下载资源".to_string());
    }

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let allowed = host == "lanhuapp.com"
        || host.ends_with(".lanhuapp.com")
        || host == "aliyuncs.com"
        || host.ends_with(".aliyuncs.com")
        || host == "alicdn.com"
        || host.ends_with(".alicdn.com");
    if !allowed {
        return Err(format!("不受信任的资源域名：{host}"));
    }

    Ok(url)
}

fn original_asset_url(input: &str) -> Result<Url, String> {
    let mut url = safe_asset_url(input)?;
    let retained_query = url
        .query_pairs()
        .filter(|(key, _)| key != "x-oss-process")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    if !retained_query.is_empty() {
        url.query_pairs_mut().extend_pairs(retained_query);
    }
    Ok(url)
}

fn capture_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{millis}-{sequence}")
}

#[derive(Clone, Debug, Default)]
struct LanhuRoute {
    team_id: Option<String>,
    project_id: Option<String>,
    image_id: Option<String>,
}

fn lanhu_route(url: &Url) -> LanhuRoute {
    let query = url
        .fragment()
        .and_then(|fragment| fragment.split_once('?').map(|(_, query)| query))
        .or_else(|| url.query())
        .unwrap_or_default();
    let params = url::form_urlencoded::parse(query.as_bytes());
    let mut route = LanhuRoute::default();
    for (key, value) in params {
        match key.as_ref() {
            "tid" | "team_id" => route.team_id = Some(value.into_owned()),
            "pid" | "project_id" => route.project_id = Some(value.into_owned()),
            "image_id" | "docId" => route.image_id = Some(value.into_owned()),
            _ => {}
        }
    }
    route
}

fn capture_script(capture_id: &str) -> String {
    const SCRIPT: &str = r#"
(() => {
  if (window.__designBridgeCaptureActive) return;
  window.__designBridgeCaptureActive = true;
  const captureId = "__CAPTURE_ID__";
  const encode = (value) => {
    const bytes = new TextEncoder().encode(value);
    let binary = "";
    for (let offset = 0; offset < bytes.length; offset += 0x4000) {
      binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x4000));
    }
    return btoa(binary);
  };
  const message = "蓝湖页面已打开，Rust 正在读取登录态…";
  const encoded = encode(message);
  document.title = `__DESIGNBRIDGE__|${captureId}|status|0|1|${encoded}`;
})();
"#;
    SCRIPT.replace("__CAPTURE_ID__", capture_id)
}

#[allow(dead_code)]
fn legacy_capture_script(capture_id: &str) -> String {
    const SCRIPT: &str = r#"
(() => {
  if (window.__designBridgeCaptureActive) return;
  window.__designBridgeCaptureActive = true;

  const captureId = "__CAPTURE_ID__";
  let working = false;
  let complete = false;
  let attempts = 0;
  let lastStatus = "";

  const encode = (value) => {
    const bytes = new TextEncoder().encode(value);
    let binary = "";
    for (let offset = 0; offset < bytes.length; offset += 0x4000) {
      binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x4000));
    }
    return btoa(binary);
  };

  const send = (kind, value) => {
    window.__designBridgeCaptureState = {
      captureId,
      kind,
      value,
    };
    const encoded = encode(value);
    const size = 3000;
    const total = Math.max(1, Math.ceil(encoded.length / size));
    for (let index = 0; index < total; index += 1) {
      const chunk = encoded.slice(index * size, (index + 1) * size);
      setTimeout(() => {
        document.title = `__DESIGNBRIDGE__|${captureId}|${kind}|${index}|${total}|${chunk}`;
      }, index * 140);
    }
  };

  const status = (message) => {
    if (message === lastStatus) return;
    lastStatus = message;
    send("status", message);
  };

  const paramsFromLocation = () => {
    const hash = window.location.hash || "";
    const query = hash.includes("?")
      ? hash.slice(hash.indexOf("?") + 1)
      : window.location.search.slice(1);
    const params = new URLSearchParams(query);
    return {
      teamId: params.get("tid"),
      projectId: params.get("pid") || params.get("project_id"),
      imageId: params.get("image_id") || params.get("docId"),
    };
  };

  const asNumber = (value) => {
    const number = Number(value);
    return Number.isFinite(number) ? number : null;
  };

  const asUrl = (value) => {
    if (typeof value !== "string" || !value.trim()) return null;
    const valueString = value.trim();
    if (valueString.startsWith("//")) return `https:${valueString}`;
    return /^https?:\/\//i.test(valueString) ? valueString : null;
  };

  const imageUrl = (item, version) => {
    const image = item && typeof item.image === "object" ? item.image : {};
    const images = item && typeof item.images === "object" ? item.images : {};
    const versionImages = version && typeof version.images === "object" ? version.images : {};
    return [
      item?.url,
      item?.image_url,
      item?.imageUrl,
      item?.original_url,
      item?.origin_url,
      item?.preview_url,
      item?.cover_url,
      item?.coverUrl,
      item?.src,
      image.url,
      image.image_url,
      image.imageUrl,
      image.original_url,
      image.imageUrlOriginal,
      images.png_xxxhd,
      images.png,
      images.original,
      version?.url,
      version?.image_url,
      version?.imageUrl,
      version?.original_url,
      version?.origin_url,
      version?.preview_url,
      versionImages.png_xxxhd,
      versionImages.png,
      versionImages.original,
    ].map(asUrl).find(Boolean) || null;
  };

  const toDesign = (item, fallbackId) => {
    const source = item && typeof item === "object" ? item : {};
    const versions = Array.isArray(source.versions) ? source.versions : [];
    const version = versions[0] || {};
    const size = source.size && typeof source.size === "object" ? source.size : {};
    const url = imageUrl(source, version);
    if (!url) return null;
    return {
      id: String(source.id || fallbackId || "unknown"),
      name: String(source.name || source.title || "未命名画板"),
      width: asNumber(source.width ?? size.width ?? version.width),
      height: asNumber(source.height ?? size.height ?? version.height),
      url,
      updateTime: source.update_time
        ? String(source.update_time)
        : source.updateTime
          ? String(source.updateTime)
          : null,
      hasComment: Boolean(source.has_comment ?? source.hasComment),
    };
  };

  const capture = async () => {
    if (working || complete) return;
    attempts += 1;

    const { teamId, projectId, imageId } = paramsFromLocation();
    if (!projectId || (!teamId && !imageId)) {
      status(attempts < 4 ? "正在打开蓝湖页面…" : "等待蓝湖登录或分享链接跳转…");
      return;
    }

    working = true;
    status(imageId ? "已识别画板，正在读取原图地址…" : "已识别项目，正在读取设计稿列表…");
    try {
      const endpoint = new URL(
        imageId
          ? "https://lanhuapp.com/api/project/image"
          : "https://lanhuapp.com/api/project/images",
      );
      const query = imageId
        ? {
            pid: projectId,
            project_id: projectId,
            image_id: imageId,
            dds_status: "1",
          }
        : {
            project_id: projectId,
            team_id: teamId,
            dds_status: "1",
            position: "1",
            show_cb_src: "1",
            comment: "1",
          };
      if (teamId && imageId) query.team_id = teamId;
      endpoint.search = new URLSearchParams(query).toString();

      // Legacy bridge retained for compatibility with old capture state; it is never used for probing.
      const response = { ok: false, json: async () => ({}) };
      const json = await response.json();
      if (!response.ok || !["00000", "0", 0].includes(json.code)) {
        status("等待蓝湖授权完成…");
        working = false;
        return;
      }

      const project = json.data || json.result || {};
      const designs = imageId
        ? [toDesign(project, imageId)].filter(Boolean)
        : (project.images || []).map((item) => toDesign(item, item?.id)).filter(Boolean);

      if (designs.length === 0) {
        status("蓝湖暂未返回可下载的原图地址，等待页面就绪…");
        working = false;
        return;
      }

      complete = true;
      send("payload", JSON.stringify({
        resolvedUrl: window.location.href,
        teamId: teamId || String(project.team_id || project.teamId || ""),
        projectId,
        projectName: String(
          project.name ||
            project.project_name ||
            project.projectName ||
            project.project?.name ||
            "未命名项目",
        ),
        designs,
      }));
    } catch (error) {
      working = false;
      status("暂未读取到项目，等待页面就绪…");
    }
  };

  window.setInterval(capture, 1600);
  window.addEventListener("hashchange", capture);
  capture();
})();
"#;

    SCRIPT.replace("__CAPTURE_ID__", capture_id)
}

fn decode_message(data: &str) -> Result<String, String> {
    let bytes = BASE64
        .decode(data)
        .map_err(|_| "蓝湖页面返回了无效数据".to_string())?;
    String::from_utf8(bytes).map_err(|_| "蓝湖页面返回的文本编码无效".to_string())
}

fn emit_progress(
    app: &tauri::AppHandle,
    capture_id: &str,
    stage: &str,
    message: &str,
    percent: u8,
) {
    let _ = app.emit(
        "capture-progress",
        CaptureProgress {
            capture_id: capture_id.to_string(),
            stage: stage.to_string(),
            message: message.to_string(),
            percent,
        },
    );
}

fn emit_failure(app: &tauri::AppHandle, capture_id: &str, message: impl Into<String>) {
    let _ = app.emit(
        "capture-failed",
        CaptureFailure {
            capture_id: capture_id.to_string(),
            message: message.into(),
        },
    );
}

fn collect_chunk(
    runtime: &CaptureRuntime,
    message: &TitleMessage<'_>,
) -> Option<Result<String, String>> {
    let mut inner = runtime.inner.lock().ok()?;
    if !inner.active.contains(message.capture_id) {
        return None;
    }

    let key = format!("{}:{}", message.capture_id, message.kind);
    let accumulator = inner
        .chunks
        .entry(key.clone())
        .or_insert_with(|| ChunkAccumulator {
            parts: vec![None; message.total],
        });

    if accumulator.parts.len() != message.total {
        inner.chunks.remove(&key);
        return Some(Err("蓝湖页面返回的数据分片不一致".to_string()));
    }

    accumulator.parts[message.index] = Some(message.data.to_string());
    if accumulator.parts.iter().any(Option::is_none) {
        return None;
    }

    let encoded = accumulator
        .parts
        .iter()
        .filter_map(|part| part.as_deref())
        .collect::<String>();
    inner.chunks.remove(&key);
    Some(decode_message(&encoded))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct BridgeState {
    capture_id: String,
    kind: String,
    value: String,
}

fn take_source(runtime: &CaptureRuntime, capture_id: &str) -> Option<String> {
    let mut inner = runtime.inner.lock().ok()?;
    inner.active.remove(capture_id);
    inner
        .chunks
        .retain(|key, _| !key.starts_with(&format!("{capture_id}:")));
    inner.sources.remove(capture_id)
}

fn capture_cancelled(runtime: &CaptureRuntime, capture_id: &str) -> bool {
    runtime
        .inner
        .lock()
        .map(|inner| inner.cancelled.contains(capture_id))
        .unwrap_or(true)
}

fn mark_capture_cancelled(runtime: &CaptureRuntime, capture_id: &str) {
    if let Ok(mut inner) = runtime.inner.lock() {
        inner.cancelled.insert(capture_id.to_string());
    }
}

fn handle_message(
    app: tauri::AppHandle,
    window: WebviewWindow,
    expected_capture_id: String,
    kind: &str,
    decoded: String,
) {
    let runtime = app.state::<CaptureRuntime>();
    match kind {
        "status" => emit_progress(&app, &expected_capture_id, "authorize", &decoded, 18),
        "payload" => {
            let Some(source_url) = take_source(&runtime, &expected_capture_id) else {
                return;
            };
            let project: LanhuProjectPayload = match serde_json::from_str(&decoded) {
                Ok(project) => project,
                Err(_) => {
                    emit_failure(&app, &expected_capture_id, "无法解析蓝湖设计数据");
                    return;
                }
            };

            emit_progress(
                &app,
                &expected_capture_id,
                "download",
                &format!("已读取 {} 个画板，开始下载原图…", project.designs.len()),
                35,
            );
            let _ = window.close();

            tauri::async_runtime::spawn(async move {
                match persist_project(&app, &expected_capture_id, &source_url, project).await {
                    Ok(result) => {
                        let _ = app.emit("capture-complete", result);
                    }
                    Err(error) => emit_failure(&app, &expected_capture_id, error),
                }
            });
        }
        _ => {}
    }
}

fn handle_title(
    app: tauri::AppHandle,
    window: WebviewWindow,
    expected_capture_id: String,
    title: String,
) {
    let Some(message) = parse_title_message(&title) else {
        return;
    };
    if message.capture_id != expected_capture_id {
        return;
    }

    let runtime = app.state::<CaptureRuntime>();
    let Some(result) = collect_chunk(&runtime, &message) else {
        return;
    };

    let decoded = match result {
        Ok(value) => value,
        Err(error) => {
            emit_failure(&app, &expected_capture_id, error);
            return;
        }
    };
    handle_message(app, window, expected_capture_id, message.kind, decoded);
}

#[allow(dead_code)]
fn poll_bridge_state(app: tauri::AppHandle, window: WebviewWindow, capture_id: String) {
    std::thread::spawn(move || {
        let script = "JSON.stringify(window.__designBridgeCaptureState || null)";
        let capture_script = capture_script(&capture_id);
        let mut last_state = String::new();

        for _ in 0..1_200 {
            let _ = window.eval(capture_script.clone());
            let (sender, receiver) = std::sync::mpsc::channel();
            if window
                .eval_with_callback(script, move |value| {
                    let _ = sender.send(value);
                })
                .is_err()
            {
                break;
            }

            if let Ok(value) = receiver.recv_timeout(std::time::Duration::from_secs(2)) {
                if !value.is_empty() && value != last_state {
                    last_state = value.clone();
                    if value == "null" {
                        continue;
                    }
                    if let Ok(state) = serde_json::from_str::<BridgeState>(&value) {
                        if state.capture_id == capture_id {
                            let terminal = state.kind == "payload";
                            handle_message(
                                app.clone(),
                                window.clone(),
                                capture_id.clone(),
                                &state.kind,
                                state.value,
                            );
                            if terminal {
                                break;
                            }
                        }
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

fn cookie_header(window: &WebviewWindow, url: &Url) -> Result<Option<String>, String> {
    let cookies = window
        .cookies_for_url(url.clone())
        .map_err(|error| format!("无法读取蓝湖登录态：{error}"))?;
    if cookies.is_empty() {
        return Ok(None);
    }

    let value = cookies
        .iter()
        .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
        .collect::<Vec<_>>()
        .join("; ");
    Ok((!value.is_empty()).then_some(value))
}

fn lanhu_http_client(cookie: &str) -> Result<Client, String> {
    let mut headers = header::HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static("application/json, text/plain, */*"),
    );
    headers.insert(
        header::REFERER,
        header::HeaderValue::from_static("https://lanhuapp.com/"),
    );
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static("Mozilla/5.0 DesignBridge/0.1"),
    );
    headers.insert(
        header::HeaderName::from_static("request-from"),
        header::HeaderValue::from_static("web"),
    );
    headers.insert(
        header::HeaderName::from_static("real-path"),
        header::HeaderValue::from_static("/item/project/detailDetach"),
    );
    if !cookie.is_empty() {
        headers.insert(
            header::COOKIE,
            header::HeaderValue::from_str(cookie).map_err(|_| "蓝湖登录态格式无效".to_string())?,
        );
    }

    Client::builder()
        .default_headers(headers)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many redirects");
            }
            if safe_asset_url(attempt.url().as_str()).is_ok() {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| format!("无法创建蓝湖请求客户端：{error}"))
}

fn api_url(route: &LanhuRoute) -> Result<Url, String> {
    let project_id = route
        .project_id
        .as_deref()
        .ok_or_else(|| "链接中缺少项目 ID".to_string())?;
    let mut url = Url::parse(if route.image_id.is_some() {
        "https://lanhuapp.com/api/project/image"
    } else {
        "https://lanhuapp.com/api/project/images"
    })
    .map_err(|error| format!("蓝湖接口地址无效：{error}"))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("pid", project_id);
        query.append_pair("project_id", project_id);
        query.append_pair("dds_status", "1");
        if let Some(team_id) = route.team_id.as_deref() {
            query.append_pair("team_id", team_id);
        }
        if let Some(image_id) = route.image_id.as_deref() {
            query.append_pair("image_id", image_id);
            query.append_pair("comment", "1");
        } else {
            query.append_pair("position", "1");
            query.append_pair("show_cb_src", "1");
            query.append_pair("comment", "1");
        }
    }
    Ok(url)
}

async fn request_lanhu_json(client: &Client, url: Url) -> Result<serde_json::Value, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("蓝湖接口请求失败：{error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取蓝湖接口响应失败：{error}"))?;
    if !status.is_success() {
        return Err(format!("蓝湖接口返回 HTTP {status}"));
    }
    let value = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|_| "蓝湖接口返回的不是 JSON，可能尚未完成登录".to_string())?;
    if let Some(code) = value.get("code") {
        let success = code
            .as_str()
            .map(|value| value == "00000" || value == "0")
            .or_else(|| code.as_i64().map(|value| value == 0))
            .unwrap_or(false);
        if !success {
            let message = value
                .get("msg")
                .or_else(|| value.get("message"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("蓝湖未授权当前请求");
            return Err(message.to_string());
        }
    }
    Ok(value)
}

fn response_data(value: &serde_json::Value) -> &serde_json::Value {
    value
        .get("data")
        .or_else(|| value.get("result"))
        .unwrap_or(value)
}

fn value_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(found) = object.get(*key).and_then(serde_json::Value::as_str) {
            if !found.trim().is_empty() {
                return Some(found.trim().to_string());
            }
        }
    }
    None
}

fn value_number(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(number) = object.get(*key).and_then(serde_json::Value::as_f64) {
            if number.is_finite() && number > 0.0 {
                return Some(number);
            }
        }
        if let Some(number) = object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .and_then(|value| value.parse::<f64>().ok())
        {
            if number.is_finite() && number > 0.0 {
                return Some(number);
            }
        }
    }
    None
}

fn finite_number(value: &serde_json::Value, keys: &[&str]) -> Option<f64> {
    let object = value.as_object()?;
    for key in keys {
        let number = object.get(*key).and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
        });
        if let Some(number) = number.filter(|number| number.is_finite()) {
            return Some(number);
        }
    }
    None
}

fn normalize_https_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let candidate = if raw.starts_with("//") {
        format!("https:{raw}")
    } else if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else if raw.contains('/') && raw.contains('.') {
        // Lanhu sometimes stores json_url as `host/path` and adds the scheme in WebView.
        format!("https://{raw}")
    } else {
        return None;
    };
    Url::parse(&candidate)
        .ok()
        .filter(|url| url.scheme() == "https")
        .map(|_| candidate)
}

fn nested_url(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    if let Some(raw) = value_string(value, keys) {
        if let Some(url) = normalize_https_url(&raw) {
            return Some(url);
        }
    }
    if let Some(object) = value.as_object() {
        for child in object.values() {
            if let Some(url) = nested_url(child, keys) {
                return Some(url);
            }
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            if let Some(url) = nested_url(child, keys) {
                return Some(url);
            }
        }
    }
    None
}

fn design_image_url(value: &serde_json::Value) -> Option<String> {
    nested_url(
        value,
        &[
            "url",
            "image_url",
            "imageUrl",
            "original_url",
            "origin_url",
            "preview_url",
            "cover_url",
            "coverUrl",
            "src",
            "png_xxxhd",
            "png_xhdpi",
            "png",
            "original",
        ],
    )
}

fn string_or_number(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        let Some(found) = object.get(*key) else {
            continue;
        };
        if let Some(text) = found
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(text.to_string());
        }
        if let Some(number) = found.as_i64() {
            return Some(number.to_string());
        }
        if let Some(number) = found.as_u64() {
            return Some(number.to_string());
        }
    }
    None
}

fn decoded_json_value(value: &serde_json::Value) -> Option<serde_json::Value> {
    if value.is_object() {
        return Some(value.clone());
    }
    value
        .as_str()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(serde_json::Value::is_object)
}

fn latest_design_version(value: &serde_json::Value) -> Option<&serde_json::Value> {
    let versions = value.get("versions")?.as_array()?;
    let latest_id = string_or_number(value, &["latest_version", "latestVersion"]);
    latest_id
        .as_deref()
        .and_then(|latest_id| {
            versions.iter().find(|version| {
                string_or_number(version, &["id", "version_id", "versionId"]).as_deref()
                    == Some(latest_id)
            })
        })
        .or_else(|| versions.first())
}

fn comment_author(value: &serde_json::Value) -> String {
    if let Some(author) = value_string(
        value,
        &[
            "author_name",
            "authorName",
            "creator_name",
            "creatorName",
            "nickname",
            "user_name",
            "userName",
        ],
    ) {
        return author;
    }
    for key in [
        "author",
        "user",
        "creator",
        "member",
        "editor_info",
        "editorInfo",
        "create_by",
        "createBy",
    ] {
        let Some(person) = value.get(key) else {
            continue;
        };
        if let Some(author) = value_string(
            person,
            &["nickname", "name", "username", "user_name", "userName"],
        ) {
            return author;
        }
        if let Some(author) = person
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return author.to_string();
        }
    }
    "未知成员".to_string()
}

fn comment_resolved(value: &serde_json::Value) -> bool {
    for key in [
        "resolved",
        "is_resolved",
        "isResolved",
        "finished",
        "closed",
    ] {
        let Some(state) = value.get(key) else {
            continue;
        };
        if let Some(state) = state.as_bool() {
            return state;
        }
        if let Some(state) = state.as_i64() {
            return state != 0;
        }
    }
    string_or_number(value, &["status", "state"])
        .map(|status| {
            matches!(
                status.to_ascii_lowercase().as_str(),
                "2" | "resolved" | "finished" | "done" | "closed"
            )
        })
        .unwrap_or(false)
}

fn comment_position(
    comment: &serde_json::Value,
    design: &serde_json::Value,
    source_width: Option<f64>,
    source_height: Option<f64>,
) -> (Option<f64>, Option<f64>) {
    let ext_data = comment
        .get("extData")
        .or_else(|| comment.get("ext_data"))
        .and_then(decoded_json_value);

    let percent_x = finite_number(comment, &["positionX", "position_x"]).or_else(|| {
        ext_data
            .as_ref()
            .and_then(|ext_data| finite_number(ext_data, &["positionX", "position_x"]))
    });
    let percent_y = finite_number(comment, &["positionY", "position_y"]).or_else(|| {
        ext_data
            .as_ref()
            .and_then(|ext_data| finite_number(ext_data, &["positionY", "position_y"]))
    });
    if let (Some(percent_x), Some(percent_y), Some(width), Some(height)) =
        (percent_x, percent_y, source_width, source_height)
    {
        if (0.0..=1.0).contains(&percent_x) && (0.0..=1.0).contains(&percent_y) {
            return (Some(percent_x * width), Some(percent_y * height));
        }
    }

    let Some(position) = ext_data
        .as_ref()
        .and_then(|ext_data| ext_data.get("position"))
        .and_then(decoded_json_value)
    else {
        return (None, None);
    };
    let Some(absolute_x) = finite_number(&position, &["x", "left"]) else {
        return (None, None);
    };
    let Some(absolute_y) = finite_number(&position, &["y", "top"]) else {
        return (None, None);
    };
    let origin_x = finite_number(design, &["position_x", "positionX", "x"]).unwrap_or(0.0);
    let origin_y = finite_number(design, &["position_y", "positionY", "y"]).unwrap_or(0.0);

    let within_source = |x: f64, y: f64| match (source_width, source_height) {
        (Some(width), Some(height)) => x >= 0.0 && y >= 0.0 && x <= width && y <= height,
        _ => x >= 0.0 && y >= 0.0,
    };
    let local_x = absolute_x - origin_x;
    let local_y = absolute_y - origin_y;
    if within_source(local_x, local_y) {
        (Some(local_x), Some(local_y))
    } else if within_source(absolute_x, absolute_y) {
        (Some(absolute_x), Some(absolute_y))
    } else {
        (None, None)
    }
}

fn comment_target(value: &serde_json::Value) -> (Option<String>, Option<String>) {
    let ext_data = value
        .get("extData")
        .or_else(|| value.get("ext_data"))
        .and_then(decoded_json_value);
    let direct_target_id =
        string_or_number(value, &["resource_id", "resourceId", "layer_id", "layerId"]);
    let ext_target_id = ext_data.as_ref().and_then(|ext_data| {
        string_or_number(
            ext_data,
            &[
                "image_id",
                "imageId",
                "resource_id",
                "resourceId",
                "layer_id",
                "layerId",
            ],
        )
    });
    let target_type = string_or_number(
        value,
        &["resource_type", "resourceType", "target_type", "targetType"],
    )
    .or_else(|| {
        ext_data.as_ref().and_then(|ext_data| {
            string_or_number(
                ext_data,
                &["resource_type", "resourceType", "target_type", "targetType"],
            )
        })
    });
    let target_id = if target_type.as_deref() == Some("3") {
        ext_target_id.or(direct_target_id)
    } else {
        direct_target_id.or(ext_target_id)
    };
    (target_id, target_type)
}

fn comment_replies(value: &serde_json::Value) -> Vec<DesignCommentReply> {
    let replies = find_array(
        value,
        &["replies", "replys", "reply_list", "replyList", "children"],
    );
    replies
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, reply)| {
            let content = value_string(
                reply,
                &["content", "comment", "text", "message", "description"],
            )?;
            Some(DesignCommentReply {
                id: string_or_number(reply, &["id", "comment_id", "commentId"])
                    .unwrap_or_else(|| format!("reply-{}", index + 1)),
                author: comment_author(reply),
                content,
                created_at: string_or_number(
                    reply,
                    &[
                        "create_time",
                        "createTime",
                        "created_at",
                        "createdAt",
                        "time",
                    ],
                ),
            })
        })
        .collect()
}

fn collect_design_comments(value: &serde_json::Value) -> Vec<DesignComment> {
    let version = latest_design_version(value);
    let version_id =
        version.and_then(|version| string_or_number(version, &["id", "version_id", "versionId"]));
    let version_name = version.and_then(|version| {
        value_string(
            version,
            &[
                "version_info",
                "versionInfo",
                "name",
                "version_name",
                "versionName",
            ],
        )
    });
    let source_width = version
        .and_then(|version| value_number(version, &["width", "w"]))
        .or_else(|| value_number(value, &["width", "w"]));
    let source_height = version
        .and_then(|version| value_number(version, &["height", "h"]))
        .or_else(|| value_number(value, &["height", "h"]));
    let comments = version
        .and_then(|version| find_array(version, &["comments", "comment_list", "commentList"]))
        .or_else(|| find_array(value, &["comments", "comment_list", "commentList"]));

    comments
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(position, comment)| {
            let content = value_string(
                comment,
                &["content", "comment", "text", "message", "description"],
            )?;
            let index = string_or_number(
                comment,
                &[
                    "index",
                    "number",
                    "order",
                    "sequence",
                    "serial_number",
                    "serialNumber",
                    "text",
                ],
            )
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(position + 1);
            let (x, y) = comment_position(comment, value, source_width, source_height);
            let (target_id, target_type) = comment_target(comment);
            Some(DesignComment {
                id: string_or_number(
                    comment,
                    &["id", "comment_id", "commentId", "anchor_uuid", "anchorUuid"],
                )
                .unwrap_or_else(|| format!("comment-{index}")),
                index,
                author: comment_author(comment),
                content,
                created_at: string_or_number(
                    comment,
                    &[
                        "create_time",
                        "createTime",
                        "created_at",
                        "createdAt",
                        "time",
                    ],
                ),
                resolved: comment_resolved(comment),
                x,
                y,
                source_width,
                source_height,
                version_id: version_id.clone(),
                version_name: version_name.clone(),
                target_id,
                target_type,
                replies: comment_replies(comment),
            })
        })
        .collect()
}

fn design_from_value(value: &serde_json::Value, fallback_id: &str) -> Option<LanhuDesignPayload> {
    let version = latest_design_version(value).unwrap_or(&serde_json::Value::Null);
    let url = design_image_url(value).or_else(|| design_image_url(version))?;
    let comments = collect_design_comments(value);
    Some(LanhuDesignPayload {
        id: value_string(value, &["id", "image_id", "imageId", "web_id"])
            .unwrap_or_else(|| fallback_id.to_string()),
        name: value_string(value, &["name", "title"]).unwrap_or_else(|| "未命名画板".to_string()),
        width: value_number(value, &["width", "w"]),
        height: value_number(value, &["height", "h"]),
        coordinate_space: None,
        url,
        update_time: value_string(value, &["update_time", "updateTime"]),
        has_comment: value
            .get("has_comment")
            .or_else(|| value.get("hasComment"))
            .and_then(serde_json::Value::as_bool),
        comments,
    })
}

fn android_coordinate_space(value: &serde_json::Value) -> Option<DesignCoordinateSpace> {
    let artboard = value.get("artboard")?;
    let frame = artboard
        .get("frame")
        .or_else(|| artboard.get("realFrame"))?;
    let width = value_number(frame, &["width", "w"])?;
    let height = value_number(frame, &["height", "h"])?;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }

    Some(DesignCoordinateSpace {
        platform: "android".to_string(),
        width,
        height,
        unit: "dp".to_string(),
    })
}

fn layer_frame(value: &serde_json::Value) -> Option<LayerFrame> {
    let frame = value.get("realFrame").or_else(|| value.get("frame"))?;
    let width = finite_number(frame, &["width", "w"])?;
    let height = finite_number(frame, &["height", "h"])?;
    if width < 0.0 || height < 0.0 {
        return None;
    }
    Some(LayerFrame {
        x: finite_number(frame, &["left", "x"]).unwrap_or(0.0),
        y: finite_number(frame, &["top", "y"]).unwrap_or(0.0),
        width,
        height,
    })
}

fn radius_value(value: &serde_json::Value) -> Option<LayerRadius> {
    Some(LayerRadius {
        top_left: finite_number(value, &["topLeft", "top_left"]).unwrap_or(0.0),
        top_right: finite_number(value, &["topRight", "top_right"]).unwrap_or(0.0),
        bottom_right: finite_number(value, &["bottomRight", "bottom_right"]).unwrap_or(0.0),
        bottom_left: finite_number(value, &["bottomLeft", "bottom_left"]).unwrap_or(0.0),
    })
}

fn layer_radius(value: &serde_json::Value) -> LayerRadius {
    let path_radius = value
        .get("paths")
        .and_then(serde_json::Value::as_array)
        .and_then(|paths| {
            paths.iter().find_map(|path| {
                let radius = path.get("radius").and_then(radius_value)?;
                let has_radius = radius.top_left != 0.0
                    || radius.top_right != 0.0
                    || radius.bottom_right != 0.0
                    || radius.bottom_left != 0.0;
                has_radius.then_some(radius)
            })
        });
    path_radius
        .or_else(|| value.get("radius").and_then(radius_value))
        .unwrap_or_default()
}

fn color_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("color")
        .and_then(|color| value_string(color, &["value", "hex"]))
        .or_else(|| value_string(value, &["value", "hex"]))
}

fn color_token(value: &serde_json::Value) -> Option<String> {
    value
        .pointer("/boundVariables/color/name")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .pointer("/color/boundVariables/color/name")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string)
}

fn layer_paints(value: &serde_json::Value) -> Vec<LayerPaint> {
    value
        .pointer("/style/fills")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|fill| {
            fill.get("isEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
        .map(|fill| LayerPaint {
            paint_type: value_string(fill, &["type"]).unwrap_or_else(|| "color".to_string()),
            color: color_value(fill),
            token: color_token(fill),
            opacity: finite_number(fill, &["opacity"]).unwrap_or(1.0),
        })
        .collect()
}

fn layer_borders(value: &serde_json::Value) -> Vec<LayerBorder> {
    value
        .pointer("/style/borders")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|border| {
            border
                .get("isEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
        .map(|border| LayerBorder {
            width: finite_number(border, &["width"]).unwrap_or(0.0),
            style: value_string(border, &["style"]).unwrap_or_else(|| "solid".to_string()),
            color: color_value(border),
            token: color_token(border),
            opacity: finite_number(border, &["opacity"]).unwrap_or(1.0),
        })
        .collect()
}

fn layer_shadows(value: &serde_json::Value) -> Vec<LayerShadow> {
    value
        .pointer("/style/shadows")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|shadow| {
            shadow
                .get("isEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
        .map(|shadow| {
            let offset = shadow.get("offset").unwrap_or(shadow);
            LayerShadow {
                shadow_type: value_string(shadow, &["type"])
                    .unwrap_or_else(|| "dropShadow".to_string()),
                color: color_value(shadow),
                offset_x: finite_number(offset, &["x", "left"]).unwrap_or(0.0),
                offset_y: finite_number(offset, &["y", "top"]).unwrap_or(0.0),
                blur: finite_number(shadow, &["blur", "radius"]).unwrap_or(0.0),
                spread: finite_number(shadow, &["spread"]).unwrap_or(0.0),
            }
        })
        .collect()
}

fn layer_blurs(value: &serde_json::Value) -> Vec<LayerBlur> {
    value
        .pointer("/style/blurs")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|blur| {
            blur.get("isEnabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        })
        .map(|blur| LayerBlur {
            blur_type: value_string(blur, &["type"]).unwrap_or_else(|| "layerBlur".to_string()),
            radius: finite_number(blur, &["radius", "blur"]).unwrap_or(0.0),
        })
        .collect()
}

fn exact_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

fn text_range_index(value: &serde_json::Value, key: &str) -> Option<usize> {
    finite_number(value, &[key])
        .filter(|index| *index >= 0.0 && index.fract() == 0.0)
        .map(|index| index as usize)
}

fn text_metric(font: &serde_json::Value, key: &str) -> (Option<f64>, Option<String>) {
    let Some(metric) = font.get(key) else {
        return (None, None);
    };
    match metric {
        serde_json::Value::Object(_) => (
            finite_number(metric, &["value"]),
            value_string(metric, &["unit"]),
        ),
        _ => (finite_number(font, &[key]), None),
    }
}

fn text_content_for_range(content: &str, from: Option<usize>, to: Option<usize>) -> String {
    let Some((from, to)) = from.zip(to) else {
        return String::new();
    };
    if to < from {
        return String::new();
    }
    content.chars().skip(from).take(to - from).collect()
}

fn layer_text_style(value: &serde_json::Value, full_content: &str) -> LayerTextStyle {
    let font = value.get("font").unwrap_or(value);
    let color = value.get("color").unwrap_or(value);
    let from = text_range_index(value, "from");
    let to = text_range_index(value, "to");
    let (line_height, line_height_unit) = text_metric(font, "lineHeight");
    let (letter_spacing, letter_spacing_unit) = text_metric(font, "letterSpacing");
    let content = exact_string(value, &["content"])
        .unwrap_or_else(|| text_content_for_range(full_content, from, to));

    LayerTextStyle {
        content,
        from,
        to,
        font_family: value_string(font, &["name", "fontFamily"]),
        post_script_name: value_string(font, &["postScriptName"]),
        font_style: value_string(font, &["type", "style"]),
        font_size: finite_number(font, &["size", "fontSize"]),
        font_weight: finite_number(font, &["fontWeight", "weight"]),
        alignment: value_string(font, &["align", "textAlign"]),
        vertical_alignment: value_string(font, &["verticalAlignment", "verticalAlign"]),
        line_height,
        line_height_unit,
        letter_spacing,
        letter_spacing_unit,
        color: value_string(color, &["value", "hex"]),
        token: color_token(color),
    }
}

fn layer_text(value: &serde_json::Value) -> Option<LayerText> {
    let text = value.get("text")?;
    let style = text.get("style").unwrap_or(text);
    let font = style.get("font").unwrap_or(style);
    let content =
        exact_string(text, &["value", "content"]).or_else(|| exact_string(style, &["content"]))?;
    let color = style.get("color").unwrap_or(style);
    let styles = text
        .get("styles")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|style| layer_text_style(style, &content))
        .filter(|style| !style.content.is_empty())
        .collect();
    Some(LayerText {
        content,
        font_family: value_string(font, &["name", "fontFamily", "postScriptName"]),
        font_size: finite_number(font, &["size", "fontSize"]),
        font_weight: finite_number(font, &["fontWeight", "weight"]),
        alignment: value_string(font, &["align", "textAlign"]),
        line_height: font
            .get("lineHeight")
            .and_then(|line_height| finite_number(line_height, &["value"]))
            .or_else(|| finite_number(font, &["lineHeight"])),
        letter_spacing: font
            .get("letterSpacing")
            .and_then(|spacing| finite_number(spacing, &["value"]))
            .or_else(|| finite_number(font, &["letterSpacing"])),
        color: value_string(color, &["value", "hex"]),
        token: color_token(color),
        styles,
    })
}

fn collect_layers(value: &serde_json::Value) -> Vec<InspectableLayer> {
    fn visit(
        value: &serde_json::Value,
        parent_id: Option<&str>,
        depth: usize,
        is_root: bool,
        order: &mut usize,
        output: &mut Vec<InspectableLayer>,
    ) {
        if depth > 64 || output.len() >= 20_000 {
            return;
        }

        let id = value_string(value, &["id", "web_id", "webId"]);
        if let Some(id) = id.as_deref() {
            let mut frame = layer_frame(value);
            if is_root {
                if let Some(frame) = frame.as_mut() {
                    frame.x = 0.0;
                    frame.y = 0.0;
                }
            }
            output.push(InspectableLayer {
                id: id.to_string(),
                parent_id: parent_id.map(str::to_string),
                name: value_string(value, &["name", "title"])
                    .unwrap_or_else(|| "未命名图层".to_string()),
                layer_type: value_string(value, &["type"]).unwrap_or_else(|| "layer".to_string()),
                depth,
                order: *order,
                frame,
                frame_is_visual: value.get("realFrame").is_some(),
                opacity: finite_number(value, &["opacity"]).unwrap_or(1.0),
                rotation: finite_number(value, &["rotation"]).unwrap_or(0.0),
                visible: value
                    .get("visible")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true),
                radius: layer_radius(value),
                fills: layer_paints(value),
                borders: layer_borders(value),
                shadows: layer_shadows(value),
                blurs: layer_blurs(value),
                text: layer_text(value),
                is_asset: value
                    .get("isAsset")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                    || value
                        .get("hasExportImage")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                    || value.get("type").and_then(serde_json::Value::as_str) == Some("bitmapLayer"),
                has_slice: false,
            });
            *order += 1;
        }

        if let Some(children) = value.get("layers").and_then(serde_json::Value::as_array) {
            for child in children {
                visit(
                    child,
                    id.as_deref().or(parent_id),
                    depth + usize::from(id.is_some()),
                    false,
                    order,
                    output,
                );
            }
        }
    }

    let Some(artboard) = value.get("artboard") else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut order = 0;
    visit(artboard, None, 0, true, &mut order, &mut output);
    output
}

fn link_slices_to_layers(layers: &mut [InspectableLayer], slices: &[LanhuSlicePayload]) {
    let slice_ids = slices
        .iter()
        .map(|slice| slice.id.as_str())
        .collect::<HashSet<_>>();
    for layer in layers {
        layer.has_slice = slice_ids.contains(layer.id.as_str());
        layer.is_asset |= layer.has_slice;
    }
}

fn align_layer_bound_comments(designs: &mut [LanhuDesignPayload], layers: &[InspectableLayer]) {
    for design in designs {
        let Some(coordinate_space) = design.coordinate_space.as_ref() else {
            continue;
        };
        for comment in &mut design.comments {
            let Some(target_id) = comment.target_id.as_deref() else {
                continue;
            };
            if target_id == design.id {
                continue;
            }
            let Some(frame) = layers
                .iter()
                .find(|layer| layer.id == target_id)
                .and_then(|layer| layer.frame.as_ref())
            else {
                continue;
            };
            let (Some(x), Some(y), Some(source_width), Some(source_height)) = (
                comment.x,
                comment.y,
                comment.source_width,
                comment.source_height,
            ) else {
                continue;
            };
            if source_width <= 0.0 || source_height <= 0.0 {
                continue;
            }
            comment.x = Some(frame.x + (x / source_width) * frame.width);
            comment.y = Some(frame.y + (y / source_height) * frame.height);
            comment.source_width = Some(coordinate_space.width);
            comment.source_height = Some(coordinate_space.height);
        }
    }
}

fn normalize_legacy_layer_frames(layers: &mut [InspectableLayer]) {
    for layer in layers {
        if layer.frame_is_visual {
            continue;
        }
        layer.frame_is_visual = true;

        let Some(frame) = layer.frame.as_mut() else {
            continue;
        };
        let rotation = layer.rotation.rem_euclid(360.0);
        if rotation.abs() < 0.0001 || (rotation - 360.0).abs() < 0.0001 {
            continue;
        }

        // Figma represents a horizontal flip as a 180-degree rotation while Lanhu's
        // visual frame only moves to the opposite horizontal edge.
        if (rotation - 180.0).abs() < 0.0001 {
            frame.x -= frame.width;
            continue;
        }

        let radians = rotation.to_radians();
        let cosine = radians.cos();
        let sine = radians.sin();
        let corners = [
            (0.0, 0.0),
            (frame.width, 0.0),
            (0.0, frame.height),
            (frame.width, frame.height),
        ];
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for (x, y) in corners {
            let rotated_x = x * cosine + y * sine;
            let rotated_y = -x * sine + y * cosine;
            min_x = min_x.min(rotated_x);
            min_y = min_y.min(rotated_y);
            max_x = max_x.max(rotated_x);
            max_y = max_y.max(rotated_y);
        }
        frame.x += min_x;
        frame.y += min_y;
        frame.width = max_x - min_x;
        frame.height = max_y - min_y;
    }
}

fn find_array<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a Vec<serde_json::Value>> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(array) = object.get(*key).and_then(serde_json::Value::as_array) {
            return Some(array);
        }
    }
    None
}

fn project_from_response(
    value: &serde_json::Value,
    route: &LanhuRoute,
    resolved_url: &str,
) -> Result<LanhuProjectPayload, String> {
    let data = response_data(value);
    let mut designs = Vec::new();
    if let Some(image_id) = route.image_id.as_deref() {
        let candidate = design_from_value(data, image_id).or_else(|| {
            data.as_object().and_then(|object| {
                object
                    .values()
                    .find_map(|child| design_from_value(child, image_id))
            })
        });
        if let Some(design) = candidate {
            designs.push(design);
        }
    } else if let Some(array) = find_array(data, &["images", "designs", "items", "list"]) {
        designs.extend(
            array
                .iter()
                .enumerate()
                .filter_map(|(index, item)| design_from_value(item, &format!("design-{index}"))),
        );
    }
    if designs.is_empty() {
        return Err("蓝湖接口暂未返回可下载的画板".to_string());
    }

    Ok(LanhuProjectPayload {
        resolved_url: resolved_url.to_string(),
        team_id: route
            .team_id
            .clone()
            .or_else(|| value_string(data, &["team_id", "teamId"]))
            .unwrap_or_default(),
        project_id: route.project_id.clone().unwrap_or_default(),
        project_name: value_string(data, &["name", "project_name", "projectName"])
            .unwrap_or_else(|| "未命名项目".to_string()),
        designs,
        slices: Vec::new(),
        layers: Vec::new(),
    })
}

fn find_json_url(value: &serde_json::Value) -> Option<String> {
    nested_url(value, &["jsonurl", "json_url", "jsonUrl"])
}

fn slice_image_url(value: &serde_json::Value) -> Option<String> {
    nested_url(
        value,
        &[
            "png_xxxhd",
            "png_xhdpi",
            "png_hdpi",
            "bitmap",
            "image_url",
            "imageUrl",
            "orgUrl",
            "org_url",
            "url",
        ],
    )
}

fn collect_slices(value: &serde_json::Value) -> Vec<LanhuSlicePayload> {
    fn visit(
        value: &serde_json::Value,
        output: &mut Vec<LanhuSlicePayload>,
        seen: &mut HashSet<String>,
        depth: usize,
    ) {
        if depth > 64 || output.len() >= 20_000 {
            return;
        }
        if let Some(object) = value.as_object() {
            let is_asset = object
                .get("isAsset")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                || object
                    .get("exportable")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                || object.contains_key("exportable")
                || object
                    .get("slice")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                || object
                    .get("image")
                    .and_then(|item| item.get("bitmap"))
                    .is_some()
                || object.get("type").and_then(serde_json::Value::as_str) == Some("bitmapLayer")
                || object
                    .get("image")
                    .and_then(|item| item.get("imageUrl"))
                    .is_some()
                || object
                    .get("images")
                    .and_then(|item| item.get("png_xxxhd"))
                    .is_some();
            if is_asset {
                if let Some(url) = slice_image_url(value) {
                    let name = value_string(value, &["name", "title"])
                        .unwrap_or_else(|| "untitled".to_string());
                    let id = value_string(value, &["web_id", "webId", "id"])
                        .unwrap_or_else(|| format!("slice-{}", output.len()));
                    let key = format!("{name}\n{url}");
                    if seen.insert(key) {
                        output.push(LanhuSlicePayload {
                            id,
                            name,
                            width: value_number(value, &["width", "w"]),
                            height: value_number(value, &["height", "h"]),
                            url,
                            org_url: nested_url(value, &["orgUrl", "org_url"]),
                            svg_url: nested_url(value, &["svg", "svgUrl", "svg_url"]),
                        });
                    }
                }
            }
            for child in object.values() {
                visit(child, output, seen, depth + 1);
            }
        } else if let Some(array) = value.as_array() {
            for child in array {
                visit(child, output, seen, depth + 1);
            }
        }
    }

    let mut output = Vec::new();
    let mut seen = HashSet::new();
    visit(value, &mut output, &mut seen, 0);
    output
}

fn poll_lanhu_capture(
    app: tauri::AppHandle,
    window: WebviewWindow,
    capture_id: String,
    source_url: String,
    parsed_url: Url,
) {
    std::thread::spawn(move || {
        let route = lanhu_route(&parsed_url);
        if route.project_id.is_none() {
            emit_failure(&app, &capture_id, "链接中缺少项目 ID");
            return;
        }
        let cookie_url = Url::parse("https://lanhuapp.com/").expect("static URL");
        for attempt in 0..240u32 {
            let still_active = app
                .state::<CaptureRuntime>()
                .inner
                .lock()
                .map(|inner| inner.active.contains(&capture_id))
                .unwrap_or(false);
            if !still_active {
                return;
            }

            let cookie = match cookie_header(&window, &cookie_url) {
                Ok(cookie) => cookie,
                Err(error) => {
                    if attempt == 239 {
                        emit_failure(&app, &capture_id, error);
                    }
                    None
                }
            };
            if let Some(cookie) = cookie {
                emit_progress(
                    &app,
                    &capture_id,
                    "authorize",
                    "已读取蓝湖登录态，Rust 正在请求设计数据…",
                    24,
                );
                let app_for_task = app.clone();
                let window_for_task = window.clone();
                let capture_for_task = capture_id.clone();
                let source_for_task = source_url.clone();
                let route_for_task = route.clone();
                let result = tauri::async_runtime::block_on(fetch_and_persist_lanhu(
                    &app_for_task,
                    &window_for_task,
                    &capture_for_task,
                    &source_for_task,
                    &route_for_task,
                    &cookie,
                ));
                match result {
                    Ok(capture) => {
                        let _ = app.emit("capture-complete", capture);
                        return;
                    }
                    Err(error) if is_retryable_lanhu_error(&error) => {
                        emit_progress(
                            &app,
                            &capture_id,
                            "authorize",
                            "蓝湖尚未返回授权数据，继续等待…",
                            24,
                        );
                    }
                    Err(error) => {
                        emit_failure(&app, &capture_id, error);
                        return;
                    }
                }
            } else if attempt % 5 == 0 {
                emit_progress(&app, &capture_id, "authorize", "等待蓝湖登录完成…", 16);
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
        emit_failure(
            &app,
            &capture_id,
            "等待蓝湖登录态超时，请确认已在弹窗中登录",
        );
    });
}

fn is_retryable_lanhu_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("未授权")
        || lower.contains("未登录")
        || lower.contains("登录")
        || lower.contains("401")
        || lower.contains("403")
        || lower.contains("暂未返回")
        || lower.contains("json")
}

async fn fetch_and_persist_lanhu(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
    capture_id: &str,
    source_url: &str,
    route: &LanhuRoute,
    cookie: &str,
) -> Result<CaptureResult, String> {
    let client = lanhu_http_client(cookie)?;
    let endpoint = api_url(route)?;
    let response = request_lanhu_json(&client, endpoint).await?;
    let mut project = project_from_response(&response, route, source_url)?;

    // `project/image` has appeared in both `{data: {result: ...}}` and `{result: ...}` forms.
    if let Some(json_url) = find_json_url(&response) {
        let json_url = safe_asset_url(&json_url)?;
        emit_progress(
            app,
            capture_id,
            "parse",
            "已取得设计 JSON，正在识别切图…",
            30,
        );
        let json_client = if json_url
            .host_str()
            .map(|host| host == "lanhuapp.com" || host.ends_with(".lanhuapp.com"))
            .unwrap_or(false)
        {
            client.clone()
        } else {
            lanhu_http_client("")?
        };
        let design_json = request_lanhu_json(&json_client, json_url).await?;
        if let (Some(design), Some(coordinate_space)) = (
            project.designs.first_mut(),
            android_coordinate_space(&design_json),
        ) {
            design.coordinate_space = Some(coordinate_space);
        }
        project.slices = collect_slices(&design_json);
        project.layers = collect_layers(&design_json);
        link_slices_to_layers(&mut project.layers, &project.slices);
        align_layer_bound_comments(&mut project.designs, &project.layers);
    }

    if project.slices.is_empty() {
        emit_progress(
            app,
            capture_id,
            "parse",
            "未识别到可导出的切图，将保存画板原图…",
            32,
        );
    } else {
        emit_progress(
            app,
            capture_id,
            "parse",
            &format!(
                "已读取 {} 个图层和 {} 个目标切图，正在保存画板…",
                project.layers.len(),
                project.slices.len()
            ),
            34,
        );
    }

    let runtime = app.state::<CaptureRuntime>();
    let source = take_source(&runtime, capture_id).unwrap_or_else(|| source_url.to_string());
    let _ = window.close();
    persist_project(app, capture_id, &source, project).await
}

fn sanitize_filename(name: &str) -> String {
    let mut value = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect::<String>();
    value = value.trim_matches([' ', '.']).to_string();
    if value.is_empty() {
        "untitled".to_string()
    } else {
        value.chars().take(80).collect()
    }
}

fn extension_for(content_type: Option<&header::HeaderValue>, url: &Url) -> &'static str {
    let mime = content_type
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if mime.contains("jpeg") {
        return "jpg";
    }
    if mime.contains("webp") {
        return "webp";
    }
    if mime.contains("avif") {
        return "avif";
    }
    if mime.contains("svg") {
        return "svg";
    }

    match Path::new(url.path())
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "jpg",
        "webp" => "webp",
        "avif" => "avif",
        "svg" => "svg",
        _ => "png",
    }
}

async fn download_design(
    client: &Client,
    output_dir: &Path,
    index: usize,
    design: LanhuDesignPayload,
) -> CapturedDesign {
    let has_comment = design.has_comment.unwrap_or(!design.comments.is_empty());
    let mut captured = CapturedDesign {
        id: design.id,
        name: design.name,
        width: design.width,
        height: design.height,
        coordinate_space: design.coordinate_space,
        update_time: design.update_time,
        has_comment,
        comments: design.comments,
        remote_url: design.url.clone(),
        local_path: None,
        error: None,
    };

    let url = match original_asset_url(&design.url) {
        Ok(url) => url,
        Err(error) => {
            captured.error = Some(error);
            return captured;
        }
    };

    let response = match client.get(url.clone()).send().await {
        Ok(response) => response,
        Err(error) => {
            captured.error = Some(format!("下载失败：{error}"));
            return captured;
        }
    };
    if !response.status().is_success() {
        captured.error = Some(format!("下载失败：HTTP {}", response.status()));
        return captured;
    }
    if response.content_length().unwrap_or_default() > MAX_IMAGE_BYTES {
        captured.error = Some("图片超过 100 MB 限制".to_string());
        return captured;
    }

    let extension = extension_for(response.headers().get(header::CONTENT_TYPE), &url);
    let bytes = match response.bytes().await {
        Ok(bytes) if bytes.len() as u64 <= MAX_IMAGE_BYTES => bytes,
        Ok(_) => {
            captured.error = Some("图片超过 100 MB 限制".to_string());
            return captured;
        }
        Err(error) => {
            captured.error = Some(format!("读取图片失败：{error}"));
            return captured;
        }
    };

    let file_name = format!(
        "{:03}_{}.{}",
        index + 1,
        sanitize_filename(&captured.name),
        extension
    );
    let file_path = output_dir.join(file_name);
    match tokio::fs::write(&file_path, bytes).await {
        Ok(()) => captured.local_path = Some(file_path.to_string_lossy().into_owned()),
        Err(error) => captured.error = Some(format!("写入文件失败：{error}")),
    }

    captured
}

async fn download_slice(
    app: &tauri::AppHandle,
    capture_id: &str,
    client: &Client,
    output_dir: &Path,
    index: usize,
    slice: LanhuSlicePayload,
) -> Option<CapturedSlice> {
    let density_dir = output_dir.join("mipmap-xxhdpi");
    let mut captured = CapturedSlice {
        id: slice.id,
        name: slice.name,
        width: slice.width,
        height: slice.height,
        remote_url: slice.url.clone(),
        output_format: "webp".to_string(),
        output_scale: 3.0,
        output_dir: density_dir.to_string_lossy().into_owned(),
        local_path: None,
        error: None,
    };

    let url = match original_asset_url(&slice.url) {
        Ok(url) => url,
        Err(error) => {
            captured.error = Some(error);
            return Some(captured);
        }
    };
    let response = match client.get(url.clone()).send().await {
        Ok(response) => response,
        Err(error) => {
            captured.error = Some(format!("切图下载失败：{error}"));
            return Some(captured);
        }
    };
    if !response.status().is_success() {
        captured.error = Some(format!("切图下载失败：HTTP {}", response.status()));
        return Some(captured);
    }
    if response.content_length().unwrap_or_default() > MAX_IMAGE_BYTES {
        captured.error = Some("切图超过 100 MB 限制".to_string());
        return Some(captured);
    }
    let bytes = match response.bytes().await {
        Ok(bytes) if bytes.len() as u64 <= MAX_IMAGE_BYTES => bytes,
        Ok(_) => {
            captured.error = Some("切图超过 100 MB 限制".to_string());
            return Some(captured);
        }
        Err(error) => {
            captured.error = Some(format!("读取切图失败：{error}"));
            return Some(captured);
        }
    };

    let decoded = match image::load_from_memory(&bytes) {
        Ok(image) => image,
        Err(error) => {
            captured.error = Some(format!("无法解码切图：{error}"));
            return Some(captured);
        }
    };
    if should_skip_slice(&decoded) {
        return None;
    }

    // Lanhu's source URL is the xxxhdpi bitmap. Android xxhdpi is 3/4 of it.
    let width = ((decoded.width() as f64) * 3.0 / 4.0).round().max(1.0) as u32;
    let height = ((decoded.height() as f64) * 3.0 / 4.0).round().max(1.0) as u32;
    captured.width = Some(width as f64);
    captured.height = Some(height as f64);
    let resized = decoded.resize_exact(width, height, FilterType::Lanczos3);
    let mut encoded = Cursor::new(Vec::new());
    if let Err(error) = resized.write_to(&mut encoded, ImageFormat::WebP) {
        captured.error = Some(format!("无法编码 WebP：{error}"));
        return Some(captured);
    }

    let runtime = app.state::<CaptureRuntime>();
    let _file_guard = runtime.file_ops.lock().await;
    if capture_cancelled(&runtime, capture_id) {
        return None;
    }
    if let Err(error) = tokio::fs::create_dir_all(&density_dir).await {
        captured.error = Some(format!("无法创建 mipmap-xxhdpi 目录：{error}"));
        return Some(captured);
    }

    let file_name = format!(
        "{:03}_{}.webp",
        index + 1,
        sanitize_filename(&captured.name)
    );
    let file_path = density_dir.join(file_name);
    match tokio::fs::write(&file_path, encoded.into_inner()).await {
        Ok(()) => captured.local_path = Some(file_path.to_string_lossy().into_owned()),
        Err(error) => captured.error = Some(format!("写入切图失败：{error}")),
    }
    Some(captured)
}

fn should_skip_slice(image: &DynamicImage) -> bool {
    if image.width() == 1 && image.height() == 1 {
        return true;
    }

    image.to_rgba8().pixels().all(|pixel| pixel[3] == 0)
}

fn export_target(platform: &str, scale: &str) -> Option<ExportTarget> {
    match (platform, scale) {
        ("android", "mdpi") => Some(ExportTarget {
            label: "mipmap-mdpi",
            directory: "mipmap-mdpi",
            suffix: "",
            factor: 1.0,
        }),
        ("android", "hdpi") => Some(ExportTarget {
            label: "mipmap-hdpi",
            directory: "mipmap-hdpi",
            suffix: "",
            factor: 1.5,
        }),
        ("android", "xhdpi") => Some(ExportTarget {
            label: "mipmap-xhdpi",
            directory: "mipmap-xhdpi",
            suffix: "",
            factor: 2.0,
        }),
        ("android", "xxhdpi") => Some(ExportTarget {
            label: "mipmap-xxhdpi",
            directory: "mipmap-xxhdpi",
            suffix: "",
            factor: 3.0,
        }),
        ("android", "xxxhdpi") => Some(ExportTarget {
            label: "mipmap-xxxhdpi",
            directory: "mipmap-xxxhdpi",
            suffix: "",
            factor: 4.0,
        }),
        ("ios", "1x") => Some(ExportTarget {
            label: "@1x",
            directory: "",
            suffix: "",
            factor: 1.0,
        }),
        ("ios", "2x") => Some(ExportTarget {
            label: "@2x",
            directory: "",
            suffix: "@2x",
            factor: 2.0,
        }),
        ("ios", "3x") => Some(ExportTarget {
            label: "@3x",
            directory: "",
            suffix: "@3x",
            factor: 3.0,
        }),
        _ => None,
    }
}

fn export_dimension(logical: f64, factor: f64) -> Result<u32, String> {
    let pixels = (logical * factor).round();
    if !pixels.is_finite() || !(1.0..=16_384.0).contains(&pixels) {
        return Err("切图目标尺寸无效".to_string());
    }
    Ok(pixels as u32)
}

fn encode_export_image(image: &DynamicImage, format: &str) -> Result<Vec<u8>, String> {
    let encoded_image = if format == "jpg" {
        let rgba = image.to_rgba8();
        let rgb = image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
            let pixel = rgba.get_pixel(x, y);
            let alpha = u16::from(pixel[3]);
            image::Rgb([
                ((u16::from(pixel[0]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                ((u16::from(pixel[1]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
                ((u16::from(pixel[2]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8,
            ])
        });
        DynamicImage::ImageRgb8(rgb)
    } else {
        image.clone()
    };
    let image_format = match format {
        "png" => ImageFormat::Png,
        "jpg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        _ => return Err("仅支持 PNG、JPG 和 WEBP 格式".to_string()),
    };
    let mut encoded = Cursor::new(Vec::new());
    encoded_image
        .write_to(&mut encoded, image_format)
        .map_err(|error| format!("无法编码切图：{error}"))?;
    Ok(encoded.into_inner())
}

async fn write_capture_metadata(
    app: &tauri::AppHandle,
    capture_id: &str,
    output_dir: &Path,
    result: &CaptureResult,
) -> Result<bool, String> {
    let runtime = app.state::<CaptureRuntime>();
    let _file_guard = runtime.file_ops.lock().await;
    if capture_cancelled(&runtime, capture_id) {
        return Ok(false);
    }
    if !tokio::fs::try_exists(output_dir)
        .await
        .map_err(|error| format!("无法检查抓取目录：{error}"))?
    {
        return Ok(false);
    }

    let metadata = serde_json::to_vec_pretty(result)
        .map_err(|error| format!("无法生成项目元数据：{error}"))?;
    tokio::fs::write(output_dir.join("capture.json"), metadata)
        .await
        .map_err(|error| format!("无法保存项目元数据：{error}"))?;
    Ok(true)
}

async fn download_slices_in_background(
    app: tauri::AppHandle,
    client: Client,
    output_dir: PathBuf,
    mut result: CaptureResult,
    pending_slices: Vec<LanhuSlicePayload>,
) {
    let total = pending_slices.len();
    let mut slices = Vec::with_capacity(total);
    for (index, slice) in pending_slices.into_iter().enumerate() {
        if capture_cancelled(&app.state::<CaptureRuntime>(), &result.capture_id) {
            return;
        }
        if let Some(captured) =
            download_slice(&app, &result.capture_id, &client, &output_dir, index, slice).await
        {
            slices.push(captured);
        }
    }

    result.slice_downloaded_count = slices
        .iter()
        .filter(|slice| slice.local_path.is_some())
        .count();
    result.slice_failed_count = slices.iter().filter(|slice| slice.error.is_some()).count();
    result.slices = slices;
    result.slices_complete = true;

    match write_capture_metadata(&app, &result.capture_id, &output_dir, &result).await {
        Ok(true) => {
            let _ = app.emit("capture-updated", result);
        }
        Ok(false) => {}
        Err(error) => eprintln!("failed to persist background slices: {error}"),
    }
}

async fn persist_project(
    app: &tauri::AppHandle,
    capture_id: &str,
    source_url: &str,
    project: LanhuProjectPayload,
) -> Result<CaptureResult, String> {
    let LanhuProjectPayload {
        resolved_url,
        team_id,
        project_id,
        project_name,
        designs: pending_designs,
        slices: pending_slices,
        layers,
    } = project;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?
        .join("captures");
    let directory_name = format!("{}_{}", capture_id, sanitize_filename(&project_name));
    let output_dir = root.join(directory_name);
    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|error| format!("无法创建抓取目录：{error}"))?;

    let client = Client::builder()
        .user_agent("Mozilla/5.0 DesignBridge/0.1")
        .referer(true)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many redirects");
            }
            if safe_asset_url(attempt.url().as_str()).is_ok() {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .default_headers({
            let mut headers = header::HeaderMap::new();
            headers.insert(
                header::REFERER,
                header::HeaderValue::from_static("https://lanhuapp.com/"),
            );
            headers
        })
        .build()
        .map_err(|error| format!("无法创建下载客户端：{error}"))?;

    let total = pending_designs.len();
    let mut designs = Vec::with_capacity(total);
    for (index, design) in pending_designs.into_iter().enumerate() {
        let captured = download_design(&client, &output_dir, index, design).await;
        designs.push(captured);
        let progress = (index + 1)
            .checked_mul(58)
            .and_then(|value| value.checked_div(total))
            .map_or(90, |value| 35 + value as u8);
        emit_progress(
            app,
            capture_id,
            "download",
            &format!("正在下载画板 {}/{}", index + 1, total),
            progress,
        );
    }

    let downloaded_count = designs
        .iter()
        .filter(|design| design.local_path.is_some())
        .count();
    let failed_count = designs.len().saturating_sub(downloaded_count);
    let slice_total_count = pending_slices.len();
    let captured_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let result = CaptureResult {
        capture_id: capture_id.to_string(),
        data_version: 4,
        captured_at,
        source_url: source_url.to_string(),
        resolved_url,
        team_id,
        project_id,
        project_name,
        output_dir: output_dir.to_string_lossy().into_owned(),
        downloaded_count,
        failed_count,
        designs,
        slices: Vec::new(),
        slice_downloaded_count: 0,
        slice_failed_count: 0,
        slice_total_count,
        slices_complete: slice_total_count == 0,
        layers,
    };

    write_capture_metadata(app, capture_id, &output_dir, &result).await?;

    let complete_message = if slice_total_count == 0 {
        "设计稿和图层已就绪"
    } else {
        "设计稿和图层已就绪，切图正在后台下载"
    };
    emit_progress(app, capture_id, "complete", complete_message, 100);
    if slice_total_count > 0 {
        tauri::async_runtime::spawn(download_slices_in_background(
            app.clone(),
            client,
            output_dir,
            result.clone(),
            pending_slices,
        ));
    }
    Ok(result)
}

#[tauri::command]
fn start_lanhu_capture(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, CaptureRuntime>,
    url: String,
) -> Result<String, String> {
    let parsed_url = lanhu_url(&url)?;
    let capture_id = capture_id();
    let window_label = format!("lanhu-{capture_id}");

    {
        let mut inner = runtime
            .inner
            .lock()
            .map_err(|_| "抓取状态不可用".to_string())?;
        inner.cancelled.remove(&capture_id);
        inner.active.insert(capture_id.clone());
        inner
            .sources
            .insert(capture_id.clone(), url.trim().to_string());
    }

    let app_for_title = app.clone();
    let capture_for_title = capture_id.clone();
    let app_for_close = app.clone();
    let capture_for_close = capture_id.clone();
    let window = WebviewWindowBuilder::new(
        &app,
        &window_label,
        WebviewUrl::External(parsed_url.clone()),
    )
    .title("蓝湖授权与抓取")
    .inner_size(1160.0, 780.0)
    .min_inner_size(820.0, 600.0)
    .center()
    .initialization_script(capture_script(&capture_id))
    .on_document_title_changed(move |window, title| {
        let _ = window.set_title("蓝湖授权与抓取");
        handle_title(
            app_for_title.clone(),
            window,
            capture_for_title.clone(),
            title,
        );
    })
    .build()
    .map_err(|error| {
        take_source(&runtime, &capture_id);
        format!("无法打开蓝湖窗口：{error}")
    })?;

    // API probing and downloads run in Rust; the WebView only keeps the signed-in session alive.
    poll_lanhu_capture(
        app.clone(),
        window.clone(),
        capture_id.clone(),
        url.trim().to_string(),
        parsed_url,
    );

    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let state = app_for_close.state::<CaptureRuntime>();
            if take_source(&state, &capture_for_close).is_some() {
                emit_failure(&app_for_close, &capture_for_close, "抓取窗口已关闭");
            }
        }
    });

    emit_progress(&app, &capture_id, "authorize", "蓝湖窗口已打开", 8);
    Ok(capture_id)
}

#[tauri::command]
fn cancel_lanhu_capture(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, CaptureRuntime>,
    capture_id: String,
) -> Result<(), String> {
    take_source(&runtime, &capture_id);
    if let Some(window) = app.get_webview_window(&format!("lanhu-{capture_id}")) {
        window
            .close()
            .map_err(|error| format!("无法关闭抓取窗口：{error}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn list_saved_captures(app: tauri::AppHandle) -> Result<Vec<CaptureResult>, String> {
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
            captures.push(capture);
        }
    }

    captures.sort_by_key(|capture| Reverse(capture.captured_at));
    captures.truncate(30);
    Ok(captures)
}

fn valid_capture_id(capture_id: &str) -> bool {
    !capture_id.is_empty()
        && capture_id.len() <= 80
        && capture_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

#[tauri::command]
async fn export_slice_variants(
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

    let output_root = capture_dir.join("exports").join(&platform);
    let base_name = sanitize_filename(&slice.name);
    let mut files = Vec::with_capacity(targets.len());
    for target in targets {
        let width = export_dimension(logical_width, target.factor)?;
        let height = export_dimension(logical_height, target.factor)?;
        let resized = source_image.resize_exact(width, height, FilterType::Lanczos3);
        let encoded = encode_export_image(&resized, &format)?;
        let target_dir = if target.directory.is_empty() {
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
async fn delete_saved_capture(app: tauri::AppHandle, capture_id: String) -> Result<(), String> {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(CaptureRuntime::default())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            start_lanhu_capture,
            cancel_lanhu_capture,
            list_saved_captures,
            export_slice_variants,
            delete_saved_capture
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn accepts_lanhu_project_and_invite_urls() {
        assert!(lanhu_url("https://lanhuapp.com/link/#/invite?sid=abc").is_ok());
        assert!(lanhu_url("https://lanhuapp.com/web/#/item/project/stage?tid=t1&pid=p1").is_ok());
    }

    #[test]
    fn extracts_route_from_hash_query() {
        let url = lanhu_url("https://lanhuapp.com/web/#/item/project/detailDetach?tid=t1&pid=p1&project_id=p1&image_id=i1").unwrap();
        let route = lanhu_route(&url);
        assert_eq!(route.team_id.as_deref(), Some("t1"));
        assert_eq!(route.project_id.as_deref(), Some("p1"));
        assert_eq!(route.image_id.as_deref(), Some("i1"));
    }

    #[test]
    fn rejects_non_lanhu_and_insecure_urls() {
        assert!(lanhu_url("https://example.com/design").is_err());
        assert!(lanhu_url("http://lanhuapp.com/web/").is_err());
    }

    #[test]
    fn validates_asset_hosts() {
        assert!(safe_asset_url("https://alipic.lanhuapp.com/example.png").is_ok());
        assert!(safe_asset_url("https://bucket.oss-cn-hangzhou.aliyuncs.com/a.png").is_ok());
        assert!(safe_asset_url("https://127.0.0.1/a.png").is_err());
        assert!(safe_asset_url("https://example.com/a.png").is_err());
    }

    #[test]
    fn removes_only_oss_preview_processing() {
        let url = original_asset_url(
            "https://alipic.lanhuapp.com/a.png?token=abc&x-oss-process=image%2Fresize,w_300",
        )
        .expect("valid asset URL");
        assert_eq!(url.as_str(), "https://alipic.lanhuapp.com/a.png?token=abc");
    }

    #[test]
    fn parses_title_protocol() {
        let title = "__DESIGNBRIDGE__|123|payload|0|1|YWJj";
        let message = parse_title_message(title).expect("valid title message");
        assert_eq!(message.capture_id, "123");
        assert_eq!(message.kind, "payload");
        assert_eq!(message.index, 0);
        assert_eq!(message.total, 1);
        assert_eq!(message.data, "YWJj");
    }

    #[test]
    fn sanitizes_cross_platform_file_names() {
        assert_eq!(sanitize_filename("登录/注册:页面?"), "登录_注册_页面_");
        assert_eq!(sanitize_filename("..."), "untitled");
    }

    #[test]
    fn validates_capture_ids_before_deletion() {
        assert!(valid_capture_id("1788429897760-1"));
        assert!(!valid_capture_id("../capture"));
        assert!(!valid_capture_id("capture/child"));
        assert!(!valid_capture_id(""));
    }

    #[test]
    fn keeps_all_detected_slices() {
        let design_json = serde_json::json!({
            "info": {
                "layers": [
                    {
                        "web_id": "background",
                        "name": "background",
                        "isAsset": true,
                        "images": {"png_xxxhd": "https://alipic.lanhuapp.com/background.png"},
                        "width": 400,
                        "height": 200
                    },
                    {
                        "web_id": "icon-tab",
                        "name": "icon/inside/tab_rat",
                        "isAsset": true,
                        "images": {"png_xxxhd": "https://alipic.lanhuapp.com/icon.png"},
                        "width": 24,
                        "height": 24
                    }
                ]
            }
        });
        let slices = collect_slices(&design_json);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].name, "background");
        assert_eq!(slices[1].name, "icon/inside/tab_rat");
        assert_eq!(slices[1].url, "https://alipic.lanhuapp.com/icon.png");
    }

    #[test]
    fn reads_android_coordinate_space_from_artboard_frame() {
        let design_json = serde_json::json!({
            "artboard": {
                "frame": {"left": 54561, "top": 48540, "width": 375, "height": 2337}
            }
        });
        assert_eq!(
            android_coordinate_space(&design_json),
            Some(DesignCoordinateSpace {
                platform: "android".to_string(),
                width: 375.0,
                height: 2337.0,
                unit: "dp".to_string(),
            })
        );
    }

    #[test]
    fn extracts_comments_from_the_latest_design_version() {
        let design = serde_json::json!({
            "id": "design-1",
            "name": "Commented design",
            "width": 187.5,
            "height": 400,
            "url": "https://alipic.lanhuapp.com/design.png",
            "latest_version": "version-2",
            "versions": [
                {
                    "id": "version-1",
                    "version_info": "版本1",
                    "width": 187.5,
                    "height": 400,
                    "comments": [{"id": "old", "content": "旧评论"}]
                },
                {
                    "id": "version-2",
                    "version_info": "版本2",
                    "width": 187.5,
                    "height": 400,
                    "comments": [{
                        "id": "comment-1",
                        "content": "夜间#042A36-#440A0B",
                        "create_time": 1787739576,
                        "position_x": 0.25,
                        "position_y": 0.5,
                        "text": "7",
                        "user": {"nickname": "郑向萍"},
                        "replies": [{
                            "id": "reply-1",
                            "content": "已确认",
                            "user": {"name": "Reviewer"}
                        }]
                    }]
                }
            ]
        });

        let captured = design_from_value(&design, "fallback").expect("valid design");
        assert_eq!(captured.comments.len(), 1);
        let comment = &captured.comments[0];
        assert_eq!(comment.id, "comment-1");
        assert_eq!(comment.index, 7);
        assert_eq!(comment.author, "郑向萍");
        assert_eq!(comment.content, "夜间#042A36-#440A0B");
        assert_eq!(comment.created_at.as_deref(), Some("1787739576"));
        assert_eq!(comment.x, Some(46.875));
        assert_eq!(comment.y, Some(200.0));
        assert_eq!(comment.version_id.as_deref(), Some("version-2"));
        assert_eq!(comment.version_name.as_deref(), Some("版本2"));
        assert_eq!(comment.replies[0].content, "已确认");
    }

    #[test]
    fn prefers_lanhu_visual_frame_for_rotated_layers() {
        let layer = serde_json::json!({
            "rotation": 180,
            "frame": {"left": 273, "top": 1021, "width": 12, "height": 12},
            "realFrame": {"left": 261, "top": 1021, "width": 12, "height": 12}
        });

        assert_eq!(
            layer_frame(&layer),
            Some(LayerFrame {
                x: 261.0,
                y: 1021.0,
                width: 12.0,
                height: 12.0,
            })
        );
    }

    #[test]
    fn normalizes_rotated_frames_from_legacy_captures_once() {
        let mut layer = InspectableLayer {
            id: "legacy-flipped-icon".to_string(),
            parent_id: None,
            name: "icon/general/enter".to_string(),
            layer_type: "bitmapLayer".to_string(),
            depth: 1,
            order: 0,
            frame: Some(LayerFrame {
                x: 273.0,
                y: 1021.0,
                width: 12.0,
                height: 12.0,
            }),
            frame_is_visual: false,
            opacity: 1.0,
            rotation: 180.0,
            visible: true,
            radius: LayerRadius::default(),
            fills: Vec::new(),
            borders: Vec::new(),
            shadows: Vec::new(),
            blurs: Vec::new(),
            text: None,
            is_asset: true,
            has_slice: true,
        };

        normalize_legacy_layer_frames(std::slice::from_mut(&mut layer));
        assert_eq!(layer.frame.as_ref().unwrap().x, 261.0);
        assert_eq!(layer.frame.as_ref().unwrap().y, 1021.0);
        assert!(layer.frame_is_visual);

        normalize_legacy_layer_frames(std::slice::from_mut(&mut layer));
        assert_eq!(layer.frame.as_ref().unwrap().x, 261.0);
    }

    #[test]
    fn extracts_inspectable_layers_and_prefers_path_radius() {
        let design_json = serde_json::json!({
            "artboard": {
                "id": "root",
                "name": "Screen",
                "type": "artboard",
                "frame": {"left": 54561, "top": 48540, "width": 375, "height": 2337},
                "layers": [{
                    "id": "5046:66772",
                    "name": "Frame 427318893",
                    "type": "artboard",
                    "frame": {"left": 221, "top": 138, "width": 80, "height": 28},
                    "opacity": 1,
                    "visible": true,
                    "radius": {"topLeft": 0, "topRight": 0, "bottomRight": 0, "bottomLeft": 0},
                    "paths": [{
                        "radius": {"topLeft": 20, "topRight": 20, "bottomRight": 20, "bottomLeft": 20}
                    }],
                    "style": {
                        "fills": [{
                            "type": "color",
                            "isEnabled": true,
                            "opacity": 1,
                            "boundVariables": {"color": {"name": "sys/bg/bg-1"}},
                            "color": {"value": "rgba(245,245,245,1)"}
                        }],
                        "borders": [],
                        "shadows": [],
                        "blurs": []
                    },
                    "layers": []
                }]
            }
        });

        let layers = collect_layers(&design_json);
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].frame.as_ref().unwrap().x, 0.0);
        assert_eq!(layers[0].frame.as_ref().unwrap().y, 0.0);
        assert_eq!(layers[1].parent_id.as_deref(), Some("root"));
        assert_eq!(layers[1].frame.as_ref().unwrap().x, 221.0);
        assert_eq!(layers[1].frame.as_ref().unwrap().y, 138.0);
        assert!(!layers[1].frame_is_visual);
        assert_eq!(layers[1].radius.top_left, 20.0);
        assert_eq!(layers[1].fills[0].token.as_deref(), Some("sys/bg/bg-1"));
        assert_eq!(
            layers[1].fills[0].color.as_deref(),
            Some("rgba(245,245,245,1)")
        );
    }

    #[test]
    fn extracts_every_rich_text_style_run() {
        let design_json = serde_json::json!({
            "artboard": {
                "id": "root",
                "frame": {"left": 0, "top": 0, "width": 375, "height": 800},
                "layers": [{
                    "id": "mixed-text",
                    "name": "Goalkeeper, #22",
                    "type": "textLayer",
                    "frame": {"left": 82, "top": 150, "width": 90, "height": 14},
                    "text": {
                        "value": "Goalkeeper, #22",
                        "style": {
                            "content": "Goalkeeper, #22",
                            "font": {"name": "Sofascore Sans", "size": 12, "fontWeight": 400},
                            "color": {"value": "rgba(153,153,153,1)"}
                        },
                        "styles": [
                            {
                                "from": 0,
                                "to": 12,
                                "content": "Goalkeeper, ",
                                "font": {
                                    "name": "Sofascore Sans",
                                    "postScriptName": "Sofascore Sans-Regular",
                                    "type": "Regular",
                                    "size": 12,
                                    "fontWeight": 400,
                                    "align": "left",
                                    "verticalAlignment": "center",
                                    "letterSpacing": {"unit": "percent", "value": 0},
                                    "lineHeight": {"unit": "AUTO"}
                                },
                                "color": {"value": "rgba(153,153,153,1)"}
                            },
                            {
                                "from": 12,
                                "to": 15,
                                "content": "#22",
                                "font": {
                                    "name": "Sofascore Sans",
                                    "postScriptName": "Sofascore Sans-Regular",
                                    "type": "Regular",
                                    "size": 12,
                                    "fontWeight": 400,
                                    "align": "left",
                                    "verticalAlignment": "center",
                                    "letterSpacing": {"unit": "percent", "value": 0},
                                    "lineHeight": {"unit": "AUTO"}
                                },
                                "color": {"value": "rgba(208,164,5,1)"}
                            }
                        ]
                    },
                    "layers": []
                }]
            }
        });

        let layers = collect_layers(&design_json);
        let text = layers[1].text.as_ref().expect("text layer");
        assert_eq!(text.styles.len(), 2);
        assert_eq!(text.styles[0].content, "Goalkeeper, ");
        assert_eq!(
            text.styles[0].letter_spacing_unit.as_deref(),
            Some("percent")
        );
        assert_eq!(text.styles[0].line_height_unit.as_deref(), Some("AUTO"));
        assert_eq!(text.styles[1].content, "#22");
        assert_eq!(text.styles[1].color.as_deref(), Some("rgba(208,164,5,1)"));
    }

    #[test]
    fn links_downloadable_slice_to_its_layer_id() {
        let design_json = serde_json::json!({
            "artboard": {
                "id": "root",
                "frame": {"left": 0, "top": 0, "width": 375, "height": 800},
                "layers": [{
                    "id": "asset-layer",
                    "name": "icon/inside/tab-stats-red",
                    "type": "bitmapLayer",
                    "frame": {"left": 242, "top": 102, "width": 12, "height": 12},
                    "image": {"imageUrl": "https://alipic.lanhuapp.com/icon.png"},
                    "layers": []
                }]
            }
        });
        let slices = collect_slices(&design_json);
        let mut layers = collect_layers(&design_json);
        link_slices_to_layers(&mut layers, &slices);

        assert_eq!(slices.len(), 1);
        assert!(layers[1].has_slice);
        assert!(layers[1].is_asset);
    }

    #[test]
    fn skips_one_pixel_and_fully_transparent_slices() {
        let one_pixel = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])));
        assert!(should_skip_slice(&one_pixel));

        let transparent =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(20, 12, Rgba([255, 255, 255, 0])));
        assert!(should_skip_slice(&transparent));

        let visible =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(20, 12, Rgba([255, 255, 255, 255])));
        assert!(!should_skip_slice(&visible));
    }

    #[test]
    fn limits_slice_exports_to_supported_platform_scales() {
        assert_eq!(
            export_target("android", "xxhdpi"),
            Some(ExportTarget {
                label: "mipmap-xxhdpi",
                directory: "mipmap-xxhdpi",
                suffix: "",
                factor: 3.0,
            })
        );
        assert_eq!(export_target("ios", "3x").unwrap().suffix, "@3x");
        assert!(export_target("web", "1x").is_none());
        assert!(export_target("android", "5x").is_none());
    }

    #[test]
    fn calculates_slice_export_pixel_dimensions() {
        assert_eq!(export_dimension(20.0, 1.0).unwrap(), 20);
        assert_eq!(export_dimension(20.0, 1.5).unwrap(), 30);
        assert_eq!(export_dimension(20.0, 4.0).unwrap(), 80);
        assert!(export_dimension(0.0, 3.0).is_err());
    }

    #[test]
    fn limits_slice_export_encoders_and_flattens_jpg_alpha() {
        let transparent =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([20, 40, 60, 0])));
        assert!(encode_export_image(&transparent, "png").is_ok());
        assert!(encode_export_image(&transparent, "webp").is_ok());
        let jpg = encode_export_image(&transparent, "jpg").unwrap();
        let pixel = image::load_from_memory(&jpg)
            .unwrap()
            .to_rgb8()
            .get_pixel(0, 0)
            .0;
        assert!(pixel.iter().all(|channel| *channel > 245));
        assert!(encode_export_image(&transparent, "avif").is_err());
    }

    #[test]
    fn normalizes_lanhu_bare_json_url() {
        let value = serde_json::json!({
            "version": {"json_url": "alipic.lanhuapp.com/design.json"}
        });
        assert_eq!(
            find_json_url(&value).as_deref(),
            Some("https://alipic.lanhuapp.com/design.json")
        );
    }

    #[test]
    fn finds_figma_bitmap_layer_slices() {
        let design_json = serde_json::json!({
            "artboard": {
                "layers": [{
                    "type": "bitmapLayer",
                    "id": "I1",
                    "name": "icon/inside/tab-overview-gary",
                    "image": {
                        "imageUrl": "https://lanhu-oss-2537-2.lanhuapp.com/FigmaSlicePNG044970105cf846b14bc6ed13f63137b7.png",
                        "svgUrl": "https://lanhu-oss-2537-2.lanhuapp.com/FigmaSliceSVG6615709efefa7da031590121756a086a.svg"
                    },
                    "frame": {"width": 12, "height": 12}
                }]
            },
            "assets": []
        });
        let slices = collect_slices(&design_json);
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].name, "icon/inside/tab-overview-gary");
        assert_eq!(slices[0].url, "https://lanhu-oss-2537-2.lanhuapp.com/FigmaSlicePNG044970105cf846b14bc6ed13f63137b7.png");
    }
}
