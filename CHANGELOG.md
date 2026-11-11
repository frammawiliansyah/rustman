# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Flat design: no gradients anywhere. The UI uses solid colors for buttons, the logo tile, the active tab indicator, the progress bar, the agent badge and the AI Agents page, and the loading skeleton pulses instead of shimmering
- New app icon in the dark theme of the app: an indigo hexagon (a metal nut, for Rust and strength) holding curly braces with the letter R in between, on a flat dark tile, using only the app's own colors, with no gradients

## [0.4.0] - 2026-11-11

### Added

- **AI Agents** tab in the desktop app: what the MCP server is, ready-to-copy setup for Claude Code, Claude Desktop, Cursor and other clients (with the right command path for the machine), example prompts, the list of tools, the access token (hidden by default, with show and copy) and the last agent that connected
- `/api/agent/info` endpoint that returns these details

## [0.3.0] - 2026-11-11

### Added

- App icon for the desktop app: window and taskbar icon, `Rustman.app` icon on macOS, Start Menu shortcut icon on Windows and application menu icon on Linux
- `rustman update` (also `rustman --update`): checks the latest release and installs it, updating the CLI and the desktop app together. Options: `--check`, `--to <tag>` and `--force`

## [0.2.0] - 2026-11-11

### Added

- `rustman mcp`: a Model Context Protocol server so AI agents can drive the desktop app. Tools: `send_request`, `open_request`, `run_collection`, `list_collections`, `get_collection`, `save_request`, `list_environments`, `set_environment_variable`, `list_history`, `get_history_entry`, `import_collection` and `generate_snippet`, plus resources for collections, environments and history
- The desktop app follows agent activity live: the request form fills in, the response and test results appear, and collections, environments and history refresh
- History entries record who sent them. Agent requests show a robot icon, and the full request config is stored
- Live event stream at `/api/events` and a `source` field on `/api/execute`
- `rustman mcp` starts the desktop app automatically when it is not running

### Security

- Every `/api` call now requires an access token, stored in `~/.rustman/agent-token` with owner-only permissions (override with `RUSTMAN_TOKEN`)
- The backend refuses requests with a non-loopback `Host` header when it listens on loopback only, which blocks DNS rebinding

### Changed

- The history API accepts `?limit=` (default 50, maximum 500)
- The frontend dev server proxy adds the access token automatically

## [0.1.1] - 2026-11-11

### Added

- MIT license, contributing guide, security policy and issue templates
- One-line installers for macOS, Linux (`install.sh`) and Windows (`install.ps1`) with checksum verification, an uninstall option and a CLI-only option
- Desktop app builds for Linux and Windows, in addition to macOS
- Install instructions on every release page

### Changed

- Release assets use stable names without the version, so `releases/latest/download/<asset>` links always work
- The desktop app now finds its frontend next to the executable or inside the macOS app bundle
- The Windows desktop app no longer opens a console window

### Fixed

- The desktop app on macOS and Windows no longer starts in headless mode because of a Linux-only display check

## [0.1.0] - 2026-11-11

Initial release.

### Added

- Core library with HTTP executor, timing metrics, variable interpolation and dynamic variables
- Postman v2.1 and OpenAPI 3.0/3.1 importers, with Postman and native exporters
- Test assertion engine and status evaluator
- SQLite persistence and request history
- JavaScript scripting engine with the `pm.*` API, crypto utilities and `pm.sendRequest`
- `postman.setNextRequest` flow control with loop bounds
- RFC 6265 cookie jar with session persistence
- WebSocket and Server-Sent Events clients with assertions
- GraphQL client with schema introspection
- Data-driven runs from CSV and JSON iteration files
- Local mock server generated from a collection
- API documentation generator in HTML and Markdown
- Code snippet generator for cURL, Python, JavaScript, Go and Rust
- `rustman` CLI with `run`, `import`, `mock`, `doc` and `request` commands
- CLI reporters: `cli`, `json` and `htmlextra`
- Desktop app with a local Axum backend, a React frontend and a native Tao and Wry window

[Unreleased]: https://github.com/frammawiliansyah/rustman/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/frammawiliansyah/rustman/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/frammawiliansyah/rustman/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/frammawiliansyah/rustman/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/frammawiliansyah/rustman/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/frammawiliansyah/rustman/releases/tag/v0.1.0
