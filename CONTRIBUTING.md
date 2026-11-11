# Contributing

Thanks for your interest in rustman. Bug reports, ideas and pull requests are welcome.

## Getting started

```sh
git clone https://github.com/frammawiliansyah/rustman.git
cd rustman
(cd frontend && pnpm install && pnpm build)
cargo test --workspace
```

On Linux, install the GTK and WebKitGTK development packages first (for example `libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev` on Debian and Ubuntu).

## Pull requests

- Open an issue first for larger changes so the approach can be discussed.
- Keep changes focused and add or update tests where it makes sense.
- Make sure `cargo test --workspace` and `pnpm build` (in `frontend/`) pass.
- Write commit messages and PR descriptions in English, using the [Conventional Commits](https://www.conventionalcommits.org/) style, for example `fix(runner): handle empty iteration data`.

## License

By contributing you agree that your contributions are licensed under the [MIT License](LICENSE).
