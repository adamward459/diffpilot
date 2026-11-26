---
name: feature-tests
description: Pick and run the unit, clippy, perf (time and memory), and screenshot checks for the diffpilot features and crates a change touches, using cargo xtask. Use before you report a code change as complete, when you add a crate, source file, or test, when you add a performance-sensitive code path, or after you open a PR.
---

# Feature tests

The Cargo workspace is split by crate:

| Crate (folder) | Holds |
| --- | --- |
| `core` (`diffpilot-core`) | logic: git, diff, review, viewed store, settings |
| `ui-kit` (`diffpilot-ui-kit`) | theme, icons, and shared components; has a `gallery` example |
| `gpui-app` (`diffpilot`) | the binary: app shell, sidebar, comparison. Its library holds the diff view (`src/diff_pane/`) and the review UI (`src/review/`); the `diff_pane` and `review` examples are benches |

Unit tests sit next to the code (`#[cfg(test)]`) or in a crate's `tests/` folder. They are grouped by feature, in the table in `xtask/src/features.rs`. Each feature lists its files and the tests that cover them: a module of a crate's unit tests, or one integration test file. Run `cargo xtask features` to see the table and the files under `core/src` and `gpui-app/src` that no feature lists.

| Feature | Holds |
| --- | --- |
| `diff-pane` | diff model, file kinds, anchors, and the diff view |
| `review` | review model, threads, fix runs, and the review UI |
| `sidebar` | file tree, viewed store, change list, and the sidebar |
| `comparison` | comparisons, refs, worktrees, and the comparison control |
| `settings` | settings model and the settings window |
| `claude-runs` | Claude runs, stream, stage, snapshot, guard, policy, MCP |
| `github` | the GitHub client and sync |
| `guide` | guide building and its store |
| `app-shell` | workspace, repo view, bench, the `diffpilot::testing` helpers, and the flows |
| `shared` | tests for the files no feature lists (git, paths, store, `ui-kit`) |
| `xtask` | the dev tooling |

A change to a file that a feature lists selects that feature and every feature that uses it directly. A feature uses another when one of its Rust files names one of the other's modules through `crate::`, `super::`, `diffpilot_core::` or `diffpilot::`. `cargo xtask features` prints who uses each feature. Users of users are not followed, because the core modules use each other and that would select everything. Cargo still compiles whole crates, so a broken type or API anywhere fails the build. Run `cargo xtask unit` with no names to run every feature. A change to a file that no feature lists selects every feature in the crates that build it, because anything can depend on it. A change to the root Cargo files selects every feature. `xtask/**` selects only the `xtask` feature.

## Flows

`cargo xtask flows` runs the end-to-end user flows in `gpui-app/tests/flows/`. Each flow opens a fixture repo in a headless `TestAppContext` through `diffpilot::testing` and drives the whole `RepoView` the way a user does: pick a comparison, mark a file viewed, comment, run a scripted review, change settings. They open no window, so you can run them at any time. The test helpers build only with the `testing` Cargo feature, so the app binary has no test code.

## Workflow

1. Put new code in the crate that owns the feature. Put tests next to it. When you add a file, list it in the feature that owns it in `xtask/src/features.rs`, and add its tests there.
2. When you add code on a hot path, such as scroll, resize, keystroke, launch, or large-diff code, make sure the bench of its feature measures it.
3. Before you report the work as complete, run `cargo xtask related`. It prints the commands for the features and crates your changes touch, compared with the upstream branch, including uncommitted and untracked files: `cargo xtask unit <features>`, `cargo xtask clippy <crates>`, `cargo xtask flows`, and `cargo xtask perf <features>` for the chosen features that have a bench, and `cargo xtask visual <features> --compare` for the chosen features that have scenes.
4. Run the commands that cover your change. Do not report the work as complete until they pass. If you skip a printed command, say which one and why.
5. Before you open or update a PR, run `cargo xtask baseline` and commit `baselines/bench.json` in the PR.
6. After you open or update a PR, run `cargo xtask pr-screenshots`. It captures the scenes of the features the PR touches, saves the images to `screenshots/<feature>/<scene>.png`, and pixel-diffs each one with the base branch's copy. It commits and pushes the images and puts them, grouped by feature, with the list of changed images, in a `## Screenshots

- A scene is one screenshot. It lives in the `scenes` field of a feature in `xtask/src/features.rs`: the binary to open (the app, or an example), the environment variables, and the file name. `cargo xtask visual [feature...]` builds each binary once in release mode, opens each scene on the fixture repo, finds the window of the process, captures it with `screencapture`, and saves `screenshots/<feature>/<scene>.png`. The terminal needs Screen Recording permission.
- `--compare` pixel-diffs each image with its copy on the base ref (`--base REF` to override). It prints the number of changed pixels and the largest channel change for each image that differs, and writes a red-marked diff image under `screenshot-diff/` in the Cargo build dir. A size change counts every pixel as changed.
- `cargo xtask visual --dry-run` lists the scenes and does not build or open anything.

  | Feature | Scenes |
  | --- | --- |
  | `app-shell` | the app, light and dark |
  | `diff-pane` | the `diff_pane` example: split light and dark, split without wrap, split with threads (light and dark), unified |
  | `review` | the `review` example, light and dark |
  | `shared` | the `gallery` example of `ui-kit`, light and dark |

- The binaries read these variables: `DIFFPILOT_REPO` (the fixture repo in `.fixtures/large`, built by `cargo xtask fixture`), `DIFFPILOT_VIEWED_SEED`, `DIFFPILOT_APPEARANCE` (`light` or `dark`), `DIFFPILOT_DIFF_MODE` (`unified` or `split`), and `DIFFPILOT_REVIEWER=scripted`. The `diff_pane` example also takes `DIFFPILOT_WRAP=0`, `DIFFPILOT_REVEAL=<path>` (scroll to that file), and `DIFFPILOT_DEMO_THREADS=1` (a comment on each side of the first changed line pair, in the revealed file when one is set).
- To add a scene, add a `Scene` to its feature and run `cargo xtask visual <feature>`.
- Use deterministic data, so screenshots only change when the UI changes.
- `cargo xtask visual` opens windows on your screen. Run it only when you mean to look at them.

## Perf checks (time and memory)

- The target is 120 FPS, so one frame has a budget of 8.33 ms. Code that runs on every frame must fit in that budget with room to spare.
- A bench belongs to a feature, in the `bench` field of `xtask/src/features.rs`: `diff-pane` runs the `diff_pane` example, `review` runs the `review` example, and `app-shell` runs the app binary. With `DIFFPILOT_BENCH=1`, a bench prints one JSON object of numbers on stdout. Nested objects join their names with `_`. Every metric is smaller-is-better. `baselines/bench.json` keys the benches by feature name.
- The bench binaries install the counting allocator from `gpui-app/src/bench.rs` (`#[global_allocator]`). It counts only after `Counting::enable()`, which a binary calls in bench mode, so the normal app pays one relaxed atomic load per allocation. Each scenario, such as a scroll phase, reports:
  - `<scenario>_allocs` and `<scenario>_alloc_bytes`: allocations from all threads during the scenario.
  - `<scenario>_peak_rss_bytes`: the highest resident size seen during the scenario, read from `task_info`.
  - `<scenario>_retained_bytes`: live bytes that stay after the bench opens and closes the scenario 5 times. The first cycle fills process-wide caches, so it does not count. A steady leak shows up here.
- Only metrics with a limit are compared. The limit depends on the name's suffix:

  | Suffix | Limit |
  | --- | --- |
  | `_median_ms`, `_p95_ms` | 5% worse |
  | `_allocs`, `_alloc_bytes` | 5% worse |
  | `_peak_rss_bytes` | none, reported only |
  | `_retained_bytes` | 64 KiB more |

  Maxima, over-budget counts, and interval counts swing 20-50% between runs, so they have no limit. Peak RSS is not gated either: on the same build, the split-view scroll phases of the diff pane peak at 195 MB in some runs and 237 MB in others, which is 21% apart.
- `cargo xtask baseline [feature...]` runs every bench once and writes `baselines/bench.json`: each metric's value, and the host (CPU, macOS, display refresh rate, rustc, profile). With feature names, it updates only those features when the host is the same.
- `cargo xtask perf [feature...]` runs each bench once on your branch and compares it with `baselines/bench.json` on the PR's base ref (`--base REF` to override). It fails when a metric is worse than its limit. A metric the baseline does not have counts as new.
- The diff pane and app benches draw on frame callbacks, and a window gets none while another window covers it. A bench run that takes more than 180 s fails with a message. Keep the bench window visible. `perf` and `baseline` skip, and say so, when the 1-minute load average is above half the cores, because timings on a busy machine are noise. `--force` runs them anyway.
- `perf` skips when the base ref has no baseline or its host or toolchain differs from yours. Run `cargo xtask baseline` on your machine first. Each bench runs once, so a single slow run can fail a time metric: rerun that one feature before you trust it.
- Wall-clock numbers depend on the host and the load. Allocation counts barely move between runs (under 0.1% for the scroll phases, about 1% for launch and the review stream), so they catch a hot path that starts to allocate more even when the timing noise hides it.
