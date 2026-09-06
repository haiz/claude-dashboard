#!/usr/bin/env bash
# Executable form of the five release-workflow guardrails
# (docs/superpowers/specs/2026-08-30-linux-tauri-port-design.md:231-247).
# A workflow that violates one of these broke every Homebrew install once before.
set -euo pipefail
cd "$(dirname "$0")/.."

WF=.github/workflows/release-linux.yml
fail() { echo "FAIL: $*" >&2; exit 1; }

[[ -f "$WF" ]] || fail "$WF does not exist"

# PyYAML is not guaranteed to be installed; parse when it is, skip when it is not.
if python3 -c "import yaml" 2>/dev/null; then
    python3 -c "import yaml; yaml.safe_load(open('$WF'))" || fail "$WF is not valid YAML"
else
    echo "skip: PyYAML unavailable, YAML not parsed"
fi

# The forbidden-string checks (guardrails 2-4) must not fire on the explanatory
# header comment, which spells out the guardrails and the two forbidden macOS
# artifact names on purpose so a future maintainer knows why the rules exist.
# Strip comment-only lines before grepping so the checks look at what the
# workflow DOES, not at what it documents.
STRIPPED="$(mktemp)"
trap 'rm -f "$STRIPPED"' EXIT
grep -v '^[[:space:]]*#' "$WF" > "$STRIPPED"

# 1. Published-release trigger, never a tag push.
grep -q 'types: *\[published\]' "$WF" || fail "guardrail 1: no 'types: [published]' trigger"
grep -qE '^[[:space:]]*tags:' "$WF"   && fail "guardrail 1: workflow triggers on tags"

# 2. Upload only. No action may create or replace a release.
grep -q 'gh release upload' "$STRIPPED"     || fail "guardrail 2: no 'gh release upload' step"
grep -q 'gh release create' "$STRIPPED"     && fail "guardrail 2: workflow creates a release"
grep -qi 'softprops' "$STRIPPED"            && fail "guardrail 2: workflow uses a release action"

# 3. The two macOS artifacts are release.sh's and must never be named here.
grep -q 'ClaudeDashboard.app.zip' "$STRIPPED"        && fail "guardrail 3: names the macOS app zip"
grep -qE 'claude-dashboard-cli\.tar\.gz' "$STRIPPED" && fail "guardrail 3: names the macOS CLI tarball"

# 4. release.sh writes the notes.
grep -qi 'generate_release_notes' "$STRIPPED" && fail "guardrail 4: workflow generates release notes"

# 5. CI never writes back into the repository.
grep -qE 'git (commit|push)' "$STRIPPED" && fail "guardrail 5: workflow commits or pushes"

# Constraints this plan adds on top of the guardrails.
grep -q 'ubuntu-24.04-arm' "$WF" || fail "no aarch64 runner in the matrix"
grep -q 'fail-fast: false' "$WF" || fail "one failing arch would cancel the other"
locked=$(grep -c -- '--locked' "$WF" || true)
(( locked >= 2 )) || fail "--locked appears $locked time(s); clippy and test both need it"
grep -q 'ref: ' "$WF" || fail "checkout has no explicit ref: workflow_dispatch would build the default branch"

echo "PASS: release-linux workflow satisfies all five guardrails"
