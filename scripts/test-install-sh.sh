#!/usr/bin/env bash
# Unit test for install.sh's Linux helpers: architecture normalisation and
# asset-URL resolution. No network, nothing installed, no OS branch executed.
set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "FAIL: $*" >&2; exit 1; }

bash -n install.sh || fail "install.sh is not valid bash"
grep -q 'CLAUDE_DASHBOARD_INSTALL_TEST' install.sh || fail "install.sh has no test guard"

# shellcheck disable=SC1091
CLAUDE_DASHBOARD_INSTALL_TEST=1 source ./install.sh

for pair in "x86_64:x86_64" "amd64:x86_64" "aarch64:aarch64" "arm64:aarch64"; do
    got="$(normalize_arch "${pair%%:*}")"
    [[ "$got" == "${pair##*:}" ]] || fail "normalize_arch ${pair%%:*} = '$got', want '${pair##*:}'"
done

# error() ends in exit, so the negative case must run in a subshell.
if ( normalize_arch armv7l ) >/dev/null 2>&1; then
    fail "normalize_arch accepted armv7l"
fi

[[ "$(linux_asset_name x86_64)"  == "claude-dashboard-cli-linux-x86_64.tar.gz"  ]] || fail "linux_asset_name x86_64"
[[ "$(linux_asset_name aarch64)" == "claude-dashboard-cli-linux-aarch64.tar.gz" ]] || fail "linux_asset_name aarch64"

# The collision case. A real release payload carries the macOS tarball too, and
# release.sh uploads it before CI uploads anything, so a wildcard suffix would
# resolve to it.
JSON='{"tag_name":"v1.17.0","assets":[
 {"browser_download_url": "https://example.test/v1.17.0/ClaudeDashboard.app.zip"},
 {"browser_download_url": "https://example.test/v1.17.0/claude-dashboard-cli.tar.gz"},
 {"browser_download_url": "https://example.test/v1.17.0/claude-dashboard-cli-linux-x86_64.tar.gz"},
 {"browser_download_url": "https://example.test/v1.17.0/claude-dashboard-cli-linux-aarch64.tar.gz"}]}'

got="$(linux_asset_url "$JSON" "$(linux_asset_name x86_64)")"
[[ "$got" == "https://example.test/v1.17.0/claude-dashboard-cli-linux-x86_64.tar.gz" ]] \
    || fail "x86_64 resolution picked '$got'"

got="$(linux_asset_url "$JSON" "$(linux_asset_name aarch64)")"
[[ "$got" == "https://example.test/v1.17.0/claude-dashboard-cli-linux-aarch64.tar.gz" ]] \
    || fail "aarch64 resolution picked '$got'"

# A release with no Linux asset yet must resolve to empty, never to the macOS
# tarball — that is what makes the "CI may still be building" message reachable.
MAC_ONLY='{"assets":[{"browser_download_url": "https://example.test/v1.16.0/claude-dashboard-cli.tar.gz"}]}'
got="$(linux_asset_url "$MAC_ONLY" "$(linux_asset_name x86_64)")"
[[ -z "$got" ]] || fail "empty-release case resolved to '$got'"

echo "PASS: install.sh Linux helpers"
