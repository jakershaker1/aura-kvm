#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="Aura KVM.app"
APP_DIR="$ROOT_DIR/target/release/$APP_NAME"
UI_BIN="$ROOT_DIR/target/release/aura-ui"
AGENT_BIN="$ROOT_DIR/target/aarch64-unknown-linux-musl/release/aura-puck-agent"

if [ ! -f "$UI_BIN" ]; then
    echo "error: missing $UI_BIN (build with cargo build --release -p aura-ui)" >&2
    exit 1
fi

if [ ! -f "$AGENT_BIN" ]; then
    echo "error: missing $AGENT_BIN (build with cargo build --release -p aura-puck-agent --target aarch64-unknown-linux-musl)" >&2
    exit 1
fi

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$UI_BIN" "$APP_DIR/Contents/MacOS/aura-ui"
chmod 755 "$APP_DIR/Contents/MacOS/aura-ui"

cp "$AGENT_BIN" "$APP_DIR/Contents/Resources/aura-puck-agent"
chmod 755 "$APP_DIR/Contents/Resources/aura-puck-agent"

cat > "$APP_DIR/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Aura KVM</string>
    <key>CFBundleDisplayName</key>
    <string>Aura KVM</string>
    <key>CFBundleIdentifier</key>
    <string>com.aurakvm.app</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleExecutable</key>
    <string>aura-ui</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
</dict>
</plist>
PLIST

if command -v codesign >/dev/null 2>&1; then
    SIGN_ID="${AURA_CODESIGN_IDENTITY:-}"
    if [ -z "$SIGN_ID" ] && command -v security >/dev/null 2>&1; then
        SIGN_ID="$(security find-identity -v -p codesigning 2>/dev/null \
            | awk -F'"' '/Aura KVM Signing|Apple Development/ { print $2; exit }')"
    fi
    if [ -z "$SIGN_ID" ]; then
        SIGN_ID="-"
    fi
    codesign --force --deep --sign "$SIGN_ID" "$APP_DIR"
    if [ "$SIGN_ID" = "-" ]; then
        echo "Signed bundle with ad-hoc identity (note: macOS Privacy & Security entries may need removing with '-' and re-adding after rebuilds)"
    else
        echo "Signed bundle with certificate identity: $SIGN_ID"
    fi
fi

echo "Created macOS app bundle: $APP_DIR"
