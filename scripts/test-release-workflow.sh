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

# Almost every assertion below reads a comment-stripped copy, not the raw
# file. Two separate reasons, not one:
#   - The explanatory header comment spells out both forbidden macOS artifact
#     names and the word "--locked" on purpose, so a raw-file grep for those
#     strings fails against a correct workflow.
#   - A POSITIVE raw-file check (must-contain) can be satisfied by a comment
#     alone: commenting out the real `types: [published]` trigger line while
#     leaving that same text sitting dead in a comment still makes a raw-file
#     `grep -q 'types: *\[published\]'` pass, even though the workflow's live
#     trigger has silently become something else (e.g. a branch push).
#     Stripping comments first closes that hole for every must-contain check,
#     not just the must-not-contain ones.
# The lone exception is the guardrail-1 "no tags:" check just below: it is a
# must-NOT-contain check on a key that, live or commented, never starts a
# line with only leading whitespace before "tags:" in a comment (comments
# start with "#"), so raw vs. stripped makes no difference there and the
# original form is kept.
STRIPPED="$(mktemp)"
trap 'rm -f "$STRIPPED"' EXIT
grep -v '^[[:space:]]*#' "$WF" > "$STRIPPED"

# 1. Published-release trigger, never a tag push.
grep -q 'types: *\[published\]' "$STRIPPED" || fail "guardrail 1: no live 'types: [published]' trigger"
grep -qE '^[[:space:]]*tags:' "$WF"          && fail "guardrail 1: workflow triggers on tags"

# 2. Upload only. No action may create or replace a release.
#    Positive assertions carry the weight here. A blacklist of literal
#    strings ("gh release create", "softprops", ...) is a finite list an
#    adversarial edit can walk around entirely: `gh api -X DELETE .../assets/N`
#    plus `gh api -X PATCH .../releases/N -f body=...` deletes assets and
#    rewrites notes — the exact cab82a9 outcome — while matching none of
#    them. So assert what the workflow IS allowed to do, not just what it
#    must not spell.
gh_release_lines=$(grep -c 'gh release' "$STRIPPED" || true)
(( gh_release_lines == 1 )) || fail "guardrail 2: expected exactly one 'gh release' line, found $gh_release_lines"
grep -q 'gh release upload' "$STRIPPED" || fail "guardrail 2: the one 'gh release' line is not 'gh release upload'"
grep -q 'gh api' "$STRIPPED" && fail "guardrail 2: workflow calls 'gh api' directly (can delete assets / rewrite notes, bypassing the upload-only rule)"

total_uses=$(grep -c 'uses:' "$STRIPPED" || true)
checkout_uses=$(grep -c 'uses: actions/checkout@' "$STRIPPED" || true)
(( total_uses > 0 )) || fail "guardrail 2: no 'uses:' step found (expected actions/checkout)"
(( total_uses == checkout_uses )) || fail "guardrail 2: a 'uses:' step other than actions/checkout@ is present (could be a release-creating action)"

# Belt-and-braces: keep the original literal blacklist too, extended to catch
# the specific evasions found in review (a release action other than
# softprops is already covered by the positive check above; these two cover
# case/separator variants of the forbidden literals themselves).
grep -q 'gh release create' "$STRIPPED" && fail "guardrail 2: workflow creates a release"
grep -qi 'softprops' "$STRIPPED"        && fail "guardrail 2: workflow uses a release action"

# 3. The two macOS artifacts are release.sh's and must never be named here,
#    and the Linux upload path must be the exact per-arch name — no wildcard
#    suffix that could glob onto a stale or unrelated asset.
grep -q 'ClaudeDashboard.app.zip' "$STRIPPED"        && fail "guardrail 3: names the macOS app zip"
grep -qE 'claude-dashboard-cli\.tar\.gz' "$STRIPPED" && fail "guardrail 3: names the macOS CLI tarball"
grep -qF '.build/claude-dashboard-cli-linux-${{ matrix.arch }}.tar.gz' "$STRIPPED" \
    || fail "guardrail 3: upload step does not reference the exact per-arch asset path"

# 4. release.sh writes the notes. Match snake_case and camelCase alike
#    (grep -i alone does not fold "generateReleaseNotes" onto
#    "generate_release_notes" — the underscores differ, not just the case).
grep -qiE 'generate_?release_?notes' "$STRIPPED" && fail "guardrail 4: workflow generates release notes"

# 5. CI never writes back into the repository. Tolerate extra flags between
#    "git" and the subcommand (git -C . commit) and multiple spaces
#    (git  push), which a single required space would miss.
grep -qE 'git[[:space:]].*(commit|push)' "$STRIPPED" && fail "guardrail 5: workflow commits or pushes"

# Constraints this plan adds on top of the guardrails.
grep -q 'ubuntu-24.04-arm' "$STRIPPED" || fail "no aarch64 runner in the matrix"
grep -q 'fail-fast: false' "$STRIPPED" || fail "one failing arch would cancel the other"
locked=$(grep -c -- '--locked' "$STRIPPED" || true)
(( locked >= 2 )) || fail "--locked appears $locked time(s); clippy and test both need it"
# Exact match, not mere presence: a checkout pinned to some other ref (or to
# nothing) would satisfy a bare "ref: " substring search.
grep -qF 'ref: ${{ steps.resolve.outputs.tag }}' "$STRIPPED" \
    || fail "checkout ref is not pinned to the resolved release tag"

echo "PASS: release-linux workflow satisfies all five guardrails"
