import { useEffect, useMemo, useState } from "react";
import { Bot, Eye, EyeOff, KeyRound, ShieldAlert } from "lucide-react";
import { api } from "./api";
import { CopyButton, Tabs } from "./ui";
import type { AgentInfo } from "./types";

type Client = "claude-code" | "json" | "other";

const TOOLS: [string, string][] = [
  ["send_request", "Send an HTTP request and get the status, headers, body, timing and test results"],
  ["open_request", "Show a request here without sending it"],
  ["run_collection", "Run every request of a saved collection and summarize the results"],
  ["list_collections", "Browse saved collections, folders and requests"],
  ["get_collection", "Read the full definition of one collection"],
  ["save_request", "Save or update a request in a collection"],
  ["list_environments", "Read environments and their variables"],
  ["set_environment_variable", "Set a variable, so {{placeholders}} resolve"],
  ["list_history", "List recent requests, including those sent by agents"],
  ["get_history_entry", "Read one past request with its full config and response"],
  ["import_collection", "Import a Postman v2.1 collection or an OpenAPI document"],
  ["generate_snippet", "Generate cURL, Python, JavaScript, Go or Rust code"],
];

const PROMPTS = [
  "Using Rustman, test the login and profile endpoints of my API at http://localhost:3000 and tell me which ones fail.",
  "Import this OpenAPI spec into Rustman, then run every GET endpoint and report the slow ones.",
];

const DEFAULT_PORT = "51730";

function ago(iso: string): string {
  const s = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}

const quote = (s: string) => (/\s/.test(s) ? `"${s}"` : s);

function Code({ text }: { text: string }) {
  return (
    <div className="mcp-code">
      <pre>{text}</pre>
      <CopyButton text={text} />
    </div>
  );
}

export function McpPane({ liveAgent }: { liveAgent: string | null }) {
  const [info, setInfo] = useState<AgentInfo | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [client, setClient] = useState<Client>("claude-code");
  const [reveal, setReveal] = useState(false);

  // Reload when an agent starts or stops working, so "last activity" stays current.
  useEffect(() => {
    let live = true;
    api.agentInfo().then((i) => live && setInfo(i)).catch((e) => live && setErr((e as Error).message));
    return () => { live = false; };
  }, [liveAgent]);

  const port = window.location.port && window.location.port !== DEFAULT_PORT ? window.location.port : "";
  const args = useMemo(() => ["mcp", ...(port ? ["--port", port] : [])], [port]);

  if (err) return <div className="mcp-page"><div className="mcp-card"><p className="mcp-error">Could not load agent details: {err}</p></div></div>;
  if (!info) return <div className="mcp-page" />;

  const claudeCode = `claude mcp add rustman -- ${quote(info.command)} ${args.join(" ")}`;
  const json = JSON.stringify({ mcpServers: { rustman: { command: info.command, args } } }, null, 2);
  const masked = "•".repeat(Math.min(info.token.length, 32)) + (info.token.length > 4 ? info.token.slice(-4) : "");
  const configPath = info.os === "macos" ? "~/Library/Application Support/Claude/claude_desktop_config.json"
    : info.os === "windows" ? "%APPDATA%\\Claude\\claude_desktop_config.json" : null;

  return (
    <div className="mcp-page">
      <section className="mcp-hero">
        <div className="mcp-hero-ico"><Bot size={26} /></div>
        <div>
          <h2>Let AI agents test your APIs</h2>
          <p>Connect an agent like Claude Code or Cursor. It sends requests through Rustman, and you watch every step here.</p>
          <div className="mcp-status">
            {liveAgent
              ? <span className="mcp-pill live"><i />{liveAgent} is working now</span>
              : info.last_agent
                ? <span className="mcp-pill"><i />Last agent: {info.last_agent.agent}, {ago(info.last_agent.at)}</span>
                : <span className="mcp-pill idle"><i />No agent has connected yet</span>}
            <span className="mcp-pill muted">Rustman v{info.version}</span>
          </div>
        </div>
      </section>

      <section className="mcp-card">
        <h3><span className="mcp-num">1</span> Connect your agent</h3>
        {!info.command_found && (
          <p className="mcp-warn">
            The <code>rustman</code> command-line tool was not found. Install it with the command in the README first.
          </p>
        )}
        <Tabs value={client} onChange={setClient} items={[
          { key: "claude-code", label: "Claude Code" },
          { key: "json", label: "Claude Desktop / Cursor" },
          { key: "other", label: "Other" },
        ]} />
        <div className="mcp-tab">
          {client === "claude-code" && (<>
            <p>Run this in a terminal:</p>
            <Code text={claudeCode} />
          </>)}
          {client === "json" && (<>
            <p>
              Add this to your client's MCP config, then restart it.
              {configPath && <> Claude Desktop: <code>{configPath}</code>.</>}
              {" "}Cursor: <code>~/.cursor/mcp.json</code>.
            </p>
            <Code text={json} />
          </>)}
          {client === "other" && (<>
            <p>Any MCP client with local (stdio) support works:</p>
            <dl className="mcp-kv">
              <dt>Command</dt><dd><code>{info.command}</code> <CopyButton text={info.command} /></dd>
              <dt>Arguments</dt><dd><code>{args.join(" ")}</code> <CopyButton text={args.join(" ")} /></dd>
            </dl>
          </>)}
          <p className="hint">No token needed. If this app is closed, the agent starts it when it needs it.</p>
        </div>
      </section>

      <section className="mcp-card">
        <h3><span className="mcp-num">2</span> Ask in plain language</h3>
        <ul className="mcp-prompts">
          {PROMPTS.map((p) => <li key={p}><span>{p}</span><CopyButton text={p} /></li>)}
        </ul>
        <h4>What can the agent do?</h4>
        <div className="mcp-tools">
          {TOOLS.map(([name, desc]) => (
            <div key={name} className="mcp-tool"><code>{name}</code><span>{desc}</span></div>
          ))}
        </div>
      </section>

      <p className="mcp-safety"><ShieldAlert size={14} /> Agents can reach any address this machine can. Only connect agents you trust, and check History for requests with a robot icon.</p>

      <section className="mcp-card">
        <h3><KeyRound size={15} /> Access token (only for your own scripts)</h3>
        <p className="mcp-lead">
          <code>rustman mcp</code> reads the token automatically. Use it as <code>Authorization: Bearer &lt;token&gt;</code> in custom integrations.
        </p>
        <div className="mcp-token">
          <code>{reveal ? info.token : masked}</code>
          <button className="btn sm" onClick={() => setReveal(!reveal)}>{reveal ? <EyeOff size={13} /> : <Eye size={13} />}{reveal ? "Hide" : "Show"}</button>
          <CopyButton text={info.token} label="Copy token" />
        </div>
        <p className="hint">
          {info.token_source === "file"
            ? <>Stored in <code>{info.token_file}</code>. Delete it and restart Rustman to create a new one.</>
            : <>Set by the <code>RUSTMAN_TOKEN</code> environment variable.</>}
        </p>
      </section>
    </div>
  );
}
