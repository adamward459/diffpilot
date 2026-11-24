---
name: feature-tests
description: Pick and run the unit, e2e, hot path, and screenshot tests for the diffpilot features a change touches, using the feature folder layout and scripts/test.py. Use before you report a code change as complete, when you add a feature, source file, or test, when you add a performance-sensitive code path, or after you open a PR.
---

# Feature tests

Code and tests are grouped by feature folder:

| Folder | Holds |
| --- | --- |
| `diffpilot/<Feature>/` | source of one feature |
| `diffpilot/Shared/` | code used by more than one feature; a change here touches every feature |
| `diffpilotTests/<Feature>/` | unit tests |
| `diffpilotUITests/<Feature>/` | e2e tests that launch the app |

A test type whose name contains `Performance` is a hot path test, in either test target. A UI test type whose name contains `Screenshot` captures the feature's screenshots. `features.json` holds only what the folders cannot say:

- `hotpaths`: a list of `{feature, glob, tests}`. The glob names source files on a hot path, such as scroll, resize, keystroke, launch, or large-diff code. `tests` are ids like `Target/Type` or `Target/Type/method` of hot path tests in that feature's folders.
- `na`: `{"<Feature>": {"unit" | "e2e" | "screenshot": "<reason>"}}` for a feature where that kind of test does not apply. Write a real reason.

`scripts/test.py lint` fails when a Swift file is outside a feature folder, a feature has no unit, e2e, or screenshot test and no `na` reason, an `na` reason is stale, or a hot path test is not listed in `hotpaths`. Every run command lints first.

## Workflow

1. Put new code in its feature folder, and new tests in the matching test folders. A new folder is a new feature.
2. When you add code on a hot path, add a `*Performance*` test and a `hotpaths` entry.
3. Before you report the work as complete, run `scripts/test.py related`. It prints the commands for the features your changes touch, compared with the upstream branch, including uncommitted and untracked files.
4. Run the commands that cover your change, and every `hotpath` command it prints. Do not report the work as complete until they pass. If you skip a printed command, say which one and why.

5. After you open or update a PR, run `scripts/pr_screenshots.py`. It runs the screenshot tests of the features the PR touches, saves the images to `screenshots/feature/<Feature>/`, commits and pushes them, and puts them in a `## Screenshots` section of the PR body. Look at each image before you report the PR as ready.

On failure, the script prints an `xcresulttool` command that shows the failure messages.

## Screenshot tests

- One `<Feature>ScreenshotUITests` type in `diffpilotUITests/<Feature>/`. Drive the app to the state that shows the feature, then attach `window.screenshot()` with `attachment.name = "<kebab-case-name>"` and `attachment.lifetime = .keepAlways`. The name becomes the file name.
- Use deterministic data, such as a fixture launch environment, so screenshots only change when the UI changes.

## Hot path tests

- The target is 120 FPS, so one frame has a budget of 8.33 ms. Code that runs on every frame must fit in that budget with room to spare.
- A bare `measure {}` with no baseline never fails. A hot path test must fail when performance gets worse. Assert an explicit budget, or count deterministic work (cell reloads, layout passes, diff operations) against a recorded baseline.
- Wall-clock numbers depend on the host. Prefer deterministic counts for the gate. When you use time or frame metrics (`XCTOSSignpostMetric.scrollingAndDecelerationMetric`, `XCTClockMetric`), set a budget well above the noise.
