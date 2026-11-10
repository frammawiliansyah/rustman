//! API documentation generators (Markdown and standalone HTML) for a [`Collection`].

mod html;
mod markdown;

pub use html::HtmlDocGenerator;
pub use markdown::MarkdownDocGenerator;

use crate::models::http::{AuthConfig, HttpMethod, HttpRequest, RequestBody};

pub(crate) fn method_str(m: HttpMethod) -> &'static str {
    match m {
        HttpMethod::GET => "GET",
        HttpMethod::POST => "POST",
        HttpMethod::PUT => "PUT",
        HttpMethod::DELETE => "DELETE",
        HttpMethod::PATCH => "PATCH",
        HttpMethod::HEAD => "HEAD",
        HttpMethod::OPTIONS => "OPTIONS",
    }
}

/// Human-readable auth requirement (secrets are never printed).
pub(crate) fn auth_text(a: &AuthConfig) -> String {
    match a {
        AuthConfig::None => "None".into(),
        AuthConfig::Bearer { .. } => "Bearer token".into(),
        AuthConfig::Basic { username, .. } => format!("Basic auth (user: {username})"),
        AuthConfig::ApiKey { key, in_header, .. } => {
            format!("API key `{key}` (in {})", if *in_header { "header" } else { "query" })
        }
    }
}

/// Body rendered for display: `(language, text)`; JSON is pretty-printed when valid.
pub(crate) fn body_text(b: &RequestBody) -> Option<(&'static str, String)> {
    match b {
        RequestBody::None => None,
        RequestBody::Raw { content_type, body } => {
            if body.trim().is_empty() {
                return None;
            }
            let looks_json = content_type.to_ascii_lowercase().contains("json");
            if looks_json || body.trim_start().starts_with(['{', '[']) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                    if let Ok(p) = serde_json::to_string_pretty(&v) {
                        return Some(("json", p));
                    }
                }
            }
            Some((if looks_json { "json" } else { "text" }, body.clone()))
        }
        RequestBody::FormUrlEncoded(p) | RequestBody::FormData(p) => {
            let t = p
                .iter()
                .filter(|h| h.enabled)
                .map(|h| format!("{}={}", h.key, h.value))
                .collect::<Vec<_>>()
                .join("\n");
            if t.is_empty() { None } else { Some(("text", t)) }
        }
    }
}

/// cURL snippet for documentation, with credentials replaced by placeholders.
pub(crate) fn curl_snippet(r: &HttpRequest) -> String {
    let mut r = r.clone();
    r.auth = match r.auth {
        AuthConfig::None => AuthConfig::None,
        AuthConfig::Bearer { .. } => AuthConfig::Bearer { token: "<token>".into() },
        AuthConfig::Basic { .. } => AuthConfig::Basic { username: "<username>".into(), password: "<password>".into() },
        AuthConfig::ApiKey { key, in_header, .. } => AuthConfig::ApiKey { key, value: "<api-key>".into(), in_header },
    };
    crate::codegen::generate_snippet(crate::codegen::SnippetTarget::Curl, &r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::collection::{Collection, Folder, Item};
    use crate::models::http::{HeaderPair, QueryParam};
    use uuid::Uuid;

    pub(crate) fn sample() -> Collection {
        let mut create = HttpRequest::new("Create user", HttpMethod::POST, "https://api.example.com/api/users");
        create.headers.push(HeaderPair { key: "X-Trace".into(), value: "abc".into(), enabled: true });
        create.query_params.push(QueryParam { key: "dryRun".into(), value: "true".into(), enabled: true });
        create.body = RequestBody::Raw { content_type: "application/json".into(), body: r#"{"name":"Ann"}"#.into() };
        create.auth = AuthConfig::Bearer { token: "secret-token".into() };
        let list = HttpRequest::new("List <users>", HttpMethod::GET, "https://api.example.com/api/users");
        Collection {
            id: Uuid::new_v4(),
            name: "Users API".into(),
            description: Some("Manage users".into()),
            items: vec![
                Item::Folder(Folder {
                    id: Uuid::new_v4(),
                    name: "Users".into(),
                    description: None,
                    items: vec![Item::Request(create), Item::Request(list)],
                }),
            ],
        }
    }

    #[test]
    fn markdown_contains_expected_sections() {
        let md = MarkdownDocGenerator::generate(&sample());
        assert!(md.starts_with("# Users API"));
        assert!(md.contains("Manage users"));
        assert!(md.contains("## Table of Contents"));
        assert!(md.contains("POST https://api.example.com/api/users"));
        assert!(md.contains("| dryRun | true |"));
        assert!(md.contains("| X-Trace | abc |"));
        assert!(md.contains("Bearer token"));
        assert!(!md.contains("secret-token"));
        assert!(md.contains("```json"));
        assert!(md.contains("\"name\": \"Ann\""));
        assert!(md.contains("curl -X POST"));
    }

    #[test]
    fn html_contains_expected_structure() {
        let html = HtmlDocGenerator::generate(&sample());
        assert!(html.contains("<!DOCTYPE html>") && html.contains("<html"));
        assert!(html.contains("</html>"));
        assert!(html.contains("<aside id=\"sidebar\""));
        assert!(html.contains("<script>") && html.contains("filterEndpoints"));
        assert!(html.contains("badge m-post") && html.contains("badge m-get"));
        assert!(html.contains("List &lt;users&gt;"));
        assert!(!html.contains("List <users>"));
        assert!(html.contains("curl -X POST"));
        assert!(!html.contains("http://") || !html.contains("<script src"));
        assert!(!html.contains("secret-token"));
    }
}
