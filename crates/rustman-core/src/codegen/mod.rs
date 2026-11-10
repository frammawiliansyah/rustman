//! Multi-language code snippet generation for [`HttpRequest`].
//!
//! Every generator works from a [`Prepared`] request: the final URL (enabled query
//! params and API-key query auth merged in), the effective header list (enabled
//! headers plus auth/content-type) and a normalized body.

mod curl;
mod golang;
mod javascript;
mod python;
mod rust;

use crate::models::http::{AuthConfig, HttpMethod, HttpRequest, RequestBody};
use serde::{Deserialize, Serialize};

/// Language/tool a snippet is generated for.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnippetTarget {
    #[serde(rename = "curl")]
    Curl,
    #[serde(rename = "javascript")]
    JavaScriptFetch,
    #[serde(rename = "python")]
    PythonRequests,
    #[serde(rename = "go")]
    GoNetHttp,
    #[serde(rename = "rust")]
    RustReqwest,
}

/// Generate a ready-to-run snippet for `req` in the given `target`.
pub fn generate_snippet(target: SnippetTarget, req: &HttpRequest) -> String {
    let p = Prepared::from_request(req);
    match target {
        SnippetTarget::Curl => curl::generate(&p),
        SnippetTarget::JavaScriptFetch => javascript::generate(&p),
        SnippetTarget::PythonRequests => python::generate(&p),
        SnippetTarget::GoNetHttp => golang::generate(&p),
        SnippetTarget::RustReqwest => rust::generate(&p),
    }
}

pub(crate) enum PBody {
    None,
    Raw(String),
    UrlEncoded(Vec<(String, String)>),
    Multipart(Vec<(String, String)>),
}

pub(crate) struct Prepared {
    pub method: &'static str,
    pub url: String,
    /// Effective headers, excluding Basic auth (see `basic`).
    pub headers: Vec<(String, String)>,
    pub basic: Option<(String, String)>,
    pub body: PBody,
}

impl Prepared {
    pub fn from_request(req: &HttpRequest) -> Self {
        let method = match req.method {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::PATCH => "PATCH",
            HttpMethod::HEAD => "HEAD",
            HttpMethod::OPTIONS => "OPTIONS",
        };

        let mut query: Vec<(String, String)> = req
            .query_params
            .iter()
            .filter(|q| q.enabled && !q.key.is_empty())
            .map(|q| (q.key.clone(), q.value.clone()))
            .collect();
        let mut headers: Vec<(String, String)> = req
            .headers
            .iter()
            .filter(|h| h.enabled && !h.key.is_empty())
            .map(|h| (h.key.clone(), h.value.clone()))
            .collect();
        let has = |hs: &[(String, String)], name: &str| hs.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));

        let mut basic = None;
        match &req.auth {
            AuthConfig::None => {}
            AuthConfig::Bearer { token } => {
                if !has(&headers, "authorization") {
                    headers.push(("Authorization".into(), format!("Bearer {token}")));
                }
            }
            AuthConfig::Basic { username, password } => {
                if !has(&headers, "authorization") {
                    basic = Some((username.clone(), password.clone()));
                }
            }
            AuthConfig::ApiKey { key, value, in_header } => {
                if *in_header {
                    if !has(&headers, key) {
                        headers.push((key.clone(), value.clone()));
                    }
                } else {
                    query.push((key.clone(), value.clone()));
                }
            }
        }

        let body = match &req.body {
            RequestBody::None => PBody::None,
            RequestBody::Raw { content_type, body } => {
                if !content_type.is_empty() && !has(&headers, "content-type") {
                    headers.push(("Content-Type".into(), content_type.clone()));
                }
                PBody::Raw(body.clone())
            }
            RequestBody::FormUrlEncoded(f) => {
                if !has(&headers, "content-type") {
                    headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
                }
                PBody::UrlEncoded(enabled_pairs(f))
            }
            RequestBody::FormData(f) => {
                // The boundary must come from the client library.
                headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
                PBody::Multipart(enabled_pairs(f))
            }
        };

        Prepared { method, url: merge_query(&req.url, &query), headers, basic, body }
    }
}

fn enabled_pairs(f: &[crate::models::http::HeaderPair]) -> Vec<(String, String)> {
    f.iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .map(|p| (p.key.clone(), p.value.clone()))
        .collect()
}

/// Percent-encode everything except RFC 3986 unreserved characters.
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(crate) fn form_encode(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Append `params` to `url`, skipping pairs already present in its query string.
fn merge_query(url: &str, params: &[(String, String)]) -> String {
    let (base, frag) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    let (path, existing) = match base.find('?') {
        Some(i) => (&base[..i], &base[i + 1..]),
        None => (base, ""),
    };
    let present: Vec<&str> = existing.split('&').filter(|s| !s.is_empty()).collect();
    let mut segs: Vec<String> = present.iter().map(|s| s.to_string()).collect();
    for (k, v) in params {
        let enc = format!("{}={}", percent_encode(k), percent_encode(v));
        let raw = format!("{k}={v}");
        if !present.iter().any(|s| *s == enc || *s == raw) {
            segs.push(enc);
        }
    }
    if segs.is_empty() {
        return format!("{path}{frag}");
    }
    format!("{path}?{}{frag}", segs.join("&"))
}

pub(crate) fn base64(input: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in input.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Double-quoted string literal escaping shared by Go and Rust.
pub(crate) fn dq_escape(s: &str, unicode_braces: bool) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                if unicode_braces {
                    o.push_str(&format!("\\u{{{:x}}}", c as u32))
                } else {
                    o.push_str(&format!("\\x{:02x}", c as u32))
                }
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::http::{HeaderPair, QueryParam};

    fn hp(k: &str, v: &str) -> HeaderPair {
        HeaderPair { key: k.into(), value: v.into(), enabled: true }
    }

    fn post_json() -> HttpRequest {
        let mut r = HttpRequest::new("t", HttpMethod::POST, "https://api.example.com/users");
        r.headers.push(hp("X-Trace", "abc"));
        r.headers.push(HeaderPair { key: "X-Off".into(), value: "no".into(), enabled: false });
        r.body = RequestBody::Raw { content_type: "application/json".into(), body: r#"{"name":"O'Brien"}"#.into() };
        r
    }

    #[test]
    fn target_serde_names() {
        let t: SnippetTarget = serde_json::from_str("\"go\"").unwrap();
        assert_eq!(t, SnippetTarget::GoNetHttp);
        assert!(serde_json::from_str::<SnippetTarget>("\"cobol\"").is_err());
    }

    #[test]
    fn query_merge_skips_existing_and_disabled() {
        let mut r = HttpRequest::new("t", HttpMethod::GET, "https://a.io/p?x=1#frag");
        r.query_params.push(QueryParam { key: "x".into(), value: "1".into(), enabled: true });
        r.query_params.push(QueryParam { key: "q".into(), value: "a b&c".into(), enabled: true });
        r.query_params.push(QueryParam { key: "off".into(), value: "1".into(), enabled: false });
        assert_eq!(Prepared::from_request(&r).url, "https://a.io/p?x=1&q=a%20b%26c#frag");
    }

    #[test]
    fn base64_known_values() {
        assert_eq!(base64("user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64("a"), "YQ==");
        assert_eq!(base64("ab"), "YWI=");
    }

    #[test]
    fn curl_json_post_is_shell_safe() {
        let s = generate_snippet(SnippetTarget::Curl, &post_json());
        assert!(s.starts_with("curl -X POST 'https://api.example.com/users'"));
        assert!(s.contains("-H 'X-Trace: abc'"));
        assert!(s.contains("-H 'Content-Type: application/json'"));
        assert!(s.contains(r#"-d '{"name":"O'\''Brien"}'"#));
        assert!(!s.contains("X-Off"));
    }

    #[test]
    fn curl_basic_bearer_apikey_and_form() {
        let mut r = HttpRequest::new("t", HttpMethod::GET, "https://a.io");
        r.auth = AuthConfig::Basic { username: "u".into(), password: "p w".into() };
        assert!(generate_snippet(SnippetTarget::Curl, &r).contains("-u 'u:p w'"));
        r.auth = AuthConfig::Bearer { token: "tok".into() };
        assert!(generate_snippet(SnippetTarget::Curl, &r).contains("-H 'Authorization: Bearer tok'"));
        r.auth = AuthConfig::ApiKey { key: "api_key".into(), value: "k".into(), in_header: false };
        assert!(generate_snippet(SnippetTarget::Curl, &r).contains("'https://a.io?api_key=k'"));
        r.body = RequestBody::FormData(vec![hp("f", "@/etc/passwd")]);
        r.method = HttpMethod::POST;
        assert!(generate_snippet(SnippetTarget::Curl, &r).contains("--form-string 'f=@/etc/passwd'"));
    }

    #[test]
    fn curl_head_and_urlencoded() {
        let mut r = HttpRequest::new("t", HttpMethod::HEAD, "https://a.io");
        assert!(generate_snippet(SnippetTarget::Curl, &r).starts_with("curl -I 'https://a.io'"));
        r.method = HttpMethod::PUT;
        r.body = RequestBody::FormUrlEncoded(vec![hp("a", "1 2"), hp("b", "x&y")]);
        assert!(generate_snippet(SnippetTarget::Curl, &r).contains("-d 'a=1%202&b=x%26y'"));
    }

    #[test]
    fn javascript_fetch_json_and_basic() {
        let mut r = post_json();
        r.auth = AuthConfig::Basic { username: "user".into(), password: "pass".into() };
        let s = generate_snippet(SnippetTarget::JavaScriptFetch, &r);
        assert!(s.contains("await fetch(\"https://api.example.com/users\""));
        assert!(s.contains("method: \"POST\""));
        assert!(s.contains("\"Authorization\": \"Basic dXNlcjpwYXNz\""));
        assert!(s.contains(r#"body: "{\"name\":\"O'Brien\"}""#));
    }

    #[test]
    fn javascript_forms() {
        let mut r = HttpRequest::new("t", HttpMethod::POST, "https://a.io");
        r.body = RequestBody::FormData(vec![hp("a", "1")]);
        let s = generate_snippet(SnippetTarget::JavaScriptFetch, &r);
        assert!(s.contains("new FormData()") && s.contains("form.append(\"a\", \"1\");"));
        assert!(!s.contains("Content-Type"));
        r.body = RequestBody::FormUrlEncoded(vec![hp("a", "1")]);
        assert!(generate_snippet(SnippetTarget::JavaScriptFetch, &r).contains("new URLSearchParams("));
    }

    #[test]
    fn python_variants() {
        let mut r = post_json();
        r.auth = AuthConfig::Basic { username: "u".into(), password: "p".into() };
        let s = generate_snippet(SnippetTarget::PythonRequests, &r);
        assert!(s.contains("import requests"));
        assert!(s.contains("requests.request(\"POST\", url"));
        assert!(s.contains("auth=(\"u\", \"p\")"));
        assert!(s.contains(r#"data="{\"name\":\"O'Brien\"}""#));
        r.body = RequestBody::FormData(vec![hp("a", "é")]);
        let s = generate_snippet(SnippetTarget::PythonRequests, &r);
        assert!(s.contains("files=[(\"a\", (None, \"é\"))]"));
    }

    #[test]
    fn python_urlencoded_and_apikey_header() {
        let mut r = HttpRequest::new("t", HttpMethod::PATCH, "https://a.io");
        r.auth = AuthConfig::ApiKey { key: "X-Key".into(), value: "v".into(), in_header: true };
        r.body = RequestBody::FormUrlEncoded(vec![hp("a", "1")]);
        let s = generate_snippet(SnippetTarget::PythonRequests, &r);
        assert!(s.contains("\"X-Key\": \"v\""));
        assert!(s.contains("data=[(\"a\", \"1\")]"));
    }

    #[test]
    fn go_json_and_imports() {
        let mut r = post_json();
        r.auth = AuthConfig::Basic { username: "u".into(), password: "p".into() };
        let s = generate_snippet(SnippetTarget::GoNetHttp, &r);
        assert!(s.contains("package main"));
        assert!(s.contains("\"net/http\"") && s.contains("\"io\"") && s.contains("\"strings\""));
        assert!(!s.contains("\"bytes\""));
        assert!(s.contains("http.NewRequest(\"POST\", \"https://api.example.com/users\", strings.NewReader("));
        assert!(s.contains("req.SetBasicAuth(\"u\", \"p\")"));
        assert!(s.contains("req.Header.Set(\"X-Trace\", \"abc\")"));
    }

    #[test]
    fn go_multipart_and_urlencoded() {
        let mut r = HttpRequest::new("t", HttpMethod::POST, "https://a.io");
        r.body = RequestBody::FormData(vec![hp("a", "1")]);
        let s = generate_snippet(SnippetTarget::GoNetHttp, &r);
        assert!(s.contains("\"mime/multipart\"") && s.contains("\"bytes\""));
        assert!(s.contains("writer.WriteField(\"a\", \"1\")"));
        assert!(s.contains("writer.FormDataContentType()"));
        r.body = RequestBody::FormUrlEncoded(vec![hp("a", "1")]);
        let s = generate_snippet(SnippetTarget::GoNetHttp, &r);
        assert!(s.contains("\"net/url\"") && s.contains("form.Add(\"a\", \"1\")"));
    }

    #[test]
    fn go_no_body_uses_nil() {
        let r = HttpRequest::new("t", HttpMethod::DELETE, "https://a.io/x");
        let s = generate_snippet(SnippetTarget::GoNetHttp, &r);
        assert!(s.contains("http.NewRequest(\"DELETE\", \"https://a.io/x\", nil)"));
        assert!(!s.contains("\"strings\""));
    }

    #[test]
    fn rust_json_bearer() {
        let mut r = post_json();
        r.auth = AuthConfig::Bearer { token: "t".into() };
        let s = generate_snippet(SnippetTarget::RustReqwest, &r);
        assert!(s.contains("#[tokio::main]"));
        assert!(s.contains(".request(reqwest::Method::POST, \"https://api.example.com/users\")"));
        assert!(s.contains(".header(\"Authorization\", \"Bearer t\")"));
        assert!(s.contains(r#".body("{\"name\":\"O'Brien\"}")"#));
    }

    #[test]
    fn rust_forms_and_basic() {
        let mut r = HttpRequest::new("t", HttpMethod::POST, "https://a.io");
        r.auth = AuthConfig::Basic { username: "u".into(), password: "p".into() };
        r.body = RequestBody::FormData(vec![hp("a", "1")]);
        let s = generate_snippet(SnippetTarget::RustReqwest, &r);
        assert!(s.contains("reqwest::multipart::Form::new()") && s.contains(".text(\"a\", \"1\")"));
        assert!(s.contains(".basic_auth(\"u\", Some(\"p\"))"));
        r.body = RequestBody::FormUrlEncoded(vec![hp("a", "1")]);
        assert!(generate_snippet(SnippetTarget::RustReqwest, &r).contains(".form(&[(\"a\", \"1\")])"));
    }

    #[test]
    fn control_chars_are_escaped() {
        assert_eq!(dq_escape("a\u{1}\n\"", true), "\"a\\u{1}\\n\\\"\"");
        assert_eq!(dq_escape("a\u{1}", false), "\"a\\x01\"");
    }
}
