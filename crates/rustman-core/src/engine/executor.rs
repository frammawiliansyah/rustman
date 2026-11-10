use std::time::Instant;

use chrono::Utc;
use reqwest::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::{multipart, Client, Method, Request, RequestBuilder};

use crate::error::RustmanError;
use crate::models::http::{
    AuthConfig, HeaderPair, HttpMethod, HttpRequest, HttpResponse, RequestBody,
};
use crate::models::telemetry::TimingMetrics;

/// Executes [`HttpRequest`]s over the network using reqwest.
#[derive(Clone, Debug)]
pub struct HttpExecutor {
    client: Client,
}

impl Default for HttpExecutor {
    fn default() -> Self {
        Self::new()
    }
}

fn to_method(m: HttpMethod) -> Method {
    match m {
        HttpMethod::GET => Method::GET,
        HttpMethod::POST => Method::POST,
        HttpMethod::PUT => Method::PUT,
        HttpMethod::DELETE => Method::DELETE,
        HttpMethod::PATCH => Method::PATCH,
        HttpMethod::HEAD => Method::HEAD,
        HttpMethod::OPTIONS => Method::OPTIONS,
    }
}

fn header_parts(key: &str, value: &str) -> Result<(HeaderName, HeaderValue), RustmanError> {
    let name = HeaderName::from_bytes(key.trim().as_bytes())
        .map_err(|e| RustmanError::InvalidHeader(format!("{key}: {e}")))?;
    let val = HeaderValue::from_str(value)
        .map_err(|e| RustmanError::InvalidHeader(format!("{key}: {e}")))?;
    Ok((name, val))
}

impl HttpExecutor {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .cookie_store(true)
                .build()
                .expect("failed to build reqwest client"),
        }
    }

    /// Build (without sending) the reqwest request for a model request.
    pub fn build_request(&self, req: &HttpRequest) -> Result<Request, RustmanError> {
        self.build_builder(req)?.build().map_err(Into::into)
    }

    fn build_builder(&self, req: &HttpRequest) -> Result<RequestBuilder, RustmanError> {
        let mut url = reqwest::Url::parse(req.url.trim())
            .map_err(|e| RustmanError::InvalidUrl(format!("{}: {e}", req.url)))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(RustmanError::InvalidUrl(format!(
                "unsupported scheme: {}",
                url.scheme()
            )));
        }

        // Query params (enabled only), plus API key in query if configured.
        let mut query: Vec<(&str, &str)> = req
            .query_params
            .iter()
            .filter(|q| q.enabled)
            .map(|q| (q.key.as_str(), q.value.as_str()))
            .collect();
        if let AuthConfig::ApiKey {
            key,
            value,
            in_header: false,
        } = &req.auth
        {
            query.push((key, value));
        }
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }

        let mut rb = self.client.request(to_method(req.method), url);

        for h in req.headers.iter().filter(|h| h.enabled) {
            let (n, v) = header_parts(&h.key, &h.value)?;
            rb = rb.header(n, v);
        }

        rb = match &req.auth {
            AuthConfig::None => rb,
            AuthConfig::Bearer { token } => rb.bearer_auth(token),
            AuthConfig::Basic { username, password } => {
                rb.basic_auth(username, Some(password))
            }
            AuthConfig::ApiKey {
                key,
                value,
                in_header: true,
            } => {
                let (n, v) = header_parts(key, value)?;
                rb.header(n, v)
            }
            AuthConfig::ApiKey { .. } => rb,
        };

        let has_content_type = req
            .headers
            .iter()
            .any(|h| h.enabled && h.key.trim().eq_ignore_ascii_case("content-type"));

        rb = match &req.body {
            RequestBody::None => rb,
            RequestBody::Raw { content_type, body } => {
                if !has_content_type && !content_type.trim().is_empty() {
                    let v = HeaderValue::from_str(content_type)
                        .map_err(|e| RustmanError::InvalidHeader(format!("Content-Type: {e}")))?;
                    rb = rb.header(CONTENT_TYPE, v);
                }
                rb.body(body.clone())
            }
            RequestBody::FormUrlEncoded(pairs) => {
                let enabled: Vec<(&str, &str)> = pairs
                    .iter()
                    .filter(|p| p.enabled)
                    .map(|p| (p.key.as_str(), p.value.as_str()))
                    .collect();
                rb.form(&enabled)
            }
            RequestBody::FormData(pairs) => rb.multipart(build_multipart(pairs)),
        };

        Ok(rb)
    }

    pub async fn execute(
        &self,
        req: &HttpRequest,
    ) -> Result<(HttpResponse, TimingMetrics), RustmanError> {
        let rb = self.build_builder(req)?;
        let start = Instant::now();
        let resp = rb.send().await?;
        let ttfb_ms = start.elapsed().as_millis() as u64;

        let status = resp.status().as_u16();
        let headers: Vec<HeaderPair> = resp
            .headers()
            .iter()
            .map(|(k, v)| HeaderPair {
                key: k.to_string(),
                value: String::from_utf8_lossy(v.as_bytes()).into_owned(),
                enabled: true,
            })
            .collect();
        let body = resp.text().await?;
        let total_ms = start.elapsed().as_millis() as u64;

        let response = HttpResponse {
            status,
            headers,
            body,
            elapsed_ms: total_ms,
            timestamp: Utc::now(),
        };
        let metrics = TimingMetrics {
            ttfb_ms: Some(ttfb_ms),
            download_ms: Some(total_ms.saturating_sub(ttfb_ms)),
            total_ms,
            ..Default::default()
        };
        Ok((response, metrics))
    }
}

fn build_multipart(pairs: &[HeaderPair]) -> multipart::Form {
    pairs
        .iter()
        .filter(|p| p.enabled)
        .fold(multipart::Form::new(), |form, p| {
            form.text(p.key.clone(), p.value.clone())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn hp(k: &str, v: &str, enabled: bool) -> HeaderPair {
        HeaderPair { key: k.into(), value: v.into(), enabled }
    }

    fn build(req: &HttpRequest) -> Request {
        HttpExecutor::new().build_request(req).unwrap()
    }

    #[test]
    fn maps_all_methods() {
        for (m, expected) in [
            (HttpMethod::GET, Method::GET),
            (HttpMethod::POST, Method::POST),
            (HttpMethod::PUT, Method::PUT),
            (HttpMethod::DELETE, Method::DELETE),
            (HttpMethod::PATCH, Method::PATCH),
            (HttpMethod::HEAD, Method::HEAD),
            (HttpMethod::OPTIONS, Method::OPTIONS),
        ] {
            let r = build(&HttpRequest::new("t", m, "http://localhost/"));
            assert_eq!(r.method(), &expected);
        }
    }

    #[test]
    fn only_enabled_query_and_headers() {
        let mut req = HttpRequest::new("t", HttpMethod::GET, "http://localhost/p?x=1");
        req.query_params = vec![
            crate::QueryParam { key: "a".into(), value: "b c".into(), enabled: true },
            crate::QueryParam { key: "off".into(), value: "1".into(), enabled: false },
        ];
        req.headers = vec![hp("X-On", "1", true), hp("X-Off", "1", false)];
        let r = build(&req);
        assert_eq!(r.url().query(), Some("x=1&a=b+c"));
        assert!(r.headers().contains_key("x-on"));
        assert!(!r.headers().contains_key("x-off"));
    }

    #[test]
    fn auth_variants() {
        let mut req = HttpRequest::new("t", HttpMethod::GET, "http://localhost/");
        req.auth = AuthConfig::Bearer { token: "tok".into() };
        assert_eq!(build(&req).headers()["authorization"], "Bearer tok");

        req.auth = AuthConfig::Basic { username: "user".into(), password: "pass".into() };
        assert_eq!(build(&req).headers()["authorization"], "Basic dXNlcjpwYXNz");

        req.auth = AuthConfig::ApiKey { key: "X-Key".into(), value: "v".into(), in_header: true };
        assert_eq!(build(&req).headers()["x-key"], "v");

        req.auth = AuthConfig::ApiKey { key: "k".into(), value: "v".into(), in_header: false };
        let r = build(&req);
        assert_eq!(r.url().query(), Some("k=v"));
        assert!(!r.headers().contains_key("k"));
    }

    #[test]
    fn raw_body_content_type() {
        let mut req = HttpRequest::new("t", HttpMethod::POST, "http://localhost/");
        req.body = RequestBody::Raw { content_type: "application/json".into(), body: "{}".into() };
        let r = build(&req);
        assert_eq!(r.headers()[CONTENT_TYPE], "application/json");
        assert_eq!(r.body().unwrap().as_bytes().unwrap(), b"{}");

        // Explicit header wins.
        req.headers = vec![hp("Content-Type", "text/plain", true)];
        assert_eq!(build(&req).headers()[CONTENT_TYPE], "text/plain");
    }

    #[test]
    fn form_bodies() {
        let mut req = HttpRequest::new("t", HttpMethod::POST, "http://localhost/");
        req.body = RequestBody::FormUrlEncoded(vec![hp("a", "1 2", true), hp("b", "x", false)]);
        let r = build(&req);
        assert_eq!(r.body().unwrap().as_bytes().unwrap(), b"a=1+2");
        assert_eq!(r.headers()[CONTENT_TYPE], "application/x-www-form-urlencoded");

        req.body = RequestBody::FormData(vec![hp("a", "1", true)]);
        let ct = build(&req).headers()[CONTENT_TYPE].to_str().unwrap().to_string();
        assert!(ct.starts_with("multipart/form-data; boundary="));
    }

    #[test]
    fn invalid_inputs_error() {
        let ex = HttpExecutor::new();
        let req = HttpRequest::new("t", HttpMethod::GET, "not a url");
        assert!(matches!(ex.build_request(&req), Err(RustmanError::InvalidUrl(_))));
        let req = HttpRequest::new("t", HttpMethod::GET, "ftp://localhost/");
        assert!(matches!(ex.build_request(&req), Err(RustmanError::InvalidUrl(_))));
        let mut req = HttpRequest::new("t", HttpMethod::GET, "http://localhost/");
        req.headers = vec![hp("bad header", "v", true)];
        assert!(matches!(ex.build_request(&req), Err(RustmanError::InvalidHeader(_))));
    }

    #[tokio::test]
    async fn execute_against_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).await.unwrap();
            s.write_all(
                b"HTTP/1.1 201 Created\r\nX-Test: yes\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
            )
            .await
            .unwrap();
        });
        let req = HttpRequest::new("t", HttpMethod::GET, format!("http://{addr}/"));
        let (resp, metrics) = HttpExecutor::new().execute(&req).await.unwrap();
        assert_eq!(resp.status, 201);
        assert_eq!(resp.body, "hello");
        assert!(resp.headers.iter().any(|h| h.key == "x-test" && h.value == "yes"));
        assert_eq!(resp.elapsed_ms, metrics.total_ms);
        assert!(metrics.ttfb_ms.unwrap() <= metrics.total_ms);
    }

    #[tokio::test]
    async fn execute_connection_refused_is_network_error() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let req = HttpRequest::new("t", HttpMethod::GET, format!("http://127.0.0.1:{port}/"));
        let err = HttpExecutor::new().execute(&req).await.unwrap_err();
        assert!(matches!(err, RustmanError::NetworkError(_)));
    }
}
