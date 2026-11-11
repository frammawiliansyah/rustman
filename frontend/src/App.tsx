import { useCallback, useEffect, useRef, useState } from "react";
import { Bot, Globe, Plus, Radio, Terminal, Zap } from "lucide-react";
import { api, withToken } from "./api";
import { CodeModal } from "./CodeModal";
import { CookieManager } from "./CookieManager";
import { GraphqlPane } from "./GraphqlPane";
import { McpPane } from "./McpPane";
import { RealtimePane } from "./RealtimePane";
import { EnvEditor } from "./EnvEditor";
import { RequestPane } from "./RequestPane";
import { ResponsePane } from "./ResponsePane";
import { Sidebar } from "./Sidebar";
import { Tabs } from "./ui";
import type { BackendEvent, Collection, Environment, ExecuteReply, HistoryEntry, HttpRequest } from "./types";

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
  const [mode, setMode] = useState<"http" | "ws" | "sse" | "graphql" | "mcp">("http");
  /** Name of the AI agent currently driving the UI, shown as a banner. */
  const [agent, setAgent] = useState<string | null>(null);

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

  // Follow along while an AI agent works through `rustman mcp`. Events are handled one at
  // a time with short pauses, so a fast burst of requests stays readable.
  const queue = useRef<Promise<void>>(Promise.resolve());
  useEffect(() => {
    const pause = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
    let hideTimer: ReturnType<typeof setTimeout> | undefined;
    const handle = async (ev: BackendEvent) => {
      if (ev.type === "data_changed") { refresh(); return; }
      if (ev.type === "focus_request") {
        setMode("http"); setReq(ev.request); setReply(null); setError(null);
        if (ev.source) setAgent(ev.source.agent);
        await pause(500);
        return;
      }
      setAgent(ev.source.agent);
      clearTimeout(hideTimer);
      if (ev.type === "request_started") {
        setMode("http"); setReq(ev.request); setReply(null); setError(null); setSending(true);
        await pause(700);
      } else if (ev.type === "request_finished") {
        setReply(ev.reply); setError(null); setSending(false);
        await pause(900);
      } else {
        setReply(null); setError(ev.error); setSending(false);
        await pause(900);
      }
      hideTimer = setTimeout(() => setAgent(null), 4000);
    };
    const es = new EventSource(withToken("/api/events"));
    es.onmessage = (m) => {
      let ev: BackendEvent;
      try { ev = JSON.parse(m.data) as BackendEvent; } catch { return; }
      queue.current = queue.current.then(() => handle(ev)).catch(() => {});
    };
    return () => { es.close(); clearTimeout(hideTimer); };
  }, [refresh]);

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
            { key: "mcp", label: "AI Agents", icon: Bot },
          ]} />
          {agent && <span className="agent-badge" title="An AI agent is controlling Rustman"><Bot size={14} />{agent} is working</span>}
          {mode === "http" && <>
          <input className="req-name" value={req.name} onChange={(e) => setReq({ ...req, name: e.target.value })} />
          <button className="btn ghost sm" onClick={() => open(blank())}><Plus size={14} />New request</button>
          </>}
        </div>
        {mode === "http" ? (<>
          <RequestPane req={req} onChange={setReq} onSend={send} sending={sending}
            onCode={() => setShowCode(true)} onCookies={() => setShowCookies(true)} />
          <ResponsePane reply={reply} error={error} loading={sending} />
        </>) : mode === "graphql" ? <GraphqlPane envId={envId} />
          : mode === "mcp" ? <McpPane liveAgent={agent} />
          : <RealtimePane key={mode} kind={mode} />}
      </main>
      {showCode && <CodeModal req={req} onClose={() => setShowCode(false)} />}
      {showCookies && <CookieManager onClose={() => setShowCookies(false)} />}
      {showEnv && <EnvEditor environments={environments} onClose={() => setShowEnv(false)} onChanged={refresh} />}
    </div>
  );
}
