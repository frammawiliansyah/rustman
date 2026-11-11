#!/usr/bin/env bash
# Install Rustman on macOS from the `dist` branch, over SSH, without keeping a clone.
#
#   - Rustman.app      -> /Applications (shows up in Launchpad and Spotlight)
#   - rustman          -> ~/.local/bin  (CLI runner)
#   - rustman-desktop  -> ~/.local/bin  (launches the desktop app from zsh)
#
# One-liner (needs an SSH key with access to the repository):
#
#   d=$(mktemp -d) && git clone -q --depth 1 -b dist git@github.com:frammawiliansyah/rustman.git "$d" \
#     && bash "$d/install-macos.sh"; rm -rf "$d"
#
# The script expects the release archives to sit next to it, which is how the
# `dist` branch is laid out. Re-run the same command to update.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
APP_DIR="${APP_DIR:-/Applications}"

[ "$(uname -s)" = "Darwin" ] || { echo "This installer is for macOS only." >&2; exit 1; }

case "$(uname -m)" in
  arm64) TARGET="aarch64-apple-darwin" ;;
  x86_64) TARGET="x86_64-apple-darwin" ;;
  *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

app_zip="$(ls "$HERE"/rustman-desktop-v*-"$TARGET".zip 2>/dev/null | head -n1 || true)"
cli_tgz="$(ls "$HERE"/rustman-v*-"$TARGET".tar.gz 2>/dev/null | head -n1 || true)"
[ -n "$app_zip" ] && [ -n "$cli_tgz" ] || { echo "No macOS archives for $TARGET next to the installer." >&2; exit 1; }

echo "Installing ${VERSION:-$(cat "$HERE/VERSION" 2>/dev/null || echo unknown)} for $TARGET..."

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Desktop app
unzip -q "$app_zip" -d "$tmp"
rm -rf "$APP_DIR/Rustman.app"
mv "$tmp/Rustman.app" "$APP_DIR/Rustman.app"
xattr -dr com.apple.quarantine "$APP_DIR/Rustman.app" 2>/dev/null || true

# CLI
tar xzf "$cli_tgz" -C "$tmp"
mkdir -p "$BIN_DIR"
install -m 755 "$tmp"/rustman-v*/rustman "$BIN_DIR/rustman"
xattr -d com.apple.quarantine "$BIN_DIR/rustman" 2>/dev/null || true
ln -sf "$APP_DIR/Rustman.app/Contents/MacOS/rustman-desktop" "$BIN_DIR/rustman-desktop"

echo "Installed:"
echo "  $APP_DIR/Rustman.app"
echo "  $BIN_DIR/rustman"
echo "  $BIN_DIR/rustman-desktop"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo
     echo "Add $BIN_DIR to your PATH:"
     echo "  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc && source ~/.zshrc" ;;
esac
