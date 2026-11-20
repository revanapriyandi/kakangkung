#!/usr/bin/env bash
# Package a real LibWeb/Lagom runtime tree for Kakangkung (Boat).
# Produces ladybird-runtime.tgz with:
#   kakangkung-ladybird-host
#   lib/liblagom-*.so*
#   share/Lagom/  (resource://ladybird)
#
# Usage (on Boat bx_cnfw3mch after build.sh --ladybird):
#   ./engines/ladybird/package-runtime.sh
#   # → dist/ladybird-runtime-linux-x64.tgz
#
# Windows GHA can consume a zip of the same layout via secrets.LADYBIRD_RUNTIME_URL
# once a Windows Lagom build exists. Linux .so trees are for Linux releases only.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HOST_OUT="${HOST_OUT:-$ROOT/engines/ladybird/out/kakangkung-ladybird-host}"
LB_BUILD="${LADYBIRD_BUILD_DIR:-$HOME/ladybird/Build/release}"
STAGE="${TMPDIR:-/tmp}/kk-ladybird-runtime-$$"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
ARCH="${ARCH:-linux-x64}"

if [[ ! -x "$HOST_OUT" ]]; then
  echo "missing host: $HOST_OUT (build engines/ladybird with HAVE_LADYBIRD=1)" >&2
  exit 1
fi
if [[ ! -d "$LB_BUILD/lib" ]]; then
  echo "missing Lagom libs under $LB_BUILD/lib" >&2
  exit 1
fi

rm -rf "$STAGE"
mkdir -p "$STAGE/lib" "$STAGE/share" "$OUT_DIR"
cp -a "$HOST_OUT" "$STAGE/kakangkung-ladybird-host"
# Shared objects the host actually links (ldd) plus webview.
mapfile -t NEED < <(ldd "$HOST_OUT" | awk '/liblagom/{print $3}' | sort -u)
for so in "${NEED[@]}"; do
  [[ -f "$so" ]] || continue
  cp -a "$so" "$STAGE/lib/"
  # Also copy SONAME siblings (liblagom-foo.so.0 → .so.0.1.0).
  base="$(basename "$so")"
  for sib in "$LB_BUILD/lib/$base"*; do
    [[ -e "$sib" ]] && cp -a "$sib" "$STAGE/lib/" || true
  done
done
# Ensure webview is present even if not yet linked in a partial build.
cp -a "$LB_BUILD/lib"/liblagom-webview.so* "$STAGE/lib/" 2>/dev/null || true
if [[ -d "$LB_BUILD/share/Lagom" ]]; then
  cp -a "$LB_BUILD/share/Lagom" "$STAGE/share/"
elif [[ -d "$LB_BUILD/Share/Lagom" ]]; then
  cp -a "$LB_BUILD/Share/Lagom" "$STAGE/share/"
fi

TGZ="$OUT_DIR/ladybird-runtime-$ARCH.tgz"
tar -C "$STAGE" -czf "$TGZ" .
rm -rf "$STAGE"
BYTES=$(wc -c <"$TGZ")
echo "wrote $TGZ ($((BYTES / 1024 / 1024)) MB)"
if (( BYTES < 5 * 1024 * 1024 )); then
  echo "warning: archive looks small for a full Lagom runtime" >&2
fi
