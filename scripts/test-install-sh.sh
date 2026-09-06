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

# --- End-to-end: install_linux() itself, fully offline ---
# The assertions above only exercise the pure-function helpers; none of them
# call install_linux, so none of them could have caught the EXIT-trap /
# out-of-scope-`local` regression (Finding 1 of fix round 1). This case runs
# install_linux for real, with only `curl` stubbed (no network), a throwaway
# $HOME, and the same top-level `install_linux; exit 0` shape install.sh's
# Linux dispatch uses — that shape is what makes the trap fire after the
# function has already returned.

E2E_TMP="$(mktemp -d)"
trap 'rm -rf "$E2E_TMP"' EXIT

ARCH="$(normalize_arch "$(uname -m)")"
ASSET="$(linux_asset_name "$ARCH")"

# The tarball the stubbed download will hand back: two dummy executables
# under the exact names install_linux installs.
PAYLOAD_DIR="${E2E_TMP}/payload"
mkdir -p "$PAYLOAD_DIR"
cat >"${PAYLOAD_DIR}/claude-dashboard-helper" <<'EOF'
#!/usr/bin/env bash
echo "helper"
EOF
cat >"${PAYLOAD_DIR}/claude-dashboard-cli" <<'EOF'
#!/usr/bin/env bash
echo "cli"
EOF
chmod +x "${PAYLOAD_DIR}/claude-dashboard-helper" "${PAYLOAD_DIR}/claude-dashboard-cli"

FIXTURE_TARBALL="${E2E_TMP}/${ASSET}"
tar czf "$FIXTURE_TARBALL" -C "$PAYLOAD_DIR" claude-dashboard-helper claude-dashboard-cli

# The canned releases-API payload. Reuses the real ASSET name computed above
# (via the real normalize_arch/linux_asset_name), so this is deterministic on
# whatever arch the test happens to run on — no uname stub needed.
FIXTURE_JSON="${E2E_TMP}/release.json"
cat >"$FIXTURE_JSON" <<EOF
{"tag_name":"v9.9.9","assets":[{"browser_download_url":"https://example.test/v9.9.9/${ASSET}"}]}
EOF

# Where the stub reports the per-run temp dir install_linux created via
# mktemp, so we can assert afterward that it was actually removed.
REPORTED_TMP_DIR_FILE="${E2E_TMP}/reported_tmp_dir"

# A stub `curl` placed ahead of the real one on PATH. install_linux calls
# curl exactly two ways: `-fsSL <url>` for the releases API, and
# `-fSL --progress-bar -o <path> <url>` for the download. Distinguish on the
# first flag (-fsSL vs -fSL — they differ) rather than on the URL, so this
# stays honest to what the function actually invokes.
STUB_BIN="${E2E_TMP}/bin"
mkdir -p "$STUB_BIN"
cat >"${STUB_BIN}/curl" <<STUBEOF
#!/usr/bin/env bash
if [[ "\$1" == "-fsSL" ]]; then
    cat "$FIXTURE_JSON"
    exit 0
fi
# -fSL --progress-bar -o <path> <url>
out=""
prev=""
for arg in "\$@"; do
    if [[ "\$prev" == "-o" ]]; then
        out="\$arg"
    fi
    prev="\$arg"
done
echo "\$(dirname "\$out")" > "$REPORTED_TMP_DIR_FILE"
cp "$FIXTURE_TARBALL" "\$out"
STUBEOF
chmod +x "${STUB_BIN}/curl"

# Disposable $HOME so LINUX_BIN_DIR (${HOME}/.local/bin) lands somewhere we
# can inspect and throw away, never the real one.
FAKE_HOME="${E2E_TMP}/home"
mkdir -p "$FAKE_HOME"

E2E_OUT="${E2E_TMP}/e2e.log"
E2E_RC=0
(
    export HOME="$FAKE_HOME"
    export PATH="${STUB_BIN}:${PATH}"
    # shellcheck disable=SC1091
    CLAUDE_DASHBOARD_INSTALL_TEST=1 source ./install.sh
    install_linux
    # Mirrors install.sh's own Linux dispatch (`install_linux; exit 0`) — the
    # exit is what fires the EXIT trap after the function has returned.
    exit 0
) >"$E2E_OUT" 2>&1 || E2E_RC=$?

[[ "$E2E_RC" -eq 0 ]] || fail "install_linux (top-level exit 0) exited $E2E_RC, expected 0; output:
$(cat "$E2E_OUT")"

[[ -f "${FAKE_HOME}/.local/bin/claude-dashboard-helper" && -x "${FAKE_HOME}/.local/bin/claude-dashboard-helper" ]] \
    || fail "claude-dashboard-helper not installed executable under \$HOME/.local/bin"
[[ -f "${FAKE_HOME}/.local/bin/claude-dashboard-cli" && -x "${FAKE_HOME}/.local/bin/claude-dashboard-cli" ]] \
    || fail "claude-dashboard-cli not installed executable under \$HOME/.local/bin"

# Nothing should land outside the fake $HOME's .local/bin.
found="$(find "$FAKE_HOME" -type f | wc -l | tr -d ' ')"
[[ "$found" -eq 2 ]] || fail "expected exactly 2 files under fake HOME, found $found: $(find "$FAKE_HOME" -type f)"

# The per-run temp dir install_linux created via mktemp must be gone — proof
# the EXIT trap actually ran (and ran with a bound variable).
reported_tmp_dir="$(cat "$REPORTED_TMP_DIR_FILE" 2>/dev/null || true)"
[[ -n "$reported_tmp_dir" ]] || fail "curl stub never reported install_linux's temp dir"
[[ ! -d "$reported_tmp_dir" ]] || fail "temp dir $reported_tmp_dir was not cleaned up"

echo "PASS: install.sh Linux helpers"
