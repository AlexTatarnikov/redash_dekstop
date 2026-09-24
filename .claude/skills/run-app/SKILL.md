---
name: run-app
description: Launch the Redash desktop app or verify a UI change visually. Use when asked to run, start, screenshot or check the app, or to confirm a UI change works.
---

# Running and seeing the app

Prefer headless verification. It needs no display permissions and is reproducible.

1. **See the UI**: run `cargo test --test ui`. The screenshots in `tests/snapshots/*.png`
   show the setup screen, the error state and a results table. Read the PNGs to inspect them.
   - If a snapshot differs, kittest writes `tests/snapshots/<name>.new.png` and `<name>.diff.png`.
     Read them. If the change is intended, run `UPDATE_SNAPSHOTS=1 cargo test --test ui`.
   - To see a new screen or state, add a test in `tests/ui.rs` that drives the app there
     (use `type_into`, `get_by_label(..).click()`, `wait_for`) and call `snapshot(&mut h, "name")`.
2. **Run it for real**: `cargo run -- --mock` opens a window backed by the in-process fake
   Redash (`src/mock.rs`), so no real server or saved settings are used. Run it in the
   background and stop it when you're done. Taking OS screenshots of it needs
   screen-recording permission and may not be available.
3. `cargo run` without `--mock` uses the user's real saved settings. Only do this if asked.
