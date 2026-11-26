#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

echo "[1/4] Building native aura-host & aura-ui (release)..."
cargo build --release -p aura-host -p aura-ui

echo "[2/4] Building static ARM64 aura-puck-agent (aarch64-unknown-linux-musl)..."
cargo build --release -p aura-puck-agent --target aarch64-unknown-linux-musl

if [ "$(uname -s)" = "Darwin" ]; then
    echo "[3/4] Packaging macOS app bundle..."
    "$ROOT_DIR/scripts/bundle-macos.sh"
else
    echo "[3/4] Skipping macOS .app bundle (non-macOS host)"
fi

echo "[4/4] Checking Linux and Windows cross-targets..."
cargo check -p aura-host --target x86_64-unknown-linux-musl
cargo check -p aura-host --target x86_64-pc-windows-msvc
cargo check -p aura-ui --target x86_64-pc-windows-msvc

echo ""
echo "Build complete:"
if [ "$(uname -s)" = "Darwin" ]; then
    echo "  macOS App   : $ROOT_DIR/target/release/Aura KVM.app"
fi
echo "  Desktop UI  : $ROOT_DIR/target/release/aura-ui"
echo "  Headless CLI: $ROOT_DIR/target/release/aura-host"
echo "  Puck binary : $ROOT_DIR/target/aarch64-unknown-linux-musl/release/aura-puck-agent"
file "$ROOT_DIR/target/aarch64-unknown-linux-musl/release/aura-puck-agent" || true
