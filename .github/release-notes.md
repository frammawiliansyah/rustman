## Install

### macOS and Linux

```sh
curl -fsSL https://github.com/frammawiliansyah/rustman/releases/download/{{TAG}}/install.sh | sh
```

This installs the `rustman` CLI and the `rustman-desktop` launcher into `~/.local/bin`.
On macOS it also installs `Rustman.app` into `/Applications`. On Linux it adds an application menu entry.

### Windows (PowerShell)

```powershell
irm https://github.com/frammawiliansyah/rustman/releases/download/{{TAG}}/install.ps1 | iex
```

This installs the CLI and the desktop app under `%LOCALAPPDATA%\Rustman`, adds them to your `PATH` and creates a Start Menu shortcut. No administrator rights needed.

### After installing

```sh
rustman --version
rustman run collection.json -e environment.json
rustman-desktop              # opens the desktop app (on Windows, also from the Start Menu)
rustman-desktop --headless   # backend only, then open the printed URL in a browser
```

Installer options (environment variables): `RUSTMAN_NO_DESKTOP=1` installs the CLI only, `RUSTMAN_VERSION=vX.Y.Z` picks a version, `RUSTMAN_UNINSTALL=1` removes Rustman.

### Update

```sh
rustman update
```

Or run the install command again.

### Use with AI agents

Connect an MCP client, for example `claude mcp add rustman -- rustman mcp`. The desktop app then shows every request the agent makes. See the [README](https://github.com/frammawiliansyah/rustman#use-with-ai-agents-mcp).

### Manual download

| Platform | CLI | Desktop app |
| --- | --- | --- |
| macOS Apple Silicon | `rustman-aarch64-apple-darwin.tar.gz` | `rustman-desktop-aarch64-apple-darwin.zip` |
| macOS Intel | `rustman-x86_64-apple-darwin.tar.gz` | `rustman-desktop-x86_64-apple-darwin.zip` |
| Linux x86_64 | `rustman-x86_64-unknown-linux-gnu.tar.gz` | `rustman-desktop-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `rustman-x86_64-pc-windows-msvc.zip` | `rustman-desktop-x86_64-pc-windows-msvc.zip` |

Verify downloads with `SHA256SUMS.txt`.

### Requirements

- **Linux:** glibc 2.35 or newer and OpenSSL 3 (Ubuntu 22.04+, Debian 12+, Fedora 36+, RHEL and CentOS Stream 9+). The desktop app also needs WebKitGTK 4.1 and GTK 3:
  - Debian and Ubuntu: `sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libxdo3`
  - Fedora, RHEL and CentOS: `sudo dnf install webkit2gtk4.1 gtk3 xdotool`
- **macOS:** 11 or newer. The app is not notarized. The installer clears the download quarantine flag; for a manual download run `xattr -dr com.apple.quarantine Rustman.app`.
- **Windows:** Windows 10 or newer with the WebView2 runtime (included in Windows 11 and current Windows 10).

## What's changed

