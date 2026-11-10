import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { Cookie } from "./types";

const empty = (): Cookie => ({ name: "", value: "", domain: "", path: "/", http_only: false, secure: false });

export function CookieManager({ onClose }: { onClose: () => void }) {
  const [cookies, setCookies] = useState<Cookie[]>([]);
  const [draft, setDraft] = useState<Cookie>(empty);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => api.cookies().then(setCookies).catch((e) => setErr((e as Error).message)), []);
  useEffect(() => { load(); }, [load]);

  const add = async () => {
    setErr(null);
    try { await api.saveCookie(draft); setDraft(empty()); await load(); }
    catch (e) { setErr((e as Error).message); }
  };
  const remove = async (c: Cookie) => {
    setErr(null);
    try { await api.deleteCookie(c.domain, c.name); await load(); }
    catch (e) { setErr((e as Error).message); }
  };
  const set = (p: Partial<Cookie>) => setDraft({ ...draft, ...p });

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head"><b>Cookies</b><button className="icon" onClick={onClose}>×</button></div>
        <div className="modal-body block">
          {cookies.length === 0 ? <div className="muted">No cookies stored.</div> : (
            <table className="cookie-table">
              <thead><tr><th>Domain</th><th>Name</th><th>Value</th><th>Path</th><th>Flags</th><th /></tr></thead>
              <tbody>
                {cookies.map((c) => (
                  <tr key={`${c.domain}|${c.path}|${c.name}`}>
                    <td>{c.domain}</td><td>{c.name}</td><td>{c.value}</td><td>{c.path}</td>
                    <td>{[c.secure && "Secure", c.http_only && "HttpOnly"].filter(Boolean).join(", ")}</td>
                    <td><button className="icon danger" title="Delete" onClick={() => remove(c)}>×</button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div className="cookie-form">
            <input placeholder="domain (example.com)" value={draft.domain} onChange={(e) => set({ domain: e.target.value })} />
            <input placeholder="name" value={draft.name} onChange={(e) => set({ name: e.target.value })} />
            <input placeholder="value" value={draft.value} onChange={(e) => set({ value: e.target.value })} />
            <input placeholder="path" value={draft.path} onChange={(e) => set({ path: e.target.value })} />
            <label className="chk"><input type="checkbox" checked={draft.secure} onChange={(e) => set({ secure: e.target.checked })} />Secure</label>
            <label className="chk"><input type="checkbox" checked={draft.http_only} onChange={(e) => set({ http_only: e.target.checked })} />HttpOnly</label>
            <button disabled={!draft.name.trim() || !draft.domain.trim()} onClick={add}>Add cookie</button>
          </div>
          {err && <div className="error-text">{err}</div>}
        </div>
      </div>
    </div>
  );
}
