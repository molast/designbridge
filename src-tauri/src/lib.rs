use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use reqwest::{header, redirect::Policy, Client};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use url::{Host, Url};

#[cfg(unix)]
use std::{
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
};

pub mod design_data;
mod app_lifecycle;
mod browser_extension;
pub(crate) use browser_extension::*;
mod capture_storage;
pub(crate) use capture_storage::*;
mod lanhu_data;
pub(crate) use lanhu_data::*;
mod slice_export;
pub(crate) use slice_export::*;
#[cfg(test)]
mod tests;

use design_data::*;

const TITLE_PREFIX: &str = "__DESIGNBRIDGE__";
const MAX_IMAGE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_BROWSER_MESSAGE_BYTES: usize = 256 * 1024;
const BROWSER_EXTENSION_ID: &str = "gdpjdkhhfielmlddencemafipiebldcf";
const BROWSER_NATIVE_HOST_NAME: &str = "com.designbridge.browser";
const BROWSER_EXTENSION_VERSION: &str = "0.3.2";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const BROWSER_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(8);
static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn version_tuple(value: &str) -> [u64; 3] {
    let normalized = value.trim().trim_start_matches('v');
    let core = normalized.split(['-', '+']).next().unwrap_or(normalized);
    let mut parts = core.split('.').map(|part| part.parse::<u64>().unwrap_or(0));
    [parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0)]
}

#[derive(Default)]
struct CaptureRuntime {
    inner: Mutex<CaptureRuntimeInner>,
    file_ops: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct CaptureRuntimeInner {
    active: HashSet<String>,
    browser_started: HashSet<String>,
    cancelled: HashSet<String>,
    sources: HashMap<String, String>,
    chunks: HashMap<String, ChunkAccumulator>,
    browser_heartbeats: HashMap<String, BrowserHeartbeat>,
}

#[derive(Clone, Debug)]
struct BrowserHeartbeat {
    received_at: Instant,
    extension_version: String,
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserCaptureRequested {
    capture_id: String,
    source_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserExtensionHeartbeat {
    browser: String,
    extension_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowserCaptureRequest {
    version: u8,
    #[serde(rename = "type")]
    request_type: String,
    #[serde(default)]
    capture_id: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    cookie: String,
    #[serde(default)]
    auth_token: String,
    #[serde(default)]
    browser: String,
    #[serde(default)]
    extension_version: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserCaptureResponse {
    ok: bool,
    capture_id: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserExtensionInstallResult {
    extension_path: String,
    extension_id: String,
    configured_browsers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserExtensionStatus {
    browser: String,
    installed: bool,
    enabled: bool,
    auto_capture_ready: bool,
    native_host_installed: bool,
    connected: bool,
    version_current: bool,
    extension_version: Option<String>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteSavedDesignsRequest {
    project_id: String,
    design_ids: Vec<String>,
    #[serde(default)]
    delete_capture: bool,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateInfo {
    available: bool,
    current_version: String,
    latest_version: String,
    release_url: String,
    release_name: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    name: Option<String>,
    draft: bool,
    prerelease: bool,
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
    child: Option<String>,
    link_type: Option<String>,
    single_page: bool,
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
            "child" => route.child = Some(value.into_owned()),
            "designbridge_single_page" => route.single_page = value == "1" || value == "true",
            "type" => {
                if value == "image" {
                    route.single_page = true;
                }
                route.link_type = Some(value.into_owned());
            }
            _ => {}
        }
    }
    if route.image_id.is_some()
        && route.child.is_none()
        && !matches!(route.link_type.as_deref(), Some("set" | "sectionImageChange"))
    {
        route.single_page = true;
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

fn reserve_capture(
    runtime: &CaptureRuntime,
    capture_id: &str,
    source_url: &str,
) -> Result<(), String> {
    let mut inner = runtime
        .inner
        .lock()
        .map_err(|_| "抓取状态不可用".to_string())?;
    if !inner.active.is_empty() {
        return Err("已有设计稿正在抓取，请完成或取消后再试".to_string());
    }
    inner.cancelled.remove(capture_id);
    inner.active.insert(capture_id.to_string());
    inner
        .sources
        .insert(capture_id.to_string(), source_url.trim().to_string());
    Ok(())
}

fn claim_browser_capture(
    runtime: &CaptureRuntime,
    capture_id: &str,
    request_route: &LanhuRoute,
) -> Result<String, String> {
    if !valid_capture_id(capture_id) {
        return Err("浏览器扩展返回了无效的抓取任务 ID".to_string());
    }

    let mut inner = runtime
        .inner
        .lock()
        .map_err(|_| "抓取状态不可用".to_string())?;
    if !inner.active.contains(capture_id) {
        return Err("抓取任务已取消或已经超时，请在客户端重新开始".to_string());
    }
    if inner.browser_started.contains(capture_id) {
        return Err("浏览器扩展已经提交过该抓取任务".to_string());
    }

    let source_url = inner
        .sources
        .get(capture_id)
        .cloned()
        .ok_or_else(|| "找不到浏览器抓取任务".to_string())?;
    let source_route = lanhu_route(&lanhu_url(&source_url)?);
    if source_route.project_id != request_route.project_id
        || source_route.image_id != request_route.image_id
    {
        return Err("浏览器返回的设计稿与客户端请求不一致".to_string());
    }

    inner.browser_started.insert(capture_id.to_string());
    Ok(source_url)
}

fn mark_browser_capture_started(runtime: &CaptureRuntime, capture_id: &str) -> Result<(), String> {
    runtime
        .inner
        .lock()
        .map_err(|_| "抓取状态不可用".to_string())?
        .browser_started
        .insert(capture_id.to_string());
    Ok(())
}

fn expire_pending_browser_capture(runtime: &CaptureRuntime, capture_id: &str) -> bool {
    let Ok(mut inner) = runtime.inner.lock() else {
        return false;
    };
    if !inner.active.contains(capture_id) || inner.browser_started.contains(capture_id) {
        return false;
    }

    inner.active.remove(capture_id);
    inner.sources.remove(capture_id);
    inner
        .chunks
        .retain(|key, _| !key.starts_with(&format!("{capture_id}:")));
    true
}

#[cfg(unix)]
fn read_browser_frame(stream: &mut impl Read) -> Result<Option<Vec<u8>>, String> {
    let mut length = [0u8; 4];
    match stream.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("无法读取浏览器扩展消息长度：{error}")),
    }
    let length = u32::from_ne_bytes(length) as usize;
    if length == 0 || length > MAX_BROWSER_MESSAGE_BYTES {
        return Err("浏览器扩展消息大小无效".to_string());
    }
    let mut payload = vec![0u8; length];
    stream
        .read_exact(&mut payload)
        .map_err(|error| format!("无法读取浏览器扩展消息：{error}"))?;
    Ok(Some(payload))
}

#[cfg(unix)]
fn write_browser_frame(stream: &mut impl Write, payload: &[u8]) -> Result<(), String> {
    if payload.is_empty() || payload.len() > MAX_BROWSER_MESSAGE_BYTES {
        return Err("浏览器扩展响应大小无效".to_string());
    }
    stream
        .write_all(&(payload.len() as u32).to_ne_bytes())
        .and_then(|_| stream.write_all(payload))
        .and_then(|_| stream.flush())
        .map_err(|error| format!("无法写入浏览器扩展响应：{error}"))
}

fn queue_browser_capture(
    app: tauri::AppHandle,
    request: BrowserCaptureRequest,
) -> Result<String, String> {
    if request.version != 1 || request.request_type != "capture" {
        return Err("浏览器扩展协议版本不受支持".to_string());
    }
    if request.cookie.len() > 128 * 1024 || request.auth_token.len() > 16 * 1024 {
        return Err("未读取到有效的蓝湖登录状态".to_string());
    }
    if request.cookie.is_empty() && request.auth_token.is_empty() {
        return Err("未读取到有效的蓝湖登录状态".to_string());
    }
    if !request.cookie.is_empty() {
        header::HeaderValue::from_str(&request.cookie)
            .map_err(|_| "蓝湖登录状态格式无效".to_string())?;
    }
    if !request.auth_token.is_empty() {
        lanhu_authorization_header(&request.auth_token)?;
    }

    let request_url = lanhu_url(&request.url)?;
    let request_route = lanhu_route(&request_url);
    if request_route.project_id.is_none() || request_route.image_id.is_none() {
        return Err("请在浏览器中打开具体的蓝湖设计稿页面".to_string());
    }

    let runtime = app.state::<CaptureRuntime>();
    let (capture_id, source_url) = if let Some(requested_id) = request.capture_id.as_deref() {
        let source_url = claim_browser_capture(&runtime, requested_id, &request_route)?;
        (requested_id.to_string(), source_url)
    } else {
        let capture_id = capture_id();
        let source_url = request.url.trim().to_string();
        reserve_capture(&runtime, &capture_id, &source_url)?;
        if let Err(error) = mark_browser_capture_started(&runtime, &capture_id) {
            take_source(&runtime, &capture_id);
            return Err(error);
        }
        (capture_id, source_url)
    };
    let route = lanhu_route(&lanhu_url(&source_url)?);
    let _ = app.emit(
        "browser-capture-requested",
        BrowserCaptureRequested {
            capture_id: capture_id.clone(),
            source_url: source_url.clone(),
        },
    );
    emit_progress(
        &app,
        &capture_id,
        "authorize",
        "已收到浏览器扩展请求，Rust 正在读取设计数据…",
        18,
    );

    let task_app = app.clone();
    let task_capture_id = capture_id.clone();
    tauri::async_runtime::spawn(async move {
        let result = fetch_and_persist_browser_lanhu(
            &task_app,
            &task_capture_id,
            &source_url,
            &route,
            &request.cookie,
            &request.auth_token,
        )
        .await;
        match result {
            Ok(capture) => {
                let _ = task_app.emit("capture-complete", capture);
            }
            Err(error) => {
                take_source(&task_app.state::<CaptureRuntime>(), &task_capture_id);
                emit_failure(&task_app, &task_capture_id, error);
            }
        }
    });
    Ok(capture_id)
}

fn record_browser_heartbeat(
    app: &tauri::AppHandle,
    request: &BrowserCaptureRequest,
) -> Result<(), String> {
    if request.version != 1 || request.request_type != "heartbeat" {
        return Err("浏览器扩展协议版本不受支持".to_string());
    }
    browser_extension_manager_target(&request.browser)?;
    if request.extension_version.is_empty()
        || request.extension_version.len() > 32
        || request.session_id.is_empty()
        || request.session_id.len() > 80
    {
        return Err("浏览器扩展心跳信息无效".to_string());
    }

    app.state::<CaptureRuntime>()
        .inner
        .lock()
        .map_err(|_| "浏览器扩展状态不可用".to_string())?
        .browser_heartbeats
        .insert(
            request.browser.clone(),
            BrowserHeartbeat {
                received_at: Instant::now(),
                extension_version: request.extension_version.clone(),
            },
        );
    let _ = app.emit(
        "browser-extension-heartbeat",
        BrowserExtensionHeartbeat {
            browser: request.browser.clone(),
            extension_version: request.extension_version.clone(),
        },
    );
    Ok(())
}

fn record_browser_capture_error(
    app: &tauri::AppHandle,
    request: &BrowserCaptureRequest,
) -> Result<(), String> {
    let capture_id = request
        .capture_id
        .as_deref()
        .ok_or_else(|| "浏览器扩展未提供抓取任务 ID".to_string())?;
    if !valid_capture_id(capture_id) || request.message.is_empty() || request.message.len() > 500 {
        return Err("浏览器扩展错误信息无效".to_string());
    }

    let source_url = take_source(&app.state::<CaptureRuntime>(), capture_id)
        .ok_or_else(|| "抓取任务已取消或已经超时".to_string())?;
    let expected_route = lanhu_route(&lanhu_url(&source_url)?);
    let request_route = lanhu_route(&lanhu_url(&request.url)?);
    if expected_route.project_id != request_route.project_id
        || expected_route.image_id != request_route.image_id
    {
        return Err("浏览器返回的设计稿与客户端请求不一致".to_string());
    }

    emit_failure(app, capture_id, &request.message);
    Ok(())
}

#[cfg(unix)]
fn handle_browser_connection(app: tauri::AppHandle, mut stream: UnixStream) {
    let response = match read_browser_frame(&mut stream) {
        Ok(Some(payload)) => match serde_json::from_slice::<BrowserCaptureRequest>(&payload) {
            Ok(request) if request.request_type == "capture" => {
                match queue_browser_capture(app, request) {
                    Ok(capture_id) => BrowserCaptureResponse {
                        ok: true,
                        capture_id: Some(capture_id),
                        message: "已开始抓取当前蓝湖设计稿".to_string(),
                    },
                    Err(message) => BrowserCaptureResponse {
                        ok: false,
                        capture_id: None,
                        message,
                    },
                }
            }
            Ok(request) if request.request_type == "heartbeat" => {
                match record_browser_heartbeat(&app, &request) {
                    Ok(()) => BrowserCaptureResponse {
                        ok: true,
                        capture_id: None,
                        message: "DesignBridge 客户端已连接".to_string(),
                    },
                    Err(message) => BrowserCaptureResponse {
                        ok: false,
                        capture_id: None,
                        message,
                    },
                }
            }
            Ok(request) if request.request_type == "captureError" => {
                match record_browser_capture_error(&app, &request) {
                    Ok(()) => BrowserCaptureResponse {
                        ok: true,
                        capture_id: request.capture_id,
                        message: "抓取失败信息已发送到客户端".to_string(),
                    },
                    Err(message) => BrowserCaptureResponse {
                        ok: false,
                        capture_id: None,
                        message,
                    },
                }
            }
            Ok(_) => BrowserCaptureResponse {
                ok: false,
                capture_id: None,
                message: "浏览器扩展协议版本不受支持".to_string(),
            },
            Err(_) => BrowserCaptureResponse {
                ok: false,
                capture_id: None,
                message: "无法解析浏览器扩展请求".to_string(),
            },
        },
        Ok(None) => return,
        Err(message) => BrowserCaptureResponse {
            ok: false,
            capture_id: None,
            message,
        },
    };
    if let Ok(payload) = serde_json::to_vec(&response) {
        let _ = write_browser_frame(&mut stream, &payload);
    }
}

#[cfg(unix)]
fn start_browser_extension_listener(app: tauri::AppHandle) -> Result<(), String> {
    let socket_path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法确定应用数据目录：{error}"))?
        .join("browser-extension.sock");
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建浏览器扩展数据目录：{error}"))?;
    }
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)
            .map_err(|error| format!("无法清理浏览器扩展通信文件：{error}"))?;
    }
    let listener = UnixListener::bind(&socket_path)
        .map_err(|error| format!("无法启动浏览器扩展通信服务：{error}"))?;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("无法设置浏览器扩展通信权限：{error}"))?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_browser_connection(app.clone(), stream),
                Err(error) => eprintln!("browser extension connection failed: {error}"),
            }
        }
    });
    Ok(())
}

#[cfg(not(unix))]
fn start_browser_extension_listener(_app: tauri::AppHandle) -> Result<(), String> {
    Ok(())
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
    inner.browser_started.remove(capture_id);
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

fn lanhu_authorization_header(auth_token: &str) -> Result<header::HeaderValue, String> {
    let encoded = BASE64.encode(format!("{}:", auth_token.trim()));
    header::HeaderValue::from_str(&format!("Basic {encoded}"))
        .map_err(|_| "蓝湖登录令牌格式无效".to_string())
}

fn lanhu_http_client(cookie: &str, auth_token: &str) -> Result<Client, String> {
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
    if !auth_token.is_empty() {
        headers.insert(
            header::AUTHORIZATION,
            lanhu_authorization_header(auth_token)?,
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

async fn fetch_lanhu_project(
    app: &tauri::AppHandle,
    capture_id: &str,
    source_url: &str,
    route: &LanhuRoute,
    cookie: &str,
    auth_token: &str,
) -> Result<LanhuProjectPayload, String> {
    let client = lanhu_http_client(cookie, auth_token)?;
    let endpoint = api_url(route)?;
    let response = request_lanhu_json(&client, endpoint).await?;
    let mut project = project_from_response(&response, route, source_url)?;

    // The image endpoint is needed for the selected page's layers, while the
    // project endpoint gives us sibling page metadata without downloading
    // their bitmaps. Some editor links omit `child`, so image_id alone is
    // enough to trigger this second request.
    // An editor link may omit `child` (for example `type=set`) while still
    // pointing at one page. Always request the project image list so sibling
    // pages are available in that case as well.
    if route.image_id.is_some() && !route.single_page {
        let mut pages_route = route.clone();
        pages_route.image_id = None;
        let mut pages_project = request_lanhu_json(&client, api_url(&pages_route)?).await
            .ok()
            .and_then(|response| project_from_response(&response, &pages_route, source_url).ok());
        // Some Lanhu responses scope the child route to one page. Retry the
        // project-wide list without child only when the first result is empty
        // or single-page, preserving the normal child-scoped result otherwise.
        if pages_project.as_ref().map(|value| value.designs.len()).unwrap_or(0) <= 1 {
            let mut broad_pages_route = pages_route.clone();
            broad_pages_route.child = None;
            if let Ok(response) = request_lanhu_json(&client, api_url(&broad_pages_route)?).await {
                if let Ok(candidate) = project_from_response(&response, &broad_pages_route, source_url) {
                    if candidate.designs.len() > pages_project.as_ref().map(|value| value.designs.len()).unwrap_or(0) {
                        pages_project = Some(candidate);
                    }
                }
            }
        }
        if let Some(pages_project) = pages_project {
            let selected_id = route.image_id.as_deref().unwrap_or_default();
            let selected_design = project.designs.into_iter().find(|design| design.id == selected_id);
            let mut designs = pages_project.designs;
            if let Some(selected_design) = selected_design {
                designs.retain(|design| design.id != selected_id);
                designs.insert(0, selected_design);
            } else if let Some(position) = designs.iter().position(|design| design.id == selected_id) {
                let selected_design = designs.remove(position);
                designs.insert(0, selected_design);
            }
            project.designs = designs;
        }
    }

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
            lanhu_http_client("", "")?
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

    Ok(project)
}

async fn fetch_and_persist_lanhu(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
    capture_id: &str,
    source_url: &str,
    route: &LanhuRoute,
    cookie: &str,
) -> Result<CaptureResult, String> {
    let project = fetch_lanhu_project(app, capture_id, source_url, route, cookie, "").await?;
    let source = take_source(&app.state::<CaptureRuntime>(), capture_id)
        .unwrap_or_else(|| source_url.to_string());
    let _ = window.close();
    persist_project(app, capture_id, &source, project).await
}

async fn fetch_and_persist_browser_lanhu(
    app: &tauri::AppHandle,
    capture_id: &str,
    source_url: &str,
    route: &LanhuRoute,
    cookie: &str,
    auth_token: &str,
) -> Result<CaptureResult, String> {
    let project =
        fetch_lanhu_project(app, capture_id, source_url, route, cookie, auth_token).await?;
    let source = take_source(&app.state::<CaptureRuntime>(), capture_id)
        .unwrap_or_else(|| source_url.to_string());
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
    let mut captured = captured_design_metadata(&design);

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

fn captured_design_metadata(design: &LanhuDesignPayload) -> CapturedDesign {
    CapturedDesign {
        id: design.id.clone(),
        name: design.name.clone(),
        width: design.width,
        height: design.height,
        coordinate_space: design.coordinate_space.clone(),
        update_time: design.update_time.clone(),
        has_comment: design.has_comment.unwrap_or(!design.comments.is_empty()),
        comments: design.comments.clone(),
        remote_url: design.url.clone(),
        local_path: None,
        error: None,
        layers: Vec::new(),
        slices: Vec::new(),
        slice_downloaded_count: 0,
        slice_failed_count: 0,
        slice_total_count: 0,
        slices_complete: true,
    }
}

fn failed_design_count(designs: &[CapturedDesign]) -> usize {
    designs.iter().filter(|design| design.error.is_some()).count()
}

fn normalize_legacy_layer_radius(layers: &mut [InspectableLayer]) {
    for layer in layers {
        let all_zero = [
            layer.radius.top_left,
            layer.radius.top_right,
            layer.radius.bottom_right,
            layer.radius.bottom_left,
        ]
        .into_iter()
        .all(|value| value == Some(0.0));
        if all_zero {
            layer.radius = LayerRadius::default();
        }
    }
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
        captured.error = Some("切图为空像素或 1 × 1 px，无法生成预览".to_string());
        return Some(captured);
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
    design_id: String,
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
        let completed = index + 1;
        let percent = completed
            .checked_mul(100)
            .and_then(|value| value.checked_div(total))
            .map_or(100, |value| value as u8);
        emit_progress(
            &app,
            &result.capture_id,
            "slices",
            &format!("正在下载切图 {completed}/{total}"),
            percent,
        );
    }

    result.slice_downloaded_count = slices
        .iter()
        .filter(|slice| slice.local_path.is_some())
        .count();
    result.slice_failed_count = slices.iter().filter(|slice| slice.error.is_some()).count();
    result.slices = slices;
    result.slices_complete = true;
    if let Some(design) = result.designs.iter_mut().find(|design| design.id == design_id) {
        design.slices = result.slices.clone();
        design.slice_downloaded_count = result.slice_downloaded_count;
        design.slice_failed_count = result.slice_failed_count;
        design.slice_total_count = result.slice_total_count;
        design.slices_complete = true;
    }

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
    let lazy_pages = total > 1;
    for (index, design) in pending_designs.into_iter().enumerate() {
        let captured = if lazy_pages && index > 0 {
            captured_design_metadata(&design)
        } else {
            download_design(&client, &output_dir, index, design).await
        };
        designs.push(captured);
        let progress = (index + 1)
            .checked_mul(58)
            .and_then(|value| value.checked_div(total))
            .map_or(90, |value| 35 + value as u8);
        emit_progress(
            app,
            capture_id,
            "download",
            &format!("{}画板 {}/{}", if lazy_pages && index > 0 { "已读取" } else { "正在下载" }, index + 1, total),
            progress,
        );
    }

    let downloaded_count = designs
        .iter()
        .filter(|design| design.local_path.is_some())
        .count();
    // Pages after the first one are intentionally metadata-only for multi-page
    // captures and are loaded on demand. They are not failed downloads.
    let failed_count = failed_design_count(&designs);
    let slice_total_count = pending_slices.len();
    let primary_design_id = designs.first().map(|design| design.id.clone());
    if let Some(design) = designs.first_mut() {
        design.layers = layers.clone();
        design.slice_total_count = slice_total_count;
        design.slices_complete = slice_total_count == 0;
    }
    let captured_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let result = CaptureResult {
        capture_id: capture_id.to_string(),
        data_version: 5,
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

    if slice_total_count == 0 {
        emit_progress(app, capture_id, "complete", "设计稿和图层已就绪", 100);
    } else {
        emit_progress(
            app,
            capture_id,
            "slices",
            &format!("正在下载切图 0/{slice_total_count}"),
            0,
        );
    }
    if slice_total_count > 0 {
        tauri::async_runtime::spawn(download_slices_in_background(
            app.clone(),
            client,
            output_dir,
            result.clone(),
            pending_slices,
            primary_design_id.expect("slice downloads require a design"),
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

    reserve_capture(&runtime, &capture_id, url.trim())?;

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
    .inner_size(900.0, 620.0)
    .min_inner_size(700.0, 480.0)
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
    mark_capture_cancelled(&runtime, &capture_id);
    if let Some(window) = app.get_webview_window(&format!("lanhu-{capture_id}")) {
        window
            .close()
            .map_err(|error| format!("无法关闭抓取窗口：{error}"))?;
    }
    Ok(())
}

#[tauri::command]
fn codex_mcp_status() -> Result<bool, String> {
    let Some(home) = dirs::home_dir() else {
        return Ok(false);
    };
    let config_path = home.join(".codex").join("config.toml");
    let config = match std::fs::read_to_string(config_path) {
        Ok(config) => config,
        Err(_) => return Ok(false),
    };
    if !config.contains("[mcp_servers.designbridge]") {
        return Ok(false);
    }

    let candidates = [
        home.join("plugins/designbridge/scripts/mcp.sh"),
        home.join(".codex/plugins/cache/personal/designbridge/0.1.0/scripts/mcp.sh"),
    ];
    Ok(candidates.iter().any(|path| path.is_file()))
}

#[tauri::command]
fn install_codex_plugin(app: tauri::AppHandle) -> Result<String, String> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "无法确定 DesignBridge 项目目录".to_string())?;
    let bundled_installer = app
        .path()
        .resource_dir()
        .map_err(|error| format!("无法确定客户端资源目录：{error}"))?
        .join("designbridge-installer");
    let installer_root = if bundled_installer.join("install-plugin.sh").is_file() {
        bundled_installer
    } else if project_root.join("install-plugin.sh").is_file() {
        project_root
    } else {
        return Err("客户端中缺少 DesignBridge 插件安装资源，请重新安装最新版客户端".to_string());
    };
    let script = installer_root.join("install-plugin.sh");

    let script_arg = script.to_string_lossy().into_owned();
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("/bin/zsh");
        command
            .arg("-lic")
            .arg("exec /bin/bash \"$1\"")
            .arg("designbridge-install")
            .arg(&script_arg);
        command
    } else {
        let mut command = Command::new("bash");
        command.arg(&script);
        command
    };
    let output = command
        .current_dir(&installer_root)
        .output()
        .map_err(|error| format!("无法启动插件安装程序：{error}"))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        Err(format!("插件安装失败（退出码 {:?}）", output.status.code()))
    } else {
        Err(detail)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(CaptureRuntime::default())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app_lifecycle::configure(app.handle())?;
            start_browser_extension_listener(app.handle().clone())
                .map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_lanhu_capture,
            start_browser_extension_capture,
            cancel_lanhu_capture,
            list_saved_captures,
            check_for_update,
            export_slice_variants,
            delete_saved_capture,
            delete_saved_designs,
            clear_saved_design_cache,
            install_browser_extension,
            browser_extension_status,
            open_browser_extension_manager,
            codex_mcp_status,
            install_codex_plugin
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
