const captureButton = document.querySelector("#capture-button");
const designSummary = document.querySelector(".design-summary");
const designName = document.querySelector("#design-name");
const designId = document.querySelector("#design-id");
const status = document.querySelector("#status");
let currentTab = null;

function lanhuRoute(rawUrl) {
  try {
    const url = new URL(rawUrl);
    if (url.protocol !== "https:") return null;
    const host = url.hostname.toLowerCase();
    if (!(host === "lanhuapp.com" || host.endsWith(".lanhuapp.com") || host === "lanhu.com" || host.endsWith(".lanhu.com"))) {
      return null;
    }
    const queryIndex = url.hash.indexOf("?");
    const params = new URLSearchParams(queryIndex >= 0 ? url.hash.slice(queryIndex + 1) : url.search);
    const projectId = params.get("project_id") || params.get("pid");
    const imageId = params.get("image_id") || params.get("docId");
    if (!projectId || !imageId) return null;
    return { projectId, imageId };
  } catch {
    return null;
  }
}

function setStatus(message, tone = "") {
  status.textContent = message;
  status.className = tone;
}

function queryCurrentTab() {
  return new Promise((resolve) => {
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => resolve(tabs[0] || null));
  });
}

function requestCapture(tabId) {
  return new Promise((resolve, reject) => {
    chrome.tabs.sendMessage(tabId, { type: "designbridge:capture-current" }, (response) => {
      const error = chrome.runtime.lastError;
      if (error) {
        reject(new Error(error.message));
        return;
      }
      resolve(response);
    });
  });
}

async function initialize() {
  currentTab = await queryCurrentTab();
  const route = currentTab?.url ? lanhuRoute(currentTab.url) : null;
  if (!currentTab || !route) {
    designName.textContent = "当前不是蓝湖设计稿页面";
    designId.textContent = "请打开一个具体设计稿后重试";
    designSummary.classList.add("error");
    setStatus("无法识别项目 ID 或设计稿 ID", "error");
    return;
  }

  designName.textContent = currentTab.title || "蓝湖设计稿";
  designId.textContent = route.imageId;
  designSummary.classList.add("ready");
  captureButton.disabled = false;
  setStatus("已识别当前设计稿");
}

captureButton.addEventListener("click", async () => {
  if (!currentTab?.url) return;
  captureButton.disabled = true;
  captureButton.classList.add("working");
  captureButton.querySelector("span:last-child").textContent = "正在发送";
  setStatus("正在连接 DesignBridge…");

  try {
    const response = await requestCapture(currentTab.id);
    if (!response?.ok) throw new Error(response?.message || "DesignBridge 未接受抓取任务");
    setStatus("已发送到 DesignBridge，可回到客户端查看进度", "success");
    captureButton.querySelector("span:last-child").textContent = "已开始抓取";
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), "error");
    captureButton.disabled = false;
    captureButton.querySelector("span:last-child").textContent = "重新抓取";
  } finally {
    captureButton.classList.remove("working");
  }
});

void initialize();
