use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

fn serve() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let code = if req.contains("/bad") { 500 } else { 200 };
            let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    format!("http://{addr}")
}

fn run(tag: &str, path: &str) -> (i32, String) {
    let base = serve();
    let dir = std::env::temp_dir().join(format!("rustman-smoke-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let coll = serde_json::json!({
        "info": {"name": "smoke", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "item": [{"name": "folder", "item": [{"name": "r", "request": {"method": "GET", "url": format!("{{{{base}}}}{path}")}}]}]
    });
    let env = serde_json::json!({"name": "e", "values": [{"key": "base", "value": base, "enabled": true}]});
    let (cp, ep) = (dir.join("c.json"), dir.join("env.json"));
    std::fs::write(&cp, coll.to_string()).unwrap();
    std::fs::write(&ep, env.to_string()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rustman"))
        .args(["run", cp.to_str().unwrap(), "-e", ep.to_str().unwrap(), "--expect-status", "200"])
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn run_success_exits_zero() {
    let (code, out) = run("ok", "/ok");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("[PASS]") && out.contains("total requests"));
}

#[test]
fn run_failure_exits_one() {
    let (code, out) = run("bad", "/bad");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[FAIL]"));
}
