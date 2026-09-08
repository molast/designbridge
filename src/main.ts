import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import { mountPhotoPreview, showPhotoPreview, type PreviewItem } from "./photo-preview";
import {
  mountPlatformPicker,
  showPlatformPicker,
  type PlatformFrame,
  type TargetPlatform,
} from "./platform-picker";
import { mountSliceExportPanel, showSliceExportPanel } from "./slice-export-panel";
import { mountSidebarToggle } from "./sidebar-toggle";
import {
  mountMcpLinkButton,
  showMcpLinkButton,
  showMcpRefreshState,
  type McpLinkTarget,
} from "./mcp-link-button";

type CaptureProgress = {
  captureId: string;
  stage: string;
  message: string;
  percent: number;
};

type CaptureFailure = {
  captureId: string;
  message: string;
};

type BrowserCaptureRequested = {
  captureId: string;
  sourceUrl: string;
};

type BrowserExtensionInstallResult = {
  extensionPath: string;
  extensionId: string;
  configuredBrowsers: string[];
};

type BrowserExtensionStatus = {
  browser: string;
  installed: boolean;
  enabled: boolean;
  autoCaptureReady: boolean;
  nativeHostInstalled: boolean;
  connected: boolean;
  versionCurrent: boolean;
  extensionVersion: string | null;
};

type BrowserExtensionHeartbeat = {
  browser: string;
  extensionVersion: string;
};

type CaptureMethod = "browser" | "webview";

type DesignCommentReply = {
  id: string;
  author: string;
  content: string;
  createdAt: string | null;
};

type DesignComment = {
  id: string;
  index: number;
  author: string;
  content: string;
  createdAt: string | null;
  resolved: boolean;
  x: number | null;
  y: number | null;
  sourceWidth: number | null;
  sourceHeight: number | null;
  versionId: string | null;
  versionName: string | null;
  targetId: string | null;
  targetType: string | null;
  replies: DesignCommentReply[];
};

type CapturedDesign = {
  id: string;
  name: string;
  width: number | null;
  height: number | null;
  coordinateSpace?: PlatformFrame | null;
  updateTime: string | null;
  hasComment: boolean;
  comments: DesignComment[];
  remoteUrl: string;
  localPath: string | null;
  error: string | null;
};

type CapturedSlice = {
  id: string;
  name: string;
  width: number | null;
  height: number | null;
  remoteUrl: string;
  outputFormat: string;
  outputScale: number;
  outputDir: string;
  localPath: string | null;
  error: string | null;
};

type LayerFrame = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type LayerRadius = {
  topLeft: number;
  topRight: number;
  bottomRight: number;
  bottomLeft: number;
};

type LayerPaint = {
  paintType: string;
  color: string | null;
  token: string | null;
  opacity: number;
};

type LayerBorder = {
  width: number;
  style: string;
  color: string | null;
  token: string | null;
  opacity: number;
};

type LayerShadow = {
  shadowType: string;
  color: string | null;
  offsetX: number;
  offsetY: number;
  blur: number;
  spread: number;
};

type LayerBlur = {
  blurType: string;
  radius: number;
};

type LayerTextStyle = {
  content: string;
  from: number | null;
  to: number | null;
  fontFamily: string | null;
  postScriptName: string | null;
  fontStyle: string | null;
  fontSize: number | null;
  fontWeight: number | null;
  alignment: string | null;
  verticalAlignment: string | null;
  lineHeight: number | null;
  lineHeightUnit: string | null;
  letterSpacing: number | null;
  letterSpacingUnit: string | null;
  color: string | null;
  token: string | null;
};

type LayerText = {
  content: string;
  fontFamily: string | null;
  fontSize: number | null;
  fontWeight: number | null;
  alignment: string | null;
  lineHeight: number | null;
  letterSpacing: number | null;
  color: string | null;
  token: string | null;
  styles?: LayerTextStyle[];
};

type InspectableLayer = {
  id: string;
  parentId: string | null;
  name: string;
  layerType: string;
  depth: number;
  order: number;
  frame: LayerFrame | null;
  opacity: number;
  rotation: number;
  visible: boolean;
  radius: LayerRadius;
  fills: LayerPaint[];
  borders: LayerBorder[];
  shadows: LayerShadow[];
  blurs: LayerBlur[];
  text: LayerText | null;
  isAsset: boolean;
  hasSlice: boolean;
};

type CaptureResult = {
  captureId: string;
  dataVersion?: number;
  capturedAt: number;
  sourceUrl: string;
  resolvedUrl: string;
  teamId: string;
  projectId: string;
  projectName: string;
  outputDir: string;
  downloadedCount: number;
  failedCount: number;
  designs: CapturedDesign[];
  slices: CapturedSlice[];
  sliceDownloadedCount: number;
  sliceFailedCount: number;
  sliceTotalCount?: number;
  slicesComplete?: boolean;
  layers?: InspectableLayer[];
};

type CaptureAttempt = {
  key: string;
  sourceUrl: string;
  captureId: string | null;
  status: "capturing" | "failed";
  message: string;
  percent: number;
  updatedAt: number;
};

type HistoryEntry =
  | { key: string; updatedAt: number; capture: CaptureResult; attempt?: never }
  | { key: string; updatedAt: number; capture?: never; attempt: CaptureAttempt };

const captureForm = document.querySelector<HTMLFormElement>("#capture-form")!;
const urlInput = document.querySelector<HTMLInputElement>("#lanhu-url")!;
const captureButton = document.querySelector<HTMLButtonElement>("#capture-button")!;
const cancelButton = document.querySelector<HTMLButtonElement>("#cancel-button")!;
const captureMethodButtons = [...document.querySelectorAll<HTMLButtonElement>("[data-capture-method]")];
const browserConnectionState = document.querySelector<HTMLElement>("#browser-connection-state")!;
const fieldError = document.querySelector<HTMLElement>("#field-error")!;
const progressPanel = document.querySelector<HTMLElement>("#progress-panel")!;
const progressMessage = document.querySelector<HTMLElement>("#progress-message")!;
const progressValue = document.querySelector<HTMLElement>("#progress-value")!;
const progressBar = document.querySelector<HTMLElement>("#progress-bar")!;
const appStatus = document.querySelector<HTMLElement>("#app-status")!;
const installBrowserExtensionButton = document.querySelector<HTMLButtonElement>("#install-browser-extension-button")!;
const installPluginButton = document.querySelector<HTMLButtonElement>("#install-plugin-button")!;
const historyCount = document.querySelector<HTMLElement>("#history-count")!;
const historyList = document.querySelector<HTMLElement>("#history-list")!;
const sidebar = document.querySelector<HTMLElement>("#sidebar")!;
const sidebarSurface = document.querySelector<HTMLElement>("#sidebar-surface")!;
const sidebarResizeHandle = document.querySelector<HTMLElement>("#sidebar-resize-handle")!;
const sidebarToggleRoot = document.querySelector<HTMLElement>("#sidebar-toggle-root")!;
const resultMeta = document.querySelector<HTMLElement>("#result-meta")!;
const emptyState = document.querySelector<HTMLElement>("#empty-state")!;
const designInspector = document.querySelector<HTMLElement>("#design-inspector")!;
const inspectorDetails = document.querySelector<HTMLElement>("#inspector-details")!;
const platformPickerRoot = document.querySelector<HTMLElement>("#platform-picker-root")!;
const mcpLinkRoot = document.querySelector<HTMLElement>("#mcp-link-root")!;
const canvasTitle = document.querySelector<HTMLElement>("#canvas-title")!;
const zoomOutButton = document.querySelector<HTMLButtonElement>("#zoom-out-button")!;
const zoomInButton = document.querySelector<HTMLButtonElement>("#zoom-in-button")!;
const zoomInput = document.querySelector<HTMLInputElement>("#zoom-input")!;
const artboardScroll = document.querySelector<HTMLElement>("#artboard-scroll")!;
const artboardWrap = document.querySelector<HTMLElement>("#artboard-wrap")!;
const artboardImage = document.querySelector<HTMLImageElement>("#artboard-image")!;
const sliceOutlines = document.querySelector<HTMLElement>("#slice-outlines")!;
const commentMarkers = document.querySelector<HTMLElement>("#comment-markers")!;
const commentPopover = document.querySelector<HTMLElement>("#comment-popover")!;
const layerHighlight = document.querySelector<HTMLElement>("#layer-highlight")!;
const layerHighlightSize = document.querySelector<HTMLElement>("#layer-highlight-size")!;
const layerDetails = document.querySelector<HTMLElement>("#layer-details")!;
const sliceExportRoot = document.querySelector<HTMLElement>("#slice-export-root")!;
const deleteConfirmDialog = document.querySelector<HTMLDialogElement>("#delete-confirm-dialog")!;
const deleteConfirmMessage = document.querySelector<HTMLElement>("#delete-confirm-message")!;
const deleteConfirmError = document.querySelector<HTMLElement>("#delete-confirm-error")!;
const deleteCancelButton = document.querySelector<HTMLButtonElement>("#delete-cancel-button")!;
const deleteConfirmButton = document.querySelector<HTMLButtonElement>("#delete-confirm-button")!;
const browserExtensionDialog = document.querySelector<HTMLDialogElement>("#browser-extension-dialog")!;
const browserExtensionPath = document.querySelector<HTMLElement>("#browser-extension-path")!;
const browserExtensionBrowsers = document.querySelector<HTMLElement>("#browser-extension-browsers")!;
const browserExtensionBrowser = document.querySelector<HTMLSelectElement>("#browser-extension-browser")!;
const openBrowserExtensionManagerButton = document.querySelector<HTMLButtonElement>("#open-browser-extension-manager-button")!;
const openBrowserExtensionFolderButton = document.querySelector<HTMLButtonElement>("#open-browser-extension-folder-button")!;
const browserExtensionDoneButton = document.querySelector<HTMLButtonElement>("#browser-extension-done-button")!;
const browserExtensionError = document.querySelector<HTMLElement>("#browser-extension-error")!;
const previewRoot = document.querySelector<HTMLElement>("#preview-root")!;

const CURRENT_CAPTURE_VERSION = 4;
let activeCaptureId: string | null = null;
const replacementCaptureIds = new Map<string, string>();
let selectedCapture: CaptureResult | null = null;
let selectedLayerId: string | null = null;
let selectedCommentId: string | null = null;
let selectedPlatform: TargetPlatform = "android";
let hitStack: { captureId: string; x: number; y: number; layerIds: string[]; index: number } | null = null;
let panState: {
  pointerId: number;
  startX: number;
  startY: number;
  panX: number;
  panY: number;
  moved: boolean;
} | null = null;
let canvasZoom = 100;
let canvasPanX = 0;
let canvasPanY = 0;
let history: CaptureResult[] = [];
let captureAttempts: CaptureAttempt[] = [];
let activeCaptureKey: string | null = null;
let selectedHistoryKey: string | null = null;
let pendingDeleteCaptureId: string | null = null;
let inspectorFitFrame = 0;
let sidebarResizeState: { pointerId: number; startX: number; width: number } | null = null;
let installedBrowserExtensionPath: string | null = null;

const MIN_CANVAS_ZOOM = 4;
const MAX_CANVAS_ZOOM = 400;
const CANVAS_ZOOM_STEPS = [4, 8, 12.5, 25, 50, 75, 100, 125, 150, 200, 300, 400];
const MIN_SIDEBAR_WIDTH = 220;
const MAX_SIDEBAR_WIDTH = 480;
const SIDEBAR_WIDTH_STORAGE_KEY = "designbridge.sidebar.width";
const SIDEBAR_COLLAPSED_STORAGE_KEY = "designbridge.sidebar.collapsed";
const CAPTURE_ATTEMPTS_STORAGE_KEY = "designbridge.capture.attempts";
const HISTORY_SELECTED_KEY_STORAGE_KEY = "designbridge.history.selected-key";
const HISTORY_SCROLL_TOP_STORAGE_KEY = "designbridge.history.scroll-top";
const CAPTURE_METHOD_STORAGE_KEY = "designbridge.capture.method";
const BROWSER_STORAGE_KEY = "designbridge.browser";
let sidebarWidth = storedNumber(SIDEBAR_WIDTH_STORAGE_KEY, MIN_SIDEBAR_WIDTH);
let sidebarCollapsed = storedValue(SIDEBAR_COLLAPSED_STORAGE_KEY) === "true";
let captureMethod: CaptureMethod = storedValue(CAPTURE_METHOD_STORAGE_KEY) === "webview" ? "webview" : "browser";

mountPhotoPreview(previewRoot);
mountPlatformPicker(platformPickerRoot, changeTargetPlatform);
mountSliceExportPanel(sliceExportRoot);
mountMcpLinkButton(mcpLinkRoot, refreshSelectedDesign);
mountSidebarToggle(sidebarToggleRoot, {
  collapsed: sidebarCollapsed,
  onChange: setSidebarCollapsed,
});
applySidebarState();

const storedBrowser = storedValue(BROWSER_STORAGE_KEY);
if (["chrome", "edge", "brave", "chromium"].includes(storedBrowser || "")) {
  browserExtensionBrowser.value = storedBrowser!;
}

function setCaptureMethod(method: CaptureMethod, persist = true) {
  captureMethod = method;
  captureMethodButtons.forEach((button) => {
    const active = button.dataset.captureMethod === method;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-checked", String(active));
  });
  if (persist) storeValue(CAPTURE_METHOD_STORAGE_KEY, method);
}

setCaptureMethod(captureMethod, false);

function storedValue(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storedNumber(key: string, fallback: number): number {
  const value = Number(storedValue(key));
  return Number.isFinite(value) ? value : fallback;
}

function storeValue(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // The panel still works when storage is unavailable.
  }
}

function removeStoredValue(key: string) {
  try {
    window.localStorage.removeItem(key);
  } catch {
    // The panel still works when storage is unavailable.
  }
}

function captureSourceKey(sourceUrl: string): string {
  try {
    const url = new URL(sourceUrl);
    const params = new URLSearchParams(url.search);
    const hashQueryIndex = url.hash.indexOf("?");
    if (hashQueryIndex >= 0) {
      const hashParams = new URLSearchParams(url.hash.slice(hashQueryIndex + 1));
      hashParams.forEach((value, key) => {
        if (!params.has(key)) params.set(key, value);
      });
    }

    const projectId = params.get("project_id") || params.get("pid");
    const imageId = params.get("image_id");
    if (projectId && imageId) return `lanhu:${projectId.toLowerCase()}:${imageId.toLowerCase()}`;
    if (imageId) return `lanhu:image:${imageId.toLowerCase()}`;

    url.hostname = url.hostname.toLowerCase();
    url.searchParams.delete("fromEditor");
    return `url:${url.toString()}`;
  } catch {
    return `url:${sourceUrl.trim()}`;
  }
}

function designMcpLink(capture: CaptureResult, design: CapturedDesign, nodeId: string | null = null): string {
  const link = `designbridge://design/${encodeURIComponent(capture.projectId)}/${encodeURIComponent(design.id)}`;
  if (!nodeId) return link;

  const query = new URLSearchParams();
  query.set("node-id", nodeId);
  return `${link}?${query.toString()}`;
}

function designMcpTarget(
  capture: CaptureResult,
  design: CapturedDesign,
  layer: InspectableLayer | null = null,
): McpLinkTarget {
  return {
    link: designMcpLink(capture, design, layer?.id || null),
    layerName: layer?.name || null,
  };
}

function loadCaptureAttempts(): CaptureAttempt[] {
  const stored = storedValue(CAPTURE_ATTEMPTS_STORAGE_KEY);
  if (!stored) return [];

  try {
    const parsed = JSON.parse(stored) as unknown;
    if (!Array.isArray(parsed)) return [];
    const attempts = parsed.flatMap((value): CaptureAttempt[] => {
      if (!value || typeof value !== "object") return [];
      const item = value as Partial<CaptureAttempt>;
      if (typeof item.sourceUrl !== "string" || !item.sourceUrl.trim()) return [];
      const wasCapturing = item.status === "capturing";
      return [{
        key: captureSourceKey(item.sourceUrl),
        sourceUrl: item.sourceUrl,
        captureId: typeof item.captureId === "string" ? item.captureId : null,
        status: "failed",
        message: wasCapturing
          ? "上次抓取未完成，可重试"
          : typeof item.message === "string" && item.message
            ? item.message
            : "抓取失败，可重试",
        percent: Number.isFinite(item.percent) ? Math.max(0, Math.min(100, Number(item.percent))) : 0,
        updatedAt: Number.isFinite(item.updatedAt) ? Number(item.updatedAt) : Date.now(),
      }];
    });
    return attempts.filter(
      (attempt, index) => attempts.findIndex((candidate) => candidate.key === attempt.key) === index,
    );
  } catch {
    return [];
  }
}

function persistCaptureAttempts() {
  storeValue(CAPTURE_ATTEMPTS_STORAGE_KEY, JSON.stringify(captureAttempts));
}

function setSelectedHistoryKey(key: string | null) {
  selectedHistoryKey = key;
  if (key) storeValue(HISTORY_SELECTED_KEY_STORAGE_KEY, key);
  else removeStoredValue(HISTORY_SELECTED_KEY_STORAGE_KEY);
}

captureAttempts = loadCaptureAttempts();
selectedHistoryKey = storedValue(HISTORY_SELECTED_KEY_STORAGE_KEY);
persistCaptureAttempts();

function clampSidebarWidth(width: number): number {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, width));
}

function applySidebarWidth(width: number, persist = false) {
  sidebarWidth = clampSidebarWidth(width);
  sidebar.style.setProperty("--sidebar-width", `${sidebarWidth}px`);
  sidebarResizeHandle.setAttribute("aria-valuemin", String(MIN_SIDEBAR_WIDTH));
  sidebarResizeHandle.setAttribute("aria-valuemax", String(MAX_SIDEBAR_WIDTH));
  sidebarResizeHandle.setAttribute("aria-valuenow", String(Math.round(sidebarWidth)));
  if (persist) storeValue(SIDEBAR_WIDTH_STORAGE_KEY, String(Math.round(sidebarWidth)));
}

function applySidebarState() {
  applySidebarWidth(sidebarWidth);
  sidebar.classList.toggle("is-collapsed", sidebarCollapsed);
  sidebarSurface.inert = sidebarCollapsed;
  sidebarSurface.setAttribute("aria-hidden", String(sidebarCollapsed));
  sidebarResizeHandle.tabIndex = sidebarCollapsed ? -1 : 0;
}

function setSidebarCollapsed(collapsed: boolean) {
  sidebarCollapsed = collapsed;
  applySidebarState();
  storeValue(SIDEBAR_COLLAPSED_STORAGE_KEY, String(collapsed));
}

function fitInspectorToViewport() {
  inspectorFitFrame = 0;
  if (designInspector.classList.contains("hidden")) return;
  const top = Math.max(76, designInspector.getBoundingClientRect().top);
  const availableHeight = Math.max(280, window.innerHeight - top - 20);
  designInspector.style.height = `${availableHeight}px`;
}

function scheduleInspectorFit() {
  if (inspectorFitFrame) return;
  inspectorFitFrame = window.requestAnimationFrame(fitInspectorToViewport);
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>'"]/g, (character) => {
    const entities: Record<string, string> = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      "'": "&#39;",
      '"': "&quot;",
    };
    return entities[character];
  });
}

function displayDate(timestamp: number): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp * 1000));
}

function numberValue(value: number, maximumFractionDigits = 2): string {
  return new Intl.NumberFormat("zh-CN", { maximumFractionDigits }).format(value);
}

function unitValue(value: number): string {
  return `${numberValue(value)}${selectedPlatform === "ios" ? "pt" : "dp"}`;
}

function imageSize(width: number | null, height: number | null): string {
  if (width == null || height == null) return "尺寸未知";
  return `${Math.round(width)} × ${Math.round(height)} px`;
}

function localImageSource(localPath: string | null): string | null {
  return localPath ? convertFileSrc(localPath) : null;
}

function androidFrame(design: CapturedDesign | undefined): PlatformFrame | null {
  if (!design) return null;
  if (design.coordinateSpace?.platform === "android") return design.coordinateSpace;
  if (design.width == null || design.height == null) return null;
  return {
    platform: "android",
    width: design.width * 2,
    height: design.height * 2,
    unit: "dp",
  };
}

function updatePlatformPicker(capture: CaptureResult | null) {
  const design = capture?.designs.find((item) => item.error == null);
  showPlatformPicker(androidFrame(design), selectedPlatform);
}

function changeTargetPlatform(platform: TargetPlatform) {
  if (selectedPlatform === platform) return;
  selectedPlatform = platform;
  updatePlatformPicker(selectedCapture);
  renderLayerSelection();
}

function sliceDensity(slice: CapturedSlice): string {
  const pathParts = slice.outputDir.replace(/\\/g, "/").split("/").filter(Boolean);
  return pathParts[pathParts.length - 1] || "mipmap-xxhdpi";
}

function captureLayers(capture: CaptureResult): InspectableLayer[] {
  return capture.layers || [];
}

function slicesForLayer(capture: CaptureResult, layer: InspectableLayer): CapturedSlice[] {
  const exactMatches = capture.slices.filter((slice) => slice.id === layer.id);
  if (exactMatches.length > 0 || !layer.hasSlice) return exactMatches;
  return capture.slices.filter((slice) => slice.name === layer.name);
}

function sliceTotal(capture: CaptureResult): number {
  return Math.max(capture.sliceTotalCount || 0, capture.slices.length);
}

function slicesComplete(capture: CaptureResult): boolean {
  return capture.slicesComplete ?? true;
}

function currentCoordinate(): PlatformFrame | null {
  const design = selectedCapture?.designs.find((item) => item.localPath && item.error == null);
  return androidFrame(design);
}

function currentDesign(capture: CaptureResult | null = selectedCapture): CapturedDesign | null {
  return capture?.designs.find((item) => item.localPath && item.error == null) || null;
}

function designComments(design: CapturedDesign | null): DesignComment[] {
  return design?.comments || [];
}

function applyCanvasZoom() {
  const coordinate = currentCoordinate();
  if (!coordinate) return;
  const scale = canvasZoom / 100;
  artboardWrap.style.width = `${coordinate.width * scale}px`;
  artboardWrap.style.aspectRatio = `${coordinate.width} / ${coordinate.height}`;
  zoomInput.value = numberValue(canvasZoom);
  zoomOutButton.disabled = canvasZoom <= MIN_CANVAS_ZOOM;
  zoomInButton.disabled = canvasZoom >= MAX_CANVAS_ZOOM;
  window.requestAnimationFrame(positionCommentPopover);
}

function applyCanvasPan() {
  artboardWrap.style.setProperty("--canvas-pan-x", `${canvasPanX}px`);
  artboardWrap.style.setProperty("--canvas-pan-y", `${canvasPanY}px`);
  window.requestAnimationFrame(positionCommentPopover);
}

function setCanvasZoom(nextZoom: number, anchor?: { x: number; y: number }) {
  const next = Math.max(MIN_CANVAS_ZOOM, Math.min(MAX_CANVAS_ZOOM, nextZoom));
  if (!Number.isFinite(next)) return;
  const oldBounds = artboardWrap.getBoundingClientRect();
  const anchorPoint = anchor || {
    x: artboardScroll.getBoundingClientRect().left + artboardScroll.clientWidth / 2,
    y: artboardScroll.getBoundingClientRect().top + artboardScroll.clientHeight / 2,
  };
  const relativeX = oldBounds.width > 0 ? (anchorPoint.x - oldBounds.left) / oldBounds.width : 0.5;
  const relativeY = oldBounds.height > 0 ? (anchorPoint.y - oldBounds.top) / oldBounds.height : 0.5;

  canvasZoom = Math.round(next * 10) / 10;
  hitStack = null;
  applyCanvasZoom();

  if (oldBounds.width > 0 && oldBounds.height > 0) {
    const newBounds = artboardWrap.getBoundingClientRect();
    const newAnchorX = newBounds.left + relativeX * newBounds.width;
    const newAnchorY = newBounds.top + relativeY * newBounds.height;
    canvasPanX += anchorPoint.x - newAnchorX;
    canvasPanY += anchorPoint.y - newAnchorY;
    applyCanvasPan();
  }
}

function steppedZoom(direction: -1 | 1): number {
  if (direction > 0) {
    return CANVAS_ZOOM_STEPS.find((step) => step > canvasZoom + 0.05) || MAX_CANVAS_ZOOM;
  }
  return [...CANVAS_ZOOM_STEPS].reverse().find((step) => step < canvasZoom - 0.05) || MIN_CANVAS_ZOOM;
}

function safeCssColor(value: string | null): string {
  if (!value) return "transparent";
  const valid = /^(#[0-9a-f]{3,8}|rgba?\([\d\s.,%]+\)|hsla?\([\d\s.,%]+\))$/i.test(value.trim());
  return valid ? value : "transparent";
}

function colorLabel(value: string | null): string {
  if (!value) return "无颜色值";
  const match = value.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/i);
  if (!match) return value;
  const hex = match
    .slice(1, 4)
    .map((item) => Math.max(0, Math.min(255, Math.round(Number(item)))).toString(16).padStart(2, "0"))
    .join("")
    .toUpperCase();
  return `#${hex}`;
}

function layerTypeLabel(type: string): string {
  const labels: Record<string, string> = {
    artboard: "Frame",
    bitmapLayer: "位图",
    groupLayer: "分组",
    shapeLayer: "形状",
    symbolInstence: "组件实例",
    symbolInstance: "组件实例",
    textLayer: "文本",
  };
  return labels[type] || type;
}

function previewItems(capture: CaptureResult): PreviewItem[] {
  return capture.slices.flatMap((slice, index) => {
      const src = localImageSource(slice.localPath);
      return !src || slice.error
        ? []
        : [{
            key: `slice-${index}`,
            src,
            title: slice.name,
            detail: `尺寸 ${imageSize(slice.width, slice.height)} · 格式 ${slice.outputFormat.toUpperCase()} · 目录 ${sliceDensity(slice)}`,
          }];
    });
}

function showPreview(key: string) {
  if (!selectedCapture) return;
  const items = previewItems(selectedCapture);
  const index = items.findIndex((item) => item.key === key);
  if (index >= 0) showPhotoPreview({ items, index });
}

function setStatus(label: string, tone: "idle" | "working" | "success" | "error" = "idle") {
  appStatus.className = `app-status ${tone}`;
  const statusLabel = appStatus.querySelector<HTMLElement>("span:last-child")!;
  statusLabel.textContent = label;
  appStatus.title = label;
}

async function installCodexPlugin() {
  installPluginButton.disabled = true;
  installPluginButton.textContent = "正在安装…";
  setStatus("正在安装 Codex 插件", "working");
  clearError();

  try {
    await invoke<string>("install_codex_plugin");
    installPluginButton.textContent = "插件与 MCP 已安装";
    setStatus("安装完成，请完全退出并重新打开 Codex，然后新建任务", "success");
  } catch (error) {
    installPluginButton.textContent = "安装失败，重试";
    setStatus("插件安装失败", "error");
    showError(String(error));
  } finally {
    installPluginButton.disabled = false;
  }
}

async function openInstalledBrowserExtension() {
  if (!installedBrowserExtensionPath) return;
  try {
    await openPath(installedBrowserExtensionPath);
  } catch (error) {
    setStatus("无法自动打开扩展目录", "error");
    showError(`扩展已经安装，但无法打开目录：${String(error)}`);
  }
}

async function openBrowserExtensionManager() {
  openBrowserExtensionManagerButton.disabled = true;
  openBrowserExtensionManagerButton.textContent = "正在打开…";
  browserExtensionError.textContent = "";
  browserExtensionError.classList.add("hidden");
  try {
    await invoke("open_browser_extension_manager", { browser: browserExtensionBrowser.value });
    openBrowserExtensionManagerButton.textContent = "扩展管理页已打开";
    setStatus("请在浏览器中点击“加载已解压的扩展程序”", "working");
  } catch (error) {
    openBrowserExtensionManagerButton.textContent = "重新打开";
    browserExtensionError.textContent = String(error);
    browserExtensionError.classList.remove("hidden");
  } finally {
    openBrowserExtensionManagerButton.disabled = false;
  }
}

async function installBrowserExtension() {
  installBrowserExtensionButton.disabled = true;
  installBrowserExtensionButton.textContent = "正在准备…";
  setStatus("正在准备浏览器扩展", "working");
  clearError();

  try {
    const result = await invoke<BrowserExtensionInstallResult>("install_browser_extension");
    installedBrowserExtensionPath = result.extensionPath;
    browserExtensionPath.textContent = result.extensionPath;
    browserExtensionBrowsers.textContent = result.configuredBrowsers.length
      ? result.configuredBrowsers.join("、")
      : "Chrome / Edge / Chromium";
    browserExtensionError.textContent = "";
    browserExtensionError.classList.add("hidden");
    openBrowserExtensionManagerButton.textContent = "打开扩展管理页";
    browserExtensionDialog.showModal();
    browserExtensionDialog.focus({ preventScroll: true });
    installBrowserExtensionButton.textContent = "浏览器扩展已准备";
    setStatus("请在浏览器中确认加载扩展", "working");
  } catch (error) {
    installBrowserExtensionButton.textContent = "安装失败，重试";
    setStatus("浏览器扩展安装失败", "error");
    showError(String(error));
  } finally {
    installBrowserExtensionButton.disabled = false;
  }
}

function browserExtensionReady(status: BrowserExtensionStatus): boolean {
  return status.installed
    && status.enabled
    && status.autoCaptureReady
    && status.nativeHostInstalled
    && status.connected
    && status.versionCurrent;
}

function renderBrowserExtensionStatus(status: BrowserExtensionStatus | null) {
  const methodButton = captureMethodButtons.find((button) => button.dataset.captureMethod === "browser");
  const ready = status ? browserExtensionReady(status) : false;
  methodButton?.classList.toggle("is-available", ready);
  methodButton?.classList.toggle("is-unavailable", Boolean(status) && !ready);
  browserConnectionState.classList.toggle("is-checking", !status);
  browserConnectionState.classList.toggle("is-connected", ready);
  browserConnectionState.classList.toggle("is-disconnected", Boolean(status) && !ready);
  browserConnectionState.textContent = !status ? "检测中" : ready ? "已连接" : "未连接";

  if (!methodButton) return;
  methodButton.title = !status
    ? "正在检测浏览器扩展连接"
    : ready
      ? `${status.browser}：扩展 ${status.extensionVersion} 已连接当前客户端`
      : status.connected && !status.versionCurrent
        ? `${status.browser}：扩展版本 ${status.extensionVersion || "未知"}，需要更新并重新加载`
        : status.installed
          ? `${status.browser}：扩展未连接当前客户端`
          : `${status.browser}：未检测到 DesignBridge 扩展`;
}

function browserExtensionUnavailableMessage(status: BrowserExtensionStatus): string {
  if (status.connected && !status.versionCurrent) {
    return `${status.browser} 中仍是旧版 DesignBridge 扩展（${status.extensionVersion || "版本未知"}），请重新安装并在扩展管理页点击“重新加载”`;
  }
  if (!status.nativeHostInstalled) {
    return "浏览器通信程序尚未安装，请重新点击“安装浏览器扩展”";
  }
  if (status.installed && !status.enabled) {
    return `${status.browser} 中的 DesignBridge 扩展未启用，请先在扩展管理页启用`;
  }
  if (status.installed && status.enabled && !status.autoCaptureReady) {
    return `${status.browser} 中仍是旧版 DesignBridge 扩展，请在扩展管理页点击“重新加载”`;
  }
  if (status.installed) {
    return `${status.browser} 中的 DesignBridge 扩展未连接当前客户端，请确认浏览器正在运行并重新加载扩展`;
  }
  return `未检测到 ${status.browser} 中的 DesignBridge 扩展，请先点击“安装浏览器扩展”`;
}

async function refreshBrowserExtensionStatus(announce = false): Promise<BrowserExtensionStatus> {
  const status = await invoke<BrowserExtensionStatus>("browser_extension_status", {
    browser: browserExtensionBrowser.value,
  });
  const ready = browserExtensionReady(status);
  renderBrowserExtensionStatus(status);
  if (["浏览器扩展已准备", "安装失败，重试"].includes(installBrowserExtensionButton.textContent || "")) {
    installBrowserExtensionButton.textContent = "安装浏览器扩展";
  }

  if (announce) {
    const label = ready ? `${status.browser} 扩展已连接` : "浏览器扩展未连接";
    setStatus(label, ready ? "success" : "error");
  }
  return status;
}

function reportBrowserExtensionStatus() {
  renderBrowserExtensionStatus(null);
  void refreshBrowserExtensionStatus(true).catch((error) => {
    renderBrowserExtensionStatus({
      browser: browserExtensionBrowser.selectedOptions[0]?.textContent || "当前浏览器",
      installed: false,
      enabled: false,
      autoCaptureReady: false,
      nativeHostInstalled: false,
      connected: false,
      versionCurrent: false,
      extensionVersion: null,
    });
    setStatus("无法检测浏览器插件", "error");
    showError(String(error));
  });
}

function showError(message: string) {
  fieldError.textContent = message;
  fieldError.classList.remove("hidden");
}

function clearError() {
  fieldError.textContent = "";
  fieldError.classList.add("hidden");
}

function setProgress(progress: CaptureProgress) {
  const percent = Math.max(0, Math.min(100, progress.percent));
  progressPanel.classList.remove("hidden");
  progressMessage.textContent = progress.message;
  progressValue.textContent = `${percent}%`;
  progressBar.style.width = `${percent}%`;
}

function updateAttemptProgress(progress: CaptureProgress) {
  const attempt = captureAttempts.find(
    (item) => item.captureId === progress.captureId || item.key === activeCaptureKey,
  );
  if (!attempt) return null;
  if (!attempt.captureId && progress.captureId) attempt.captureId = progress.captureId;
  attempt.status = "capturing";
  attempt.message = progress.message;
  attempt.percent = Math.max(0, Math.min(100, progress.percent));
  persistCaptureAttempts();
  renderHistory();
  return attempt;
}

function markAttemptFailed(key: string, message: string) {
  const attempt = captureAttempts.find((item) => item.key === key);
  if (!attempt) return;
  attempt.status = "failed";
  attempt.message = message;
  persistCaptureAttempts();
  if (selectedHistoryKey === key) renderCaptureAttempt(attempt);
  else renderHistory();
}

function failCaptureAttempt(key: string, message: string) {
  const retainedCapture = history.find((capture) => captureSourceKey(capture.sourceUrl) === key);
  const wasReplacing = replacementCaptureIds.delete(key);
  if (!wasReplacing || !retainedCapture) {
    markAttemptFailed(key, message);
    return;
  }

  captureAttempts = captureAttempts.filter((attempt) => attempt.key !== key);
  persistCaptureAttempts();
  if (selectedHistoryKey === key) renderCapture(retainedCapture);
  else renderHistory();
}

function setCapturing(capturing: boolean) {
  captureButton.disabled = capturing;
  captureButton.textContent = capturing ? "抓取中…" : "开始抓取";
  cancelButton.classList.toggle("hidden", !capturing);
  urlInput.disabled = capturing;
  captureMethodButtons.forEach((button) => {
    button.disabled = capturing;
  });
  showMcpRefreshState(capturing && selectedCapture != null && activeCaptureKey === selectedHistoryKey);
  if (!capturing) {
    activeCaptureId = null;
    activeCaptureKey = null;
  }
}

function beginBrowserCapture(request: BrowserCaptureRequested) {
  clearError();
  setCaptureMethod("browser");
  const key = captureSourceKey(request.sourceUrl);
  const existingCapture = history.find((capture) => captureSourceKey(capture.sourceUrl) === key);
  if (existingCapture) replacementCaptureIds.set(key, existingCapture.captureId);

  let attempt = captureAttempts.find((item) => item.key === key);
  if (!attempt) {
    attempt = {
      key,
      sourceUrl: request.sourceUrl,
      captureId: request.captureId,
      status: "capturing",
      message: "已从浏览器接收设计稿，Rust 正在读取数据…",
      percent: 18,
      updatedAt: Date.now(),
    };
    captureAttempts.push(attempt);
  } else {
    attempt.sourceUrl = request.sourceUrl;
    attempt.captureId = request.captureId;
    attempt.status = "capturing";
    attempt.message = "已从浏览器接收设计稿，Rust 正在读取数据…";
    attempt.percent = 18;
    attempt.updatedAt = Date.now();
  }

  persistCaptureAttempts();
  activeCaptureId = request.captureId;
  activeCaptureKey = key;
  urlInput.value = request.sourceUrl;
  if (existingCapture) renderCapture(existingCapture);
  else renderCaptureAttempt(attempt);
  scrollHistoryEntryIntoView(key);
  setCapturing(true);
  setStatus("浏览器扩展抓取中", "working");
  setProgress({
    captureId: request.captureId,
    stage: "authorize",
    message: attempt.message,
    percent: attempt.percent,
  });
}

function dedupeHistory(captures: CaptureResult[]): CaptureResult[] {
  const sorted = [...captures].sort((a, b) => b.capturedAt - a.capturedAt);
  return sorted.filter(
    (capture, index) =>
      sorted.findIndex((candidate) => captureSourceKey(candidate.sourceUrl) === captureSourceKey(capture.sourceUrl)) === index,
  );
}

function historyEntries(): HistoryEntry[] {
  const captures = history.map((capture): HistoryEntry => ({
    key: captureSourceKey(capture.sourceUrl),
    updatedAt: capture.capturedAt * 1000,
    capture,
  }));
  const successfulKeys = new Set(captures.map((entry) => entry.key));
  const attempts = captureAttempts
    .filter((attempt) => !successfulKeys.has(attempt.key))
    .map((attempt): HistoryEntry => ({ key: attempt.key, updatedAt: attempt.updatedAt, attempt }));
  return [...captures, ...attempts].sort((a, b) => b.updatedAt - a.updatedAt);
}

function attemptName(attempt: CaptureAttempt): string {
  try {
    const url = new URL(attempt.sourceUrl);
    const hashQueryIndex = url.hash.indexOf("?");
    const params = new URLSearchParams(hashQueryIndex >= 0 ? url.hash.slice(hashQueryIndex + 1) : url.search);
    const imageId = params.get("image_id");
    return imageId ? `设计稿 ${imageId.slice(0, 8)}` : "蓝湖设计稿";
  } catch {
    return "蓝湖设计稿";
  }
}

function renderHistory() {
  const entries = historyEntries();
  historyCount.textContent = String(entries.length);
  if (entries.length === 0) {
    historyList.innerHTML = '<p class="history-empty">暂无记录</p>';
    return;
  }

  historyList.innerHTML = entries
    .map(
      (entry) => entry.capture ? `
        <div class="history-item${selectedHistoryKey === entry.key ? " active" : ""}" data-history-key="${escapeHtml(entry.key)}">
          <button class="history-select" type="button" data-select-history-key="${escapeHtml(entry.key)}">
            <span class="history-thumbnail-count">${entry.capture.downloadedCount}</span>
            <span class="history-copy">
              <strong>${escapeHtml(entry.capture.projectName)}</strong>
              <span>${slicesComplete(entry.capture) ? "抓取完成" : "切图处理中"} · ${displayDate(entry.capture.capturedAt)}</span>
            </span>
          </button>
          <button
            class="history-delete"
            type="button"
            data-delete-capture-id="${escapeHtml(entry.capture.captureId)}"
            aria-label="删除 ${escapeHtml(entry.capture.projectName)}"
            title="删除抓取记录"
          >×</button>
        </div>
      ` : `
        <div
          class="history-item is-${entry.attempt.status}${selectedHistoryKey === entry.key ? " active" : ""}"
          data-history-key="${escapeHtml(entry.key)}"
          title="${escapeHtml(entry.attempt.message)}"
        >
          <button class="history-select" type="button" data-select-history-key="${escapeHtml(entry.key)}">
            <span class="history-thumbnail-count">
              ${entry.attempt.status === "capturing" ? '<span class="history-spinner" aria-hidden="true"></span>' : "!"}
            </span>
            <span class="history-copy">
              <strong>${escapeHtml(attemptName(entry.attempt))}</strong>
              <span>${entry.attempt.status === "capturing" ? `${escapeHtml(entry.attempt.message)} · ${entry.attempt.percent}%` : "抓取失败 · 可重试"}</span>
            </span>
          </button>
          ${entry.attempt.status === "failed" ? `
            <button
              class="history-retry"
              type="button"
              data-retry-history-key="${escapeHtml(entry.key)}"
              aria-label="重试 ${escapeHtml(attemptName(entry.attempt))}"
              title="重新抓取"
            >重试</button>
          ` : ""}
        </div>
      `,
    )
    .join("");
}

function scrollHistoryEntryIntoView(key: string) {
  window.requestAnimationFrame(() => {
    const item = [...historyList.querySelectorAll<HTMLElement>("[data-history-key]")]
      .find((candidate) => candidate.dataset.historyKey === key);
    item?.scrollIntoView({ block: "nearest" });
  });
}

function renderResultMeta(capture: CaptureResult) {
  const totalSlices = sliceTotal(capture);
  const sliceStatus = totalSlices
    ? slicesComplete(capture)
      ? `${capture.sliceDownloadedCount} / ${totalSlices} 个切图`
      : `${totalSlices} 个切图后台处理中`
    : "未识别到切图";
  resultMeta.innerHTML = `
    <strong>${escapeHtml(capture.projectName)}</strong>
    <span>${captureLayers(capture).length} 个图层</span>
    <span>${sliceStatus}</span>
    ${capture.failedCount ? `<span class="error-text">${capture.failedCount} 个画板失败</span>` : ""}
    ${capture.sliceFailedCount ? `<span class="error-text">${capture.sliceFailedCount} 个切图失败</span>` : ""}
  `;
}

function renderEmptyCapture(state?: { title: string; detail: string }) {
  if (!state) setSelectedHistoryKey(null);
  selectedCapture = null;
  selectedLayerId = null;
  selectedCommentId = null;
  hitStack = null;
  resultMeta.innerHTML = "";
  designInspector.classList.remove("details-open");
  inspectorDetails.classList.add("hidden");
  designInspector.classList.add("hidden");
  emptyState.classList.remove("hidden");
  artboardImage.removeAttribute("src");
  sliceOutlines.innerHTML = "";
  commentMarkers.innerHTML = "";
  commentPopover.classList.add("hidden");
  commentPopover.replaceChildren();
  showSliceExportPanel(null);
  canvasPanX = 0;
  canvasPanY = 0;
  applyCanvasPan();
  updatePlatformPicker(null);
  showMcpLinkButton(null);
  emptyState.querySelector("strong")!.textContent = state?.title || "暂无抓取结果";
  emptyState.querySelector(":scope > span")!.textContent = state?.detail || "抓取完成的画板会显示在这里";
  renderHistory();
}

function renderCaptureAttempt(attempt: CaptureAttempt) {
  setSelectedHistoryKey(attempt.key);
  renderEmptyCapture({
    title: attempt.status === "capturing" ? "正在抓取设计稿" : "抓取失败",
    detail: attempt.status === "capturing" ? `${attempt.message}（${attempt.percent}%）` : attempt.message,
  });
}

function paintMarkup(paint: LayerPaint): string {
  return `
    <div class="style-color-row">
      <span class="color-swatch" style="--swatch-color:${safeCssColor(paint.color)}"></span>
      <span class="style-color-copy">
        <strong>${escapeHtml(paint.token || paint.paintType)}</strong>
        <small>${escapeHtml(colorLabel(paint.color))}${paint.opacity < 1 ? ` · ${Math.round(paint.opacity * 100)}%` : ""}</small>
      </span>
    </div>
  `;
}

function radiusSummary(radius: LayerRadius): string {
  const values = [radius.topLeft, radius.topRight, radius.bottomRight, radius.bottomLeft];
  if (values.every((value) => value === values[0])) return unitValue(values[0]);
  return values.map(numberValue).join(" / ") + (selectedPlatform === "ios" ? "pt" : "dp");
}

function detailRow(label: string, value: string): string {
  return `<div class="detail-row"><dt>${escapeHtml(label)}</dt><dd>${escapeHtml(value)}</dd></div>`;
}

function textAlignmentLabel(value: string): string {
  const labels: Record<string, string> = {
    left: "左对齐",
    center: "居中对齐",
    right: "右对齐",
    justify: "两端对齐",
    justified: "两端对齐",
  };
  return labels[value.toLowerCase()] || value;
}

function verticalAlignmentLabel(value: string): string {
  const labels: Record<string, string> = {
    top: "顶部对齐",
    center: "垂直居中对齐",
    middle: "垂直居中对齐",
    center_vertical: "垂直居中对齐",
    bottom: "底部对齐",
  };
  return labels[value.toLowerCase()] || value;
}

function fontWeightLabel(style: LayerTextStyle): string | null {
  if (style.fontStyle) return style.fontStyle;
  if (style.fontWeight == null) return null;
  const labels: Record<number, string> = {
    100: "Thin",
    200: "Extra Light",
    300: "Light",
    400: "Regular",
    500: "Medium",
    600: "Semi Bold",
    700: "Bold",
    800: "Extra Bold",
    900: "Black",
  };
  return labels[Math.round(style.fontWeight)] || numberValue(style.fontWeight);
}

function textMetricValue(value: number | null, unit: string | null, defaultUnit: string): string | null {
  if (unit?.toUpperCase() === "AUTO") return "normal";
  if (value == null) return null;
  const normalizedUnit = unit?.toLowerCase();
  if (normalizedUnit === "percent" || normalizedUnit === "%") return `${numberValue(value)}%`;
  if (normalizedUnit === "pixel" || normalizedUnit === "pixels" || normalizedUnit === "px") {
    return `${numberValue(value)}px`;
  }
  return `${numberValue(value)}${defaultUnit}`;
}

function colorOpacityLabel(value: string | null): string {
  if (!value) return "";
  const rgba = value.match(/^rgba\([^,]+,[^,]+,[^,]+,\s*([\d.]+)\s*\)$/i);
  if (rgba) return `${Math.round(Number(rgba[1]) * 100)}%`;
  return "100%";
}

function textColorRow(style: LayerTextStyle): string {
  if (!style.color) return "";
  return `
    <div class="detail-row">
      <dt>颜色</dt>
      <dd class="text-color-value">
        <span class="color-swatch text-color-swatch" style="--swatch-color:${safeCssColor(style.color)}"></span>
        <span class="text-color-code">
          <strong>${escapeHtml(colorLabel(style.color))}</strong>
          ${style.token ? `<small>${escapeHtml(style.token)}</small>` : ""}
        </span>
        <span>${escapeHtml(colorOpacityLabel(style.color))}</span>
      </dd>
    </div>
  `;
}

function normalizedTextStyles(text: LayerText): LayerTextStyle[] {
  if (text.styles?.length) return text.styles;
  return [{
    content: text.content,
    from: 0,
    to: text.content.length,
    fontFamily: text.fontFamily,
    postScriptName: null,
    fontStyle: null,
    fontSize: text.fontSize,
    fontWeight: text.fontWeight,
    alignment: text.alignment,
    verticalAlignment: null,
    lineHeight: text.lineHeight,
    lineHeightUnit: null,
    letterSpacing: text.letterSpacing,
    letterSpacingUnit: null,
    color: text.color,
    token: text.token,
  }];
}

function textStyleMarkup(style: LayerTextStyle, showTitle: boolean): string {
  const font = style.postScriptName || style.fontFamily;
  const weight = fontWeightLabel(style);
  const fontSize = textMetricValue(style.fontSize, null, selectedPlatform === "ios" ? "pt" : "sp");
  const letterSpacing = textMetricValue(style.letterSpacing, style.letterSpacingUnit, "px");
  const lineHeight = textMetricValue(
    style.lineHeight,
    style.lineHeightUnit,
    selectedPlatform === "ios" ? "pt" : "sp",
  );
  return `
    <section class="text-style-group">
      ${showTitle ? `<h5>“ ${escapeHtml(style.content)} ”</h5>` : ""}
      <dl class="detail-table compact">
        ${font ? detailRow("字体", font) : ""}
        ${weight ? detailRow("字重", weight) : ""}
        ${style.alignment ? detailRow("对齐", textAlignmentLabel(style.alignment)) : ""}
        ${style.verticalAlignment ? detailRow("垂直对齐", verticalAlignmentLabel(style.verticalAlignment)) : ""}
        ${textColorRow(style)}
        ${fontSize ? detailRow("字号", fontSize) : ""}
        ${letterSpacing ? detailRow("字间距", letterSpacing) : ""}
        ${lineHeight ? detailRow("行高", lineHeight) : ""}
        ${detailRow("内容", style.content)}
      </dl>
    </section>
  `;
}

function commentShortDate(value: string | null): string {
  if (!value) return "时间未知";
  const numericValue = Number(value);
  const date = Number.isFinite(numericValue)
    ? new Date(numericValue < 1_000_000_000_000 ? numericValue * 1000 : numericValue)
    : new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", { month: "long", day: "numeric" }).format(date);
}

function commentAuthorBadge(author: string): string {
  const characters = Array.from(author.trim());
  return characters.slice(Math.max(0, characters.length - 2)).join("") || "评论";
}

function commentPoint(comment: DesignComment, coordinate: PlatformFrame): { x: number; y: number } | null {
  if (comment.x == null || comment.y == null) return null;
  const sourceWidth = comment.sourceWidth || coordinate.width;
  const sourceHeight = comment.sourceHeight || coordinate.height;
  if (sourceWidth <= 0 || sourceHeight <= 0) return null;
  return {
    x: (comment.x / sourceWidth) * coordinate.width,
    y: (comment.y / sourceHeight) * coordinate.height,
  };
}

function renderCommentMarkers(capture: CaptureResult | null) {
  commentMarkers.replaceChildren();
  const design = currentDesign(capture);
  const coordinate = androidFrame(design || undefined);
  if (!design || !coordinate) return;

  const fragment = document.createDocumentFragment();
  for (const comment of designComments(design)) {
    const point = commentPoint(comment, coordinate);
    if (!point) continue;
    const left = (point.x / coordinate.width) * 100;
    const top = (point.y / coordinate.height) * 100;
    if (left < 0 || left > 100 || top < 0 || top > 100) continue;

    const marker = document.createElement("button");
    marker.type = "button";
    marker.className = "comment-marker";
    marker.classList.toggle("selected", comment.id === selectedCommentId);
    marker.dataset.commentId = comment.id;
    marker.style.left = `${left}%`;
    marker.style.top = `${top}%`;
    marker.textContent = String(comment.index);
    marker.title = `${comment.author}: ${comment.content}`;
    marker.setAttribute("aria-label", `评论 ${comment.index}：${comment.content}`);
    fragment.append(marker);
  }
  commentMarkers.append(fragment);
}

function positionCommentPopover() {
  if (commentPopover.classList.contains("hidden") || !selectedCommentId) return;
  const marker = commentMarkers.querySelector<HTMLElement>(
    `[data-comment-id="${CSS.escape(selectedCommentId)}"]`,
  );
  if (!marker) return;
  const containerBounds = artboardScroll.getBoundingClientRect();
  const markerBounds = marker.getBoundingClientRect();
  const popoverBounds = commentPopover.getBoundingClientRect();
  const gap = 16;
  const edge = 12;
  let side: "left" | "right" = "right";
  let left = markerBounds.right - containerBounds.left + gap;
  if (left + popoverBounds.width > containerBounds.width - edge) {
    side = "left";
    left = markerBounds.left - containerBounds.left - popoverBounds.width - gap;
  }
  left = Math.max(edge, Math.min(left, containerBounds.width - popoverBounds.width - edge));
  const preferredTop = markerBounds.top - containerBounds.top - 64;
  const top = Math.max(edge, Math.min(preferredTop, containerBounds.height - popoverBounds.height - edge));
  commentPopover.dataset.side = side;
  commentPopover.style.left = `${left}px`;
  commentPopover.style.top = `${top}px`;
}

function renderCommentPopover(comment: DesignComment) {
  const replies = comment.replies || [];
  commentPopover.innerHTML = `
    <header class="comment-popover-header">
      <div>
        <strong>评论</strong>
        <span>${escapeHtml(comment.versionName || "当前版本")}</span>
      </div>
    </header>
    <div class="comment-thread">
      <span class="comment-avatar">${escapeHtml(commentAuthorBadge(comment.author))}</span>
      <div class="comment-thread-content">
        <div class="comment-byline">
          <strong>${escapeHtml(comment.author)}</strong>
          <span>${escapeHtml(commentShortDate(comment.createdAt))}</span>
        </div>
        <p>${escapeHtml(comment.content)}</p>
        ${replies.map((reply) => `
          <div class="comment-reply">
            <div class="comment-byline">
              <strong>${escapeHtml(reply.author)}</strong>
              <span>${escapeHtml(commentShortDate(reply.createdAt))}</span>
            </div>
            <p>${escapeHtml(reply.content)}</p>
          </div>
        `).join("")}
      </div>
    </div>
  `;
  commentPopover.classList.remove("hidden");
  window.requestAnimationFrame(positionCommentPopover);
}

function renderSliceOutlines(capture: CaptureResult | null) {
  sliceOutlines.replaceChildren();
  if (!capture) return;
  const coordinate = androidFrame(capture.designs.find((design) => design.error == null));
  if (!coordinate) return;

  const fragment = document.createDocumentFragment();
  for (const layer of captureLayers(capture)) {
    const frame = layer.frame;
    if (!layer.visible || !layer.hasSlice || !frame || frame.width <= 0 || frame.height <= 0) continue;
    const left = Math.max(0, frame.x);
    const top = Math.max(0, frame.y);
    const right = Math.min(coordinate.width, frame.x + frame.width);
    const bottom = Math.min(coordinate.height, frame.y + frame.height);
    if (right <= left || bottom <= top) continue;

    const outline = document.createElement("span");
    outline.className = "slice-outline";
    outline.style.left = `${(left / coordinate.width) * 100}%`;
    outline.style.top = `${(top / coordinate.height) * 100}%`;
    outline.style.width = `${((right - left) / coordinate.width) * 100}%`;
    outline.style.height = `${((bottom - top) / coordinate.height) * 100}%`;
    fragment.append(outline);
  }
  sliceOutlines.append(fragment);
}

function renderLayerSelection() {
  const capture = selectedCapture;
  if (!capture) {
    showSliceExportPanel(null);
    return;
  }
  const layers = captureLayers(capture);
  const layer = layers.find((item) => item.id === selectedLayerId) || null;
  const design = currentDesign(capture);
  const comment = designComments(design).find((item) => item.id === selectedCommentId) || null;
  showMcpLinkButton(design ? designMcpTarget(capture, design, layer) : null);

  layerHighlight.classList.add("hidden");
  if (comment) {
    designInspector.classList.remove("details-open");
    inspectorDetails.classList.add("hidden");
    layerDetails.innerHTML = "";
    showSliceExportPanel(null);
    renderCommentMarkers(capture);
    renderCommentPopover(comment);
    return;
  }
  commentPopover.classList.add("hidden");
  commentPopover.replaceChildren();
  if (!layer) {
    designInspector.classList.remove("details-open");
    inspectorDetails.classList.add("hidden");
    layerDetails.innerHTML = "";
    showSliceExportPanel(null);
    scheduleInspectorFit();
    renderCommentMarkers(capture);
    return;
  }

  designInspector.classList.add("details-open");
  inspectorDetails.classList.remove("hidden");
  scheduleInspectorFit();

  const coordinate = androidFrame(capture.designs.find((design) => design.error == null));
  if (layer.frame && coordinate && layer.frame.width > 0 && layer.frame.height > 0) {
    const left = Math.max(0, layer.frame.x);
    const top = Math.max(0, layer.frame.y);
    const right = Math.min(coordinate.width, layer.frame.x + layer.frame.width);
    const bottom = Math.min(coordinate.height, layer.frame.y + layer.frame.height);
    if (right > left && bottom > top) {
      layerHighlight.style.left = `${(left / coordinate.width) * 100}%`;
      layerHighlight.style.top = `${(top / coordinate.height) * 100}%`;
      layerHighlight.style.width = `${((right - left) / coordinate.width) * 100}%`;
      layerHighlight.style.height = `${((bottom - top) / coordinate.height) * 100}%`;
      const unit = selectedPlatform === "ios" ? "pt" : "dp";
      layerHighlightSize.textContent = `${numberValue(layer.frame.width)} × ${numberValue(layer.frame.height)}${unit}`;
      layerHighlight.classList.remove("hidden");
    }
  }

  const parent = layers.find((item) => item.id === layer.parentId);
  const hitPosition = hitStack?.captureId === capture.captureId
    ? hitStack.layerIds.indexOf(layer.id)
    : -1;
  const frameRows = layer.frame
    ? `
      <div class="detail-row">
        <dt>位置</dt>
        <dd class="detail-value-pair"><span>${unitValue(layer.frame.x)}</span><span>${unitValue(layer.frame.y)}</span></dd>
      </div>
      <div class="detail-row">
        <dt>大小</dt>
        <dd class="detail-value-pair"><span>${unitValue(layer.frame.width)}</span><span>${unitValue(layer.frame.height)}</span></dd>
      </div>
    `
    : detailRow("Frame", "无有效坐标");
  const fills = layer.fills.length
    ? `<div class="style-list">${layer.fills.map(paintMarkup).join("")}</div>`
    : "";
  const borders = layer.borders.length
    ? `<div class="style-subsection"><h4>边框</h4>${layer.borders
        .map(
          (border) => `
            <div class="style-color-row">
              <span class="color-swatch" style="--swatch-color:${safeCssColor(border.color)}"></span>
              <span class="style-color-copy">
                <strong>${escapeHtml(border.token || colorLabel(border.color))}</strong>
                <small>${unitValue(border.width)} · ${escapeHtml(border.style)}</small>
              </span>
            </div>
          `,
        )
        .join("")}</div>`
    : "";
  const effects = [
    ...layer.shadows.map(
      (shadow) => `${shadow.shadowType} ${numberValue(shadow.offsetX)}, ${numberValue(shadow.offsetY)}, ${numberValue(shadow.blur)}dp`,
    ),
    ...layer.blurs.map((blur) => `${blur.blurType} ${numberValue(blur.radius)}dp`),
  ];
  const textStyles = layer.text ? normalizedTextStyles(layer.text) : [];
  const text = layer.text
    ? `
      <div class="style-subsection">
        <h4>文本</h4>
        <p class="text-content">${escapeHtml(layer.text.content)}</p>
        ${textStyles.map((style) => textStyleMarkup(style, textStyles.length > 1)).join("")}
      </div>
    `
    : "";

  layerDetails.innerHTML = `
    <section class="detail-section">
      <div class="detail-heading">
        <h3>组件</h3>
        ${hitPosition >= 0 && hitStack && hitStack.layerIds.length > 1 ? `<span>${hitPosition + 1} / ${hitStack.layerIds.length} 层</span>` : ""}
      </div>
      <div class="component-name" title="${escapeHtml(layer.name)}">${escapeHtml(layer.name)}</div>
    </section>
    <section class="detail-section">
      <h3>样式信息</h3>
      <dl class="detail-table">
        ${detailRow("图层", layer.name)}
        ${detailRow("节点 ID", layer.id)}
        ${detailRow("类型", layerTypeLabel(layer.layerType))}
        ${parent ? detailRow("父级", parent.name) : ""}
        ${frameRows}
        ${detailRow("不透明度", `${Math.round(layer.opacity * 100)}%`)}
        ${layer.rotation !== 0 ? detailRow("旋转", `${numberValue(layer.rotation)}°`) : ""}
        ${detailRow("圆角", radiusSummary(layer.radius))}
      </dl>
      ${fills}
      ${borders}
      ${effects.length ? `<div class="style-subsection"><h4>效果</h4><p class="effect-copy">${effects.map(escapeHtml).join("<br>")}</p></div>` : ""}
      ${text}
    </section>
  `;

  const slice = slicesForLayer(capture, layer)[0] || null;
  const sliceIndex = slice ? capture.slices.indexOf(slice) : -1;
  showSliceExportPanel({
    captureId: capture.captureId,
    outputDir: capture.outputDir,
    layerId: layer.id,
    layerName: layer.name,
    layerWidth: layer.frame?.width ?? null,
    layerHeight: layer.frame?.height ?? null,
    hasSlice: layer.hasSlice,
    pending: layer.hasSlice && !slicesComplete(capture),
    platform: selectedPlatform,
    slice: slice
      ? {
          id: slice.id,
          name: slice.name,
          src: localImageSource(slice.localPath),
          width: slice.width,
          height: slice.height,
          error: slice.error,
        }
      : null,
    onPlatformChange: changeTargetPlatform,
    onPreview: () => {
      if (sliceIndex >= 0) showPreview(`slice-${sliceIndex}`);
    },
  });
  renderCommentMarkers(capture);
}

function renderCapture(capture: CaptureResult) {
  setSelectedHistoryKey(captureSourceKey(capture.sourceUrl));
  selectedCapture = capture;
  selectedLayerId = null;
  selectedCommentId = null;
  hitStack = null;
  showSliceExportPanel(null);
  designInspector.classList.remove("details-open");
  inspectorDetails.classList.add("hidden");
  commentPopover.classList.add("hidden");
  commentPopover.replaceChildren();
  renderHistory();
  renderResultMeta(capture);
  emptyState.classList.add("hidden");
  designInspector.classList.remove("hidden");
  scheduleInspectorFit();
  updatePlatformPicker(capture);

  const designIndex = capture.designs.findIndex((item) => item.error == null && item.localPath != null);
  const design = designIndex >= 0 ? capture.designs[designIndex] : null;
  const source = design ? localImageSource(design.localPath) : null;
  if (!design || !source) {
    showMcpLinkButton(null);
    canvasTitle.textContent = capture.projectName;
    artboardImage.removeAttribute("src");
    artboardImage.alt = "";
    renderSliceOutlines(null);
    renderCommentMarkers(null);
    layerDetails.innerHTML = "";
    return;
  }

  const coordinate = androidFrame(design);
  showMcpLinkButton(designMcpTarget(capture, design));
  canvasTitle.textContent = design.name;
  artboardImage.alt = design.name;
  canvasZoom = 100;
  canvasPanX = 0;
  canvasPanY = 0;
  applyCanvasPan();
  if (coordinate) applyCanvasZoom();
  artboardImage.src = source;
  renderSliceOutlines(capture);
  renderCommentMarkers(capture);
  renderLayerSelection();
}

function updateCapturedSlices(capture: CaptureResult) {
  const key = captureSourceKey(capture.sourceUrl);
  history = dedupeHistory([capture, ...history.filter((item) => captureSourceKey(item.sourceUrl) !== key)]);
  if (selectedHistoryKey === key) {
    selectedCapture = capture;
    renderResultMeta(capture);
    updatePlatformPicker(capture);
    renderSliceOutlines(capture);
    renderCommentMarkers(capture);
    renderLayerSelection();
  }
  renderHistory();
}

function layersAtPoint(capture: CaptureResult, x: number, y: number): InspectableLayer[] {
  return captureLayers(capture)
    .filter((layer) => {
      const frame = layer.frame;
      return Boolean(
        layer.visible &&
          frame &&
          frame.width > 0 &&
          frame.height > 0 &&
          x >= frame.x &&
          x <= frame.x + frame.width &&
          y >= frame.y &&
          y <= frame.y + frame.height,
      );
    })
    .sort((a, b) => {
      if (a.hasSlice !== b.hasSlice) return a.hasSlice ? -1 : 1;
      const areaA = (a.frame?.width || 0) * (a.frame?.height || 0);
      const areaB = (b.frame?.width || 0) * (b.frame?.height || 0);
      if (Math.abs(areaA - areaB) > 0.01) return areaA - areaB;
      if (a.depth !== b.depth) return b.depth - a.depth;
      return b.order - a.order;
    });
}

function hasReliableRectHitArea(layer: InspectableLayer, coordinate: PlatformFrame | null): boolean {
  const frame = layer.frame;
  if (!frame) return false;
  if (layer.hasSlice || layer.isAsset || layer.text) return true;

  const extendsOutsideArtboard =
    coordinate &&
    (frame.x < -0.5 ||
      frame.y < -0.5 ||
      frame.x + frame.width > coordinate.width + 0.5 ||
      frame.y + frame.height > coordinate.height + 0.5);
  if (layer.layerType === "shapeLayer" && extendsOutsideArtboard) {
    return false;
  }

  const largestBorder = layer.borders.reduce((width, border) => Math.max(width, border.width), 0);
  const isSparseStrokeBounds =
    layer.layerType === "shapeLayer" &&
    layer.fills.length === 0 &&
    largestBorder > 0 &&
    frame.width >= 32 &&
    frame.height >= 32 &&
    frame.width * frame.height >= 4096 &&
    frame.width > largestBorder * 12 &&
    frame.height > largestBorder * 12;

  return !isSparseStrokeBounds;
}

function selectableLayersAtPoint(capture: CaptureResult, x: number, y: number): InspectableLayer[] {
  const coordinate = currentCoordinate();
  const layers = layersAtPoint(capture, x, y).filter((layer) => hasReliableRectHitArea(layer, coordinate));
  const artboardArea = coordinate ? coordinate.width * coordinate.height : 0;
  const hasMeaningfulLayer = layers.some((layer) => {
    const frame = layer.frame;
    const area = frame ? frame.width * frame.height : 0;
    const coversArtboard = artboardArea > 0 && area / artboardArea >= 0.85;
    return layer.depth > 0 && (!coversArtboard || layer.hasSlice || Boolean(layer.text));
  });
  return hasMeaningfulLayer ? layers : [];
}

function designPointAt(clientX: number, clientY: number) {
  const capture = selectedCapture;
  if (!capture) return null;
  const coordinate = currentCoordinate();
  if (!coordinate) return null;
  const bounds = artboardImage.getBoundingClientRect();
  if (
    bounds.width <= 0 ||
    bounds.height <= 0 ||
    clientX < bounds.left ||
    clientX > bounds.right ||
    clientY < bounds.top ||
    clientY > bounds.bottom
  ) {
    return null;
  }
  return {
    capture,
    coordinate,
    bounds,
    x: ((clientX - bounds.left) / bounds.width) * coordinate.width,
    y: ((clientY - bounds.top) / bounds.height) * coordinate.height,
  };
}

function clearLayerSelection() {
  if (!selectedLayerId && !selectedCommentId && !hitStack) return;
  selectedLayerId = null;
  selectedCommentId = null;
  hitStack = null;
  renderLayerSelection();
}

function selectLayerAt(clientX: number, clientY: number) {
  const point = designPointAt(clientX, clientY);
  if (!point) {
    clearLayerSelection();
    return;
  }

  const layerIds = selectableLayersAtPoint(point.capture, point.x, point.y).map((layer) => layer.id);
  const toleranceX = (point.coordinate.width / point.bounds.width) * 5;
  const toleranceY = (point.coordinate.height / point.bounds.height) * 5;
  const samePoint =
    hitStack?.captureId === point.capture.captureId &&
    Math.abs(hitStack.x - point.x) <= toleranceX &&
    Math.abs(hitStack.y - point.y) <= toleranceY;
  const currentIndex = samePoint && selectedLayerId ? layerIds.indexOf(selectedLayerId) : -1;
  const index = layerIds.length > 0 ? (currentIndex + 1) % layerIds.length : -1;
  hitStack = layerIds.length
    ? { captureId: point.capture.captureId, x: point.x, y: point.y, layerIds, index }
    : null;
  selectedLayerId = index >= 0 ? layerIds[index] : null;
  selectedCommentId = null;
  renderLayerSelection();
}

function updateLayerCursor(clientX: number, clientY: number) {
  const point = designPointAt(clientX, clientY);
  const hasLayer = point
    ? selectableLayersAtPoint(point.capture, point.x, point.y).length > 0
    : false;
  artboardImage.classList.toggle("has-layer", hasLayer);
}

function closeDeleteDialog() {
  pendingDeleteCaptureId = null;
  deleteConfirmButton.disabled = false;
  deleteCancelButton.disabled = false;
  deleteConfirmButton.textContent = "删除";
  deleteConfirmError.textContent = "";
  deleteConfirmError.classList.add("hidden");
  if (deleteConfirmDialog.open) deleteConfirmDialog.close();
}

function requestCaptureDeletion(capture: CaptureResult) {
  pendingDeleteCaptureId = capture.captureId;
  deleteConfirmMessage.textContent = `确定删除“${capture.projectName}”及其本地图片吗？`;
  deleteConfirmError.textContent = "";
  deleteConfirmError.classList.add("hidden");
  deleteConfirmDialog.showModal();
  deleteConfirmDialog.focus({ preventScroll: true });
}

async function confirmCaptureDeletion() {
  const capture = history.find((item) => item.captureId === pendingDeleteCaptureId);
  if (!capture) {
    closeDeleteDialog();
    return;
  }

  deleteConfirmButton.disabled = true;
  deleteCancelButton.disabled = true;
  deleteConfirmButton.textContent = "删除中…";
  try {
    await invoke("delete_saved_capture", { captureId: capture.captureId });
    const deletedSelected = selectedCapture?.captureId === capture.captureId;
    history = history.filter((item) => item.captureId !== capture.captureId);
    closeDeleteDialog();
    if (deletedSelected) {
      const next = historyEntries()[0];
      if (next?.capture) renderCapture(next.capture);
      else if (next?.attempt) renderCaptureAttempt(next.attempt);
      else renderEmptyCapture();
    } else {
      renderHistory();
    }
    setStatus("记录已删除", "success");
  } catch (error) {
    deleteConfirmButton.disabled = false;
    deleteConfirmButton.textContent = "重新删除";
    deleteConfirmError.textContent = `删除失败：${String(error)}`;
    deleteConfirmError.classList.remove("hidden");
  } finally {
    deleteCancelButton.disabled = false;
  }
}

type StartCaptureOptions = {
  force?: boolean;
};

function refreshSelectedDesign() {
  if (!selectedCapture) return;
  void startCapture(selectedCapture.sourceUrl, { force: true });
}

async function startCapture(sourceUrl = urlInput.value.trim(), options: StartCaptureOptions = {}) {
  clearError();
  let parsed: URL;
  try {
    parsed = new URL(sourceUrl);
  } catch {
    showError("请输入完整的蓝湖链接");
    return;
  }

  if (parsed.protocol !== "https:" || !/(^|\.)lanhu(app)?\.com$/i.test(parsed.hostname)) {
    showError("当前仅支持 lanhuapp.com 的 HTTPS 链接");
    return;
  }

  const normalizedUrl = parsed.toString();
  const key = captureSourceKey(normalizedUrl);
  if (activeCaptureKey && activeCaptureKey !== key) {
    renderHistory();
    scrollHistoryEntryIntoView(activeCaptureKey);
    setStatus("已有设计稿正在抓取", "working");
    return;
  }

  const existingCapture = history.find((capture) => captureSourceKey(capture.sourceUrl) === key);
  if (!options.force && existingCapture && (existingCapture.dataVersion ?? 0) >= CURRENT_CAPTURE_VERSION) {
    if (selectedCapture?.captureId !== existingCapture.captureId) renderCapture(existingCapture);
    else {
      setSelectedHistoryKey(key);
      renderHistory();
    }
    scrollHistoryEntryIntoView(key);
    setStatus("已定位到抓取记录", "success");
    progressPanel.classList.add("hidden");
    return;
  }
  if (existingCapture) replacementCaptureIds.set(key, existingCapture.captureId);

  if (captureMethod === "browser") {
    try {
      const extensionStatus = await refreshBrowserExtensionStatus();
      if (!browserExtensionReady(extensionStatus)) {
        replacementCaptureIds.delete(key);
        setStatus("浏览器扩展未连接", "error");
        showError(browserExtensionUnavailableMessage(extensionStatus));
        return;
      }
    } catch (error) {
      replacementCaptureIds.delete(key);
      setStatus("无法检测浏览器扩展", "error");
      showError(String(error));
      return;
    }
  }

  const existingAttempt = captureAttempts.find((attempt) => attempt.key === key);
  if (existingAttempt?.status === "capturing") {
    renderCaptureAttempt(existingAttempt);
    scrollHistoryEntryIntoView(key);
    setStatus("设计稿正在抓取", "working");
    return;
  }

  const attempt: CaptureAttempt = existingAttempt || {
    key,
    sourceUrl: normalizedUrl,
    captureId: null,
    status: "capturing",
    message: captureMethod === "browser" ? "正在打开系统浏览器…" : "正在打开蓝湖窗口…",
    percent: 4,
    updatedAt: Date.now(),
  };
  attempt.sourceUrl = normalizedUrl;
  attempt.captureId = null;
  attempt.status = "capturing";
  attempt.message = options.force
    ? captureMethod === "browser" ? "正在通过浏览器扩展刷新设计稿…" : "正在强制刷新设计稿…"
    : existingCapture
      ? "正在更新旧版图层数据…"
      : captureMethod === "browser" ? "正在打开系统浏览器…" : "正在打开蓝湖窗口…";
  attempt.percent = 4;
  attempt.updatedAt = Date.now();
  if (!existingAttempt) captureAttempts.push(attempt);
  persistCaptureAttempts();
  activeCaptureKey = key;
  if (existingCapture) renderCapture(existingCapture);
  else renderCaptureAttempt(attempt);
  scrollHistoryEntryIntoView(key);

  setCapturing(true);
  setStatus(
    options.force
      ? "正在刷新设计稿"
      : existingCapture
        ? "正在更新图层数据"
        : captureMethod === "browser" ? "等待浏览器扩展" : "等待蓝湖授权",
    "working",
  );
  setProgress({ captureId: "", stage: "authorize", message: attempt.message, percent: 4 });

  try {
    const captureId = captureMethod === "browser"
      ? await invoke<string>("start_browser_extension_capture", {
          url: normalizedUrl,
          browser: browserExtensionBrowser.value,
        })
      : await invoke<string>("start_lanhu_capture", { url: normalizedUrl });
    if (attempt.status !== "capturing" || activeCaptureKey !== key) return;
    activeCaptureId = captureId;
    attempt.captureId = captureId;
    persistCaptureAttempts();
  } catch (error) {
    failCaptureAttempt(key, String(error));
    setCapturing(false);
    setStatus("抓取失败", "error");
    showError(String(error));
    progressPanel.classList.add("hidden");
  }
}

captureForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void startCapture();
});

installPluginButton.addEventListener("click", () => void installCodexPlugin());
installBrowserExtensionButton.addEventListener("click", () => void installBrowserExtension());
captureMethodButtons.forEach((button) => {
  button.addEventListener("click", () => {
    const method = button.dataset.captureMethod === "webview" ? "webview" : "browser";
    setCaptureMethod(method);
    clearError();
    if (method === "browser") reportBrowserExtensionStatus();
    else setStatus("将使用内置窗口抓取", "idle");
  });
});
browserExtensionBrowser.addEventListener("change", () => {
  storeValue(BROWSER_STORAGE_KEY, browserExtensionBrowser.value);
  if (captureMethod === "browser") reportBrowserExtensionStatus();
});
openBrowserExtensionManagerButton.addEventListener("click", () => void openBrowserExtensionManager());
openBrowserExtensionFolderButton.addEventListener("click", () => void openInstalledBrowserExtension());
browserExtensionDoneButton.addEventListener("click", () => {
  browserExtensionDialog.close();
  reportBrowserExtensionStatus();
});
browserExtensionDialog.addEventListener("cancel", (event) => {
  event.preventDefault();
  browserExtensionDialog.close();
});

cancelButton.addEventListener("click", async () => {
  if (!activeCaptureId || !activeCaptureKey) return;
  const captureId = activeCaptureId;
  const captureKey = activeCaptureKey;
  try {
    await invoke("cancel_lanhu_capture", { captureId });
  } finally {
    failCaptureAttempt(captureKey, "抓取已取消，可重试");
    setCapturing(false);
    progressPanel.classList.add("hidden");
    setStatus("已取消");
  }
});

historyList.addEventListener("click", (event) => {
  const deleteTarget = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-delete-capture-id]");
  if (deleteTarget) {
    const capture = history.find((item) => item.captureId === deleteTarget.dataset.deleteCaptureId);
    if (capture) requestCaptureDeletion(capture);
    return;
  }

  const retryTarget = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-retry-history-key]");
  if (retryTarget) {
    const attempt = captureAttempts.find((item) => item.key === retryTarget.dataset.retryHistoryKey);
    if (attempt) {
      urlInput.value = attempt.sourceUrl;
      void startCapture(attempt.sourceUrl);
    }
    return;
  }

  const target = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-select-history-key]");
  if (!target) return;
  const key = target.dataset.selectHistoryKey;
  const capture = history.find((item) => captureSourceKey(item.sourceUrl) === key);
  const attempt = captureAttempts.find((item) => item.key === key);
  if (capture) renderCapture(capture);
  else if (attempt) renderCaptureAttempt(attempt);
});

historyList.addEventListener(
  "scroll",
  () => storeValue(HISTORY_SCROLL_TOP_STORAGE_KEY, String(historyList.scrollTop)),
  { passive: true },
);

sidebarResizeHandle.addEventListener("pointerdown", (event) => {
  if (sidebarCollapsed || !event.isPrimary || event.button !== 0) return;
  event.preventDefault();
  sidebarResizeState = {
    pointerId: event.pointerId,
    startX: event.clientX,
    width: sidebarWidth,
  };
  sidebarResizeHandle.setPointerCapture(event.pointerId);
  document.body.classList.add("is-resizing-sidebar");
});

window.addEventListener("pointermove", (event) => {
  if (!sidebarResizeState || sidebarResizeState.pointerId !== event.pointerId) return;
  applySidebarWidth(sidebarResizeState.width + event.clientX - sidebarResizeState.startX);
});

function finishSidebarResize(event: PointerEvent) {
  if (!sidebarResizeState || sidebarResizeState.pointerId !== event.pointerId) return;
  sidebarResizeState = null;
  if (sidebarResizeHandle.hasPointerCapture(event.pointerId)) {
    sidebarResizeHandle.releasePointerCapture(event.pointerId);
  }
  document.body.classList.remove("is-resizing-sidebar");
  applySidebarWidth(sidebarWidth, true);
}

window.addEventListener("pointerup", finishSidebarResize);
window.addEventListener("pointercancel", finishSidebarResize);
sidebarResizeHandle.addEventListener("lostpointercapture", finishSidebarResize);
sidebarResizeHandle.addEventListener("keydown", (event) => {
  let nextWidth = sidebarWidth;
  if (event.key === "ArrowLeft") nextWidth -= 16;
  else if (event.key === "ArrowRight") nextWidth += 16;
  else if (event.key === "Home") nextWidth = MIN_SIDEBAR_WIDTH;
  else if (event.key === "End") nextWidth = MAX_SIDEBAR_WIDTH;
  else return;
  event.preventDefault();
  applySidebarWidth(nextWidth, true);
});

deleteCancelButton.addEventListener("click", closeDeleteDialog);
deleteConfirmButton.addEventListener("click", () => void confirmCaptureDeletion());
deleteConfirmDialog.addEventListener("cancel", (event) => {
  event.preventDefault();
  if (!deleteConfirmButton.disabled) closeDeleteDialog();
});

zoomOutButton.addEventListener("click", () => setCanvasZoom(steppedZoom(-1)));
zoomInButton.addEventListener("click", () => setCanvasZoom(steppedZoom(1)));
zoomInput.addEventListener("change", () => setCanvasZoom(Number(zoomInput.value)));
zoomInput.addEventListener("blur", () => {
  zoomInput.value = numberValue(canvasZoom);
});

artboardScroll.addEventListener(
  "wheel",
  (event) => {
    event.preventDefault();
    if (event.ctrlKey) {
      const factor = Math.exp(-event.deltaY * 0.006);
      setCanvasZoom(canvasZoom * factor, { x: event.clientX, y: event.clientY });
      return;
    }
    canvasPanX -= event.deltaX;
    canvasPanY -= event.deltaY;
    applyCanvasPan();
  },
  { passive: false },
);

layerDetails.addEventListener("click", (event) => {
  const target = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-preview-slice-index]");
  if (!target) return;
  const index = Number(target.dataset.previewSliceIndex);
  if (Number.isInteger(index)) showPreview(`slice-${index}`);
});

commentMarkers.addEventListener("pointerdown", (event) => {
  event.stopPropagation();
});

commentMarkers.addEventListener("click", (event) => {
  const marker = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-comment-id]");
  if (!marker || !selectedCapture) return;
  const design = currentDesign(selectedCapture);
  const comment = designComments(design).find((item) => item.id === marker.dataset.commentId);
  if (!comment) return;
  selectedLayerId = null;
  selectedCommentId = comment.id;
  hitStack = null;
  renderLayerSelection();
});

artboardImage.addEventListener("load", () => {
  if (!selectedCapture) return;
  const design = selectedCapture.designs.find((item) => item.localPath && item.error == null);
  if (!design) return;
  if (design.width == null || design.height == null) {
    design.width = artboardImage.naturalWidth;
    design.height = artboardImage.naturalHeight;
  }
  if (!design.coordinateSpace) {
    design.coordinateSpace = {
      platform: "android",
      width: artboardImage.naturalWidth / 2,
      height: artboardImage.naturalHeight / 2,
      unit: "dp",
    };
    applyCanvasZoom();
    updatePlatformPicker(selectedCapture);
  }
  renderSliceOutlines(selectedCapture);
  renderCommentMarkers(selectedCapture);
});

artboardImage.addEventListener("pointerdown", (event) => {
  if (!event.isPrimary || event.button !== 0) return;
  event.preventDefault();
  panState = {
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    panX: canvasPanX,
    panY: canvasPanY,
    moved: false,
  };
  artboardImage.setPointerCapture(event.pointerId);
});

artboardImage.addEventListener("pointermove", (event) => {
  if (!panState || panState.pointerId !== event.pointerId) {
    updateLayerCursor(event.clientX, event.clientY);
    return;
  }

  const deltaX = event.clientX - panState.startX;
  const deltaY = event.clientY - panState.startY;
  if (!panState.moved && Math.hypot(deltaX, deltaY) < 3) {
    updateLayerCursor(event.clientX, event.clientY);
    return;
  }

  panState.moved = true;
  artboardImage.classList.add("is-panning");
  canvasPanX = panState.panX + deltaX;
  canvasPanY = panState.panY + deltaY;
  applyCanvasPan();
});

function finishArtboardPointer(event: PointerEvent, cancelled = false) {
  if (!panState || panState.pointerId !== event.pointerId) return;
  const moved = panState.moved;
  panState = null;
  artboardImage.classList.remove("is-panning");
  if (artboardImage.hasPointerCapture(event.pointerId)) {
    artboardImage.releasePointerCapture(event.pointerId);
  }
  if (!cancelled && !moved) selectLayerAt(event.clientX, event.clientY);
  updateLayerCursor(event.clientX, event.clientY);
}

artboardImage.addEventListener("pointerup", (event) => finishArtboardPointer(event));
artboardImage.addEventListener("pointercancel", (event) => finishArtboardPointer(event, true));
artboardImage.addEventListener("pointerleave", () => {
  if (!panState) artboardImage.classList.remove("has-layer");
});

document.addEventListener("pointerdown", (event) => {
  const target = event.target as Node;
  if (
    artboardImage.contains(target) ||
    commentMarkers.contains(target) ||
    commentPopover.contains(target) ||
    inspectorDetails.contains(target) ||
    mcpLinkRoot.contains(target)
  ) return;
  clearLayerSelection();
});

window.addEventListener("resize", () => {
  applySidebarWidth(sidebarWidth);
  scheduleInspectorFit();
  positionCommentPopover();
});
window.addEventListener("scroll", scheduleInspectorFit, { passive: true });

async function initialize() {
  await listen<BrowserExtensionHeartbeat>("browser-extension-heartbeat", ({ payload }) => {
    if (payload.browser !== browserExtensionBrowser.value) return;
    void refreshBrowserExtensionStatus().catch((error) => {
      console.warn("浏览器插件心跳状态更新失败", error);
    });
  });

  await listen<BrowserCaptureRequested>("browser-capture-requested", ({ payload }) => {
    beginBrowserCapture(payload);
  });

  await listen<CaptureProgress>("capture-progress", ({ payload }) => {
    if (activeCaptureId && payload.captureId !== activeCaptureId) return;
    const attempt = updateAttemptProgress(payload);
    const isSelectedSliceDownload =
      payload.stage === "slices" && selectedCapture?.captureId === payload.captureId;
    if (!attempt && !isSelectedSliceDownload) return;
    setProgress(payload);
    if (payload.stage !== "complete") {
      const status = payload.stage === "download"
        ? "正在下载画板"
        : payload.stage === "parse"
          ? "正在解析图层"
          : payload.stage === "slices"
            ? "正在下载切图"
          : "正在读取蓝湖数据";
      setStatus(status, "working");
    }
  });

  await listen<CaptureFailure>("capture-failed", ({ payload }) => {
    if (activeCaptureId && payload.captureId !== activeCaptureId) return;
    const attempt = captureAttempts.find(
      (item) => item.captureId === payload.captureId || item.key === activeCaptureKey,
    );
    if (!attempt) return;
    failCaptureAttempt(attempt.key, payload.message);
    setCapturing(false);
    setStatus("抓取失败", "error");
    showError(payload.message);
    progressPanel.classList.add("hidden");
  });

  await listen<CaptureResult>("capture-complete", ({ payload }) => {
    if (activeCaptureId && payload.captureId !== activeCaptureId) return;
    const key = captureSourceKey(payload.sourceUrl);
    const replacedCaptureId = replacementCaptureIds.get(key);
    replacementCaptureIds.delete(key);
    captureAttempts = captureAttempts.filter((attempt) => attempt.key !== key);
    persistCaptureAttempts();
    setCapturing(false);
    const complete = slicesComplete(payload);
    setStatus(complete ? "抓取完成" : "切图后台下载中", complete ? "success" : "working");
    if (complete) {
      progressPanel.classList.add("hidden");
    } else {
      setProgress({
        captureId: payload.captureId,
        stage: "slices",
        message: `正在下载切图 0/${sliceTotal(payload)}`,
        percent: 0,
      });
    }
    history = dedupeHistory([payload, ...history.filter((item) => captureSourceKey(item.sourceUrl) !== key)]);
    renderCapture(payload);
    scrollHistoryEntryIntoView(key);
    if (replacedCaptureId && replacedCaptureId !== payload.captureId) {
      void invoke("delete_saved_capture", { captureId: replacedCaptureId }).catch((error) => {
        console.warn("旧版抓取记录清理失败", error);
      });
    }
  });

  await listen<CaptureResult>("capture-updated", ({ payload }) => {
    updateCapturedSlices(payload);
    const isVisibleCapture =
      selectedCapture?.captureId === payload.captureId &&
      (!activeCaptureId || activeCaptureId === payload.captureId);
    if (!isVisibleCapture) return;
    progressPanel.classList.add("hidden");
    const tone = payload.sliceFailedCount ? "error" : "success";
    setStatus(payload.sliceFailedCount ? "部分切图失败" : "切图下载完成", tone);
  });

  const rememberedAttempt = captureAttempts.find((attempt) => attempt.key === selectedHistoryKey);
  if (rememberedAttempt) renderCaptureAttempt(rememberedAttempt);
  else renderHistory();

  try {
    await refreshBrowserExtensionStatus();
  } catch (error) {
    console.warn("浏览器插件状态检测失败", error);
    renderBrowserExtensionStatus(null);
  }

  window.setInterval(() => {
    if (captureMethod !== "browser") return;
    void refreshBrowserExtensionStatus().catch((error) => {
      console.warn("浏览器插件实时状态检测失败", error);
      renderBrowserExtensionStatus(null);
    });
  }, 3000);

  try {
    history = dedupeHistory(await invoke<CaptureResult[]>("list_saved_captures"));
    const successfulKeys = new Set(history.map((capture) => captureSourceKey(capture.sourceUrl)));
    captureAttempts = captureAttempts.filter((attempt) => !successfulKeys.has(attempt.key));
    persistCaptureAttempts();

    const entries = historyEntries();
    const remembered = entries.find((entry) => entry.key === selectedHistoryKey) || entries[0];
    if (remembered?.capture) renderCapture(remembered.capture);
    else if (remembered?.attempt) renderCaptureAttempt(remembered.attempt);
    else renderEmptyCapture();

  } catch (error) {
    showError(`无法读取抓取历史：${String(error)}`);
  } finally {
    const savedScrollTop = storedNumber(HISTORY_SCROLL_TOP_STORAGE_KEY, 0);
    window.requestAnimationFrame(() => {
      historyList.scrollTop = savedScrollTop;
    });
  }
}

void initialize();
