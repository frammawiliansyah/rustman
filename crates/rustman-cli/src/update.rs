//! `rustman update`: check for a newer release and install it.
//!
//! The update runs the installer that ships with the release (`install.sh` on Linux and
//! macOS, `install.ps1` on Windows), so the CLI and the desktop app are updated together
//! by the same code that installed them, including checksum verification.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

const REPO: &str = "frammawiliansyah/rustman";

pub struct UpdateOptions {
    /// Only report whether a newer version exists.
    pub check: bool,
    /// Install this release tag (for example `v0.2.0`) instead of the latest one.
    pub to: Option<String>,
    /// Reinstall even when already on the latest version.
    pub force: bool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

/// Parses `v1.2.3` or `1.2.3`. Pre-release suffixes are not supported on purpose:
/// Rustman only publishes plain `vX.Y.Z` tags.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().trim_start_matches('v').split('.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    let c = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

fn normalize_tag(v: &str) -> Result<String> {
    parse_version(v).ok_or_else(|| anyhow!("\"{v}\" is not a valid version; use a form like v0.2.0"))?;
    Ok(format!("v{}", v.trim().trim_start_matches('v')))
}

/// URL of the "latest release" API. Overridable for tests and mirrors.
fn releases_url() -> String {
    std::env::var("RUSTMAN_RELEASES_URL")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases/latest"))
}

/// Base URL that holds one folder per release tag. Overridable for tests and mirrors.
fn download_base() -> String {
    std::env::var("RUSTMAN_DOWNLOAD_URL").unwrap_or_else(|_| format!("https://github.com/{REPO}/releases/download"))
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!("rustman-update/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(60))
        .build()?)
}

async fn latest_tag(client: &reqwest::Client) -> Result<String> {
    let res = client
        .get(releases_url())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("could not reach GitHub to look for updates")?;
    let status = res.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        bail!("GitHub rate limit reached; try again in a few minutes");
    }
    if !status.is_success() {
        bail!("could not read the latest release (HTTP {status})");
    }
    let rel: Release = res.json().await.context("unexpected response from GitHub")?;
    normalize_tag(&rel.tag_name)
}

/// Whether the desktop app is running. Replacing it while it runs would fail or leave it half updated.
fn desktop_running() -> bool {
    if std::env::var_os("RUSTMAN_UPDATE_SKIP_RUNNING_CHECK").is_some() {
        return false;
    }
    #[cfg(unix)]
    {
        std::process::Command::new("pgrep")
            .args(["-x", "rustman-desktop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq rustman-desktop.exe", "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("rustman-desktop.exe"))
            .unwrap_or(false)
    }
}

pub async fn run(opts: UpdateOptions) -> Result<i32> {
    let current = env!("CARGO_PKG_VERSION");
    let current_v = parse_version(current).ok_or_else(|| anyhow!("cannot parse own version {current}"))?;
    let client = client()?;

    let explicit = opts.to.is_some();
    let target = match &opts.to {
        Some(t) => normalize_tag(t)?,
        None => latest_tag(&client).await?,
    };
    let target_v = parse_version(&target).expect("normalized tag parses");

    println!("Current version: v{current}");
    println!("{} version: {target}", if explicit { "Requested" } else { "Latest" });

    let newer = target_v > current_v;
    if opts.check {
        if newer {
            println!("An update is available. Run `rustman update` to install it.");
        } else {
            println!("You are up to date.");
        }
        return Ok(0);
    }
    if !explicit && !newer && !opts.force {
        println!("You are up to date.");
        return Ok(0);
    }
    if explicit && target_v < current_v {
        println!("Note: {target} is older than the installed version.");
    }

    if desktop_running() {
        bail!("the Rustman desktop app is running. Quit it and run `rustman update` again.");
    }

    println!("Installing {target}...");
    install(&client, &target).await?;
    println!("Done. Run `rustman --version` to confirm.");
    Ok(0)
}

#[cfg(unix)]
async fn install(client: &reqwest::Client, tag: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let url = format!("{}/{tag}/install.sh", download_base());
    let res = client.get(&url).send().await.context("could not download the installer")?;
    if !res.status().is_success() {
        bail!("could not download the installer for {tag} (HTTP {}). Does that release exist?", res.status());
    }
    let script = res.text().await?;

    // Replace the binary that is running, wherever it lives.
    let exe = std::env::current_exe()?.canonicalize()?;
    let bin_dir = exe.parent().ok_or_else(|| anyhow!("cannot find the directory of the rustman binary"))?;

    let mut cmd = tokio::process::Command::new("sh");
    cmd.env("RUSTMAN_VERSION", tag).env("BIN_DIR", bin_dir).stdin(Stdio::piped());
    // Respect CLI-only installs: do not add the desktop app that was never there.
    if crate::mcp::find_desktop().is_none() {
        cmd.env("RUSTMAN_NO_DESKTOP", "1");
    }
    let mut child = cmd.spawn().context("could not run sh")?;
    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin for the installer"))?;
    stdin.write_all(script.as_bytes()).await?;
    drop(stdin);
    let status = child.wait().await?;
    if !status.success() {
        bail!("the installer failed ({status})");
    }
    Ok(())
}

#[cfg(windows)]
async fn install(_client: &reqwest::Client, tag: &str) -> Result<()> {
    let url = format!("{}/{tag}/install.ps1", download_base());
    let mut cmd = tokio::process::Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        &format!("$ErrorActionPreference='Stop'; irm '{url}' | iex"),
    ])
    .env("RUSTMAN_VERSION", tag);
    if crate::mcp::find_desktop().is_none() {
        cmd.env("RUSTMAN_NO_DESKTOP", "1");
    }
    let status = cmd.status().await.context("could not run PowerShell")?;
    if !status.success() {
        bail!("the installer failed ({status})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.10.3"), Some((1, 10, 3)));
        assert_eq!(parse_version(" v2.0.1 "), Some((2, 0, 1)));
        for bad in ["", "v1", "1.2", "1.2.3.4", "v1.2.x", "latest", "1.2.3-beta"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn compares_numerically_not_lexically() {
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
        assert!(parse_version("v1.0.0") > parse_version("v0.99.99"));
        assert!(parse_version("v0.2.0") == parse_version("0.2.0"));
    }

    #[test]
    fn normalizes_tags() {
        assert_eq!(normalize_tag("0.2.0").unwrap(), "v0.2.0");
        assert_eq!(normalize_tag("v0.2.0").unwrap(), "v0.2.0");
        assert!(normalize_tag("main").is_err());
    }
}
