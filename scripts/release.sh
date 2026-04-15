#!/usr/bin/env bash
# Cut a new release: bump version, build, create artifacts, update Homebrew sha256,
# commit, tag, push, and create a GitHub release.
#
# Usage: ./scripts/release.sh <new-version>
#   e.g. ./scripts/release.sh 1.3.0
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# ── Args ──────────────────────────────────────────────────────────────────────
NEW_VERSION="${1:-}"
if [[ -z "$NEW_VERSION" ]]; then
    echo "Usage: $0 <new-version>"
    echo "  e.g. $0 1.3.0"
    exit 1
fi

if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "error: '$NEW_VERSION' is not valid semver (X.Y.Z)" >&2
    exit 1
fi

OLD_VERSION="$(tr -d '[:space:]' < VERSION)"
if [[ "$NEW_VERSION" == "$OLD_VERSION" ]]; then
    echo "error: new version ($NEW_VERSION) is the same as current ($OLD_VERSION)" >&2
    exit 1
fi

# ── Preflight checks ─────────────────────────────────────────────────────────
for cmd in xcodegen xcodebuild gh shasum ditto; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "error: '$cmd' not found in PATH" >&2
        exit 1
    fi
done

if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree is dirty — commit or stash first" >&2
    exit 1
fi

echo "==> Releasing $OLD_VERSION → $NEW_VERSION"

# ── 1. Bump version ──────────────────────────────────────────────────────────
echo ""
echo "==> Step 1: Bump version"
printf '%s\n' "$NEW_VERSION" > VERSION
./scripts/sync-version.sh

# ── 2. Regenerate Xcode project ──────────────────────────────────────────────
echo ""
echo "==> Step 2: Regenerate Xcode project"
xcodegen generate

# ── 3. Build ──────────────────────────────────────────────────────────────────
echo ""
echo "==> Step 3: Build app + helper"
DERIVED_DATA="$REPO_ROOT/.build/DerivedData"
RELEASE_DIR="$DERIVED_DATA/Build/Products/Release"

xcodebuild -project ClaudeDashboard.xcodeproj \
    -scheme ClaudeDashboard \
    -configuration Release \
    -derivedDataPath "$DERIVED_DATA" \
    build 2>&1 | tail -3

xcodebuild -project ClaudeDashboard.xcodeproj \
    -scheme ClaudeDashboardHelper \
    -configuration Release \
    -derivedDataPath "$DERIVED_DATA" \
    build 2>&1 | tail -3

# ── 4. Run tests ─────────────────────────────────────────────────────────────
echo ""
echo "==> Step 4: Run tests"
xcodebuild -project ClaudeDashboard.xcodeproj \
    -scheme ClaudeDashboardTests \
    -derivedDataPath "$DERIVED_DATA" \
    test 2>&1 | tail -3

# ── 5. Create artifacts ──────────────────────────────────────────────────────
echo ""
echo "==> Step 5: Create release artifacts"
STAGING="$REPO_ROOT/.build/release-staging"
rm -rf "$STAGING"
mkdir -p "$STAGING"

# App zip — name must match Cask url: ClaudeDashboard.app.zip
APP_PATH="$RELEASE_DIR/ClaudeDashboard.app"
if [[ ! -d "$APP_PATH" ]]; then
    echo "error: $APP_PATH not found" >&2
    exit 1
fi
ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$STAGING/ClaudeDashboard.app.zip"

# CLI tarball — must contain both claude-dashboard-cli AND claude-dashboard-helper
HELPER_PATH="$RELEASE_DIR/claude-dashboard-helper"
CLI_PATH="$REPO_ROOT/cli/claude-dashboard-cli"
if [[ ! -f "$HELPER_PATH" ]]; then
    echo "error: $HELPER_PATH not found" >&2
    exit 1
fi
cp "$HELPER_PATH" "$STAGING/claude-dashboard-helper"
cp "$CLI_PATH" "$STAGING/claude-dashboard-cli"
tar czf "$STAGING/claude-dashboard-cli.tar.gz" -C "$STAGING" claude-dashboard-helper claude-dashboard-cli

echo "  ClaudeDashboard.app.zip  — $(du -h "$STAGING/ClaudeDashboard.app.zip" | cut -f1 | xargs)"
echo "  claude-dashboard-cli.tar.gz — $(du -h "$STAGING/claude-dashboard-cli.tar.gz" | cut -f1 | xargs)"

# ── 6. Update Homebrew sha256 ─────────────────────────────────────────────────
echo ""
echo "==> Step 6: Update Homebrew sha256"
CLI_SHA="$(shasum -a 256 "$STAGING/claude-dashboard-cli.tar.gz" | awk '{print $1}')"
APP_SHA="$(shasum -a 256 "$STAGING/ClaudeDashboard.app.zip" | awk '{print $1}')"

sed -i '' "s/sha256 \"[a-f0-9]*\"/sha256 \"${CLI_SHA}\"/" Formula/claude-dashboard-cli.rb
sed -i '' "s/sha256 \"[a-f0-9]*\"/sha256 \"${APP_SHA}\"/" Casks/claude-dashboard.rb

echo "  Formula sha256: $CLI_SHA"
echo "  Cask    sha256: $APP_SHA"

# ── 7. Commit, tag, push ─────────────────────────────────────────────────────
echo ""
echo "==> Step 7: Commit, tag, push"
git add VERSION ClaudeDashboard/Info.plist cli/claude-dashboard-cli \
    Formula/claude-dashboard-cli.rb Casks/claude-dashboard.rb
git commit -m "chore: release v${NEW_VERSION}"
git tag "v${NEW_VERSION}"
git push
git push --tags

# ── 8. Create GitHub release ─────────────────────────────────────────────────
echo ""
echo "==> Step 8: Create GitHub release"
RELEASE_URL=$(gh release create "v${NEW_VERSION}" \
    "$STAGING/ClaudeDashboard.app.zip" \
    "$STAGING/claude-dashboard-cli.tar.gz" \
    --title "v${NEW_VERSION}" \
    --generate-notes)

# ── 9. Verify uploaded checksums ─────────────────────────────────────────────
echo ""
echo "==> Step 9: Verify uploaded artifacts match checksums"
VERIFY_DIR="$(mktemp -d)"
gh release download "v${NEW_VERSION}" \
    -p 'claude-dashboard-cli.tar.gz' \
    -p 'ClaudeDashboard.app.zip' \
    -D "$VERIFY_DIR"

DL_CLI_SHA="$(shasum -a 256 "$VERIFY_DIR/claude-dashboard-cli.tar.gz" | awk '{print $1}')"
DL_APP_SHA="$(shasum -a 256 "$VERIFY_DIR/ClaudeDashboard.app.zip" | awk '{print $1}')"
rm -rf "$VERIFY_DIR"

if [[ "$DL_CLI_SHA" != "$CLI_SHA" ]] || [[ "$DL_APP_SHA" != "$APP_SHA" ]]; then
    echo "  ✗ Checksum mismatch detected — fixing..."
    sed -i '' "s/sha256 \"[a-f0-9]*\"/sha256 \"${DL_CLI_SHA}\"/" Formula/claude-dashboard-cli.rb
    sed -i '' "s/sha256 \"[a-f0-9]*\"/sha256 \"${DL_APP_SHA}\"/" Casks/claude-dashboard.rb
    git add Formula/claude-dashboard-cli.rb Casks/claude-dashboard.rb
    git commit -m "fix: update SHA-256 checksums for v${NEW_VERSION} release artifacts"
    git push
    echo "  ✓ Checksums fixed and pushed"
else
    echo "  ✓ Both checksums verified"
fi

# ── Cleanup ───────────────────────────────────────────────────────────────────
rm -rf "$STAGING"

echo ""
echo "==> Done! $RELEASE_URL"
