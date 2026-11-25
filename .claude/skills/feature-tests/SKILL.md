---
name: feature-tests
description: Pick and run the unit, clippy, hot path, and screenshot checks for the diffpilot crates a change touches, using scripts/test.py. Use before you report a code change as complete, when you add a crate, source file, or test, when you add a performance-sensitive code path, or after you open a PR.
---

# Feature tests

The Cargo workspace is split by crate:

| Crate (folder) | Holds |
| --- | --- |
| `core` (`diffpilot-core`) | logic: git, diff, review, viewed store, settings |
| `ui-kit` (`diffpilot-ui-kit`) | theme, icons, and shared components; has a `gallery` example |
| `diff-pane` (`diffpilot-diff-pane`) | the diff view; has a bench example |
| `review-ui` (`diffpilot-review-ui`) | review UI components |
| `gpui-app` (`diffpilot`) | app shell, sidebar, comparison; the binary |

Unit tests sit next to the code (`#[cfg(test)]`) or in a crate's `tests/` folder. A change in a crate also touches the crates that depend on it. A change to `Cargo.toml`, `Cargo.lock`, or `rustfmt.toml` touches all of them.

`scripts/test.py lint` fails when the bench or screenshot tables in the script name a crate that is not in the workspace, or when ruff finds a problem in the scripts.

## Workflow

1. Put new code in the crate that owns the feature. Put tests next to it.
2. When you add code on a hot path, such as scroll, resize, keystroke, launch, or large-diff code, make sure the crate's `DIFFPILOT_BENCH=1` mode measures it.
3. Before you report the work as complete, run `scripts/test.py related`. It prints the commands for the crates your changes touch, compared with the upstream branch, including uncommitted and untracked files: `cargo test -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings`, and `scripts/test.py hotpath <crate>`.
4. Run the commands that cover your change. Do not report the work as complete until they pass. If you skip a printed command, say which one and why.
5. After you open or update a PR, run `scripts/pr_screenshots.py`. It builds the release app, opens it on the fixture repo in light and dark mode, captures the window, saves the images to `screenshots/feature/<crate>/`, commits and pushes them, and puts them in a `## Screenshots` section of the PR body. Look at each image before you report the PR as ready. The terminal needs Screen Recording permission.

`scripts/test.py unit`, `clippy`, and `screenshot` run the same checks for all crates, or for the crates you name.

## Screenshots

- The app reads these variables: `DIFFPILOT_REPO` (the fixture repo in `.fixtures/large`, built by `scripts/fixture_repo.py`), `DIFFPILOT_VIEWED_SEED`, `DIFFPILOT_APPEARANCE` (`light` or `dark`), and `DIFFPILOT_REVIEWER=scripted`.
- Use deterministic data, so screenshots only change when the UI changes.

## Hot path checks

- The target is 120 FPS, so one frame has a budget of 8.33 ms. Code that runs on every frame must fit in that budget with room to spare.
- A crate with a bench mode prints one JSON object of numeric metrics on stdout when `DIFFPILOT_BENCH=1`. Nested objects become dotted names. Every metric is smaller-is-better. Benches: `cargo run --release -p diffpilot-diff-pane --example diff_pane` and `cargo run --release -p diffpilot`.
- `scripts/test.py hotpath` builds and runs each bench on your branch and on the PR's base ref (`--base REF` to override), in 3 alternating rounds per side. It fails when the median of any metric is more than 5% worse than the base. A metric the base does not report counts as new.
- Wall-clock numbers depend on the host. Prefer deterministic counts, such as layout passes or diff operations, and set time budgets well above the noise.
