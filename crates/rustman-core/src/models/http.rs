use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HttpMethod {
    #[default]
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct HeaderPair {
    pub key: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct QueryParam {
    pub key: String,
    pub value: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum RequestBody {
    #[default]
    None,
    Raw {
        content_type: String,
        body: String,
    },
    FormUrlEncoded(Vec<HeaderPair>),
    FormData(Vec<HeaderPair>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum AuthConfig {
    #[default]
    None,
    Bearer {
        token: String,
    },
    Basic {
        username: String,
        password: String,
    },
    ApiKey {
        key: String,
        value: String,
        in_header: bool,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HttpRequest {
    pub id: Uuid,
    pub name: String,
    pub method: HttpMethod,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
    #[serde(default)]
    pub query_params: Vec<QueryParam>,
    #[serde(default)]
    pub body: RequestBody,
    #[serde(default)]
    pub auth: AuthConfig,
    /// Script executed before the request is sent (Postman `pm.*` API).
    #[serde(default)]
    pub pre_request_script: Option<String>,
    /// Script executed after the response is received (Postman `pm.*` API).
    #[serde(default)]
    pub test_script: Option<String>,
}

impl HttpRequest {
    pub fn new(name: impl Into<String>, method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            method,
            url: url.into(),
            headers: Vec::new(),
            query_params: Vec::new(),
            body: RequestBody::None,
            auth: AuthConfig::None,
            pre_request_script: None,
            test_script: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<HeaderPair>,
    pub body: String,
    pub elapsed_ms: u64,
    pub timestamp: DateTime<Utc>,
}
