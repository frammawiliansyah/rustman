//! `rustman mcp`: a Model Context Protocol server (stdio) that lets AI agents drive the
//! Rustman desktop app.
//!
//! The server is a thin client of the desktop backend. Every request an agent sends goes
//! through the backend's `/api/execute` with an agent `source`, so the desktop UI follows
//! along live and the request lands in history with its full config.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use rustman_core::storage::token::{data_dir, read_token};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

const SUPPORTED_PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_BODY_CHARS: usize = 20_000;
const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

pub struct McpOptions {
    pub host: String,
    pub port: u16,
    /// Do not start the desktop app automatically when it is not running.
    pub no_launch: bool,
}

/// Runs the server on stdin/stdout until stdin closes.
pub async fn serve(opts: McpOptions) -> Result<()> {
    let mut server = Server::new(opts);
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(msg).await,
            Err(e) => Some(rpc_error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            out.write_all(reply.to_string().as_bytes()).await?;
            out.write_all(b"\n").await?;
            out.flush().await?;
        }
    }
    Ok(())
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// ---------- backend client ----------

enum Payload {
    None,
    Json(Value),
    Raw(String),
}

struct Backend {
    base: String,
    host: String,
    port: u16,
    no_launch: bool,
    http: reqwest::Client,
    launched: bool,
}

impl Backend {
    fn new(opts: &McpOptions) -> Self {
        let host = match opts.host.as_str() {
            "0.0.0.0" => "127.0.0.1".to_string(),
            "::" => "::1".to_string(),
            h => h.to_string(),
        };
        let url_host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.clone() };
        Self {
            base: format!("http://{url_host}:{}", opts.port),
            host: opts.host.clone(),
            port: opts.port,
            no_launch: opts.no_launch,
            http: reqwest::Client::builder().timeout(Duration::from_secs(300)).build().expect("http client"),
            launched: false,
        }
    }

    async fn send(&self, method: reqwest::Method, path: &str, payload: &Payload) -> Result<reqwest::Response, reqwest::Error> {
        let mut rb = self.http.request(method, format!("{}/api{path}", self.base));
        if let Some(t) = read_token(&data_dir()) {
            rb = rb.bearer_auth(t);
        }
        rb = match payload {
            Payload::None => rb,
            Payload::Json(v) => rb.json(v),
            Payload::Raw(s) => rb.body(s.clone()),
        };
        rb.send().await
    }

    async fn call(&mut self, method: reqwest::Method, path: &str, payload: Payload) -> Result<Value, String> {
        let res = match self.send(method.clone(), path, &payload).await {
            Ok(r) => r,
            Err(e) if e.is_connect() => {
                self.launch().await?;
                self.send(method, path, &payload).await.map_err(|e| format!("cannot reach the Rustman backend: {e}"))?
            }
            Err(e) => return Err(format!("backend request failed: {e}")),
        };
        let status = res.status();
        let text = res.text().await.map_err(|e| e.to_string())?;
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err("the Rustman backend rejected the access token. Restart Rustman, or make sure RUSTMAN_TOKEN \
                        matches the one the app was started with."
                .into());
        }
        if !status.is_success() {
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
                .unwrap_or(text);
            return Err(format!("{status}: {msg}"));
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| format!("invalid backend response: {e}"))
    }

    async fn get(&mut self, path: &str) -> Result<Value, String> {
        self.call(reqwest::Method::GET, path, Payload::None).await
    }

    async fn post(&mut self, path: &str, body: Value) -> Result<Value, String> {
        self.call(reqwest::Method::POST, path, Payload::Json(body)).await
    }

    /// Starts the desktop app (it opens its window when a display is available) and waits
    /// until its API answers.
    async fn launch(&mut self) -> Result<(), String> {
        if self.no_launch {
            return Err("Rustman is not running and automatic launch is disabled. Start the desktop app first.".into());
        }
        if !self.launched {
            let exe = find_desktop().ok_or_else(|| {
                "Rustman is not running and the desktop app was not found. Install it from \
                 https://github.com/frammawiliansyah/rustman/releases and try again."
                    .to_string()
            })?;
            let mut cmd = std::process::Command::new(exe);
            cmd.args(["--host", &self.host, "--port", &self.port.to_string()])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            #[cfg(unix)]
            std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
            #[cfg(windows)]
            std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0000_0208);
            cmd.spawn().map_err(|e| format!("failed to start the Rustman desktop app: {e}"))?;
            self.launched = true;
        }
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if self.send(reqwest::Method::GET, "/collections", &Payload::None).await.is_ok() {
                return Ok(());
            }
        }
        Err("started the Rustman desktop app but it did not become ready in 30 seconds".into())
    }
}

/// Looks for the desktop binary next to this executable, on PATH and in the usual install locations.
fn find_desktop() -> Option<PathBuf> {
    let name = if cfg!(windows) { "rustman-desktop.exe" } else { "rustman-desktop" };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
        candidates.push(dir.join(name));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(name)));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    candidates.push(PathBuf::from("/Applications/Rustman.app/Contents/MacOS/rustman-desktop"));
    if let Some(h) = &home {
        candidates.push(h.join("Applications/Rustman.app/Contents/MacOS/rustman-desktop"));
        candidates.push(h.join(".local/share/rustman/rustman-desktop"));
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(l).join("Rustman").join("desktop").join(name));
    }
    candidates.into_iter().find(|p| p.is_file())
}

// ---------- request building and helpers (pure) ----------

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Accepts `{ "Key": "value" }` or `[ { "key": .., "value": .. } ]` and returns Rustman key/value rows.
fn kv_rows(v: &Value) -> Result<Vec<Value>, String> {
    match v {
        Value::Null => Ok(vec![]),
        Value::Object(m) => Ok(m
            .iter()
            .map(|(k, v)| {
                let value = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
                json!({ "key": k, "value": value, "enabled": true })
            })
            .collect()),
        Value::Array(a) => a
            .iter()
            .map(|r| {
                let key = r.get("key").and_then(Value::as_str).ok_or("each row needs a \"key\"")?;
                let value = r.get("value").and_then(Value::as_str).unwrap_or("");
                let enabled = r.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                Ok(json!({ "key": key, "value": value, "enabled": enabled }))
            })
            .collect(),
        _ => Err("expected an object or an array of {key, value}".into()),
    }
}

/// Turns tool arguments into Rustman's serde representation of `HttpRequest`.
fn build_request(args: &Value) -> Result<Value, String> {
    let method = str_arg(args, "method").unwrap_or("GET").to_ascii_uppercase();
    if !METHODS.contains(&method.as_str()) {
        return Err(format!("unsupported method {method}; use one of {}", METHODS.join(", ")));
    }
    let url = str_arg(args, "url").ok_or("\"url\" is required")?;
    let name = str_arg(args, "name").map(str::to_string).unwrap_or_else(|| format!("{method} {url}"));

    let body = if let Some(b) = args.get("body").filter(|b| !b.is_null()) {
        let (text, guessed) = match b {
            Value::String(s) => (s.clone(), if serde_json::from_str::<Value>(s).is_ok() { "application/json" } else { "text/plain" }),
            other => (other.to_string(), "application/json"),
        };
        let content_type = str_arg(args, "content_type").unwrap_or(guessed);
        json!({ "Raw": { "content_type": content_type, "body": text } })
    } else if let Some(f) = args.get("form_urlencoded").filter(|f| !f.is_null()) {
        json!({ "FormUrlEncoded": kv_rows(f)? })
    } else if let Some(f) = args.get("form_data").filter(|f| !f.is_null()) {
        json!({ "FormData": kv_rows(f)? })
    } else {
        json!("None")
    };

    let auth = match args.get("auth").filter(|a| !a.is_null()) {
        None => json!("None"),
        Some(a) => match a.get("type").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
            Some("bearer") => json!({ "Bearer": { "token": str_arg(a, "token").ok_or("auth.token is required")? } }),
            Some("basic") => json!({ "Basic": {
                "username": str_arg(a, "username").ok_or("auth.username is required")?,
                "password": a.get("password").and_then(Value::as_str).unwrap_or(""),
            } }),
            Some("api_key") | Some("apikey") => json!({ "ApiKey": {
                "key": str_arg(a, "key").ok_or("auth.key is required")?,
                "value": a.get("value").and_then(Value::as_str).unwrap_or(""),
                "in_header": a.get("in").and_then(Value::as_str).map(|i| i != "query").unwrap_or(true),
            } }),
            Some("none") => json!("None"),
            _ => return Err("auth.type must be bearer, basic, api_key or none".into()),
        },
    };

    Ok(json!({
        "id": Uuid::new_v4(),
        "name": name,
        "method": method,
        "url": url,
        "headers": kv_rows(args.get("headers").unwrap_or(&Value::Null))?,
        "query_params": kv_rows(args.get("query").unwrap_or(&Value::Null))?,
        "body": body,
        "auth": auth,
        "pre_request_script": null,
        "test_script": str_arg(args, "test_script"),
    }))
}

/// Finds an item by id, exact name (case-insensitive) or a unique name fragment.
fn pick<'a>(list: &'a [Value], key: &str, what: &str) -> Result<&'a Value, String> {
    let name = |v: &Value| v.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    if let Some(v) = list.iter().find(|v| v.get("id").and_then(Value::as_str) == Some(key)) {
        return Ok(v);
    }
    let lower = key.to_lowercase();
    if let Some(v) = list.iter().find(|v| name(v).to_lowercase() == lower) {
        return Ok(v);
    }
    let partial: Vec<&Value> = list.iter().filter(|v| name(v).to_lowercase().contains(&lower)).collect();
    match partial.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no {what} matches \"{key}\". Available: {}",
            list.iter().map(name).collect::<Vec<_>>().join(", ")
        )),
        many => Err(format!(
            "\"{key}\" matches several {what}s: {}. Use the exact name or the id.",
            many.iter().map(|v| name(v)).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Depth-first list of requests with their folder path.
fn collect_requests(items: &[Value], prefix: &str, out: &mut Vec<(String, Value)>) {
    for it in items {
        if let Some(r) = it.get("Request") {
            out.push((prefix.to_string(), r.clone()));
        } else if let Some(f) = it.get("Folder") {
            let n = f.get("name").and_then(Value::as_str).unwrap_or("");
            let path = if prefix.is_empty() { n.to_string() } else { format!("{prefix}/{n}") };
            collect_requests(f.get("items").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]), &path, out);
        }
    }
}

/// Returns the item list of the folder at `path`, creating folders that do not exist yet.
fn items_at<'a>(items: &'a mut Vec<Value>, path: &[&str]) -> &'a mut Vec<Value> {
    let Some((first, rest)) = path.split_first() else { return items };
    let idx = match items
        .iter()
        .position(|it| it.get("Folder").and_then(|f| f.get("name")).and_then(Value::as_str) == Some(first))
    {
        Some(i) => i,
        None => {
            items.push(json!({ "Folder": { "id": Uuid::new_v4(), "name": first, "description": null, "items": [] } }));
            items.len() - 1
        }
    };
    let folder = items[idx].get_mut("Folder").and_then(Value::as_object_mut).expect("folder object");
    let inner = folder.entry("items").or_insert_with(|| json!([]));
    items_at(inner.as_array_mut().expect("items array"), rest)
}

fn truncate(s: &str) -> (String, bool) {
    if s.chars().count() <= MAX_BODY_CHARS {
        return (s.to_string(), false);
    }
    (s.chars().take(MAX_BODY_CHARS).collect(), true)
}

fn headers_map(v: &Value) -> Value {
    let mut m = Map::new();
    for h in v.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        if let (Some(k), Some(val)) = (h.get("key").and_then(Value::as_str), h.get("value").and_then(Value::as_str)) {
            match m.get_mut(k) {
                Some(Value::String(existing)) => {
                    existing.push_str(", ");
                    existing.push_str(val);
                }
                _ => {
                    m.insert(k.to_string(), json!(val));
                }
            }
        }
    }
    Value::Object(m)
}

/// Compact, agent-friendly view of a backend execute reply.
fn summarize_reply(request: &Value, reply: &Value) -> Value {
    let res = &reply["response"];
    let (body, truncated) = truncate(res["body"].as_str().unwrap_or(""));
    let mut out = json!({
        "request": { "method": request["method"], "url": request["url"], "name": request["name"] },
        "status": res["status"],
        "elapsed_ms": res["elapsed_ms"],
        "headers": headers_map(&res["headers"]),
        "body": body,
        "history_id": reply["history_id"],
    });
    if truncated {
        out["body_truncated"] = json!(true);
    }
    if let Some(t) = reply.get("test_results").filter(|t| t.as_array().map(|a| !a.is_empty()).unwrap_or(false)) {
        out["tests"] = t.clone();
    }
    if let Some(e) = reply.get("script_error").filter(|e| !e.is_null()) {
        out["script_error"] = e.clone();
    }
    out
}

fn tests_failed(reply: &Value) -> usize {
    reply["test_results"]
        .as_array()
        .map(|a| a.iter().filter(|t| t["passed"] == json!(false)).count())
        .unwrap_or(0)
}

// ---------- tool schemas ----------

fn request_props() -> Value {
    json!({
        "method": { "type": "string", "enum": METHODS, "description": "HTTP method. Default GET." },
        "url": { "type": "string", "description": "Full URL. May use {{variables}} from the selected environment." },
        "name": { "type": "string", "description": "Name shown in the Rustman UI and history." },
        "headers": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Request headers as {name: value}." },
        "query": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Query parameters as {name: value}." },
        "body": { "description": "Raw body. A string is sent as is; an object or array is sent as JSON." },
        "content_type": { "type": "string", "description": "Content-Type of the raw body. Guessed when omitted." },
        "form_urlencoded": { "type": "object", "additionalProperties": { "type": "string" }, "description": "application/x-www-form-urlencoded fields." },
        "form_data": { "type": "object", "additionalProperties": { "type": "string" }, "description": "multipart/form-data fields." },
        "auth": {
            "type": "object",
            "description": "bearer: {type, token}; basic: {type, username, password}; api_key: {type, key, value, in: header|query}.",
            "properties": { "type": { "type": "string", "enum": ["bearer", "basic", "api_key", "none"] } },
            "required": ["type"]
        },
        "test_script": { "type": "string", "description": "JavaScript test script using the Postman pm.* API, for example pm.test(\"ok\", () => pm.response.to.have.status(200))." }
    })
}

fn with_props(extra: Value, required: &[&str]) -> Value {
    let mut props = request_props().as_object().cloned().unwrap_or_default();
    if let Some(e) = extra.as_object() {
        props.extend(e.clone());
    }
    json!({ "type": "object", "properties": props, "required": required })
}

fn tools() -> Value {
    json!([
        {
            "name": "send_request",
            "description": "Send an HTTP request through Rustman. The Rustman desktop app shows the request config, the response and a history entry live. Returns status, headers, body, timing and test results.",
            "inputSchema": with_props(json!({
                "environment": { "type": "string", "description": "Environment name or id whose variables resolve {{placeholders}}." }
            }), &["url"])
        },
        {
            "name": "open_request",
            "description": "Show a request in the Rustman UI without sending it, for example to let the user review it first.",
            "inputSchema": with_props(json!({}), &["url"])
        },
        {
            "name": "run_collection",
            "description": "Run every request of a saved collection in order, with the Rustman UI following along. Returns a per-request summary with status and test results.",
            "inputSchema": { "type": "object", "properties": {
                "collection": { "type": "string", "description": "Collection name or id." },
                "environment": { "type": "string", "description": "Environment name or id." },
                "stop_on_failure": { "type": "boolean", "description": "Stop at the first request that errors or has a failing test. Default false." }
            }, "required": ["collection"] }
        },
        {
            "name": "list_collections",
            "description": "List saved collections with their folders and requests.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "get_collection",
            "description": "Get the full definition of one collection.",
            "inputSchema": { "type": "object", "properties": { "collection": { "type": "string", "description": "Collection name or id." } }, "required": ["collection"] }
        },
        {
            "name": "save_request",
            "description": "Save a request into a collection (the collection and folder are created when missing). A request with the same name in the same place is replaced.",
            "inputSchema": with_props(json!({
                "collection": { "type": "string", "description": "Collection name or id." },
                "folder": { "type": "string", "description": "Optional folder path such as \"Users\" or \"Users/Admin\"." }
            }), &["collection", "name", "url"])
        },
        {
            "name": "list_environments",
            "description": "List environments and their variables. Secret values are included.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "set_environment_variable",
            "description": "Set a variable in an environment, creating the environment when it does not exist.",
            "inputSchema": { "type": "object", "properties": {
                "environment": { "type": "string", "description": "Environment name or id." },
                "key": { "type": "string" },
                "value": { "type": "string" },
                "secret": { "type": "boolean", "description": "Mask the value in the UI. Default false." }
            }, "required": ["environment", "key", "value"] }
        },
        {
            "name": "list_history",
            "description": "List recent requests, newest first, including those sent by agents.",
            "inputSchema": { "type": "object", "properties": { "limit": { "type": "integer", "minimum": 1, "maximum": 200, "description": "Default 20." } } }
        },
        {
            "name": "get_history_entry",
            "description": "Get one history entry with the full request config and response.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }
        },
        {
            "name": "import_collection",
            "description": "Import a Postman v2.1 collection or an OpenAPI 3.x document (JSON text) into Rustman.",
            "inputSchema": { "type": "object", "properties": {
                "format": { "type": "string", "enum": ["postman", "openapi"] },
                "content": { "type": "string", "description": "The file content." }
            }, "required": ["format", "content"] }
        },
        {
            "name": "generate_snippet",
            "description": "Generate client code for a request.",
            "inputSchema": with_props(json!({
                "language": { "type": "string", "enum": ["curl", "javascript", "python", "go", "rust"] }
            }), &["language", "url"])
        }
    ])
}

// ---------- server ----------

struct Server {
    backend: Backend,
    agent: String,
    session: String,
}

impl Server {
    fn new(opts: McpOptions) -> Self {
        Self { backend: Backend::new(&opts), agent: "mcp".into(), session: Uuid::new_v4().to_string() }
    }

    fn source(&self) -> Value {
        json!({ "agent": self.agent, "session": self.session })
    }

    async fn handle(&mut self, msg: Value) -> Option<Value> {
        let method = msg.get("method").and_then(Value::as_str)?.to_string();
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // Notifications carry no id and never get a reply.
        let id = match id {
            Some(id) => id,
            None => return None,
        };
        Some(match method.as_str() {
            "initialize" => {
                if let Some(n) = params["clientInfo"]["name"].as_str() {
                    let clean: String = n.chars().filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ')).collect();
                    if !clean.trim().is_empty() {
                        self.agent = clean.trim().to_string();
                    }
                }
                let wanted = params["protocolVersion"].as_str().unwrap_or("");
                let version = if SUPPORTED_PROTOCOLS.contains(&wanted) { wanted } else { SUPPORTED_PROTOCOLS[0] };
                rpc_result(id, json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {}, "resources": {} },
                    "serverInfo": { "name": "rustman", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Drives the Rustman desktop app. Requests you send appear live in its window and in its history."
                }))
            }
            "ping" => rpc_result(id, json!({})),
            "tools/list" => rpc_result(id, json!({ "tools": tools() })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("").to_string();
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                if !tools().as_array().map(|t| t.iter().any(|x| x["name"] == name.as_str())).unwrap_or(false) {
                    return Some(rpc_error(id, -32602, &format!("unknown tool: {name}")));
                }
                match self.call_tool(&name, &args).await {
                    Ok(v) => rpc_result(id, tool_text(&v, false)),
                    Err(e) => rpc_result(id, tool_text(&json!(e), true)),
                }
            }
            "resources/list" => rpc_result(id, json!({ "resources": [
                { "uri": "rustman://collections", "name": "Collections", "mimeType": "application/json" },
                { "uri": "rustman://environments", "name": "Environments", "mimeType": "application/json" },
                { "uri": "rustman://history", "name": "Recent history", "mimeType": "application/json" },
            ] })),
            "resources/templates/list" => rpc_result(id, json!({ "resourceTemplates": [] })),
            "resources/read" => {
                let uri = params["uri"].as_str().unwrap_or("").to_string();
                let path = match uri.as_str() {
                    "rustman://collections" => "/collections",
                    "rustman://environments" => "/environments",
                    "rustman://history" => "/history?limit=50",
                    _ => return Some(rpc_error(id, -32002, &format!("unknown resource: {uri}"))),
                };
                match self.backend.get(path).await {
                    Ok(v) => rpc_result(id, json!({ "contents": [{
                        "uri": uri, "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&v).unwrap_or_default()
                    }] })),
                    Err(e) => rpc_error(id, -32603, &e),
                }
            }
            other => rpc_error(id, -32601, &format!("method not found: {other}")),
        })
    }

    async fn call_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "send_request" => {
                let request = build_request(args)?;
                let env = self.resolve_env_id(args).await?;
                let reply = self
                    .backend
                    .post("/execute", json!({ "request": request, "environment_id": env, "source": self.source() }))
                    .await?;
                Ok(summarize_reply(&request, &reply))
            }
            "open_request" => {
                let request = build_request(args)?;
                self.backend.post("/focus", json!({ "request": request, "source": self.source() })).await?;
                Ok(json!({ "shown": true, "name": request["name"] }))
            }
            "run_collection" => self.run_collection(args).await,
            "list_collections" => {
                let cols = self.backend.get("/collections").await?;
                Ok(json!(cols
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .map(|c| {
                        let mut reqs = Vec::new();
                        collect_requests(c["items"].as_array().map(Vec::as_slice).unwrap_or(&[]), "", &mut reqs);
                        json!({
                            "id": c["id"], "name": c["name"], "description": c["description"],
                            "requests": reqs.iter().map(|(folder, r)| json!({
                                "folder": folder, "name": r["name"], "method": r["method"], "url": r["url"]
                            })).collect::<Vec<_>>()
                        })
                    })
                    .collect::<Vec<_>>()))
            }
            "get_collection" => {
                let key = str_arg(args, "collection").ok_or("\"collection\" is required")?;
                let cols = self.backend.get("/collections").await?;
                pick(cols.as_array().map(Vec::as_slice).unwrap_or(&[]), key, "collection").cloned()
            }
            "save_request" => self.save_request(args).await,
            "list_environments" => self.backend.get("/environments").await,
            "set_environment_variable" => self.set_variable(args).await,
            "list_history" => {
                let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 200);
                let h = self.backend.get(&format!("/history?limit={limit}")).await?;
                Ok(json!(h
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .map(|e| json!({
                        "id": e["id"], "name": e["request_name"], "method": e["method"], "url": e["url"],
                        "status": e["status"], "elapsed_ms": e["elapsed_ms"], "source": e["source"],
                        "created_at": e["created_at"]
                    }))
                    .collect::<Vec<_>>()))
            }
            "get_history_entry" => {
                let id = str_arg(args, "id").ok_or("\"id\" is required")?;
                let h = self.backend.get("/history?limit=500").await?;
                let mut entry = h
                    .as_array()
                    .and_then(|a| a.iter().find(|e| e["id"] == id))
                    .cloned()
                    .ok_or_else(|| format!("no history entry {id} among the 500 most recent"))?;
                let (body, truncated) = truncate(entry["response"]["body"].as_str().unwrap_or(""));
                entry["response"]["body"] = json!(body);
                if truncated {
                    entry["response"]["body_truncated"] = json!(true);
                }
                Ok(entry)
            }
            "import_collection" => {
                let content = str_arg(args, "content").ok_or("\"content\" is required")?.to_string();
                let path = match str_arg(args, "format") {
                    Some("postman") => "/import/postman",
                    Some("openapi") => "/import/openapi",
                    _ => return Err("\"format\" must be postman or openapi".into()),
                };
                let c = self.backend.call(reqwest::Method::POST, path, Payload::Raw(content)).await?;
                let mut reqs = Vec::new();
                collect_requests(c["items"].as_array().map(Vec::as_slice).unwrap_or(&[]), "", &mut reqs);
                Ok(json!({ "id": c["id"], "name": c["name"], "requests": reqs.len() }))
            }
            "generate_snippet" => {
                let request = build_request(args)?;
                let target = str_arg(args, "language").ok_or("\"language\" is required")?;
                self.backend.post("/codegen", json!({ "target": target, "request": request })).await
            }
            _ => Err(format!("unknown tool: {name}")),
        }
    }

    async fn resolve_env_id(&mut self, args: &Value) -> Result<Value, String> {
        let Some(key) = str_arg(args, "environment") else { return Ok(Value::Null) };
        let envs = self.backend.get("/environments").await?;
        let env = pick(envs.as_array().map(Vec::as_slice).unwrap_or(&[]), key, "environment")?;
        Ok(env["id"].clone())
    }

    async fn run_collection(&mut self, args: &Value) -> Result<Value, String> {
        let key = str_arg(args, "collection").ok_or("\"collection\" is required")?;
        let stop = args["stop_on_failure"].as_bool().unwrap_or(false);
        let env = self.resolve_env_id(args).await?;
        let cols = self.backend.get("/collections").await?;
        let col = pick(cols.as_array().map(Vec::as_slice).unwrap_or(&[]), key, "collection")?.clone();
        let mut reqs = Vec::new();
        collect_requests(col["items"].as_array().map(Vec::as_slice).unwrap_or(&[]), "", &mut reqs);
        if reqs.is_empty() {
            return Err(format!("collection \"{}\" has no requests", col["name"].as_str().unwrap_or(key)));
        }

        let (mut passed, mut failed, mut stopped) = (0usize, 0usize, false);
        let mut results = Vec::new();
        for (folder, request) in reqs {
            let label = json!({ "folder": folder, "name": request["name"], "method": request["method"], "url": request["url"] });
            let outcome = self
                .backend
                .post("/execute", json!({ "request": request, "environment_id": env, "source": self.source() }))
                .await;
            let bad = match outcome {
                Ok(reply) => {
                    let failing = tests_failed(&reply) + reply["script_error"].is_string() as usize;
                    let mut entry = label;
                    entry["status"] = reply["response"]["status"].clone();
                    entry["elapsed_ms"] = reply["response"]["elapsed_ms"].clone();
                    entry["history_id"] = reply["history_id"].clone();
                    if let Some(t) = reply["test_results"].as_array().filter(|t| !t.is_empty()) {
                        entry["tests"] = json!(t);
                    }
                    if reply["script_error"].is_string() {
                        entry["script_error"] = reply["script_error"].clone();
                    }
                    results.push(entry);
                    failing > 0
                }
                Err(e) => {
                    let mut entry = label;
                    entry["error"] = json!(e);
                    results.push(entry);
                    true
                }
            };
            if bad {
                failed += 1;
                if stop {
                    stopped = true;
                    break;
                }
            } else {
                passed += 1;
            }
        }
        Ok(json!({
            "collection": col["name"],
            "summary": { "requests_run": results.len(), "passed": passed, "failed": failed, "stopped_early": stopped },
            "results": results
        }))
    }

    async fn save_request(&mut self, args: &Value) -> Result<Value, String> {
        let key = str_arg(args, "collection").ok_or("\"collection\" is required")?;
        if str_arg(args, "name").is_none() {
            return Err("\"name\" is required".into());
        }
        let mut request = build_request(args)?;
        let cols = self.backend.get("/collections").await?;
        let list = cols.as_array().cloned().unwrap_or_default();
        // Reuse the collection when it exists; otherwise create one named after the argument.
        let mut col = match pick(&list, key, "collection") {
            Ok(c) => c.clone(),
            Err(_) if !list.iter().any(|c| c["name"].as_str().map(|n| n.to_lowercase().contains(&key.to_lowercase())).unwrap_or(false)) => {
                json!({ "id": Uuid::new_v4(), "name": key, "description": null, "items": [] })
            }
            Err(e) => return Err(e),
        };
        let path: Vec<&str> = str_arg(args, "folder").map(|f| f.split('/').filter(|p| !p.is_empty()).collect()).unwrap_or_default();
        if !col["items"].is_array() {
            col["items"] = json!([]);
        }
        let items = items_at(col["items"].as_array_mut().expect("items array"), &path);
        let name = request["name"].as_str().unwrap_or("").to_string();
        let existing = items.iter().position(|it| it.get("Request").and_then(|r| r["name"].as_str()) == Some(name.as_str()));
        let replaced = existing.is_some();
        match existing {
            Some(i) => {
                // Keep the stored id so references to the request stay valid.
                request["id"] = items[i]["Request"]["id"].clone();
                items[i] = json!({ "Request": request.clone() });
            }
            None => items.push(json!({ "Request": request.clone() })),
        }
        self.backend.post("/collections", col.clone()).await?;
        self.backend.post("/focus", json!({ "request": request, "source": self.source() })).await?;
        Ok(json!({
            "saved": true, "replaced": replaced, "collection": col["name"], "folder": path.join("/"), "request": name
        }))
    }

    async fn set_variable(&mut self, args: &Value) -> Result<Value, String> {
        let key = str_arg(args, "environment").ok_or("\"environment\" is required")?;
        let var = str_arg(args, "key").ok_or("\"key\" is required")?;
        let value = args["value"].as_str().ok_or("\"value\" is required")?;
        let secret = args["secret"].as_bool().unwrap_or(false);
        let envs = self.backend.get("/environments").await?;
        let list = envs.as_array().cloned().unwrap_or_default();
        let mut env = match pick(&list, key, "environment") {
            Ok(e) => e.clone(),
            Err(_) if !list.iter().any(|e| e["name"].as_str().map(|n| n.to_lowercase().contains(&key.to_lowercase())).unwrap_or(false)) => {
                json!({ "id": Uuid::new_v4(), "name": key, "variables": [] })
            }
            Err(e) => return Err(e),
        };
        if !env["variables"].is_array() {
            env["variables"] = json!([]);
        }
        let vars = env["variables"].as_array_mut().expect("variables array");
        let row = json!({ "key": var, "value": value, "enabled": true, "secret": secret });
        match vars.iter().position(|v| v["key"] == var) {
            Some(i) => vars[i] = row,
            None => vars.push(row),
        }
        self.backend.post("/environments", env.clone()).await?;
        Ok(json!({ "environment": env["name"], "key": var, "set": true }))
    }
}

fn tool_text(v: &Value, is_error: bool) -> Value {
    let text = match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server::new(McpOptions { host: "127.0.0.1".into(), port: 1, no_launch: true })
    }

    #[test]
    fn builds_requests() {
        let r = build_request(&json!({
            "method": "post", "url": "https://x.test/a",
            "headers": { "X-A": "1" }, "query": { "q": "v" },
            "body": { "k": 1 }, "auth": { "type": "bearer", "token": "t" },
            "test_script": "pm.test('x', () => {})"
        }))
        .unwrap();
        assert_eq!(r["method"], "POST");
        assert_eq!(r["name"], "POST https://x.test/a");
        assert_eq!(r["headers"][0], json!({ "key": "X-A", "value": "1", "enabled": true }));
        assert_eq!(r["query_params"][0]["key"], "q");
        assert_eq!(r["body"]["Raw"]["content_type"], "application/json");
        assert_eq!(r["body"]["Raw"]["body"], "{\"k\":1}");
        assert_eq!(r["auth"], json!({ "Bearer": { "token": "t" } }));
        assert_eq!(r["test_script"], "pm.test('x', () => {})");
    }

    #[test]
    fn builds_bodies_and_auth_variants() {
        let r = build_request(&json!({ "url": "u", "body": "plain text" })).unwrap();
        assert_eq!(r["body"]["Raw"]["content_type"], "text/plain");
        let r = build_request(&json!({ "url": "u", "body": "{\"a\":1}" })).unwrap();
        assert_eq!(r["body"]["Raw"]["content_type"], "application/json");
        let r = build_request(&json!({ "url": "u", "body": "x", "content_type": "text/csv" })).unwrap();
        assert_eq!(r["body"]["Raw"]["content_type"], "text/csv");
        let r = build_request(&json!({ "url": "u", "form_urlencoded": { "a": "b" } })).unwrap();
        assert_eq!(r["body"]["FormUrlEncoded"][0]["key"], "a");
        let r = build_request(&json!({ "url": "u", "form_data": [{ "key": "f", "value": "v" }] })).unwrap();
        assert_eq!(r["body"]["FormData"][0]["value"], "v");
        let r = build_request(&json!({ "url": "u", "auth": { "type": "basic", "username": "me", "password": "pw" } })).unwrap();
        assert_eq!(r["auth"]["Basic"]["username"], "me");
        let r = build_request(&json!({ "url": "u", "auth": { "type": "api_key", "key": "k", "value": "v", "in": "query" } })).unwrap();
        assert_eq!(r["auth"]["ApiKey"]["in_header"], false);
        assert_eq!(build_request(&json!({ "url": "u" })).unwrap()["body"], "None");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(build_request(&json!({})).unwrap_err().contains("url"));
        assert!(build_request(&json!({ "url": "u", "method": "FETCH" })).unwrap_err().contains("unsupported method"));
        assert!(build_request(&json!({ "url": "u", "auth": { "type": "magic" } })).is_err());
        assert!(build_request(&json!({ "url": "u", "headers": 5 })).is_err());
    }

    #[test]
    fn picks_by_id_name_or_fragment() {
        let list = vec![
            json!({ "id": "1", "name": "Users API" }),
            json!({ "id": "2", "name": "Orders API" }),
            json!({ "id": "3", "name": "Users Admin" }),
        ];
        assert_eq!(pick(&list, "2", "collection").unwrap()["name"], "Orders API");
        assert_eq!(pick(&list, "users api", "collection").unwrap()["id"], "1");
        assert_eq!(pick(&list, "orders", "collection").unwrap()["id"], "2");
        assert!(pick(&list, "users", "collection").unwrap_err().contains("several"));
        assert!(pick(&list, "nope", "collection").unwrap_err().contains("Available"));
    }

    #[test]
    fn walks_and_edits_folders() {
        let mut items = vec![json!({ "Request": { "name": "top" } })];
        {
            let inner = items_at(&mut items, &["A", "B"]);
            inner.push(json!({ "Request": { "name": "deep" } }));
        }
        // Reusing an existing folder must not duplicate it.
        items_at(&mut items, &["A"]).push(json!({ "Request": { "name": "mid" } }));
        assert_eq!(items.len(), 2);
        let mut all = Vec::new();
        collect_requests(&items, "", &mut all);
        let seen: Vec<(String, String)> =
            all.iter().map(|(f, r)| (f.clone(), r["name"].as_str().unwrap().to_string())).collect();
        let want = [("", "top"), ("A/B", "deep"), ("A", "mid")];
        let want: Vec<(String, String)> = want.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        assert_eq!(seen, want);
    }

    #[test]
    fn truncates_long_bodies_on_char_boundaries() {
        let long = "é".repeat(MAX_BODY_CHARS + 5);
        let (t, cut) = truncate(&long);
        assert!(cut);
        assert_eq!(t.chars().count(), MAX_BODY_CHARS);
        assert!(!truncate("short").1);
    }

    #[test]
    fn summarizes_replies() {
        let req = json!({ "method": "GET", "url": "https://x", "name": "n" });
        let reply = json!({
            "response": { "status": 200, "elapsed_ms": 5, "body": "ok",
                "headers": [{ "key": "Set-Cookie", "value": "a" }, { "key": "Set-Cookie", "value": "b" }] },
            "history_id": "h1",
            "test_results": [{ "name": "t", "passed": false, "message": "no" }],
        });
        let s = summarize_reply(&req, &reply);
        assert_eq!(s["status"], 200);
        assert_eq!(s["headers"]["Set-Cookie"], "a, b");
        assert_eq!(s["tests"][0]["passed"], false);
        assert_eq!(tests_failed(&reply), 1);
        assert!(s.get("body_truncated").is_none());
    }

    #[tokio::test]
    async fn speaks_json_rpc() {
        let mut s = server();
        let init = s
            .handle(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2024-11-05", "clientInfo": { "name": "Claude Code" } } }))
            .await
            .unwrap();
        assert_eq!(init["id"], 1);
        assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(init["result"]["serverInfo"]["name"], "rustman");
        assert_eq!(s.agent, "Claude Code");

        // Unknown protocol versions fall back to ours.
        let init = s.handle(json!({ "id": 2, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } })).await.unwrap();
        assert_eq!(init["result"]["protocolVersion"], SUPPORTED_PROTOCOLS[0]);

        // Notifications get no reply.
        assert!(s.handle(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.is_none());

        let list = s.handle(json!({ "id": 3, "method": "tools/list" })).await.unwrap();
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        for n in ["send_request", "run_collection", "save_request", "list_history", "open_request"] {
            assert!(names.contains(&n), "missing tool {n}");
        }
        for t in list["result"]["tools"].as_array().unwrap() {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        }

        let bad = s.handle(json!({ "id": 4, "method": "nope" })).await.unwrap();
        assert_eq!(bad["error"]["code"], -32601);
        let bad = s.handle(json!({ "id": 5, "method": "tools/call", "params": { "name": "nope" } })).await.unwrap();
        assert_eq!(bad["error"]["code"], -32602);
        assert_eq!(s.handle(json!({ "id": 6, "method": "ping" })).await.unwrap()["result"], json!({}));
    }

    #[tokio::test]
    async fn tool_errors_are_reported_to_the_model() {
        let mut s = server();
        // Invalid arguments are tool errors (isError), not protocol errors.
        let r = s
            .handle(json!({ "id": 1, "method": "tools/call", "params": { "name": "send_request", "arguments": {} } }))
            .await
            .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("url"));

        // The backend is not running and launching is disabled.
        let r = s
            .handle(json!({ "id": 2, "method": "tools/call", "params": { "name": "list_collections", "arguments": {} } }))
            .await
            .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("not running"));
    }
}
