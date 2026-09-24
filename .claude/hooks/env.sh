# Sourced by the hook scripts. Prefer the standard rustup install (~/.cargo) even
# when the parent environment was set up by another Rust version manager.
if [ -x "$HOME/.cargo/bin/rustup" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
  unset CARGO_HOME RUSTUP_HOME
fi
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"
