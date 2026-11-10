import { useState } from "react";
import { api } from "./api";
import { JsonView } from "./JsonView";
import { KVEditor } from "./KVEditor";
import type { GqlSchema, HttpRequest, KV } from "./types";

const DEFAULT_QUERY = "query {\n  __typename\n}";

function SchemaViewer({ schema, onClose }: { schema: GqlSchema; onClose: () => void }) {
  const list = (title: string, fields: GqlSchema["queries"]) => (
    <section>
      <h4>{title} ({fields.length})</h4>
      {fields.length === 0 ? <div className="muted">None</div> : (
        <ul className="schema-list">{fields.map((f) => (
          <li key={f.name}><code>{f.name}</code>{f.type && <span className="muted">: {f.type.name ?? f.type.kind}</span>}</li>
        ))}</ul>
      )}
    </section>
  );
  const types = schema.types.filter((t) => !t.name.startsWith("__"));
  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head"><b>GraphQL Schema</b><button className="icon" onClick={onClose}>×</button></div>
        <div className="modal-body block">
          {list("Queries", schema.queries)}
          {list("Mutations", schema.mutations)}
          <section>
            <h4>Types ({types.length})</h4>
            {types.map((t) => (
              <details key={t.name}>
                <summary><code>{t.name}</code> <span className="muted">{t.kind}</span></summary>
                {t.description && <div className="muted">{t.description}</div>}
                <ul className="schema-list">{(t.fields ?? []).map((f) => (
                  <li key={f.name}><code>{f.name}</code>{f.type && <span className="muted">: {f.type.name ?? f.type.kind}</span>}</li>
                ))}</ul>
              </details>
            ))}
          </section>
        </div>
      </div>
    </div>
  );
}

export function GraphqlPane({ envId }: { envId: string | null }) {
  const [url, setUrl] = useState("https://");
  const [headers, setHeaders] = useState<KV[]>([]);
  const [query, setQuery] = useState(DEFAULT_QUERY);
  const [vars, setVars] = useState("{}");
  const [tab, setTab] = useState<"variables" | "headers">("variables");
  const [body, setBody] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [schema, setSchema] = useState<GqlSchema | null>(null);

  const valid = /^https?:\/\/.+|^\{\{.+\}\}/i.test(url.trim());

  const run = async () => {
    setError(null);
    let variables: unknown = undefined;
    if (vars.trim()) {
      try { variables = JSON.parse(vars); } catch (e) { setError(`Variables are not valid JSON: ${(e as Error).message}`); return; }
    }
    // Same payload GraphQLRequest::to_http_request builds: POST + JSON {query, variables}.
    const req: HttpRequest = {
      id: crypto.randomUUID(), name: "GraphQL", method: "POST", url: url.trim(),
      headers: [{ key: "Content-Type", value: "application/json", enabled: true }, ...headers],
      query_params: [], auth: "None",
      body: { Raw: { content_type: "application/json", body: JSON.stringify({ query, variables }) } },
    };
    setBusy(true);
    try {
      const r = await api.execute(req, envId);
      setStatus(`${r.response.status} · ${r.response.elapsed_ms} ms`);
      setBody(r.response.body);
    } catch (e) {
      setBody(null); setStatus(null); setError((e as Error).message);
    } finally { setBusy(false); }
  };

  const introspect = async () => {
    setError(null); setBusy(true);
    try { setSchema(await api.graphqlIntrospect(url.trim(), headers)); }
    catch (e) { setError(`Introspection failed: ${(e as Error).message}`); }
    finally { setBusy(false); }
  };

  let result: { data?: unknown; errors?: unknown } | null = null;
  if (body) { try { result = JSON.parse(body); } catch { /* raw */ } }

  return (
    <div className="gql">
      <div className="urlbar">
        <input className="url" value={url} placeholder="GraphQL endpoint URL" onChange={(e) => setUrl(e.target.value)} />
        <button disabled={!valid || busy} onClick={introspect}>Introspect Schema</button>
        <button className="send" disabled={!valid || busy || !query.trim()} onClick={run}>{busy ? "Running…" : "Run Query"}</button>
      </div>
      {error && <div className="error-text">{error}</div>}
      <div className="gql-grid">
        <div className="gql-left">
          <label className="muted">Query / Mutation</label>
          <textarea className="editor gql-query" spellCheck={false} value={query} onChange={(e) => setQuery(e.target.value)} />
          <div className="tabs">
            <button className={tab === "variables" ? "active" : ""} onClick={() => setTab("variables")}>Query Variables</button>
            <button className={tab === "headers" ? "active" : ""} onClick={() => setTab("headers")}>Headers ({headers.filter((h) => h.enabled && h.key).length})</button>
          </div>
          <div className="tab-body">
            {tab === "variables"
              ? <textarea className="editor" spellCheck={false} value={vars} onChange={(e) => setVars(e.target.value)} />
              : <KVEditor rows={headers} onChange={setHeaders} />}
          </div>
        </div>
        <div className="gql-right">
          <div className="resp-meta"><b>Response</b>{status && <span>{status}</span>}</div>
          <div className="tab-body">
            {body === null ? <div className="muted">Run a query to see the response.</div>
              : result ? (<>
                {result.errors != null && <><h4 className="bad">errors</h4><JsonView text={JSON.stringify(result.errors)} /></>}
                {result.data !== undefined && <><h4>data</h4><JsonView text={JSON.stringify(result.data)} /></>}
              </>) : <JsonView text={body} />}
          </div>
        </div>
      </div>
      {schema && <SchemaViewer schema={schema} onClose={() => setSchema(null)} />}
    </div>
  );
}
