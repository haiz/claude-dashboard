#!/usr/bin/env bash
# Build the Linux release artifact: a static-musl claude-dashboard-helper plus
# the shared bash CLI, packed flat the way the macOS tarball is packed
# (scripts/release.sh:130).
#
# Usage: scripts/build-linux.sh <x86_64|aarch64>
#
# Builds and packs only. It runs no tests, uploads nothing, and writes no
# checksum into the repository (release-workflow guardrail 5).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

error() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Linux" ]] \
    || error "this script builds musl natively and only runs on Linux (this is $(uname -s))"

ARCH="${1:-}"
case "$ARCH" in
    x86_64|aarch64) ;;
    "") error "usage: $0 <x86_64|aarch64>" ;;
    *)  error "unsupported arch '$ARCH' (supported: x86_64, aarch64)" ;;
esac

TARGET="${ARCH}-unknown-linux-musl"
STAGING="$REPO_ROOT/.build/linux-staging/${ARCH}"
TARBALL="$REPO_ROOT/.build/claude-dashboard-cli-linux-${ARCH}.tar.gz"

# rust-toolchain.toml lives in apps/linux and pins channel 1.98.0. Running cargo
# from the repo root would silently use the default toolchain instead.
( cd apps/linux && cargo build --release --locked --target "$TARGET" -p claude-dashboard-helper )

HELPER="apps/linux/target/${TARGET}/release/claude-dashboard-helper"
[[ -x "$HELPER" ]] || error "$HELPER not found after the build"

rm -rf "$STAGING"
mkdir -p "$STAGING" "$REPO_ROOT/.build"
install -m755 "$HELPER" "$STAGING/claude-dashboard-helper"
install -m755 cli/claude-dashboard-cli "$STAGING/claude-dashboard-cli"

tar czf "$TARBALL" -C "$STAGING" claude-dashboard-helper claude-dashboard-cli

echo "  $TARBALL"
echo "  size    $(du -h "$TARBALL" | cut -f1 | xargs)"
echo "  sha256  $(sha256sum "$TARBALL" | awk '{print $1}')"
