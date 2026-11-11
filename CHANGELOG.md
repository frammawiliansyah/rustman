# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/frammawiliansyah/rustman/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/frammawiliansyah/rustman/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/frammawiliansyah/rustman/releases/tag/v0.1.0
