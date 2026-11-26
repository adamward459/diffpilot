# diffpilot

Rust app built with GPUI. The Cargo workspace has three app crates: `core` (logic), `ui-kit` (theme, icons, shared components), and `gpui-app` (the app: shell, sidebar, comparison, the diff pane in `src/diff_pane/`, and the review UI in `src/review/`). `xtask` holds the dev tooling, run as `cargo xtask`.

- Run `git config core.hooksPath .githooks` once per clone. The pre-commit hook runs `cargo xtask precommit`: it formats all Rust code and blocks `env!("CARGO_BIN_EXE_…")`, which bakes in another worktree's path.
- When a change adds, changes, removes, or fixes something users can see, add a line under `## [Unreleased]` in `CHANGELOG.md`, in the same commit. Use the `### Added`, `### Changed`, `### Removed`, or `### Fixed` heading. Write it for users, one change per line, 225 characters max. Refactors, tests, and dev tooling need no line.
- All worktrees share one Cargo build dir, `~/.cargo/build/diffpilot` (set in `.cargo/config.toml`). Dependencies build once for every worktree, so you do not need your own `CARGO_TARGET_DIR`.
- Before you report a code change as complete, run `cargo xtask related` and the commands it prints (`cargo xtask unit <feature...>`, `cargo xtask clippy <crate...>`, `cargo xtask perf <feature...>`). `unit` picks tests by feature, not by crate. A feature or crate that already passed with the same inputs is skipped. `perf` opens windows, so keep them visible and run it on a quiet machine.
- Before you open or update a PR, run `cargo xtask baseline` and commit `baselines/bench.json`. After you open it, run `cargo xtask pr-screenshots`; it commits `screenshots/<feature>/` and lists the images that changed. The merged PR then holds the baseline for the next PR. See `.claude/skills/feature-tests`.
