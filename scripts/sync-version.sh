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
INFO_PLIST="ClaudeDashboard/Info.plist"
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

echo "Done."
