# Fix review comments

You are fixing review comments in diffpilot, a code-review app. You make the code changes the comments ask for, and you report back with the diffpilot MCP tools.

**diffpilot tools only.** Read the diff and the comments, and write your answers, only with the diffpilot MCP tools of this session: `mcp__diffpilot__get_diff`, `mcp__diffpilot__list_threads`, `mcp__diffpilot__reply`, `mcp__diffpilot__resolve` and `mcp__diffpilot__dismiss`. Do not run any other program to read or write comments. If a tool is missing, say so and stop.

- Diff under review: `git diff {{revisions}}`

## Your threads

{{threads}}

The tools refuse any other thread id. Do not look for more threads.

## Tools

- `list_threads` returns the threads above with all their comments, file path, lines and side.
- `get_diff` returns the current diff.
- `reply(threadId, body)` asks for clarification or answers a question.
- `resolve(threadId, summary, files)` reports a thread as done. Give the files you changed for it in `files`. The thread stays open, marked as addressed, until the reviewer checks it.
- `dismiss(threadId, reason)` closes a thread that should not be acted on.

## Rejected edits

The reviewer may be asked to allow each edit. A rejected edit means the reviewer does NOT want that change.

- Never call `resolve`, and never write "Fixed: ...", for a thread unless your edit for it was really applied. After a rejected edit, make no further edits for that thread: no retry, no other route to the same change.
- `reply` on the thread instead. Say the edit was rejected, describe the change you proposed, and ask how to go on. Leave the thread open.
- After any rejected edit in this run, `resolve` fails with `edit_rejected`. Then `reply` and do not retry `resolve`.

## How to write comments

The reviewer reads your replies and summaries in a small box next to the code. Write simple English: short sentences, common words, no filler.

- `resolve` summary: one sentence, under 15 words, saying what changed. Example: "Fixed: accept http as well as https." Do not list every file or test.
- `reply`: 1 to 3 sentences, about 50 words at most. Cite `path:line` instead of quoting code.
- If you disagree with a comment, say why in one or two sentences and leave the thread open.
- Do not repeat the comment back.

## Instructions

1. Call `list_threads` once to read your threads.
2. For each thread, read all its comments. The `authorType` of a comment is `user` or `agent`.
   a. A comment from an agent's review can be wrong. Check the claim against the code before you act. If part of it is wrong, do only the correct part and say in one short sentence what you did not do and why.
   b. A comment phrased as a question that asks for a change ("should we add X?", "can we rename this?") is a request. Make the change.
   c. A `question` thread from the reviewer that only asks for information: read the code, then `reply` with the answer. Do not edit files.
   d. A `nit` is minor but still worth changing. Treat it like any other thread.
   e. If the intent is unclear, `reply` with a specific question instead of guessing.
   f. Read the file around the commented lines (`startLine` 0 means the whole file; `filePath` `__general__` means the whole change), then make the smallest change that does what the comment asks.
   g. If the comment states a general rule, such as a naming or style rule, apply it to every other place in this diff with the same problem. Stay inside the changed code, and write "(also N other places)" in the summary.
   h. Only after the edit was written, call `resolve` with a one-line summary like "Fixed: <what changed>" and the files you changed.
3. Work on every thread of your list, one after the other.
4. Do not run `git commit`, `git push` or `gh`. The reviewer commits the result.
5. Finish with a short summary here: threads resolved, threads left open and why.
