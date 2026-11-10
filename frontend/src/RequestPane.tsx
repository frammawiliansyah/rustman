import { useEffect, useRef, useState } from "react";
import { ChevronDown, Cookie, Check, Code2, Send } from "lucide-react";
import { CodeEditor } from "./CodeEditor";
import { KVEditor } from "./KVEditor";
import { Spinner, Tabs } from "./ui";
import type { AuthConfig, HttpRequest, KV, Method, RequestBody } from "./types";

const METHODS: Method[] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
type Tab = "params" | "headers" | "auth" | "body" | "pre" | "tests";

const authKind = (a: AuthConfig) => (a === "None" ? "None" : (Object.keys(a)[0] as "Bearer" | "Basic" | "ApiKey"));
const bodyKind = (b: RequestBody) =>
  b === "None" ? "None" : "Raw" in b ? "JSON" : "FormUrlEncoded" in b ? "FormUrlEncoded" : "FormData";

function MethodPicker({ value, onChange }: { value: Method; onChange: (m: Method) => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const h = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) setOpen(false); };
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", h); document.addEventListener("keydown", k);
    return () => { document.removeEventListener("mousedown", h); document.removeEventListener("keydown", k); };
  }, [open]);
  return (
    <div className="method-picker" ref={ref}>
      <button className={`method-btn m-${value}`} aria-haspopup="listbox" aria-expanded={open} onClick={() => setOpen(!open)}>
        {value}<ChevronDown size={14} className={open ? "flip" : ""} />
      </button>
      {open && (
        <ul className="popover" role="listbox">
          {METHODS.map((m) => (
            <li key={m} role="option" aria-selected={m === value} className={`m-${m}`} onClick={() => { onChange(m); setOpen(false); }}>
              <span>{m}</span>{m === value && <Check size={13} />}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function RequestPane({ req, onChange, onSend, sending, onCode, onCookies }: {
  req: HttpRequest; onChange: (r: HttpRequest) => void; onSend: () => void; sending: boolean; onCode: () => void; onCookies: () => void;
}) {
  const [tab, setTab] = useState<Tab>("params");
  const patch = (p: Partial<HttpRequest>) => onChange({ ...req, ...p });

  const setAuthKind = (k: string) =>
    patch({
      auth:
        k === "Bearer" ? { Bearer: { token: "" } }
        : k === "Basic" ? { Basic: { username: "", password: "" } }
        : k === "ApiKey" ? { ApiKey: { key: "", value: "", in_header: true } }
        : "None",
    });

  const setBodyKind = (k: string) =>
    patch({
      body:
        k === "JSON" ? { Raw: { content_type: "application/json", body: "" } }
        : k === "FormUrlEncoded" ? { FormUrlEncoded: [] }
        : k === "FormData" ? { FormData: [] }
        : "None",
    });

  const a = req.auth;
  const b = req.body;
  const enabledCount = (rows: KV[]) => rows.filter((r) => r.enabled && r.key).length;

  return (
    <div className="pane request">
      <div className="urlbar">
        <div className="url-field">
          <MethodPicker value={req.method} onChange={(method) => patch({ method })} />
          <input
            className="url" placeholder="https://api.example.com/path or {{baseUrl}}/path" value={req.url} spellCheck={false}
            onChange={(e) => patch({ url: e.target.value })}
            onKeyDown={(e) => e.key === "Enter" && !sending && req.url.trim() && onSend()}
          />
        </div>
        <button className="btn primary send" disabled={sending || !req.url.trim()} onClick={onSend}>
          {sending ? <Spinner /> : <Send size={14} />}{sending ? "Sending…" : "Send"}
        </button>
        <button className="btn" title="Generate code snippet" onClick={onCode}><Code2 size={14} />Code</button>
        <button className="btn" title="Manage cookies" onClick={onCookies}><Cookie size={14} />Cookies</button>
      </div>
      <Tabs value={tab} onChange={setTab} items={[
        { key: "params", label: "Params", badge: enabledCount(req.query_params) || "" },
        { key: "headers", label: "Headers", badge: enabledCount(req.headers) || "" },
        { key: "auth", label: "Auth", dot: a !== "None" },
        { key: "body", label: "Body", dot: b !== "None" },
        { key: "pre", label: "Pre-request", dot: !!req.pre_request_script?.trim() },
        { key: "tests", label: "Tests", dot: !!req.test_script?.trim() },
      ]} />
      <div className={`tab-body${tab === "pre" || tab === "tests" ? " fill" : ""}`}>
        {tab === "params" && <KVEditor rows={req.query_params} onChange={(query_params) => patch({ query_params })} />}
        {tab === "headers" && <KVEditor rows={req.headers} onChange={(headers) => patch({ headers })} />}
        {tab === "pre" && (
          <CodeEditor language="javascript" value={req.pre_request_script ?? ""}
            placeholder={'// Runs before the request is sent\npm.environment.set("token", "abc123");'}
            onChange={(v) => patch({ pre_request_script: v || null })} />
        )}
        {tab === "tests" && (
          <CodeEditor language="javascript" value={req.test_script ?? ""}
            placeholder={'// Runs after the response is received\npm.test("Status is 200", () => {\n  pm.expect(pm.response.code).to.equal(200);\n});'}
            onChange={(v) => patch({ test_script: v || null })} />
        )}
        {tab === "auth" && (
          <div className="form">
            <label>Type
              <select value={authKind(a)} onChange={(e) => setAuthKind(e.target.value)}>
                <option>None</option><option>Bearer</option><option>Basic</option><option value="ApiKey">API Key</option>
              </select>
            </label>
            {typeof a === "object" && "Bearer" in a && (
              <label>Token<input value={a.Bearer.token} onChange={(e) => patch({ auth: { Bearer: { token: e.target.value } } })} /></label>
            )}
            {typeof a === "object" && "Basic" in a && (<>
              <label>Username<input value={a.Basic.username} onChange={(e) => patch({ auth: { Basic: { ...a.Basic, username: e.target.value } } })} /></label>
              <label>Password<input type="password" value={a.Basic.password} onChange={(e) => patch({ auth: { Basic: { ...a.Basic, password: e.target.value } } })} /></label>
            </>)}
            {typeof a === "object" && "ApiKey" in a && (<>
              <label>Key<input value={a.ApiKey.key} onChange={(e) => patch({ auth: { ApiKey: { ...a.ApiKey, key: e.target.value } } })} /></label>
              <label>Value<input value={a.ApiKey.value} onChange={(e) => patch({ auth: { ApiKey: { ...a.ApiKey, value: e.target.value } } })} /></label>
              <label>Add to
                <select value={a.ApiKey.in_header ? "header" : "query"} onChange={(e) => patch({ auth: { ApiKey: { ...a.ApiKey, in_header: e.target.value === "header" } } })}>
                  <option value="header">Header</option><option value="query">Query params</option>
                </select>
              </label>
            </>)}
          </div>
        )}
        {tab === "body" && (
          <div className="body-tab">
            <div className="form inline">
              <label>Type
                <select value={bodyKind(b)} onChange={(e) => setBodyKind(e.target.value)}>
                  <option>None</option><option value="JSON">JSON (raw)</option>
                  <option value="FormUrlEncoded">Form URL-encoded</option><option value="FormData">Form data</option>
                </select>
              </label>
            </div>
            {typeof b === "object" && "Raw" in b && (
              <div className="editor-slot">
                <CodeEditor language="json" value={b.Raw.body} placeholder={'{\n  "hello": "world"\n}'}
                  onChange={(v) => patch({ body: { Raw: { ...b.Raw, body: v } } })} />
              </div>
            )}
            {typeof b === "object" && "FormUrlEncoded" in b && (
              <KVEditor rows={b.FormUrlEncoded} onChange={(rows) => patch({ body: { FormUrlEncoded: rows } })} />
            )}
            {typeof b === "object" && "FormData" in b && (
              <KVEditor rows={b.FormData} onChange={(rows) => patch({ body: { FormData: rows } })} />
            )}
          </div>
        )}
      </div>
    </div>
  );
}
