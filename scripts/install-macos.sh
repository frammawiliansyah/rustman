#!/usr/bin/env bash
# Install the latest Rustman release on macOS without cloning the repository.
#
#   - Rustman.app      -> /Applications (shows up in Launchpad and Spotlight)
#   - rustman          -> ~/.local/bin  (CLI runner)
#   - rustman-desktop  -> ~/.local/bin  (launches the desktop app from zsh)
#
# Usage:
#   GITHUB_TOKEN=<token> bash install-macos.sh [vX.Y.Z]
#
# The repository is private, so GITHUB_TOKEN must be a token that can read it
# (fine-grained token with "Contents: read" on this repository). If the
# repository is public, no token is needed. Only curl, tar, unzip and
# osascript are used, all of which ship with macOS.
set -euo pipefail

REPO="frammawiliansyah/rustman"
TAG="${1:-}"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
APP_DIR="${APP_DIR:-/Applications}"
API="https://api.github.com/repos/$REPO"

[ "$(uname -s)" = "Darwin" ] || { echo "This installer is for macOS only." >&2; exit 1; }

case "$(uname -m)" in
  arm64) TARGET="aarch64-apple-darwin" ;;
  x86_64) TARGET="x86_64-apple-darwin" ;;
  *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

auth=()
[ -n "${GITHUB_TOKEN:-}" ] && auth=(-H "Authorization: Bearer $GITHUB_TOKEN")

api_get() { curl -fsSL "${auth[@]+"${auth[@]}"}" -H "Accept: application/vnd.github+json" "$1"; }

if [ -n "$TAG" ]; then
  release_url="$API/releases/tags/$TAG"
else
  release_url="$API/releases/latest"
fi

echo "Looking up release ${TAG:-latest}..."
release_json="$(api_get "$release_url")" || {
  echo "Could not read the release. Is GITHUB_TOKEN set and allowed to read $REPO?" >&2
  exit 1
}

# Print the API URL of the asset whose name matches the given regex.
asset_url() {
  RELEASE_JSON="$release_json" PATTERN="$1" osascript -l JavaScript -e '
    ObjC.import("stdlib");
    const rel = JSON.parse($.getenv("RELEASE_JSON"));
    const re = new RegExp($.getenv("PATTERN"));
    const a = rel.assets.find(x => re.test(x.name));
    a ? a.url : "";
  '
}

app_url="$(asset_url "^rustman-desktop-.*-$TARGET\\.zip\$")"
cli_url="$(asset_url "^rustman-v.*-$TARGET\\.tar\\.gz\$")"
[ -n "$app_url" ] && [ -n "$cli_url" ] || { echo "No macOS assets for $TARGET in this release." >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

download() {
  curl -fSL --progress-bar "${auth[@]+"${auth[@]}"}" -H "Accept: application/octet-stream" -o "$2" "$1"
}

echo "Downloading desktop app..."
download "$app_url" "$tmp/app.zip"
echo "Downloading CLI..."
download "$cli_url" "$tmp/cli.tar.gz"

# Desktop app
unzip -q "$tmp/app.zip" -d "$tmp"
rm -rf "$APP_DIR/Rustman.app"
mv "$tmp/Rustman.app" "$APP_DIR/Rustman.app"
xattr -dr com.apple.quarantine "$APP_DIR/Rustman.app" 2>/dev/null || true

# CLI
tar xzf "$tmp/cli.tar.gz" -C "$tmp"
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
