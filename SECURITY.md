# Security Policy

## Supported versions

Only the latest released version receives security fixes.

## Reporting a vulnerability

Please do not open a public issue for security problems. Use GitHub's private reporting instead:
[Report a vulnerability](https://github.com/frammawiliansyah/rustman/security/advisories/new).

Include the affected version, steps to reproduce and the impact. You can expect an initial response within a few days.

## Security model of the desktop app

The desktop app runs a backend on `127.0.0.1` that can send HTTP requests on your behalf. It is protected by an access token (`~/.rustman/agent-token`, owner-only permissions) required on every API call, and by a `Host` header check against DNS rebinding. AI agents connected through `rustman mcp` act with your permissions, so only connect agents you trust. If you make the backend listen on a non-loopback address with `--host`, anyone who can reach it and read the page can obtain the token, so keep it on a trusted network.
