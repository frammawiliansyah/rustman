#!/usr/bin/env bash
# Install the latest Rustman release on macOS.
#
#   - Rustman.app      -> /Applications (shows up in Launchpad and Spotlight)
#   - rustman          -> ~/.local/bin  (CLI runner)
#   - rustman-desktop  -> ~/.local/bin  (launches the desktop app from zsh)
#
# Usage: scripts/install-macos.sh [vX.Y.Z]
# Requires the GitHub CLI (`gh`) logged in, because the repository is private.
set -euo pipefail

REPO="frammawiliansyah/rustman"
TAG="${1:-}"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
APP_DIR="${APP_DIR:-/Applications}"

[ "$(uname -s)" = "Darwin" ] || { echo "This installer is for macOS only." >&2; exit 1; }
command -v gh >/dev/null || { echo "Install the GitHub CLI first: brew install gh && gh auth login" >&2; exit 1; }

case "$(uname -m)" in
  arm64) TARGET="aarch64-apple-darwin" ;;
  x86_64) TARGET="x86_64-apple-darwin" ;;
  *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading ${TAG:-latest} for $TARGET..."
gh release download $TAG --repo "$REPO" --dir "$tmp" \
  --pattern "rustman-v*-$TARGET.tar.gz" \
  --pattern "rustman-desktop-v*-$TARGET.zip"

# Desktop app
unzip -q "$tmp"/rustman-desktop-*.zip -d "$tmp"
rm -rf "$APP_DIR/Rustman.app"
mv "$tmp/Rustman.app" "$APP_DIR/Rustman.app"
xattr -dr com.apple.quarantine "$APP_DIR/Rustman.app" 2>/dev/null || true

# CLI
tar xzf "$tmp"/rustman-v*.tar.gz -C "$tmp"
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
