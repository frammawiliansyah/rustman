import { useState } from "react";
import { KVEditor } from "./KVEditor";
import type { AuthConfig, HttpRequest, KV, Method, RequestBody } from "./types";

const METHODS: Method[] = ["GET", "POST", "PUT", "DELETE", "PATCH"];
type Tab = "params" | "headers" | "auth" | "body";

const authKind = (a: AuthConfig) => (a === "None" ? "None" : (Object.keys(a)[0] as "Bearer" | "Basic" | "ApiKey"));
const bodyKind = (b: RequestBody) =>
  b === "None" ? "None" : "Raw" in b ? "JSON" : "FormUrlEncoded" in b ? "FormUrlEncoded" : "FormData";

export function RequestPane({ req, onChange, onSend, sending }: {
  req: HttpRequest; onChange: (r: HttpRequest) => void; onSend: () => void; sending: boolean;
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
    <div className="pane">
      <div className="urlbar">
        <select className={`method m-${req.method}`} value={req.method} onChange={(e) => patch({ method: e.target.value as Method })}>
          {METHODS.map((m) => <option key={m}>{m}</option>)}
        </select>
        <input
          className="url" placeholder="https://api.example.com/path or {{baseUrl}}/path" value={req.url}
          onChange={(e) => patch({ url: e.target.value })}
          onKeyDown={(e) => e.key === "Enter" && onSend()}
        />
        <button className="send" disabled={sending || !req.url.trim()} onClick={onSend}>{sending ? "Sending…" : "Send"}</button>
      </div>
      <div className="tabs">
        {([
          ["params", `Params${enabledCount(req.query_params) ? ` (${enabledCount(req.query_params)})` : ""}`],
          ["headers", `Headers${enabledCount(req.headers) ? ` (${enabledCount(req.headers)})` : ""}`],
          ["auth", "Auth"],
          ["body", "Body"],
        ] as [Tab, string][]).map(([k, label]) => (
          <button key={k} className={tab === k ? "active" : ""} onClick={() => setTab(k)}>{label}</button>
        ))}
      </div>
      <div className="tab-body">
        {tab === "params" && <KVEditor rows={req.query_params} onChange={(query_params) => patch({ query_params })} />}
        {tab === "headers" && <KVEditor rows={req.headers} onChange={(headers) => patch({ headers })} />}
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
          <div className="form">
            <label>Type
              <select value={bodyKind(b)} onChange={(e) => setBodyKind(e.target.value)}>
                <option>None</option><option value="JSON">JSON (raw)</option>
                <option value="FormUrlEncoded">Form URL-encoded</option><option value="FormData">Form data</option>
              </select>
            </label>
            {typeof b === "object" && "Raw" in b && (
              <textarea className="editor" spellCheck={false} value={b.Raw.body} placeholder='{"hello": "world"}'
                onChange={(e) => patch({ body: { Raw: { ...b.Raw, body: e.target.value } } })} />
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
