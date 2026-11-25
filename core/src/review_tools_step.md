## Step 3: Leave the findings

Leave each finding as soon as you have checked it, with the diffpilot MCP tools (`mcp__diffpilot__add_comment` and `mcp__diffpilot__add_general_comment`). Do not wait for the end. Use these tools only. Do not run any other program to write comments.

1. Call `add_comment` once per finding.
   - `file` is relative to the repository root. `startLine` is the line number on the new side of the diff, and it must be an added or context line inside a hunk. For an issue on removed code, use `side: "old"` with the old line number. Use `startLine: 0` only when no line fits.
   - `severity` is one of:
     - `must-fix`: bugs, security issues, data loss; code that will break or produce wrong results.
     - `suggestion`: concrete improvements with a clear reason (including missing tests and incomplete changes). Not style preferences.
     - `nit`: minor but still worth changing.
     - `question`: something unclear that needs the author's clarification.
   - `title` is one short line naming the problem. `body` follows **How to write findings** below.
   - If a call fails because the line is not in the diff, pick a line that is.
2. When you are done, call `add_general_comment` once with the summary:
   - No findings: "No issues found. Checked for bugs and project-rule compliance."
   - 1-2 findings: one sentence verdict.
   - 3+ findings: summarize the themes.
   - Large diffs: always note the scope reviewed and group findings by area.
   - No severity labels in the summary. Lead with the verdict in one sentence, then at most 3 short bullets; no compliments, no filler, no narrating the code. If the user gave instructions, say how you addressed them.
3. Finish with one sentence here.

