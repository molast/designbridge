import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import Lightbox from "yet-another-react-lightbox";
import Captions from "yet-another-react-lightbox/plugins/captions";
import Zoom from "yet-another-react-lightbox/plugins/zoom";
import "yet-another-react-lightbox/styles.css";
import "yet-another-react-lightbox/plugins/captions.css";

const checkerboardBackground = {
  backgroundColor: "#ffffff",
  backgroundImage: "conic-gradient(#e1e4df 25%, #ffffff 0 50%, #e1e4df 0 75%, #ffffff 0)",
  backgroundSize: "20px 20px",
};

export type PreviewItem = {
  key: string;
  src: string;
  title: string;
  detail: string;
};

export type PreviewRequest = {
  items: PreviewItem[];
  index: number;
};

let requestPreview: ((request: PreviewRequest) => void) | null = null;

export function mountPhotoPreview(element: HTMLElement) {
  createRoot(element).render(<PhotoPreviewHost />);
}

export function showPhotoPreview(request: PreviewRequest) {
  requestPreview?.(request);
}

function PhotoPreviewHost() {
  const [request, setRequest] = useState<PreviewRequest | null>(null);

  useEffect(() => {
    requestPreview = setRequest;
    return () => {
      requestPreview = null;
    };
  }, []);

  if (!request || request.items.length === 0) return null;

  return (
    <Lightbox
      open
      close={() => setRequest(null)}
      slides={request.items.map((item) => ({
        src: item.src,
        alt: item.title,
        title: item.title,
        description: item.detail,
      }))}
      index={request.index}
      plugins={[Zoom, Captions]}
      carousel={{ finite: request.items.length < 2 }}
      controller={{ closeOnBackdropClick: true }}
      zoom={{ scrollToZoom: false, pinchZoomV4: true }}
      styles={{ container: checkerboardBackground }}
      labels={{
        Close: "关闭",
        Previous: "上一张",
        Next: "下一张",
        "Zoom in": "放大",
        "Zoom out": "缩小",
      }}
      on={{
        view: ({ index }) => setRequest((current) => (current ? { ...current, index } : current)),
      }}
    />
  );
}
