#!/usr/bin/env bash
# Test harness for scripts/sync-version.sh.
# Builds a temp repo skeleton from real fixtures, runs the script, asserts.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SYNC="$SCRIPT_DIR/sync-version.sh"

PASS=0
FAIL=0

# Build a temp repo skeleton from real files at the current HEAD.
make_fixture() {
    local tmp
    tmp="$(mktemp -d)"
    mkdir -p "$tmp/ClaudeDashboard" "$tmp/cli" "$tmp/Formula" "$tmp/Casks" "$tmp/scripts"
    cp "$REPO_ROOT/ClaudeDashboard/Info.plist" "$tmp/ClaudeDashboard/Info.plist"
    cp "$REPO_ROOT/cli/claude-dashboard-cli" "$tmp/cli/claude-dashboard-cli"
    cp "$REPO_ROOT/Formula/claude-dashboard-cli.rb" "$tmp/Formula/claude-dashboard-cli.rb"
    cp "$REPO_ROOT/Casks/claude-dashboard.rb" "$tmp/Casks/claude-dashboard.rb"
    cp "$SYNC" "$tmp/scripts/sync-version.sh"
    chmod +x "$tmp/scripts/sync-version.sh"
    echo "$tmp"
}

ok() { PASS=$((PASS + 1)); echo "  PASS: $1"; }
ko() { FAIL=$((FAIL + 1)); echo "  FAIL: $1" >&2; }

# --- Test 1: happy path bumps all four files to the given version. ---
echo "Test 1: happy path"
T1="$(make_fixture)"
echo "9.9.9" > "$T1/VERSION"
"$T1/scripts/sync-version.sh" >/dev/null
grep -q "<string>9.9.9</string>" "$T1/ClaudeDashboard/Info.plist" && ok "Info.plist bumped" || ko "Info.plist bumped"
grep -q '^VERSION="9.9.9"' "$T1/cli/claude-dashboard-cli" && ok "CLI bumped" || ko "CLI bumped"
grep -q '^  version "9.9.9"' "$T1/Formula/claude-dashboard-cli.rb" && ok "Formula version bumped" || ko "Formula version bumped"
grep -q '/v9.9.9/' "$T1/Formula/claude-dashboard-cli.rb" && ok "Formula url bumped" || ko "Formula url bumped"
grep -q '^  version "9.9.9"' "$T1/Casks/claude-dashboard.rb" && ok "Cask bumped" || ko "Cask bumped"
rm -rf "$T1"

# --- Test 2: idempotency — running twice with same VERSION produces zero diff. ---
echo "Test 2: idempotency"
T2="$(make_fixture)"
echo "2.3.4" > "$T2/VERSION"
"$T2/scripts/sync-version.sh" >/dev/null
SNAPSHOT="$(mktemp -d)"
cp "$T2/ClaudeDashboard/Info.plist" "$SNAPSHOT/"
cp "$T2/cli/claude-dashboard-cli" "$SNAPSHOT/"
cp "$T2/Formula/claude-dashboard-cli.rb" "$SNAPSHOT/"
cp "$T2/Casks/claude-dashboard.rb" "$SNAPSHOT/"
"$T2/scripts/sync-version.sh" >/dev/null
diff -q "$SNAPSHOT/Info.plist" "$T2/ClaudeDashboard/Info.plist" >/dev/null && ok "Info.plist idempotent" || ko "Info.plist idempotent"
diff -q "$SNAPSHOT/claude-dashboard-cli" "$T2/cli/claude-dashboard-cli" >/dev/null && ok "CLI idempotent" || ko "CLI idempotent"
diff -q "$SNAPSHOT/claude-dashboard-cli.rb" "$T2/Formula/claude-dashboard-cli.rb" >/dev/null && ok "Formula idempotent" || ko "Formula idempotent"
diff -q "$SNAPSHOT/claude-dashboard.rb" "$T2/Casks/claude-dashboard.rb" >/dev/null && ok "Cask idempotent" || ko "Cask idempotent"
rm -rf "$T2" "$SNAPSHOT"

# --- Test 3: malformed VERSION is rejected. ---
echo "Test 3: malformed VERSION rejected"
T3="$(make_fixture)"
echo "v1.2.3" > "$T3/VERSION"
if "$T3/scripts/sync-version.sh" >/dev/null 2>&1; then
    ko "malformed 'v1.2.3' should have been rejected"
else
    ok "malformed 'v1.2.3' rejected"
fi
rm -rf "$T3"

# --- Test 4: empty VERSION is rejected. ---
echo "Test 4: empty VERSION rejected"
T4="$(make_fixture)"
: > "$T4/VERSION"
if "$T4/scripts/sync-version.sh" >/dev/null 2>&1; then
    ko "empty VERSION should have been rejected"
else
    ok "empty VERSION rejected"
fi
rm -rf "$T4"

# --- Test 5: missing VERSION file is rejected. ---
echo "Test 5: missing VERSION rejected"
T5="$(make_fixture)"
# deliberately do NOT create VERSION
if "$T5/scripts/sync-version.sh" >/dev/null 2>&1; then
    ko "missing VERSION should have been rejected"
else
    ok "missing VERSION rejected"
fi
rm -rf "$T5"

echo
echo "Results: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
