import { useEffect, useMemo, useState } from "react";
import { Bot, Eye, EyeOff, KeyRound, ShieldAlert, TerminalSquare } from "lucide-react";
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
  "Save a request called \"Create order\" in a collection named Shop, then send it with a test that checks for a 201.",
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
          <p>
            Rustman includes an <b>MCP server</b> (Model Context Protocol). Connect an AI agent such as Claude Code, Claude Desktop or Cursor,
            and it can send requests, run collections and manage environments through Rustman.
            This window follows along: the request form fills in, the response appears and every call lands in History with a robot icon,
            so you always see what the agent does.
          </p>
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
        <h3>How it works</h3>
        <ol className="mcp-steps">
          <li><b>Connect</b><span>Add Rustman to your agent once (see Set up below).</span></li>
          <li><b>Ask</b><span>Tell the agent what to test, in plain language.</span></li>
          <li><b>Watch</b><span>The agent calls Rustman, and you see each request, response and history entry live.</span></li>
        </ol>
      </section>

      <section className="mcp-card">
        <h3>Set up</h3>
        <p className="mcp-lead">
          Pick your agent and copy the setup. The agent starts <code>rustman mcp</code>, which talks to this app and reads the access token on its own,
          so you do not need to paste the token anywhere.
        </p>
        {!info.command_found && (
          <p className="mcp-warn">
            The <code>rustman</code> command-line tool was not found on this machine. Install it by running the install command from the README,
            then use the setup below.
          </p>
        )}
        <Tabs value={client} onChange={setClient} items={[
          { key: "claude-code", label: "Claude Code" },
          { key: "json", label: "Claude Desktop / Cursor" },
          { key: "other", label: "Other clients" },
        ]} />
        <div className="mcp-tab">
          {client === "claude-code" && (<>
            <p>Run this in a terminal:</p>
            <Code text={claudeCode} />
          </>)}
          {client === "json" && (<>
            <p>
              Add this to the <code>mcpServers</code> section of your client's MCP config
              {configPath && <> (Claude Desktop: <code>{configPath}</code>)</>}
              {" "}(Cursor: <code>~/.cursor/mcp.json</code>), then restart the client:
            </p>
            <Code text={json} />
          </>)}
          {client === "other" && (<>
            <p>Any MCP client that supports a local (stdio) server can use these values:</p>
            <dl className="mcp-kv">
              <dt>Command</dt><dd><code>{info.command}</code> <CopyButton text={info.command} /></dd>
              <dt>Arguments</dt><dd><code>{args.join(" ")}</code> <CopyButton text={args.join(" ")} /></dd>
              <dt>Transport</dt><dd><code>stdio</code></dd>
            </dl>
          </>)}
          <p className="hint">
            If the desktop app is not running, <code>rustman mcp</code> starts it the first time the agent uses a tool.
            {port && <> This app listens on port {port}, so the setup above passes <code>--port {port}</code>.</>}
          </p>
        </div>
      </section>

      <section className="mcp-card">
        <h3>How to use it</h3>
        <p className="mcp-lead">Once connected, just ask your agent. For example:</p>
        <ul className="mcp-prompts">
          {PROMPTS.map((p) => <li key={p}><span>{p}</span><CopyButton text={p} /></li>)}
        </ul>
        <h4>What the agent can do</h4>
        <div className="mcp-tools">
          {TOOLS.map(([name, desc]) => (
            <div key={name} className="mcp-tool"><code>{name}</code><span>{desc}</span></div>
          ))}
        </div>
        <p className="hint">Collections, environments and recent history are also available to agents as MCP resources.</p>
      </section>

      <section className="mcp-card">
        <h3><KeyRound size={15} /> Access token</h3>
        <p className="mcp-lead">
          Every call to Rustman's local backend needs this token. {info.token_source === "env"
            ? <>It comes from the <code>RUSTMAN_TOKEN</code> environment variable.</>
            : <>Rustman created it the first time it started and stores it in a file only your user can read.</>}
          {" "}<code>rustman mcp</code> reads it automatically. You only need it for your own scripts or custom integrations.
        </p>
        <div className="mcp-token">
          <code>{reveal ? info.token : masked}</code>
          <button className="btn sm" onClick={() => setReveal(!reveal)}>{reveal ? <EyeOff size={13} /> : <Eye size={13} />}{reveal ? "Hide" : "Show"}</button>
          <CopyButton text={info.token} label="Copy token" />
        </div>
        <dl className="mcp-kv">
          {info.token_source === "file"
            ? <><dt>Stored in</dt><dd><code>{info.token_file}</code></dd></>
            : <><dt>Source</dt><dd><code>RUSTMAN_TOKEN</code> environment variable</dd></>}
          <dt>Use with the API</dt>
          <dd><code>Authorization: Bearer &lt;token&gt;</code></dd>
        </dl>
        <p className="hint">
          To use a fixed token (for example in a container), set <code>RUSTMAN_TOKEN</code> for both the app and <code>rustman mcp</code>.
          To create a new token, delete the token file and restart Rustman.
        </p>
      </section>

      <section className="mcp-card">
        <h3><ShieldAlert size={15} /> Good to know</h3>
        <ul className="mcp-list">
          <li>Agents act with your permissions. They can send requests to any address this machine can reach, including <code>localhost</code> and your internal network.</li>
          <li>Only connect agents you trust, and check History when an agent has been working. Requests sent by agents have a robot icon.</li>
          <li>The backend only listens on this machine and refuses requests without the token. Web pages in your browser cannot read the token.</li>
        </ul>
        <p className="hint"><TerminalSquare size={12} /> Prefer the terminal? The same engine runs collections in CI with <code>rustman run</code>.</p>
      </section>
    </div>
  );
}
