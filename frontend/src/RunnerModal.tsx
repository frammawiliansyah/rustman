import { useRef, useState } from "react";
import { api } from "./api";
import type { Environment, HttpRequest, Item } from "./types";

type Row = Record<string, string>;
interface Step { path: string; req: HttpRequest }
interface Result { n: number; iteration: number; path: string; method: string; url: string; status?: number; ms?: number; error?: string }

/** RFC 4180-ish CSV parser (quoted fields, escaped quotes, CRLF). First row is the header. */
export function parseCsv(text: string): Row[] {
  const rows: string[][] = [];
  let row: string[] = [], cur = "", q = false;
  const src = text.replace(/^﻿/, "");
  for (let i = 0; i < src.length; i++) {
    const c = src[i];
    if (q) {
      if (c === '"') { if (src[i + 1] === '"') { cur += '"'; i++; } else q = false; } else cur += c;
    } else if (c === '"') q = true;
    else if (c === ",") { row.push(cur); cur = ""; }
    else if (c === "\n" || c === "\r") {
      if (c === "\r" && src[i + 1] === "\n") i++;
      row.push(cur); cur = ""; rows.push(row); row = [];
    } else cur += c;
  }
  if (cur !== "" || row.length) { row.push(cur); rows.push(row); }
  const [head, ...body] = rows.filter((r) => r.some((v) => v.trim() !== ""));
  if (!head) return [];
  const keys = head.map((h) => h.trim());
  return body.map((r) => Object.fromEntries(keys.map((k, i) => [k, (r[i] ?? "").trim()])));
}

export function parseData(name: string, text: string): Row[] {
  if (name.toLowerCase().endsWith(".json")) {
    const v = JSON.parse(text);
    if (!Array.isArray(v)) throw new Error("JSON iteration data must be an array of objects");
    return v.map((o, i) => {
      if (o === null || typeof o !== "object" || Array.isArray(o)) throw new Error(`Row ${i} is not an object`);
      return Object.fromEntries(Object.entries(o).map(([k, x]) =>
        [k, x === null ? "" : typeof x === "string" ? x : JSON.stringify(x)]));
    });
  }
  return parseCsv(text);
}

/** Substitute {{key}} from the data row (highest priority; env vars are resolved by the backend). */
function applyRow(req: HttpRequest, row: Row): HttpRequest {
  const text = JSON.stringify(req).replace(/\{\{\s*([^{}\s]+)\s*\}\}/g, (m, k: string) =>
    Object.prototype.hasOwnProperty.call(row, k) ? JSON.stringify(row[k]).slice(1, -1) : m);
  return JSON.parse(text);
}

function flatten(items: Item[], path: string[], out: Step[]) {
  for (const it of items) {
    if ("Request" in it) out.push({ path: [...path, it.Request.name].join(" / "), req: it.Request });
    else flatten(it.Folder.items, [...path, it.Folder.name], out);
  }
}

export function RunnerModal(p: { title: string; items: Item[]; environments: Environment[]; envId: string | null; onClose: () => void }) {
  const [envId, setEnvId] = useState<string | null>(p.envId);
  const [iterations, setIterations] = useState(1);
  const [delay, setDelay] = useState(0);
  const [bail, setBail] = useState(false);
  const [data, setData] = useState<{ name: string; rows: Row[] } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [results, setResults] = useState<Result[]>([]);
  const [running, setRunning] = useState(false);
  const [done, setDone] = useState<{ bailed: boolean; stopped: boolean } | null>(null);
  const [total, setTotal] = useState(0);
  const stop = useRef(false);

  const steps: Step[] = []; flatten(p.items, [], steps);

  const onFile = async (f: File | undefined) => {
    if (!f) return;
    try {
      const rows = parseData(f.name, await f.text());
      if (rows.length === 0) throw new Error("No rows found in data file");
      setData({ name: f.name, rows }); setIterations(rows.length); setErr(null);
    } catch (e) { setData(null); setErr(`Invalid data file: ${(e as Error).message}`); }
  };

  const start = async () => {
    setRunning(true); setDone(null); setResults([]); setErr(null); stop.current = false;
    const iters = Math.max(1, Math.floor(iterations) || 1);
    setTotal(iters * steps.length);
    let n = 0, bailed = false, first = true;
    outer: for (let i = 0; i < iters; i++) {
      const row = data ? data.rows[i % data.rows.length] : {};
      for (const s of steps) {
        if (stop.current) break outer;
        if (!first && delay > 0) await new Promise((r) => setTimeout(r, delay));
        first = false;
        const req = applyRow(s.req, row);
        const base = { n: ++n, iteration: i + 1, path: s.path, method: req.method, url: req.url };
        let res: Result;
        try {
          const r = await api.execute(req, envId);
          res = { ...base, status: r.response.status, ms: r.response.elapsed_ms };
        } catch (e) { res = { ...base, error: (e as Error).message }; }
        setResults((rs) => [...rs, res]);
        if (bail && (res.error || (res.status ?? 0) >= 400)) { bailed = true; break outer; }
      }
    }
    setDone({ bailed, stopped: stop.current });
    setRunning(false);
  };

  const failed = (r: Result) => !!r.error || (r.status ?? 0) >= 400;
  const nFailed = results.filter(failed).length;

  return (
    <div className="modal-bg" onClick={() => !running && p.onClose()}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head"><b>Run — {p.title}</b><button className="icon" disabled={running} onClick={p.onClose}>×</button></div>
        <div className="modal-body block">
          <div className="runner-form">
            <label>Environment
              <select value={envId ?? ""} disabled={running} onChange={(e) => setEnvId(e.target.value || null)}>
                <option value="">No environment</option>
                {p.environments.map((e) => <option key={e.id} value={e.id}>{e.name}</option>)}
              </select>
            </label>
            <label>Iterations
              <input type="number" min={1} value={iterations} disabled={running}
                onChange={(e) => setIterations(Number(e.target.value))} />
            </label>
            <label>Delay per request (ms)
              <input type="number" min={0} value={delay} disabled={running}
                onChange={(e) => setDelay(Math.max(0, Number(e.target.value)))} />
            </label>
            <label>Iteration data (.csv / .json)
              <input type="file" accept=".csv,.json" disabled={running} onChange={(e) => onFile(e.target.files?.[0])} />
            </label>
            <label className="chk"><input type="checkbox" checked={bail} disabled={running} onChange={(e) => setBail(e.target.checked)} /> Bail on error</label>
          </div>
          {data && <div className="muted">{data.name}: {data.rows.length} row(s); rows cycle if iterations exceed row count.</div>}
          {err && <div className="error-text">{err}</div>}
          <div className="actions">
            <button className="send" disabled={running || steps.length === 0} onClick={start}>
              {running ? "Running…" : "Start Run"}
            </button>
            {running && <button onClick={() => { stop.current = true; }}>Stop</button>}
            <span className="muted">{steps.length} request(s) × {Math.max(1, Math.floor(iterations) || 1)} iteration(s)</span>
          </div>
          {(running || results.length > 0) && (<>
            <progress className="run-progress" max={total} value={results.length} />
            <table className="cookie-table">
              <thead><tr><th>#</th><th>Iter</th><th>Request</th><th>Status</th><th>Time</th></tr></thead>
              <tbody>{results.map((r) => (
                <tr key={r.n}>
                  <td>{r.n}</td><td>{r.iteration}</td>
                  <td>{r.path}<div className="muted" style={{ padding: 0 }}>{r.method} {r.url}</div></td>
                  <td className={failed(r) ? "bad" : "ok"}>{r.error ? `Error: ${r.error}` : r.status}</td>
                  <td>{r.ms != null ? `${r.ms} ms` : ""}</td>
                </tr>
              ))}</tbody>
            </table>
          </>)}
          {done && (
            <div className="run-summary">
              <b>Summary:</b> <span className="ok">{results.length - nFailed} passed</span>, <span className={nFailed ? "bad" : ""}>{nFailed} failed</span> of {results.length} executed
              {done.bailed && " — stopped early (bail on error)"}{done.stopped && " — stopped by user"}
              <div className="muted" style={{ padding: 0 }}>A request passes when it gets a response with status &lt; 400.</div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
