#!/usr/bin/env bash
# Contract test for the bash CLI's usage parsing: the Fable window is derived
# from limits[] per contract/cases/usage-decoding.json, the same cases the
# Swift and Rust implementations read. Run from anywhere; no network, no helper.
set -euo pipefail
cd "$(dirname "$0")/.."

CLI=cli/claude-dashboard-cli
CASES=contract/cases/usage-decoding.json

fail() { echo "FAIL: $*" >&2; exit 1; }

# The API removed seven_day_sonnet; the contract says it is ignored, not decoded.
if grep -q "seven_day_sonnet" "$CLI"; then
    fail "CLI still references the removed seven_day_sonnet field"
fi

# Sourcing the CLI must not launch the dashboard: it needs the test guard.
if ! grep -q 'CLAUDE_DASHBOARD_CLI_TEST' "$CLI"; then
    fail "CLI has no CLAUDE_DASHBOARD_CLI_TEST source guard"
fi

# shellcheck disable=SC1090
CLAUDE_DASHBOARD_CLI_TEST=1 source "$CLI"
declare -f fable_from_usage_json >/dev/null || fail "fable_from_usage_json is not defined"

n=$(jq 'length' "$CASES")
for ((i = 0; i < n; i++)); do
    name=$(jq -r ".[$i].name" "$CASES")
    input=$(jq -c ".[$i].input" "$CASES")
    want_present=$(jq -r ".[$i].expect.fable_present" "$CASES")

    got=$(fable_from_usage_json "$input")

    if [[ "$want_present" == "false" ]]; then
        [[ -z "$got" ]] || fail "$name: expected no fable window, got: $got"
    else
        [[ -n "$got" ]] || fail "$name: expected a fable window, got none"
        want_pct=$(jq -r ".[$i].expect.fable_utilization" "$CASES")
        got_pct="${got%%$'\t'*}"
        awk -v a="$got_pct" -v b="$want_pct" 'BEGIN { exit (a + 0 == b + 0) ? 0 : 1 }' \
            || fail "$name: percent $got_pct != $want_pct"
        # The raw resets_at string must be the Fable entry's own, untouched.
        want_reset=$(jq -r ".[$i].input.limits[] | select(.scope.model.display_name? == \"Fable\") | .resets_at // \"\"" "$CASES")
        got_reset="${got#*$'\t'}"
        [[ "$got_reset" == "$want_reset" ]] || fail "$name: resets_at '$got_reset' != '$want_reset'"
    fi
    echo "ok: $name"
done

echo "PASS: all $n usage-decoding cases"
