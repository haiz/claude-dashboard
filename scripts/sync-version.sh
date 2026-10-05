#!/usr/bin/env bash
# Sync the version in VERSION to Info.plist, CLI, Formula, Cask, the Linux and
# Windows Cargo workspaces (Cargo.toml + Cargo.lock), the GNOME extension
# metadata, and the Windows browser extension manifest.
# Usage: ./scripts/sync-version.sh
set -euo pipefail

# Locate repo root from script path (works when invoked from any cwd).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

VERSION_FILE="$REPO_ROOT/VERSION"

if [[ ! -f "$VERSION_FILE" ]]; then
    echo "error: $VERSION_FILE not found" >&2
    exit 1
fi

VERSION="$(tr -d '[:space:]' < "$VERSION_FILE")"

if [[ -z "$VERSION" ]]; then
    echo "error: VERSION file is empty" >&2
    exit 1
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: VERSION '$VERSION' is not semver (X.Y.Z)" >&2
    exit 1
fi

# Returns 0 if file already contains the expected string, 1 otherwise.
# Usage: report <path> <grep-pattern-for-expected-line>
report() {
    local path="$1" pattern="$2"
    if grep -qE "$pattern" "$path"; then
        echo "  $path — OK"
    else
        echo "  $path — FAILED to apply" >&2
        return 1
    fi
}

# In-place edit that works on BSD sed (macOS) and GNU sed (Linux, Git Bash):
# both accept -i with an attached suffix; the backup is removed straight away.
sedi() {
    local file="${*: -1}"
    sed -i.bak "$@" && rm -f "${file}.bak"
}

echo "Syncing version $VERSION..."

# 1. Info.plist — replace the <string> on the line AFTER <key>CFBundleShortVersionString</key>.
INFO_PLIST="apps/macos/ClaudeDashboard/Info.plist"
sedi "/<key>CFBundleShortVersionString<\/key>/{n;s|<string>[^<]*</string>|<string>${VERSION}</string>|;}" "$INFO_PLIST"
report "$INFO_PLIST" "<string>${VERSION}</string>"

# 2. CLI — replace the VERSION="..." line.
CLI="cli/claude-dashboard-cli"
sedi "s|^VERSION=\"[^\"]*\"|VERSION=\"${VERSION}\"|" "$CLI"
report "$CLI" "^VERSION=\"${VERSION}\""

# 3. Formula — replace version "..." line AND the /vX.Y.Z/ segment in the url.
FORMULA="Formula/claude-dashboard-cli.rb"
sedi "s|^  version \"[^\"]*\"|  version \"${VERSION}\"|" "$FORMULA"
sedi "s|/v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*/|/v${VERSION}/|" "$FORMULA"
report "$FORMULA" "^  version \"${VERSION}\""
report "$FORMULA" "/v${VERSION}/"

# 4. Cask — replace version "..." line. url uses #{version} interpolation, no edit needed.
CASK="Casks/claude-dashboard.rb"
sedi "s|^  version \"[^\"]*\"|  version \"${VERSION}\"|" "$CASK"
report "$CASK" "^  version \"${VERSION}\""

# 5. Rust workspace — the [workspace.package] version, plus the copy the lock
# file keeps for each member. Leaving the lock behind makes `cargo --locked`
# fail on the next build, so both files move together or neither does.
CARGO_TOML="apps/linux/Cargo.toml"
sedi "s|^version = \"[^\"]*\"|version = \"${VERSION}\"|" "$CARGO_TOML"
report "$CARGO_TOML" "^version = \"${VERSION}\""

CARGO_LOCK="apps/linux/Cargo.lock"
for member in claude-dashboard-core claude-dashboard-helper; do
    sedi "/^name = \"${member}\"$/{n;s|^version = \"[^\"]*\"|version = \"${VERSION}\"|;}" "$CARGO_LOCK"
done
# Checked per member rather than by counting matches: a third-party dep may
# legitimately sit at the same version, which would make a count lie.
for member in claude-dashboard-core claude-dashboard-helper; do
    got="$(awk -v m="$member" '$0 == "name = \"" m "\"" { getline; print; exit }' "$CARGO_LOCK")"
    if [[ "$got" == "version = \"${VERSION}\"" ]]; then
        echo "  $CARGO_LOCK ($member) — OK"
    else
        echo "  $CARGO_LOCK ($member) — FAILED to apply" >&2
        exit 1
    fi
done

# 6. GNOME Shell extension metadata — the "version-name" key, which is the
# human release string GNOME shows and the one the extension's Help dialog
# prints in its footer. ("version" is e.g.o's own revision counter and is
# deliberately not touched here.)
EXT_METADATA="apps/linux/gnome-extension/metadata.json"
sedi "s|\"version-name\": \"[^\"]*\"|\"version-name\": \"${VERSION}\"|" "$EXT_METADATA"
report "$EXT_METADATA" "\"version-name\": \"${VERSION}\""

# 7. Windows Cargo workspace — [workspace.package] version (trailing comment kept).
WIN_CARGO_TOML="apps/windows/Cargo.toml"
sedi "s|^version = \"[^\"]*\"|version = \"${VERSION}\"|" "$WIN_CARGO_TOML"
report "$WIN_CARGO_TOML" "^version = \"${VERSION}\""

# 8. Windows lock — the app, the bridge, and the path-dep core all carry the version.
WIN_CARGO_LOCK="apps/windows/Cargo.lock"
for member in claude-dashboard claude-dashboard-bridge claude-dashboard-core; do
    sedi "/^name = \"${member}\"$/{n;s|^version = \"[^\"]*\"|version = \"${VERSION}\"|;}" "$WIN_CARGO_LOCK"
done
for member in claude-dashboard claude-dashboard-bridge claude-dashboard-core; do
    got="$(awk -v m="$member" '$0 == "name = \"" m "\"" { getline; print; exit }' "$WIN_CARGO_LOCK")"
    if [[ "$got" == "version = \"${VERSION}\"" ]]; then
        echo "  $WIN_CARGO_LOCK ($member) — OK"
    else
        echo "  $WIN_CARGO_LOCK ($member) — FAILED to apply" >&2
        exit 1
    fi
done

# 9. Browser extension manifest — Chrome requires 1-4 dot-separated integers.
EXT_MANIFEST="apps/windows/extension/manifest.json"
sedi "s|\"version\": \"[^\"]*\"|\"version\": \"${VERSION}\"|" "$EXT_MANIFEST"
report "$EXT_MANIFEST" "\"version\": \"${VERSION}\""

echo "Done."
