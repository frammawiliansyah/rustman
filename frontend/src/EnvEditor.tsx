import { useState } from "react";
import { Globe, Plus, Save, Trash2, X } from "lucide-react";
import { api } from "./api";
import { Modal } from "./ui";
import type { Environment } from "./types";

export function EnvEditor({ environments, onClose, onChanged }: {
  environments: Environment[]; onClose: () => void; onChanged: () => void;
}) {
  const [sel, setSel] = useState<string | null>(environments[0]?.id ?? null);
  const [draft, setDraft] = useState<Environment | null>(environments[0] ?? null);
  const [err, setErr] = useState<string | null>(null);

  const pick = (id: string) => { setSel(id); setDraft(environments.find((e) => e.id === id) ?? null); };
  const create = () => {
    const e: Environment = { id: crypto.randomUUID(), name: "New environment", variables: [] };
    setSel(e.id); setDraft(e);
  };
  const run = async (fn: () => Promise<unknown>) => {
    try { setErr(null); await fn(); onChanged(); } catch (e) { setErr((e as Error).message); }
  };
  const setVar = (i: number, patch: Partial<Environment["variables"][number]>) =>
    setDraft((d) => d && { ...d, variables: d.variables.map((v, j) => (j === i ? { ...v, ...patch } : v)) });

  return (
    <Modal title="Environments" icon={Globe} onClose={onClose}>
      <div className="split">
        <div className="env-list">
          {environments.map((e) => (
            <div key={e.id} className={e.id === sel ? "sel" : ""} onClick={() => pick(e.id)}>{e.name}</div>
          ))}
          {draft && !environments.some((e) => e.id === draft.id) && <div className="sel">{draft.name}</div>}
          <button className="btn ghost sm" onClick={create}><Plus size={13} />New</button>
        </div>
        {draft && (
          <div className="env-form">
            <input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
            {draft.variables.map((v, i) => (
              <div className={`kv-row${v.enabled ? "" : " off"}`} key={i}>
                <input type="checkbox" checked={v.enabled} onChange={(e) => setVar(i, { enabled: e.target.checked })} />
                <input placeholder="variable" value={v.key} onChange={(e) => setVar(i, { key: e.target.value })} />
                <input placeholder="value" type={v.secret ? "password" : "text"} value={v.value} onChange={(e) => setVar(i, { value: e.target.value })} />
                <button className="icon-btn xs danger" onClick={() => setDraft({ ...draft, variables: draft.variables.filter((_, j) => j !== i) })}><X size={14} /></button>
              </div>
            ))}
            <button className="btn ghost sm" onClick={() => setDraft({ ...draft, variables: [...draft.variables, { key: "", value: "", enabled: true, secret: false }] })}><Plus size={13} />Add variable</button>
            {err && <div className="error-text">{err}</div>}
            <div className="actions">
              <button className="btn primary" onClick={() => run(() => api.saveEnvironment(draft))}><Save size={14} />Save</button>
              <button className="btn danger" disabled={!environments.some((e) => e.id === draft.id)}
                onClick={() => run(async () => { await api.deleteEnvironment(draft.id); setDraft(null); setSel(null); })}><Trash2 size={14} />Delete</button>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
