//! Evaluates assertions against an `HttpResponse`.

use crate::models::assertion::{AssertionResult, AssertionType};
use crate::models::http::HttpResponse;
use serde_json::Value;

/// Aggregated outcome of a set of assertions.
#[derive(Clone, Debug, PartialEq)]
pub struct AssertionSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<AssertionResult>,
}

impl AssertionSummary {
    pub fn summarize(results: Vec<AssertionResult>) -> AssertionSummary {
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        AssertionSummary {
            total,
            passed,
            failed: total - passed,
            results,
        }
    }
}

pub fn evaluate_all(assertions: &[AssertionType], response: &HttpResponse) -> Vec<AssertionResult> {
    assertions
        .iter()
        .map(|a| evaluate_assertion(a, response))
        .collect()
}

fn result(name: String, passed: bool, fail_msg: impl FnOnce() -> String) -> AssertionResult {
    AssertionResult {
        name,
        passed,
        message: if passed { None } else { Some(fail_msg()) },
    }
}

fn find_header<'a>(response: &'a HttpResponse, key: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|h| h.key.eq_ignore_ascii_case(key))
        .map(|h| h.value.as_str())
}

/// Resolve a JSON pointer (`/a/b`) or dotted path (`a.b`, `items.0.id`, `$.a.b`).
fn lookup<'a>(json: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(json);
    }
    if path.starts_with('/') {
        return json.pointer(path);
    }
    let path = path.strip_prefix("$.").or_else(|| path.strip_prefix('$')).unwrap_or(path);
    let mut cur = json;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = match cur {
            Value::Object(m) => m.get(seg)?,
            Value::Array(a) => a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn evaluate_assertion(assertion: &AssertionType, response: &HttpResponse) -> AssertionResult {
    match assertion {
        AssertionType::StatusCode(expected) => result(
            format!("Status code is {expected}"),
            response.status == *expected,
            || format!("Expected status {expected}, got {}", response.status),
        ),
        AssertionType::MaxResponseTime(max_ms) => result(
            format!("Response time <= {max_ms}ms"),
            response.elapsed_ms <= *max_ms,
            || format!("Response took {}ms, max allowed {max_ms}ms", response.elapsed_ms),
        ),
        AssertionType::HeaderExists(key) => result(
            format!("Header '{key}' exists"),
            find_header(response, key).is_some(),
            || format!("Header '{key}' not found"),
        ),
        AssertionType::HeaderValue { key, expected } => {
            let actual = find_header(response, key);
            result(
                format!("Header '{key}' equals '{expected}'"),
                actual == Some(expected.as_str()),
                || match actual {
                    Some(a) => format!("Header '{key}' expected '{expected}', got '{a}'"),
                    None => format!("Header '{key}' not found"),
                },
            )
        }
        AssertionType::JsonPathEquals { path, expected } => {
            let name = format!("JSON path '{path}' equals '{expected}'");
            match serde_json::from_str::<Value>(&response.body) {
                Err(e) => result(name, false, || format!("Body is not valid JSON: {e}")),
                Ok(json) => {
                    let actual = lookup(&json, path).map(value_to_string);
                    result(name, actual.as_deref() == Some(expected.as_str()), || {
                        match &actual {
                            Some(a) => format!("Path '{path}' expected '{expected}', got '{a}'"),
                            None => format!("Path '{path}' not found in JSON body"),
                        }
                    })
                }
            }
        }
        AssertionType::BodyContains(substr) => result(
            format!("Body contains '{substr}'"),
            response.body.contains(substr.as_str()),
            || format!("Body does not contain '{substr}'"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::http::HeaderPair;
    use chrono::Utc;

    fn resp() -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: vec![HeaderPair {
                key: "Content-Type".into(),
                value: "application/json".into(),
                enabled: true,
            }],
            body: r#"{"user":{"name":"alice","age":30},"items":[{"id":7}]}"#.into(),
            elapsed_ms: 120,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn status_code() {
        assert!(evaluate_assertion(&AssertionType::StatusCode(200), &resp()).passed);
        let r = evaluate_assertion(&AssertionType::StatusCode(404), &resp());
        assert!(!r.passed);
        assert!(r.message.unwrap().contains("404"));
    }

    #[test]
    fn max_response_time() {
        assert!(evaluate_assertion(&AssertionType::MaxResponseTime(120), &resp()).passed);
        assert!(!evaluate_assertion(&AssertionType::MaxResponseTime(100), &resp()).passed);
    }

    #[test]
    fn header_exists_case_insensitive() {
        assert!(evaluate_assertion(&AssertionType::HeaderExists("content-type".into()), &resp()).passed);
        assert!(!evaluate_assertion(&AssertionType::HeaderExists("x-missing".into()), &resp()).passed);
    }

    #[test]
    fn header_value() {
        let ok = AssertionType::HeaderValue { key: "CONTENT-TYPE".into(), expected: "application/json".into() };
        let bad = AssertionType::HeaderValue { key: "content-type".into(), expected: "text/html".into() };
        let missing = AssertionType::HeaderValue { key: "x-none".into(), expected: "a".into() };
        assert!(evaluate_assertion(&ok, &resp()).passed);
        assert!(!evaluate_assertion(&bad, &resp()).passed);
        assert!(!evaluate_assertion(&missing, &resp()).passed);
    }

    #[test]
    fn json_path_pointer_and_dot() {
        let eq = |p: &str, e: &str| {
            evaluate_assertion(
                &AssertionType::JsonPathEquals { path: p.into(), expected: e.into() },
                &resp(),
            )
        };
        assert!(eq("/user/name", "alice").passed);
        assert!(eq("user.name", "alice").passed);
        assert!(eq("$.user.age", "30").passed);
        assert!(eq("items.0.id", "7").passed);
        assert!(!eq("user.name", "bob").passed);
        assert!(!eq("user.missing", "x").passed);
    }

    #[test]
    fn json_path_invalid_body() {
        let mut r = resp();
        r.body = "not json".into();
        let res = evaluate_assertion(
            &AssertionType::JsonPathEquals { path: "a".into(), expected: "b".into() },
            &r,
        );
        assert!(!res.passed);
        assert!(res.message.unwrap().contains("not valid JSON"));
    }

    #[test]
    fn body_contains() {
        assert!(evaluate_assertion(&AssertionType::BodyContains("alice".into()), &resp()).passed);
        assert!(!evaluate_assertion(&AssertionType::BodyContains("zzz".into()), &resp()).passed);
    }

    #[test]
    fn evaluate_all_and_summarize() {
        let assertions = vec![
            AssertionType::StatusCode(200),
            AssertionType::StatusCode(500),
            AssertionType::BodyContains("alice".into()),
        ];
        let results = evaluate_all(&assertions, &resp());
        assert_eq!(results.len(), 3);
        let s = AssertionSummary::summarize(results);
        assert_eq!((s.total, s.passed, s.failed), (3, 2, 1));
        assert_eq!(s.results.len(), 3);
    }
}
