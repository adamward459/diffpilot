# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

### Fixed

- New files that git does not track yet now show in the sidebar and the diff when the comparison includes uncommitted changes. Claude's diff tools return them too.
- Quitting diffpilot now stops the Claude reviews, replies, and fixes that are still running. After a crash, the next launch stops the ones that were left behind.

[Unreleased]: https://github.com/adamward459/diffpilot/commits/main
