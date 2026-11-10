import { useState } from "react";
import { JsonView } from "./JsonView";
import type { ExecuteReply } from "./types";

function fmtSize(n: number) {
  return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`;
}

export function ResponsePane({ reply, error, loading }: { reply: ExecuteReply | null; error: string | null; loading: boolean }) {
  const [tab, setTab] = useState<"body" | "headers" | "timing">("body");
  if (loading) return <div className="pane empty">Sending…</div>;
  if (error) return <div className="pane error">{error}</div>;
  if (!reply) return <div className="pane empty">Send a request to see the response.</div>;

  const { response: r, timing: t } = reply;
  const ok = r.status >= 200 && r.status < 400;
  const size = new TextEncoder().encode(r.body).length;
  const segs = [
    ["DNS", t.dns_ms], ["TCP", t.tcp_ms], ["TLS", t.tls_ms], ["TTFB", t.ttfb_ms], ["Download", t.download_ms],
  ] as const;
  const max = Math.max(t.total_ms, 1);

  return (
    <div className="pane">
      <div className="resp-meta">
        <span className={`badge ${ok ? "ok" : "bad"}`}>{r.status}</span>
        <span>{r.elapsed_ms} ms</span>
        <span>{fmtSize(size)}</span>
      </div>
      <div className="tabs">
        {(["body", "headers", "timing"] as const).map((k) => (
          <button key={k} className={tab === k ? "active" : ""} onClick={() => setTab(k)}>
            {k === "headers" ? `Headers (${r.headers.length})` : k[0].toUpperCase() + k.slice(1)}
          </button>
        ))}
      </div>
      <div className="tab-body">
        {tab === "body" && <JsonView text={r.body} />}
        {tab === "headers" && (
          <table className="headers">
            <tbody>
              {r.headers.map((h, i) => (
                <tr key={i}><td className="hk">{h.key}</td><td>{h.value}</td></tr>
              ))}
            </tbody>
          </table>
        )}
        {tab === "timing" && (
          <div className="waterfall">
            {segs.map(([label, ms]) => (
              <div className="wf-row" key={label}>
                <span className="wf-label">{label}</span>
                <div className="wf-track">
                  {ms != null && <div className={`wf-bar wf-${label.toLowerCase()}`} style={{ width: `${Math.max((ms / max) * 100, 0.5)}%` }} />}
                </div>
                <span className="wf-ms">{ms == null ? "n/a" : `${ms} ms`}</span>
              </div>
            ))}
            <div className="wf-row total">
              <span className="wf-label">Total</span>
              <div className="wf-track"><div className="wf-bar wf-total" style={{ width: "100%" }} /></div>
              <span className="wf-ms">{t.total_ms} ms</span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
