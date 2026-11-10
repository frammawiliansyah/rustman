import { useEffect, useRef, useState } from "react";
import { Plug, PlugZap, Send, Trash2, Wifi } from "lucide-react";
import { Spinner } from "./ui";
import type { WsMessage } from "./types";

interface Entry { id: number; dir: "in" | "out" | "sys"; label: string; text: string; time: string }

const clock = (iso?: string) => new Date(iso ?? Date.now()).toLocaleTimeString();
const wsOrigin = () => location.origin.replace(/^http/, "ws");

export function RealtimePane({ kind }: { kind: "ws" | "sse" }) {
  const isWs = kind === "ws";
  const [url, setUrl] = useState(isWs ? "wss://echo.websocket.events" : "https://");
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [msg, setMsg] = useState("");
  const [log, setLog] = useState<Entry[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const esRef = useRef<EventSource | null>(null);
  const seq = useRef(0);
  const endRef = useRef<HTMLDivElement>(null);

  const push = (e: Omit<Entry, "id" | "time"> & { time?: string }) =>
    setLog((l) => [...l, { ...e, id: seq.current++, time: clock(e.time) }]);

  useEffect(() => { endRef.current?.scrollIntoView({ block: "end" }); }, [log]);
  useEffect(() => () => { wsRef.current?.close(); esRef.current?.close(); }, []);

  const connect = () => {
    const target = encodeURIComponent(url.trim());
    setConnecting(true);
    if (isWs) {
      const ws = new WebSocket(`${wsOrigin()}/api/ws/connect?target=${target}`);
      wsRef.current = ws;
      ws.onopen = () => { setConnecting(false); setConnected(true); push({ dir: "sys", label: "SYS", text: `Connected to ${url}` }); };
      ws.onmessage = (ev) => {
        try {
          const m = JSON.parse(ev.data as string) as WsMessage;
          push({ dir: m.direction === "In" ? "in" : "out", label: m.direction === "In" ? "IN" : "OUT",
            text: m.payload_type === "Binary" ? `[binary, base64] ${m.content}` : m.content, time: m.timestamp });
        } catch { push({ dir: "in", label: "IN", text: String(ev.data) }); }
      };
      ws.onerror = () => push({ dir: "sys", label: "ERR", text: "Connection error" });
      ws.onclose = () => { setConnecting(false); setConnected(false); push({ dir: "sys", label: "SYS", text: "Disconnected" }); wsRef.current = null; };
    } else {
      const es = new EventSource(`/api/sse/connect?target=${target}`);
      esRef.current = es;
      es.onopen = () => { setConnecting(false); setConnected(true); push({ dir: "sys", label: "SYS", text: `Listening on ${url}` }); };
      es.onmessage = (ev) => push({ dir: "in", label: "message", text: ev.data });
      es.onerror = () => {
        // EventSource auto-reconnects; close explicitly so a failed target doesn't loop.
        push({ dir: "sys", label: "ERR", text: "Stream closed or failed" });
        es.close(); esRef.current = null; setConnecting(false); setConnected(false);
      };
    }
  };

  const disconnect = () => {
    wsRef.current?.close();
    if (esRef.current) {
      esRef.current.close(); esRef.current = null; setConnected(false);
      push({ dir: "sys", label: "SYS", text: "Stopped listening" });
    }
  };

  const send = () => {
    if (!msg || wsRef.current?.readyState !== WebSocket.OPEN) return;
    wsRef.current.send(msg); // backend echoes an Out entry into the log
    setMsg("");
  };

  const active = connected || connecting;
  const valid = isWs ? /^wss?:\/\/.+/i.test(url.trim()) : /^https?:\/\/.+/i.test(url.trim());

  return (
    <div className="rt">
      <div className="urlbar">
        <div className="url-field">
          <span className="url-tag"><Wifi size={13} />{isWs ? "WS" : "SSE"}</span>
          <input className="url" value={url} disabled={active} spellCheck={false} placeholder={isWs ? "ws:// or wss://" : "http:// or https://"}
            onChange={(e) => setUrl(e.target.value)} onKeyDown={(e) => e.key === "Enter" && !active && valid && connect()} />
        </div>
        {active
          ? <button className="btn danger" onClick={disconnect}>{connecting && !connected ? <Spinner /> : <Plug size={14} />}{isWs ? "Disconnect" : "Close"}</button>
          : <button className="btn primary send" disabled={!valid} onClick={connect}><PlugZap size={14} />{isWs ? "Connect" : "Listen"}</button>}
        <button className="btn" onClick={() => setLog([])}><Trash2 size={14} />Clear</button>
      </div>
      <div className="hint">
        <span className={`status-dot${connected ? " on" : ""}`} />{connected ? "Connected" : connecting ? "Connecting…" : "Not connected"}
      </div>
      <div className="rt-log">
        {log.length === 0 && <div className="hint">{isWs ? "Messages will appear here." : "Events will appear here."}</div>}
        {log.map((e) => (
          <div className="rt-item" key={e.id}>
            <span className={`dir ${e.dir}`}>{e.label}</span>
            <span className="body">{e.text}</span>
            <time>{e.time}</time>
          </div>
        ))}
        <div ref={endRef} />
      </div>
      {isWs && (
        <div className="rt-send">
          <input placeholder="Message" value={msg} disabled={!connected} onChange={(e) => setMsg(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && send()} />
          <button className="btn primary" disabled={!connected || !msg} onClick={send}><Send size={14} />Send</button>
        </div>
      )}
    </div>
  );
}
