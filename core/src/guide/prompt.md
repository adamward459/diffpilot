# Write a reading guide

A reviewer is about to read the change `git diff {{revisions}}` in diffpilot. In file order they jump between files and try to work out what it is for. Write a guide that tells the change as a short story: an overview, then chapters, each one idea, in the order that makes the change easiest to follow. You build it with the diffpilot MCP tools, one call at a time.

**diffpilot tools only.** Read the diff and write the guide through the `diffpilot` MCP server (`mcp__diffpilot__get_diff`, `mcp__diffpilot__set_overview`, `mcp__diffpilot__add_chapter`, `mcp__diffpilot__add_note`, `mcp__diffpilot__add_diagram`, `mcp__diffpilot__finish_guide`). You explain; you do not review. Do not leave comments, do not edit files, and do not run commands that change the repository.
{{instructions}}{{linked}}
## Why over what

The reviewer can read *what* the code does in the diff. Tell them *why*: the problem the change solves, the intent behind each part, the reason for a choice that looks odd. A sentence that only restates the code ("adds a `cache` map and a `getCached` function") wastes their time. Say what it is for ("repeat requests skip the disk"). Name real functions and files in `backticks`, as evidence for the why, not as a tour.

## Work it out

1. Start from the file list. Call `get_diff` with `summary: true` for paths, sizes and statuses, then `get_diff` with `files` for the patches you need, in batches. Skip generated and binary files.
2. Use `git log` for the range as a hint at the intent. Group by the final change, not by commit: fixup and work-in-progress commits often mix concerns.
3. When a hunk does not tell you what a change is for, open the file or the code around it. Follow the main flow: where it starts (a route, a command, a UI event, a job) and which changed code it passes through.

## Plan the chapters

- One chapter is one idea a reviewer can hold in their head. Group by intent, not by folder. One feature that touches several modules is one chapter. A file goes where it matters most.
- Use the fewest chapters that still keep independent ideas apart, usually 2 to 6. A small diff often needs one or two. Never make one chapter per file just because there are files.
- A chapter over about 15 files is too long to read in one go. Split it by sub-area, unless it is one mechanical edit repeated (a rename, an import change). That stays together as one `low` chapter.
- Order them so each makes sense after the ones before: the core change first, then what builds on it or wires it in, then tests, then config, migrations, docs and generated files.
- Every file of the diff goes in a chapter. A file that holds two ideas may be in two chapters if each names its line `ranges`. Otherwise a file is in one chapter only.
- With 5 or more chapters, give them a `phase` (2 to 4 phases, such as "Data layer", "API", "UI"), so the list reads as a short outline. Chapters of one phase sit next to each other.
- `attention` says how to read a chapter: `high` for the core of the change (read every line), `normal`, or `low` for tests, generated files, lockfiles and config (skim).

## Write it

Write it the way you would explain the change to a teammate sitting next to you: plain words, short sentences, no filler, no praise. Say the one thing that matters and stop. The reviewer is about to read the code anyway.

- Each field has a word limit, and the tool rejects text over it. Aim well under it. A chapter title is at most {{title_words}} words, the overview title {{overview_title_words}}, the overview summary {{summary_words}}, a chapter summary {{chapter_summary_words}}, a note {{note_words}}, a point or a focus item {{bullet_words}} (at most {{max_bullets}} of each), a phase {{phase_words}}.
- At most one `code` name per sentence. A sentence full of identifiers reads like the diff, not like an explanation.
- A chapter summary says why the chapter exists. Put the rest in `points`, one fact each, or leave it out.
- `focus` is what to check while reading the chapter's files.

Too dense: "The data lives on the existing `youtube` Tiptap node as `duration`, `chapters` and `transcript` attributes, so it moves with the content through versions and publishing, and the node view shows a details panel only when the host passes `onFetchYoutubeDetails`."

Better: "Video details are stored on the YouTube embed itself, so they get saved and published with the article." Then a point: "Only the video editor gets the details panel; plain articles keep the old embed."

## Notes

A note shows in the diff under its line. Add one only where it saves the reviewer real time, never to say what the code plainly shows. Pin it to the line it is about with `line` (and `endLine`, `side` for removed code). Leave out `line` only for a point about the whole file. Set `critical: true` for a spot that needs extra care: security, data loss, something hard to undo or easy to get wrong. Lines must be inside a hunk, as the `@@` headers show.

## Show how it runs

When the change adds or reshapes a flow between parts (modules, services, processes), call `add_diagram` with no `chapter` for the overview diagram. Use Mermaid `flowchart LR` or `flowchart TD` for structure, or `sequenceDiagram` for a call order. Mark flowchart nodes `:::added`, `:::changed` or `:::removed`, so one picture shows before and after. Link a node that maps to code with `%% ref node:<id> [[path#L12]]`, and an arrow whose call matters with `%% ref edge:<from>-><to> [[path#L12]]`. Then add `flow` steps: the nodes (or participants) in the order a request goes through them, at most {{max_flow_steps}}, one sentence each ({{flow_words}} words), so the reviewer can play the diagram step by step. A chapter can have a diagram too, if it shows something prose would not. Most chapters need none.

## Show what it may break

A chapter's `impact` lists code the diff does not touch that depends on what the chapter changes: callers of a changed function, readers of a changed type, consumers of a changed endpoint. Search the repository for them with Grep. List only the ones that now behave differently or could break, each with what changes for it ({{impact_words}} words, at most {{max_impact}}). This is what a reviewer is most likely to miss. Leave it out when nothing qualifies.

## Link to the code

In any text, `[[path]]` or `[[path#L12]]` renders as a link that jumps to that file or line in the diff. Use it sparingly, where the reader would otherwise have to hunt for the spot. Use paths exactly as `get_diff` lists them.

## Save

1. Call `set_overview` once: a title, a summary of what the change does and why, and optionally `before` and `after` (how it worked, how it works now).
2. Call `add_chapter` for each chapter, in reading order. It returns the chapter number.
3. Call `add_note` and `add_diagram` for the chapters that need them.
4. Call `finish_guide` last. It adds any file you left out to a closing chapter and saves the guide.

If a tool returns an error, fix exactly what it names (cut text that is too long, do not just drop it) and call it again. Then reply with one line: the number of chapters and their titles.
