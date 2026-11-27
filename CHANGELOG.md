# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The Commit… button in the bottom bar now opens a message box and commits all uncommitted changes.
- Generate in the commit box asks Claude to write the commit message from your uncommitted changes. You can edit it before you commit, and the run shows in the Claude area.
- Send to Claude in the comment box (also ⌘⇧↩) posts your comment and asks Claude to fix just that thread.
- Click a line, drag over the line numbers, or click a hunk header to select lines. A floating bar lets you copy, open in your editor, comment, and stage, unstage, or discard the selected lines.
- Drag over code, or double-click a word, to select characters, and press ⌘C to copy them. Esc clears the selection.
- Link folders outside the repo to a guide by path. Claude reads them for context and can list code there that the change may break.
- Edit your own comments. Edit puts the text back in the comment box, and Save keeps the new text.
- A Data tab in Settings lists the threads, runs, guides, and viewed marks stored for each repo and branch. Delete them per branch, per repo, or all at once.

### Changed

- A guide now belongs to the branch you are on, not to the compare target. Change the target and the guide stays; if the diff is now different, it shows as out of date.
- Menus, popovers, context menus and the selection bar now share one liquid-glass look, and they fade in when they open.
- The sidebar, inspector and Claude area now slide in and out instead of jumping.
- Popovers (Checks, Sync, Ask Claude, Fix, Queue) now have an arrow that points at the button that opened them, like native macOS popovers.
- Settings now open inside the main window, like in VS Code, with a search box and a section list. Press Esc to go back.
- Fix is now its own toolbar button, next to Ask Claude. It opens a list where you pick which comments to send, and it stays available while a review runs.
- Glass controls now look like real glass: a sheen lit from the top, a crisp rim, and a shadow that sinks when you hover or press. Buttons are now capsules, and controls, toolbars, and text are larger.
- The app is now named Diffpilot in the menu bar and the Dock.
- Hunk actions no longer appear when you hover a hunk header. Click the header to select the hunk and use the same floating bar as for selected lines.
- Copy Path and Reveal in Finder are now buttons in each file header, so you no longer need the More menu for them.
- Markdown Preview now fills the whole width. A changed file shows as one document, like GitHub's rich diff: added blocks are marked green and removed blocks red.
- The app uses about 100 MB less memory. A diff window now takes about 117 MB instead of 219 MB.
- Guide runs now show in the Claude area like other runs: their log, the queue, Past Runs, and a status line with the chapter count.
- Ask Claude and @claude replies are now Claude runs: they wait in the queue, show in the Claude area, and the reply appears in the thread when Claude writes it instead of word by word.
- When you ask for a change in a thread, Claude can now make it and mark the thread addressed with a one-line summary. A question gets an answer and stays open.

### Removed

- The comment button next to Ask Claude in the toolbar. Use Comment on file in each file header instead.
- The Diff/Edit switch in file headers. Edit was never available; files that can show a preview still have the Preview/Diff switch.
- The Viewed checkbox on file rows in the sidebar. Mark a file as viewed from its file header. Viewed files still show dimmed in the sidebar.

### Fixed

- The Previous and Next thread arrows in the top bar now jump to the thread above or below. Before, they were always disabled.
- Comment on file in a file header now opens a comment on that file.
- The menu bar and shortcuts (Settings…, Open Folder…, ⌘, and the rest) no longer stop working after you hide the sidebar, open the Guide, or close a menu.
- Guide text now wraps inside the Before and After cards, the chapter list, and the chapter panel. Backticks in chapter summaries show as code, and a long Next label ends in an ellipsis.
- The Swipe and Onion Skin sliders on a changed image now follow the mouse until you let go, even when the pointer leaves the slider.
- You can delete a comment thread again: each thread card has a Delete button. Cancel on a new comment now deletes it instead of keeping an empty resolved thread.

### Added

- Compare a branch with its base: another branch, a tag, a recent commit, only the working tree, or only the staged changes.
- Mark files as viewed. The mark is kept per repository and branch, and it clears when the file changes again.
- A sidebar that shows the changed files as a tree or a list, with a name filter and filters for uncommitted and viewed files.
- Image files show a preview with image size and file size. Compare a changed image 2-up, with a Swipe handle, or as an Onion Skin fade.
- Markdown files show the diff. Click Preview in the file header to see the rendered document.
- Comment threads on diff lines. Type `@claude` in a comment to get a reply from Claude.
- Ask Claude to review the change, with focus areas, extra instructions, and a model choice. Findings appear as threads on their lines.
- Guide mode (⌘2): Claude writes a guide to the change, and you read it chapter by chapter, with notes under the lines they explain and a panel that lists what to check.
- Guide diagrams: flowcharts and sequence diagrams drawn in the app, with a Source view and a step-by-step walk through the flow.
- The guide shows when it is out of date because the branch moved, and Rewrite writes a new one.
- Click the ahead and behind counts to fetch, pull, or push. The popover says what each action does and tells you to pull first when the remote is ahead. Shortcuts: ⇧⌘F, ⇧⌘L, ⇧⌘U.
- The branch menu lists open GitHub pull requests. Pick one to compare it with its base without a checkout, or choose Open on GitHub. The branch you are on shows its PR number and checks state.
- The toolbar shows the checks and review state of the branch's pull request. Open the checks to see each job, re-run one or all failed jobs, and open a job's log in the browser.
- The Claude area (⇧⌘Y): a panel under the diff that shows the plan, the comments so far, and the log of the run Claude is working on. It also has Stop, Run Again, Queue…, and Past Runs.
- The bottom bar shows what Claude is doing or did last, with Show Log and Run Again.
- A `diffpilot` command that opens a repository in the app.
- An app icon.
- Releases: pushing a `vX.Y.Z` tag publishes a universal (Apple Silicon and Intel) app to GitHub Releases, and `install.sh` installs the latest release.

### Changed

- Reviews now show their plan and log in the Claude area while they run, and you can queue a second run.

### Fixed

- New files that git does not track yet now show in the sidebar and the diff when the comparison includes uncommitted changes. Claude's diff tools return them too.
- The More button on a file header opens a menu: Copy Path, Copy Diff, Reveal in Finder, Collapse File, and Mark as Viewed.
- The sidebar filter matches each word on its own, in any order, so a query like `diff pane` finds `diff_pane/view.rs`.
- Quitting diffpilot now stops the Claude reviews, replies, and fixes that are still running. After a crash, the next launch stops the ones that were left behind.

[Unreleased]: https://github.com/adamward459/diffpilot/commits/main
