use serde_json::{json, Value};

use super::client::{GraphQLRequest, GraphQLResponse};
use crate::engine::HttpExecutor;
use crate::error::RustmanError;
use crate::models::http::HeaderPair;

/// Standard introspection query (types, kinds, fields) plus root operation type names.
pub const INTROSPECTION_QUERY: &str = "query IntrospectionQuery { __schema { \
queryType { name } mutationType { name } \
types { name kind description fields { name type { name kind } } } } }";

pub struct GraphQLIntrospector;

impl GraphQLIntrospector {
    /// Introspect a GraphQL endpoint. Returns the `__schema` object with two extra
    /// convenience keys: `queries` and `mutations` (the fields of the root types).
    pub async fn introspect(url: &str, headers: &[HeaderPair]) -> Result<Value, RustmanError> {
        let http = GraphQLRequest::new(INTROSPECTION_QUERY).to_http_request(url, headers);
        let (res, _) = HttpExecutor::new().execute(&http).await?;
        let gql = GraphQLResponse::from_http_response(&res).map_err(|e| {
            if res.status >= 400 {
                RustmanError::NetworkError(format!("introspection failed: HTTP {}", res.status))
            } else {
                e
            }
        })?;
        if let Some(errs) = gql.errors.as_ref().filter(|e| !e.is_empty()) {
            let msg = errs
                .iter()
                .filter_map(|e| e.get("message").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(RustmanError::InvalidRequest(format!("introspection error: {msg}")));
        }
        let mut schema = gql
            .data
            .and_then(|d| d.get("__schema").cloned())
            .filter(Value::is_object)
            .ok_or_else(|| {
                RustmanError::SerializationError("response has no data.__schema".into())
            })?;
        let queries = root_fields(&schema, "queryType");
        let mutations = root_fields(&schema, "mutationType");
        schema["queries"] = queries;
        schema["mutations"] = mutations;
        Ok(schema)
    }
}

fn root_fields(schema: &Value, key: &str) -> Value {
    let Some(name) = schema.get(key).and_then(|t| t.get("name")).and_then(Value::as_str) else {
        return json!([]);
    };
    schema
        .get("types")
        .and_then(Value::as_array)
        .and_then(|ts| ts.iter().find(|t| t["name"] == name))
        .and_then(|t| t.get("fields").cloned())
        .filter(Value::is_array)
        .unwrap_or_else(|| json!([]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};

    async fn serve(app: Router) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}/graphql")
    }

    #[tokio::test]
    async fn introspects_mock_server() {
        let app = Router::new().route(
            "/graphql",
            post(|headers: axum::http::HeaderMap, Json(b): Json<Value>| async move {
                assert!(b["query"].as_str().unwrap().contains("__schema"));
                assert_eq!(headers["x-token"], "abc");
                Json(json!({"data": {"__schema": {
                    "queryType": {"name": "Query"},
                    "mutationType": {"name": "Mutation"},
                    "types": [
                        {"name": "Query", "kind": "OBJECT", "description": null,
                         "fields": [{"name": "user", "type": {"name": "User", "kind": "OBJECT"}}]},
                        {"name": "Mutation", "kind": "OBJECT", "description": null,
                         "fields": [{"name": "addUser", "type": {"name": "User", "kind": "OBJECT"}}]},
                        {"name": "User", "kind": "OBJECT", "description": null, "fields": []}
                    ]}}}))
            }),
        );
        let url = serve(app).await;
        let h = [HeaderPair { key: "x-token".into(), value: "abc".into(), enabled: true }];
        let s = GraphQLIntrospector::introspect(&url, &h).await.unwrap();
        assert_eq!(s["types"].as_array().unwrap().len(), 3);
        assert_eq!(s["queries"][0]["name"], "user");
        assert_eq!(s["mutations"][0]["name"], "addUser");
    }

    #[tokio::test]
    async fn introspection_errors_are_reported() {
        let app = Router::new().route(
            "/graphql",
            post(|| async { Json(json!({"errors": [{"message": "introspection disabled"}]})) }),
        );
        let url = serve(app).await;
        let e = GraphQLIntrospector::introspect(&url, &[]).await.unwrap_err();
        assert!(e.to_string().contains("introspection disabled"));
    }
}
