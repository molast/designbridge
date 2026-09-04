import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { ChevronDown, Download, FolderOpen } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import type { TargetPlatform } from "./platform-picker";

type ExportFormat = "png" | "jpg" | "webp";

type SliceItem = {
  id: string;
  name: string;
  src: string | null;
  width: number | null;
  height: number | null;
  error: string | null;
};

export type SliceExportRequest = {
  captureId: string;
  outputDir: string;
  layerId: string;
  layerName: string;
  layerWidth: number | null;
  layerHeight: number | null;
  hasSlice: boolean;
  pending: boolean;
  platform: TargetPlatform;
  slice: SliceItem | null;
  onPlatformChange: (platform: TargetPlatform) => void;
  onPreview: () => void;
};

type ExportResult = {
  outputDir: string;
  files: Array<{ label: string; path: string; width: number; height: number }>;
};

type ScaleOption = {
  key: string;
  label: string;
  factor: number;
};

const ANDROID_SCALES: ScaleOption[] = [
  { key: "mdpi", label: "mipmap-mdpi", factor: 1 },
  { key: "hdpi", label: "mipmap-hdpi", factor: 1.5 },
  { key: "xhdpi", label: "mipmap-xhdpi", factor: 2 },
  { key: "xxhdpi", label: "mipmap-xxhdpi", factor: 3 },
  { key: "xxxhdpi", label: "mipmap-xxxhdpi", factor: 4 },
];

const IOS_SCALES: ScaleOption[] = [
  { key: "1x", label: "标准点 @1x", factor: 1 },
  { key: "2x", label: "视网膜 @2x", factor: 2 },
  { key: "3x", label: "高清视网膜 @3x", factor: 3 },
];

let currentRequest: SliceExportRequest | null = null;
let updateRequest: ((request: SliceExportRequest | null) => void) | null = null;

export function mountSliceExportPanel(element: HTMLElement) {
  createRoot(element).render(<SliceExportPanel />);
}

export function showSliceExportPanel(request: SliceExportRequest | null) {
  currentRequest = request;
  updateRequest?.(request);
}

function pixelSize(value: number | null, factor: number): string {
  if (value == null) return "尺寸未知";
  return `${Math.max(1, Math.round(value * factor))}px`;
}

function SliceExportPanel() {
  const [request, setRequest] = useState<SliceExportRequest | null>(currentRequest);
  const [format, setFormat] = useState<ExportFormat>("webp");
  const [androidScales, setAndroidScales] = useState<string[]>(["xxhdpi"]);
  const [iosScales, setIosScales] = useState<string[]>(["1x", "2x", "3x"]);
  const [exporting, setExporting] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [exportDir, setExportDir] = useState<string | null>(null);

  useEffect(() => {
    updateRequest = setRequest;
    return () => {
      updateRequest = null;
    };
  }, []);

  useEffect(() => {
    setFormat("webp");
    setAndroidScales(["xxhdpi"]);
    setIosScales(["1x", "2x", "3x"]);
    setExporting(false);
    setFeedback("");
    setExportDir(null);
  }, [request?.captureId, request?.layerId]);

  const scaleOptions = request?.platform === "ios" ? IOS_SCALES : ANDROID_SCALES;
  const selectedScales = request?.platform === "ios" ? iosScales : androidScales;
  const setSelectedScales = request?.platform === "ios" ? setIosScales : setAndroidScales;
  const selectedSet = useMemo(() => new Set(selectedScales), [selectedScales]);

  if (!request) return null;

  const toggleScale = (key: string) => {
    setSelectedScales((current) =>
      current.includes(key) ? current.filter((item) => item !== key) : [...current, key],
    );
    setFeedback("");
    setExportDir(null);
  };

  const exportSlices = async () => {
    if (!request.slice || selectedScales.length === 0) return;
    setExporting(true);
    setFeedback("");
    try {
      const result = await invoke<ExportResult>("export_slice_variants", {
        request: {
          captureId: request.captureId,
          layerId: request.layerId,
          sliceId: request.slice.id,
          format,
          platform: request.platform,
          scales: selectedScales,
        },
      });
      setExportDir(result.outputDir);
      setFeedback(`已导出 ${result.files.length} 个文件`);
    } catch (error) {
      setFeedback(`导出失败：${String(error)}`);
    } finally {
      setExporting(false);
    }
  };

  if (!request.slice) {
    const state = request.hasSlice
      ? request.pending
        ? "切图正在后台下载"
        : "该切图为空像素或 1 × 1 px，已过滤"
      : "该图层没有可导出的切图";
    return (
      <section className="detail-section slice-export-panel">
        <h3>切图</h3>
        <p className={request.pending ? "slice-state working" : "slice-state"}>{state}</p>
      </section>
    );
  }

  return (
    <section className="detail-section slice-export-panel">
      <h3>切图</h3>
      <label className="slice-field-label" htmlFor="slice-export-name">切图名称</label>
      <div className="slice-export-name" id="slice-export-name" title={request.slice.name}>
        {request.slice.name}
      </div>

      {request.slice.error ? (
        <p className="slice-state error-text">{request.slice.error}</p>
      ) : request.slice.src ? (
        <button className="slice-export-preview" type="button" onClick={request.onPreview} aria-label={`预览 ${request.slice.name}`}>
          <img src={request.slice.src} alt={request.slice.name} />
        </button>
      ) : (
        <p className="slice-state working">切图正在后台下载</p>
      )}

      <div className="slice-export-controls">
        <label>
          <span>下载切图格式</span>
          <span className="slice-select">
            <select
              value={format}
              onChange={(event) => {
                setFormat(event.target.value as ExportFormat);
                setFeedback("");
                setExportDir(null);
              }}
            >
              <option value="png">PNG</option>
              <option value="jpg">JPG</option>
              <option value="webp">WEBP</option>
            </select>
            <ChevronDown aria-hidden="true" size={15} strokeWidth={2} />
          </span>
        </label>
        <label>
          <span>切图使用平台</span>
          <span className="slice-select">
            <select
              value={request.platform}
              onChange={(event) => {
                setFeedback("");
                setExportDir(null);
                request.onPlatformChange(event.target.value as TargetPlatform);
              }}
            >
              <option value="android">Android</option>
              <option value="ios">iOS</option>
            </select>
            <ChevronDown aria-hidden="true" size={15} strokeWidth={2} />
          </span>
        </label>
      </div>

      <div className="slice-scale-list">
        {scaleOptions.map((option) => (
          <label className="slice-scale-option" key={option.key}>
            <input
              type="checkbox"
              checked={selectedSet.has(option.key)}
              onChange={() => toggleScale(option.key)}
            />
            <span>{option.label}</span>
            <small>
              {pixelSize(request.layerWidth, option.factor)} × {pixelSize(request.layerHeight, option.factor)}
            </small>
          </label>
        ))}
      </div>

      <div className="slice-export-actions">
        <button
          className="slice-download-button"
          type="button"
          disabled={!request.slice.src || Boolean(request.slice.error) || selectedScales.length === 0 || exporting}
          onClick={() => void exportSlices()}
        >
          <Download aria-hidden="true" size={16} />
          {exporting ? "正在导出…" : "下载切图"}
        </button>
        <button className="slice-folder-button" type="button" onClick={() => void openPath(exportDir || request.outputDir)}>
          <FolderOpen aria-hidden="true" size={16} />
          {exportDir ? "打开导出目录" : "查看所有切图"}
        </button>
      </div>
      {feedback ? <p className={feedback.startsWith("导出失败") ? "slice-export-feedback error-text" : "slice-export-feedback"}>{feedback}</p> : null}
    </section>
  );
}
