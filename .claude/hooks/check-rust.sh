#!/usr/bin/env bash
# Stop: when Rust code changed since the last commit, require clippy and tests to
# pass before the agent finishes. Blocks once; if it still fails on the retry, the
# stop goes through and the user is warned instead (avoids an endless loop).
set -uo pipefail
retry=$(jq -r '.stop_hook_active // false')
source "$(dirname "$0")/env.sh"

if [ -z "$(git status --porcelain -- '*.rs' Cargo.toml Cargo.lock tests/snapshots)" ]; then
  exit 0
fi

fail() {
  if [ "$retry" = "true" ]; then
    jq -n --arg m "$1 is still failing after a fix attempt" '{systemMessage: $m}'
    exit 0
  fi
  printf '%s\n\n%s\n' "$1 failed. Fix it before finishing:" "$2" | tail -n 80 >&2
  exit 2
}

out=$(cargo clippy --all-targets --quiet -- -D warnings 2>&1) || fail "cargo clippy" "$out"
out=$(cargo test --quiet 2>&1) || fail "cargo test" "$out"
exit 0
