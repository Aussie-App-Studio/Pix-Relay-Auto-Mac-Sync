#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> Installing Node dependencies..."
npm ci || npm install

echo "==> Ensuring Rust macOS Universal targets (Apple Silicon + Intel)..."
rustup target add aarch64-apple-darwin x86_64-apple-darwin

echo "==> Building React frontend..."
npm run build

echo "==> Running Rust backend test suite..."
(cd src-tauri && cargo test)

echo "==> Building macOS Universal .app & .dmg..."
npx tauri build --target universal-apple-darwin

BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle"
DMG_PATH="$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name '*.dmg' | head -n 1 || true)"
APP_PATH="$BUNDLE_DIR/macos/Pix Relay Auto Mac Sync.app"

if [[ -n "${APPLE_SIGNING_IDENTITY:-}" && -d "$APP_PATH" ]]; then
  echo "==> Signing app bundle with identity: $APPLE_SIGNING_IDENTITY"
  codesign --deep --force --options runtime --sign "$APPLE_SIGNING_IDENTITY" "$APP_PATH"
fi

if [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" && -n "${DMG_PATH:-}" ]]; then
  echo "==> Submitting DMG for Apple Notarization..."
  xcrun notarytool submit "$DMG_PATH" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait
  xcrun stapler staple "$DMG_PATH"
  echo "==> Notarization complete and stapled to $DMG_PATH"
fi

echo "==> Build output:"
ls -lh "$BUNDLE_DIR/dmg/" || true
ls -ld "$APP_PATH" || true
