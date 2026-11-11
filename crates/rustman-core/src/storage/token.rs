//! Local access token shared by the desktop backend and `rustman mcp`.
//!
//! The backend requires the token on every `/api` call. It lives in a file in the
//! user's data directory so that only processes running as the same user can read it.

use std::io;
use std::path::{Path, PathBuf};

use uuid::Uuid;

/// `~/.rustman` (`%USERPROFILE%\.rustman` on Windows).
pub fn data_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    home.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".rustman")
}

/// Path of the token file inside `dir`.
pub fn token_path(dir: &Path) -> PathBuf {
    dir.join("agent-token")
}

fn generate() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Returns the token from `RUSTMAN_TOKEN`, or the token file in `dir`, creating the file
/// (readable only by the current user on Unix) when it does not exist yet.
pub fn load_or_create_token(dir: &Path) -> io::Result<String> {
    if let Ok(t) = std::env::var("RUSTMAN_TOKEN") {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let path = token_path(dir);
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    std::fs::create_dir_all(dir)?;
    let token = generate();
    write_private(&path, &token)?;
    Ok(token)
}

/// Reads the token without creating it. Returns `None` when there is none yet.
pub fn read_token(dir: &Path) -> Option<String> {
    if let Ok(t) = std::env::var("RUSTMAN_TOKEN") {
        if !t.trim().is_empty() {
            return Some(t.trim().to_string());
        }
    }
    std::fs::read_to_string(token_path(dir)).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

#[cfg(unix)]
fn write_private(path: &Path, content: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(content.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, content: &str) -> io::Result<()> {
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_and_reuses() {
        let dir = std::env::temp_dir().join(format!("rustman-token-{}", Uuid::new_v4()));
        assert_eq!(read_token(&dir), None);
        let a = load_or_create_token(&dir).unwrap();
        let b = load_or_create_token(&dir).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_eq!(read_token(&dir), Some(a));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(token_path(&dir)).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
