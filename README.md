# rustman

A Postman-compatible API client written in Rust. It ships as a headless CLI runner, a reusable core library, and a native desktop app with a React frontend.

## Features

- HTTP requests with headers, query params, raw, form, multipart and binary bodies, plus a cookie jar
- GraphQL client with schema introspection
- WebSocket and Server-Sent Events clients with assertions
- Collections, environments and variables with Postman-style scoping
- Pre-request and test scripts (embedded JavaScript via Boa) including `postman.setNextRequest` flow control with loop bounds
- Declarative assertions for status, timing, headers and body
- Importers for Postman v2.1 and OpenAPI, and exporters for Postman and native formats
- Data-driven runs from CSV or JSON iteration files
- Local mock server generated from a collection
- API documentation generator (HTML and Markdown)
- Code snippet generation for cURL, Python, JavaScript, Go and Rust
- SQLite-backed storage
- Hashing and HMAC helpers available to scripts

## Repository layout

```
crates/
  rustman-core/      Core library: models, engine, scripting, importers, storage, mock, docs, codegen
  rustman-cli/       `rustman` command line tool (Newman-style runner and utilities)
  rustman-desktop/   Local backend server and native window (Tao + Wry)
frontend/            React + TypeScript + Vite UI used by the desktop app
```

## Requirements

- Rust (stable, edition 2021)
- Node.js and pnpm, for the frontend
- On Linux, the GTK and WebKitGTK development packages needed by Wry (for example `libwebkit2gtk-4.1-dev` on Debian and Ubuntu)

## Build

```sh
# Rust workspace
cargo build --release

# Frontend
cd frontend
pnpm install
pnpm build
```

## CLI usage

```sh
# Run a collection (Postman v2.1, OpenAPI or native format)
rustman run collection.json -e environment.json

# Data-driven run with a CSV or JSON file
rustman run collection.json -d data.csv -n 5

# Choose a reporter: cli (default), json, htmlextra
rustman run collection.json --reporter htmlextra --reporter-html-path report.html

# Stop at the first failure, add a delay between requests
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

Run `rustman --help` or `rustman <command> --help` for the full option list.

## Desktop app

The desktop binary starts a local backend (Axum) that serves the built frontend and opens it in a native window. If no display is available, it falls back to headless mode.

```sh
# Build the frontend first
(cd frontend && pnpm install && pnpm build)

# Launch the native window
cargo run -p rustman-desktop

# Backend only, then open the printed URL in a browser
cargo run -p rustman-desktop -- --headless
```

The default port is `51730`.

Environment variables:

| Variable | Purpose |
| --- | --- |
| `RUSTMAN_DB` | Path to the SQLite database file |
| `RUSTMAN_DIST` | Path to the built frontend (default `./frontend/dist`) |
| `RUSTMAN_PORT` | Port for the local backend |

For frontend development, run `pnpm dev` inside `frontend/`.

## Releases

Each [GitHub release](../../releases) includes:

| Asset | Platform |
| --- | --- |
| `rustman-vX.Y.Z-aarch64-apple-darwin.tar.gz` | CLI, macOS Apple Silicon |
| `rustman-vX.Y.Z-x86_64-apple-darwin.tar.gz` | CLI, macOS Intel |
| `rustman-desktop-vX.Y.Z-aarch64-apple-darwin.zip` | Desktop app (`Rustman.app`), macOS Apple Silicon |
| `rustman-desktop-vX.Y.Z-x86_64-apple-darwin.zip` | Desktop app (`Rustman.app`), macOS Intel |
| `rustman-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` | CLI, Linux |
| `rustman-vX.Y.Z-x86_64-pc-windows-msvc.zip` | CLI, Windows |
| `SHA256SUMS.txt` | Checksums for all assets |

### Installing on macOS

The easiest way is the install script. It uses your existing SSH key, so you only need `git`. It clones the `dist` branch (release binaries only) into a temporary folder, installs, and deletes the folder, so no copy of the repository stays on your Mac:

```sh
d=$(mktemp -d) && git clone -q --depth 1 -b dist git@github.com:frammawiliansyah/rustman.git "$d" \
  && bash "$d/install-macos.sh"; rm -rf "$d"
```

To update, run the same command again. The `dist` branch always holds the latest release; older versions are available from the GitHub release page.

It picks the right build for your Mac (Apple Silicon or Intel) and installs:

- `Rustman.app` in `/Applications`, so it appears in Launchpad and Spotlight
- `rustman` (CLI) and `rustman-desktop` in `~/.local/bin`, so both work from zsh

```sh
rustman run collection.json
rustman-desktop            # opens the desktop window
rustman-desktop --headless # backend only
```

To install manually, download the zip or tar.gz from the release page and remove the quarantine flag with `xattr -dr com.apple.quarantine Rustman.app`. Builds are not notarized.

Releases follow semantic versioning and are published by pushing a `vX.Y.Z` tag. See [CHANGELOG.md](CHANGELOG.md) for the history.

## Testing

```sh
cargo test --workspace
```

## Status

The project is under active development. The CLI and core APIs may change between releases.
