#!/usr/bin/env bash
# Run inside a Boat sandbox (https://boat.dev/dashboard) to prepare Ladybird + Kakangkung host builds.
set -euo pipefail
echo "Kakangkung Ladybird setup on Boat"
echo "Sandbox tip: boat desktop / dashboard Desktop to watch Qt Ladybird later."

sudo apt-get update -y
sudo apt-get install -y --no-install-recommends \
  build-essential cmake ninja-build nasm pkg-config curl git \
  libgl1-mesa-dev libpulse-dev 2>/dev/null || true

if ! command -v rustc >/dev/null; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

echo "Rust stub host builds with the Kakangkung repo:"
echo "  cargo build -p ladybird-host --release"
echo "Full LibWeb: clone Ladybird, ./Meta/ladybird.py build, then engines/ladybird/build.sh --ladybird"
echo "See docs/LADYBIRD.md"
