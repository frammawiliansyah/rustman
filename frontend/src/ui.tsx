import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Check, Copy, X, type LucideIcon } from "lucide-react";

export function MethodBadge({ method }: { method: string }) {
  return <span className={`mbadge m-${method}`}>{method === "DELETE" ? "DEL" : method.slice(0, 5)}</span>;
}

export interface TabItem<T extends string> {
  key: T; label: string; icon?: LucideIcon; badge?: string | number; dot?: boolean;
}

/** Tab strip with a sliding active indicator (underline or pill). */
export function Tabs<T extends string>({ items, value, onChange, variant = "line", className = "" }: {
  items: TabItem<T>[]; value: T; onChange: (k: T) => void; variant?: "line" | "pill"; className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [ind, setInd] = useState({ left: 0, width: 0, ready: false });
  useLayoutEffect(() => {
    const el = ref.current?.querySelector<HTMLElement>('[data-active="true"]');
    if (!el) return;
    setInd((p) => (p.left === el.offsetLeft && p.width === el.offsetWidth && p.ready ? p : { left: el.offsetLeft, width: el.offsetWidth, ready: true }));
  });
  return (
    <div className={`tabs tabs-${variant} ${className}`} ref={ref} role="tablist">
      {items.map((t) => (
        <button key={t.key} role="tab" aria-selected={value === t.key} data-active={value === t.key}
          className={value === t.key ? "active" : ""} onClick={() => onChange(t.key)}>
          {t.icon && <t.icon size={14} />}
          <span>{t.label}</span>
          {t.badge !== undefined && t.badge !== "" && <span className="tab-badge">{t.badge}</span>}
          {t.dot && <span className="tab-dot" />}
        </button>
      ))}
      <span className={`tab-indicator${ind.ready ? " ready" : ""}`} style={{ transform: `translateX(${ind.left}px)`, width: ind.width }} />
    </div>
  );
}

export function Modal({ title, icon: Icon, onClose, size = "md", actions, flush, dismissible = true, children }: {
  title: ReactNode; icon?: LucideIcon; onClose: () => void; size?: "md" | "wide" | "docs";
  actions?: ReactNode; flush?: boolean; dismissible?: boolean; children: ReactNode;
}) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key === "Escape" && dismissible) onClose(); };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onClose, dismissible]);
  return (
    <div className="modal-bg" onMouseDown={(e) => { if (e.target === e.currentTarget && dismissible) onClose(); }}>
      <div className={`modal ${size}`} role="dialog" aria-modal="true">
        <div className="modal-head">
          <div className="modal-title">{Icon && <span className="modal-ico"><Icon size={15} /></span>}<b>{title}</b></div>
          <div className="modal-actions">
            {actions}
            <button className="icon-btn" title="Close" aria-label="Close" disabled={!dismissible} onClick={onClose}><X size={16} /></button>
          </div>
        </div>
        <div className={`modal-body${flush ? " flush" : ""}`}>{children}</div>
      </div>
    </div>
  );
}

/** Animated collapsible region (grid-rows 0fr → 1fr). */
export function Collapse({ open, children }: { open: boolean; children: ReactNode }) {
  return <div className={`collapse${open ? " open" : ""}`}><div className="collapse-inner">{children}</div></div>;
}

export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  const copy = async () => {
    try { await navigator.clipboard.writeText(text); setDone(true); setTimeout(() => setDone(false), 1500); } catch { /* clipboard unavailable */ }
  };
  return (
    <button className="btn sm" disabled={!text} onClick={copy}>
      {done ? <Check size={13} /> : <Copy size={13} />}{done ? "Copied" : label}
    </button>
  );
}

export function Spinner({ size = 14 }: { size?: number }) {
  return <span className="spinner" style={{ width: size, height: size }} />;
}
