#!/usr/bin/env bash
# Run inside Boat sandbox kakangkung-ladybird / bx_cnfw3mch
# Dashboard: https://boat.dev/dashboard
set -euo pipefail
echo "=== Kakangkung Ladybird setup on Boat (no stubs) ==="
echo "Sandbox: kakangkung-ladybird (bx_cnfw3mch)"

export DEBIAN_FRONTEND=noninteractive
sudo apt-get update -y --allow-releaseinfo-change
sudo apt-get install -y --no-install-recommends \
  build-essential cmake ninja-build ccache nasm pkg-config curl git ca-certificates \
  python3 python-is-python3 clang clang++ lld autoconf automake libtool \
  zip unzip tar \
  libgl1-mesa-dev libpulse-dev libssl-dev libfontconfig1-dev libx11-dev libx11-xcb-dev \
  qt6-base-dev qt6-tools-dev qt6-multimedia-dev \
  || sudo apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build nasm pkg-config curl git ca-certificates libx11-dev

if ! command -v rustc >/dev/null; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

ROOT="${1:-$HOME/kakangkung}"
mkdir -p "$ROOT"
cd "$ROOT"

echo ""
echo "Real LibWeb (required — no stub paint):"
echo "  git clone --depth 1 https://github.com/LadybirdBrowser/ladybird.git ~/ladybird"
echo "  cd ~/ladybird && ./Meta/ladybird.py build"
echo "  cd $ROOT && LADYBIRD_SOURCE_DIR=~/ladybird \\"
echo "    LADYBIRD_BUILD_DIR=~/ladybird/Build/release \\"
echo "    ./engines/ladybird/build.sh --ladybird"
echo ""
echo "See docs/LADYBIRD.md and store docs/chrome-parity-roadmap.md"
