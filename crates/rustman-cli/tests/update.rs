//! End-to-end tests for `rustman update` against a fake release server.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};

/// Serves `GET /releases/latest` and `GET /download/<tag>/install.sh` for a few requests.
fn fake_releases(latest: &'static str, installer: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            let (status, body) = if path == "/releases/latest" {
                ("200 OK", format!("{{\"tag_name\":\"{latest}\"}}"))
            } else if path.starts_with("/download/") && path.ends_with("/install.sh") {
                ("200 OK", installer.to_string())
            } else {
                ("404 Not Found", "not found".to_string())
            };
            let _ = write!(
                s,
                "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://{addr}")
}

/// Runs a private copy of the binary so an update can never touch the real build output.
fn rustman(dir: &std::path::Path, base: &str, args: &[&str]) -> Output {
    let exe: PathBuf = dir.join("rustman");
    if !exe.exists() {
        std::fs::copy(env!("CARGO_BIN_EXE_rustman"), &exe).unwrap();
    }
    Command::new(exe)
        .args(args)
        .env("RUSTMAN_RELEASES_URL", format!("{base}/releases/latest"))
        .env("RUSTMAN_DOWNLOAD_URL", format!("{base}/download"))
        .env("RUSTMAN_UPDATE_SKIP_RUNNING_CHECK", "1")
        .output()
        .unwrap()
}

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rustman-update-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn check_reports_available_update() {
    let base = fake_releases("v99.0.0", "");
    let d = tmp();
    let o = rustman(&d, &base, &["update", "--check"]);
    assert!(o.status.success(), "{}", text(&o));
    let t = text(&o);
    assert!(t.contains("Latest version: v99.0.0"), "{t}");
    assert!(t.contains("An update is available"), "{t}");
}

#[test]
fn check_reports_up_to_date_and_update_does_nothing() {
    let base = fake_releases("v0.0.1", "echo SHOULD-NOT-RUN > ran.txt");
    let d = tmp();
    let o = rustman(&d, &base, &["update", "--check"]);
    assert!(text(&o).contains("You are up to date"), "{}", text(&o));
    let o = rustman(&d, &base, &["update"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("You are up to date"));
    assert!(!d.join("ran.txt").exists());
}

#[test]
fn rejects_invalid_tag() {
    let base = fake_releases("v99.0.0", "");
    let d = tmp();
    let o = rustman(&d, &base, &["update", "--to", "main"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("not a valid version"), "{}", text(&o));
}

#[test]
fn update_flag_is_an_alias() {
    let base = fake_releases("v99.0.0", "");
    let d = tmp();
    let o = rustman(&d, &base, &["--update", "--check"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("An update is available"));
}

#[test]
fn missing_release_is_reported() {
    let base = fake_releases("v99.0.0", "echo should-not-run\n");
    let d = tmp();
    let exe = d.join("rustman");
    std::fs::copy(env!("CARGO_BIN_EXE_rustman"), &exe).unwrap();
    // The fake server only serves installers under /download, so this path answers 404.
    let o = Command::new(exe)
        .args(["update", "--to", "v1.2.3"])
        .env("RUSTMAN_DOWNLOAD_URL", format!("{base}/nowhere"))
        .env("RUSTMAN_UPDATE_SKIP_RUNNING_CHECK", "1")
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("could not download the installer for v1.2.3"), "{}", text(&o));
}

#[cfg(unix)]
#[test]
fn update_runs_the_release_installer_for_this_binary() {
    // The fake installer records what it was asked to do instead of installing anything.
    let base = fake_releases(
        "v99.0.0",
        "printf '%s|%s|%s' \"$RUSTMAN_VERSION\" \"$BIN_DIR\" \"${RUSTMAN_NO_DESKTOP:-}\" > \"$BIN_DIR/installer-ran\"\n",
    );
    let d = tmp();
    let o = rustman(&d, &base, &["update"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("Installing v99.0.0"), "{}", text(&o));
    let ran = std::fs::read_to_string(d.canonicalize().unwrap().join("installer-ran")).expect("installer was not run");
    let parts: Vec<&str> = ran.split('|').collect();
    assert_eq!(parts[0], "v99.0.0");
    assert_eq!(parts[1], d.canonicalize().unwrap().to_str().unwrap(), "installs next to the running binary");
    assert_eq!(parts[2], "1", "no desktop app was found, so the CLI-only install is kept");
}

#[cfg(unix)]
#[test]
fn failing_installer_fails_the_update() {
    let base = fake_releases("v99.0.0", "exit 3\n");
    let d = tmp();
    let o = rustman(&d, &base, &["update"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("the installer failed"), "{}", text(&o));
}

#[cfg(unix)]
#[test]
fn explicit_version_and_force_reinstall() {
    let base = fake_releases("v0.0.1", "printf '%s' \"$RUSTMAN_VERSION\" > \"$BIN_DIR/v\"\n");
    let d = tmp();
    let o = rustman(&d, &base, &["update", "--to", "1.2.3"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(d.canonicalize().unwrap().join("v")).unwrap(), "v1.2.3");
    // Already on the latest, but --force reinstalls it.
    let o = rustman(&d, &base, &["update", "--force"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(d.canonicalize().unwrap().join("v")).unwrap(), "v0.0.1");
}
