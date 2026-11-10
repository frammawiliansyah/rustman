//! OpenAPI 3.0 / 3.1 (JSON) importer.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::RustmanError;
use crate::models::collection::{Collection, Folder, Item};
use crate::models::http::{
    AuthConfig, HeaderPair, HttpMethod, HttpRequest, QueryParam, RequestBody,
};

type Result<T> = std::result::Result<T, RustmanError>;

const METHODS: [(&str, HttpMethod); 7] = [
    ("get", HttpMethod::GET),
    ("post", HttpMethod::POST),
    ("put", HttpMethod::PUT),
    ("delete", HttpMethod::DELETE),
    ("patch", HttpMethod::PATCH),
    ("head", HttpMethod::HEAD),
    ("options", HttpMethod::OPTIONS),
];

/// Parse an OpenAPI 3.0/3.1 JSON document into a [`Collection`].
///
/// Operations are grouped into folders by their first tag; untagged operations
/// are placed at the top level. The base URL is the first server URL, or
/// `{{baseUrl}}` when no servers are declared.
pub fn import_openapi(json_str: &str) -> Result<Collection> {
    let root: Value = serde_json::from_str(json_str)?;
    let version = root.get("openapi").and_then(Value::as_str).ok_or_else(|| {
        RustmanError::InvalidRequest("missing `openapi` version field".into())
    })?;
    if !(version.starts_with("3.0") || version.starts_with("3.1")) {
        return Err(RustmanError::InvalidRequest(format!(
            "unsupported OpenAPI version: {version} (expected 3.0.x or 3.1.x)"
        )));
    }
    let info = root
        .get("info")
        .ok_or_else(|| RustmanError::InvalidRequest("missing `info` object".into()))?;
    let name = info.get("title").and_then(Value::as_str).unwrap_or("Untitled").to_string();
    let description = info
        .get("description")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);

    let base = root
        .pointer("/servers/0/url")
        .and_then(Value::as_str)
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| "{{baseUrl}}".to_string());

    let mut top: Vec<Item> = Vec::new();
    let mut folders: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    let mut folder_order: Vec<String> = Vec::new();

    if let Some(paths) = root.get("paths").and_then(Value::as_object) {
        for (path, item) in paths {
            let item = resolve(&root, item);
            for (m, method) in METHODS {
                let Some(op) = item.get(m) else { continue };
                let op = resolve(&root, op);
                let req = build_request(&root, &base, path, method, item, op);
                let tag = op
                    .pointer("/tags/0")
                    .and_then(Value::as_str)
                    .map(String::from);
                match tag {
                    Some(t) => {
                        if !folders.contains_key(&t) {
                            folder_order.push(t.clone());
                        }
                        folders.entry(t).or_default().push(Item::Request(req));
                    }
                    None => top.push(Item::Request(req)),
                }
            }
        }
    }

    let mut items: Vec<Item> = folder_order
        .into_iter()
        .map(|t| {
            let reqs = folders.remove(&t).unwrap_or_default();
            Item::Folder(Folder {
                id: Uuid::new_v4(),
                description: tag_description(&root, &t),
                name: t,
                items: reqs,
            })
        })
        .collect();
    items.extend(top);

    Ok(Collection { id: Uuid::new_v4(), name, description, items })
}

fn tag_description(root: &Value, tag: &str) -> Option<String> {
    root.get("tags")?.as_array()?.iter().find_map(|t| {
        (t.get("name")?.as_str()? == tag)
            .then(|| t.get("description")?.as_str().map(String::from))
            .flatten()
    })
}

/// Follow local `$ref`s (`#/...`), with a depth limit to survive cycles.
fn resolve<'a>(root: &'a Value, mut v: &'a Value) -> &'a Value {
    for _ in 0..16 {
        match v.get("$ref").and_then(Value::as_str) {
            Some(r) if r.starts_with('#') => match root.pointer(&r[1..]) {
                Some(t) => v = t,
                None => return v,
            },
            _ => return v,
        }
    }
    v
}

fn build_request(
    root: &Value,
    base: &str,
    path: &str,
    method: HttpMethod,
    path_item: &Value,
    op: &Value,
) -> HttpRequest {
    let name = op
        .get("summary")
        .and_then(Value::as_str)
        .or_else(|| op.get("operationId").and_then(Value::as_str))
        .map(String::from)
        .unwrap_or_else(|| format!("{} {}", format!("{method:?}"), path));
    let url = format!("{base}{path}");
    let mut req = HttpRequest::new(name, method, url);

    // Path-level parameters first; operation-level override by (in, name).
    let mut params: Vec<&Value> = Vec::new();
    for src in [path_item.get("parameters"), op.get("parameters")] {
        for p in src.and_then(Value::as_array).into_iter().flatten() {
            let p = resolve(root, p);
            let key = (p.get("in"), p.get("name"));
            params.retain(|q| (q.get("in"), q.get("name")) != key);
            params.push(p);
        }
    }
    for p in params {
        let (Some(loc), Some(pname)) = (
            p.get("in").and_then(Value::as_str),
            p.get("name").and_then(Value::as_str),
        ) else {
            continue;
        };
        let value = example_value(root, p)
            .map(|v| match v {
                Value::String(s) => s,
                other => other.to_string(),
            })
            .unwrap_or_default();
        let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
        match loc {
            "query" => req.query_params.push(QueryParam {
                key: pname.into(),
                value,
                enabled: required,
            }),
            "header" => req.headers.push(HeaderPair {
                key: pname.into(),
                value,
                enabled: required,
            }),
            _ => {} // path params stay as `{name}` in the URL; cookies unsupported
        }
    }

    if let Some(body) = op.get("requestBody") {
        apply_body(root, resolve(root, body), &mut req);
    }
    req.auth = security_auth(root, op);
    req
}

/// Example for a parameter/media object: `example`, first of `examples`, or schema-derived.
fn example_value(root: &Value, v: &Value) -> Option<Value> {
    if let Some(e) = v.get("example") {
        return Some(e.clone());
    }
    if let Some(e) = v
        .get("examples")
        .and_then(Value::as_object)
        .and_then(|m| m.values().next())
        .map(|e| resolve(root, e))
        .and_then(|e| e.get("value"))
    {
        return Some(e.clone());
    }
    v.get("schema").map(|s| schema_example(root, s, 0)).filter(|v| !v.is_null())
}

fn schema_example(root: &Value, schema: &Value, depth: u8) -> Value {
    if depth > 6 {
        return Value::Null;
    }
    let s = resolve(root, schema);
    if let Some(e) = s.get("example").or_else(|| s.get("default")) {
        return e.clone();
    }
    if let Some(e) = s.pointer("/enum/0") {
        return e.clone();
    }
    if let Some(sub) = s.get("allOf").and_then(Value::as_array) {
        let mut merged = serde_json::Map::new();
        for part in sub {
            if let Value::Object(o) = schema_example(root, part, depth + 1) {
                merged.extend(o);
            }
        }
        return Value::Object(merged);
    }
    for k in ["oneOf", "anyOf"] {
        if let Some(first) = s.get(k).and_then(|a| a.get(0)) {
            return schema_example(root, first, depth + 1);
        }
    }
    // 3.1 allows `type` to be an array.
    let ty = match s.get("type") {
        Some(Value::String(t)) => Some(t.as_str()),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).find(|t| *t != "null"),
        _ => None,
    }
    .or_else(|| s.get("properties").map(|_| "object"));
    match ty {
        Some("string") => match s.get("format").and_then(Value::as_str) {
            Some("date-time") => json!("2024-01-01T00:00:00Z"),
            Some("date") => json!("2024-01-01"),
            Some("uuid") => json!("00000000-0000-0000-0000-000000000000"),
            Some("email") => json!("user@example.com"),
            _ => json!("string"),
        },
        Some("integer") => json!(0),
        Some("number") => json!(0.0),
        Some("boolean") => json!(false),
        Some("array") => {
            let item = s.get("items").map(|i| schema_example(root, i, depth + 1));
            Value::Array(item.into_iter().collect())
        }
        Some("object") => {
            let mut o = serde_json::Map::new();
            if let Some(props) = s.get("properties").and_then(Value::as_object) {
                for (k, v) in props {
                    o.insert(k.clone(), schema_example(root, v, depth + 1));
                }
            }
            Value::Object(o)
        }
        _ => Value::Null,
    }
}

fn apply_body(root: &Value, body: &Value, req: &mut HttpRequest) {
    let Some(content) = body.get("content").and_then(Value::as_object) else { return };
    // Prefer JSON, then whatever comes first.
    let (ct, media) = content
        .iter()
        .find(|(k, _)| k.contains("json"))
        .or_else(|| content.iter().next())
        .map(|(k, v)| (k.as_str(), v))
        .unwrap();
    let example = example_value(root, media);
    let fields = |ex: &Option<Value>| -> Vec<HeaderPair> {
        ex.as_ref()
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(k, v)| HeaderPair {
                        key: k.clone(),
                        value: match v {
                            Value::String(s) => s.clone(),
                            Value::Null => String::new(),
                            other => other.to_string(),
                        },
                        enabled: true,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    req.body = if ct.starts_with("application/x-www-form-urlencoded") {
        RequestBody::FormUrlEncoded(fields(&example))
    } else if ct.starts_with("multipart/form-data") {
        RequestBody::FormData(fields(&example))
    } else {
        let text = match example {
            Some(Value::String(s)) => s,
            Some(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            None => String::new(),
        };
        RequestBody::Raw { content_type: ct.to_string(), body: text }
    };
}

/// Map the first applicable security requirement to an [`AuthConfig`].
fn security_auth(root: &Value, op: &Value) -> AuthConfig {
    let reqs = op.get("security").or_else(|| root.get("security"));
    let Some(first) = reqs
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find_map(|r| r.as_object().and_then(|o| o.keys().next())))
    else {
        return AuthConfig::None;
    };
    let Some(scheme) = root
        .pointer("/components/securitySchemes")
        .and_then(|s| s.get(first))
        .map(|s| resolve(root, s))
    else {
        return AuthConfig::None;
    };
    match scheme.get("type").and_then(Value::as_str) {
        Some("http") => match scheme
            .get("scheme")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("bearer") => AuthConfig::Bearer { token: "{{token}}".into() },
            Some("basic") => AuthConfig::Basic {
                username: "{{username}}".into(),
                password: "{{password}}".into(),
            },
            _ => AuthConfig::None,
        },
        Some("oauth2") | Some("openIdConnect") => AuthConfig::Bearer { token: "{{token}}".into() },
        Some("apiKey") => match scheme.get("in").and_then(Value::as_str) {
            Some(loc @ ("header" | "query")) => AuthConfig::ApiKey {
                key: scheme.get("name").and_then(Value::as_str).unwrap_or("api_key").into(),
                value: "{{apiKey}}".into(),
                in_header: loc == "header",
            },
            _ => AuthConfig::None,
        },
        _ => AuthConfig::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = r##"{
      "openapi": "3.0.3",
      "info": {"title": "Pet Store", "version": "1.0", "description": "A store"},
      "servers": [{"url": "https://petstore.example.com/v1/"}],
      "tags": [{"name": "pets", "description": "Pet ops"}],
      "security": [{"bearerAuth": []}],
      "paths": {
        "/pets": {
          "parameters": [{"name": "X-Trace", "in": "header", "schema": {"type": "string"}}],
          "get": {
            "tags": ["pets"],
            "summary": "List pets",
            "parameters": [
              {"name": "limit", "in": "query", "required": true, "schema": {"type": "integer"}},
              {"$ref": "#/components/parameters/Offset"}
            ]
          },
          "post": {
            "tags": ["pets"],
            "operationId": "createPet",
            "requestBody": {
              "required": true,
              "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Pet"}}}
            }
          }
        },
        "/pets/{petId}": {
          "delete": {
            "summary": "Delete pet",
            "parameters": [{"name": "petId", "in": "path", "required": true, "schema": {"type": "string"}}],
            "security": [{"keyAuth": []}]
          }
        },
        "/login": {
          "post": {
            "summary": "Login",
            "security": [],
            "requestBody": {"content": {"application/x-www-form-urlencoded": {
              "schema": {"type": "object", "properties": {"user": {"type": "string"}, "pw": {"type": "string"}}}
            }}}
          }
        }
      },
      "components": {
        "parameters": {
          "Offset": {"name": "offset", "in": "query", "schema": {"type": "integer", "default": 5}}
        },
        "schemas": {
          "Pet": {
            "type": "object",
            "required": ["name"],
            "properties": {
              "name": {"type": "string", "example": "Rex"},
              "age": {"type": "integer"},
              "tags": {"type": "array", "items": {"type": "string"}}
            }
          }
        },
        "securitySchemes": {
          "bearerAuth": {"type": "http", "scheme": "bearer"},
          "keyAuth": {"type": "apiKey", "in": "header", "name": "X-API-Key"}
        }
      }
    }"##;

    fn find<'a>(items: &'a [Item], name: &str) -> Option<&'a HttpRequest> {
        items.iter().find_map(|i| match i {
            Item::Request(r) if r.name == name => Some(r),
            Item::Folder(f) => find(&f.items, name),
            _ => None,
        })
    }

    #[test]
    fn imports_info_and_grouping() {
        let c = import_openapi(SPEC).unwrap();
        assert_eq!(c.name, "Pet Store");
        assert_eq!(c.description.as_deref(), Some("A store"));
        // folder "pets" + 2 untagged requests
        assert_eq!(c.items.len(), 3);
        let Item::Folder(f) = &c.items[0] else { panic!("expected folder first") };
        assert_eq!(f.name, "pets");
        assert_eq!(f.description.as_deref(), Some("Pet ops"));
        assert_eq!(f.items.len(), 2);
    }

    #[test]
    fn imports_parameters() {
        let c = import_openapi(SPEC).unwrap();
        let r = find(&c.items, "List pets").unwrap();
        assert_eq!(r.method, HttpMethod::GET);
        assert_eq!(r.url, "https://petstore.example.com/v1/pets");
        assert_eq!(r.query_params.len(), 2);
        assert_eq!(r.query_params[0].key, "limit");
        assert!(r.query_params[0].enabled);
        assert_eq!(r.query_params[1].key, "offset");
        assert_eq!(r.query_params[1].value, "5");
        assert!(!r.query_params[1].enabled);
        // path-level header parameter is inherited
        assert_eq!(r.headers.len(), 1);
        assert_eq!(r.headers[0].key, "X-Trace");
    }

    #[test]
    fn imports_request_body_from_ref_schema() {
        let c = import_openapi(SPEC).unwrap();
        let r = find(&c.items, "createPet").unwrap();
        assert_eq!(r.method, HttpMethod::POST);
        let RequestBody::Raw { content_type, body } = &r.body else { panic!() };
        assert_eq!(content_type, "application/json");
        let v: Value = serde_json::from_str(body).unwrap();
        assert_eq!(v, json!({"name": "Rex", "age": 0, "tags": ["string"]}));
    }

    #[test]
    fn imports_form_body_and_security() {
        let c = import_openapi(SPEC).unwrap();
        let login = find(&c.items, "Login").unwrap();
        let RequestBody::FormUrlEncoded(f) = &login.body else { panic!() };
        assert_eq!(f.len(), 2);
        assert_eq!(login.auth, AuthConfig::None, "empty security overrides global");

        let del = find(&c.items, "Delete pet").unwrap();
        assert_eq!(del.method, HttpMethod::DELETE);
        assert_eq!(del.url, "https://petstore.example.com/v1/pets/{petId}");
        assert_eq!(
            del.auth,
            AuthConfig::ApiKey { key: "X-API-Key".into(), value: "{{apiKey}}".into(), in_header: true }
        );

        let list = find(&c.items, "List pets").unwrap();
        assert_eq!(list.auth, AuthConfig::Bearer { token: "{{token}}".into() });
    }

    #[test]
    fn supports_3_1_and_default_base_url() {
        let spec = r#"{"openapi":"3.1.0","info":{"title":"T","version":"1"},
          "paths":{"/a":{"get":{"summary":"A","parameters":[
            {"name":"q","in":"query","schema":{"type":["string","null"]}}]}}}}"#;
        let c = import_openapi(spec).unwrap();
        let r = find(&c.items, "A").unwrap();
        assert_eq!(r.url, "{{baseUrl}}/a");
        assert_eq!(r.query_params[0].value, "string");
    }

    #[test]
    fn rejects_invalid() {
        assert!(import_openapi("{").is_err());
        assert!(import_openapi(r#"{"info":{"title":"x"}}"#).is_err());
        assert!(import_openapi(r#"{"swagger":"2.0","info":{"title":"x"},"paths":{}}"#).is_err());
        assert!(import_openapi(r#"{"openapi":"2.0.0","info":{"title":"x"},"paths":{}}"#).is_err());
    }

    #[test]
    fn openapi_exports_to_postman() {
        let c = import_openapi(SPEC).unwrap();
        let out = crate::importers::postman::export_postman(&c).unwrap();
        let back = crate::importers::postman::import_postman(&out).unwrap();
        assert_eq!(back.name, "Pet Store");
        assert!(find(&back.items, "createPet").is_some());
    }
}
