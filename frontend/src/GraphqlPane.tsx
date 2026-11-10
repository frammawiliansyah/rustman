import { useState } from "react";
import { api } from "./api";
import { Database, Play, Terminal } from "lucide-react";
import { CodeEditor } from "./CodeEditor";
import { Modal, Spinner, Tabs } from "./ui";
import { KVEditor } from "./KVEditor";
import type { GqlSchema, HttpRequest, KV } from "./types";

const DEFAULT_QUERY = "query {\n  __typename\n}";

function SchemaViewer({ schema, onClose }: { schema: GqlSchema; onClose: () => void }) {
  const list = (title: string, fields: GqlSchema["queries"]) => (
    <section>
      <h4>{title} ({fields.length})</h4>
      {fields.length === 0 ? <div className="hint">None</div> : (
        <ul className="schema-list">{fields.map((f) => (
          <li key={f.name}><code>{f.name}</code>{f.type && <span className="muted">: {f.type.name ?? f.type.kind}</span>}</li>
        ))}</ul>
      )}
    </section>
  );
  const types = schema.types.filter((t) => !t.name.startsWith("__"));
  return (
    <Modal title="GraphQL Schema" icon={Database} size="wide" onClose={onClose}>
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
    </Modal>
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

  const pretty = (v: unknown) => JSON.stringify(v, null, 2);
  let display = "";
  if (body !== null) {
    display = result
      ? pretty({ ...(result.errors != null ? { errors: result.errors } : {}), ...(result.data !== undefined ? { data: result.data } : {}) })
      : body;
  }

  return (
    <div className="gql">
      <div className="urlbar">
        <div className="url-field">
          <span className="url-tag"><Database size={13} />GraphQL</span>
          <input className="url" value={url} spellCheck={false} placeholder="GraphQL endpoint URL" onChange={(e) => setUrl(e.target.value)} />
        </div>
        <button className="btn" disabled={!valid || busy} onClick={introspect}><Database size={14} />Introspect</button>
        <button className="btn primary send" disabled={!valid || busy || !query.trim()} onClick={run}>
          {busy ? <Spinner /> : <Play size={14} />}{busy ? "Running…" : "Run Query"}
        </button>
      </div>
      {error && <div className="error-text">{error}</div>}
      <div className="gql-grid">
        <div className="gql-left">
          <div className="section-label"><Terminal size={13} />Query / Mutation</div>
          <div className="gql-query"><CodeEditor language="graphql" value={query} onChange={setQuery} /></div>
          <Tabs value={tab} onChange={setTab} items={[
            { key: "variables", label: "Query Variables" },
            { key: "headers", label: "Headers", badge: headers.filter((h) => h.enabled && h.key).length || "" },
          ]} />
          <div className={`tab-body${tab === "variables" ? " fill" : ""}`}>
            {tab === "variables"
              ? <CodeEditor language="json" value={vars} onChange={setVars} />
              : <KVEditor rows={headers} onChange={setHeaders} />}
          </div>
        </div>
        <div className="gql-right">
          <div className="resp-meta"><span className="section-label">Response</span>{status && <span className="chip ok"><i className="pulse" />{status}</span>}</div>
          <div className="tab-body fill">
            {body === null
              ? <div className="empty-note"><Terminal size={22} /><p>Run a query to see the response.</p></div>
              : <CodeEditor language="json" value={display} readOnly />}
          </div>
        </div>
      </div>
      {schema && <SchemaViewer schema={schema} onClose={() => setSchema(null)} />}
    </div>
  );
}
