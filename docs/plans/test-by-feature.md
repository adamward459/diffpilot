# Plan: test tiers by feature, in a cargo xtask

Goal: pick and run checks by feature, not by crate. Since the merge, `gpui-app` holds almost every feature, so "the crates you touched" now means "everything". At the same time, move the test tooling from Python to a `cargo xtask` crate, so the repo has one language and the feature table is typed.

Status (2026-11-26): done as local commits on `test-by-feature`, not as PRs. PR1 is 45d1da5, PR2 is aaacb70 and ed2ea6a, PR5 is 2692743 (plus `cargo xtask flows` in ed2ea6a), PR3 is ffac2ff, PR4 follows (visual and pr-screenshots in xtask, no Python left; `scripts/install.sh` moved to `install.sh` at the repo root). Two decisions changed the plan while it ran:

- Selection follows usage. A change selects the owning feature and every feature whose Rust files name one of its modules through a workspace crate root. Only direct users count: the core modules use each other, so following users of users selects every feature.
- Perf runs each bench once. `perf` and `baseline` skip when the 1-minute load average is above half the cores (`--force` runs anyway). There is no base-worktree build and no confirming second run. The diff pane bench scrolls 240 frames per sweep and 120 per flick. The PR3 section below still describes the earlier multi-round design.

Tiers after this plan:

| Tier | Grouped by | Command |
| --- | --- | --- |
| unit | feature | `cargo xtask unit [feature...]` |
| flows (e2e) | user flow | `cargo xtask flows` |
| perf (hot path + memory) | feature | `cargo xtask perf [feature...]`, `cargo xtask baseline` (one run each) |
| visual | feature | `cargo xtask visual [feature...]`, `cargo xtask pr-screenshots` |
| clippy | crate | `cargo xtask clippy [crate...]` |

`lint` goes away. After PR4, `scripts/`, `ruff.toml` and the Python pre-commit hook are gone.

## Landing order and dependencies

```
PR1 xtask + hook ──> PR2 FEATURES + unit/clippy/related ──┬──> PR3 perf ──> PR4 visual (deletes scripts/)
                                                          └──> PR5 flows
```

PR1 to PR4 land in order. PR5 only needs PR2.

Between PR2 and PR4, `cargo xtask` runs unit, clippy and related, and `scripts/test.py` still runs hotpath, baseline and screenshot. Each command lives in exactly one tool at any time, and each PR deletes the Python it replaces.

| PR | May touch |
| --- | --- |
| PR1 | `xtask/**`, root `Cargo.toml`, `.cargo/config.toml`, `.githooks/pre-commit`, delete `scripts/format.py` and `scripts/fixture_repo.py`, `scripts/test.py` (drop `lint`, call `cargo xtask fixture`), `CLAUDE.md`, skill |
| PR2 | `xtask/**`, `scripts/test.py` (delete unit, clippy, related), `CLAUDE.md`, skill |
| PR3 | `xtask/**`, `scripts/test.py` (delete hotpath, baseline), `gpui-app/src/bench.rs`, `gpui-app/examples/*.rs`, `gpui-app/src/main.rs`, `baselines/bench.json`, skill |
| PR4 | `xtask/**`, delete `scripts/` and `ruff.toml`, `gpui-app/src/main.rs` (scene env vars), `screenshots/`, `CLAUDE.md`, skill |
| PR5 | `xtask/**`, `gpui-app/src/repo_view.rs` (move test helper), `gpui-app/src/testing.rs`, `gpui-app/tests/flows.rs`, `gpui-app/tests/flows/**`, skill |

PR3 and PR4 open windows, so their live boxes need the operator's OK before they run.

## PR1: xtask crate, Rust pre-commit hook, drop `lint`

- [ ] Depends on: nothing
- [ ] Change: add an `xtask` workspace member (a binary, `publish = false`) and the alias `xtask = "run -q -p xtask --"` in `.cargo/config.toml`. Parse arguments with `clap`.
- [ ] Change: `cargo xtask fmt` runs `cargo fmt --all`. `cargo xtask precommit` runs fmt, re-adds the staged files, and fails on any `env!("CARGO_BIN_EXE_` in a `.rs` file. That check guards the bug fixed in 4594b82, where the shared build dir baked in another worktree's path.
- [ ] Change: `.githooks/pre-commit` becomes a two-line shell script that runs `cargo xtask precommit`. It adds `$HOME/.cargo/bin` to `PATH` first, because a GUI git client or a bare shell may not have cargo on PATH.
- [ ] Change: port `scripts/fixture_repo.py` to `cargo xtask fixture`. The fixture must be byte-identical: same commits, same file contents, same viewed seed.
- [ ] Change: delete `lint` from `scripts/test.py`, and make its `ensure_fixture` call `cargo xtask fixture`. Delete `scripts/format.py` and `scripts/fixture_repo.py`.
- [ ] Result: commits format Rust through xtask. `scripts/test.py lint` prints usage.
- [ ] Verify (unit): xtask tests for the `env!` scan (a hit, a miss, a hit in a comment). Stage a file with `env!("CARGO_BIN_EXE_x")`; the hook fails. Remove it; the hook passes.
- [ ] Verify (equivalence): build the fixture with the old Python script and with `cargo xtask fixture` into two folders. `git rev-parse HEAD` and the viewed seed JSON must match.
- [ ] Verify (live): n/a, no app change.
- [ ] Verify (perf): n/a.

## PR2: the FEATURES table, unit, clippy, related

- [ ] Depends on: PR1
- [ ] Change: one table in `xtask/src/features.rs`. Bench and scenes start empty and PR3 and PR4 fill them:

  ```rust
  pub struct Feature {
      pub name: &'static str,
      pub paths: &'static [&'static str],
      pub tests: &'static [TestSel],
      pub bench: Option<Bench>,
      pub scenes: &'static [Scene],
  }

  pub enum TestSel {
      Lib { package: &'static str, module: &'static str },
      Target { package: &'static str, test: &'static str },
  }
  ```

- [ ] Draft map. Check each entry against the `use` graph before you commit it:

  | Feature | Paths |
  | --- | --- |
  | diff-pane | `core/src/{diff,file_kind,anchor}.rs`, `gpui-app/src/diff_pane/**`, `gpui-app/examples/diff_pane.rs`, `gpui-app/tests/diff_pane_*.rs` |
  | review | `core/src/{review,threads,fix}.rs`, `core/src/*review*.md`, `core/src/reply_thread.md`, `gpui-app/src/review/**`, `gpui-app/examples/review.rs`, `gpui-app/tests/review_*.rs` |
  | sidebar | `core/src/{tree,viewed,changes}.rs`, `gpui-app/src/sidebar*.rs` |
  | comparison | `core/src/{comparison,refs,worktree}.rs`, `gpui-app/src/comparison_*.rs` |
  | settings | `core/src/settings.rs`, `gpui-app/src/settings/**` |
  | claude-runs | `core/src/{claude,run,runner,stream,stage,snapshot,guard,policy,prompt,connection}.rs`, `core/src/mcp/**`, `core/src/bin/**`, `core/src/make_changes.md`, `core/src/fix_comments.md`, `core/tests/{runs,mcp,mcp_stdio,guard_hook,e2e,smoke}.rs` |
  | github | `core/src/github/**`, `core/src/sync.rs`, `core/tests/fixtures/github/**` |
  | guide | `core/src/guide/**`, `core/src/store/guides.rs`, `core/tests/guide.rs` |
  | app-shell | `gpui-app/src/{main,lib,workspace,repo_view,pane,format,bench}.rs` |

- [ ] Change: a changed file that no feature lists (for example `core/src/git.rs`, `core/src/store/mod.rs`, `ui-kit/**`, root Cargo files) selects every feature in the crates that depend on it. The selection can only run less than today's per-crate rule when every changed file belongs to a feature. `xtask/**` selects only xtask's own tests.
- [ ] Change: port `unit`, `clippy` and `related`, with the pass-record cache and the build-dir lock (`fs2` or `libc::flock`). Pass records key on feature for `unit` and on crate for `clippy`. The feature hash covers the feature's paths, every unlisted file in its crate and its dependency crates, the root files, and the toolchain.
- [ ] Change: `unit` runs one `cargo test` call with every chosen filter, so Cargo still builds in parallel.
- [ ] Change: `cargo xtask features` prints the table and lists any file under `core/src` or `gpui-app/src` that no feature lists.
- [ ] Change: delete `unit`, `clippy`, `related` and their helpers from `scripts/test.py`. Update `CLAUDE.md` and the skill to name `cargo xtask related`.
- [ ] Result: after a change to `core/src/threads.rs`, `cargo xtask related` prints `cargo xtask unit review`.
- [ ] Verify (unit): xtask tests feed 10 file sets to the selection. A golden file holds the crate set the old Python `touched_crates` picked for each one. Generate it once, before this PR deletes that code. Every crate the new selection runs tests in must be in the old set. When an unlisted file changes, the two sets must be equal.
- [ ] Verify (live): `cargo xtask unit review` runs the review tests and no others. Count the test names in the output.
- [ ] Verify (perf): wall time of `unit` after a one-line change in `core/src/threads.rs`, old tool against new tool, warm build both times. Measure it, do not assume it.

## PR3: perf = hot path + memory

- [ ] Depends on: PR2
- [ ] Change: port `hotpath` as `perf`, and port `baseline`, including the base worktree build and the alternating rounds. Bench names follow features: `diff-pane`, `review` (was `review-ui`), `app-shell` (was `gpui-app`). Regenerate `baselines/bench.json` in this PR.
- [ ] Change: a counting `#[global_allocator]` in `gpui-app/src/bench.rs`, installed only by the bench binaries. Each scenario reports:
  - `<scenario>_allocs` and `<scenario>_alloc_bytes`: the same on every run, so the 5% limit works
  - `<scenario>_peak_rss_bytes`: from `task_info`, changes between runs, so it gets a 15% limit
  - `<scenario>_retained_bytes`: live bytes after opening and closing the scenario N times; the limit is a fixed number of bytes, not a percent
- [ ] Change: the stable-metric rule becomes a table from suffix to limit, instead of one tolerance.
- [ ] Change: delete `hotpath`, `baseline` and their helpers from `scripts/test.py`.
- [ ] Result: `cargo xtask perf review` prints time and memory rows for the review bench.
- [ ] Verify (unit): add a deliberate `Vec::with_capacity(1 << 20)` in one scroll frame; `perf diff-pane` fails on `_alloc_bytes`. Remove it; it passes.
- [ ] Verify (live): needs the operator's OK. Run `cargo xtask baseline` twice on a quiet machine. The allocation counts must be equal, and RSS must stay within its limit.
- [ ] Verify (perf): the allocator must not move the timing metrics. Compare `scroll_flick_*_median_ms` with and without it; the change must stay under 2%.

## PR4: visual per feature, and delete the Python

- [ ] Depends on: PR3
- [ ] Change: port `screenshot` as `visual`, and port `scripts/pr_screenshots.py` as `cargo xtask pr-screenshots`. Capture the window with `screencapture` as today. Diff the pixels with the `image` crate instead of `sips`. Images go to `screenshots/<feature>/<scene>.png`. Delete the old `screenshots/feature/` folders in the same PR.
- [ ] Change: scenes live in the FEATURES table. Start with what the app can show today:
  - app-shell: the app, light and dark
  - diff-pane: the `diff_pane` example, split and unified, wrap and no wrap, with threads (the scenes already in `screenshots/feature/gpui-app/pane-*`)
  - review: the `review` example, light and dark
  - settings, sidebar, comparison: add a `DIFFPILOT_OPEN=<surface>` env var to `main.rs` only if a scene needs a state that the app does not show on launch
- [ ] Change: the PR section groups images by feature.
- [ ] Change: delete `scripts/` and `ruff.toml`. Update `CLAUDE.md`: no Python remains, and the hook runs `cargo xtask precommit`.
- [ ] Result: changing `gpui-app/src/review/card.rs` captures only the review scenes. `rg --files -g '*.py'` finds nothing.
- [ ] Verify (unit): xtask tests for the pixel diff (same image, one changed pixel, a size change). `visual --dry-run` lists the scenes for 3 file sets.
- [ ] Verify (equivalence): diff one old and one new screenshot pair with the Python diff and the Rust diff. The changed-pixel counts must match.
- [ ] Verify (live): needs the operator's OK. Run `visual` twice on the same commit; `--compare` reports 0 changed pixels.
- [ ] Verify (perf): n/a.

## PR5: flows

- [ ] Depends on: PR2
- [ ] Change: move the `open(cx)` helper from the `repo_view.rs` tests into a `diffpilot::testing` module that integration tests can use. It already opens a real fixture repo in a `TestAppContext`.
- [ ] Change: `gpui-app/tests/flows.rs`, with one file per flow under `gpui-app/tests/flows/`. Each flow drives the full `RepoView` through events and asserts what the user sees:
  - open repo, pick a comparison, scroll to a file, mark it viewed, reopen: still viewed
  - open repo, add a comment, run a scripted review, reply to a thread
  - change the diff mode in settings; every open session uses it
- [ ] Change: `cargo xtask flows` runs `cargo test -p diffpilot --test flows`. `related` prints it when any `src/` file in `core` or `gpui-app` changes.
- [ ] Result: `cargo xtask flows` passes headless and opens no window.
- [ ] Verify (unit): break one step on purpose (skip the viewed-store write); the first flow fails with a clear assertion.
- [ ] Verify (live): n/a, the tier itself is headless.
- [ ] Verify (perf): `flows` must finish in under 30 s on a warm build.

## Not proven yet

- Whether the draft feature map is right. PR2's `features` command and the golden-file check will show it.
- Whether peak RSS is stable enough to gate on. PR3 measures it before it sets the limit.
- Whether `RepoView` can run the review and settings flows headless without new seams. PR5 starts with the viewed-files flow, which `repo_view.rs` tests already prove is possible.
- How long `cargo xtask` takes on a cold build. If the first run is slow, the pre-commit hook will be too; PR1 measures it.
