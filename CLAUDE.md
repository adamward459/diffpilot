# diffpilot

Rust app built with GPUI. The Cargo workspace has three crates: `core` (logic), `ui-kit` (theme, icons, shared components), and `gpui-app` (the app: shell, sidebar, comparison, the diff pane in `src/diff_pane/`, and the review UI in `src/review/`).

- Run `git config core.hooksPath .githooks` once per clone. The pre-commit hook formats all Rust (`cargo fmt`) and Python (ruff) code with `scripts/format.py`.
- All worktrees share one Cargo build dir, `~/.cargo/build/diffpilot` (set in `.cargo/config.toml`). Dependencies build once for every worktree, so you do not need your own `CARGO_TARGET_DIR`.
- Before you report a code change as complete, run `scripts/test.py related` and the commands it prints. A crate that already passed with the same inputs is skipped.
- Before you open or update a PR, run `scripts/test.py baseline` and commit `baselines/bench.json`. After you open it, run `scripts/pr_screenshots.py`; it commits `screenshots/` and lists the images that changed. The merged PR then holds the baseline for the next PR. See `.claude/skills/feature-tests`.
