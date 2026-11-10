//! In-memory, thread-safe cookie jar implementing RFC 6265 storage and matching.

use std::sync::{Arc, RwLock};

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use reqwest::Url;

use crate::models::cookie::Cookie;

/// Thread-safe cookie store. Cloning shares the same underlying storage.
#[derive(Clone, Debug, Default)]
pub struct CookieJar {
    cookies: Arc<RwLock<Vec<Cookie>>>,
}

fn is_expired(c: &Cookie, now: DateTime<Utc>) -> bool {
    c.expires.is_some_and(|e| e <= now)
}

fn same_key(a: &Cookie, name: &str, domain: &str, path: &str) -> bool {
    a.name == name && a.domain.eq_ignore_ascii_case(domain) && a.path == path
}

/// RFC 6265 §5.1.3 domain-match (host vs. cookie domain, with optional leading dot).
fn domain_matches(host: &str, cookie_domain: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let d = cookie_domain.to_ascii_lowercase();
    match d.strip_prefix('.') {
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == d,
    }
}

/// RFC 6265 §5.1.4 path-match.
fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    if request_path == cookie_path {
        return true;
    }
    request_path.starts_with(cookie_path)
        && (cookie_path.ends_with('/') || request_path[cookie_path.len()..].starts_with('/'))
}

/// RFC 6265 §5.1.4 default-path.
fn default_path(request_path: &str) -> String {
    if !request_path.starts_with('/') {
        return "/".into();
    }
    match request_path.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => request_path[..i].to_string(),
    }
}

fn parse_http_date(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc2822(s) {
        return Some(d.with_timezone(&Utc));
    }
    let s = s.trim_end_matches("GMT").trim_end_matches("UTC").trim();
    for fmt in [
        "%a, %d-%b-%Y %H:%M:%S",
        "%a, %d %b %Y %H:%M:%S",
        "%A, %d-%b-%y %H:%M:%S",
        "%a, %d-%b-%y %H:%M:%S",
        "%a %b %e %H:%M:%S %Y",
    ] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(n.and_utc());
        }
    }
    None
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse `Set-Cookie` headers received from `url` and store the resulting cookies.
    /// Cookies that are already expired (or `Max-Age<=0`) remove any matching stored cookie.
    pub fn parse_and_store(&self, url: &str, set_cookie_headers: &[String]) {
        let Ok(url) = Url::parse(url) else { return };
        let Some(host) = url.host_str().map(|h| h.to_ascii_lowercase()) else { return };
        let now = Utc::now();

        for header in set_cookie_headers {
            let mut parts = header.split(';');
            let Some((name, value)) = parts.next().and_then(|nv| nv.split_once('=')) else {
                continue;
            };
            let (name, value) = (name.trim(), value.trim().trim_matches('"'));
            if name.is_empty() {
                continue;
            }

            let mut domain_attr: Option<String> = None;
            let mut path_attr: Option<String> = None;
            let mut expires: Option<DateTime<Utc>> = None;
            let mut max_age: Option<i64> = None;
            let (mut secure, mut http_only) = (false, false);

            for attr in parts {
                let (k, v) = match attr.split_once('=') {
                    Some((k, v)) => (k.trim(), v.trim()),
                    None => (attr.trim(), ""),
                };
                match k.to_ascii_lowercase().as_str() {
                    "domain" if !v.is_empty() => {
                        domain_attr = Some(v.trim_start_matches('.').to_ascii_lowercase())
                    }
                    "path" if v.starts_with('/') => path_attr = Some(v.to_string()),
                    "expires" => expires = parse_http_date(v),
                    "max-age" => max_age = v.parse::<i64>().ok(),
                    "secure" => secure = true,
                    "httponly" => http_only = true,
                    _ => {}
                }
            }

            let domain = match domain_attr {
                Some(d) => {
                    // The request host must domain-match the attribute, otherwise reject.
                    if !domain_matches(&host, &format!(".{d}")) {
                        continue;
                    }
                    format!(".{d}")
                }
                None => host.clone(),
            };
            let path = path_attr.unwrap_or_else(|| default_path(url.path()));

            // Max-Age takes precedence over Expires.
            let expires = match max_age {
                Some(secs) => Some(now + Duration::seconds(secs.max(0))),
                None => expires,
            };
            let remove = max_age.is_some_and(|s| s <= 0) || expires.is_some_and(|e| e <= now);

            let mut jar = self.cookies.write().unwrap();
            jar.retain(|c| !same_key(c, name, &domain, &path));
            if !remove {
                jar.push(Cookie {
                    name: name.to_string(),
                    value: value.to_string(),
                    domain,
                    path,
                    expires,
                    http_only,
                    secure,
                });
            }
        }
    }

    /// Build the `Cookie` header value for a request to `url`, if any cookies match.
    pub fn get_cookie_header(&self, url: &str) -> Option<String> {
        let url = Url::parse(url).ok()?;
        let host = url.host_str()?;
        let https = url.scheme() == "https";
        let path = if url.path().is_empty() { "/" } else { url.path() };
        let now = Utc::now();

        let mut matched: Vec<Cookie> = self
            .cookies
            .read()
            .unwrap()
            .iter()
            .filter(|c| {
                !is_expired(c, now)
                    && domain_matches(host, &c.domain)
                    && path_matches(path, &c.path)
                    && (!c.secure || https)
            })
            .cloned()
            .collect();
        if matched.is_empty() {
            return None;
        }
        // RFC 6265 §5.4: longer paths first (stable sort keeps creation order otherwise).
        matched.sort_by(|a, b| b.path.len().cmp(&a.path.len()));
        Some(
            matched
                .iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    /// Add a cookie, replacing any existing one with the same name/domain/path.
    pub fn add_cookie(&self, cookie: Cookie) {
        let mut jar = self.cookies.write().unwrap();
        jar.retain(|c| !same_key(c, &cookie.name, &cookie.domain, &cookie.path));
        jar.push(cookie);
    }

    /// All stored, non-expired cookies.
    pub fn get_cookies(&self) -> Vec<Cookie> {
        let now = Utc::now();
        self.cookies.read().unwrap().iter().filter(|c| !is_expired(c, now)).cloned().collect()
    }

    /// Non-expired cookies whose domain equals `domain` (ignoring a leading dot and case).
    pub fn get_cookies_for_domain(&self, domain: &str) -> Vec<Cookie> {
        let want = domain.trim_start_matches('.');
        self.get_cookies()
            .into_iter()
            .filter(|c| c.domain.trim_start_matches('.').eq_ignore_ascii_case(want))
            .collect()
    }

    /// Delete all cookies with this name on this domain (ignoring a leading dot).
    /// Returns `true` if anything was removed.
    pub fn delete_cookie(&self, domain: &str, name: &str) -> bool {
        let want = domain.trim_start_matches('.');
        let mut jar = self.cookies.write().unwrap();
        let before = jar.len();
        jar.retain(|c| !(c.name == name && c.domain.trim_start_matches('.').eq_ignore_ascii_case(want)));
        jar.len() != before
    }

    pub fn clear(&self) {
        self.cookies.write().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::http::{HttpMethod, HttpRequest};
    use crate::HttpExecutor;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn s(v: &str) -> Vec<String> {
        vec![v.to_string()]
    }

    #[test]
    fn defaults_domain_and_path_from_url() {
        let jar = CookieJar::new();
        jar.parse_and_store("http://example.com/a/b/c", &s("sid=1; HttpOnly"));
        let c = &jar.get_cookies()[0];
        assert_eq!((c.domain.as_str(), c.path.as_str()), ("example.com", "/a/b"));
        assert!(c.http_only && !c.secure);
        // Host-only: no subdomain match.
        assert_eq!(jar.get_cookie_header("http://example.com/a/b/x").as_deref(), Some("sid=1"));
        assert_eq!(jar.get_cookie_header("http://sub.example.com/a/b/x"), None);
    }

    #[test]
    fn domain_attribute_matches_subdomains_and_rejects_foreign() {
        let jar = CookieJar::new();
        jar.parse_and_store("http://www.example.com/", &s("a=1; Domain=example.com"));
        jar.parse_and_store("http://www.example.com/", &s("evil=1; Domain=other.com"));
        assert_eq!(jar.get_cookies().len(), 1);
        assert_eq!(jar.get_cookie_header("http://api.example.com/").as_deref(), Some("a=1"));
        assert_eq!(jar.get_cookie_header("http://example.com/").as_deref(), Some("a=1"));
        assert_eq!(jar.get_cookie_header("http://notexample.com/"), None);
    }

    #[test]
    fn path_matching_rules() {
        assert!(path_matches("/foo", "/foo"));
        assert!(path_matches("/foo/bar", "/foo"));
        assert!(path_matches("/foo/bar", "/foo/"));
        assert!(!path_matches("/foobar", "/foo"));
        assert!(path_matches("/anything", "/"));
        assert_eq!(default_path("/"), "/");
        assert_eq!(default_path("/a"), "/");
        assert_eq!(default_path("/a/b"), "/a");

        let jar = CookieJar::new();
        jar.parse_and_store("http://h.com/", &s("p=1; Path=/api"));
        assert!(jar.get_cookie_header("http://h.com/api/users").is_some());
        assert!(jar.get_cookie_header("http://h.com/apix").is_none());
        assert!(jar.get_cookie_header("http://h.com/").is_none());
    }

    #[test]
    fn expiration_and_max_age() {
        let jar = CookieJar::new();
        jar.parse_and_store("http://h.com/", &s("old=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT"));
        assert!(jar.get_cookies().is_empty());

        jar.parse_and_store("http://h.com/", &s("fut=1; Expires=Wed, 21 Oct 2099 07:28:00 GMT"));
        assert_eq!(jar.get_cookies()[0].expires.unwrap().format("%Y").to_string(), "2099");

        // Max-Age beats Expires and <=0 deletes the existing cookie.
        jar.parse_and_store("http://h.com/", &s("fut=1; Max-Age=0; Expires=Wed, 21 Oct 2099 07:28:00 GMT"));
        assert!(jar.get_cookies().is_empty());

        // Cookies that expire after being stored are not sent.
        jar.add_cookie(Cookie {
            name: "gone".into(),
            value: "1".into(),
            domain: "h.com".into(),
            path: "/".into(),
            expires: Some(Utc::now() - Duration::seconds(1)),
            http_only: false,
            secure: false,
        });
        assert_eq!(jar.get_cookie_header("http://h.com/"), None);
    }

    #[test]
    fn secure_flag_requires_https() {
        let jar = CookieJar::new();
        jar.parse_and_store("https://h.com/", &s("t=1; Secure"));
        assert_eq!(jar.get_cookie_header("https://h.com/").as_deref(), Some("t=1"));
        assert_eq!(jar.get_cookie_header("http://h.com/"), None);
    }

    #[test]
    fn crud_replace_and_ordering() {
        let jar = CookieJar::new();
        jar.parse_and_store("http://h.com/", &s("a=1"));
        jar.parse_and_store("http://h.com/", &s("a=2"));
        assert_eq!(jar.get_cookies().len(), 1);
        jar.parse_and_store("http://h.com/x/y", &s("b=3; Path=/x"));
        // Longer path first.
        assert_eq!(jar.get_cookie_header("http://h.com/x/y").as_deref(), Some("b=3; a=2"));
        assert_eq!(jar.get_cookies_for_domain("H.com").len(), 2);
        assert!(jar.get_cookies_for_domain("other.com").is_empty());
        assert!(jar.delete_cookie("h.com", "a"));
        assert!(!jar.delete_cookie("h.com", "a"));
        jar.clear();
        assert!(jar.get_cookies().is_empty());
    }

    #[tokio::test]
    async fn executor_stores_and_sends_cookies() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                tx.send(req).unwrap();
                let resp = "HTTP/1.1 200 OK\r\nSet-Cookie: sid=abc; Path=/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                sock.write_all(resp.as_bytes()).await.unwrap();
            }
        });

        let jar = Arc::new(CookieJar::new());
        let exec = HttpExecutor::new().with_cookie_jar(jar.clone());
        let req = HttpRequest::new("t", HttpMethod::GET, format!("http://{addr}/login"));
        exec.execute(&req).await.unwrap();
        assert!(!rx.recv().await.unwrap().to_lowercase().contains("cookie:"));
        assert_eq!(jar.get_cookies()[0].name, "sid");

        exec.execute(&req).await.unwrap();
        let second = rx.recv().await.unwrap().to_lowercase();
        assert!(second.contains("cookie: sid=abc"), "{second}");
    }
}
