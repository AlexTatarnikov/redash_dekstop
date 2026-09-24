# Redash Desktop — agent guide

Native desktop client for [Redash](https://redash.io), written in Rust with egui/eframe.
This project is maintained by AI agents; keep this file accurate when you change
architecture, commands, or conventions.

## Commands

```sh
cargo run                    # the app, using saved settings
cargo run -- --mock          # the app against an in-process fake Redash (no real server/key needed)
cargo test                   # unit tests + headless UI tests + screenshot comparisons
cargo clippy --all-targets -- -D warnings
cargo fmt
UPDATE_SNAPSHOTS=1 cargo test   # accept intended visual changes (then review the PNG diffs)
./scripts/bundle-macos.sh    # universal Redash.app + zip/dmg in target/dist/ (ad-hoc signed)
```

The toolchain is pinned in `rust-toolchain.toml`; rustup installs it automatically.

## Definition of done

1. `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` all pass
   (a Claude Code Stop hook enforces this when Rust files changed).
2. New behaviour has a test: logic in `state.rs` tests, user flows in `tests/ui.rs`.
3. Visual changes: look at the updated `tests/snapshots/*.png` (Read the image) before
   accepting them with `UPDATE_SNAPSHOTS=1`.
4. This file is updated if commands, architecture or conventions changed.

## Architecture

Unidirectional data flow: **UI → Event → `AppState::update` → Effects → runtime → Event**.

| File | Role |
|------|------|
| `src/state.rs` | All app logic. Pure: no egui, no I/O. `update(Event) -> Vec<Effect>`. Most tests live here. |
| `src/app.rs` | Runtime (`RedashApp`). Performs effects (HTTP on background threads, settings I/O, clipboard, the native save dialog via `rfd`), feeds results back as events. |
| `src/ui/*.rs` | Drawing only, one file per screen (`setup`, `editor`, `results`). Returns the `Event` the user triggered. |
| `src/ui/theme.rs` | The visual theme, modelled on Figma's desktop UI (UI3): Inter font, palette, sizes, and helpers like `primary_button`. Light and dark follow the OS. |
| `src/sql.rs` | Pure SQL tokenizer for editor highlighting; colours (GitHub's palette) are `theme::syntax_color`. |
| `src/complete.rs` | Pure SQL autocompletion: suggestions at the cursor from the schema (aliases, `schema.`, `FROM` context). |
| `src/ui/completion.rs` | The editor's autocomplete popup: when it opens, its keys (↑/↓, Enter/Tab, Esc, Ctrl+Space), drawing. |
| `src/vars.rs` | Pure variables: `{{ name }}` references and substitution, a query result as a value (first column as SQL literals). `state.rs` runs the query variables a run needs before it. |
| `src/ui/variables.rs` | The variables panel (right side): value and query variables, edited in place. |
| `src/history.rs` | Pure execution history: each Execute records a snapshot (SQL, data source, variable definitions), newest first, last 20; rerunning the newest only updates its time. |
| `src/ui/history.rs` | The history panel (left, shown by default, collapsed with the toolbar's leftmost icon button): click an entry to restore it. |
| `src/export.rs` | Pure result export: Markdown table (current page, to the clipboard) and CSV (whole result). |
| `src/api.rs` | Blocking Redash REST client (`ureq`). |
| `src/config.rs` | `Config` (host + API key) and `ConfigStore` (file, or in-memory for tests); also saves variables to `variables.json` and history to `history.json` next to the config. |
| `src/mock.rs` | In-process fake Redash used by tests and `--mock`. Its doc comment lists the canned behaviour; `queries()` returns the SQL it was sent. |
| `src/main.rs` | Entry point; parses `--mock`. |
| `tests/ui.rs` | Headless end-to-end tests with `egui_kittest` + snapshots in `tests/snapshots/`. |
| `.github/workflows/ci.yml` | CI on macOS: fmt, clippy, tests; then (not on PRs) `scripts/bundle-macos.sh`, uploaded as an artifact. A `v*` tag matching the `Cargo.toml` version publishes a GitHub Release. |

Adding a feature usually means: new `Event`/`Effect` variants and a transition in
`state.rs` (+ tests), perform any new effect in `app.rs`, draw it in `ui/`, cover the
flow in `tests/ui.rs`.

## Redash API notes

- Auth header: `Authorization: Key <user API key>` (query API keys can't run ad-hoc SQL).
- `GET /api/data_sources` — also used as the credentials check.
- `POST /api/query_results {data_source_id, query, max_age: 0, parameters: {}}` returns
  either `{query_result}` or `{job}`; poll `GET /api/jobs/{id}` (status 3 = success,
  4 = failure, 5 = cancelled), then `GET /api/query_results/{query_result_id}`.
- `GET /api/data_sources/{id}/schema` returns `{schema: [{name, columns}]}` from cache, or
  `{job}` whose finished `result` is that list; `{error: {code: 1}}` means the source
  can't list its schema (treated as empty). Columns are bare names or `{name, type}`.

## Gotchas

- **egui/eframe 0.36 differs from what most training data shows**:
  `eframe::App` has `fn ui(&mut self, ui: &mut Ui, frame)` (not `update(ctx, …)`);
  panels are `egui::Panel::top(id).show(ui, …)` / `CentralPanel::default_margins().show(ui, …)`;
  style is `ctx.global_style()`; font measuring is `ctx.fonts_mut(|f| …)`.
  When unsure, read the source in `~/.cargo/registry/src/*/egui-0.36.*/` rather than guessing.
- The default egui font lacks many symbols (`→`, `⟳` render as ☐). Stick to ASCII or
  glyphs already used in the UI (`▶`, `…`), and check the snapshot.
- Styling goes through `ui/theme.rs`: no hard-coded colours or font sizes in screens. Use
  `theme::primary_button` for a screen's main action and `theme::bar_frame` for bars.
  Icon-only buttons are painted, not glyphs: see `theme::sidebar_toggle` (tests find it by its label).
  Use `ui.button` (not `small_button`) so controls in a row share the 24px height.
  Fonts are embedded from `assets/fonts/` (Inter, SIL Open Font License; keep `Inter-LICENSE.txt`).
  Check both `editor_results.png` (dark) and `editor_results_light.png` after visual changes.
- Widgets must be findable by tests: give text inputs an accessible label with
  `.labelled_by(label.id)`; buttons are found by their text. A `ComboBox` exposes its
  selected text as its *value*, not its label: `h.get_by(|n| n.value().as_deref() == Some("…"))`.
- Snapshots must be deterministic: `tests/ui.rs::snapshot` hides the cursor and masks the
  mock's random `http://127.0.0.1:<port>`. Mask anything else that varies per run.
- Never touch the real settings file in tests; use `ConfigStore::memory`.
- `{{ name }}` in SQL is expanded client-side from the user's variables before sending;
  an unknown name is an error. UI tests that run `SAMPLE_SQL` need `tests/ui.rs::store`,
  which saves the variables it uses.
- Never open the native save dialog in tests; use `RedashApp::with_save_dialog` to return a temp path.
  Clipboard copies show up in `h.output().platform_output.commands` as `OutputCommand::CopyText`.
- Errors are shown in the UI, never panics: `unwrap`/`expect`/`panic!` are linted outside tests.

## Manual run (optional)

`cargo run -- --mock` opens a window with fake data. Prefer the headless UI tests for
verification: they need no screen-recording or accessibility permissions and give you
PNGs to inspect.
