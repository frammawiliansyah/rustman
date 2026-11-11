import { useMemo, useRef, useState } from "react";
import {
  Bot, BookOpen, ChevronDown, ChevronRight, FileCode, Folder, FolderOpen, History as HistoryIcon,
  Layers, Play, Search, Settings, Trash2, Upload, X,
} from "lucide-react";
import { api } from "./api";
import { DocsModal } from "./DocsModal";
import { RunnerModal } from "./RunnerModal";
import { Collapse, MethodBadge, Tabs } from "./ui";
import type { Collection, Environment, HistoryEntry, HttpRequest, Item } from "./types";

/** Keep only items whose name/url/method match, preserving folders that contain matches. */
function filterItems(items: Item[], q: string): Item[] {
  if (!q) return items;
  const out: Item[] = [];
  for (const it of items) {
    if ("Request" in it) {
      const r = it.Request;
      if (`${r.name} ${r.url} ${r.method}`.toLowerCase().includes(q)) out.push(it);
    } else {
      const kids = filterItems(it.Folder.items, q);
      if (kids.length || it.Folder.name.toLowerCase().includes(q)) out.push({ Folder: { ...it.Folder, items: it.Folder.name.toLowerCase().includes(q) && !kids.length ? it.Folder.items : kids } });
    }
  }
  return out;
}

function FolderNode({ folder, forceOpen, onOpen, onRun }: {
  folder: { name: string; items: Item[] }; forceOpen: boolean;
  onOpen: (r: HttpRequest) => void; onRun: (title: string, items: Item[]) => void;
}) {
  const [open, setOpen] = useState(true);
  const isOpen = open || forceOpen;
  return (
    <li>
      <div className="node folder" onClick={() => setOpen(!open)}>
        {isOpen ? <ChevronDown size={14} className="chev" /> : <ChevronRight size={14} className="chev" />}
        {isOpen ? <FolderOpen size={14} className="ico-folder" /> : <Folder size={14} className="ico-folder" />}
        <span className="node-name">{folder.name}</span>
        <span className="node-actions">
          <button className="icon-btn xs" title="Run folder" onClick={(e) => { e.stopPropagation(); onRun(folder.name, folder.items); }}><Play size={12} /></button>
        </span>
      </div>
      <Collapse open={isOpen}><Tree items={folder.items} forceOpen={forceOpen} onOpen={onOpen} onRun={onRun} /></Collapse>
    </li>
  );
}

function Tree({ items, forceOpen, onOpen, onRun }: {
  items: Item[]; forceOpen: boolean; onOpen: (r: HttpRequest) => void; onRun: (title: string, items: Item[]) => void;
}) {
  return (
    <ul className="tree">
      {items.map((it, i) =>
        "Request" in it ? (
          <li key={i}>
            <div className="node req" onClick={() => onOpen(it.Request)}>
              <MethodBadge method={it.Request.method} />
              <span className="node-name">{it.Request.name || it.Request.url}</span>
            </div>
          </li>
        ) : <FolderNode key={i} folder={it.Folder} forceOpen={forceOpen} onOpen={onOpen} onRun={onRun} />,
      )}
    </ul>
  );
}

function CollectionNode({ c, forceOpen, onOpen, onRun, onDocs, onDelete }: {
  c: Collection; forceOpen: boolean; onOpen: (r: HttpRequest) => void;
  onRun: (title: string, items: Item[]) => void; onDocs: () => void; onDelete: () => void;
}) {
  const [open, setOpen] = useState(true);
  const isOpen = open || forceOpen;
  return (
    <div className="collection">
      <div className="node coll" onClick={() => setOpen(!open)}>
        {isOpen ? <ChevronDown size={14} className="chev" /> : <ChevronRight size={14} className="chev" />}
        <Layers size={14} className="ico-coll" />
        <span className="node-name">{c.name}</span>
        <span className="node-actions">
          <button className="icon-btn xs" title="Run collection" onClick={(e) => { e.stopPropagation(); onRun(c.name, c.items); }}><Play size={12} /></button>
          <button className="icon-btn xs" title="View docs" onClick={(e) => { e.stopPropagation(); onDocs(); }}><BookOpen size={12} /></button>
          <button className="icon-btn xs danger" title="Delete collection" onClick={(e) => { e.stopPropagation(); onDelete(); }}><Trash2 size={12} /></button>
        </span>
      </div>
      <Collapse open={isOpen}><Tree items={c.items} forceOpen={forceOpen} onOpen={onOpen} onRun={onRun} /></Collapse>
    </div>
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
  const [search, setSearch] = useState("");
  const fileRef = useRef<HTMLInputElement>(null);
  const kind = useRef<"postman" | "openapi">("postman");

  const q = search.trim().toLowerCase();
  const shown = useMemo(
    () => p.collections.map((c) => ({ ...c, items: filterItems(c.items, q) })).filter((c) => !q || c.items.length > 0),
    [p.collections, q],
  );
  const hist = useMemo(
    () => (q ? p.history.filter((h) => `${h.method} ${h.url}`.toLowerCase().includes(q)) : p.history),
    [p.history, q],
  );

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
  const original = (id: string) => p.collections.find((c) => c.id === id)!;

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-logo">
          <svg viewBox="12 25 1000 1000" width="24" height="24" aria-hidden="true">
            <g fill="none" stroke="currentColor" strokeWidth="40" strokeLinecap="round" strokeLinejoin="round">
              <path d="M352 612L262 664L236 746M672 612L762 664L788 746M372 656L308 728L300 800M652 656L716 728L724 800" />
              <path d="M356 540C290 520 262 470 270 400M668 540C734 520 762 470 754 400" strokeWidth="50" />
            </g>
            <g fill="currentColor">
              <circle cx="268" cy="330" r="82" /><circle cx="756" cy="330" r="82" />
              <ellipse cx="512" cy="580" rx="204" ry="136" />
            </g>
            <g fill="var(--accent)"><polygon points="268,336 226,236 310,236" /><polygon points="756,336 714,236 798,236" /></g>
          </svg>
        </span>
        <b>Rustman</b>
      </div>
      <div className="env">
        <select value={p.envId ?? ""} onChange={(e) => p.onEnv(e.target.value || null)} aria-label="Environment">
          <option value="">No environment</option>
          {p.environments.map((e) => <option key={e.id} value={e.id}>{e.name}</option>)}
        </select>
        <button className="icon-btn" title="Manage environments" onClick={p.onEditEnv}><Settings size={15} /></button>
      </div>
      <div className="search">
        <Search size={14} />
        <input placeholder="Search collections…" value={search} onChange={(e) => setSearch(e.target.value)} />
        {search && <button className="icon-btn xs" title="Clear" onClick={() => setSearch("")}><X size={12} /></button>}
      </div>
      <Tabs variant="pill" className="side-tabs" value={tab} onChange={setTab} items={[
        { key: "collections", label: "Collections", icon: Layers },
        { key: "history", label: "History", icon: HistoryIcon },
      ]} />
      <div className="side-list">
        {tab === "collections" && (shown.length === 0
          ? <div className="empty-note"><FileCode size={22} /><p>{q ? "No matches." : "No collections yet."}</p>{!q && <span>Import one below to get started.</span>}</div>
          : shown.map((c) => (
            <CollectionNode key={c.id} c={c} forceOpen={!!q} onOpen={p.onOpen}
              onRun={(title, items) => setRun({ title, items })}
              onDocs={() => setDocs(original(c.id))} onDelete={() => del(original(c.id))} />
          )))}
        {tab === "history" && (hist.length === 0
          ? <div className="empty-note"><HistoryIcon size={22} /><p>{q ? "No matches." : "No history yet."}</p></div>
          : <ul className="tree flat">{hist.map((h) => (
            <li key={h.id}>
              <div className="node req" onClick={() => p.onOpen(h.request)}>
                <MethodBadge method={h.method} />
                {h.source && <span className="agent-mark" title={`Sent by ${h.source.replace("agent:", "")}`}><Bot size={12} /></span>}
                <span className="node-name hurl">{h.url}</span>
                <span className={`hstatus ${h.status < 400 ? "ok" : "bad"}`}>{h.status}</span>
              </div>
            </li>
          ))}</ul>)}
      </div>
      <div className="import">
        <button className="btn sm" onClick={() => pick("postman")}><Upload size={13} />Postman</button>
        <button className="btn sm" onClick={() => pick("openapi")}><Upload size={13} />OpenAPI</button>
        <input ref={fileRef} type="file" accept=".json,application/json" hidden onChange={(e) => onFile(e.target.files?.[0])} />
      </div>
      {docs && <DocsModal collection={docs} onClose={() => setDocs(null)} />}
      {run && <RunnerModal title={run.title} items={run.items} environments={p.environments} envId={p.envId} onClose={() => setRun(null)} />}
    </aside>
  );
}
