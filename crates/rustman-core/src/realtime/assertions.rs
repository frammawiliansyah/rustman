//! Assertion framework for WebSocket / SSE message streams.

use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use crate::assertions::evaluator::lookup;
use crate::models::assertion::AssertionResult;
use crate::realtime::ws::{WebSocketSession, WsDirection, WsMessage};

/// An expectation about inbound frames. Only inbound (server -> client) frames are considered.
#[derive(Clone, Debug, PartialEq)]
pub enum SocketAssertion {
    /// Next inbound frame must equal `expected`.
    ReceiveTextEquals { expected: String, timeout_ms: u64 },
    /// Next inbound frame must contain `substr`.
    ReceiveTextContains { substr: String, timeout_ms: u64 },
    /// Next inbound frame must be JSON whose value at `path` (pointer or dotted) equals `expected`.
    ReceiveJsonPathEquals { path: String, expected: String, timeout_ms: u64 },
    /// Exactly `count` inbound frames must arrive within `timeout_ms` (waits the full window
    /// unless more than `count` frames arrive earlier).
    ExpectMessageCount { count: usize, timeout_ms: u64 },
}

impl SocketAssertion {
    fn name(&self) -> String {
        match self {
            Self::ReceiveTextEquals { expected, .. } => format!("Receive text equals '{expected}'"),
            Self::ReceiveTextContains { substr, .. } => format!("Receive text contains '{substr}'"),
            Self::ReceiveJsonPathEquals { path, expected, .. } => {
                format!("Receive JSON path '{path}' equals '{expected}'")
            }
            Self::ExpectMessageCount { count, .. } => format!("Receive exactly {count} message(s)"),
        }
    }

    fn timeout_ms(&self) -> u64 {
        match self {
            Self::ReceiveTextEquals { timeout_ms, .. }
            | Self::ReceiveTextContains { timeout_ms, .. }
            | Self::ReceiveJsonPathEquals { timeout_ms, .. }
            | Self::ExpectMessageCount { timeout_ms, .. } => *timeout_ms,
        }
    }
}

enum Recv {
    Msg(WsMessage),
    Timeout,
    Closed,
}

/// A WebSocket session paired with its event receiver, for scripted message tests.
pub struct SocketTestSession {
    session: WebSocketSession,
    rx: UnboundedReceiver<WsMessage>,
}

impl SocketTestSession {
    pub fn new(session: WebSocketSession, rx: UnboundedReceiver<WsMessage>) -> Self {
        Self { session, rx }
    }

    pub fn session(&self) -> &WebSocketSession {
        &self.session
    }

    /// Next inbound message before `deadline`, skipping echoed outbound events.
    async fn recv_inbound(&mut self, deadline: Instant) -> Recv {
        loop {
            match tokio::time::timeout_at(deadline, self.rx.recv()).await {
                Err(_) => return Recv::Timeout,
                Ok(None) => return Recv::Closed,
                Ok(Some(m)) if m.direction == WsDirection::In => return Recv::Msg(m),
                Ok(Some(_)) => continue,
            }
        }
    }

    pub async fn send_and_expect(&mut self, send_payload: &str, assertion: SocketAssertion) -> AssertionResult {
        if let Err(e) = self.session.send_text(send_payload).await {
            return AssertionResult {
                name: assertion.name(),
                passed: false,
                message: Some(format!("Failed to send payload: {e}")),
            };
        }
        self.assert_next(assertion).await
    }

    pub async fn assert_next(&mut self, assertion: SocketAssertion) -> AssertionResult {
        let name = assertion.name();
        let timeout_ms = assertion.timeout_ms();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let fail = |msg: String| AssertionResult { name: name.clone(), passed: false, message: Some(msg) };
        let pass = || AssertionResult { name: name.clone(), passed: true, message: None };

        if let SocketAssertion::ExpectMessageCount { count, .. } = &assertion {
            let mut got = 0usize;
            loop {
                match self.recv_inbound(deadline).await {
                    Recv::Msg(_) => {
                        got += 1;
                        if got > *count {
                            return fail(format!("Expected {count} message(s), received more"));
                        }
                    }
                    Recv::Timeout | Recv::Closed => break,
                }
            }
            return if got == *count {
                pass()
            } else {
                fail(format!("Expected {count} message(s) within {timeout_ms}ms, got {got}"))
            };
        }

        let msg = match self.recv_inbound(deadline).await {
            Recv::Msg(m) => m,
            Recv::Timeout => return fail(format!("Timed out after {timeout_ms}ms waiting for a message")),
            Recv::Closed => return fail("Connection closed before a message was received".into()),
        };
        let content = msg.content;

        match &assertion {
            SocketAssertion::ReceiveTextEquals { expected, .. } => {
                if content == *expected {
                    pass()
                } else {
                    fail(format!("Expected message '{expected}', got '{content}'"))
                }
            }
            SocketAssertion::ReceiveTextContains { substr, .. } => {
                if content.contains(substr.as_str()) {
                    pass()
                } else {
                    fail(format!("Expected message to contain '{substr}', got '{content}'"))
                }
            }
            SocketAssertion::ReceiveJsonPathEquals { path, expected, .. } => {
                let json: serde_json::Value = match serde_json::from_str(&content) {
                    Ok(j) => j,
                    Err(e) => return fail(format!("Message is not valid JSON ({e}): '{content}'")),
                };
                match lookup(&json, path) {
                    None => fail(format!("Path '{path}' not found in message '{content}'")),
                    Some(v) => {
                        let actual = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        if actual == *expected {
                            pass()
                        } else {
                            fail(format!("Expected '{path}' to equal '{expected}', got '{actual}'"))
                        }
                    }
                }
            }
            SocketAssertion::ExpectMessageCount { .. } => unreachable!(),
        }
    }

    /// Run assertions sequentially, each consuming from the same stream.
    pub async fn assert_all(&mut self, assertions: Vec<SocketAssertion>) -> Vec<AssertionResult> {
        let mut out = Vec::with_capacity(assertions.len());
        for a in assertions {
            out.push(self.assert_next(a).await);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;

    async fn echo_server() -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(s).await.unwrap();
            let (mut tx, mut rx) = ws.split();
            while let Some(Ok(m)) = rx.next().await {
                if m.is_text() {
                    tx.send(m).await.unwrap();
                } else if m.is_close() {
                    break;
                }
            }
        });
        format!("ws://{addr}")
    }

    async fn session() -> SocketTestSession {
        let url = echo_server().await;
        let (s, rx) = WebSocketSession::connect(&url, &[]).await.unwrap();
        SocketTestSession::new(s, rx)
    }

    #[tokio::test]
    async fn receive_text_equals_against_echo() {
        let mut s = session().await;
        let ok = s
            .send_and_expect("hello", SocketAssertion::ReceiveTextEquals { expected: "hello".into(), timeout_ms: 2000 })
            .await;
        assert!(ok.passed, "{ok:?}");
        let bad = s
            .send_and_expect("hello", SocketAssertion::ReceiveTextEquals { expected: "nope".into(), timeout_ms: 2000 })
            .await;
        assert!(!bad.passed);
        assert!(bad.message.unwrap().contains("got 'hello'"));
    }

    #[tokio::test]
    async fn receive_json_path_equals() {
        let mut s = session().await;
        let r = s
            .send_and_expect(
                r#"{"user":{"id":42,"name":"ann"},"items":[{"k":"v"}]}"#,
                SocketAssertion::ReceiveJsonPathEquals { path: "user.id".into(), expected: "42".into(), timeout_ms: 2000 },
            )
            .await;
        assert!(r.passed, "{r:?}");
        let r = s
            .send_and_expect(
                r#"{"items":[{"k":"v"}]}"#,
                SocketAssertion::ReceiveJsonPathEquals { path: "/items/0/k".into(), expected: "v".into(), timeout_ms: 2000 },
            )
            .await;
        assert!(r.passed, "{r:?}");
        let r = s
            .send_and_expect(
                "{}",
                SocketAssertion::ReceiveJsonPathEquals { path: "a.b".into(), expected: "1".into(), timeout_ms: 2000 },
            )
            .await;
        assert!(!r.passed);
        let r = s
            .send_and_expect(
                "plain",
                SocketAssertion::ReceiveJsonPathEquals { path: "a".into(), expected: "1".into(), timeout_ms: 2000 },
            )
            .await;
        assert!(!r.passed);
    }

    #[tokio::test]
    async fn timeout_when_no_message() {
        let mut s = session().await;
        let r = s
            .assert_next(SocketAssertion::ReceiveTextContains { substr: "x".into(), timeout_ms: 100 })
            .await;
        assert!(!r.passed);
        assert!(r.message.unwrap().contains("Timed out after 100ms"));
    }

    #[tokio::test]
    async fn contains_count_and_assert_all() {
        let mut s = session().await;
        s.session().send_text("alpha beta").await.unwrap();
        s.session().send_text("two").await.unwrap();
        let res = s
            .assert_all(vec![
                SocketAssertion::ReceiveTextContains { substr: "beta".into(), timeout_ms: 2000 },
                SocketAssertion::ReceiveTextEquals { expected: "two".into(), timeout_ms: 2000 },
                SocketAssertion::ExpectMessageCount { count: 0, timeout_ms: 100 },
            ])
            .await;
        assert!(res.iter().all(|r| r.passed), "{res:?}");

        s.session().send_text("a").await.unwrap();
        s.session().send_text("b").await.unwrap();
        let r = s.assert_next(SocketAssertion::ExpectMessageCount { count: 2, timeout_ms: 300 }).await;
        assert!(r.passed, "{r:?}");
        let r = s.assert_next(SocketAssertion::ExpectMessageCount { count: 1, timeout_ms: 100 }).await;
        assert!(!r.passed);
    }
}
