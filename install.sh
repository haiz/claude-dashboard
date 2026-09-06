#!/bin/bash
set -euo pipefail

# Claude Dashboard Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/haiz/claude-dashboard/main/install.sh | bash

APP_NAME="Claude Dashboard"
APP_BUNDLE="ClaudeDashboard.app"
INSTALL_DIR="/Applications"
REPO="haiz/claude-dashboard"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info()  { echo -e "${BLUE}==>${NC} $1"; }
ok()    { echo -e "${GREEN}==>${NC} $1"; }
warn()  { echo -e "${YELLOW}==>${NC} $1"; }
error() { echo -e "${RED}==>${NC} $1"; exit 1; }

# --- Linux ---
LINUX_BIN_DIR="${HOME}/.local/bin"

normalize_arch() {
    case "$1" in
        x86_64|amd64)  echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) error "Unsupported architecture: $1. Prebuilt binaries exist for x86_64 and aarch64 only." ;;
    esac
}

linux_asset_name() { echo "claude-dashboard-cli-linux-$1.tar.gz"; }

# Resolve one exact asset name out of a releases API payload. Matching the full
# filename matters: the macOS tarball is a .tar.gz too and is uploaded first, so
# a "[^\"]*\.tar\.gz" pattern would return it instead. The trailing `|| true`
# keeps a no-match from tripping `set -o pipefail`, so the caller can report a
# missing asset rather than dying silently.
linux_asset_url() {
    echo "$1" | grep -o "\"browser_download_url\": *\"[^\"]*/$2\"" | head -1 | cut -d'"' -f4 || true
}

install_linux() {
    command -v jq   >/dev/null 2>&1 || error "jq is required. Install it with: apt install jq   (or: dnf install jq)"
    command -v tar  >/dev/null 2>&1 || error "tar is required."
    command -v curl >/dev/null 2>&1 || error "curl is required."
    command -v secret-tool >/dev/null 2>&1 \
        || warn "secret-tool not found — browser scanning (sync) will not work. Install libsecret-tools to enable it."

    local arch asset
    arch="$(normalize_arch "$(uname -m)")"
    asset="$(linux_asset_name "$arch")"

    info "Installing ${APP_NAME} CLI (${arch})..."
    info "Fetching latest release..."

    local release_json version url
    release_json=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest") \
        || error "Failed to fetch release info. Check your internet connection."
    version=$(echo "$release_json" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)
    url=$(linux_asset_url "$release_json" "$asset")

    [[ -n "$url" ]] || error "Release ${version} has no ${asset} yet. The Linux build may still be running — try again in a few minutes, or check https://github.com/${REPO}/releases"

    info "Found ${APP_NAME} CLI ${version}"

    local tmp
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    info "Downloading..."
    curl -fSL --progress-bar -o "${tmp}/${asset}" "$url" || error "Download failed."
    tar xzf "${tmp}/${asset}" -C "$tmp" || error "Failed to extract ${asset}."

    mkdir -p "$LINUX_BIN_DIR"
    install -m755 "${tmp}/claude-dashboard-helper" "${LINUX_BIN_DIR}/claude-dashboard-helper"
    install -m755 "${tmp}/claude-dashboard-cli"    "${LINUX_BIN_DIR}/claude-dashboard-cli"

    echo ""
    ok "${APP_NAME} CLI ${version} installed to ${LINUX_BIN_DIR}"
    echo ""
    case ":${PATH}:" in
        *":${LINUX_BIN_DIR}:"*)
            echo "  Get started:  claude-dashboard-cli sync" ;;
        *)
            warn "${LINUX_BIN_DIR} is not on your PATH. Add it with:"
            echo "    echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc"
            echo ""
            echo "  Or run it directly:  ${LINUX_BIN_DIR}/claude-dashboard-cli sync" ;;
    esac
    echo ""
    warn "There is no Linux GUI yet — the menu bar app is macOS only."
}

# When sourced by a test, stop here: definitions only, nothing installed.
if [[ -n "${CLAUDE_DASHBOARD_INSTALL_TEST:-}" ]]; then
    return 0 2>/dev/null || exit 0
fi

# --- Pre-flight checks ---
OS="$(uname -s)"
if [[ "$OS" == "Linux" ]]; then
    install_linux
    exit 0
fi
[[ "$OS" == "Darwin" ]] || error "Unsupported OS: $OS. macOS and Linux only."

MACOS_VERSION=$(sw_vers -productVersion)
MAJOR=$(echo "$MACOS_VERSION" | cut -d. -f1)
[[ "$MAJOR" -ge 13 ]] || error "macOS 13 (Ventura) or later is required. You have $MACOS_VERSION."

info "Installing ${APP_NAME}..."

# --- Get latest release URL ---
info "Fetching latest release..."
RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest") \
  || error "Failed to fetch release info. Check your internet connection."

DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep -o '"browser_download_url": *"[^"]*\.zip"' | head -1 | cut -d'"' -f4)
VERSION=$(echo "$RELEASE_JSON" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)

[[ -n "$DOWNLOAD_URL" ]] || error "No .zip asset found in the latest release."

info "Found ${APP_NAME} ${VERSION}"

# --- Download ---
TMPDIR_PATH=$(mktemp -d)
trap 'rm -rf "$TMPDIR_PATH"' EXIT

ZIP_PATH="${TMPDIR_PATH}/${APP_BUNDLE}.zip"
info "Downloading..."
curl -fSL --progress-bar -o "$ZIP_PATH" "$DOWNLOAD_URL" \
  || error "Download failed."

# --- Install ---
info "Installing to ${INSTALL_DIR}..."

# Remove previous installation if exists
if [[ -d "${INSTALL_DIR}/${APP_BUNDLE}" ]]; then
  warn "Removing previous installation..."
  rm -rf "${INSTALL_DIR}/${APP_BUNDLE}"
fi

# Unzip
ditto -xk "$ZIP_PATH" "$INSTALL_DIR" \
  || error "Failed to extract app."

# Remove quarantine attribute (app is unsigned)
xattr -cr "${INSTALL_DIR}/${APP_BUNDLE}" 2>/dev/null || true

# --- Done ---
echo ""
ok "${APP_NAME} ${VERSION} installed successfully!"
echo ""
echo "  Open from menu bar:  open -a '${APP_NAME}'"
echo "  Or find it in:       ${INSTALL_DIR}/${APP_BUNDLE}"
echo ""
warn "Note: On first launch, if macOS blocks the app:"
echo "  System Settings > Privacy & Security > scroll down > Open Anyway"
echo ""
