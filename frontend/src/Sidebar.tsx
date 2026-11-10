import { useRef, useState } from "react";
import { api } from "./api";
import { DocsModal } from "./DocsModal";
import { RunnerModal } from "./RunnerModal";
import type { Collection, Environment, HistoryEntry, HttpRequest, Item } from "./types";

function Badge({ method }: { method: string }) {
  return <span className={`mbadge m-${method}`}>{method.slice(0, 4)}</span>;
}

function Tree({ items, onOpen, onRun }: { items: Item[]; onOpen: (r: HttpRequest) => void; onRun: (title: string, items: Item[]) => void }) {
  return (
    <ul className="tree">
      {items.map((it, i) =>
        "Request" in it ? (
          <li key={i} className="req" onClick={() => onOpen(it.Request)}>
            <Badge method={it.Request.method} /> <span>{it.Request.name || it.Request.url}</span>
          </li>
        ) : (
          <li key={i}>
            <details open>
              <summary>
                <span>📁 {it.Folder.name}</span>
                <button className="icon" title="Run folder" onClick={(e) => { e.preventDefault(); onRun(it.Folder.name, it.Folder.items); }}>▶ Run</button>
              </summary>
              <Tree items={it.Folder.items} onOpen={onOpen} onRun={onRun} />
            </details>
          </li>
        ),
      )}
    </ul>
  );
}

export function Sidebar(p: {
  collections: Collection[]; history: HistoryEntry[]; environments: Environment[];
  envId: string | null; onEnv: (id: string | null) => void;
  onOpen: (r: HttpRequest) => void; onRefresh: () => void; onError: (m: string) => void;
  onEditEnv: () => void;
}) {
  const [tab, setTab] = useState<"collections" | "history">("collections");
  const [docs, setDocs] = useState<Collection | null>(null);
  const [run, setRun] = useState<{ title: string; items: Item[] } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const kind = useRef<"postman" | "openapi">("postman");

  const pick = (k: "postman" | "openapi") => { kind.current = k; fileRef.current?.click(); };
  const onFile = async (f: File | undefined) => {
    if (!f) return;
    try {
      const raw = await f.text();
      await (kind.current === "postman" ? api.importPostman(raw) : api.importOpenapi(raw));
      p.onRefresh();
    } catch (e) {
      p.onError(`Import failed: ${(e as Error).message}`);
    }
    if (fileRef.current) fileRef.current.value = "";
  };
  const del = async (c: Collection) => {
    if (!confirm(`Delete collection "${c.name}"?`)) return;
    try { await api.deleteCollection(c.id); p.onRefresh(); } catch (e) { p.onError((e as Error).message); }
  };

  return (
    <aside className="sidebar">
      <div className="env">
        <select value={p.envId ?? ""} onChange={(e) => p.onEnv(e.target.value || null)}>
          <option value="">No environment</option>
          {p.environments.map((e) => <option key={e.id} value={e.id}>{e.name}</option>)}
        </select>
        <button className="icon" title="Manage environments" onClick={p.onEditEnv}>⚙</button>
      </div>
      <div className="import">
        <button onClick={() => pick("postman")}>Import Postman</button>
        <button onClick={() => pick("openapi")}>Import OpenAPI</button>
        <input ref={fileRef} type="file" accept=".json,application/json" hidden onChange={(e) => onFile(e.target.files?.[0])} />
      </div>
      <div className="tabs">
        <button className={tab === "collections" ? "active" : ""} onClick={() => setTab("collections")}>Collections</button>
        <button className={tab === "history" ? "active" : ""} onClick={() => setTab("history")}>History</button>
      </div>
      <div className="side-list">
        {tab === "collections" && (p.collections.length === 0
          ? <p className="muted">No collections. Import one to get started.</p>
          : p.collections.map((c) => (
            <details key={c.id} open>
              <summary>
                <span>{c.name}</span>
                <span className="side-actions">
                  <button className="icon" title="Run Collection" onClick={(e) => { e.preventDefault(); setRun({ title: c.name, items: c.items }); }}>▶ Run</button>
                  <button className="icon" title="View Docs" onClick={(e) => { e.preventDefault(); setDocs(c); }}>📖 View Docs</button>
                  <button className="icon" title="Delete" onClick={(e) => { e.preventDefault(); del(c); }}>🗑</button>
                </span>
              </summary>
              <Tree items={c.items} onOpen={p.onOpen} onRun={(title, items) => setRun({ title, items })} />
            </details>
          )))}
        {tab === "history" && (p.history.length === 0
          ? <p className="muted">No history yet.</p>
          : <ul className="tree">{p.history.map((h) => (
            <li key={h.id} className="req" onClick={() => p.onOpen(h.request)}>
              <Badge method={h.method} /> <span className="hurl">{h.url}</span>
              <span className={`hstatus ${h.status < 400 ? "ok" : "bad"}`}>{h.status}</span>
            </li>
          ))}</ul>)}
      </div>
      {docs && <DocsModal collection={docs} onClose={() => setDocs(null)} />}
      {run && <RunnerModal title={run.title} items={run.items} environments={p.environments} envId={p.envId} onClose={() => setRun(null)} />}
    </aside>
  );
}
