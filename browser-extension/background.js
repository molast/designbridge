const NATIVE_HOST = "com.designbridge.browser";
const CAPTURE_PARAM = "designbridge_capture";
const HEARTBEAT_RETRY_DELAY_MS = 2000;
const HEARTBEAT_ACK_TIMEOUT_MS = 5000;
const captureTasks = new Map();
const extensionVersion = chrome.runtime.getManifest().version;
const heartbeatSessionId = crypto.randomUUID();
let heartbeatPort = null;
let heartbeatBrowser = "chrome";
let heartbeatAcknowledgedAt = 0;

async function detectBrowser() {
  if (/Edg\//.test(navigator.userAgent)) return "edge";
  if (navigator.brave?.isBrave && await navigator.brave.isBrave()) return "brave";
  if (/Chromium\//.test(navigator.userAgent) && !/Chrome\//.test(navigator.userAgent)) return "chromium";
  return "chrome";
}

function sendHeartbeat() {
  if (!heartbeatPort) return;
  const port = heartbeatPort;
  const sentAt = Date.now();
  try {
    port.postMessage({
      version: 1,
      type: "heartbeat",
      browser: heartbeatBrowser,
      extensionVersion,
      sessionId: heartbeatSessionId,
    });
  } catch {
    reconnectHeartbeat(port);
    return;
  }

  setTimeout(() => {
    if (heartbeatPort === port && heartbeatAcknowledgedAt < sentAt) reconnectHeartbeat(port);
  }, HEARTBEAT_ACK_TIMEOUT_MS);
}

function reconnectHeartbeat(port) {
  if (heartbeatPort !== port) return;
  heartbeatPort = null;
  heartbeatAcknowledgedAt = 0;
  try {
    port.disconnect();
  } catch {
    // The native port may already be closed.
  }
  setTimeout(connectHeartbeat, HEARTBEAT_RETRY_DELAY_MS);
}

function connectHeartbeat() {
  if (heartbeatPort) return;
  try {
    const port = chrome.runtime.connectNative(NATIVE_HOST);
    heartbeatPort = port;
    heartbeatAcknowledgedAt = 0;
    port.onMessage.addListener((response) => {
      if (response?.ok) {
        heartbeatAcknowledgedAt = Date.now();
        return;
      }
      reconnectHeartbeat(port);
    });
    port.onDisconnect.addListener(() => {
      if (heartbeatPort !== port) return;
      heartbeatPort = null;
      heartbeatAcknowledgedAt = 0;
      void chrome.runtime.lastError;
      setTimeout(connectHeartbeat, HEARTBEAT_RETRY_DELAY_MS);
    });
    sendHeartbeat();
  } catch {
    heartbeatPort = null;
    heartbeatAcknowledgedAt = 0;
    setTimeout(connectHeartbeat, HEARTBEAT_RETRY_DELAY_MS);
  }
}

void detectBrowser().then((browser) => {
  heartbeatBrowser = browser;
  connectHeartbeat();
});
setInterval(sendHeartbeat, 3000);

function captureTarget(rawUrl) {
  try {
    const url = new URL(rawUrl);
    const host = url.hostname.toLowerCase();
    const isLanhu = url.protocol === "https:"
      && (host === "lanhuapp.com" || host.endsWith(".lanhuapp.com") || host === "lanhu.com" || host.endsWith(".lanhu.com"));
    const captureId = url.searchParams.get(CAPTURE_PARAM);
    if (!isLanhu || !captureId) return null;
    url.searchParams.delete(CAPTURE_PARAM);
    return { captureId, sourceUrl: url.toString() };
  } catch {
    return null;
  }
}

function lanhuDomain(rawUrl) {
  const hostname = new URL(rawUrl).hostname.toLowerCase();
  return hostname === "lanhu.com" || hostname.endsWith(".lanhu.com")
    ? "lanhu.com"
    : "lanhuapp.com";
}

function readLanhuCookies(rawUrl) {
  return new Promise((resolve, reject) => {
    chrome.cookies.getAll({ domain: lanhuDomain(rawUrl) }, (cookies) => {
      const error = chrome.runtime.lastError;
      if (error) {
        reject(new Error(error.message));
        return;
      }
      resolve([...cookies]
        .sort((left, right) => right.path.length - left.path.length)
        .map((cookie) => `${cookie.name}=${cookie.value}`)
        .join("; "));
    });
  });
}

function sendToDesignBridge(message) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(NATIVE_HOST, message, (response) => {
      const error = chrome.runtime.lastError;
      if (error) {
        reject(new Error("请先打开 DesignBridge 客户端，并在客户端中重新安装浏览器扩展"));
        return;
      }
      resolve(response);
    });
  });
}

async function performCapture(message) {
  const cookie = await readLanhuCookies(message.url);
  const authToken = typeof message.authToken === "string" ? message.authToken.trim() : "";
  if (!cookie && !authToken) throw new Error("未读取到蓝湖登录状态，请先在当前浏览器登录蓝湖");

  const request = {
    version: 1,
    type: "capture",
    url: message.url,
    cookie,
    authToken,
  };
  if (message.captureId) request.captureId = message.captureId;

  const response = await sendToDesignBridge(request);
  if (!response?.ok) throw new Error(response?.message || "DesignBridge 未接受抓取任务");
  return response;
}

function capturePage(message) {
  if (!message.captureId) return performCapture(message);
  const existing = captureTasks.get(message.captureId);
  if (existing) return existing;
  const task = performCapture(message).catch(async (error) => {
    captureTasks.delete(message.captureId);
    try {
      await sendToDesignBridge({
        version: 1,
        type: "captureError",
        captureId: message.captureId,
        url: message.url,
        message: error instanceof Error ? error.message : String(error),
      });
    } catch {
      // The page console still receives the original failure below.
    }
    throw error;
  });
  captureTasks.set(message.captureId, task);
  return task;
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== "designbridge:capture" || typeof message.url !== "string") return false;

  capturePage(message)
    .then((response) => sendResponse(response))
    .catch((error) => sendResponse({
      ok: false,
      message: error instanceof Error ? error.message : String(error),
    }));
  return true;
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  const target = captureTarget(changeInfo.url || tab.url || "");
  if (!target || captureTasks.has(target.captureId) || changeInfo.status !== "complete") return;

  chrome.tabs.sendMessage(tabId, { type: "designbridge:capture-current" }, () => {
    const error = chrome.runtime.lastError;
    if (error) console.warn("DesignBridge 无法唤起页面抓取", error.message);
  });
});
