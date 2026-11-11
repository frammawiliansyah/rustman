#!/bin/sh
# Rustman installer for Linux and macOS.
#
#   curl -fsSL https://github.com/frammawiliansyah/rustman/releases/latest/download/install.sh | sh
#
# Installs:
#   rustman          CLI runner            -> ~/.local/bin
#   rustman-desktop  desktop app launcher  -> ~/.local/bin
#   macOS: Rustman.app in /Applications (or ~/Applications when not writable)
#   Linux: an application menu entry
#
# Environment variables:
#   RUSTMAN_VERSION=vX.Y.Z   install a specific version (default: latest)
#   RUSTMAN_NO_DESKTOP=1     install the CLI only
#   RUSTMAN_UNINSTALL=1      remove everything this script installed
#   BIN_DIR=/path            where to put the commands (default: ~/.local/bin)
set -eu

REPO="frammawiliansyah/rustman"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
SHARE_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
VERSION="${RUSTMAN_VERSION:-latest}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS/$ARCH" in
  Darwin/arm64) TARGET="aarch64-apple-darwin" ;;
  Darwin/x86_64) TARGET="x86_64-apple-darwin" ;;
  Linux/x86_64|Linux/amd64) TARGET="x86_64-unknown-linux-gnu" ;;
  *) die "unsupported platform: $OS/$ARCH (see https://github.com/$REPO/releases for other downloads)" ;;
esac

if [ "$OS" = "Darwin" ]; then
  APP_DIR="/Applications"
  [ -w "$APP_DIR" ] || { APP_DIR="$HOME/Applications"; mkdir -p "$APP_DIR"; }
fi

uninstall() {
  rm -f "$BIN_DIR/rustman" "$BIN_DIR/rustman-desktop"
  if [ "$OS" = "Darwin" ]; then
    rm -rf /Applications/Rustman.app "$HOME/Applications/Rustman.app" 2>/dev/null || true
  else
    rm -rf "$SHARE_DIR/rustman" "$SHARE_DIR/applications/rustman.desktop"
  fi
  say "Rustman removed. Your data in ~/.rustman was kept."
  exit 0
}
[ "${RUSTMAN_UNINSTALL:-}" = "1" ] && uninstall

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -O "$2" "$1"; }
else
  die "curl or wget is required"
fi

if [ -n "${RUSTMAN_BASE_URL:-}" ]; then
  BASE="$RUSTMAN_BASE_URL"
elif [ "$VERSION" = "latest" ]; then
  BASE="https://github.com/$REPO/releases/latest/download"
else
  BASE="https://github.com/$REPO/releases/download/$VERSION"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

download() { # asset name
  say "Downloading $1..."
  fetch "$BASE/$1" "$TMP/$1" || die "could not download $1 from $BASE"
  want="$(grep " $1\$" "$TMP/SHA256SUMS.txt" | cut -d' ' -f1)"
  [ -n "$want" ] || die "no checksum listed for $1"
  [ "$(sha256 "$TMP/$1")" = "$want" ] || die "checksum mismatch for $1"
}

fetch "$BASE/SHA256SUMS.txt" "$TMP/SHA256SUMS.txt" || die "could not download release checksums from $BASE"

mkdir -p "$BIN_DIR"

# CLI
CLI="rustman-$TARGET.tar.gz"
download "$CLI"
tar xzf "$TMP/$CLI" -C "$TMP"
install -m 755 "$TMP/rustman-$TARGET/rustman" "$BIN_DIR/rustman"

# Desktop app
if [ "${RUSTMAN_NO_DESKTOP:-}" != "1" ]; then
  if [ "$OS" = "Darwin" ]; then
    APP="rustman-desktop-$TARGET.zip"
    download "$APP"
    unzip -q "$TMP/$APP" -d "$TMP"
    rm -rf "$APP_DIR/Rustman.app"
    mv "$TMP/Rustman.app" "$APP_DIR/Rustman.app"
    xattr -dr com.apple.quarantine "$APP_DIR/Rustman.app" 2>/dev/null || true
    ln -sf "$APP_DIR/Rustman.app/Contents/MacOS/rustman-desktop" "$BIN_DIR/rustman-desktop"
  else
    APP="rustman-desktop-$TARGET.tar.gz"
    download "$APP"
    tar xzf "$TMP/$APP" -C "$TMP"
    rm -rf "$SHARE_DIR/rustman"
    mkdir -p "$SHARE_DIR/rustman" "$SHARE_DIR/applications"
    cp -R "$TMP/rustman-desktop-$TARGET/." "$SHARE_DIR/rustman/"
    ln -sf "$SHARE_DIR/rustman/rustman-desktop" "$BIN_DIR/rustman-desktop"
    cat > "$SHARE_DIR/applications/rustman.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Rustman
Comment=API client
Exec=$SHARE_DIR/rustman/rustman-desktop
Terminal=false
Categories=Development;Network;
DESKTOP
    if command -v ldconfig >/dev/null 2>&1 && ! ldconfig -p 2>/dev/null | grep -q 'libwebkit2gtk-4.1'; then
      say ""
      say "The desktop app needs WebKitGTK 4.1 and GTK 3. Install them with one of:"
      say "  Debian/Ubuntu: sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libxdo3"
      say "  Fedora/RHEL:   sudo dnf install webkit2gtk4.1 gtk3 xdotool"
    fi
  fi
fi

say ""
say "Installed Rustman ($VERSION) for $TARGET:"
say "  $BIN_DIR/rustman"
[ "${RUSTMAN_NO_DESKTOP:-}" = "1" ] || say "  $BIN_DIR/rustman-desktop"
[ "$OS" = "Darwin" ] && [ "${RUSTMAN_NO_DESKTOP:-}" != "1" ] && say "  $APP_DIR/Rustman.app"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say ""
     say "Add $BIN_DIR to your PATH, then restart your shell:"
     say "  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.profile" ;;
esac
say ""
say "Try: rustman --version"
