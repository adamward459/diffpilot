#!/usr/bin/env python3
"""Keep the committed DiffPane web build in step with its source.

web/web.stamp holds a hash of the build inputs (web/src, package.json, package-lock.json,
build.mjs) and a hash of the committed build output. `check` fails when either one changed
without a rebuild.

  scripts/web_stamp.py write   record the current hashes (npm run build in web/ calls this)
  scripts/web_stamp.py check   fail when the build is stale
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WEB = ROOT / "web"
OUTPUT = ROOT / "diffpilot" / "Features" / "DiffPane" / "Web"
STAMP = WEB / "web.stamp"
INPUT_FILES = ["package.json", "package-lock.json", "build.mjs"]


def digest(root: Path, files: list[Path]) -> str:
    h = hashlib.sha256()
    for f in sorted(files):
        h.update(f.relative_to(root).as_posix().encode() + b"\0" + f.read_bytes() + b"\0")
    return h.hexdigest()


def inputs() -> str:
    files = [p for p in (WEB / "src").rglob("*") if p.is_file()] + [WEB / n for n in INPUT_FILES]
    return digest(WEB, files)


def output() -> str:
    return digest(OUTPUT, [p for p in OUTPUT.rglob("*") if p.is_file()])


def problems() -> list[str]:
    if not STAMP.exists():
        return ["web/web.stamp is missing: run `npm run build` in web/ and commit the result"]
    recorded = json.loads(STAMP.read_text())
    found = []
    if recorded.get("inputs") != inputs():
        found.append(
            "web/src, package.json, package-lock.json, or build.mjs changed since the last build: "
            "run `npm run build` in web/ and commit diffpilot/Features/DiffPane/Web/ and web/web.stamp"
        )
    if recorded.get("output") != output():
        found.append(
            "diffpilot/Features/DiffPane/Web/ does not match web/web.stamp: "
            "run `npm run build` in web/ instead of editing the build output"
        )
    return found


def main(argv: list[str]) -> int:
    if argv == ["write"]:
        STAMP.write_text(json.dumps({"inputs": inputs(), "output": output()}, indent=2) + "\n")
        return 0
    if argv == ["check"]:
        found = problems()
        print("\n".join(found) if found else "web build is up to date", file=sys.stderr)
        return 1 if found else 0
    print(__doc__.strip(), file=sys.stderr)
    return 64


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
