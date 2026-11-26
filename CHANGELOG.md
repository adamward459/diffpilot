# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Glass controls now look like real glass: a sheen lit from the top, a crisp rim, and a shadow that sinks when you hover or press. Buttons are now capsules, and controls, toolbars, and text are larger.
- The app is now named Diffpilot in the menu bar and the Dock.

### Removed

- The comment button next to Ask Claude in the toolbar. Use Comment on file in each file header instead.

### Fixed

- Comment on file in a file header now opens a comment on that file.

### Added

- Compare a branch with its base: another branch, a tag, a recent commit, only the working tree, or only the staged changes.
- Mark files as viewed. The mark is kept per repository and branch, and it clears when the file changes again.
- A sidebar that shows the changed files as a tree or a list, with a name filter and filters for uncommitted and viewed files.
- Comment threads on diff lines. Type `@claude` in a comment to get a reply from Claude.
- Ask Claude to review the change, with focus areas, extra instructions, and a model choice. Findings appear as threads on their lines.
- Guide mode (⌘2): Claude writes a guide to the change, and you read it chapter by chapter, with notes under the lines they explain and a panel that lists what to check.
- Guide diagrams: flowcharts and sequence diagrams drawn in the app, with a Source view and a step-by-step walk through the flow.
- The guide shows when it is out of date because the branch moved, and Rewrite writes a new one.
- Click the ahead and behind counts to fetch, pull, or push. The popover says what each action does and tells you to pull first when the remote is ahead. Shortcuts: ⇧⌘F, ⇧⌘L, ⇧⌘U.
- The branch menu lists open GitHub pull requests. Pick one to compare it with its base without a checkout, or choose Open on GitHub. The branch you are on shows its PR number and checks state.
- The toolbar shows the checks and review state of the branch's pull request. Open the checks to see each job, re-run one or all failed jobs, and open a job's log in the browser.
- A `diffpilot` command that opens a repository in the app.
- An app icon.
- Releases: pushing a `vX.Y.Z` tag publishes a universal (Apple Silicon and Intel) app to GitHub Releases, and `install.sh` installs the latest release.

### Changed

- Hunk actions (Stage Hunk, Revert, Open in Editor, Copy, Ask Claude) appear in a floating bar when you hover a hunk header, like the selection bar.

### Fixed

- New files that git does not track yet now show in the sidebar and the diff when the comparison includes uncommitted changes. Claude's diff tools return them too.
- The More button on a file header opens a menu: Copy Path, Copy Diff, Reveal in Finder, Collapse File, and Mark as Viewed.
- The sidebar filter matches each word on its own, in any order, so a query like `diff pane` finds `diff_pane/view.rs`.
- Quitting diffpilot now stops the Claude reviews, replies, and fixes that are still running. After a crash, the next launch stops the ones that were left behind.

[Unreleased]: https://github.com/adamward459/diffpilot/commits/main
