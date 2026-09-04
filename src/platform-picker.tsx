import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { ChevronDown, Smartphone } from "lucide-react";

export type TargetPlatform = "android" | "ios";

export type PlatformFrame = {
  platform: string;
  width: number;
  height: number;
  unit: string;
};

type PickerState = {
  frame: PlatformFrame | null;
  platform: TargetPlatform;
};

let currentState: PickerState = { frame: null, platform: "android" };
let updatePicker: ((state: PickerState) => void) | null = null;
let changePlatform: ((platform: TargetPlatform) => void) | null = null;

export function mountPlatformPicker(element: HTMLElement, onPlatformChange: (platform: TargetPlatform) => void) {
  changePlatform = onPlatformChange;
  createRoot(element).render(<PlatformPicker />);
}

export function showPlatformPicker(frame: PlatformFrame | null, platform: TargetPlatform) {
  currentState = { frame, platform };
  updatePicker?.(currentState);
}

function PlatformPicker() {
  const [state, setState] = useState<PickerState>(currentState);

  useEffect(() => {
    updatePicker = setState;
    return () => {
      updatePicker = null;
    };
  }, []);

  const { frame, platform } = state;
  if (!frame) return null;
  const label = platform === "ios" ? "iOS" : "Android";
  const unit = platform === "ios" ? "pt" : "dp";

  return (
    <div className="platform-picker">
      <Smartphone aria-hidden="true" size={20} strokeWidth={1.8} />
      <strong>{label}</strong>
      <span>{Math.round(frame.width)} × {Math.round(frame.height)} {unit}</span>
      <ChevronDown className="platform-picker-chevron" aria-hidden="true" size={17} strokeWidth={1.8} />
      <select
        aria-label="目标平台"
        value={platform}
        onChange={(event) => changePlatform?.(event.target.value as TargetPlatform)}
      >
        <option value="android">Android</option>
        <option value="ios">iOS</option>
      </select>
    </div>
  );
}
