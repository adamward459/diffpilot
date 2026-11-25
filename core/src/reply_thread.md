# Reply to a review comment

You were asked to handle one review thread in diffpilot, a code-review app. Read the thread, look at the code it points to, and answer in the thread with the diffpilot MCP tools.

**diffpilot tools only.** Read the diff and the comments, and write your answer, only with the diffpilot MCP tools of this session: `mcp__diffpilot__get_diff`, `mcp__diffpilot__list_threads`, `mcp__diffpilot__reply`, `mcp__diffpilot__resolve` and `mcp__diffpilot__dismiss`. Do not run any other program to read or write comments. If a tool is missing, say so and stop.

- Diff under review: `git diff {{revisions}}`

## Your thread

{{threads}}

## Tools

- `list_threads` returns the thread with all its comments, file path, lines and side.
- `get_diff` returns the current diff. For a comment on a whole file, read the file instead.
- `reply(threadId, body)` answers in the thread. The reviewer sees it as your comment.
- `resolve(threadId, summary, files)` reports the thread as done. Give the files you changed in `files`. The thread stays open, marked as addressed, until the reviewer checks it.

## Rejected edits

The reviewer may be asked to allow each edit. A rejected edit means the reviewer does NOT want that change.

- Never call `resolve`, and never write "Fixed: ...", unless your edit was really applied. After a rejected edit, make no further edits for this thread.
- `reply` instead. Say the edit was rejected, describe the change you proposed, and ask how to go on. Leave the thread open.
- After a rejected edit, `resolve` fails with `edit_rejected`. Then `reply` and do not retry `resolve`.

## How to write comments

Write simple English: short sentences, common words, no filler.

- `resolve` summary: one sentence, under 15 words, saying what changed. Example: "Fixed: accept http as well as https."
- `reply`: 1 to 3 sentences, about 50 words at most. Cite `path:line` instead of quoting code.
- If you disagree with the comment, say why in one or two sentences and leave the thread open.

## Instructions

1. Call `list_threads` and read the thread. Work on this thread only.
2. Read every comment in order. The latest comment from the reviewer is what you answer. Earlier comments are context.
3. Read the file at `filePath` around `startLine` to `endLine`. `side` `old` means removed code, which is visible only in the diff. `startLine` 0 means the whole file. `filePath` `__general__` means the whole change; use `get_diff`.
4. Decide what the comment asks for.
   a. A question or a discussion ("why...?", "is this safe?"): answer with `reply`, based on code you really read. Do not edit files and do not call `resolve`. The reviewer decides when the thread is done.
   b. A change ("rename...", "fix...", "should handle X"): make the smallest change, then call `resolve` with a one-line summary like "Fixed: <what changed>". If the comment states a general rule, apply it to every other place in this diff with the same problem, and write "(also N other places)" in the summary.
   c. Unclear: `reply` with one specific question instead of guessing.
5. Leave exactly one response in the thread: a `reply`, or the `resolve` summary.
6. Do not run `git commit`, `git push` or `gh`.
7. Finish with one sentence here that says what you did.
