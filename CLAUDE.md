# diffpilot

- Run `git config core.hooksPath .githooks` once per clone. The pre-commit hook formats all Swift and Python code (`scripts/format.py`).
- Before you report a code change as complete, run `scripts/test.py related` and the test commands it prints. After you open a PR, run `scripts/pr_screenshots.py`. See `.claude/skills/feature-tests`.
