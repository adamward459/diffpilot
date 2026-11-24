#!/usr/bin/env python3
"""Build the git repo that UI tests pass to the app in DIFFPILOT_REPO.

The repo lives in .fixtures/ at the repo root. Xcode test builds give the sandboxed app read access to /.
Branch `main` holds the base files; branch `feature` holds committed changes, and the
working tree holds uncommitted ones. .git/diffpilot-review.json lists viewed files and
comment counts, which git cannot store.

  scripts/fixture_repo.py           build it if the script changed since the last build
  scripts/fixture_repo.py --force   rebuild it
"""

from __future__ import annotations

import hashlib
import json
import random
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent / ".fixtures" / "large"
STAMP = REPO / ".git" / "diffpilot-fixture-version"
GENERATED_COUNT = 1988
CONTEXT_LINES = 3


@dataclass(frozen=True)
class Spec:
    path: str
    status: str
    additions: int
    deletions: int
    viewed: bool = False
    uncommitted: bool = False
    comments: int = 0

    @property
    def old_path(self) -> str:
        folder, _, name = self.path.rpartition("/")
        return f"{folder}/Old{name}" if folder else f"Old{self.path}"


DESIGN = [
    Spec("docs/archive.md", "A", 19, 0, viewed=True),
    Spec("src/api/archive.ts", "M", 38, 6, comments=1),
    Spec("src/api/client.ts", "M", 4, 1, viewed=True),
    Spec("src/components/inbox/ArchiveToast.tsx", "A", 48, 0),
    Spec("src/components/inbox/MessageRow.tsx", "M", 23, 15),
    Spec("src/hooks/useArchiveUndo.ts", "A", 64, 0, uncommitted=True, comments=2),
    Spec("src/lib/queryKeys.ts", "M", 3, 1, viewed=True),
    Spec("src/types/message.ts", "M", 5, 0),
    Spec("tests/api/archive.test.ts", "M", 31, 4, uncommitted=True),
    Spec("tests/hooks/useArchiveUndo.test.ts", "A", 112, 0),
    Spec("package.json", "M", 1, 1),
    Spec("pnpm-lock.yaml", "M", 21, 8),
]


def generated(count: int, taken: set[str]) -> list[Spec]:
    rng = random.Random(0x9E3779B9)
    roots = ["src", "src", "src", "src", "tests", "tests", "docs", "packages", "scripts"]
    areas = ["api", "components", "hooks", "lib", "types", "utils", "store", "features/auth", "views", "models"]
    subs = ["", "", "", "inbox", "compose", "settings", "shared/forms", "legacy"]
    words = ["Message", "Inbox", "Query", "Client", "Thread", "Label", "Filter", "Draft", "Sync", "Cache", "Session"]
    kinds = ["", "Row", "List", "View", "Provider", "Store", "Service", "Helper", "Config"]
    extensions = ["ts", "ts", "tsx", "tsx", "swift", "swift", "md", "json", "yaml"]
    specs: list[Spec] = []
    seen = set(taken)
    while len(specs) < count:
        root = rng.choice(roots)
        parts = [root] if root == "docs" else [root, rng.choice(areas), rng.choice(subs)]
        ext = "md" if root == "docs" else rng.choice(extensions)
        base = rng.choice(words) + rng.choice(kinds)
        path = "/".join([p for p in parts if p] + [f"{base}.{ext}"])
        suffix = 2
        while path in seen:
            path = "/".join([p for p in parts if p] + [f"{base}{suffix}.{ext}"])
            suffix += 1
        seen.add(path)
        roll = rng.randrange(100)
        status = "M" if roll < 60 else "A" if roll < 82 else "D" if roll < 90 else "R"
        size = 1 + rng.randrange(40)
        specs.append(
            Spec(
                path,
                status,
                additions=0 if status == "D" else size,
                deletions=0 if status == "A" else (1 + rng.randrange(14) if status == "D" else rng.randrange(15)),
                viewed=rng.randrange(100) < 25,
                uncommitted=rng.randrange(100) < 12,
                comments=1 + rng.randrange(5) if rng.randrange(100) < 15 else 0,
            )
        )
    return specs


def lines(spec: Spec, kind: str, count: int) -> list[str]:
    return [f"{spec.path} {kind} {i}" for i in range(count)]


def shared_lines(spec: Spec) -> list[str]:
    # Rename detection needs the old and new file to be mostly the same.
    return lines(spec, "shared", 2 * max(spec.additions, spec.deletions) + 10)


def base_content(spec: Spec) -> list[str] | None:
    if spec.status == "A":
        return None
    if spec.status == "R":
        return shared_lines(spec) + lines(spec, "old", spec.deletions)
    if spec.status == "D":
        return lines(spec, "old", spec.deletions)
    return lines(spec, "context", CONTEXT_LINES) + lines(spec, "old", spec.deletions)


def head_content(spec: Spec) -> list[str] | None:
    if spec.status == "D":
        return None
    if spec.status == "R":
        return shared_lines(spec) + lines(spec, "new", spec.additions)
    context = [] if spec.status == "A" else lines(spec, "context", CONTEXT_LINES)
    return context + lines(spec, "new", spec.additions)


def write(path: Path, content: list[str] | None) -> None:
    if content is None:
        path.unlink(missing_ok=True)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(line + "\n" for line in content))


def git(*args: str) -> None:
    subprocess.run(["git", "-C", str(REPO), *args], check=True, capture_output=True)


def apply(specs: list[Spec]) -> None:
    for spec in specs:
        if spec.status == "R":
            (REPO / spec.old_path).unlink()
        write(REPO / spec.path, head_content(spec))


def build(specs: list[Spec]) -> None:
    shutil.rmtree(REPO, ignore_errors=True)
    REPO.mkdir(parents=True)
    git("init", "-q", "-b", "main")
    git("config", "user.name", "diffpilot fixture")
    git("config", "user.email", "fixture@diffpilot.invalid")
    for spec in specs:
        write(REPO / (spec.old_path if spec.status == "R" else spec.path), base_content(spec))
    git("add", "-A")
    git("commit", "-q", "-m", "base")
    git("checkout", "-q", "-b", "feature")
    apply([s for s in specs if not s.uncommitted])
    git("add", "-A")
    git("commit", "-q", "-m", "feature")
    apply([s for s in specs if s.uncommitted])
    git("add", "-A")
    review = {
        "viewed": sorted(s.path for s in specs if s.viewed),
        "comments": {s.path: s.comments for s in specs if s.comments},
    }
    (REPO / ".git" / "diffpilot-review.json").write_text(json.dumps(review, indent=2) + "\n")


def main(argv: list[str]) -> int:
    version = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    if "--force" not in argv and STAMP.exists() and STAMP.read_text() == version:
        return 0
    specs = DESIGN + generated(GENERATED_COUNT, {s.path for s in DESIGN})
    build(specs)
    STAMP.write_text(version)
    print(f"Built {REPO} with {len(specs)} changed files", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
