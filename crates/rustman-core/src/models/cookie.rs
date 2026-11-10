use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

fn default_path() -> String {
    "/".to_string()
}

/// A single HTTP cookie as stored in the cookie jar.
///
/// A `domain` with a leading dot (e.g. `.example.com`) matches the domain and all of its
/// subdomains; a bare domain (e.g. `example.com`) is host-only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default)]
    pub expires: Option<DateTime<Utc>>,
    #[serde(default)]
    pub http_only: bool,
    #[serde(default)]
    pub secure: bool,
}
