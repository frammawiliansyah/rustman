import { useCallback, useEffect, useState } from "react";
import { Globe, Plus, Radio, Terminal, Zap } from "lucide-react";
import { api } from "./api";
import { CodeModal } from "./CodeModal";
import { CookieManager } from "./CookieManager";
import { GraphqlPane } from "./GraphqlPane";
import { RealtimePane } from "./RealtimePane";
import { EnvEditor } from "./EnvEditor";
import { RequestPane } from "./RequestPane";
import { ResponsePane } from "./ResponsePane";
import { Sidebar } from "./Sidebar";
import { Tabs } from "./ui";
import type { Collection, Environment, ExecuteReply, HistoryEntry, HttpRequest } from "./types";

const blank = (): HttpRequest => ({
  id: crypto.randomUUID(), name: "Untitled request", method: "GET", url: "",
  headers: [], query_params: [], body: "None", auth: "None",
});

export default function App() {
  const [req, setReq] = useState<HttpRequest>(blank);
  const [reply, setReply] = useState<ExecuteReply | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [environments, setEnvironments] = useState<Environment[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [envId, setEnvId] = useState<string | null>(null);
  const [showEnv, setShowEnv] = useState(false);
  const [showCode, setShowCode] = useState(false);
  const [showCookies, setShowCookies] = useState(false);
  const [mode, setMode] = useState<"http" | "ws" | "sse" | "graphql">("http");

  const refresh = useCallback(async () => {
    try {
      const [c, e, h] = await Promise.all([api.collections(), api.environments(), api.history()]);
      setCollections(c); setEnvironments(e); setHistory(h);
      setEnvId((cur) => (cur && e.some((x) => x.id === cur) ? cur : null));
    } catch (err) {
      setError(`Cannot reach backend: ${(err as Error).message}`);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const send = async () => {
    setSending(true); setError(null);
    try {
      setReply(await api.execute(req, envId));
      api.history().then(setHistory).catch(() => {});
    } catch (err) {
      setReply(null); setError((err as Error).message);
    } finally {
      setSending(false);
    }
  };

  // Fresh id so edits to a loaded request don't alias the stored one.
  const open = (r: HttpRequest) => { setReq({ ...structuredClone(r), id: crypto.randomUUID() }); setReply(null); setError(null); };

  return (
    <div className="app">
      <Sidebar
        collections={collections} history={history} environments={environments}
        envId={envId} onEnv={setEnvId} onOpen={open} onRefresh={refresh}
        onError={setError} onEditEnv={() => setShowEnv(true)}
      />
      <main className="main">
        <div className="topbar">
          <Tabs variant="pill" value={mode} onChange={setMode} items={[
            { key: "http", label: "HTTP", icon: Globe }, { key: "ws", label: "WebSocket", icon: Radio },
            { key: "sse", label: "SSE", icon: Zap }, { key: "graphql", label: "GraphQL", icon: Terminal },
          ]} />
          {mode === "http" && <>
          <input className="req-name" value={req.name} onChange={(e) => setReq({ ...req, name: e.target.value })} />
          <button className="btn ghost sm" onClick={() => open(blank())}><Plus size={14} />New request</button>
          </>}
        </div>
        {mode === "http" ? (<>
          <RequestPane req={req} onChange={setReq} onSend={send} sending={sending}
            onCode={() => setShowCode(true)} onCookies={() => setShowCookies(true)} />
          <ResponsePane reply={reply} error={error} loading={sending} />
        </>) : mode === "graphql" ? <GraphqlPane envId={envId} /> : <RealtimePane key={mode} kind={mode} />}
      </main>
      {showCode && <CodeModal req={req} onClose={() => setShowCode(false)} />}
      {showCookies && <CookieManager onClose={() => setShowCookies(false)} />}
      {showEnv && <EnvEditor environments={environments} onClose={() => setShowEnv(false)} onChanged={refresh} />}
    </div>
  );
}
