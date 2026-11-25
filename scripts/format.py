#!/usr/bin/env python3
"""Format all Rust and Python code.

scripts/format.py             format every file
scripts/format.py --restage   also re-add staged files after formatting (used by the pre-commit hook)
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PYTHON_PATHS = ["scripts", ".githooks"]


def run(*cmd: str) -> str:
    return subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True).stdout


def main(argv: list[str]) -> int:
    staged = run("git", "diff", "--cached", "--name-only", "--diff-filter=ACMR").split()
    try:
        run("cargo", "fmt", "--all")
        run("uvx", "ruff", "format", "--quiet", *PYTHON_PATHS)
    except subprocess.CalledProcessError as e:
        print(f"format: {' '.join(e.cmd)} failed\n{e.stderr}", file=sys.stderr)
        return 1
    if "--restage" in argv and staged:
        run("git", "add", "--", *staged)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
