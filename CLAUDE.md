# diffpilot

Rust app built with GPUI. The Cargo workspace has five crates: `core` (logic), `ui-kit` (theme, icons, shared components), `diff-pane`, `review-ui`, and `gpui-app` (app shell, sidebar, comparison).

- Run `git config core.hooksPath .githooks` once per clone. The pre-commit hook formats all Rust (`cargo fmt`) and Python (ruff) code with `scripts/format.py`.
- Before you report a code change as complete, run `scripts/test.py related` and the commands it prints. After you open a PR, run `scripts/pr_screenshots.py`. See `.claude/skills/feature-tests`.
