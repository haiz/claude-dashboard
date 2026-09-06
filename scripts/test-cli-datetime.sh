#!/usr/bin/env bash
# Contract test for the bash CLI's reset-time formatting across date flavours.
# BSD date (macOS) and GNU date (Linux) share none of the relevant flags, so the
# same fixtures run through both branches. No network, no helper calls.
#
# Sourcing the CLI runs its helper lookup, which exits 1 when no
# claude-dashboard-helper is installed. That is a prerequisite, not a failure of
# this test.
set -euo pipefail
cd "$(dirname "$0")/.."

CLI=cli/claude-dashboard-cli

fail() { echo "FAIL: $*" >&2; exit 1; }

# 2026-09-06T15:00:00Z is a Sunday; in UTC it renders "Sun 3p". The three input
# shapes are the ISO8601 variants the API actually returns — with and without
# fractional seconds, Z and +00:00 (CLAUDE.md, "ISO8601 date parsing").
run_cases() {
    local label="$1" iso got

    for iso in "2026-09-06T15:00:00Z" "2026-09-06T15:00:00.123Z" "2026-09-06T15:00:00+00:00"; do
        got="$(TZ=UTC format_reset_time "$iso")"
        [[ "$got" == "Sun 3p" ]] || fail "$label: format_reset_time '$iso' = '$got', want 'Sun 3p'"
    done

    for iso in "" "null" "not-a-date"; do
        got="$(TZ=UTC format_reset_time "$iso")"
        [[ -z "$got" ]] || fail "$label: format_reset_time '$iso' = '$got', want empty"
    done

    echo "ok: $label (DATE_FLAVOUR=$DATE_FLAVOUR)"
}

# --- Whatever flavour this machine has ---
# shellcheck disable=SC1090
CLAUDE_DASHBOARD_CLI_TEST=1 source "$CLI"
declare -f format_reset_time >/dev/null || fail "format_reset_time is not defined"
declare -f install_hint      >/dev/null || fail "install_hint is not defined"
[[ -n "${DATE_FLAVOUR:-}" ]] || fail "DATE_FLAVOUR is not set"
run_cases native

# --- The other flavour, via a PATH shim ---
# On macOS the native branch is bsd, and gdate (Homebrew coreutils) is GNU date,
# so shimming `date` -> gdate exercises the branch Linux takes without a VM. On
# Linux the native branch is already gnu and there is no BSD date to shim.
if [[ "$DATE_FLAVOUR" == "bsd" ]] && command -v gdate >/dev/null 2>&1; then
    SHIM="$(mktemp -d)"
    trap 'rm -rf "$SHIM"' EXIT
    ln -s "$(command -v gdate)" "$SHIM/date"
    PATH="$SHIM:$PATH"
    # shellcheck disable=SC1090
    CLAUDE_DASHBOARD_CLI_TEST=1 source "$CLI"
    [[ "$DATE_FLAVOUR" == "gnu" ]] || fail "the gdate shim did not select the gnu branch"
    run_cases shimmed-gnu
    PATH="${PATH#"$SHIM":}"
    # shellcheck disable=SC1090
    CLAUDE_DASHBOARD_CLI_TEST=1 source "$CLI"
else
    echo "skip: no second date flavour available on this machine"
fi

# --- Install hints must not be Homebrew-only ---
# uname is shimmed so the branch a Linux user takes is exercised here too. The
# subshell is required: on a wrong value the assertions call fail, which exits.
if [[ "$(uname -s)" == "Darwin" ]]; then
    [[ "$(install_hint jq)"  == "brew install jq" ]] || fail "macOS jq hint changed: $(install_hint jq)"
    [[ "$(install_hint cli)" == *"brew install haiz/claude-dashboard/claude-dashboard-cli"* ]] \
        || fail "macOS cli hint changed: $(install_hint cli)"
fi

UNAME_SHIM="$(mktemp -d)"
printf '#!/bin/sh\necho Linux\n' > "$UNAME_SHIM/uname"
chmod +x "$UNAME_SHIM/uname"
(
    PATH="$UNAME_SHIM:$PATH"
    # shellcheck disable=SC1090
    CLAUDE_DASHBOARD_CLI_TEST=1 source "$CLI"
    [[ "$(install_hint jq)"  == *"apt install jq"* ]] || fail "linux jq hint: $(install_hint jq)"
    [[ "$(install_hint cli)" == *"install.sh"*     ]] || fail "linux cli hint: $(install_hint cli)"
)
rm -rf "$UNAME_SHIM"
echo "ok: install hints are platform-aware"

echo "PASS: cli datetime + install hints"
