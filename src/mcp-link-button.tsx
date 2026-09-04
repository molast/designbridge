import { Check, Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";

let currentLink: string | null = null;
let updateLink: ((link: string | null) => void) | null = null;

export function mountMcpLinkButton(element: HTMLElement) {
  createRoot(element).render(<McpLinkButton />);
}

export function showMcpLinkButton(link: string | null) {
  currentLink = link;
  updateLink?.(link);
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
  const [link, setLink] = useState<string | null>(currentLink);
  const [copied, setCopied] = useState(false);
  const resetTimer = useRef<number | null>(null);

  useEffect(() => {
    updateLink = (nextLink) => {
      setLink(nextLink);
      setCopied(false);
    };
    return () => {
      updateLink = null;
      if (resetTimer.current != null) window.clearTimeout(resetTimer.current);
    };
  }, []);

  if (!link) return null;

  const copyLink = async () => {
    await writeClipboard(link);
    setCopied(true);
    if (resetTimer.current != null) window.clearTimeout(resetTimer.current);
    resetTimer.current = window.setTimeout(() => setCopied(false), 1600);
  };

  const Icon = copied ? Check : Copy;
  const label = copied ? "已复制" : "复制 MCP 链接";

  return (
    <button
      className="mcp-link-button"
      type="button"
      aria-label={label}
      title={label}
      onClick={() => void copyLink()}
      onPointerUp={(event) => event.currentTarget.blur()}
    >
      <Icon aria-hidden="true" size={15} strokeWidth={1.8} />
      <span>{copied ? "已复制" : "MCP 链接"}</span>
    </button>
  );
}
