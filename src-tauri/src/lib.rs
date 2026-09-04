use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::{imageops::FilterType, ImageFormat};
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

const TITLE_PREFIX: &str = "__DESIGNBRIDGE__";
const MAX_IMAGE_BYTES: u64 = 100 * 1024 * 1024;
static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct CaptureRuntime {
    inner: Mutex<CaptureRuntimeInner>,
}

#[derive(Default)]
struct CaptureRuntimeInner {
    active: HashSet<String>,
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LanhuDesignPayload {
    id: String,
    name: String,
    width: Option<f64>,
    height: Option<f64>,
    url: String,
    update_time: Option<String>,
    has_comment: Option<bool>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturedDesign {
    id: String,
    name: String,
    width: Option<f64>,
    height: Option<f64>,
    update_time: Option<String>,
    has_comment: bool,
    remote_url: String,
    local_path: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturedSlice {
    id: String,
    name: String,
    width: Option<f64>,
    height: Option<f64>,
    remote_url: String,
    output_format: String,
    output_scale: f64,
    output_dir: String,
    local_path: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptureResult {
    capture_id: String,
    captured_at: u64,
    source_url: String,
    resolved_url: String,
    team_id: String,
    project_id: String,
    project_name: String,
    output_dir: String,
    downloaded_count: usize,
    failed_count: usize,
    designs: Vec<CapturedDesign>,
    #[serde(default)]
    slices: Vec<CapturedSlice>,
    #[serde(default)]
    slice_downloaded_count: usize,
    #[serde(default)]
    slice_failed_count: usize,
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

fn design_from_value(value: &serde_json::Value, fallback_id: &str) -> Option<LanhuDesignPayload> {
    let url = design_image_url(value)?;
    Some(LanhuDesignPayload {
        id: value_string(value, &["id", "image_id", "imageId", "web_id"])
            .unwrap_or_else(|| fallback_id.to_string()),
        name: value_string(value, &["name", "title"]).unwrap_or_else(|| "未命名画板".to_string()),
        width: value_number(value, &["width", "w"]),
        height: value_number(value, &["height", "h"]),
        url,
        update_time: value_string(value, &["update_time", "updateTime"]),
        has_comment: value
            .get("has_comment")
            .or_else(|| value.get("hasComment"))
            .and_then(serde_json::Value::as_bool),
    })
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
        project.slices = collect_slices(&design_json);
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
            &format!("已识别 {} 个目标切图，开始导出…", project.slices.len()),
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
    let mut captured = CapturedDesign {
        id: design.id,
        name: design.name,
        width: design.width,
        height: design.height,
        update_time: design.update_time,
        has_comment: design.has_comment.unwrap_or(false),
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
    client: &Client,
    output_dir: &Path,
    index: usize,
    slice: LanhuSlicePayload,
) -> CapturedSlice {
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

    if let Err(error) = tokio::fs::create_dir_all(&density_dir).await {
        captured.error = Some(format!("无法创建 mipmap-xxhdpi 目录：{error}"));
        return captured;
    }
    let url = match original_asset_url(&slice.url) {
        Ok(url) => url,
        Err(error) => {
            captured.error = Some(error);
            return captured;
        }
    };
    let response = match client.get(url.clone()).send().await {
        Ok(response) => response,
        Err(error) => {
            captured.error = Some(format!("切图下载失败：{error}"));
            return captured;
        }
    };
    if !response.status().is_success() {
        captured.error = Some(format!("切图下载失败：HTTP {}", response.status()));
        return captured;
    }
    if response.content_length().unwrap_or_default() > MAX_IMAGE_BYTES {
        captured.error = Some("切图超过 100 MB 限制".to_string());
        return captured;
    }
    let bytes = match response.bytes().await {
        Ok(bytes) if bytes.len() as u64 <= MAX_IMAGE_BYTES => bytes,
        Ok(_) => {
            captured.error = Some("切图超过 100 MB 限制".to_string());
            return captured;
        }
        Err(error) => {
            captured.error = Some(format!("读取切图失败：{error}"));
            return captured;
        }
    };

    let decoded = match image::load_from_memory(&bytes) {
        Ok(image) => image,
        Err(error) => {
            captured.error = Some(format!("无法解码切图：{error}"));
            return captured;
        }
    };
    // Lanhu's source URL is the xxxhdpi bitmap. Android xxhdpi is 3/4 of it.
    let width = ((decoded.width() as f64) * 3.0 / 4.0).round().max(1.0) as u32;
    let height = ((decoded.height() as f64) * 3.0 / 4.0).round().max(1.0) as u32;
    captured.width = Some(width as f64);
    captured.height = Some(height as f64);
    let resized = decoded.resize_exact(width, height, FilterType::Lanczos3);
    let mut encoded = Cursor::new(Vec::new());
    if let Err(error) = resized.write_to(&mut encoded, ImageFormat::WebP) {
        captured.error = Some(format!("无法编码 WebP：{error}"));
        return captured;
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
    captured
}

async fn persist_project(
    app: &tauri::AppHandle,
    capture_id: &str,
    source_url: &str,
    project: LanhuProjectPayload,
) -> Result<CaptureResult, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?
        .join("captures");
    let directory_name = format!(
        "{}_{}",
        capture_id,
        sanitize_filename(&project.project_name)
    );
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

    let total = project.designs.len();
    let mut designs = Vec::with_capacity(total);
    for (index, design) in project.designs.into_iter().enumerate() {
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

    let slices_total = project.slices.len();
    let mut slices = Vec::with_capacity(slices_total);
    for (index, slice) in project.slices.into_iter().enumerate() {
        let captured = download_slice(&client, &output_dir, index, slice).await;
        slices.push(captured);
        emit_progress(
            app,
            capture_id,
            "download",
            &format!(
                "正在导出切图 {}/{}（WebP / mipmap-xxhdpi）",
                index + 1,
                slices_total
            ),
            94,
        );
    }

    let downloaded_count = designs
        .iter()
        .filter(|design| design.local_path.is_some())
        .count();
    let failed_count = designs.len().saturating_sub(downloaded_count);
    let slice_downloaded_count = slices
        .iter()
        .filter(|slice| slice.local_path.is_some())
        .count();
    let slice_failed_count = slices.len().saturating_sub(slice_downloaded_count);
    let captured_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let result = CaptureResult {
        capture_id: capture_id.to_string(),
        captured_at,
        source_url: source_url.to_string(),
        resolved_url: project.resolved_url,
        team_id: project.team_id,
        project_id: project.project_id,
        project_name: project.project_name,
        output_dir: output_dir.to_string_lossy().into_owned(),
        downloaded_count,
        failed_count,
        designs,
        slices,
        slice_downloaded_count,
        slice_failed_count,
    };

    let metadata = serde_json::to_vec_pretty(&result)
        .map_err(|error| format!("无法生成项目元数据：{error}"))?;
    tokio::fs::write(output_dir.join("capture.json"), metadata)
        .await
        .map_err(|error| format!("无法保存项目元数据：{error}"))?;

    emit_progress(app, capture_id, "complete", "抓取完成", 100);
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
        if let Ok(capture) = serde_json::from_slice::<CaptureResult>(&bytes) {
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
async fn delete_saved_capture(app: tauri::AppHandle, capture_id: String) -> Result<(), String> {
    if !valid_capture_id(&capture_id) {
        return Err("无效的抓取记录 ID".to_string());
    }

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
            delete_saved_capture
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

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
