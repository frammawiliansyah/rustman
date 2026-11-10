//! Real-time clients: WebSocket and Server-Sent Events.

pub mod assertions;
pub mod sse;
pub mod ws;

pub use sse::{SseClient, SseEvent};
pub use ws::{WebSocketSession, WsDirection, WsMessage, WsPayloadType};
pub use assertions::{SocketAssertion, SocketTestSession};
