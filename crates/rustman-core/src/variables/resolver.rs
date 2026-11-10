use std::collections::HashMap;

use chrono::{SecondsFormat, Utc};
use uuid::Uuid;

use crate::models::environment::Environment;
use crate::models::http::{AuthConfig, HeaderPair, HttpRequest, QueryParam, RequestBody};

/// Resolves `{{var}}` placeholders using layered variables.
///
/// Precedence (highest first): Environment > Collection > Global.
/// Unknown variables are left untouched.
#[derive(Debug, Clone, Default)]
pub struct VariableResolver {
    global: HashMap<String, String>,
    collection: HashMap<String, String>,
    environment: HashMap<String, String>,
}

impl VariableResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_global(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.global.insert(key.into(), value.into());
    }

    pub fn set_collection(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.collection.insert(key.into(), value.into());
    }

    pub fn set_environment(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.environment.insert(key.into(), value.into());
    }

    pub fn with_global(mut self, vars: HashMap<String, String>) -> Self {
        self.global = vars;
        self
    }

    pub fn with_collection(mut self, vars: HashMap<String, String>) -> Self {
        self.collection = vars;
        self
    }

    pub fn with_environment(mut self, vars: HashMap<String, String>) -> Self {
        self.environment = vars;
        self
    }

    /// Loads enabled variables of an `Environment` as the environment layer.
    pub fn with_environment_model(mut self, env: &Environment) -> Self {
        self.environment = env
            .variables
            .iter()
            .filter(|v| v.enabled)
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect();
        self
    }

    fn lookup(&self, name: &str) -> Option<String> {
        if let Some(rest) = name.strip_prefix('$') {
            return Self::dynamic(rest);
        }
        self.environment
            .get(name)
            .or_else(|| self.collection.get(name))
            .or_else(|| self.global.get(name))
            .cloned()
    }

    fn dynamic(name: &str) -> Option<String> {
        match name {
            "guid" | "uuid" => Some(Uuid::new_v4().to_string()),
            "timestamp" => Some(Utc::now().timestamp().to_string()),
            "isoTimestamp" => Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
            "randomInt" => {
                let b = Uuid::new_v4().as_u128();
                Some(((b % 1001) as u32).to_string())
            }
            "randomEmail" => Some(format!("test-{}@example.com", Uuid::new_v4())),
            _ => None,
        }
    }

    /// Replaces every `{{name}}` in `input`. Unknown names are kept verbatim.
    pub fn resolve(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            match after.find("}}") {
                Some(end) => {
                    let name = after[..end].trim();
                    match self.lookup(name) {
                        Some(v) => out.push_str(&v),
                        None => out.push_str(&rest[start..start + 2 + end + 2]),
                    }
                    rest = &after[end + 2..];
                }
                None => {
                    out.push_str(&rest[start..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    fn resolve_pairs(&self, pairs: &[HeaderPair]) -> Vec<HeaderPair> {
        pairs
            .iter()
            .map(|p| HeaderPair {
                key: self.resolve(&p.key),
                value: self.resolve(&p.value),
                enabled: p.enabled,
            })
            .collect()
    }

    /// Returns a copy of `req` with all interpolatable fields resolved.
    pub fn resolve_request(&self, req: &HttpRequest) -> HttpRequest {
        let mut out = req.clone();
        out.url = self.resolve(&req.url);
        out.headers = self.resolve_pairs(&req.headers);
        out.query_params = req
            .query_params
            .iter()
            .map(|p| QueryParam {
                key: self.resolve(&p.key),
                value: self.resolve(&p.value),
                enabled: p.enabled,
            })
            .collect();
        out.auth = match &req.auth {
            AuthConfig::None => AuthConfig::None,
            AuthConfig::Bearer { token } => AuthConfig::Bearer {
                token: self.resolve(token),
            },
            AuthConfig::Basic { username, password } => AuthConfig::Basic {
                username: self.resolve(username),
                password: self.resolve(password),
            },
            AuthConfig::ApiKey {
                key,
                value,
                in_header,
            } => AuthConfig::ApiKey {
                key: self.resolve(key),
                value: self.resolve(value),
                in_header: *in_header,
            },
        };
        out.body = match &req.body {
            RequestBody::None => RequestBody::None,
            RequestBody::Raw { content_type, body } => RequestBody::Raw {
                content_type: content_type.clone(),
                body: self.resolve(body),
            },
            RequestBody::FormUrlEncoded(p) => RequestBody::FormUrlEncoded(self.resolve_pairs(p)),
            RequestBody::FormData(p) => RequestBody::FormData(self.resolve_pairs(p)),
        };
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::environment::Variable;
    use crate::models::http::HttpMethod;

    fn layered() -> VariableResolver {
        let mut r = VariableResolver::new();
        r.set_global("host", "global.com");
        r.set_global("g_only", "G");
        r.set_collection("host", "collection.com");
        r.set_collection("c_only", "C");
        r.set_environment("host", "env.com");
        r
    }

    #[test]
    fn environment_overrides_collection_and_global() {
        assert_eq!(layered().resolve("https://{{host}}"), "https://env.com");
    }

    #[test]
    fn collection_overrides_global() {
        let mut r = VariableResolver::new();
        r.set_global("a", "g");
        r.set_collection("a", "c");
        assert_eq!(r.resolve("{{a}}"), "c");
    }

    #[test]
    fn falls_back_through_layers() {
        let r = layered();
        assert_eq!(r.resolve("{{g_only}}-{{c_only}}"), "G-C");
    }

    #[test]
    fn unknown_and_unclosed_left_intact() {
        let r = layered();
        assert_eq!(r.resolve("{{nope}} {{host"), "{{nope}} {{host");
        assert_eq!(r.resolve("{{ host }}"), "env.com");
    }

    #[test]
    fn dynamic_guid_uuid() {
        let r = VariableResolver::new();
        let g = r.resolve("{{$guid}}");
        assert!(Uuid::parse_str(&g).is_ok());
        let u = r.resolve("{{$uuid}}");
        assert!(Uuid::parse_str(&u).is_ok());
        assert_ne!(g, u);
    }

    #[test]
    fn dynamic_timestamps() {
        let r = VariableResolver::new();
        let ts: i64 = r.resolve("{{$timestamp}}").parse().unwrap();
        assert!((Utc::now().timestamp() - ts).abs() <= 2);
        let iso = r.resolve("{{$isoTimestamp}}");
        assert!(chrono::DateTime::parse_from_rfc3339(&iso).is_ok());
        assert!(iso.ends_with('Z'));
    }

    #[test]
    fn dynamic_random_int_and_email() {
        let r = VariableResolver::new();
        for _ in 0..50 {
            let n: u32 = r.resolve("{{$randomInt}}").parse().unwrap();
            assert!(n <= 1000);
        }
        let e = r.resolve("{{$randomEmail}}");
        assert!(e.starts_with("test-") && e.ends_with("@example.com"));
    }

    #[test]
    fn unknown_dynamic_left_intact() {
        assert_eq!(VariableResolver::new().resolve("{{$bogus}}"), "{{$bogus}}");
    }

    #[test]
    fn environment_model_skips_disabled() {
        let env = Environment {
            id: Uuid::new_v4(),
            name: "e".into(),
            variables: vec![
                Variable { key: "a".into(), value: "1".into(), enabled: true, secret: false },
                Variable { key: "b".into(), value: "2".into(), enabled: false, secret: false },
            ],
        };
        let r = VariableResolver::new().with_environment_model(&env);
        assert_eq!(r.resolve("{{a}}{{b}}"), "1{{b}}");
    }

    #[test]
    fn resolve_request_all_fields() {
        let mut r = VariableResolver::new();
        r.set_environment("host", "api.test");
        r.set_environment("tok", "secret");
        r.set_environment("q", "42");
        r.set_environment("name", "bob");

        let mut req = HttpRequest::new("r", HttpMethod::POST, "https://{{host}}/x");
        req.headers.push(HeaderPair { key: "X-{{name}}".into(), value: "{{tok}}".into(), enabled: true });
        req.query_params.push(QueryParam { key: "q".into(), value: "{{q}}".into(), enabled: true });
        req.auth = AuthConfig::Bearer { token: "{{tok}}".into() };
        req.body = RequestBody::Raw { content_type: "application/json".into(), body: "{\"n\":\"{{name}}\"}".into() };

        let out = r.resolve_request(&req);
        assert_eq!(out.url, "https://api.test/x");
        assert_eq!(out.headers[0].key, "X-bob");
        assert_eq!(out.headers[0].value, "secret");
        assert_eq!(out.query_params[0].value, "42");
        assert_eq!(out.auth, AuthConfig::Bearer { token: "secret".into() });
        assert_eq!(
            out.body,
            RequestBody::Raw { content_type: "application/json".into(), body: "{\"n\":\"bob\"}".into() }
        );
        assert_eq!(out.id, req.id);
    }

    #[test]
    fn resolve_request_form_and_apikey() {
        let mut r = VariableResolver::new();
        r.set_global("k", "key1");
        r.set_global("v", "val1");
        let mut req = HttpRequest::new("r", HttpMethod::POST, "http://a");
        req.auth = AuthConfig::ApiKey { key: "{{k}}".into(), value: "{{v}}".into(), in_header: true };
        req.body = RequestBody::FormUrlEncoded(vec![HeaderPair { key: "{{k}}".into(), value: "{{v}}".into(), enabled: true }]);
        let out = r.resolve_request(&req);
        assert_eq!(out.auth, AuthConfig::ApiKey { key: "key1".into(), value: "val1".into(), in_header: true });
        assert_eq!(
            out.body,
            RequestBody::FormUrlEncoded(vec![HeaderPair { key: "key1".into(), value: "val1".into(), enabled: true }])
        );
    }
}
