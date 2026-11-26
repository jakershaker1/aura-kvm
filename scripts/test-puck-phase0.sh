#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

AGENT_BIN="${1:-$ROOT_DIR/target/aarch64-unknown-linux-musl/release/aura-puck-agent}"

echo "=== Aura KVM Phase 0 Hardware Validation ==="
echo "1. Checking connected ADB devices..."
adb devices -l

DEVICE_COUNT=$(adb devices | awk 'NR>1 && $2=="device" {count++} END {print count+0}')
if [ "$DEVICE_COUNT" -eq 0 ]; then
    echo ""
    echo "No authorized ADB device detected."
    echo "On the XREAL Aura Compute Puck:"
    echo "  1. Open Settings -> About device -> tap Build number 7 times"
    echo "  2. Open Settings -> System -> Developer options -> enable USB debugging"
    echo "  3. Connect your USB-C cable (or DP Alt Mode cable) to the left-side Charging/DP-In port"
    echo "  4. Accept the 'Allow USB debugging' prompt inside the glasses"
    echo "  (Or connect via Wireless Debugging: adb pair <ip>:<port> && adb connect <ip>:<port>)"
    exit 1
fi

echo ""
echo "2. Checking if Wired Mirroring (DP-in) is active..."
adb shell dumpsys window windows 2>/dev/null | grep -i wiredmirror || echo "(wiredmirror window not currently focused)"

echo ""
echo "3. Checking /dev/uhid permissions..."
adb shell ls -l /dev/uhid
adb shell id

if [ -f "$AGENT_BIN" ]; then
    echo ""
    echo "4. Pushing aura-puck-agent and running --self-test..."
    adb push "$AGENT_BIN" /data/local/tmp/aura-hid-agent
    adb shell chmod 755 /data/local/tmp/aura-hid-agent
    adb shell /data/local/tmp/aura-hid-agent --self-test
    echo ""
    echo "Phase 0 PASSED! Run:"
    echo "  $ROOT_DIR/target/release/aura-host"
else
    echo ""
    echo "Agent binary not built yet at $AGENT_BIN."
    echo "Run ./scripts/build-all.sh first."
fi
