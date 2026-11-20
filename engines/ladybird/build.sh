#!/usr/bin/env bash
# Build kakangkung-ladybird-host. Default: stub (no Ladybird). Pass nothing special, or:
#   ./build.sh --stub
#   LADYBIRD_SOURCE_DIR=/path/to/ladybird ./build.sh --ladybird
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="${ROOT}/out"
MODE=stub
CLONE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --stub) MODE=stub; shift ;;
    --ladybird) MODE=ladybird; shift ;;
    --clone) CLONE=1; shift ;;
    -h|--help)
      sed -n '1,12p' "$0"
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

mkdir -p "$OUT"

if [[ "$MODE" == "ladybird" ]]; then
  if [[ -z "${LADYBIRD_SOURCE_DIR:-}" ]]; then
    if [[ "$CLONE" -eq 1 ]]; then
      LADYBIRD_SOURCE_DIR="${ROOT}/.ladybird"
      if [[ ! -d "$LADYBIRD_SOURCE_DIR/.git" ]]; then
        echo "Cloning Ladybird into $LADYBIRD_SOURCE_DIR (large)…"
        git clone --depth 1 https://github.com/LadybirdBrowser/ladybird.git "$LADYBIRD_SOURCE_DIR"
      fi
    else
      echo "Set LADYBIRD_SOURCE_DIR to a Ladybird checkout, or pass --clone." >&2
      exit 1
    fi
  fi
  if [[ -z "${LADYBIRD_BUILD_DIR:-}" ]]; then
    LADYBIRD_BUILD_DIR="${LADYBIRD_SOURCE_DIR}/Build/release"
  fi
  if [[ ! -d "$LADYBIRD_BUILD_DIR" ]]; then
    echo "Ladybird build dir missing: $LADYBIRD_BUILD_DIR" >&2
    echo "Build Ladybird first (Meta/ladybird.py / cmake --preset Release). See engines/ladybird/README.md." >&2
    exit 1
  fi
  cmake -S "$ROOT" -B "$OUT" \
    -DHAVE_LADYBIRD=ON \
    -DLADYBIRD_SOURCE_DIR="$LADYBIRD_SOURCE_DIR" \
    -DLADYBIRD_BUILD_DIR="$LADYBIRD_BUILD_DIR"
else
  cmake -S "$ROOT" -B "$OUT" -DHAVE_LADYBIRD=OFF
fi

cmake --build "$OUT" -j"${CMAKE_BUILD_PARALLEL_LEVEL:-$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)}"
echo "Built: $OUT/kakangkung-ladybird-host"

# Ladybird helpers are resolved relative to the host binary (…/libexec/<name>).
if [[ "$MODE" == "ladybird" && -d "${LADYBIRD_BUILD_DIR}/libexec" ]]; then
  ln -sfn "${LADYBIRD_BUILD_DIR}/libexec" "${ROOT}/libexec"
  ln -sfn "${LADYBIRD_BUILD_DIR}/bin" "${ROOT}/bin"
fi

"$OUT/kakangkung-ladybird-host" --version || true
