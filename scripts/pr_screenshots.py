#!/usr/bin/env python3
"""Capture screenshots of the crates the current branch's PR touches and show them in the PR.

Runs `scripts/test.py screenshot` for those crates (the release app, light and dark, on the fixture
repo), pixel-diffs each image with the base branch's committed copy, commits screenshots/feature/<crate>/,
pushes, and replaces the screenshots section of the PR body with the images and the list of changed ones.
Needs Screen Recording permission for the terminal.

  scripts/pr_screenshots.py [crate...]   default: crates touched relative to the PR base branch
"""

from __future__ import annotations

import importlib.util
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
START, END = "<!-- screenshots -->", "<!-- /screenshots -->"

# Loaded by path: "test" would otherwise resolve to Python's own test package.
_spec = importlib.util.spec_from_file_location("diffpilot_test", ROOT / "scripts" / "test.py")
tests = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = tests
_spec.loader.exec_module(tests)


def sh(*cmd: str) -> str:
    return subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()


def section(repo: str, sha: str, names: list[str], base: str, changed: list[str]) -> str:
    lines = [START, "## Screenshots", ""]
    lines.append(f"Changed vs `{base}`:" if changed else f"No image changed vs `{base}`.")
    lines.extend(f"- {line.split(' (diff: ')[0]}" for line in changed)
    lines.append("")
    for feature in names:
        shots = sorted((tests.SCREENSHOT_DIR / feature).glob("*.png"))
        if not shots:
            continue
        lines.append(f"### {feature}")
        for shot in shots:
            path = shot.relative_to(ROOT).as_posix()
            lines.append(f"![{feature}: {shot.stem}](https://github.com/{repo}/blob/{sha}/{path}?raw=true)")
        lines.append("")
    lines.append(END)
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    pr = json.loads(sh("gh", "pr", "view", "--json", "number,baseRefName,body,headRefName"))
    repo = sh("gh", "repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner")
    if argv:
        names = argv
    else:
        sh("git", "fetch", "--quiet", "origin", pr["baseRefName"])
        crates = tests.workspace()
        touched = tests.touched_crates(crates, tests.changed_files(f"origin/{pr['baseRefName']}"))
        names = tests.screenshot_crates(touched)
    if not names:
        print("No touched crate has a window to capture.", file=sys.stderr)
        return 0

    code = tests.main(["screenshot", *names])
    if code != 0:
        return code
    base = f"origin/{pr['baseRefName']}"
    changed = tests.compare_screenshots(tests.resolve(tests.workspace(), names), base)

    sh("git", "add", "--", *(str(tests.SCREENSHOT_DIR / n) for n in names))
    if subprocess.run(["git", "diff", "--cached", "--quiet"], check=False, cwd=ROOT).returncode != 0:
        sh("git", "commit", "-m", f"docs(screenshots): capture {', '.join(names)}")
    sh("git", "push", "origin", f"HEAD:{pr['headRefName']}")

    sha = sh("git", "rev-parse", "HEAD")
    body = pr["body"] or ""
    block = section(repo, sha, names, base, changed)
    if START in body and END in body:
        body = re.sub(re.escape(START) + r".*?" + re.escape(END), lambda _: block, body, flags=re.DOTALL)
    else:
        body = f"{body.rstrip()}\n\n{block}\n"
    subprocess.run(
        ["gh", "pr", "edit", str(pr["number"]), "--body-file", "-"], cwd=ROOT, input=body, text=True, check=True
    )
    print(f"Updated PR #{pr['number']} with screenshots of {', '.join(names)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
