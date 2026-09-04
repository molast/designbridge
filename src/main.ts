import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import { mountPhotoPreview, showPhotoPreview, type PreviewItem } from "./photo-preview";

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

type CapturedDesign = {
  id: string;
  name: string;
  width: number | null;
  height: number | null;
  updateTime: string | null;
  hasComment: boolean;
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

type CaptureResult = {
  captureId: string;
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
};

const captureForm = document.querySelector<HTMLFormElement>("#capture-form")!;
const urlInput = document.querySelector<HTMLInputElement>("#lanhu-url")!;
const captureButton = document.querySelector<HTMLButtonElement>("#capture-button")!;
const cancelButton = document.querySelector<HTMLButtonElement>("#cancel-button")!;
const openFolderButton = document.querySelector<HTMLButtonElement>("#open-folder-button")!;
const fieldError = document.querySelector<HTMLElement>("#field-error")!;
const progressPanel = document.querySelector<HTMLElement>("#progress-panel")!;
const progressMessage = document.querySelector<HTMLElement>("#progress-message")!;
const progressValue = document.querySelector<HTMLElement>("#progress-value")!;
const progressBar = document.querySelector<HTMLElement>("#progress-bar")!;
const appStatus = document.querySelector<HTMLElement>("#app-status")!;
const historyCount = document.querySelector<HTMLElement>("#history-count")!;
const historyList = document.querySelector<HTMLElement>("#history-list")!;
const resultMeta = document.querySelector<HTMLElement>("#result-meta")!;
const emptyState = document.querySelector<HTMLElement>("#empty-state")!;
const designGrid = document.querySelector<HTMLElement>("#design-grid")!;
const deleteConfirmDialog = document.querySelector<HTMLDialogElement>("#delete-confirm-dialog")!;
const deleteConfirmMessage = document.querySelector<HTMLElement>("#delete-confirm-message")!;
const deleteConfirmError = document.querySelector<HTMLElement>("#delete-confirm-error")!;
const deleteCancelButton = document.querySelector<HTMLButtonElement>("#delete-cancel-button")!;
const deleteConfirmButton = document.querySelector<HTMLButtonElement>("#delete-confirm-button")!;
const previewRoot = document.querySelector<HTMLElement>("#preview-root")!;
mountPhotoPreview(previewRoot);

let activeCaptureId: string | null = null;
let selectedCapture: CaptureResult | null = null;
let history: CaptureResult[] = [];
let pendingDeleteCaptureId: string | null = null;

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

function imageSize(width: number | null, height: number | null): string {
  if (width == null || height == null) return "尺寸未知";
  return `${Math.round(width)} × ${Math.round(height)} px`;
}

function imageSource(design: CapturedDesign): string {
  return design.localPath ? convertFileSrc(design.localPath) : design.remoteUrl;
}

function sliceSource(slice: CapturedSlice): string {
  return slice.localPath ? convertFileSrc(slice.localPath) : slice.remoteUrl;
}

function imageFormat(localPath: string | null, remoteUrl: string): string {
  const path = (localPath || remoteUrl).split(/[?#]/, 1)[0];
  const extension = path.match(/\.([a-z0-9]+)$/i)?.[1];
  return extension ? extension.toUpperCase() : "未知";
}

function sliceDensity(slice: CapturedSlice): string {
  const pathParts = slice.outputDir.replace(/\\/g, "/").split("/").filter(Boolean);
  const directory = pathParts[pathParts.length - 1];
  return directory || "mipmap-xxhdpi";
}

function imageDetails(
  key: string,
  name: string,
  width: number | null,
  height: number | null,
  format: string,
  resourceLabel: string,
  resourceValue: string,
): string {
  return `
    <span class="image-hover-details" aria-hidden="true">
      <strong>${escapeHtml(name)}</strong>
      <span class="image-hover-specs">
        <span><small>尺寸</small><b data-image-size="${key}">${imageSize(width, height)}</b></span>
        <span><small>格式</small><b>${escapeHtml(format)}</b></span>
        <span><small>${escapeHtml(resourceLabel)}</small><b>${escapeHtml(resourceValue)}</b></span>
      </span>
    </span>
  `;
}

function syncRenderedImageSizes(capture: CaptureResult) {
  designGrid.querySelectorAll<HTMLImageElement>("img[data-image-kind][data-image-index]").forEach((image) => {
    const updateSize = () => {
      if (!image.naturalWidth || !image.naturalHeight) return;

      const kind = image.dataset.imageKind;
      const index = Number(image.dataset.imageIndex);
      const item = kind === "slice" ? capture.slices[index] : capture.designs[index];
      if (!item) return;

      if (kind === "slice" || item.width == null || item.height == null) {
        item.width = image.naturalWidth;
        item.height = image.naturalHeight;
      }
      designGrid.querySelectorAll<HTMLElement>(`[data-image-size="${kind}-${index}"]`).forEach((element) => {
        element.textContent = imageSize(item.width, item.height);
      });
    };

    if (image.complete) updateSize();
    else image.addEventListener("load", updateSize, { once: true });
  });
}

function previewItems(capture: CaptureResult): PreviewItem[] {
  return [
    ...capture.designs.flatMap((design, index) =>
      design.error
        ? []
        : [{
            key: `design-${index}`,
            src: imageSource(design),
            title: design.name,
            detail: `尺寸 ${imageSize(design.width, design.height)} · 格式 ${imageFormat(design.localPath, design.remoteUrl)} · 类型 原始画板`,
          }],
    ),
    ...capture.slices.flatMap((slice, index) =>
      slice.error
        ? []
        : [{
            key: `slice-${index}`,
            src: sliceSource(slice),
            title: slice.name,
            detail: `尺寸 ${imageSize(slice.width, slice.height)} · 格式 ${slice.outputFormat.toUpperCase()} · 目录 ${sliceDensity(slice)}`,
          }],
    ),
  ];
}

function setStatus(label: string, tone: "idle" | "working" | "success" | "error" = "idle") {
  appStatus.className = `app-status ${tone}`;
  appStatus.querySelector("span:last-child")!.textContent = label;
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

function setCapturing(capturing: boolean) {
  captureButton.disabled = capturing;
  captureButton.textContent = capturing ? "抓取中…" : "开始抓取";
  cancelButton.classList.toggle("hidden", !capturing);
  urlInput.disabled = capturing;
  if (!capturing) activeCaptureId = null;
}

function renderHistory() {
  historyCount.textContent = String(history.length);
  if (history.length === 0) {
    historyList.innerHTML = '<p class="history-empty">暂无记录</p>';
    return;
  }

  historyList.innerHTML = history
    .map(
      (capture) => `
        <div class="history-item${selectedCapture?.captureId === capture.captureId ? " active" : ""}">
          <button
            class="history-select"
            type="button"
            data-capture-id="${escapeHtml(capture.captureId)}"
          >
            <span class="history-thumbnail-count">${capture.downloadedCount}</span>
            <span class="history-copy">
              <strong>${escapeHtml(capture.projectName)}</strong>
              <span>${displayDate(capture.capturedAt)}</span>
            </span>
          </button>
          <button
            class="history-delete"
            type="button"
            data-delete-capture-id="${escapeHtml(capture.captureId)}"
            aria-label="删除 ${escapeHtml(capture.projectName)}"
            title="删除抓取记录"
          >×</button>
        </div>
      `,
    )
    .join("");
}

function renderEmptyCapture() {
  selectedCapture = null;
  openFolderButton.classList.add("hidden");
  resultMeta.innerHTML = "";
  designGrid.innerHTML = "";
  designGrid.classList.add("hidden");
  emptyState.classList.remove("hidden");
  renderHistory();
}

function renderCapture(capture: CaptureResult) {
  selectedCapture = capture;
  renderHistory();
  openFolderButton.classList.remove("hidden");
  emptyState.classList.add("hidden");
  designGrid.classList.remove("hidden");

  resultMeta.innerHTML = `
    <strong>${escapeHtml(capture.projectName)}</strong>
    <span>${capture.downloadedCount} 个画板</span>
    ${capture.slices.length ? `<span>${capture.sliceDownloadedCount} 个切图已导出</span>` : ""}
    ${capture.failedCount ? `<span class="error-text">${capture.failedCount} 个失败</span>` : ""}
    ${capture.sliceFailedCount ? `<span class="error-text">${capture.sliceFailedCount} 个切图失败</span>` : ""}
  `;

  if (capture.designs.length === 0 && capture.slices.length === 0) {
    designGrid.innerHTML = '<p class="no-designs">项目中没有可下载的设计稿。</p>';
    return;
  }

  const designCards = capture.designs
    .map((design, index) => {
      const source = escapeHtml(imageSource(design));
      const failed = design.error != null;
      const key = `design-${index}`;
      const format = imageFormat(design.localPath, design.remoteUrl);
      return `
        <article class="design-item${failed ? " failed" : ""}">
          <button class="design-preview" type="button" data-preview-kind="design" data-preview-index="${index}" aria-label="预览 ${escapeHtml(design.name)}" ${failed ? "disabled" : ""}>
            ${
              failed
                ? `<span class="image-failure">${escapeHtml(design.error || "下载失败")}</span>`
                : `<img src="${source}" alt="${escapeHtml(design.name)}" loading="lazy" data-image-kind="design" data-image-index="${index}" />
                  ${imageDetails(key, design.name, design.width, design.height, format, "类型", "原始画板")}`
            }
          </button>
          <div class="design-caption">
            <strong title="${escapeHtml(design.name)}">${escapeHtml(design.name)}</strong>
            <span data-image-size="${key}">${imageSize(design.width, design.height)}</span>
          </div>
        </article>
      `;
    })
    .join("");
  const sliceCards = capture.slices
    .map((slice, index) => {
      const failed = slice.error != null;
      const key = `slice-${index}`;
      const density = sliceDensity(slice);
      const format = slice.outputFormat.toUpperCase();
      return `
        <article class="design-item slice-item${failed ? " failed" : ""}">
          <button class="design-preview" type="button" data-preview-kind="slice" data-preview-index="${index}" aria-label="预览 ${escapeHtml(slice.name)}" ${failed ? "disabled" : ""}>
            ${
              failed
                ? `<span class="image-failure">${escapeHtml(slice.error || "导出失败")}</span>`
                : `<img src="${escapeHtml(sliceSource(slice))}" alt="${escapeHtml(slice.name)}" loading="lazy" data-image-kind="slice" data-image-index="${index}" />
                  ${imageDetails(key, slice.name, slice.width, slice.height, format, "目录", density)}`
            }
          </button>
          <div class="design-caption">
            <strong title="${escapeHtml(slice.name)}">${escapeHtml(slice.name)}</strong>
            <span>${format} / ${escapeHtml(density)}</span>
          </div>
        </article>
      `;
    })
    .join("");
  designGrid.innerHTML = `${designCards}${sliceCards}`;
  syncRenderedImageSizes(capture);
}

function closeDeleteDialog() {
  pendingDeleteCaptureId = null;
  deleteConfirmButton.disabled = false;
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
      if (history[0]) renderCapture(history[0]);
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

async function startCapture() {
  clearError();
  let parsed: URL;
  try {
    parsed = new URL(urlInput.value.trim());
  } catch {
    showError("请输入完整的蓝湖链接");
    return;
  }

  if (parsed.protocol !== "https:" || !/(^|\.)lanhu(app)?\.com$/i.test(parsed.hostname)) {
    showError("当前仅支持 lanhuapp.com 的 HTTPS 链接");
    return;
  }

  setCapturing(true);
  setStatus("等待蓝湖授权", "working");
  setProgress({ captureId: "", stage: "authorize", message: "正在打开蓝湖窗口…", percent: 4 });

  try {
    activeCaptureId = await invoke<string>("start_lanhu_capture", {
      url: parsed.toString(),
    });
  } catch (error) {
    setCapturing(false);
    setStatus("抓取失败", "error");
    showError(String(error));
  }
}

captureForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void startCapture();
});

cancelButton.addEventListener("click", async () => {
  if (!activeCaptureId) return;
  const captureId = activeCaptureId;
  try {
    await invoke("cancel_lanhu_capture", { captureId });
  } finally {
    setCapturing(false);
    progressPanel.classList.add("hidden");
    setStatus("已取消");
  }
});

openFolderButton.addEventListener("click", async () => {
  if (!selectedCapture) return;
  try {
    await openPath(selectedCapture.outputDir);
  } catch (error) {
    showError(`无法打开目录：${String(error)}`);
  }
});

historyList.addEventListener("click", (event) => {
  const deleteTarget = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-delete-capture-id]");
  if (deleteTarget) {
    const captureId = deleteTarget.dataset.deleteCaptureId;
    const capture = history.find((item) => item.captureId === captureId);
    if (capture) requestCaptureDeletion(capture);
    return;
  }

  const target = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-capture-id]");
  if (!target) return;
  const capture = history.find((item) => item.captureId === target.dataset.captureId);
  if (capture) renderCapture(capture);
});

deleteCancelButton.addEventListener("click", closeDeleteDialog);
deleteConfirmButton.addEventListener("click", () => void confirmCaptureDeletion());
deleteConfirmDialog.addEventListener("cancel", (event) => {
  event.preventDefault();
  if (!deleteConfirmButton.disabled) closeDeleteDialog();
});

designGrid.addEventListener("click", (event) => {
  const target = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-preview-kind][data-preview-index]");
  if (!target || !selectedCapture) return;
  const index = Number(target.dataset.previewIndex);
  const kind = target.dataset.previewKind === "slice" ? "slice" : "design";
  const items = previewItems(selectedCapture);
  const itemIndex = items.findIndex((item) => item.key === `${kind}-${index}`);
  if (itemIndex < 0) return;
  showPhotoPreview({ items, index: itemIndex });
});

async function initialize() {
  await listen<CaptureProgress>("capture-progress", ({ payload }) => {
    if (activeCaptureId && payload.captureId !== activeCaptureId) return;
    setProgress(payload);
    setStatus(payload.stage === "download" ? "正在下载" : "等待蓝湖授权", "working");
  });

  await listen<CaptureFailure>("capture-failed", ({ payload }) => {
    if (activeCaptureId && payload.captureId !== activeCaptureId) return;
    setCapturing(false);
    setStatus("抓取失败", "error");
    showError(payload.message);
    progressPanel.classList.add("hidden");
  });

  await listen<CaptureResult>("capture-complete", ({ payload }) => {
    if (activeCaptureId && payload.captureId !== activeCaptureId) return;
    setCapturing(false);
    setStatus("抓取完成", payload.failedCount ? "error" : "success");
    setProgress({
      captureId: payload.captureId,
      stage: "complete",
      message: payload.failedCount ? "抓取完成，部分画板下载失败" : "抓取完成",
      percent: 100,
    });
    history = [payload, ...history.filter((item) => item.captureId !== payload.captureId)];
    renderCapture(payload);
  });

  try {
    history = await invoke<CaptureResult[]>("list_saved_captures");
    renderHistory();
    if (history[0]) renderCapture(history[0]);
  } catch (error) {
    showError(`无法读取抓取历史：${String(error)}`);
  }
}

void initialize();
