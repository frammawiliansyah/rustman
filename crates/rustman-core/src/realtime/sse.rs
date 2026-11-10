//! Server-Sent Events client.

use chrono::{DateTime, Utc};
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedReceiver};

use crate::error::RustmanError;
use crate::models::http::HeaderPair;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SseEvent {
    pub id: Option<String>,
    pub event: Option<String>,
    pub data: String,
    pub timestamp: DateTime<Utc>,
}

pub struct SseClient;

impl SseClient {
    /// Open an SSE stream. The receiver yields events until the server closes the stream
    /// or the receiver is dropped.
    pub async fn connect(url: &str, headers: &[HeaderPair]) -> Result<UnboundedReceiver<SseEvent>, RustmanError> {
        let parsed = reqwest::Url::parse(url).map_err(|e| RustmanError::InvalidUrl(e.to_string()))?;
        let mut map = HeaderMap::new();
        map.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        for h in headers.iter().filter(|h| h.enabled) {
            let name = HeaderName::from_bytes(h.key.as_bytes())
                .map_err(|e| RustmanError::InvalidHeader(format!("{}: {e}", h.key)))?;
            let value = HeaderValue::from_str(&h.value)
                .map_err(|e| RustmanError::InvalidHeader(format!("{}: {e}", h.key)))?;
            map.insert(name, value);
        }
        let resp = reqwest::Client::new().get(parsed).headers(map).send().await?;
        if !resp.status().is_success() {
            return Err(RustmanError::NetworkError(format!("SSE endpoint returned HTTP {}", resp.status())));
        }

        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut stream = resp.bytes_stream().eventsource();
            while let Some(Ok(ev)) = stream.next().await {
                let out = SseEvent {
                    id: Some(ev.id).filter(|s| !s.is_empty()),
                    event: Some(ev.event).filter(|s| !s.is_empty() && s != "message"),
                    data: ev.data,
                    timestamp: Utc::now(),
                };
                if tx.send(out).is_err() {
                    break;
                }
            }
        });
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn server(status: &str, body: &'static str) -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let status = status.to_string();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = s.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
            let body = if req.contains("x-token: abc") { body } else { "data: missing-header\n\n" };
            let head = format!("HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n");
            s.write_all(head.as_bytes()).await.unwrap();
            s.write_all(body.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn hdr() -> Vec<HeaderPair> {
        vec![HeaderPair { key: "X-Token".into(), value: "abc".into(), enabled: true }]
    }

    #[tokio::test]
    async fn parses_events() {
        let url = server("200 OK", "id: 7\nevent: tick\ndata: one\n\ndata: a\ndata: b\n\n").await;
        let mut rx = SseClient::connect(&url, &hdr()).await.unwrap();
        let t = Duration::from_secs(5);
        let e1 = tokio::time::timeout(t, rx.recv()).await.unwrap().unwrap();
        assert_eq!((e1.id.as_deref(), e1.event.as_deref(), e1.data.as_str()), (Some("7"), Some("tick"), "one"));
        let e2 = tokio::time::timeout(t, rx.recv()).await.unwrap().unwrap();
        // Per the SSE spec the last event id carries over to later events.
        assert_eq!((e2.id.as_deref(), e2.event, e2.data.as_str()), (Some("7"), None, "a\nb"));
        assert!(tokio::time::timeout(t, rx.recv()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rejects_error_status_and_bad_url() {
        let url = server("500 Internal Server Error", "").await;
        assert!(matches!(SseClient::connect(&url, &[]).await, Err(RustmanError::NetworkError(_))));
        assert!(matches!(SseClient::connect("nope", &[]).await, Err(RustmanError::InvalidUrl(_))));
    }
}
