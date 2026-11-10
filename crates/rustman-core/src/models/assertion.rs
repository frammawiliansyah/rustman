use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum AssertionType {
    StatusCode(u16),
    MaxResponseTime(u64),
    HeaderExists(String),
    HeaderValue { key: String, expected: String },
    JsonPathEquals { path: String, expected: String },
    BodyContains(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AssertionResult {
    pub name: String,
    pub passed: bool,
    pub message: Option<String>,
}
