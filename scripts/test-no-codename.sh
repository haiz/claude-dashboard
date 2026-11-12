#!/usr/bin/env bash
# Fails if the limit-resets field's raw name appears in any tracked file.
# The name is kept base64-encoded on purpose: it is an unreleased internal codename.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
name="$(printf '%s' 'Y2VkYXJfZW1iZXI=' | base64 --decode)"
if git grep -n -I -i -F "$name" -- . ; then
  echo "FAIL: raw limit-resets field name found (see above)" >&2
  exit 1
fi
echo "OK: no raw limit-resets field name in tracked files"
