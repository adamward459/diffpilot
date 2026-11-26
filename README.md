# diffpilot

diffpilot is a macOS app for reviewing Git changes before you push them. It shows a branch compared to its base, keeps track of the files you have viewed, and lets Claude review the change and answer your comments. It is written in Rust with [GPUI](https://www.gpui.rs).

![diffpilot in light mode](screenshots/feature/gpui-app/light.png)

## Requirements

- macOS 13 or later, Apple Silicon or Intel.
- Git. Install the Xcode command line tools if you don't have it: `xcode-select --install`.
- Optional, for the Claude features: [Claude Code](https://docs.claude.com/en/docs/claude-code), signed in. diffpilot runs the `claude` command on your Mac. It needs no API key.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/adamward459/diffpilot/main/install.sh | bash
```

The script does three things:

1. It downloads the latest release from [GitHub Releases](https://github.com/adamward459/diffpilot/releases). The app runs on Apple Silicon and Intel.
2. It puts `diffpilot.app` in `/Applications`, or in `~/Applications` if `/Applications` is not writable.
3. It adds a `diffpilot` command in `~/.local/bin`. If that folder is not on your `PATH`, the script tells you the line to add to `~/.zshrc`.

To pass options, use `bash -s --`, for example `curl -fsSL .../install.sh | bash -s -- --no-cli`.

| Option | What it does |
| --- | --- |
| `--version vX.Y.Z` | Install that release instead of the latest |
| `--from-source` | Build from a clone instead of downloading. Needs [Rust](https://rustup.rs). Run `./install.sh --from-source` inside the clone |
| `--dest DIR` | Install the app into `DIR` |
| `--no-cli` | Skip the `diffpilot` command |
| `--uninstall` | Remove the app and the command |

To update, run the install command again.

The app is not notarized by Apple. If you download `diffpilot-macos.zip` with a browser instead of the script, macOS blocks the app. Allow it with `xattr -dr com.apple.quarantine /Applications/diffpilot.app`.

## Run

- Open **diffpilot** from Applications or Spotlight, then press ⌘O and pick a Git repository.
- From a terminal, run `diffpilot` inside a repository, or `diffpilot path/to/repo`. Any folder inside the repository works.
- Without installing, run `cargo run --release -p diffpilot -- path/to/repo` from this folder.

diffpilot opens the last repository you used. It reloads by itself when files or branches change.

## Use it

- **Pick what to compare.** The toolbar has two menus. The first picks the branch, and the second picks the base it is compared to. The base can be a branch, a tag, a recent commit, only the working tree, or only the staged changes.
- **Track your progress.** Check **Viewed** on a file to collapse it. diffpilot remembers viewed files per repository and branch. It clears the mark when the file changes again.
- **Find files.** The sidebar shows the changed files as a tree or a list. Filter by name, show only uncommitted files, or hide viewed files.
- **Comment.** Hover a line and click `+` to start a thread. Type `@claude` in a comment to get a reply from Claude.
- **Review with Claude.** Click **Ask Claude** to run a review. You can pick focus areas, add instructions, and choose the model. Findings appear as threads on the lines they talk about. Press ⌘. to stop a review.

| Shortcut | Action |
| --- | --- |
| ⌘O | Open a repository |
| ⌃⌘S | Show or hide the sidebar |
| ⌘. | Stop the Claude review |
| ⌘↩ | Post a comment |
| ↑ ↓ ← → | Move through the sidebar |

## Where data lives

Settings and review data are in `~/Library/Application Support/diffpilot/`: `settings.json`, and `review.sqlite3` for threads, runs, and viewed files. Delete that folder to reset diffpilot. diffpilot never writes into your repository's `.git` folder.

## Develop

The Cargo workspace has three crates:

| Crate | Holds |
| --- | --- |
| `core` | Git, comparisons, the diff parser, viewed files, settings, the file watcher, and the Claude runner. No UI. |
| `ui-kit` | The theme, the icons, and the shared components (buttons, glass capsules, badges). `cargo run -p diffpilot-ui-kit --example gallery` shows them. |
| `gpui-app` | The `diffpilot` binary: window, sidebar, and comparison menus. Its library holds the diff view (`src/diff_pane/`) and the Claude review pill, popover, and thread cards (`src/review/`). |

Common commands:

```sh
git config core.hooksPath .githooks   # once per clone: format Rust on commit
cargo run -p diffpilot                # debug build
cargo xtask related                   # print the tests and checks for your change
cargo xtask unit                      # run all tests
cargo xtask perf                      # compare time and memory benchmarks with the base branch
cargo xtask visual                    # capture the screenshot scenes of every feature
```

These environment variables help with testing:

| Variable | Effect |
| --- | --- |
| `DIFFPILOT_REPO=path` | Open this repository |
| `DIFFPILOT_APPEARANCE=light` or `dark` | Force the appearance |
| `DIFFPILOT_REVIEWER=scripted` | Use a fake Claude reviewer with a fixed result |
| `DIFFPILOT_VIEWED_SEED=file.json` | Start with these files marked viewed, kept in memory |
| `DIFFPILOT_BENCH=1` | Run a benchmark, print JSON, and quit |

`cargo xtask fixture` builds `.fixtures/large`, a test repository with about 2000 changed files. The benchmarks and screenshots use it.

## Release

1. Write each change under `## [Unreleased]` in `CHANGELOG.md` as you make it. Keep each line at 250 characters or less.
2. Run the **Release** workflow on `main`, from the Actions tab or with `gh workflow run release.yml -f version=X.Y.Z`.

The workflow moves the Unreleased notes into `## [X.Y.Z] - <date>`, updates the compare links, and sets the version in `gpui-app/Cargo.toml` and `Cargo.lock` (`cargo xtask release X.Y.Z`). It builds a universal `diffpilot.app` (`cargo xtask bundle --universal --zip`). Only after the build passes does it push the `chore(release): vX.Y.Z` commit and the `vX.Y.Z` tag to `main`, and then it publishes `diffpilot-macos.zip` with that version's changelog section as the release notes. To build the same app locally, run `cargo xtask bundle --universal`.
