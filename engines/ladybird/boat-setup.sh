#!/usr/bin/env bash
# Run inside Boat sandbox kakangkung-ladybird / bx_cnfw3mch
# Dashboard: https://boat.dev/dashboard
set -euo pipefail
echo "=== Kakangkung Ladybird setup on Boat ==="
echo "Sandbox: kakangkung-ladybird (bx_cnfw3mch)"
echo "Desktop: open from https://boat.dev/dashboard to watch Qt / LibWeb later."

sudo apt-get update -y
sudo apt-get install -y --no-install-recommends \
  build-essential cmake ninja-build nasm pkg-config curl git ca-certificates \
  libgl1-mesa-dev libpulse-dev qt6-base-dev qt6-tools-dev 2>/dev/null \
  || sudo apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build nasm pkg-config curl git ca-certificates

if ! command -v rustc >/dev/null; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

ROOT="${1:-$HOME/kakangkung}"
mkdir -p "$ROOT"
cd "$ROOT"

echo ""
echo "1) Rust stub host (always):"
echo "   cargo build -p ladybird-host --release"
echo ""
echo "2) C++ stub host (HWND paint, no LibWeb):"
echo "   ./engines/ladybird/build.sh --stub"
echo ""
echo "3) Full LibWeb (slow; needs Ladybird tree):"
echo "   git clone --depth 1 https://github.com/LadybirdBrowser/ladybird.git ~/ladybird"
echo "   cd ~/ladybird && ./Meta/ladybird.py build"
echo "   LADYBIRD_SOURCE_DIR=~/ladybird LADYBIRD_BUILD_DIR=~/ladybird/Build \\"
echo "     ./engines/ladybird/build.sh --ladybird"
echo ""
echo "See docs/LADYBIRD.md and engines/ladybird/README.md"
