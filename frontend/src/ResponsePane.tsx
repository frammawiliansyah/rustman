import { useState } from "react";
import { AlertTriangle, Clock, HardDrive, Radio, Send } from "lucide-react";
import { CodeEditor } from "./CodeEditor";
import { CopyButton, Tabs } from "./ui";
import type { ExecuteReply, KV } from "./types";

function fmtSize(n: number) {
  return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;
}

const STATUS_TEXT: Record<number, string> = {
  200: "OK", 201: "Created", 202: "Accepted", 204: "No Content", 301: "Moved Permanently", 302: "Found", 304: "Not Modified",
  400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed", 409: "Conflict",
  422: "Unprocessable Entity", 429: "Too Many Requests", 500: "Internal Server Error", 502: "Bad Gateway",
  503: "Service Unavailable", 504: "Gateway Timeout",
};
const statusClass = (s: number) => (s < 300 ? "ok" : s < 400 ? "warn" : "bad");

function detect(body: string, headers: KV[]): { pretty: string; lang: string } {
  try { return { pretty: JSON.stringify(JSON.parse(body), null, 2), lang: "json" }; } catch { /* not JSON */ }
  const ct = headers.find((h) => h.key.toLowerCase() === "content-type")?.value.toLowerCase() ?? "";
  if (ct.includes("html") || /^\s*<!doctype html|^\s*<html/i.test(body)) return { pretty: body, lang: "html" };
  if (ct.includes("xml") || /^\s*<\?xml/.test(body)) return { pretty: body, lang: "xml" };
  if (ct.includes("javascript")) return { pretty: body, lang: "javascript" };
  if (ct.includes("css")) return { pretty: body, lang: "css" };
  return { pretty: body, lang: "plaintext" };
}

const SEGMENTS = [
  ["DNS", "dns_ms"], ["TCP", "tcp_ms"], ["TLS", "tls_ms"], ["TTFB", "ttfb_ms"], ["Download", "download_ms"],
] as const;

function Waterfall({ t }: { t: ExecuteReply["timing"] }) {
  // Phases run sequentially, so each bar starts where the previous one ended.
  const known = SEGMENTS.reduce((s, [, k]) => s + (t[k] ?? 0), 0);
  const scale = Math.max(t.total_ms, known, 1);
  let offset = 0;
  const rows = SEGMENTS.map(([label, k]) => {
    const ms = t[k];
    const left = offset;
    offset += ms ?? 0;
    return { label, ms, left: (left / scale) * 100, width: ms == null ? 0 : Math.max((ms / scale) * 100, 0.8) };
  });
  return (
    <div className="waterfall">
      {rows.map((r) => (
        <div className="wf-row" key={r.label}>
          <span className="wf-label"><i className={`wf-dot wf-${r.label.toLowerCase()}`} />{r.label}</span>
          <div className="wf-track">
            {r.ms != null && <div className={`wf-bar wf-${r.label.toLowerCase()}`} style={{ left: `${r.left}%`, width: `${r.width}%` }} />}
          </div>
          <span className="wf-ms">{r.ms == null ? "n/a" : `${r.ms} ms`}</span>
        </div>
      ))}
      <div className="wf-row total">
        <span className="wf-label"><i className="wf-dot wf-total" />Total</span>
        <div className="wf-track"><div className="wf-bar wf-total" style={{ left: 0, width: "100%" }} /></div>
        <span className="wf-ms">{t.total_ms} ms</span>
      </div>
    </div>
  );
}

export function ResponsePane({ reply, error, loading }: { reply: ExecuteReply | null; error: string | null; loading: boolean }) {
  const [tab, setTab] = useState<"formatted" | "raw" | "headers" | "timing">("formatted");

  if (loading) {
    return (
      <div className="pane response">
        <div className="resp-meta"><span className="chip pending"><span className="spinner" />Sending…</span></div>
        <div className="skeleton">
          {[92, 64, 78, 40, 85, 55, 70].map((w, i) => <div key={i} className="sk-line" style={{ width: `${w}%`, animationDelay: `${i * 90}ms` }} />)}
        </div>
      </div>
    );
  }
  if (error) {
    return (
      <div className="pane response empty">
        <div className="state-card err"><AlertTriangle size={22} /><b>Request failed</b><pre>{error}</pre></div>
      </div>
    );
  }
  if (!reply) {
    return (
      <div className="pane response empty">
        <div className="state-card"><span className="state-ico"><Radio size={22} /></span><b>No response yet</b>
          <span>Hit <kbd>Send</kbd> <Send size={11} /> or press <kbd>Enter</kbd> in the URL bar.</span></div>
      </div>
    );
  }

  const { response: r, timing: t } = reply;
  const size = new TextEncoder().encode(r.body).length;
  const { pretty, lang } = detect(r.body, r.headers);
  const cls = statusClass(r.status);

  return (
    <div className="pane response">
      <div className="resp-meta">
        <span className={`chip status ${cls}`}><i className="pulse" />{r.status} {STATUS_TEXT[r.status] ?? ""}</span>
        <span className="chip"><Clock size={12} />{r.elapsed_ms} ms</span>
        <span className="chip"><HardDrive size={12} />{fmtSize(size)}</span>
        <span className="spacer" />
        <CopyButton text={tab === "raw" ? r.body : pretty} />
      </div>
      <Tabs value={tab} onChange={setTab} items={[
        { key: "formatted", label: "Formatted" }, { key: "raw", label: "Raw" },
        { key: "headers", label: "Headers", badge: r.headers.length }, { key: "timing", label: "Timing" },
      ]} />
      <div className={`tab-body${tab === "formatted" || tab === "raw" ? " fill" : ""}`}>
        {tab === "formatted" && <CodeEditor key="f" language={lang} value={pretty} readOnly />}
        {tab === "raw" && <CodeEditor key="r" language="plaintext" value={r.body} readOnly wrap />}
        {tab === "headers" && (
          <table className="headers">
            <tbody>{r.headers.map((h, i) => <tr key={i}><td className="hk">{h.key}</td><td>{h.value}</td></tr>)}</tbody>
          </table>
        )}
        {tab === "timing" && <Waterfall t={t} />}
      </div>
    </div>
  );
}
