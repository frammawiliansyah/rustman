//! Core library and data models for rustman.

pub mod assertions;
pub mod codegen;
pub mod cookies;
pub mod data;
pub mod docs;
pub mod engine;
pub mod error;
pub mod graphql;
pub mod importers;
pub mod mock;
pub mod models;
pub mod realtime;
pub mod storage;
pub mod scripting;
pub mod variables;

pub use models::assertion::{AssertionResult, AssertionType};
pub use models::collection::{Collection, Folder, Item};
pub use models::cookie::Cookie;
pub use models::environment::{Environment, Variable};
pub use models::http::{
    AuthConfig, HeaderPair, HttpMethod, HttpRequest, HttpResponse, QueryParam, RequestBody,
};
pub use models::telemetry::TimingMetrics;
pub use codegen::{generate_snippet, SnippetTarget};
pub use cookies::CookieJar;
pub use data::IterationDataset;
pub use docs::{HtmlDocGenerator, MarkdownDocGenerator};
pub use engine::HttpExecutor;
pub use error::RustmanError;
pub use importers::{
    export_postman, export_postman_collection, import_openapi, import_openapi_spec, import_postman,
    import_postman_collection,
};
pub use variables::VariableResolver;
pub use storage::{HistoryEntry, SqliteStorage};
pub use scripting::{ScriptOutput, ScriptRuntime, ScriptScope};
pub use realtime::{SocketAssertion, SocketTestSession, SseClient, SseEvent, WebSocketSession, WsDirection, WsMessage, WsPayloadType};
pub use mock::{MockRoute, MockServer};
pub use graphql::{GraphQLIntrospector, GraphQLRequest, GraphQLResponse};
