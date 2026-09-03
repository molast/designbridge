import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";

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
const previewDialog = document.querySelector<HTMLDialogElement>("#preview-dialog")!;
const previewImage = document.querySelector<HTMLImageElement>("#preview-image")!;
const previewTitle = document.querySelector<HTMLElement>("#preview-title")!;
const previewSize = document.querySelector<HTMLElement>("#preview-size")!;
const closePreview = document.querySelector<HTMLButtonElement>("#close-preview")!;

let activeCaptureId: string | null = null;
let selectedCapture: CaptureResult | null = null;
let history: CaptureResult[] = [];

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

function designSize(design: CapturedDesign): string {
  if (design.width == null || design.height == null) return "尺寸未知";
  return `${Math.round(design.width)} × ${Math.round(design.height)}`;
}

function imageSource(design: CapturedDesign): string {
  return design.localPath ? convertFileSrc(design.localPath) : design.remoteUrl;
}

function sliceSource(slice: CapturedSlice): string {
  return slice.localPath ? convertFileSrc(slice.localPath) : slice.remoteUrl;
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
        <button
          class="history-item${selectedCapture?.captureId === capture.captureId ? " active" : ""}"
          type="button"
          data-capture-id="${escapeHtml(capture.captureId)}"
        >
          <span class="history-thumbnail-count">${capture.downloadedCount}</span>
          <span class="history-copy">
            <strong>${escapeHtml(capture.projectName)}</strong>
            <span>${displayDate(capture.capturedAt)}</span>
          </span>
        </button>
      `,
    )
    .join("");
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
      return `
        <article class="design-item${failed ? " failed" : ""}">
          <button class="design-preview" type="button" data-design-index="${index}" ${failed ? "disabled" : ""}>
            ${
              failed
                ? `<span class="image-failure">${escapeHtml(design.error || "下载失败")}</span>`
                : `<img src="${source}" alt="${escapeHtml(design.name)}" loading="lazy" />`
            }
          </button>
          <div class="design-caption">
            <strong title="${escapeHtml(design.name)}">${escapeHtml(design.name)}</strong>
            <span>${designSize(design)}</span>
          </div>
        </article>
      `;
    })
    .join("");
  const sliceCards = capture.slices
    .map((slice) => {
      const failed = slice.error != null;
      return `
        <article class="design-item slice-item${failed ? " failed" : ""}">
          <div class="design-preview">
            ${
              failed
                ? `<span class="image-failure">${escapeHtml(slice.error || "导出失败")}</span>`
                : `<img src="${escapeHtml(sliceSource(slice))}" alt="${escapeHtml(slice.name)}" loading="lazy" />`
            }
          </div>
          <div class="design-caption">
            <strong title="${escapeHtml(slice.name)}">${escapeHtml(slice.name)}</strong>
            <span>${slice.outputFormat.toUpperCase()} / mipmap-xxhdpi</span>
          </div>
        </article>
      `;
    })
    .join("");
  designGrid.innerHTML = `${designCards}${sliceCards}`;
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
  const target = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-capture-id]");
  if (!target) return;
  const capture = history.find((item) => item.captureId === target.dataset.captureId);
  if (capture) renderCapture(capture);
});

designGrid.addEventListener("click", (event) => {
  const target = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-design-index]");
  if (!target || !selectedCapture) return;
  const design = selectedCapture.designs[Number(target.dataset.designIndex)];
  if (!design || design.error) return;

  previewImage.src = imageSource(design);
  previewImage.alt = design.name;
  previewTitle.textContent = design.name;
  previewSize.textContent = designSize(design);
  previewDialog.showModal();
});

closePreview.addEventListener("click", () => previewDialog.close());
previewDialog.addEventListener("click", (event) => {
  if (event.target === previewDialog) previewDialog.close();
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
