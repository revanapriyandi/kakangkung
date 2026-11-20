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
  python3 python-is-python3 \
  autoconf autoconf-archive automake libtool \
  zip unzip tar \
  libgl1-mesa-dev libpulse-dev libssl-dev libfontconfig1-dev libx11-dev libx11-xcb-dev \
  libdrm-dev libdrm2 libwayland-dev libxcb-icccm4-dev libncurses-dev \
  glslang-tools fonts-liberation2 \
  || sudo apt-get install -y --no-install-recommends \
    build-essential cmake ninja-build nasm pkg-config curl git ca-certificates \
    autoconf autoconf-archive automake libtool libx11-dev libdrm-dev
# Ladybird/vcpkg needs clang ≥19 or gcc ≥14 (Ubuntu 24.04 image ships clang 18).
if ! command -v clang-19 >/dev/null 2>&1; then
  wget -qO- https://apt.llvm.org/llvm.sh | sudo bash -s -- 19 || true
  sudo apt-get install -y --no-install-recommends clang-19 clang++-19 lld-19 || true
fi
sudo apt-get install -y --no-install-recommends g++-14 gcc-14 || true
export CC="${CC:-clang-19}" CXX="${CXX:-clang++-19}"
command -v "$CC" >/dev/null || { export CC=gcc-14 CXX=g++-14; }
echo "Compiler: CC=$CC CXX=$CXX"

# Ladybird requires Qt ≥6.10; Ubuntu 24.04 apt Qt is older — use aqt (pipx/venv; PEP 668).
export PATH="${HOME}/.local/bin:${PATH}"
if ! python3 -c 'import pathlib,sys; p=pathlib.Path.home()/"Qt"; sys.exit(0 if any(p.glob("6.1*/gcc_64/lib/cmake/Qt6/Qt6Config.cmake")) else 1)' 2>/dev/null; then
  sudo apt-get install -y --no-install-recommends pipx python3-venv || true
  if ! command -v aqt >/dev/null 2>&1; then
    pipx install aqtinstall || {
      python3 -m venv "${HOME}/aqt-venv"
      "${HOME}/aqt-venv/bin/pip" install -q aqtinstall
    }
  fi
  AQT="$(command -v aqt || echo "${HOME}/aqt-venv/bin/aqt")"
  "$AQT" install-qt linux desktop 6.10.0 linux_gcc_64 -O "${HOME}/Qt" -m qtpositioning qtwayland \
    || "$AQT" install-qt linux desktop 6.10.0 gcc_64 -O "${HOME}/Qt" -m qtpositioning || true
fi
if QT_CFG=$(echo "${HOME}"/Qt/6.1*/gcc_64/lib/cmake/Qt6/Qt6Config.cmake); then
  QT_PREFIX=$(dirname "$(dirname "$(dirname "$(dirname "$QT_CFG")")")")
  export CMAKE_PREFIX_PATH="${QT_PREFIX}${CMAKE_PREFIX_PATH:+:$CMAKE_PREFIX_PATH}"
  export Qt6_DIR="$(dirname "$QT_CFG")"
  echo "Qt6: $Qt6_DIR"
fi

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
