import { Check, Copy, RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

let currentTarget: McpLinkTarget | null = null;
let updateLink: ((target: McpLinkTarget | null) => void) | null = null;
let refreshing = false;
let updateRefreshing: ((refreshing: boolean) => void) | null = null;
let refreshDesign: (() => void) | null = null;

export type McpLinkTarget = {
  link: string;
  layerName?: string | null;
};

export function mountMcpLinkButton(element: HTMLElement, onRefresh: () => void) {
  refreshDesign = onRefresh;
  createRoot(element).render(<McpLinkButton />);
}

export function showMcpLinkButton(link: string | McpLinkTarget | null) {
  currentTarget = typeof link === "string" ? { link } : link;
  updateLink?.(currentTarget);
}

export function showMcpRefreshState(nextRefreshing: boolean) {
  refreshing = nextRefreshing;
  updateRefreshing?.(nextRefreshing);
}

async function writeClipboard(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    return;
  } catch {
    const textarea = document.createElement("textarea");
    textarea.value = value;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.append(textarea);
    textarea.select();
    const copied = document.execCommand("copy");
    textarea.remove();
    if (!copied) throw new Error("无法写入剪贴板");
  }
}

function McpLinkButton() {
  const [target, setTarget] = useState<McpLinkTarget | null>(currentTarget);
  const [copied, setCopied] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(refreshing);
  const resetTimer = useRef<number | null>(null);

  useEffect(() => {
    updateLink = (nextTarget) => {
      setTarget(nextTarget);
      setCopied(false);
    };
    updateRefreshing = setIsRefreshing;
    return () => {
      updateLink = null;
      updateRefreshing = null;
      if (resetTimer.current != null) window.clearTimeout(resetTimer.current);
    };
  }, []);

  if (!target) return null;

  const copyLink = async () => {
    await writeClipboard(target.link);
    setCopied(true);
    if (resetTimer.current != null) window.clearTimeout(resetTimer.current);
    resetTimer.current = window.setTimeout(() => setCopied(false), 1600);
  };

  const Icon = copied ? Check : Copy;
  const scope = target.layerName ? `图层“${target.layerName}”` : "整张设计稿";
  const label = copied ? "已复制" : `复制${scope} MCP 链接`;

  return (
    <div className="mcp-link-actions">
      <button
        className={`mcp-link-button mcp-refresh-button${isRefreshing ? " is-refreshing" : ""}`}
        type="button"
        disabled={isRefreshing}
        aria-label={isRefreshing ? "正在刷新设计稿" : "强制刷新设计稿"}
        title={isRefreshing ? "正在刷新设计稿" : "重新从蓝湖抓取并替换当前设计稿"}
        onClick={() => refreshDesign?.()}
        onPointerUp={(event) => event.currentTarget.blur()}
      >
        <RefreshCw aria-hidden="true" size={15} strokeWidth={1.8} />
        <span>{isRefreshing ? "刷新中" : "刷新"}</span>
      </button>
      <button
        className="mcp-link-button"
        type="button"
        aria-label={label}
        title={label}
        onClick={() => void copyLink()}
        onPointerUp={(event) => event.currentTarget.blur()}
      >
        <Icon aria-hidden="true" size={15} strokeWidth={1.8} />
        <span>{copied ? "已复制" : target.layerName ? "复制图层链接" : "MCP 链接"}</span>
      </button>
    </div>
  );
}
