//! Postman Collection v2.1 importer and exporter.

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::error::RustmanError;
use crate::models::collection::{Collection, Folder, Item};
use crate::models::http::{
    AuthConfig, HeaderPair, HttpMethod, HttpRequest, QueryParam, RequestBody,
};

pub const POSTMAN_V21_SCHEMA: &str =
    "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

type Result<T> = std::result::Result<T, RustmanError>;

/// Parse a Postman Collection v2.1 JSON document into a [`Collection`].
pub fn import_postman(json_str: &str) -> Result<Collection> {
    let root: Value = serde_json::from_str(json_str)?;
    let info = root
        .get("info")
        .and_then(Value::as_object)
        .ok_or_else(|| RustmanError::InvalidRequest("missing `info` object".into()))?;
    let schema = info.get("schema").and_then(Value::as_str).unwrap_or("");
    if !schema.contains("/collection/v2.1") {
        return Err(RustmanError::InvalidRequest(format!(
            "unsupported Postman schema: {schema:?} (expected v2.1)"
        )));
    }
    let name = info
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled")
        .to_string();
    let inherited = root.get("auth").and_then(parse_auth);
    let items = parse_items(root.get("item"), inherited.as_ref())?;
    Ok(Collection {
        id: Uuid::new_v4(),
        name,
        description: description_of(info.get("description")),
        items,
    })
}

/// Alias for [`import_postman`].
pub fn import_postman_collection(json_str: &str) -> Result<Collection> {
    import_postman(json_str)
}

fn description_of(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(o) => o
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from),
        _ => None,
    }
}

fn parse_items(v: Option<&Value>, inherited: Option<&AuthConfig>) -> Result<Vec<Item>> {
    let Some(arr) = v.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    arr.iter().map(|i| parse_item(i, inherited)).collect()
}

fn parse_item(v: &Value, inherited: Option<&AuthConfig>) -> Result<Item> {
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled")
        .to_string();
    if let Some(children) = v.get("item") {
        // Folder; its own auth (if any) overrides the inherited one.
        let own = v.get("auth").and_then(parse_auth);
        let eff = own.as_ref().or(inherited);
        return Ok(Item::Folder(Folder {
            id: Uuid::new_v4(),
            name,
            description: description_of(v.get("description")),
            items: parse_items(Some(children), eff)?,
        }));
    }
    let req = v
        .get("request")
        .ok_or_else(|| RustmanError::InvalidRequest(format!("item {name:?} has no request")))?;
    Ok(Item::Request(parse_request(name, req, inherited)?))
}

fn parse_method(s: &str) -> Result<HttpMethod> {
    Ok(match s.to_ascii_uppercase().as_str() {
        "GET" => HttpMethod::GET,
        "POST" => HttpMethod::POST,
        "PUT" => HttpMethod::PUT,
        "DELETE" => HttpMethod::DELETE,
        "PATCH" => HttpMethod::PATCH,
        "HEAD" => HttpMethod::HEAD,
        "OPTIONS" => HttpMethod::OPTIONS,
        other => {
            return Err(RustmanError::InvalidRequest(format!(
                "unsupported HTTP method: {other}"
            )))
        }
    })
}

fn parse_request(name: String, req: &Value, inherited: Option<&AuthConfig>) -> Result<HttpRequest> {
    // A request may be given as a bare URL string (v2.1 allows it).
    if let Some(s) = req.as_str() {
        let (url, query_params) = split_raw_url(s);
        let mut r = HttpRequest::new(name, HttpMethod::GET, url);
        r.query_params = query_params;
        r.auth = inherited.cloned().unwrap_or_default();
        return Ok(r);
    }
    let method = parse_method(req.get("method").and_then(Value::as_str).unwrap_or("GET"))?;
    let (url, query_params) = parse_url(req.get("url"));
    let mut r = HttpRequest::new(name, method, url);
    r.query_params = query_params;
    r.headers = req
        .get("header")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|h| {
                    Some(HeaderPair {
                        key: h.get("key")?.as_str()?.to_string(),
                        value: str_field(h, "value"),
                        enabled: !h.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    r.body = parse_body(req.get("body"), &r.headers);
    r.auth = match req.get("auth") {
        Some(a) => parse_auth(a).unwrap_or_default(),
        None => inherited.cloned().unwrap_or_default(),
    };
    Ok(r)
}

fn str_field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Split a raw URL into (url without query/fragment-free base, query params).
fn split_raw_url(raw: &str) -> (String, Vec<QueryParam>) {
    match raw.split_once('?') {
        None => (raw.to_string(), Vec::new()),
        Some((base, q)) => {
            let q = q.split('#').next().unwrap_or("");
            let params = q
                .split('&')
                .filter(|p| !p.is_empty())
                .map(|p| {
                    let (k, v) = p.split_once('=').unwrap_or((p, ""));
                    QueryParam { key: k.into(), value: v.into(), enabled: true }
                })
                .collect();
            (base.to_string(), params)
        }
    }
}

fn parse_url(v: Option<&Value>) -> (String, Vec<QueryParam>) {
    match v {
        Some(Value::String(s)) => split_raw_url(s),
        Some(Value::Object(o)) => {
            let query: Option<Vec<QueryParam>> = o.get("query").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(|q| {
                        Some(QueryParam {
                            key: q.get("key")?.as_str()?.to_string(),
                            value: str_field(q, "value"),
                            enabled: !q.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                        })
                    })
                    .collect()
            });
            let (url, raw_q) = match o.get("raw").and_then(Value::as_str) {
                Some(raw) => split_raw_url(raw),
                None => (build_url_from_parts(o), Vec::new()),
            };
            (url, query.unwrap_or(raw_q))
        }
        _ => (String::new(), Vec::new()),
    }
}

fn join_parts(v: Option<&Value>, sep: &str) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|p| match p {
                Value::String(s) => Some(s.clone()),
                Value::Object(o) => o.get("value").and_then(Value::as_str).map(String::from),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(sep),
        _ => String::new(),
    }
}

fn build_url_from_parts(o: &Map<String, Value>) -> String {
    let mut out = String::new();
    if let Some(p) = o.get("protocol").and_then(Value::as_str) {
        out.push_str(p);
        out.push_str("://");
    }
    out.push_str(&join_parts(o.get("host"), "."));
    if let Some(port) = o.get("port") {
        let port = port.as_str().map(String::from).unwrap_or_else(|| port.to_string());
        out.push(':');
        out.push_str(&port);
    }
    let path = join_parts(o.get("path"), "/");
    if !path.is_empty() {
        out.push('/');
        out.push_str(&path);
    }
    out
}

fn kv_list(v: Option<&Value>) -> Vec<HeaderPair> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    // Skip file parts; they cannot be represented.
                    if p.get("type").and_then(Value::as_str) == Some("file") {
                        return None;
                    }
                    Some(HeaderPair {
                        key: p.get("key")?.as_str()?.to_string(),
                        value: str_field(p, "value"),
                        enabled: !p.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_body(v: Option<&Value>, headers: &[HeaderPair]) -> RequestBody {
    let Some(b) = v else { return RequestBody::None };
    match b.get("mode").and_then(Value::as_str) {
        Some("raw") => {
            let body = str_field(b, "raw");
            let header_ct = headers
                .iter()
                .find(|h| h.enabled && h.key.eq_ignore_ascii_case("content-type"))
                .map(|h| h.value.clone());
            let lang = b
                .pointer("/options/raw/language")
                .and_then(Value::as_str)
                .unwrap_or("text");
            let content_type = header_ct.unwrap_or_else(|| {
                match lang {
                    "json" => "application/json",
                    "xml" => "application/xml",
                    "html" => "text/html",
                    "javascript" => "application/javascript",
                    _ => "text/plain",
                }
                .to_string()
            });
            RequestBody::Raw { content_type, body }
        }
        Some("urlencoded") => RequestBody::FormUrlEncoded(kv_list(b.get("urlencoded"))),
        Some("formdata") => RequestBody::FormData(kv_list(b.get("formdata"))),
        _ => RequestBody::None,
    }
}

/// Look up a value in a Postman auth attribute array (`[{key, value}, ...]`).
fn auth_attr(a: &Value, kind: &str, key: &str) -> Option<String> {
    a.get(kind)?.as_array()?.iter().find_map(|e| {
        (e.get("key")?.as_str()? == key).then(|| str_field(e, "value"))
    })
}

fn parse_auth(a: &Value) -> Option<AuthConfig> {
    Some(match a.get("type")?.as_str()? {
        "bearer" => AuthConfig::Bearer { token: auth_attr(a, "bearer", "token").unwrap_or_default() },
        "basic" => AuthConfig::Basic {
            username: auth_attr(a, "basic", "username").unwrap_or_default(),
            password: auth_attr(a, "basic", "password").unwrap_or_default(),
        },
        "apikey" => AuthConfig::ApiKey {
            key: auth_attr(a, "apikey", "key").unwrap_or_default(),
            value: auth_attr(a, "apikey", "value").unwrap_or_default(),
            in_header: auth_attr(a, "apikey", "in").as_deref() != Some("query"),
        },
        "noauth" => AuthConfig::None,
        _ => return None,
    })
}

// ---------------------------------------------------------------- export

/// Serialize a [`Collection`] as a Postman Collection v2.1 JSON value.
pub fn export_postman_value(collection: &Collection) -> Value {
    let mut info = json!({
        "_postman_id": collection.id.to_string(),
        "name": collection.name,
        "schema": POSTMAN_V21_SCHEMA,
    });
    if let Some(d) = &collection.description {
        info["description"] = json!(d);
    }
    json!({ "info": info, "item": export_items(&collection.items) })
}

/// Serialize a [`Collection`] as a pretty-printed Postman v2.1 JSON string.
pub fn export_postman(collection: &Collection) -> Result<String> {
    Ok(serde_json::to_string_pretty(&export_postman_value(collection))?)
}

/// Alias for [`export_postman`].
pub fn export_postman_collection(collection: &Collection) -> Result<String> {
    export_postman(collection)
}

fn export_items(items: &[Item]) -> Vec<Value> {
    items
        .iter()
        .map(|i| match i {
            Item::Folder(f) => {
                let mut v = json!({ "name": f.name, "item": export_items(&f.items) });
                if let Some(d) = &f.description {
                    v["description"] = json!(d);
                }
                v
            }
            Item::Request(r) => json!({ "name": r.name, "request": export_request(r) }),
        })
        .collect()
}

fn method_str(m: HttpMethod) -> &'static str {
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

fn kv_json(p: &HeaderPair) -> Value {
    let mut v = json!({ "key": p.key, "value": p.value });
    if !p.enabled {
        v["disabled"] = json!(true);
    }
    v
}

fn export_url(r: &HttpRequest) -> Value {
    let mut raw = r.url.clone();
    let enabled: Vec<String> = r
        .query_params
        .iter()
        .filter(|q| q.enabled)
        .map(|q| format!("{}={}", q.key, q.value))
        .collect();
    if !enabled.is_empty() {
        if raw.contains('?') {
            raw.push('&');
        } else {
            raw.push('?');
        }
        raw.push_str(&enabled.join("&"));
    }
    let mut url = json!({ "raw": raw });
    // Best-effort structured parts.
    let (protocol, rest) = match r.url.split_once("://") {
        Some((p, rest)) => (Some(p), rest),
        None => (None, r.url.as_str()),
    };
    let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
    if let Some(p) = protocol {
        url["protocol"] = json!(p);
    }
    // Only split host/port when unambiguous (no template variables).
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p)),
        _ => (hostport, None),
    };
    if !host.is_empty() {
        url["host"] = json!(host.split('.').collect::<Vec<_>>());
    }
    if let Some(p) = port {
        url["port"] = json!(p);
    }
    let (path_only, _) = path.split_once('?').unwrap_or((path, ""));
    let path_segments: Vec<&str> = path_only.split('/').filter(|s| !s.is_empty()).collect();
    if !path_segments.is_empty() {
        url["path"] = json!(path_segments);
    }
    if !r.query_params.is_empty() {
        url["query"] = Value::Array(
            r.query_params
                .iter()
                .map(|q| {
                    let mut v = json!({ "key": q.key, "value": q.value });
                    if !q.enabled {
                        v["disabled"] = json!(true);
                    }
                    v
                })
                .collect(),
        );
    }
    url
}

fn export_body(b: &RequestBody) -> Option<Value> {
    match b {
        RequestBody::None => None,
        RequestBody::Raw { content_type, body } => {
            let lang = if content_type.contains("json") {
                "json"
            } else if content_type.contains("xml") {
                "xml"
            } else if content_type.contains("html") {
                "html"
            } else {
                "text"
            };
            Some(json!({
                "mode": "raw",
                "raw": body,
                "options": { "raw": { "language": lang } }
            }))
        }
        RequestBody::FormUrlEncoded(p) => Some(json!({
            "mode": "urlencoded",
            "urlencoded": p.iter().map(kv_json).collect::<Vec<_>>()
        })),
        RequestBody::FormData(p) => Some(json!({
            "mode": "formdata",
            "formdata": p.iter().map(|p| {
                let mut v = kv_json(p);
                v["type"] = json!("text");
                v
            }).collect::<Vec<_>>()
        })),
    }
}

fn attrs(pairs: &[(&str, &str)]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|(k, v)| json!({ "key": k, "value": v, "type": "string" }))
            .collect(),
    )
}

fn export_auth(a: &AuthConfig) -> Option<Value> {
    Some(match a {
        AuthConfig::None => return None,
        AuthConfig::Bearer { token } => json!({ "type": "bearer", "bearer": attrs(&[("token", token)]) }),
        AuthConfig::Basic { username, password } => json!({
            "type": "basic",
            "basic": attrs(&[("username", username), ("password", password)])
        }),
        AuthConfig::ApiKey { key, value, in_header } => json!({
            "type": "apikey",
            "apikey": attrs(&[("key", key), ("value", value), ("in", if *in_header { "header" } else { "query" })])
        }),
    })
}

fn export_request(r: &HttpRequest) -> Value {
    let mut v = json!({
        "method": method_str(r.method),
        "header": r.headers.iter().map(kv_json).collect::<Vec<_>>(),
        "url": export_url(r),
    });
    if let Some(b) = export_body(&r.body) {
        v["body"] = b;
    }
    if let Some(a) = export_auth(&r.auth) {
        v["auth"] = a;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "info": {
        "_postman_id": "abc",
        "name": "Sample API",
        "description": "demo collection",
        "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
      },
      "auth": { "type": "bearer", "bearer": [{"key":"token","value":"root-token","type":"string"}] },
      "item": [
        {
          "name": "Users",
          "description": {"content": "user endpoints", "type": "text/plain"},
          "item": [
            {
              "name": "List users",
              "request": {
                "method": "GET",
                "header": [
                  {"key": "Accept", "value": "application/json"},
                  {"key": "X-Off", "value": "1", "disabled": true}
                ],
                "url": {
                  "raw": "https://api.example.com:8080/v1/users?page=1&limit=10",
                  "protocol": "https",
                  "host": ["api","example","com"],
                  "port": "8080",
                  "path": ["v1","users"],
                  "query": [
                    {"key":"page","value":"1"},
                    {"key":"limit","value":"10","disabled":true}
                  ]
                }
              }
            },
            {
              "name": "Nested",
              "item": [
                {
                  "name": "Create user",
                  "request": {
                    "method": "POST",
                    "header": [],
                    "auth": {"type":"basic","basic":[
                      {"key":"username","value":"u"},{"key":"password","value":"p"}]},
                    "body": {
                      "mode": "raw",
                      "raw": "{\"name\":\"x\"}",
                      "options": {"raw": {"language": "json"}}
                    },
                    "url": "https://api.example.com/v1/users"
                  }
                }
              ]
            }
          ]
        },
        {
          "name": "Login",
          "request": {
            "method": "POST",
            "auth": {"type":"noauth"},
            "body": {"mode":"urlencoded","urlencoded":[
              {"key":"user","value":"a"},{"key":"pw","value":"b","disabled":true}]},
            "url": {"raw": "{{baseUrl}}/login", "host": ["{{baseUrl}}"], "path": ["login"]}
          }
        },
        {
          "name": "Upload",
          "request": {
            "method": "PUT",
            "auth": {"type":"apikey","apikey":[
              {"key":"key","value":"X-Key"},{"key":"value","value":"s3"},{"key":"in","value":"query"}]},
            "body": {"mode":"formdata","formdata":[
              {"key":"a","value":"1","type":"text"},
              {"key":"f","type":"file","src":"x.png"}]},
            "url": "http://localhost/upload"
          }
        }
      ]
    }"#;

    fn req<'a>(items: &'a [Item], name: &str) -> &'a HttpRequest {
        for i in items {
            match i {
                Item::Request(r) if r.name == name => return r,
                Item::Folder(f) => {
                    if let Some(r) = find(&f.items, name) {
                        return r;
                    }
                }
                _ => {}
            }
        }
        panic!("request {name} not found");
    }

    fn find<'a>(items: &'a [Item], name: &str) -> Option<&'a HttpRequest> {
        items.iter().find_map(|i| match i {
            Item::Request(r) if r.name == name => Some(r),
            Item::Folder(f) => find(&f.items, name),
            _ => None,
        })
    }

    #[test]
    fn imports_structure() {
        let c = import_postman(SAMPLE).unwrap();
        assert_eq!(c.name, "Sample API");
        assert_eq!(c.description.as_deref(), Some("demo collection"));
        assert_eq!(c.items.len(), 3);
        let Item::Folder(users) = &c.items[0] else { panic!("expected folder") };
        assert_eq!(users.name, "Users");
        assert_eq!(users.description.as_deref(), Some("user endpoints"));
        assert_eq!(users.items.len(), 2);
        let Item::Folder(nested) = &users.items[1] else { panic!("expected nested folder") };
        assert_eq!(nested.items.len(), 1);
    }

    #[test]
    fn imports_request_details() {
        let c = import_postman(SAMPLE).unwrap();
        let r = req(&c.items, "List users");
        assert_eq!(r.method, HttpMethod::GET);
        assert_eq!(r.url, "https://api.example.com:8080/v1/users");
        assert_eq!(r.query_params.len(), 2);
        assert!(r.query_params[0].enabled);
        assert!(!r.query_params[1].enabled);
        assert_eq!(r.headers.len(), 2);
        assert!(!r.headers[1].enabled);
        // inherited collection-level bearer
        assert_eq!(r.auth, AuthConfig::Bearer { token: "root-token".into() });
    }

    #[test]
    fn imports_body_and_auth() {
        let c = import_postman(SAMPLE).unwrap();
        let r = req(&c.items, "Create user");
        assert_eq!(r.method, HttpMethod::POST);
        assert_eq!(
            r.body,
            RequestBody::Raw { content_type: "application/json".into(), body: "{\"name\":\"x\"}".into() }
        );
        assert_eq!(r.auth, AuthConfig::Basic { username: "u".into(), password: "p".into() });

        let login = req(&c.items, "Login");
        assert_eq!(login.auth, AuthConfig::None);
        assert_eq!(login.url, "{{baseUrl}}/login");
        let RequestBody::FormUrlEncoded(p) = &login.body else { panic!() };
        assert_eq!(p.len(), 2);
        assert!(!p[1].enabled);

        let up = req(&c.items, "Upload");
        assert_eq!(
            up.auth,
            AuthConfig::ApiKey { key: "X-Key".into(), value: "s3".into(), in_header: false }
        );
        let RequestBody::FormData(p) = &up.body else { panic!() };
        assert_eq!(p.len(), 1, "file parts are skipped");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(import_postman("not json").is_err());
        assert!(import_postman(r#"{"item":[]}"#).is_err());
        let v10 = r#"{"info":{"name":"x","schema":"https://schema.getpostman.com/json/collection/v1.0.0/collection.json"},"item":[]}"#;
        assert!(import_postman(v10).is_err());
    }

    #[test]
    fn export_has_v21_shape() {
        let c = import_postman(SAMPLE).unwrap();
        let v = export_postman_value(&c);
        assert_eq!(v["info"]["schema"], POSTMAN_V21_SCHEMA);
        assert_eq!(v["info"]["name"], "Sample API");
        assert_eq!(v["item"][0]["name"], "Users");
        let list = &v["item"][0]["item"][0]["request"];
        assert_eq!(list["method"], "GET");
        assert_eq!(list["url"]["raw"], "https://api.example.com:8080/v1/users?page=1");
        assert_eq!(list["url"]["host"], json!(["api", "example", "com"]));
        assert_eq!(list["url"]["port"], "8080");
        assert_eq!(list["url"]["path"], json!(["v1", "users"]));
        assert_eq!(list["header"][1]["disabled"], true);
        assert_eq!(list["auth"]["type"], "bearer");
    }

    #[test]
    fn roundtrip_preserves_content() {
        let c = import_postman(SAMPLE).unwrap();
        let exported = export_postman(&c).unwrap();
        let c2 = import_postman(&exported).unwrap();
        assert_eq!(c2.name, c.name);
        assert_eq!(c2.description, c.description);
        for name in ["List users", "Create user", "Login", "Upload"] {
            let (a, b) = (req(&c.items, name), req(&c2.items, name));
            assert_eq!(a.method, b.method, "{name}");
            assert_eq!(a.url, b.url, "{name}");
            assert_eq!(a.headers, b.headers, "{name}");
            assert_eq!(a.query_params, b.query_params, "{name}");
            assert_eq!(a.body, b.body, "{name}");
            assert_eq!(a.auth, b.auth, "{name}");
        }
    }

    #[test]
    fn export_programmatic_collection() {
        let mut r = HttpRequest::new("Ping", HttpMethod::DELETE, "https://x.io/a/b");
        r.body = RequestBody::Raw { content_type: "text/plain".into(), body: "hi".into() };
        r.auth = AuthConfig::ApiKey { key: "k".into(), value: "v".into(), in_header: true };
        let c = Collection {
            id: Uuid::new_v4(),
            name: "C".into(),
            description: None,
            items: vec![Item::Request(r)],
        };
        let v = export_postman_value(&c);
        assert_eq!(v["item"][0]["request"]["method"], "DELETE");
        assert_eq!(v["item"][0]["request"]["body"]["options"]["raw"]["language"], "text");
        assert_eq!(v["item"][0]["request"]["auth"]["apikey"][2]["value"], "header");
    }

    #[test]
    fn aliases_and_query_merge() {
        let mut r = HttpRequest::new("Search", HttpMethod::GET, "https://api.example.com/search?base=1");
        r.query_params = vec![QueryParam { key: "extra".into(), value: "2".into(), enabled: true }];
        let c = Collection {
            id: Uuid::new_v4(),
            name: "SearchCol".into(),
            description: Some("desc".into()),
            items: vec![Item::Request(r)],
        };
        let exported = export_postman_collection(&c).unwrap();
        assert!(exported.contains("https://api.example.com/search?base=1&extra=2"));
        let imported = import_postman_collection(&exported).unwrap();
        assert_eq!(imported.name, "SearchCol");
    }
}
