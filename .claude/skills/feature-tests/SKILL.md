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

Unit tests sit next to the code (`#[cfg(test)]`) or in a crate's `tests/` folder. A change to a crate's `src/`, `assets/`, `Cargo.toml`, or `build.rs` also touches the crates that depend on it. A change to its `tests/`, `examples/`, or `benches/` touches only that crate. A change to `tests/` alone runs no hot path checks or screenshots. A change to the root `Cargo.toml`, `Cargo.lock`, or `rustfmt.toml` touches all of them.

`scripts/test.py lint` fails when the bench or screenshot tables in the script name a crate that is not in the workspace, or when ruff finds a problem in the scripts.

## Workflow

1. Put new code in the crate that owns the feature. Put tests next to it.
2. When you add code on a hot path, such as scroll, resize, keystroke, launch, or large-diff code, make sure the crate's `DIFFPILOT_BENCH=1` mode measures it.
3. Before you report the work as complete, run `scripts/test.py related`. It prints the commands for the crates your changes touch, compared with the upstream branch, including uncommitted and untracked files: `scripts/test.py unit <crates>`, `scripts/test.py clippy <crates>`, and `scripts/test.py hotpath <crate>`.
4. Run the commands that cover your change. Do not report the work as complete until they pass. If you skip a printed command, say which one and why.
5. Before you open or update a PR, run `scripts/test.py baseline` and commit `baselines/bench.json` in the PR.
6. After you open or update a PR, run `scripts/pr_screenshots.py`. It builds the app, opens it on the fixture repo in light and dark mode, captures the window, saves the images to `screenshots/feature/<crate>/`, and pixel-diffs each one with the base branch's copy. It commits and pushes the images and puts them, with the list of changed images, in a `## Screenshots` section of the PR body. Look at each changed image before you report the PR as ready. The terminal needs Screen Recording permission.

The merged PR leaves `baselines/` and `screenshots/` on the base branch, so they are the baseline for the next PR.

`scripts/test.py unit`, `clippy`, and `screenshot` run the same checks for all crates, or for the crates you name.

## Speed

- All worktrees share one Cargo build dir, `~/.cargo/build/diffpilot`, set in `.cargo/config.toml`. Dependencies build once. A setting of `CARGO_TARGET_DIR` only moves the final binaries. `rust-toolchain.toml` pins the nightly, because the shared build dir and its checksum freshness need every worktree on the same toolchain.
- `unit` runs `cargo test` once for all the crates it needs, so Cargo builds them in parallel. It holds a lock for the build and the run, so tests in two worktrees wait for each other instead of replacing each other's test binaries.
- `unit` and `clippy` skip a crate that already passed with the same inputs and print `skipped (passed at <hash>)`. The inputs are the crate's files, the `src/`, `assets/`, `Cargo.toml`, and `build.rs` of the workspace crates it depends on, the root Cargo files, `.cargo/config.toml`, the toolchain, and the features. Records live in `~/Library/Caches/diffpilot/passes`. Use `--force` to run again.

## Screenshots

- The app reads these variables: `DIFFPILOT_REPO` (the fixture repo in `.fixtures/large`, built by `scripts/fixture_repo.py`), `DIFFPILOT_VIEWED_SEED`, `DIFFPILOT_APPEARANCE` (`light` or `dark`), `DIFFPILOT_DIFF_MODE` (`unified` or `split`), and `DIFFPILOT_REVIEWER=scripted`. The diff-pane example also takes `DIFFPILOT_WRAP=0`, `DIFFPILOT_REVEAL=<path>` (scroll to that file), and `DIFFPILOT_DEMO_THREADS=1` (a comment on each side of the first changed line pair, in the revealed file when one is set).
- Use deterministic data, so screenshots only change when the UI changes.

## Hot path checks

- The target is 120 FPS, so one frame has a budget of 8.33 ms. Code that runs on every frame must fit in that budget with room to spare.
- A crate with a bench mode prints one JSON object of numeric metrics on stdout when `DIFFPILOT_BENCH=1`. Nested objects become dotted names. Every metric is smaller-is-better. Benches: `cargo run --release -p diffpilot-diff-pane --example diff_pane`, `cargo run --release -p diffpilot-review-ui --example review`, and `cargo run --release -p diffpilot`.
- `scripts/test.py baseline` runs every bench 5 times (`--rounds N`) and writes `baselines/bench.json`: the median, p95, and run count of each metric, and the host (CPU, macOS, display refresh rate, rustc, profile). `baseline <crate>` updates only that crate's part when the host is the same.
- `scripts/test.py hotpath` runs each bench 3 times on your branch and compares the medians with `baselines/bench.json` on the PR's base ref (`--base REF` to override). It fails when a metric is more than 5% worse. A metric the baseline does not have counts as new.
- The benches draw on frame callbacks, and a window gets none while another window covers it. A bench run that takes more than 180 s fails with a message. Keep the bench window visible, and run benches on a quiet machine: `baseline` and `hotpath` warn when the load average is above half the cores.
- When the base ref has no baseline for a crate, or its host or toolchain differs from yours, `hotpath` builds the base in a second worktree and runs both sides in 3 alternating rounds, and says why. It also does that to confirm a regression found against the committed baseline.
- Wall-clock numbers depend on the host. Prefer deterministic counts, such as layout passes or diff operations, and set time budgets well above the noise.
