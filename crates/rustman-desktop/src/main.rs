//! Local backend server + IPC bridge for the rustman desktop UI.
//!
//! Serves a REST API under `/api/*` and the bundled web frontend
//! (`./frontend/dist`, or a placeholder page when it has not been built).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rustman_core::importers::{import_openapi, import_postman};
use rustman_core::{
    generate_snippet, Collection, Environment, HistoryEntry, HttpExecutor, HttpRequest, HttpResponse, RustmanError,
    SnippetTarget, SqliteStorage, TimingMetrics, VariableResolver,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

const DEFAULT_PORT: u16 = 51730;
const HISTORY_LIMIT: usize = 50;

const PLACEHOLDER: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>rustman</title></head>\
<body style=\"font-family:sans-serif;padding:2rem\"><h1>rustman backend is running</h1>\
<p>The web frontend has not been built. Run <code>cd frontend &amp;&amp; pnpm install &amp;&amp; pnpm build</code> \
and restart, or use the <code>/api/*</code> endpoints directly.</p></body></html>";

#[derive(Clone)]
struct AppState {
    storage: Arc<SqliteStorage>,
    executor: HttpExecutor,
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
}

#[derive(Serialize)]
struct ExecuteReply {
    response: HttpResponse,
    timing: TimingMetrics,
    history_id: Option<Uuid>,
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

// ---------- handlers ----------

async fn codegen(Json(body): Json<CodegenBody>) -> Json<CodegenReply> {
    Json(CodegenReply { code: generate_snippet(body.target, &body.request) })
}

async fn execute(State(st): State<AppState>, Json(body): Json<ExecuteBody>) -> ApiResult<ExecuteReply> {
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
    // A history write failure must not hide a successful response.
    let history_id = match st.storage.save_history(&resolved, &response) {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!("failed to save history: {e}");
            None
        }
    };
    Ok(Json(ExecuteReply { response, timing, history_id }))
}

async fn list_collections(State(st): State<AppState>) -> ApiResult<Vec<Collection>> {
    Ok(Json(st.storage.list_collections()?))
}

async fn save_collection(State(st): State<AppState>, Json(c): Json<Collection>) -> ApiResult<Collection> {
    st.storage.save_collection(&c)?;
    Ok(Json(c))
}

async fn delete_collection(State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    if st.storage.delete_collection(&id)? {
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
    Ok(Json(e))
}

async fn delete_environment(State(st): State<AppState>, Path(id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    if st.storage.delete_environment(&id)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, format!("environment {id} not found")))
    }
}

async fn list_history(State(st): State<AppState>) -> ApiResult<Vec<HistoryEntry>> {
    Ok(Json(st.storage.list_history(HISTORY_LIMIT)?))
}

async fn import_postman_handler(State(st): State<AppState>, raw: String) -> ApiResult<Collection> {
    let c = import_postman(&raw).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    st.storage.save_collection(&c)?;
    Ok(Json(c))
}

async fn import_openapi_handler(State(st): State<AppState>, raw: String) -> ApiResult<Collection> {
    let c = import_openapi(&raw).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    st.storage.save_collection(&c)?;
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

fn build_app(storage: Arc<SqliteStorage>, dist: PathBuf) -> Router {
    let state = AppState { storage, executor: HttpExecutor::new() };

    let api = Router::new()
        .route("/execute", post(execute))
        .route("/codegen", post(codegen))
        .route("/collections", get(list_collections).post(save_collection))
        .route("/collections/:id", delete(delete_collection))
        .route("/environments", get(list_environments).post(save_environment))
        .route("/environments/:id", delete(delete_environment))
        .route("/history", get(list_history))
        .route("/import/postman", post(import_postman_handler))
        .route("/import/openapi", post(import_openapi_handler))
        .with_state(state);

    // Only local origins may call the API from a browser: it can fire arbitrary requests.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|o, _| is_local_origin(o)))
        .allow_methods(tower_http::cors::Any)
        .allow_headers([header::CONTENT_TYPE]);

    let app = Router::new().nest("/api", api);
    let app = if dist.join("index.html").is_file() {
        let index = dist.join("index.html");
        app.fallback_service(ServeDir::new(&dist).fallback(ServeFile::new(index)))
    } else {
        tracing::warn!("{} not found; serving placeholder page", dist.display());
        app.fallback(placeholder)
    };
    app.layer(cors)
}

fn db_path() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let dir = home.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".rustman");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("cannot create {}: {e}", dir.display());
    }
    dir.join("rustman.db")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().init();

    let path = std::env::var_os("RUSTMAN_DB").map(PathBuf::from).unwrap_or_else(db_path);
    let storage = Arc::new(SqliteStorage::new(&path)?);
    let dist = std::env::var_os("RUSTMAN_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./frontend/dist"));
    let port: u16 = std::env::var("RUSTMAN_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("rustman db: {}", path.display());
    tracing::info!("rustman listening on http://{}", listener.local_addr()?);
    axum::serve(listener, build_app(storage, dist)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn spawn(dist: PathBuf) -> String {
        let storage = Arc::new(SqliteStorage::new_in_memory().unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, build_app(storage, dist)).await.unwrap() });
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
    async fn serves_placeholder_without_dist() {
        let base = spawn(PathBuf::from("/nonexistent")).await;
        let body = reqwest::get(&base).await.unwrap().text().await.unwrap();
        assert!(body.contains("rustman backend is running"));
    }

    #[test]
    fn cors_only_allows_local_origins() {
        assert!(is_local_origin(&HeaderValue::from_static("http://localhost:5173")));
        assert!(is_local_origin(&HeaderValue::from_static("http://127.0.0.1:51730")));
        assert!(!is_local_origin(&HeaderValue::from_static("https://evil.com")));
        assert!(!is_local_origin(&HeaderValue::from_static("http://localhost.evil.com")));
    }
}
