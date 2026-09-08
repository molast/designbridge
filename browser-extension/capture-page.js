(() => {
  const CAPTURE_PARAM = "designbridge_capture";
  const currentUrl = new URL(window.location.href);
  const captureId = currentUrl.searchParams.get(CAPTURE_PARAM);
  currentUrl.searchParams.delete(CAPTURE_PARAM);
  const sourceUrl = currentUrl.toString();

  function authToken() {
    try {
      return window.localStorage.getItem("token") || "";
    } catch {
      return "";
    }
  }

  function requestCapture(requestedCaptureId, requestedUrl, done) {
    chrome.runtime.sendMessage({
      type: "designbridge:capture",
      captureId: requestedCaptureId || undefined,
      url: requestedUrl,
      authToken: authToken(),
    }, (response) => {
      const error = chrome.runtime.lastError;
      if (error || !response?.ok) {
        console.error("DesignBridge 自动抓取失败", error?.message || response?.message || "未知错误");
        done?.({ ok: false, message: error?.message || response?.message || "未知错误" });
        return;
      }

      if (requestedCaptureId) {
        try {
          window.history.replaceState(null, "", requestedUrl);
        } catch (historyError) {
          console.warn("DesignBridge 无法清理临时抓取参数", historyError);
        }
      }
      done?.(response);
    });
  }

  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (message?.type !== "designbridge:capture-current") return false;
    requestCapture(captureId, sourceUrl, sendResponse);
    return true;
  });

  if (captureId) requestCapture(captureId, sourceUrl);
})();
