//! Script runtime built on the pure-Rust `boa_engine` JavaScript interpreter.
//!
//! The Postman `pm.*` API is implemented as a small JavaScript prelude
//! (`prelude.js`). Input state is injected as JSON and the resulting state is
//! read back as JSON. A small set of host functions is exposed to the prelude:
//! hashing/base64 helpers (`CryptoJS`) and a blocking HTTP call backing
//! `pm.sendRequest`. There is no filesystem access, and loop/recursion limits
//! bound execution.

use std::collections::HashMap;

use base64::Engine as _;
use boa_engine::{js_string, Context, JsResult, JsValue, NativeFunction, Source};
use hmac::{Hmac, Mac};
use md5::Md5;
use sha2::{Digest, Sha256};
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
    /// Current row of the iteration data file (read-only in scripts).
    #[serde(default)]
    pub iteration_data: HashMap<String, String>,
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
            "iterationData": scope.iteration_data,
            "response": response_json,
        });
        // Double-encode: a JS string literal holding the JSON text.
        let input_literal = serde_json::to_string(&input.to_string())?;

        let mut ctx = Context::default();
        ctx.runtime_limits_mut()
            .set_loop_iteration_limit(LOOP_ITERATION_LIMIT);
        ctx.runtime_limits_mut().set_recursion_limit(RECURSION_LIMIT);

        register_host_functions(&mut ctx)
            .map_err(|e| RustmanError::ScriptError(format!("host: {e}")))?;

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
                iteration_data: scope.iteration_data.clone(),
            },
            assertions: c.results,
            console: c.logs,
        })
    }
}

const HTTP_TIMEOUT_SECS: u64 = 30;

fn arg_string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    match args.get(i) {
        Some(v) => Ok(v.to_string(ctx)?.to_std_string_escaped()),
        None => Ok(String::new()),
    }
}

fn register_host_functions(ctx: &mut Context) -> JsResult<()> {
    ctx.register_global_callable(
        js_string!("__md5"),
        1,
        NativeFunction::from_fn_ptr(|_, args, ctx| {
            let s = arg_string(args, 0, ctx)?;
            Ok(JsValue::from(boa_engine::JsString::from(hex::encode(Md5::digest(s.as_bytes())))))
        }),
    )?;
    ctx.register_global_callable(
        js_string!("__sha256"),
        1,
        NativeFunction::from_fn_ptr(|_, args, ctx| {
            let s = arg_string(args, 0, ctx)?;
            Ok(JsValue::from(boa_engine::JsString::from(hex::encode(Sha256::digest(s.as_bytes())))))
        }),
    )?;
    ctx.register_global_callable(
        js_string!("__hmac_sha256"),
        2,
        NativeFunction::from_fn_ptr(|_, args, ctx| {
            let msg = arg_string(args, 0, ctx)?;
            let key = arg_string(args, 1, ctx)?;
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key.as_bytes())
                .expect("HMAC accepts any key length");
            mac.update(msg.as_bytes());
            Ok(JsValue::from(boa_engine::JsString::from(hex::encode(mac.finalize().into_bytes()))))
        }),
    )?;
    ctx.register_global_callable(
        js_string!("__b64_encode"),
        1,
        NativeFunction::from_fn_ptr(|_, args, ctx| {
            let s = arg_string(args, 0, ctx)?;
            let out = base64::engine::general_purpose::STANDARD.encode(s.as_bytes());
            Ok(JsValue::from(boa_engine::JsString::from(out)))
        }),
    )?;
    ctx.register_global_callable(
        js_string!("__b64_decode"),
        1,
        NativeFunction::from_fn_ptr(|_, args, ctx| {
            let s = arg_string(args, 0, ctx)?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(s.trim().as_bytes())
                .map_err(|e| {
                    boa_engine::JsNativeError::error()
                        .with_message(format!("invalid base64: {e}"))
                })?;
            let out = String::from_utf8_lossy(&bytes).into_owned();
            Ok(JsValue::from(boa_engine::JsString::from(out)))
        }),
    )?;
    ctx.register_global_callable(
        js_string!("__http"),
        1,
        NativeFunction::from_fn_ptr(|_, args, ctx| {
            let req = arg_string(args, 0, ctx)?;
            let out = perform_http(&req);
            Ok(JsValue::from(boa_engine::JsString::from(out)))
        }),
    )?;
    Ok(())
}

#[derive(Deserialize)]
struct SubRequest {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
}

/// Executes a sub-request and returns a JSON string: either
/// `{"code","status","headers":[[k,v]],"body"}` or `{"error": "..."}`.
fn perform_http(req_json: &str) -> String {
    let result = (|| -> Result<serde_json::Value, String> {
        let req: SubRequest = serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        // Run on a dedicated thread with its own runtime so this works both
        // inside and outside an existing tokio runtime.
        std::thread::spawn(move || -> Result<serde_json::Value, String> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            rt.block_on(async move {
                let method = reqwest::Method::from_bytes(req.method.to_uppercase().as_bytes())
                    .map_err(|e| e.to_string())?;
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
                    .build()
                    .map_err(|e| e.to_string())?;
                let mut rb = client.request(method, &req.url);
                for (k, v) in &req.headers {
                    rb = rb.header(k, v);
                }
                if let Some(b) = req.body {
                    rb = rb.body(b);
                }
                let resp = rb.send().await.map_err(|e| e.to_string())?;
                let code = resp.status().as_u16();
                let headers: Vec<[String; 2]> = resp
                    .headers()
                    .iter()
                    .map(|(k, v)| [k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()])
                    .collect();
                let body = resp.text().await.map_err(|e| e.to_string())?;
                Ok(json!({
                    "code": code,
                    "status": status_text(code),
                    "headers": headers,
                    "body": body,
                }))
            })
        })
        .join()
        .map_err(|_| "request thread panicked".to_string())?
    })();
    match result {
        Ok(v) => v.to_string(),
        Err(e) => json!({ "error": e }).to_string(),
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
    fn pm_iteration_data_get_and_to_object() {
        let mut scope = ScriptScope::default();
        scope.iteration_data.insert("username".into(), "alice".into());
        scope.iteration_data.insert("id".into(), "1".into());
        let out = ScriptRuntime::new()
            .run_pre_request(
                r#"
                pm.variables.set("u", pm.iterationData.get("username"));
                pm.variables.set("has", pm.iterationData.has("id"));
                pm.variables.set("missing", String(pm.iterationData.get("nope")));
                pm.variables.set("obj", JSON.stringify(pm.iterationData.toObject()));
                "#,
                &scope,
            )
            .unwrap();
        assert_eq!(out.scope.variables["u"], "alice");
        assert_eq!(out.scope.variables["has"], "true");
        assert_eq!(out.scope.variables["missing"], "undefined");
        let obj: serde_json::Value = serde_json::from_str(&out.scope.variables["obj"]).unwrap();
        assert_eq!(obj["username"], "alice");
        assert_eq!(obj["id"], "1");
        assert_eq!(out.scope.iteration_data, scope.iteration_data);
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

    #[test]
    fn cryptojs_hashes() {
        let out = ScriptRuntime::new()
            .run_pre_request(
                r#"
                pm.environment.set("md5", CryptoJS.MD5("hello").toString());
                pm.environment.set("sha", CryptoJS.SHA256("hello").toString());
                pm.environment.set("hmac", CryptoJS.HmacSHA256("The quick brown fox jumps over the lazy dog", "key").toString());
                pm.environment.set("b64", CryptoJS.enc.Base64.stringify("hello world"));
                pm.environment.set("unb64", CryptoJS.enc.Base64.parse("aGVsbG8gd29ybGQ=").toString());
                pm.environment.set("md5e", CryptoJS.MD5("").toString());
                "#,
                &ScriptScope::default(),
            )
            .unwrap();
        let e = &out.scope.environment;
        assert_eq!(e["md5"], "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(e["sha"], "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert_eq!(e["hmac"], "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
        assert_eq!(e["b64"], "aGVsbG8gd29ybGQ=");
        assert_eq!(e["unb64"], "hello world");
        assert_eq!(e["md5e"], "d41d8cd98f00b204e9800998ecf8427e");
    }

    /// Spawns a one-shot-per-connection HTTP server; returns its base URL and
    /// a channel yielding each raw request received.
    fn mock_server(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if n == 0 {
                        break;
                    }
                    if let Some(idx) = text.find("\r\n\r\n") {
                        let cl = text[..idx]
                            .lines()
                            .find_map(|l| {
                                let l = l.to_lowercase();
                                l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if buf.len() >= idx + 4 + cl {
                            break;
                        }
                    }
                }
                let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nX-Test: yes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}"), rx)
    }

    #[test]
    fn send_request_fetches_token_into_environment() {
        let (base, rx) = mock_server("200 OK", r#"{"token":"tok-123"}"#);
        let script = format!(
            r#"
            pm.sendRequest({{
                url: "{base}/auth",
                method: "POST",
                headers: {{ "Content-Type": "application/json", "X-Client": "rustman" }},
                body: JSON.stringify({{ user: "ada" }})
            }}, function (err, res) {{
                if (err) throw err;
                pm.environment.set("token", res.json().token);
                pm.environment.set("code", res.code);
                pm.environment.set("status", res.status);
                pm.environment.set("raw", res.text());
                pm.environment.set("xtest", res.headers["x-test"]);
            }});
            "#
        );
        let out = ScriptRuntime::new()
            .run_pre_request(&script, &ScriptScope::default())
            .unwrap();
        let e = &out.scope.environment;
        assert_eq!(e["token"], "tok-123");
        assert_eq!(e["code"], "200");
        assert_eq!(e["status"], "OK");
        assert_eq!(e["raw"], r#"{"token":"tok-123"}"#);
        assert_eq!(e["xtest"], "yes");
        let raw = rx.recv().unwrap().to_lowercase();
        assert!(raw.starts_with("post /auth"), "{raw}");
        assert!(raw.contains("x-client: rustman"));
        assert!(raw.ends_with(r#"{"user":"ada"}"#));
    }

    #[test]
    fn send_request_accepts_url_string_and_works_in_async_context() {
        let (base, _rx) = mock_server("201 Created", r#"{"ok":true}"#);
        let script = format!(
            r#"pm.sendRequest("{base}/x", function (err, res) {{
                pm.environment.set("code", res.code);
                pm.environment.set("ok", String(res.json().ok));
            }});"#
        );
        // Must also work when invoked from inside a tokio runtime.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let out = rt.block_on(async {
            ScriptRuntime::new()
                .run_pre_request(&script, &ScriptScope::default())
                .unwrap()
        });
        assert_eq!(out.scope.environment["code"], "201");
        assert_eq!(out.scope.environment["ok"], "true");
    }

    #[test]
    fn send_request_reports_connection_errors_to_callback() {
        // Port 1 on loopback should refuse connections.
        let out = ScriptRuntime::new()
            .run_pre_request(
                r#"pm.sendRequest("http://127.0.0.1:1/", function (err, res) {
                    pm.environment.set("failed", String(!!err));
                    pm.environment.set("res", String(res));
                });"#,
                &ScriptScope::default(),
            )
            .unwrap();
        assert_eq!(out.scope.environment["failed"], "true");
        assert_eq!(out.scope.environment["res"], "null");
    }
}
