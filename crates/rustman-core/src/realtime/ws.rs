//! WebSocket client session.

use base64::Engine;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::Message;

use crate::error::RustmanError;
use crate::models::http::HeaderPair;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WsDirection {
    In,
    Out,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WsPayloadType {
    Text,
    Binary,
}

/// A frame sent or received on a WebSocket. Binary content is base64-encoded.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WsMessage {
    pub direction: WsDirection,
    pub payload_type: WsPayloadType,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

impl WsMessage {
    fn text(direction: WsDirection, s: &str) -> Self {
        Self { direction, payload_type: WsPayloadType::Text, content: s.to_string(), timestamp: Utc::now() }
    }

    fn binary(direction: WsDirection, b: &[u8]) -> Self {
        Self {
            direction,
            payload_type: WsPayloadType::Binary,
            content: base64::engine::general_purpose::STANDARD.encode(b),
            timestamp: Utc::now(),
        }
    }
}

pub struct WebSocketSession {
    outgoing: UnboundedSender<Message>,
    events: UnboundedSender<WsMessage>,
}

impl WebSocketSession {
    /// Connect to `url` (ws:// or wss://). The receiver yields both inbound and outbound
    /// messages (tagged by direction) and closes when the connection ends.
    pub async fn connect(
        url: &str,
        headers: &[HeaderPair],
    ) -> Result<(Self, UnboundedReceiver<WsMessage>), RustmanError> {
        let mut req = url.into_client_request().map_err(|e| RustmanError::InvalidUrl(e.to_string()))?;
        for h in headers.iter().filter(|h| h.enabled) {
            let name = HeaderName::from_bytes(h.key.as_bytes())
                .map_err(|e| RustmanError::InvalidHeader(format!("{}: {e}", h.key)))?;
            let value = HeaderValue::from_str(&h.value)
                .map_err(|e| RustmanError::InvalidHeader(format!("{}: {e}", h.key)))?;
            req.headers_mut().insert(name, value);
        }
        let (stream, _) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| RustmanError::NetworkError(e.to_string()))?;
        let (mut sink, mut source) = stream.split();

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel::<WsMessage>();

        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                let closing = matches!(msg, Message::Close(_));
                if sink.send(msg).await.is_err() || closing {
                    break;
                }
            }
            let _ = sink.close().await;
        });

        let reader_tx = ev_tx.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = source.next().await {
                let ev = match msg {
                    Message::Text(t) => WsMessage::text(WsDirection::In, &t),
                    Message::Binary(b) => WsMessage::binary(WsDirection::In, &b),
                    Message::Close(_) => break,
                    _ => continue, // ping/pong handled by tungstenite
                };
                if reader_tx.send(ev).is_err() {
                    break;
                }
            }
            // reader_tx dropped here; the channel closes once the session is dropped too.
        });

        Ok((Self { outgoing: out_tx, events: ev_tx }, ev_rx))
    }

    fn send(&self, msg: Message, ev: WsMessage) -> Result<(), RustmanError> {
        self.outgoing
            .send(msg)
            .map_err(|_| RustmanError::NetworkError("websocket connection closed".into()))?;
        let _ = self.events.send(ev);
        Ok(())
    }

    pub async fn send_text(&self, text: &str) -> Result<(), RustmanError> {
        self.send(Message::Text(text.to_string()), WsMessage::text(WsDirection::Out, text))
    }

    pub async fn send_binary(&self, data: &[u8]) -> Result<(), RustmanError> {
        self.send(Message::Binary(data.to_vec()), WsMessage::binary(WsDirection::Out, data))
    }

    pub async fn close(&self) -> Result<(), RustmanError> {
        self.outgoing
            .send(Message::Close(None))
            .map_err(|_| RustmanError::NetworkError("websocket connection closed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpListener;

    async fn echo_server() -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(s).await.unwrap();
            let (mut tx, mut rx) = ws.split();
            while let Some(Ok(m)) = rx.next().await {
                if m.is_text() || m.is_binary() {
                    tx.send(m).await.unwrap();
                } else if m.is_close() {
                    break;
                }
            }
        });
        format!("ws://{addr}")
    }

    async fn next(rx: &mut UnboundedReceiver<WsMessage>) -> WsMessage {
        tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn text_and_binary_echo() {
        let url = echo_server().await;
        let (s, mut rx) = WebSocketSession::connect(&url, &[]).await.unwrap();

        s.send_text("hello").await.unwrap();
        let out = next(&mut rx).await;
        assert_eq!((out.direction, out.payload_type, out.content.as_str()), (WsDirection::Out, WsPayloadType::Text, "hello"));
        let inb = next(&mut rx).await;
        assert_eq!((inb.direction, inb.content.as_str()), (WsDirection::In, "hello"));

        s.send_binary(&[1, 2, 3]).await.unwrap();
        let _ = next(&mut rx).await;
        let inb = next(&mut rx).await;
        assert_eq!(inb.payload_type, WsPayloadType::Binary);
        assert_eq!(inb.content, "AQID");

        s.close().await.unwrap();
    }

    #[tokio::test]
    async fn custom_headers_are_sent() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            let mut seen = None;
            let _ = tokio_tungstenite::accept_hdr_async(s, |req: &tokio_tungstenite::tungstenite::handshake::server::Request, resp| {
                seen = req.headers().get("x-token").map(|v| v.to_str().unwrap().to_string());
                Ok(resp)
            })
            .await;
            seen
        });
        let h = [HeaderPair { key: "X-Token".into(), value: "abc".into(), enabled: true }];
        let _c = WebSocketSession::connect(&format!("ws://{addr}"), &h).await.unwrap();
        assert_eq!(task.await.unwrap().as_deref(), Some("abc"));
    }

    #[tokio::test]
    async fn connect_errors() {
        assert!(matches!(WebSocketSession::connect("not a url", &[]).await, Err(RustmanError::InvalidUrl(_))));
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        assert!(matches!(
            WebSocketSession::connect(&format!("ws://{addr}"), &[]).await,
            Err(RustmanError::NetworkError(_))
        ));
    }
}
