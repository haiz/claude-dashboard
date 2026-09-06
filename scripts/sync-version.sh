#!/usr/bin/env bash
# Sync the version in VERSION to Info.plist, CLI, Formula, and Cask.
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

echo "Syncing version $VERSION..."

# 1. Info.plist — replace the <string> on the line AFTER <key>CFBundleShortVersionString</key>.
INFO_PLIST="apps/macos/ClaudeDashboard/Info.plist"
sed -i '' "/<key>CFBundleShortVersionString<\/key>/{n;s|<string>[^<]*</string>|<string>${VERSION}</string>|;}" "$INFO_PLIST"
report "$INFO_PLIST" "<string>${VERSION}</string>"

# 2. CLI — replace the VERSION="..." line.
CLI="cli/claude-dashboard-cli"
sed -i '' "s|^VERSION=\"[^\"]*\"|VERSION=\"${VERSION}\"|" "$CLI"
report "$CLI" "^VERSION=\"${VERSION}\""

# 3. Formula — replace version "..." line AND the /vX.Y.Z/ segment in the url.
FORMULA="Formula/claude-dashboard-cli.rb"
sed -i '' "s|^  version \"[^\"]*\"|  version \"${VERSION}\"|" "$FORMULA"
sed -i '' "s|/v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*/|/v${VERSION}/|" "$FORMULA"
report "$FORMULA" "^  version \"${VERSION}\""
report "$FORMULA" "/v${VERSION}/"

# 4. Cask — replace version "..." line. url uses #{version} interpolation, no edit needed.
CASK="Casks/claude-dashboard.rb"
sed -i '' "s|^  version \"[^\"]*\"|  version \"${VERSION}\"|" "$CASK"
report "$CASK" "^  version \"${VERSION}\""

# 5. Rust workspace — the [workspace.package] version, plus the copy the lock
# file keeps for each member. Leaving the lock behind makes `cargo --locked`
# fail on the next build, so both files move together or neither does.
CARGO_TOML="apps/linux/Cargo.toml"
sed -i '' "s|^version = \"[^\"]*\"|version = \"${VERSION}\"|" "$CARGO_TOML"
report "$CARGO_TOML" "^version = \"${VERSION}\""

CARGO_LOCK="apps/linux/Cargo.lock"
for member in claude-dashboard-core claude-dashboard-helper; do
    sed -i '' "/^name = \"${member}\"$/{n;s|^version = \"[^\"]*\"|version = \"${VERSION}\"|;}" "$CARGO_LOCK"
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

echo "Done."
