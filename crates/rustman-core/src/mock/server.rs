//! Mock server: turns the requests of a collection into live HTTP routes.

use std::net::SocketAddr;

use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, Request, Response, StatusCode},
    Router,
};
use serde_json::json;

use crate::error::RustmanError;
use crate::models::collection::{Collection, Item};
use crate::models::http::{HeaderPair, HttpMethod, RequestBody};

/// A single mocked endpoint.
#[derive(Clone, Debug, PartialEq)]
pub struct MockRoute {
    pub method: HttpMethod,
    /// e.g. `/users/:id` or `/api/status`; `*` matches any single segment.
    pub path_pattern: String,
    pub response_status: u16,
    pub response_headers: Vec<HeaderPair>,
    pub response_body: String,
}

#[derive(Clone, Debug, Default)]
pub struct MockServer {
    pub routes: Vec<MockRoute>,
}

/// Extracts the path of a URL, ignoring scheme, host, query and fragment.
/// Handles `{{var}}` host prefixes; `{{var}}` / `{var}` segments become `*` / `:var`.
fn extract_path(url: &str) -> String {
    let mut rest = url.trim();
    if let Some(i) = rest.find("://") {
        rest = &rest[i + 3..];
        rest = rest.find('/').map_or("", |j| &rest[j..]);
    } else if rest.starts_with("{{") {
        // Leading variable such as {{baseUrl}}: drop it (and any host chars after it).
        rest = match rest.find("}}") {
            Some(e) => {
                let after = &rest[e + 2..];
                after.find('/').map_or("", |j| &after[j..])
            }
            None => rest,
        };
    } else if !rest.starts_with('/') {
        // host[:port]/path
        rest = rest.find('/').map_or("", |j| &rest[j..]);
    }
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let segs: Vec<String> = rest
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with("{{") && s.ends_with("}}") {
                "*".to_string()
            } else if s.len() > 2 && s.starts_with('{') && s.ends_with('}') {
                format!(":{}", &s[1..s.len() - 1])
            } else {
                s.to_string()
            }
        })
        .collect();
    format!("/{}", segs.join("/"))
}

fn collect(items: &[Item], out: &mut Vec<MockRoute>) {
    for item in items {
        match item {
            Item::Folder(f) => collect(&f.items, out),
            Item::Request(r) => {
                let mut headers: Vec<HeaderPair> =
                    r.headers.iter().filter(|h| h.enabled).cloned().collect();
                // The response body mirrors the request's raw body if present.
                let body = match &r.body {
                    RequestBody::Raw { content_type, body } => {
                        if !headers.iter().any(|h| h.key.eq_ignore_ascii_case("content-type")) {
                            headers.push(HeaderPair {
                                key: "Content-Type".into(),
                                value: content_type.clone(),
                                enabled: true,
                            });
                        }
                        body.clone()
                    }
                    _ => {
                        if !headers.iter().any(|h| h.key.eq_ignore_ascii_case("content-type")) {
                            headers.push(HeaderPair {
                                key: "Content-Type".into(),
                                value: "application/json".into(),
                                enabled: true,
                            });
                        }
                        "{}".to_string()
                    }
                };
                out.push(MockRoute {
                    method: r.method,
                    path_pattern: extract_path(&r.url),
                    response_status: 200,
                    response_headers: headers,
                    response_body: body,
                });
            }
        }
    }
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let q: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    p.len() == q.len()
        && p.iter().zip(&q).all(|(a, b)| *a == "*" || a.starts_with(':') || a == b)
}

impl MockServer {
    pub fn from_collection(collection: &Collection) -> Self {
        let mut routes = Vec::new();
        collect(&collection.items, &mut routes);
        Self { routes }
    }

    /// Finds the first route whose method and path pattern match.
    /// Exact (parameter-free) patterns win over parameterised ones.
    pub fn match_request(&self, method: &HttpMethod, path: &str) -> Option<&MockRoute> {
        let mut found: Option<&MockRoute> = None;
        for r in &self.routes {
            if r.method == *method && path_matches(&r.path_pattern, path) {
                if !r.path_pattern.contains([':', '*']) {
                    return Some(r);
                }
                found.get_or_insert(r);
            }
        }
        found
    }

    /// Starts the listener. Pass port 0 for an ephemeral port. Returns a shutdown
    /// sender and the bound port.
    pub async fn start(
        &self,
        port: u16,
    ) -> Result<(tokio::sync::oneshot::Sender<()>, u16), RustmanError> {
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
            .await
            .map_err(|e| RustmanError::NetworkError(format!("cannot bind port {port}: {e}")))?;
        let bound = listener
            .local_addr()
            .map_err(|e| RustmanError::NetworkError(e.to_string()))?
            .port();
        let server = self.clone();
        let app = Router::new().fallback(move |req: Request<Body>| {
            let server = server.clone();
            async move { server.respond(req.method().as_str(), req.uri().path()) }
        });
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        Ok((tx, bound))
    }

    fn respond(&self, method: &str, path: &str) -> Response<Body> {
        let m = match method {
            "GET" => Some(HttpMethod::GET),
            "POST" => Some(HttpMethod::POST),
            "PUT" => Some(HttpMethod::PUT),
            "DELETE" => Some(HttpMethod::DELETE),
            "PATCH" => Some(HttpMethod::PATCH),
            "HEAD" => Some(HttpMethod::HEAD),
            "OPTIONS" => Some(HttpMethod::OPTIONS),
            _ => None,
        };
        if let Some(route) = m.and_then(|m| self.match_request(&m, path)) {
            let mut b = Response::builder()
                .status(StatusCode::from_u16(route.response_status).unwrap_or(StatusCode::OK));
            for h in &route.response_headers {
                if let (Ok(k), Ok(v)) =
                    (HeaderName::try_from(h.key.as_str()), HeaderValue::from_str(&h.value))
                {
                    b = b.header(k, v);
                }
            }
            return b.body(Body::from(route.response_body.clone())).unwrap_or_default();
        }
        let available: Vec<_> = self
            .routes
            .iter()
            .map(|r| json!({ "method": format!("{:?}", r.method), "path": r.path_pattern }))
            .collect();
        let body = json!({ "error": "Not Found", "available_endpoints": available });
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::collection::Folder;
    use crate::models::http::HttpRequest;
    use uuid::Uuid;

    fn sample() -> Collection {
        let mut status = HttpRequest::new("status", HttpMethod::GET, "https://api.example.com/api/status?x=1");
        status.body = RequestBody::Raw { content_type: "application/json".into(), body: "{\"ok\":true}".into() };
        let user = HttpRequest::new("user", HttpMethod::GET, "{{baseUrl}}/users/:id");
        let post = HttpRequest::new("create", HttpMethod::POST, "http://localhost:3000/users");
        Collection {
            id: Uuid::new_v4(),
            name: "c".into(),
            description: None,
            items: vec![
                Item::Request(status),
                Item::Folder(Folder {
                    id: Uuid::new_v4(),
                    name: "f".into(),
                    description: None,
                    items: vec![Item::Request(user), Item::Request(post)],
                }),
            ],
        }
    }

    #[test]
    fn from_collection_extracts_paths() {
        let s = MockServer::from_collection(&sample());
        let paths: Vec<_> = s.routes.iter().map(|r| (r.method, r.path_pattern.as_str())).collect();
        assert_eq!(
            paths,
            vec![
                (HttpMethod::GET, "/api/status"),
                (HttpMethod::GET, "/users/:id"),
                (HttpMethod::POST, "/users"),
            ]
        );
        assert_eq!(s.routes[0].response_body, "{\"ok\":true}");
        assert_eq!(s.routes[0].response_status, 200);
    }

    #[test]
    fn matches_exact_and_param() {
        let s = MockServer::from_collection(&sample());
        assert!(s.match_request(&HttpMethod::GET, "/api/status").is_some());
        assert!(s.match_request(&HttpMethod::POST, "/api/status").is_none());
        assert_eq!(s.match_request(&HttpMethod::GET, "/users/42").unwrap().path_pattern, "/users/:id");
        assert!(s.match_request(&HttpMethod::GET, "/users/42/extra").is_none());
        assert!(s.match_request(&HttpMethod::POST, "/users").is_some());
    }

    #[tokio::test]
    async fn live_server_over_loopback() {
        let s = MockServer::from_collection(&sample());
        let (shutdown, port) = s.start(0).await.unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let ok = reqwest::get(format!("{base}/api/status")).await.unwrap();
        assert_eq!(ok.status(), 200);
        assert_eq!(ok.text().await.unwrap(), "{\"ok\":true}");
        let user = reqwest::get(format!("{base}/users/7")).await.unwrap();
        assert_eq!(user.status(), 200);
        let nf = reqwest::get(format!("{base}/nope")).await.unwrap();
        assert_eq!(nf.status(), 404);
        let v: serde_json::Value = nf.json().await.unwrap();
        assert_eq!(v["available_endpoints"].as_array().unwrap().len(), 3);
        let _ = shutdown.send(());
    }
}
