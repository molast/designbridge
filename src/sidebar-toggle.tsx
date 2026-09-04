import { ChevronLeft, ChevronRight } from "lucide-react";
import { useState } from "react";
import { createRoot } from "react-dom/client";

type SidebarToggleOptions = {
  collapsed: boolean;
  onChange: (collapsed: boolean) => void;
};

function SidebarToggle({ collapsed: initialCollapsed, onChange }: SidebarToggleOptions) {
  const [collapsed, setCollapsed] = useState(initialCollapsed);

  function toggle() {
    const next = !collapsed;
    setCollapsed(next);
    onChange(next);
  }

  const label = collapsed ? "展开抓取记录" : "收起抓取记录";
  const Icon = collapsed ? ChevronRight : ChevronLeft;

  return (
    <button
      className="sidebar-toggle-button"
      type="button"
      aria-label={label}
      aria-expanded={!collapsed}
      title={label}
      onClick={toggle}
      onPointerUp={(event) => event.currentTarget.blur()}
    >
      <Icon aria-hidden="true" size={24} strokeWidth={1.8} />
    </button>
  );
}

export function mountSidebarToggle(element: HTMLElement, options: SidebarToggleOptions) {
  createRoot(element).render(<SidebarToggle {...options} />);
}
