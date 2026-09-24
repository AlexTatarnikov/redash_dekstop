#!/usr/bin/env bash
# PostToolUse(Edit|Write): run rustfmt after a Rust file is edited.
set -uo pipefail
file=$(jq -r '.tool_response.filePath // .tool_input.file_path // empty')
[[ "$file" == *.rs ]] || exit 0
source "$(dirname "$0")/env.sh"
cargo fmt --quiet
