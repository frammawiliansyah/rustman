//! Local backend server + IPC bridge for the rustman desktop UI.
//!
//! Serves a REST API under `/api/*` and the bundled web frontend
//! (`./frontend/dist`, or a placeholder page when it has not been built).

#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message as AxMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::middleware::{self, Next};
use axum::http::HeaderMap;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rustman_core::importers::{import_openapi, import_postman};
use rustman_core::{
    generate_snippet, HtmlDocGenerator, MarkdownDocGenerator, GraphQLIntrospector, HeaderPair, SseClient, WebSocketSession, Collection, Cookie, CookieJar, Environment, HistoryEntry, HttpExecutor, HttpRequest, HttpResponse, RustmanError,
    SnippetTarget, SqliteStorage, TimingMetrics, VariableResolver, ScriptRuntime, ScriptScope, AssertionResult,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use uuid::Uuid;

const DEFAULT_PORT: u16 = 51730;
const HISTORY_LIMIT: usize = 50;
const HISTORY_MAX: usize = 500;

const PLACEHOLDER: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>rustman</title></head>\
<body style=\"font-family:sans-serif;padding:2rem\"><h1>rustman backend is running</h1>\
<p>The web frontend has not been built. Run <code>cd frontend &amp;&amp; pnpm install &amp;&amp; pnpm build</code> \
and restart, or use the <code>/api/*</code> endpoints directly.</p></body></html>";

#[derive(Clone)]
struct AppState {
    storage: Arc<SqliteStorage>,
    executor: HttpExecutor,
    jar: Arc<CookieJar>,
    /// Live events for the UI (agent activity, data changes). See `/api/events`.
    events: tokio::sync::broadcast::Sender<serde_json::Value>,
}

impl AppState {
    fn emit(&self, event: serde_json::Value) {
        // No subscribers is fine: nobody is watching.
        let _ = self.events.send(event);
    }

    fn changed(&self, kind: &str) {
        self.emit(json!({ "type": "data_changed", "kind": kind }));
    }
}

/// Access control for the local backend.
#[derive(Clone)]
struct Security {
    /// Required on every `/api` call as `Authorization: Bearer <token>` or `?token=<token>`.
    /// `None` disables the check and is only meant for tests.
    token: Option<Arc<str>>,
    /// When the server listens on loopback only, reject requests whose `Host` is not
    /// loopback. This blocks DNS rebinding, where a web page tricks the browser into
    /// talking to the local server under its own domain name.
    loopback_only: bool,
}

impl Security {
    #[cfg(test)]
    fn disabled() -> Self {
        Self { token: None, loopback_only: false }
    }
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn host_is_loopback(headers: &HeaderMap) -> bool {
    let Some(h) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else { return true };
    let host = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn request_token(req: &Request) -> Option<String> {
    if let Some(v) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(t) = v.strip_prefix("Bearer ") {
            return Some(t.trim().to_string());
        }
    }
    req.uri().query()?.split('&').find_map(|kv| kv.strip_prefix("token=")).map(str::to_string)
}

async fn guard(State(sec): State<Security>, req: Request, next: Next) -> Response {
    if sec.loopback_only && !host_is_loopback(req.headers()) {
        return ApiError(StatusCode::FORBIDDEN, "host not allowed".into()).into_response();
    }
    if let Some(expected) = &sec.token {
        if req.uri().path().starts_with("/api") {
            let ok = request_token(&req).map(|t| ct_eq(&t, expected)).unwrap_or(false);
            if !ok {
                return ApiError(StatusCode::UNAUTHORIZED, "missing or invalid access token".into()).into_response();
            }
        }
    }
    next.run(req).await
}

// ---------- errors ----------

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<RustmanError> for ApiError {
    fn from(e: RustmanError) -> Self {
        let status = match &e {
            RustmanError::InvalidUrl(_)
            | RustmanError::InvalidHeader(_)
            | RustmanError::InvalidRequest(_)
            | RustmanError::SerializationError(_) => StatusCode::BAD_REQUEST,
            RustmanError::Timeout => StatusCode::GATEWAY_TIMEOUT,
            RustmanError::NetworkError(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(status, e.to_string())
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

// ---------- DTOs ----------

#[derive(Deserialize)]
struct ExecuteBody {
    request: HttpRequest,
    #[serde(default)]
    environment_id: Option<Uuid>,
    /// Set by AI agents (through `rustman mcp`). The UI then follows along live, and the
    /// history entry is tagged with the agent.
    #[serde(default)]
    source: Option<SourceInfo>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
struct SourceInfo {
    agent: String,
    #[serde(default)]
    session: Option<String>,
}

impl SourceInfo {
    fn label(&self) -> String {
        format!("agent:{}", self.agent)
    }
}

#[derive(Deserialize)]
struct FocusBody {
    request: HttpRequest,
    #[serde(default)]
    source: Option<SourceInfo>,
}

#[derive(Deserialize)]
struct HistoryQuery {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Serialize)]
struct ExecuteReply {
    response: HttpResponse,
    timing: TimingMetrics,
    history_id: Option<Uuid>,
    /// Results of `pm.test(...)` calls in the request's test script.
    #[serde(default)]
    test_results: Vec<AssertionResult>,
    /// Set when the test script errored.
    #[serde(default)]
    script_error: Option<String>,
    /// `postman.setNextRequest`: absent = not called, `null` = terminate run, string = target name/id.
    #[serde(default)]
    next_request: Option<Option<String>>,
}

#[derive(Deserialize)]
struct CodegenBody {
    target: SnippetTarget,
    request: HttpRequest,
}

#[derive(Serialize)]
struct CodegenReply {
    code: String,
}

#[derive(Deserialize)]
struct DocsRenderBody {
    collection: Collection,
    format: DocsFormat,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum DocsFormat {
    Html,
    Markdown,
}

#[derive(Serialize)]
struct DocsRenderReply {
    content: String,
}

#[derive(Deserialize)]
struct GraphqlIntrospectBody {
    url: String,
    #[serde(default)]
    headers: Vec<HeaderPair>,
}

// ---------- handlers ----------

async fn graphql_introspect(Json(body): Json<GraphqlIntrospectBody>) -> ApiResult<serde_json::Value> {
    Ok(Json(GraphQLIntrospector::introspect(&body.url, &body.headers).await?))
}

async fn docs_render(Json(body): Json<DocsRenderBody>) -> Json<DocsRenderReply> {
    let content = match body.format {
        DocsFormat::Html => HtmlDocGenerator::generate(&body.collection),
        DocsFormat::Markdown => MarkdownDocGenerator::generate(&body.collection),
    };
    Json(DocsRenderReply { content })
}

async fn codegen(Json(body): Json<CodegenBody>) -> Json<CodegenReply> {
    Json(CodegenReply { code: generate_snippet(body.target, &body.request) })
}

async fn execute(State(st): State<AppState>, Json(body): Json<ExecuteBody>) -> ApiResult<ExecuteReply> {
    let Some(src) = body.source.clone() else {
        return run_execute(&st, &body, None).await.map(Json);
    };
    let run_id = Uuid::new_v4();
    st.emit(json!({
        "type": "request_started", "run_id": run_id, "source": src,
        "request": body.request, "environment_id": body.environment_id,
    }));
    let result = run_execute(&st, &body, Some(&src.label())).await;
    match &result {
        Ok(reply) => st.emit(json!({ "type": "request_finished", "run_id": run_id, "source": src, "reply": reply })),
        Err(e) => st.emit(json!({ "type": "request_failed", "run_id": run_id, "source": src, "error": e.1 })),
    }
    st.changed("history");
    result.map(Json)
}

async fn run_execute(st: &AppState, body: &ExecuteBody, source: Option<&str>) -> Result<ExecuteReply, ApiError> {
    let mut resolver = VariableResolver::new();
    if let Some(id) = body.environment_id {
        let env = st
            .storage
            .get_environment(&id)?
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("environment {id} not found")))?;
        resolver = resolver.with_environment_model(&env);
    }
    let resolved = resolver.resolve_request(&body.request);
    let (response, timing) = st.executor.execute(&resolved).await?;
    persist_cookies(st);
    // A history write failure must not hide a successful response.
    let history_id = match st.storage.save_history_with_source(&resolved, &response, source) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!("failed to save history: {e}");
            None
        }
    };
    let mut test_results = Vec::new();
    let mut script_error = None;
    let mut next_request = None;
    if let Some(script) = body.request.test_script.as_deref().filter(|s| !s.trim().is_empty()) {
        let scope = ScriptScope { environment: resolver.environment_vars(), ..Default::default() };
        match ScriptRuntime::new().run_test(script, &scope, &response) {
            Ok(out) => {
                test_results = out.assertions;
                next_request = out.next_request;
            }
            Err(e) => script_error = Some(e.to_string()),
        }
    }
    Ok(ExecuteReply { response, timing, history_id, test_results, script_error, next_request })
}

/// Lets an agent show a request in the UI without sending it.
async fn focus(State(st): State<AppState>, Json(body): Json<FocusBody>) -> StatusCode {
    st.emit(json!({ "type": "focus_request", "request": body.request, "source": body.source }));
    StatusCode::NO_CONTENT
}

/// Server-sent events for the UI: agent activity and data changes.
async fn events_stream(
    State(st): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    use tokio::sync::broadcast::error::RecvError;
    let rx = st.events.subscribe();
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(v) => return Some((Ok(Event::default().data(v.to_string())), rx)),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn persist_cookies(st: &AppState) {
    if let Err(e) = st.storage.save_cookies(&st.jar) {
        tracing::warn!("failed to persist cookies: {e}");
    }
}

#[derive(Deserialize)]
struct CookieQuery {
    domain: String,
    name: String,
}

async fn list_cookies(State(st): State<AppState>) -> Json<Vec<Cookie>> {
    Json(st.jar.get_cookies())
}

async fn upsert_cookie(State(st): State<AppState>, Json(c): Json<Cookie>) -> Result<Json<Cookie>, ApiError> {
    if c.name.trim().is_empty() || c.domain.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "cookie name and domain are required".into()));
    }
    st.jar.add_cookie(c.clone());
    persist_cookies(&st);
    st.changed("cookies");
    Ok(Json(c))
}

async fn delete_cookie(State(st): State<AppState>, Query(q): Query<CookieQuery>) -> Result<StatusCode, ApiError> {
    if st.jar.delete_cookie(&q.domain, &q.name) {
        persist_cookies(&st);
        st.changed("cookies");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, format!("cookie {} for {} not found", q.name, q.domain)))
    }
}

async fn list_collections(State(st): State<AppState>) -> ApiResult<Vec<Collection>> {
    Ok(Json(st.storage.list_collections()?))
}

async fn save_collection(State(st): State<AppState>, Json(c): Json<Collection>) -> ApiResult<Collection> {
    st.storage.save_collection(&c)?;
    st.changed("collections");
    Ok(Json(c))
}

async fn delete_collection(State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    if st.storage.delete_collection(&id)? {
        st.changed("collections");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, format!("collection {id} not found")))
    }
}

async fn list_environments(State(st): State<AppState>) -> ApiResult<Vec<Environment>> {
    Ok(Json(st.storage.list_environments()?))
}

async fn save_environment(State(st): State<AppState>, Json(e): Json<Environment>) -> ApiResult<Environment> {
    st.storage.save_environment(&e)?;
    st.changed("environments");
    Ok(Json(e))
}

async fn delete_environment(State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    if st.storage.delete_environment(&id)? {
        st.changed("environments");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, format!("environment {id} not found")))
    }
}

async fn list_history(State(st): State<AppState>, Query(q): Query<HistoryQuery>) -> ApiResult<Vec<HistoryEntry>> {
    let limit = q.limit.unwrap_or(HISTORY_LIMIT).min(HISTORY_MAX);
    Ok(Json(st.storage.list_history(limit)?))
}

async fn import_postman_handler(State(st): State<AppState>, raw: String) -> ApiResult<Collection> {
    let c = import_postman(&raw).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    st.storage.save_collection(&c)?;
    st.changed("collections");
    Ok(Json(c))
}

async fn import_openapi_handler(State(st): State<AppState>, raw: String) -> ApiResult<Collection> {
    let c = import_openapi(&raw).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    st.storage.save_collection(&c)?;
    st.changed("collections");
    Ok(Json(c))
}

async fn placeholder() -> Html<&'static str> {
    Html(PLACEHOLDER)
}

// ---------- app ----------

fn is_local_origin(origin: &HeaderValue) -> bool {
    let Ok(o) = origin.to_str() else { return false };
    let rest = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://"));
    match rest {
        Some(host) => ["localhost", "127.0.0.1", "[::1]"]
            .iter()
            .any(|h| host == *h || host.starts_with(&format!("{h}:"))),
        None => false,
    }
}

// ---------- realtime bridges ----------

#[derive(Deserialize)]
struct BridgeQuery {
    target: String,
    /// Optional JSON-encoded array of `{key, value, enabled}` headers for the upstream request.
    #[serde(default)]
    headers: Option<String>,
}

impl BridgeQuery {
    fn header_pairs(&self) -> Result<Vec<HeaderPair>, ApiError> {
        match &self.headers {
            None => Ok(Vec::new()),
            Some(h) => serde_json::from_str(h)
                .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid headers JSON: {e}"))),
        }
    }
}

/// Browsers don't apply CORS to WebSocket upgrades, so reject non-local origins explicitly.
fn check_ws_origin(headers: &HeaderMap) -> Result<(), ApiError> {
    match headers.get(header::ORIGIN) {
        Some(o) if !is_local_origin(o) => Err(ApiError(StatusCode::FORBIDDEN, "origin not allowed".into())),
        _ => Ok(()),
    }
}

async fn ws_connect(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Query(q): Query<BridgeQuery>,
) -> Result<Response, ApiError> {
    check_ws_origin(&headers)?;
    let pairs = q.header_pairs()?;
    // Connect upstream first so failures surface as an HTTP error instead of a silent close.
    let (session, rx) = WebSocketSession::connect(&q.target, &pairs).await?;
    Ok(ws.on_upgrade(move |socket| ws_relay(socket, session, rx)))
}

/// Browser -> upstream: text/binary frames. Upstream -> browser: JSON-encoded `WsMessage`s
/// (both directions, so the UI can render a full message log).
async fn ws_relay(
    socket: WebSocket,
    session: WebSocketSession,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<rustman_core::WsMessage>,
) {
    use futures_util::{SinkExt, StreamExt};
    let (mut to_browser, mut from_browser) = socket.split();
    loop {
        tokio::select! {
            up = rx.recv() => match up {
                Some(m) => {
                    let Ok(text) = serde_json::to_string(&m) else { continue };
                    if to_browser.send(AxMessage::Text(text)).await.is_err() { break; }
                }
                None => break,
            },
            down = from_browser.next() => match down {
                Some(Ok(AxMessage::Text(t))) => { if session.send_text(&t).await.is_err() { break; } }
                Some(Ok(AxMessage::Binary(b))) => { if session.send_binary(&b).await.is_err() { break; } }
                Some(Ok(AxMessage::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
    let _ = session.close().await;
    let _ = to_browser.close().await;
}

async fn sse_connect(
    headers: HeaderMap,
    Query(q): Query<BridgeQuery>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    check_ws_origin(&headers)?;
    let pairs = q.header_pairs()?;
    let rx = SseClient::connect(&q.target, &pairs).await?;
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        let ev = rx.recv().await?;
        let mut out = Event::default().data(ev.data);
        if let Some(id) = ev.id {
            out = out.id(id);
        }
        if let Some(name) = ev.event {
            out = out.event(name);
        }
        Some((Ok(out), rx))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Serves `index.html` with the access token embedded, so only whoever can load the page
/// from this host (the desktop window, or a browser on this machine) gets the token.
async fn index_page(State((index, token)): State<(PathBuf, Option<Arc<str>>)>) -> Response {
    let Ok(html) = tokio::fs::read_to_string(&index).await else {
        return (StatusCode::NOT_FOUND, "index.html not found").into_response();
    };
    let html = match token {
        Some(t) => {
            let script = format!("<script>window.__RUSTMAN_TOKEN__=\"{t}\";</script>");
            match html.find("</head>") {
                Some(i) => format!("{}{}{}", &html[..i], script, &html[i..]),
                None => format!("{script}{html}"),
            }
        }
        None => html,
    };
    let mut res = Html(html).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn build_app(storage: Arc<SqliteStorage>, dist: PathBuf, security: Security) -> Router {
    let jar = Arc::new(storage.load_cookie_jar().unwrap_or_else(|e| {
        tracing::warn!("failed to load cookies: {e}");
        CookieJar::new()
    }));
    let executor = HttpExecutor::new().with_cookie_jar(jar.clone());
    let (events, _) = tokio::sync::broadcast::channel(256);
    let state = AppState { storage, executor, jar, events };

    let api = Router::new()
        .route("/execute", post(execute))
        .route("/focus", post(focus))
        .route("/events", get(events_stream))
        .route("/codegen", post(codegen))
        .route("/docs/render", post(docs_render))
        .route("/graphql/introspect", post(graphql_introspect))
        .route("/collections", get(list_collections).post(save_collection))
        .route("/collections/:id", delete(delete_collection))
        .route("/environments", get(list_environments).post(save_environment))
        .route("/environments/:id", delete(delete_environment))
        .route("/cookies", get(list_cookies).post(upsert_cookie).delete(delete_cookie))
        .route("/history", get(list_history))
        .route("/import/postman", post(import_postman_handler))
        .route("/import/openapi", post(import_openapi_handler))
        .route("/ws/connect", get(ws_connect))
        .route("/sse/connect", get(sse_connect))
        .with_state(state);

    // Only local origins may call the API from a browser: it can fire arbitrary requests.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|o, _| is_local_origin(o)))
        .allow_methods(tower_http::cors::Any)
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    let app = Router::new().nest("/api", api);
    let app = if dist.join("index.html").is_file() {
        let index = Router::new()
            .fallback(index_page)
            .with_state((dist.join("index.html"), security.token.clone()));
        app.fallback_service(ServeDir::new(&dist).append_index_html_on_directories(false).fallback(index))
    } else {
        tracing::warn!("{} not found; serving placeholder page", dist.display());
        app.fallback(placeholder)
    };
    app.layer(middleware::from_fn_with_state(security, guard)).layer(cors)
}

fn db_path() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let dir = home.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".rustman");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("cannot create {}: {e}", dir.display());
    }
    dir.join("rustman.db")
}

/// Rustman desktop: local backend + native window.
#[derive(clap::Parser, Debug)]
#[command(name = "rustman-desktop", version, about)]
struct Cli {
    /// Run the backend server only, without opening a native window.
    #[arg(long)]
    headless: bool,
    /// Port to listen on (falls back to $RUSTMAN_PORT, then 51730).
    #[arg(short, long)]
    port: Option<u16>,
    /// Address to bind to.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

#[cfg(target_os = "linux")]
fn has_display() -> bool {
    std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok()
}

// macOS and Windows always have a window server available.
#[cfg(not(target_os = "linux"))]
fn has_display() -> bool {
    true
}

/// Locate the built frontend: `RUSTMAN_DIST`, then `./frontend/dist`, then the
/// locations used by packaged builds (next to the executable, or the macOS
/// `.app` bundle's `Contents/Resources/dist`).
fn resolve_dist() -> PathBuf {
    if let Some(p) = std::env::var_os("RUSTMAN_DIST") {
        return PathBuf::from(p);
    }
    let local = PathBuf::from("./frontend/dist");
    if local.join("index.html").is_file() {
        return local;
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)) {
        for cand in [dir.join("dist"), dir.join("../Resources/dist")] {
            if cand.join("index.html").is_file() {
                return cand;
            }
        }
    }
    local
}

/// Window and taskbar icon. macOS takes its icon from the app bundle instead, where this is ignored.
fn window_icon() -> Option<tao::window::Icon> {
    let mut reader = png::Decoder::new(&include_bytes!("../../../assets/icon/icon-256.png")[..]).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    tao::window::Icon::from_rgba(buf[..info.buffer_size()].to_vec(), info.width, info.height).ok()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use clap::Parser;
    tracing_subscriber::fmt().init();
    let cli = Cli::parse();

    let path = std::env::var_os("RUSTMAN_DB").map(PathBuf::from).unwrap_or_else(db_path);
    let storage = Arc::new(SqliteStorage::new(&path)?);
    let dist = resolve_dist();
    let port = cli
        .port
        .or_else(|| std::env::var("RUSTMAN_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(DEFAULT_PORT);
    let host = cli.host;

    let rt = tokio::runtime::Runtime::new()?;
    // Bind before opening the window so the webview never races the server.
    let listener = rt.block_on(tokio::net::TcpListener::bind((host.as_str(), port)))?;
    tracing::info!("rustman db: {}", path.display());
    let data_dir = rustman_core::storage::token::data_dir();
    let token = rustman_core::storage::token::load_or_create_token(&data_dir)?;
    let loopback_only = matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1");
    let security = Security { token: Some(token.into()), loopback_only };
    let app = build_app(storage, dist, security);

    if cli.headless || !has_display() {
        println!("Rustman desktop backend running in headless mode at http://{host}:{port}");
        rt.block_on(async { axum::serve(listener, app).await })?;
        return Ok(());
    }

    std::thread::spawn(move || {
        if let Err(e) = rt.block_on(async { axum::serve(listener, app).await }) {
            tracing::error!("server error: {e}");
        }
    });

    let event_loop = tao::event_loop::EventLoop::new();
    let window = tao::window::WindowBuilder::new()
        .with_title("Rustman - API Client")
        .with_window_icon(window_icon())
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(960.0, 600.0))
        .build(&event_loop)?;
    // 0.0.0.0 / :: are bind-only addresses; connect via loopback instead.
    let url_host = match host.as_str() {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" => "[::1]".to_string(),
        h if h.contains(':') && !h.starts_with('[') => format!("[{h}]"),
        h => h.to_string(),
    };
    let builder = wry::WebViewBuilder::new().with_url(&format!("http://{url_host}:{port}/"));
    // On Linux, wry only accepts an Xlib handle via `build(&window)`, so it fails with
    // `UnsupportedWindowHandle` on Wayland (and tao's GTK window). Build into tao's GTK
    // container instead, which works on both X11 and Wayland.
    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or("tao window has no default GTK vbox")?;
        builder.build_gtk(vbox)?
    };
    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = tao::event_loop::ControlFlow::Wait;
        if let tao::event::Event::WindowEvent { event: tao::event::WindowEvent::CloseRequested, .. } = event {
            *control_flow = tao::event_loop::ControlFlow::Exit;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn spawn(dist: PathBuf) -> String {
        let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, build_app(storage, dist, Security::disabled())).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn execute_interpolates_env_and_records_history() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();

        let env = Environment {
            id: Uuid::new_v4(),
            name: "local".into(),
            variables: vec![rustman_core::Variable {
                key: "host".into(),
                value: base.clone(),
                enabled: true,
                secret: false,
            }],
        };
        let r = c.post(format!("{base}/api/environments")).json(&env).send().await.unwrap();
        assert!(r.status().is_success());

        // Call our own /api/history through the executor, with {{host}} resolved.
        let req = HttpRequest::new("h", rustman_core::HttpMethod::GET, "{{host}}/api/history");
        let r: serde_json::Value = c
            .post(format!("{base}/api/execute"))
            .json(&json!({ "request": req, "environment_id": env.id }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(r["response"]["status"], 200);
        assert!(r["history_id"].is_string());

        let h: Vec<serde_json::Value> =
            c.get(format!("{base}/api/history")).send().await.unwrap().json().await.unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0]["url"], format!("{base}/api/history"));
    }

    #[tokio::test]
    async fn collection_crud_and_import() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();

        let pm = r#"{"info":{"name":"Demo","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
            "item":[{"name":"r","request":{"method":"GET","url":"https://example.com"}}]}"#;
        let imported: Collection =
            c.post(format!("{base}/api/import/postman")).body(pm).send().await.unwrap().json().await.unwrap();
        assert_eq!(imported.name, "Demo");

        let list: Vec<Collection> = c.get(format!("{base}/api/collections")).send().await.unwrap().json().await.unwrap();
        assert_eq!(list.len(), 1);

        let bad = c.post(format!("{base}/api/import/openapi")).body("not json").send().await.unwrap();
        assert_eq!(bad.status(), 400);

        let del = c.delete(format!("{base}/api/collections/{}", imported.id)).send().await.unwrap();
        assert_eq!(del.status(), 204);
        let del = c.delete(format!("{base}/api/collections/{}", imported.id)).send().await.unwrap();
        assert_eq!(del.status(), 404);
    }

    #[tokio::test]
    async fn codegen_endpoint() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();
        let req = HttpRequest::new("g", rustman_core::HttpMethod::GET, "https://example.com");
        let r = c
            .post(format!("{base}/api/codegen"))
            .json(&json!({ "target": "curl", "request": req }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let v: serde_json::Value = r.json().await.unwrap();
        assert_eq!(v["code"], "curl -X GET 'https://example.com'");
        let bad = c
            .post(format!("{base}/api/codegen"))
            .json(&json!({ "target": "cobol", "request": req }))
            .send()
            .await
            .unwrap();
        assert!(bad.status().is_client_error());
    }

    #[tokio::test]
    async fn docs_render_endpoint() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();
        let mut col = Collection {
            id: Uuid::new_v4(),
            name: "Docs Test".into(),
            description: None,
            items: vec![],
        };
        col.items.push(rustman_core::Item::Request(HttpRequest::new(
            "ping",
            rustman_core::HttpMethod::GET,
            "https://example.com/ping",
        )));
        for (fmt, needle) in [("html", "<!DOCTYPE html>"), ("markdown", "# Docs Test")] {
            let r = c
                .post(format!("{base}/api/docs/render"))
                .json(&json!({ "collection": col, "format": fmt }))
                .send()
                .await
                .unwrap();
            assert!(r.status().is_success());
            let v: serde_json::Value = r.json().await.unwrap();
            assert!(v["content"].as_str().unwrap().contains(needle));
        }
        let bad = c
            .post(format!("{base}/api/docs/render"))
            .json(&json!({ "collection": col, "format": "pdf" }))
            .send()
            .await
            .unwrap();
        assert!(bad.status().is_client_error());
    }

    #[tokio::test]
    async fn cookie_crud() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();
        let r = c
            .post(format!("{base}/api/cookies"))
            .json(&json!({ "name": "sid", "value": "1", "domain": "example.com" }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let list: Vec<serde_json::Value> =
            c.get(format!("{base}/api/cookies")).send().await.unwrap().json().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["path"], "/");
        let url = format!("{base}/api/cookies?domain=example.com&name=sid");
        assert_eq!(c.delete(&url).send().await.unwrap().status(), 204);
        assert_eq!(c.delete(&url).send().await.unwrap().status(), 404);
    }

    #[tokio::test]
    async fn serves_placeholder_without_dist() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let body = reqwest::get(&base).await.unwrap().text().await.unwrap();
        assert!(body.contains("rustman backend is running"));
    }

    #[test]
    fn window_icon_decodes() {
        assert!(window_icon().is_some());
    }

    #[test]
    fn cors_only_allows_local_origins() {
        assert!(is_local_origin(&HeaderValue::from_static("http://localhost:5173")));
        assert!(is_local_origin(&HeaderValue::from_static("http://127.0.0.1:51730")));
        assert!(!is_local_origin(&HeaderValue::from_static("https://evil.com")));
        assert!(!is_local_origin(&HeaderValue::from_static("http://localhost.evil.com")));
    }

    #[tokio::test]
    async fn sse_proxy_streams_upstream_events() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let up = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let up_addr = up.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = up.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf).await;
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\nevent: tick\ndata: hi\n\n")
                .await
                .unwrap();
        });
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let body = reqwest::get(format!("{base}/api/sse/connect?target=http://{up_addr}"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(body.contains("event: tick") && body.contains("data: hi"), "{body}");

        let bad = reqwest::get(format!("{base}/api/sse/connect?target=nope")).await.unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn ws_bridge_relays_and_rejects_foreign_origin() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
        let up = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let up_addr = up.local_addr().unwrap();
        tokio::spawn(async move {
            let (s, _) = up.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(s).await.unwrap();
            let (mut tx, mut rx) = ws.split();
            while let Some(Ok(m)) = rx.next().await {
                if m.is_text() {
                    tx.send(m).await.unwrap();
                }
            }
        });
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let ws_base = base.replace("http://", "ws://");

        let (mut c, _) =
            tokio_tungstenite::connect_async(format!("{ws_base}/api/ws/connect?target=ws://{up_addr}")).await.unwrap();
        c.send(Message::Text("ping".into())).await.unwrap();
        let mut seen_in = false;
        for _ in 0..2 {
            let Some(Ok(Message::Text(t))) = c.next().await else { panic!("no frame") };
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert_eq!(v["content"], "ping");
            seen_in |= v["direction"] == "In";
        }
        assert!(seen_in);

        let mut req = format!("{ws_base}/api/ws/connect?target=ws://{up_addr}").into_client_request().unwrap();
        req.headers_mut().insert("origin", "http://evil.example".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(req).await.is_err());
    }

    async fn spawn_secure(dist: PathBuf, loopback_only: bool) -> String {
        let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let security = Security { token: Some("s3cret".into()), loopback_only };
        tokio::spawn(async move { axum::serve(listener, build_app(storage, dist, security)).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn api_requires_token() {
        let base = spawn_secure(PathBuf::from("/nonexistent"), true).await;
        let c = reqwest::Client::new();
        let url = format!("{base}/api/collections");
        assert_eq!(c.get(&url).send().await.unwrap().status(), 401);
        assert_eq!(c.get(&url).bearer_auth("wrong").send().await.unwrap().status(), 401);
        assert_eq!(c.get(&url).bearer_auth("s3cret").send().await.unwrap().status(), 200);
        assert_eq!(c.get(format!("{url}?token=s3cret")).send().await.unwrap().status(), 200);
        assert_eq!(c.get(format!("{url}?token=s3cre")).send().await.unwrap().status(), 401);
        // The other routes are protected too.
        let r = c.post(format!("{base}/api/execute")).json(&json!({})).send().await.unwrap();
        assert_eq!(r.status(), 401);
    }

    #[tokio::test]
    async fn rejects_foreign_host_header() {
        let base = spawn_secure(PathBuf::from("/nonexistent"), true).await;
        let c = reqwest::Client::new();
        let r = c.get(format!("{base}/")).header("Host", "evil.example").send().await.unwrap();
        assert_eq!(r.status(), 403);
        let r = c.get(format!("{base}/")).header("Host", "localhost:51730").send().await.unwrap();
        assert_eq!(r.status(), 200);
        // Not enforced when the user chose to listen on a non-loopback address.
        let open = spawn_secure(PathBuf::from("/nonexistent"), false).await;
        let r = c.get(format!("{open}/")).header("Host", "my-laptop.lan").send().await.unwrap();
        assert_eq!(r.status(), 200);
    }

    #[tokio::test]
    async fn index_embeds_token() {
        let dir = std::env::temp_dir().join(format!("rustman-dist-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<html><head><title>t</title></head><body>hi</body></html>").unwrap();
        std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
        let base = spawn_secure(dir.clone(), true).await;
        for path in ["/", "/some/spa/route"] {
            let body = reqwest::get(format!("{base}{path}")).await.unwrap().text().await.unwrap();
            assert!(body.contains("window.__RUSTMAN_TOKEN__=\"s3cret\""), "{path}: {body}");
            assert!(body.contains("</script></head>"));
        }
        let js = reqwest::get(format!("{base}/assets/app.js")).await.unwrap().text().await.unwrap();
        assert_eq!(js, "console.log(1)");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn agent_requests_emit_events_and_tag_history() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();

        let mut stream = c.get(format!("{base}/api/events")).send().await.unwrap();
        assert!(stream.status().is_success());

        let req = HttpRequest::new("agent call", rustman_core::HttpMethod::GET, format!("{base}/api/collections"));
        let r = c
            .post(format!("{base}/api/execute"))
            .json(&json!({ "request": req, "source": { "agent": "claude", "session": "s1" } }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());

        let mut seen = String::new();
        while !(seen.contains("request_finished") && seen.contains("\"history\"")) {
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), stream.chunk())
                .await
                .expect("timed out waiting for events")
                .unwrap()
                .expect("stream ended");
            seen.push_str(&String::from_utf8_lossy(&chunk));
        }
        assert!(seen.contains("request_started"));
        assert!(seen.contains("agent call"), "started event carries the request config");
        assert!(seen.find("request_started").unwrap() < seen.find("request_finished").unwrap());

        let h: Vec<serde_json::Value> =
            c.get(format!("{base}/api/history")).send().await.unwrap().json().await.unwrap();
        assert_eq!(h[0]["source"], "agent:claude");
    }

    #[tokio::test]
    async fn ui_requests_are_not_tagged_and_failed_agent_requests_emit_failure() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();
        let ok = HttpRequest::new("ui", rustman_core::HttpMethod::GET, format!("{base}/api/collections"));
        c.post(format!("{base}/api/execute")).json(&json!({ "request": ok })).send().await.unwrap();
        let h: Vec<serde_json::Value> =
            c.get(format!("{base}/api/history")).send().await.unwrap().json().await.unwrap();
        assert!(h[0]["source"].is_null());

        let mut stream = c.get(format!("{base}/api/events")).send().await.unwrap();
        let bad = HttpRequest::new("bad", rustman_core::HttpMethod::GET, "http://127.0.0.1:1/");
        let r = c
            .post(format!("{base}/api/execute"))
            .json(&json!({ "request": bad, "source": { "agent": "claude" } }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_server_error());
        let mut seen = String::new();
        while !seen.contains("request_failed") {
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), stream.chunk())
                .await
                .expect("timed out")
                .unwrap()
                .expect("stream ended");
            seen.push_str(&String::from_utf8_lossy(&chunk));
        }
    }

    #[tokio::test]
    async fn mutations_emit_data_changed_and_focus_is_broadcast() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();
        let mut stream = c.get(format!("{base}/api/events")).send().await.unwrap();
        let col = Collection { id: Uuid::new_v4(), name: "C".into(), description: None, items: vec![] };
        c.post(format!("{base}/api/collections")).json(&col).send().await.unwrap();
        let req = HttpRequest::new("look at me", rustman_core::HttpMethod::GET, "https://example.com");
        let r = c.post(format!("{base}/api/focus")).json(&json!({ "request": req })).send().await.unwrap();
        assert_eq!(r.status(), 204);
        let mut seen = String::new();
        while !(seen.contains("collections") && seen.contains("focus_request")) {
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), stream.chunk())
                .await
                .expect("timed out")
                .unwrap()
                .expect("stream ended");
            seen.push_str(&String::from_utf8_lossy(&chunk));
        }
        assert!(seen.contains("look at me"));
    }

    #[tokio::test]
    async fn history_limit_param_is_capped() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let c = reqwest::Client::new();
        for i in 0..3 {
            let r = HttpRequest::new(format!("r{i}"), rustman_core::HttpMethod::GET, format!("{base}/api/collections"));
            c.post(format!("{base}/api/execute")).json(&json!({ "request": r })).send().await.unwrap();
        }
        let h: Vec<serde_json::Value> =
            c.get(format!("{base}/api/history?limit=2")).send().await.unwrap().json().await.unwrap();
        assert_eq!(h.len(), 2);
    }
}
