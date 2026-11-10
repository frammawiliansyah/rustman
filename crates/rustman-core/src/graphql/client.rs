use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::RustmanError;
use crate::models::http::{HeaderPair, HttpMethod, HttpRequest, HttpResponse, RequestBody};

/// A GraphQL operation that can be turned into an HTTP POST request.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct GraphQLRequest {
    pub query: String,
    #[serde(default)]
    pub operation_name: Option<String>,
    #[serde(default)]
    pub variables: Option<Value>,
}

impl GraphQLRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self { query: query.into(), ..Default::default() }
    }

    /// Build the JSON payload; absent variables / operation name are sent as `null`.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("query".into(), json!(self.query));
        m.insert("variables".into(), self.variables.clone().unwrap_or(Value::Null));
        m.insert("operationName".into(), json!(self.operation_name));
        Value::Object(m)
    }

    pub fn to_http_request(&self, url: &str, headers: &[HeaderPair]) -> HttpRequest {
        let mut req = HttpRequest::new("GraphQL", HttpMethod::POST, url);
        // Drop any caller-supplied Content-Type: ours must win.
        req.headers = headers
            .iter()
            .filter(|h| !h.key.trim().eq_ignore_ascii_case("content-type"))
            .cloned()
            .collect();
        req.headers.push(HeaderPair {
            key: "Content-Type".into(),
            value: "application/json".into(),
            enabled: true,
        });
        req.body = RequestBody::Raw {
            content_type: "application/json".into(),
            body: self.to_json().to_string(),
        };
        req
    }
}

/// Standard GraphQL response envelope.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct GraphQLResponse {
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub errors: Option<Vec<Value>>,
}

impl GraphQLResponse {
    pub fn from_http_response(res: &HttpResponse) -> Result<Self, RustmanError> {
        let parsed: GraphQLResponse = serde_json::from_str(&res.body).map_err(|e| {
            RustmanError::SerializationError(format!(
                "invalid GraphQL response (HTTP {}): {e}",
                res.status
            ))
        })?;
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn http_res(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: vec![],
            body: body.into(),
            elapsed_ms: 1,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn request_to_http_request() {
        let gq = GraphQLRequest {
            query: "query Q($id: ID!){user(id:$id){name}}".into(),
            operation_name: Some("Q".into()),
            variables: Some(json!({"id": "1"})),
        };
        let auth = HeaderPair { key: "Authorization".into(), value: "Bearer x".into(), enabled: true };
        let ct = HeaderPair { key: "content-type".into(), value: "text/plain".into(), enabled: true };
        let req = gq.to_http_request("http://x/graphql", &[auth.clone(), ct]);
        assert_eq!(req.method, HttpMethod::POST);
        assert_eq!(req.url, "http://x/graphql");
        assert_eq!(req.headers.len(), 2);
        assert_eq!(req.headers[0], auth);
        assert_eq!(req.headers[1].key, "Content-Type");
        assert_eq!(req.headers[1].value, "application/json");
        let RequestBody::Raw { content_type, body } = &req.body else { panic!("not raw") };
        assert_eq!(content_type, "application/json");
        let v: Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["query"], gq.query);
        assert_eq!(v["variables"]["id"], "1");
        assert_eq!(v["operationName"], "Q");
    }

    #[test]
    fn request_without_optionals_uses_null() {
        let req = GraphQLRequest::new("{a}").to_http_request("http://x", &[]);
        let RequestBody::Raw { body, .. } = &req.body else { panic!() };
        let v: Value = serde_json::from_str(body).unwrap();
        assert!(v["variables"].is_null());
        assert!(v["operationName"].is_null());
    }

    #[test]
    fn response_data_and_errors() {
        let r = GraphQLResponse::from_http_response(&http_res(
            r#"{"data":{"a":1},"errors":[{"message":"boom"}]}"#,
        ))
        .unwrap();
        assert_eq!(r.data.unwrap()["a"], 1);
        assert_eq!(r.errors.unwrap()[0]["message"], "boom");

        let r = GraphQLResponse::from_http_response(&http_res(r#"{"data":null}"#)).unwrap();
        assert!(r.data.is_none() && r.errors.is_none());
    }

    #[test]
    fn response_invalid_json_errors() {
        let e = GraphQLResponse::from_http_response(&http_res("<html>")).unwrap_err();
        assert!(matches!(e, RustmanError::SerializationError(_)));
    }
}
