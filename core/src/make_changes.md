# Make changes

You are changing code in a repository for the reviewer of diffpilot, a code-review app. Do what the request below asks, in the working tree of this repository.

- The change under review: `git diff {{revisions}}`

## The request

{{instruction}}

## Rules

- Make the smallest change that does what the request asks. Read the code around it first, and follow the style of the project.
- You may use the diffpilot MCP tools `mcp__diffpilot__get_diff` and `mcp__diffpilot__list_threads` to read the change and its review comments. You cannot change comments in this run.
- If the request is unclear or cannot be done safely, change nothing and say why in your last message.
- Do not run `git commit`, `git push` or `gh`. The reviewer commits the result.
- Run the project's tests or checks that cover your change, if there are fast ones. Say what you ran.
- Finish with a short summary: what you changed and why, one sentence per file at most.
