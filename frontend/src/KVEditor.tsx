import { Plus, X } from "lucide-react";
import type { KV } from "./types";

export function KVEditor({ rows, onChange }: { rows: KV[]; onChange: (rows: KV[]) => void }) {
  const set = (i: number, patch: Partial<KV>) =>
    onChange(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  return (
    <div className="kv">
      {rows.map((r, i) => (
        <div className={`kv-row${r.enabled ? "" : " off"}`} key={i}>
          <input type="checkbox" checked={r.enabled} onChange={(e) => set(i, { enabled: e.target.checked })} />
          <input placeholder="key" value={r.key} onChange={(e) => set(i, { key: e.target.value })} />
          <input placeholder="value" value={r.value} onChange={(e) => set(i, { value: e.target.value })} />
          <button className="icon-btn xs danger" title="Remove" onClick={() => onChange(rows.filter((_, j) => j !== i))}><X size={14} /></button>
        </div>
      ))}
      <button className="btn ghost sm" onClick={() => onChange([...rows, { key: "", value: "", enabled: true }])}><Plus size={13} />Add row</button>
    </div>
  );
}
