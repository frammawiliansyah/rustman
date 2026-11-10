//! Script runtime built on the pure-Rust `boa_engine` JavaScript interpreter.
//!
//! The Postman `pm.*` API is implemented as a small JavaScript prelude
//! (`prelude.js`). Input state is injected as JSON and the resulting state is
//! read back as JSON, so no host functions are exposed to scripts: there is no
//! filesystem or network access, and loop/recursion limits bound execution.

use std::collections::HashMap;

use boa_engine::{Context, Source};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::RustmanError;
use crate::models::assertion::AssertionResult;
use crate::models::http::HttpResponse;

const PRELUDE: &str = include_str!("prelude.js");
const LOOP_ITERATION_LIMIT: u64 = 1_000_000;
const RECURSION_LIMIT: usize = 400;

/// Variable state visible to (and mutated by) a script.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScriptScope {
    pub environment: HashMap<String, String>,
    pub variables: HashMap<String, String>,
    pub globals: HashMap<String, String>,
}

/// Result of running a script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptOutput {
    /// Variable state after the script ran.
    pub scope: ScriptScope,
    /// One entry per `pm.test(...)` call.
    pub assertions: Vec<AssertionResult>,
    /// Lines written through `console.log` and friends.
    pub console: Vec<String>,
}

#[derive(Deserialize)]
struct Collected {
    environment: HashMap<String, String>,
    variables: HashMap<String, String>,
    globals: HashMap<String, String>,
    results: Vec<AssertionResult>,
    logs: Vec<String>,
}

/// Executes pre-request and test scripts with a Postman-compatible `pm` API.
#[derive(Clone, Debug, Default)]
pub struct ScriptRuntime;

impl ScriptRuntime {
    pub fn new() -> Self {
        Self
    }

    /// Runs a pre-request script. `pm.environment` / `pm.variables` changes are
    /// returned in `ScriptOutput::scope`; `pm.response` is unavailable.
    pub fn run_pre_request(
        &self,
        script: &str,
        scope: &ScriptScope,
    ) -> Result<ScriptOutput, RustmanError> {
        self.run(script, scope, None, "prerequest")
    }

    /// Runs a test (post-response) script against `response`.
    pub fn run_test(
        &self,
        script: &str,
        scope: &ScriptScope,
        response: &HttpResponse,
    ) -> Result<ScriptOutput, RustmanError> {
        self.run(script, scope, Some(response), "test")
    }

    fn run(
        &self,
        script: &str,
        scope: &ScriptScope,
        response: Option<&HttpResponse>,
        event: &str,
    ) -> Result<ScriptOutput, RustmanError> {
        let response_json = response.map(|r| {
            json!({
                "code": r.status,
                "status": status_text(r.status),
                "responseTime": r.elapsed_ms,
                "body": r.body,
                "headers": r.headers.iter().map(|h| [&h.key, &h.value]).collect::<Vec<_>>(),
            })
        });
        let input = json!({
            "event": event,
            "environment": scope.environment,
            "variables": scope.variables,
            "globals": scope.globals,
            "response": response_json,
        });
        // Double-encode: a JS string literal holding the JSON text.
        let input_literal = serde_json::to_string(&input.to_string())?;

        let mut ctx = Context::default();
        ctx.runtime_limits_mut()
            .set_loop_iteration_limit(LOOP_ITERATION_LIMIT);
        ctx.runtime_limits_mut().set_recursion_limit(RECURSION_LIMIT);

        let prelude = PRELUDE.replace("__INPUT__", &input_literal);
        ctx.eval(Source::from_bytes(prelude.as_str()))
            .map_err(|e| RustmanError::ScriptError(format!("prelude: {e}")))?;

        ctx.eval(Source::from_bytes(script))
            .map_err(|e| RustmanError::ScriptError(e.to_string()))?;

        let collected = ctx
            .eval(Source::from_bytes("__collect()"))
            .map_err(|e| RustmanError::ScriptError(e.to_string()))?;
        let text = collected
            .as_string()
            .map(|s| s.to_std_string_escaped())
            .ok_or_else(|| RustmanError::ScriptError("invalid script output".into()))?;
        let c: Collected = serde_json::from_str(&text)?;

        Ok(ScriptOutput {
            scope: ScriptScope {
                environment: c.environment,
                variables: c.variables,
                globals: c.globals,
            },
            assertions: c.results,
            console: c.logs,
        })
    }
}

fn status_text(code: u16) -> String {
    reqwest::StatusCode::from_u16(code)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::http::HeaderPair;
    use chrono::Utc;

    fn response(status: u16, body: &str, elapsed_ms: u64) -> HttpResponse {
        HttpResponse {
            status,
            headers: vec![HeaderPair {
                key: "Content-Type".into(),
                value: "application/json".into(),
                enabled: true,
            }],
            body: body.into(),
            elapsed_ms,
            timestamp: Utc::now(),
        }
    }

    fn test(script: &str, resp: &HttpResponse) -> ScriptOutput {
        ScriptRuntime::new()
            .run_test(script, &ScriptScope::default(), resp)
            .unwrap()
    }

    #[test]
    fn pre_request_sets_environment_and_variables() {
        let mut scope = ScriptScope::default();
        scope.environment.insert("host".into(), "example.com".into());
        let out = ScriptRuntime::new()
            .run_pre_request(
                r#"
                pm.environment.set("token", "abc-" + pm.environment.get("host"));
                pm.variables.set("count", 42);
                pm.variables.set("seen", pm.environment.get("token"));
                pm.environment.unset("host");
                console.log("hello", 1);
                "#,
                &scope,
            )
            .unwrap();
        assert_eq!(out.scope.environment.get("token").unwrap(), "abc-example.com");
        assert!(!out.scope.environment.contains_key("host"));
        assert_eq!(out.scope.variables.get("count").unwrap(), "42");
        assert_eq!(out.scope.variables.get("seen").unwrap(), "abc-example.com");
        assert_eq!(out.console, vec!["hello 1"]);
        // The input scope is untouched.
        assert!(scope.environment.contains_key("host"));
    }

    #[test]
    fn pm_variables_get_falls_back_to_environment() {
        let mut scope = ScriptScope::default();
        scope.environment.insert("a".into(), "env".into());
        let out = ScriptRuntime::new()
            .run_pre_request(r#"pm.variables.set("b", pm.variables.get("a"));"#, &scope)
            .unwrap();
        assert_eq!(out.scope.variables.get("b").unwrap(), "env");
    }

    #[test]
    fn test_script_status_time_and_json() {
        let resp = response(200, r#"{"user":{"name":"ada","id":7},"tags":["a","b"]}"#, 120);
        let out = test(
            r#"
            pm.test("status is 200", function () { pm.response.to.have.status(200); });
            pm.test("code is 200", function () { pm.expect(pm.response.code).to.eql(200); });
            pm.test("status text", function () { pm.expect(pm.response.status).to.equal("OK"); });
            pm.test("fast enough", function () { pm.expect(pm.response.responseTime).to.be.below(500); });
            pm.test("json field", function () {
                var j = pm.response.json();
                pm.expect(j.user.name).to.eql("ada");
                pm.expect(j.tags).to.eql(["a", "b"]);
                pm.expect(j.tags).to.include("b");
                pm.expect(j).to.have.property("user");
            });
            pm.test("text", function () { pm.expect(pm.response.text()).to.include("ada"); });
            pm.test("header", function () {
                pm.expect(pm.response.headers.get("content-type")).to.include("json");
            });
            "#,
            &resp,
        );
        assert_eq!(out.assertions.len(), 7);
        for a in &out.assertions {
            assert!(a.passed, "{} failed: {:?}", a.name, a.message);
        }
    }

    #[test]
    fn test_script_records_failures() {
        let resp = response(404, r#"{"id":1}"#, 900);
        let out = test(
            r#"
            pm.test("is 200", function () { pm.expect(pm.response.code).to.eql(200); });
            pm.test("fast", function () { pm.expect(pm.response.responseTime).to.be.below(500); });
            pm.test("id is 1", function () { pm.expect(pm.response.json().id).to.eql(1); });
            pm.test("negation", function () { pm.expect(pm.response.code).to.not.eql(200); });
            pm.test("throws", function () { throw new Error("boom"); });
            pm.test("bad json", function () { JSON.parse("{"); });
            "#,
            &resp,
        );
        let passed: Vec<bool> = out.assertions.iter().map(|a| a.passed).collect();
        assert_eq!(passed, vec![false, false, true, true, false, false]);
        assert!(out.assertions[0].message.as_ref().unwrap().contains("200"));
        assert_eq!(out.assertions[4].message.as_deref(), Some("boom"));
        assert_eq!(out.assertions[0].name, "is 200");
    }

    #[test]
    fn test_script_can_update_variables() {
        let resp = response(200, r#"{"token":"xyz"}"#, 10);
        let out = test(r#"pm.environment.set("token", pm.response.json().token);"#, &resp);
        assert_eq!(out.scope.environment.get("token").unwrap(), "xyz");
    }

    #[test]
    fn syntax_and_runtime_errors_are_reported() {
        let rt = ScriptRuntime::new();
        let scope = ScriptScope::default();
        assert!(matches!(
            rt.run_pre_request("this is not js", &scope),
            Err(RustmanError::ScriptError(_))
        ));
        assert!(matches!(
            rt.run_pre_request("undefinedFn()", &scope),
            Err(RustmanError::ScriptError(_))
        ));
    }

    #[test]
    fn infinite_loop_is_bounded() {
        let rt = ScriptRuntime::new();
        let r = rt.run_pre_request("while (true) {}", &ScriptScope::default());
        assert!(matches!(r, Err(RustmanError::ScriptError(_))));
    }

    #[test]
    fn special_characters_in_state_round_trip() {
        let mut scope = ScriptScope::default();
        scope.environment.insert("q".into(), "quote\" back\\slash\nnewline ü".into());
        let out = ScriptRuntime::new()
            .run_pre_request(r#"pm.environment.set("copy", pm.environment.get("q"));"#, &scope)
            .unwrap();
        assert_eq!(out.scope.environment["copy"], scope.environment["q"]);
    }
}
