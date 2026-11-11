<div align="center">

# Rustman

**A fast, open-source API client and test runner written in Rust.**

Send requests, organize collections, run tests from the command line or CI, and work in a native desktop app. No account, no cloud.

[![CI](https://github.com/frammawiliansyah/rustman/actions/workflows/ci.yml/badge.svg)](https://github.com/frammawiliansyah/rustman/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/frammawiliansyah/rustman)](https://github.com/frammawiliansyah/rustman/releases/latest)
[![License: MIT](https://img.shields.io/github/license/frammawiliansyah/rustman)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux%20%7C%20Windows-blue)

[Install](#install) · [Quick start](#quick-start) · [Features](#features) · [CLI reference](#cli-reference) · [Contributing](#contributing)

</div>

---

## Why Rustman

- **Fast and light.** A native Rust core with an embedded JavaScript engine. Nothing runs in the background.
- **Local first.** Collections, environments, history and cookies live in a local SQLite database on your machine.
- **Works with what you have.** Import Postman v2.1 collections and OpenAPI specs, and export back to Postman.
- **Built for automation.** A Newman-style headless runner with CI-friendly exit codes and JSON or HTML reports.
- **Open source.** MIT licensed, no sign-in.

## Install

### macOS and Linux

```sh
curl -fsSL https://github.com/frammawiliansyah/rustman/releases/latest/download/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://github.com/frammawiliansyah/rustman/releases/latest/download/install.ps1 | iex
```

The installers verify checksums, need no administrator rights, and set up:

- `rustman`, the CLI runner
- `rustman-desktop`, which opens the desktop app from the terminal
- the desktop app itself: `Rustman.app` on macOS, an application menu entry on Linux, a Start Menu shortcut on Windows

| Platform | Supported |
| --- | --- |
| macOS | 11 or newer, Apple Silicon and Intel |
| Linux | x86_64 with glibc 2.35+ and OpenSSL 3: Ubuntu 22.04+, Debian 12+, Fedora 36+, RHEL and CentOS Stream 9+ |
| Windows | Windows 10 or newer, x86_64, with the WebView2 runtime (included in Windows 11 and current Windows 10) |

The Linux desktop app also needs WebKitGTK 4.1 and GTK 3:

```sh
sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libxdo3   # Debian, Ubuntu
sudo dnf install webkit2gtk4.1 gtk3 xdotool              # Fedora, RHEL, CentOS
```

<details>
<summary>Installer options, manual download and uninstall</summary>

Options are environment variables:

| Variable | Effect |
| --- | --- |
| `RUSTMAN_NO_DESKTOP=1` | Install the CLI only |
| `RUSTMAN_VERSION=vX.Y.Z` | Install a specific version instead of the latest |
| `RUSTMAN_UNINSTALL=1` | Remove everything the installer set up (your data in `~/.rustman` is kept) |

```sh
# CLI only
curl -fsSL https://github.com/frammawiliansyah/rustman/releases/latest/download/install.sh | RUSTMAN_NO_DESKTOP=1 sh

# Uninstall
curl -fsSL https://github.com/frammawiliansyah/rustman/releases/latest/download/install.sh | RUSTMAN_UNINSTALL=1 sh
```

```powershell
# Windows: set the variable first, then run the installer
$env:RUSTMAN_UNINSTALL = "1"
irm https://github.com/frammawiliansyah/rustman/releases/latest/download/install.ps1 | iex
```

You can also download archives directly from the [releases page](https://github.com/frammawiliansyah/rustman/releases). Every release lists the install commands and a `SHA256SUMS.txt` file. macOS builds are not notarized, so after a manual download run `xattr -dr com.apple.quarantine Rustman.app`.

</details>

Prefer to build it yourself? See [Build from source](#build-from-source).

## Quick start

Create a collection file `demo.json`:

```json
{
  "info": {
    "name": "Demo",
    "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
  },
  "item": [
    {
      "name": "Get example",
      "request": { "method": "GET", "url": "{{baseUrl}}/" }
    }
  ]
}
```

and an environment file `env.json`:

```json
{ "name": "Local", "values": [{ "key": "baseUrl", "value": "https://example.com", "enabled": true }] }
```

Run it:

```sh
rustman run demo.json -e env.json --expect-status 200
```

```text
Iteration 1 of 1

→ GET https://example.com/
  200 · 103 ms · 577 B

+---------------------------+--------+
| iterations                |      1 |
| total requests            |      1 |
| passed                    |      1 |
| failed                    |      0 |
...
```

Prefer a UI? Run `rustman-desktop`, or open Rustman from your applications menu.

## Features

**Requests**
- HTTP with headers, query params, raw, form, multipart and binary bodies
- Cookie jar with RFC 6265 handling and session persistence
- GraphQL client with schema introspection
- WebSocket and Server-Sent Events clients with assertions

**Organize**
- Collections, folders, environments and variables with Postman-style scoping and dynamic variables
- Import Postman v2.1 and OpenAPI 3.0/3.1; export to Postman or the native format
- Request history stored in SQLite

**Test and automate**
- Pre-request and test scripts in embedded JavaScript (Boa) with the familiar `pm.*` API, `pm.sendRequest` and `postman.setNextRequest` flow control with loop bounds
- Declarative assertions for status, timing, headers and body
- Data-driven runs from CSV or JSON files
- Headless runner with `cli`, `json` and `htmlextra` reporters

**Share**
- Local mock server generated from a collection
- API documentation generator in HTML and Markdown
- Code snippets for cURL, Python, JavaScript, Go and Rust

## CLI reference

```sh
# Run a collection (Postman v2.1, OpenAPI or native format)
rustman run collection.json -e environment.json

# Data-driven run with a CSV or JSON file, 5 iterations
rustman run collection.json -d data.csv -n 5

# Choose a reporter: cli (default), json, htmlextra
rustman run collection.json --reporter htmlextra --reporter-html-path report.html

# Stop at the first failure, wait 200 ms between requests
rustman run collection.json --bail --delay 200

# Global assertions
rustman run collection.json --expect-status 200 --max-time 500

# Import a file into SQLite and/or export it
rustman import openapi.yaml --db rustman.db
rustman import collection.json --export postman -o out.json

# Serve a collection as a mock server
rustman mock collection.json --port 8080

# Generate documentation
rustman doc collection.json --format markdown -o API.md

# One-off request
rustman request GET https://example.com -H "Accept: application/json"
```

Run `rustman --help` or `rustman <command> --help` for every option.

### Use in CI

Install the CLI and run your collection as a pipeline step. For example, with GitHub Actions:

```yaml
- name: Install rustman
  run: curl -fsSL https://github.com/frammawiliansyah/rustman/releases/latest/download/install.sh | RUSTMAN_NO_DESKTOP=1 sh
- name: Run API tests
  run: ~/.local/bin/rustman run collection.json -e staging.json --reporter htmlextra
```

## Desktop app

The desktop app starts a local backend (Axum) that serves the UI and opens it in a native window (Tao and Wry, using WebKit on macOS and Linux and WebView2 on Windows). With `--headless`, or when no display is available, it only starts the backend, and you can open the printed URL in a browser.

```sh
rustman-desktop              # native window
rustman-desktop --headless   # backend only
```

The default port is `51730`.

| Variable | Purpose |
| --- | --- |
| `RUSTMAN_DB` | Path to the SQLite database file (default `~/.rustman/rustman.db`) |
| `RUSTMAN_DIST` | Path to the built frontend (default: next to the app, or `./frontend/dist`) |
| `RUSTMAN_PORT` | Port for the local backend |

## Architecture

```
crates/
  rustman-core/      Core library: models, HTTP engine, scripting, importers, storage, mock, docs, codegen
  rustman-cli/       The `rustman` command line tool
  rustman-desktop/   Local backend server and native window
frontend/            React + TypeScript + Vite UI used by the desktop app
```

`rustman-core` is a reusable library; the CLI and the desktop backend are thin layers on top of it.

## Build from source

Requirements:

- Rust (stable)
- Node.js and pnpm, for the frontend
- On Linux, the GTK and WebKitGTK development packages, for example `libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev` on Debian and Ubuntu

```sh
git clone https://github.com/frammawiliansyah/rustman.git
cd rustman

(cd frontend && pnpm install && pnpm build)
cargo build --release

./target/release/rustman --help
./target/release/rustman-desktop
```

Run the tests with `cargo test --workspace`. For frontend development, run `pnpm dev` inside `frontend/`.

## Project status

Rustman is under active development and is pre-1.0, so the CLI and core APIs may change between releases. Releases follow [semantic versioning](https://semver.org/) and are listed in the [changelog](CHANGELOG.md). Bug reports and feedback are welcome through [issues](https://github.com/frammawiliansyah/rustman/issues).

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) to get started. To report a security problem privately, see [SECURITY.md](SECURITY.md).

## License

Released under the [MIT License](LICENSE).

Rustman is an independent project and is not affiliated with or endorsed by Postman, Inc. Postman is a trademark of its respective owner.
