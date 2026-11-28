# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The guide overview's diagram has an Expand button. It opens the diagram at full size in a window you can scroll. Press Escape, click outside, or click the close button to go back.
- Guide diagrams are drawn larger, so their labels stay readable. A diagram that does not fit scrolls inside its card, with scrollbars on both sides.

### Fixed

- The guide overview no longer shows the Before and After text, chapter summaries, or other text one letter per line when the summary at the top runs over several lines.

## [0.2.1] - 2026-11-28

### Changed

- Code text in the diff is now 12.5 px instead of 13 px.
- Selecting lines in the diff no longer shows a floating action bar. Right-click a line or a selection to stage, unstage, discard, copy, open in your editor, or comment.

## [0.2.0] - 2026-11-28

### Added

- A pill next to the folder name shows how many folders are linked. Click it to add or remove linked folders, or press ⌥⌘L. The folder menu has a Linked Folders item, and shows each folder's links.
- Linked folders that moved or were deleted say so. Choose Locate… to point to the new place, or Remove.
- Switching folders while Claude is running now asks to stop the runs first. Choose Don't ask again, or turn it off in Settings, to stop and switch right away with a Go back button.
- Open Folder is now one sheet. It lists your recent folders with their branch, changed files, and open pull requests, and has a search for folders, paths, pull request titles, or #123. Browse… picks any other folder.
- On the Open Folder sheet, choose what to compare before you open: a branch and its base, or a pull request. Picking a pull request fills in both branches and opens it without a checkout. Edit… changes the linked folders.
- The Open Folder sheet shows when you start Diffpilot with no folder open.
- On the Open Folder sheet, Up and Down move through the recent folders, even while the search has focus, and Enter opens the one you picked.
- Linked folders you edit on the Open Folder sheet are kept for each folder while the sheet is open. Only the folder you open is saved.
- Type a path in the linked folder field or the Open Folder search to see up to 6 matching sub-folders. Use Up, Down, Tab, or Enter to complete one. Folders you cannot link say why.
- Select text in a Markdown preview by dragging, double-clicking a word, or triple-clicking a block, and copy it with ⌘C.
- The app tour has new stops for push and pull and for Commit, and its text covers linked folders, previews, and the comments button.
- The Open Folder sheet says "1 needs you" on a folder with a pull request waiting for your review, and marks that pull request with a Review requested pill. Team requests are not counted yet.
- Each Guide chapter has a review path: stops in the order to read them. A stop card shows why it matters, what to check, and a band marks its lines in the diff. A critical stop is marked Take care.
- Move along the review path with ] and [. Press v, or click Looks good, to mark a stop reviewed and go to the next one. Comment and Skip for now are on the card too.
- The review panel lists the stops of the open chapter with the chapter map above. The map lights the current stop's box, shows reviewed/total on each box, and a click on a box jumps to its first stop.
- A stop can point into a linked folder. It opens a read-only excerpt of that file with the lines marked, because there is nothing to diff.
- The Guide overview says "Review in N stops" and has a Start review button. The chapter list shows reviewed/total with a small progress bar for each chapter.
- The review path moves with soft motion: the card rises in, the band lights up line by line, the map pulses, and a reviewed check pops. It stops when Reduce Motion is on in macOS.

### Changed

- The folder name in the title bar no longer has the path under it. Hover the name to see the path. The linked folders and other changed folders now sit on a small second line, so they never run under the toolbar.
- Adding a linked folder is simpler: type a path and press Enter, drop a folder on the field, or choose Choose…. Changes save as you edit. The Guide start card lists the linked folders as chips.
- The folder menu item Add Folder… is now called Open Folder…. Open Folder… and ⌘O open the sheet, not the system folder picker.
- The title bar now shows what you compare as one label, like feature → main. Click it to open the Open Folder sheet on this folder and change the branches or pick a pull request.
- Guide mode no longer has its own Link button. Link folders from the title bar. Rewrite shows a hint when the linked folders changed since the guide was written.

- The toolbar is regrouped. What you compare sits next to the folder name. Ask Claude and the comments button share one pill on the right, next to push and pull.
- Every list and panel that scrolls shows a scrollbar while you scroll, like the diff: the Guide, the inspector, comments, settings, menus, checks, Claude's plan and log, and the Open Folder lists. Drag it to move fast.
- Diff text, Markdown previews, and the file list use larger text: code is 13 px, preview body text is 14 px, and file names are 13 px.

### Removed

- The step-by-step player under guide diagrams is gone. Each chapter now has a review path instead.

### Fixed

- In dark mode, a line no longer crosses the arrow of popovers and menus.
- On the Open Folder sheet, a long pull request list no longer covers the search field and the buttons, and the folder page starts at the top.

## [0.1.1] - 2026-11-27

### Added

- A short app tour shows the main parts of the window the first time you open a folder. It dims the window and points at each part. Start it again from Help, Show Tour.

### Changed

- Text across the app is smaller and uses four sizes, like VS Code on macOS: 13 px body, 12 px captions, 11 px badges, and 14 px bold titles. Diff lines are 18 px tall.
- The Comments panel now slides out when you close it, and the sidebar, inspector, and Claude area close with motion too.
- The Settings dialog now fades in and out.
- Menus and popovers now fade out when they close, as they already did when they open.
- The Previous thread and Next thread arrows are now dimmed when there is no thread in that direction.
- Switches slide their thumb, segmented controls fade the highlight to the new segment, and the Viewed check mark fades in.
- Open Folder now asks which folders to link before it opens the repo. Add several folders at once, or type a path. The Guide and the Link Folder menu can add several at once too.

### Fixed

- The app no longer crashes when you open the model and effort menu in the Ask Claude popover. The menu now opens above the button.
- Long paths and commands in Settings now wrap inside their row. Before, the Claude path ran past the dialog and under the buttons.
- The diff scroll bar can be dragged. Point at the right edge of the diff to show a wider thumb, drag it, or click the edge to jump there.
- Comment cards in the Comments panel no longer grow a few pixels when you point at them.
- Previous thread now stops on the review summary at the top of the diff, so you can step back to it from any comment.

## [0.1.0] - 2026-11-27

### Added

- Pause a running Claude run from the pill or the Claude area, then Continue it where it stopped or Stop it. Queued runs wait while a run is paused.
- Settings now show the app version at the bottom of the section list.
- The Commit… button in the bottom bar now opens a message box and commits all uncommitted changes.
- Generate in the commit box asks Claude to write the commit message from your uncommitted changes. You can edit it before you commit, and the run shows in the Claude area.
- Send to Claude in the comment box (also ⌘⇧↩) posts your comment and asks Claude to fix just that thread.
- Click a line, drag over the line numbers, or click a hunk header to select lines. A floating bar lets you copy, open in your editor, comment, and stage, unstage, or discard the selected lines.
- Drag over code, or double-click a word, to select characters, and press ⌘C to copy them. Esc clears the selection.
- Link folders outside the repo to a guide by path. Claude reads them for context and can list code there that the change may break.
- Edit your own comments. Edit puts the text back in the comment box, and Save keeps the new text.
- A Data tab in Settings lists the threads, runs, guides, and viewed marks stored for each repo and branch. Delete them per branch, per repo, or all at once.
- A Comments panel (C, View menu, or the button in the toolbar) lists every comment by file, with Open, Resolved and All filters. Resolved comments can be reopened. All views also shows comments from other views.
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

- The toolbar pill, the bottom status line and the Claude area now show the same Claude run for the repo, whichever feature started it: review, fix, reply, guide or commit message.
- Past Runs in the Claude area now lists runs of every kind on the branch, from every comparison, with their kind. Pick one to see its log.
- The project and branch menus in the toolbar now have an arrow that points at the button that opened them, like the popovers.
- Loading indicators now look like the native macOS spinner, and the bottom bar no longer spins a second one while Claude works.
- Text across the app is a little larger (about 8%). The code in the diff keeps its size.
- A guide now belongs to the branch you are on, not to the compare target. Change the target and the guide stays; if the diff is now different, it shows as out of date.
- Menus, popovers, context menus and the selection bar now share one liquid-glass look, and they fade in when they open.
- The sidebar, inspector and Claude area now slide in and out instead of jumping.
- Popovers (Checks, Sync, Ask Claude, Fix, Queue) now have an arrow that points at the button that opened them, like native macOS popovers.
- Settings now open as a dialog over the main window, like in VS Code, with a search box and a section list. Press Esc or click outside to close it.
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
- Reviews now show their plan and log in the Claude area while they run, and you can queue a second run.

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
- New files that git does not track yet now show in the sidebar and the diff when the comparison includes uncommitted changes. Claude's diff tools return them too.
- The More button on a file header opens a menu: Copy Path, Copy Diff, Reveal in Finder, Collapse File, and Mark as Viewed.
- The sidebar filter matches each word on its own, in any order, so a query like `diff pane` finds `diff_pane/view.rs`.
- Quitting diffpilot now stops the Claude reviews, replies, and fixes that are still running. After a crash, the next launch stops the ones that were left behind.

[Unreleased]: https://github.com/adamward459/diffpilot/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/adamward459/diffpilot/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/adamward459/diffpilot/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/adamward459/diffpilot/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/adamward459/diffpilot/releases/tag/v0.1.0
