#!/usr/bin/env python3
"""Run diffpilot checks per Cargo crate.

A crate is a workspace member: core (logic), ui-kit (theme, icons, shared components), diff-pane,
review-ui, gpui-app (app shell, sidebar, comparison). Crates are named by package (diffpilot-core) or folder (core). A change in a crate
also touches the crates that depend on it; a change to the root Cargo files touches all crates.

  scripts/test.py unit [crate...]          cargo test (all crates when none named)
  scripts/test.py clippy [crate...]        cargo clippy --all-targets -- -D warnings
  scripts/test.py hotpath [crate...] [--base REF]
                                           run each crate's DIFFPILOT_BENCH=1 mode here and on the PR
                                           base ref (or REF); fail on metrics more than 5% worse
  scripts/test.py screenshot [crate...]    capture the app window into screenshots/feature/<crate>/
  scripts/test.py related                  print the commands that match the current changes
  scripts/test.py lint                     check the workspace layout and the formatting of the scripts

A bench mode prints one JSON object of numeric metrics on stdout; nested objects are flattened to
dotted names. Hotpath compares only the stable ones, the names ending in median_ms or p95_ms.
They are smaller-is-better milliseconds.
"""

from __future__ import annotations

import json
import os
import statistics
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCREENSHOT_DIR = ROOT / "screenshots" / "feature"
FIXTURE_REPO = ROOT / ".fixtures" / "large"
FIXTURE_VIEWED = ROOT / ".fixtures" / "large-viewed.json"
ROOT_FILES = ("Cargo.toml", "Cargo.lock", "rustfmt.toml", "rust-toolchain.toml")
HOTPATH_TOLERANCE = 0.05
# Medians of 5 samples differ by up to 11% on identical code; 3 alternating rounds cut that to ~3%.
HOTPATH_ROUNDS = 3
# Medians and 95th percentiles settle between runs. Maxima, over-budget counts and interval counts swing 20-50%.
STABLE_METRICS = ("median_ms", "p95_ms")
HEADLESS_FEATURE = "headless-tests"
APP_PACKAGE = "diffpilot"
BENCH_ARGS = {
    "diffpilot-diff-pane": ["-p", "diffpilot-diff-pane", "--example", "diff_pane"],
    "diffpilot-review-ui": ["-p", "diffpilot-review-ui", "--example", "review"],
    APP_PACKAGE: ["-p", APP_PACKAGE],
}
SCREENSHOT_PACKAGES = (APP_PACKAGE,)
SCENES = (("light", "light"), ("dark", "dark"))
COMMANDS = ("unit", "clippy", "hotpath", "screenshot", "related", "lint")


class LintError(Exception):
    pass


@dataclass(frozen=True)
class Crate:
    package: str
    folder: str
    deps: tuple[str, ...]
    features: tuple[str, ...] = ()

    def test_flags(self) -> list[str]:
        return ["--features", HEADLESS_FEATURE] if HEADLESS_FEATURE in self.features else []


def sh(*cmd: str, cwd: Path = ROOT) -> str:
    return subprocess.run(cmd, cwd=cwd, check=True, capture_output=True, text=True).stdout


def git(*args: str) -> list[str]:
    return [line for line in sh("git", *args).splitlines() if line]


def workspace() -> list[Crate]:
    meta = json.loads(sh("cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"))
    members = {p["name"] for p in meta["packages"]}
    return sorted(
        (
            Crate(
                p["name"],
                Path(p["manifest_path"]).parent.relative_to(ROOT).as_posix(),
                tuple(d["name"] for d in p["dependencies"] if d["name"] in members),
                tuple(p["features"]),
            )
            for p in meta["packages"]
        ),
        key=lambda c: c.folder,
    )


def resolve(crates: list[Crate], names: list[str]) -> list[Crate]:
    if not names:
        return crates
    out = []
    for name in names:
        match = [c for c in crates if name in (c.package, c.folder)]
        if not match:
            raise LintError(f"unknown crate {name!r}; known: {', '.join(c.folder for c in crates)}")
        out.append(match[0])
    return out


def upstream() -> str:
    try:
        return git("rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}")[0]
    except subprocess.CalledProcessError:
        return git("merge-base", "HEAD", resolve_base(None))[0]


def resolve_base(explicit: str | None) -> str:
    if explicit:
        return explicit
    try:
        name = sh("gh", "pr", "view", "--json", "baseRefName", "-q", ".baseRefName").strip()
    except (OSError, subprocess.CalledProcessError):
        name = "main"
    subprocess.run(["git", "fetch", "-q", "origin", name], cwd=ROOT, capture_output=True)
    return f"origin/{name}"


def changed_files(base: str) -> list[str]:
    return sorted(set(git("diff", "--name-only", base)) | set(git("ls-files", "-o", "--exclude-standard")))


def touched_crates(crates: list[Crate], files: list[str]) -> list[Crate]:
    touched = {c.package for c in crates for f in files if f.startswith(c.folder + "/")}
    if any(f in ROOT_FILES for f in files):
        touched = {c.package for c in crates}
    grew = True
    while grew:
        grew = False
        for c in crates:
            if c.package not in touched and touched & set(c.deps):
                touched.add(c.package)
                grew = True
    return [c for c in crates if c.package in touched]


def cargo(*args: str, cwd: Path = ROOT, env: dict | None = None) -> int:
    print("+ cargo " + " ".join(args), file=sys.stderr)
    return subprocess.call(["cargo", *args], cwd=cwd, env={**os.environ, **(env or {})})


def run_each(crates: list[Crate], *args: str) -> int:
    failed = [c.folder for c in crates if cargo(*args, "-p", c.package, *c.test_flags()) != 0]
    if failed:
        print(f"failed: {', '.join(failed)}", file=sys.stderr)
    return 1 if failed else 0


def clippy(crates: list[Crate]) -> int:
    failed = [
        c.folder
        for c in crates
        if cargo("clippy", "-p", c.package, *c.test_flags(), "--all-targets", "--", "-D", "warnings") != 0
    ]
    if failed:
        print(f"failed: {', '.join(failed)}", file=sys.stderr)
    return 1 if failed else 0


def ensure_fixture(root: Path = ROOT) -> None:
    subprocess.run([sys.executable, str(root / "scripts" / "fixture_repo.py")], check=True)


def bench_env() -> dict:
    return {
        "DIFFPILOT_BENCH": "1",
        "DIFFPILOT_REPO": str(FIXTURE_REPO),
        "DIFFPILOT_VIEWED_SEED": str(FIXTURE_VIEWED),
    }


def flatten(value: object, prefix: str = "") -> dict[str, float]:
    if isinstance(value, bool):
        return {}
    if isinstance(value, (int, float)):
        return {prefix: float(value)}
    if isinstance(value, dict):
        out: dict[str, float] = {}
        for key, item in value.items():
            out.update(flatten(item, f"{prefix}.{key}" if prefix else key))
        return out
    return {}


def bench_once(crate: Crate, cwd: Path) -> dict[str, float]:
    done = subprocess.run(
        ["cargo", "run", "--release", "--quiet", *BENCH_ARGS[crate.package]],
        cwd=cwd,
        env={**os.environ, **bench_env()},
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        raise RuntimeError(
            f"{crate.folder}: bench run failed (exit {done.returncode}). Does it support DIFFPILOT_BENCH=1?\n"
            + done.stderr.strip()[-2000:]
        )
    try:
        metrics = flatten(json.loads(done.stdout))
    except json.JSONDecodeError:
        raise RuntimeError(f"{crate.folder}: DIFFPILOT_BENCH=1 did not print a JSON object on stdout") from None
    stable = {name: value for name, value in metrics.items() if name.endswith(STABLE_METRICS)}
    if not stable:
        raise RuntimeError(
            f"{crate.folder}: DIFFPILOT_BENCH=1 printed no median or p95 metrics (names ending {', '.join(STABLE_METRICS)})"
        )
    return stable


def base_worktree(ref: str) -> Path:
    common = Path(git("rev-parse", "--path-format=absolute", "--git-common-dir")[0])
    path = common.parent / ".claude" / "worktrees" / "hotpath-base"
    sha = git("rev-parse", ref)[0]
    if path.exists():
        subprocess.run(["git", "-C", str(path), "checkout", "-q", "-f", "--detach", sha], check=True)
    else:
        subprocess.run(["git", "worktree", "add", "-q", "--detach", str(path), sha], cwd=ROOT, check=True)
    return path


def hotpath(crates: list[Crate], base: str | None) -> int:
    benched = [c for c in crates if c.package in BENCH_ARGS]
    if not benched:
        print("No selected crate has a bench mode.", file=sys.stderr)
        return 0
    ensure_fixture()
    ref = resolve_base(base)
    path = base_worktree(ref)
    print(f"== hotpath baseline: {ref} at {git('rev-parse', '--short', ref)[0]}", file=sys.stderr)
    sides = (("base", path), ("head", ROOT))
    for side, cwd in sides:
        for c in benched:
            code = subprocess.call(["cargo", "build", "--release", *BENCH_ARGS[c.package]], cwd=cwd)
            if code != 0:
                if side == "base":
                    print(f"base cannot build {c.folder}; no baseline for it", file=sys.stderr)
                    continue
                return code

    regressions = 0
    for c in benched:
        samples: dict[str, dict[str, list[float]]] = {"base": {}, "head": {}}
        # Alternate base and head so slow drift on the machine hits both sides equally.
        try:
            for round in range(HOTPATH_ROUNDS):
                for side, cwd in sides:
                    print(f"== hotpath {c.folder} round {round + 1}/{HOTPATH_ROUNDS}: {side}", file=sys.stderr)
                    try:
                        metrics = bench_once(c, cwd)
                    except RuntimeError as e:
                        if side == "head":
                            raise
                        print(f"base has no usable bench for {c.folder}: {e}", file=sys.stderr)
                        break
                    for name, value in metrics.items():
                        samples[side].setdefault(name, []).append(value)
        except RuntimeError as e:
            print(f"hotpath: {e}", file=sys.stderr)
            return 1
        for name in sorted(samples["head"]):
            h = statistics.median(samples["head"][name])
            if name not in samples["base"]:
                print(f"  {c.folder} {name}: {h:.4g} (new, no baseline)", file=sys.stderr)
                continue
            b = statistics.median(samples["base"][name])
            change = (h - b) / b if b else 0.0
            worse = change > HOTPATH_TOLERANCE
            regressions += worse
            verdict = f"REGRESSION (limit {HOTPATH_TOLERANCE:.0%})" if worse else "ok"
            print(f"  {c.folder} {name}: base {b:.4g} -> head {h:.4g} ({change:+.1%}) {verdict}", file=sys.stderr)
    return 1 if regressions else 0


WINDOW_JXA = """
ObjC.import("CoreGraphics");
const pid = %d;
const list = ObjC.castRefToObject($.CGWindowListCopyWindowInfo(1, 0));
let best = 0, area = 0;
for (let i = 0; i < list.count; i++) {
  const w = ObjC.deepUnwrap(list.objectAtIndex(i));
  if (w.kCGWindowOwnerPID !== pid || w.kCGWindowLayer !== 0) continue;
  const a = w.kCGWindowBounds.Width * w.kCGWindowBounds.Height;
  if (a > area) { best = w.kCGWindowNumber; area = a; }
}
best
"""


def window_id(pid: int) -> int:
    out = subprocess.run(
        ["osascript", "-l", "JavaScript", "-e", WINDOW_JXA % pid], capture_output=True, text=True
    ).stdout.strip()
    return int(out) if out.isdigit() else 0


def capture_scene(binary: Path, appearance: str, out: Path) -> None:
    import time

    env = {
        **os.environ,
        "DIFFPILOT_REPO": str(FIXTURE_REPO),
        "DIFFPILOT_VIEWED_SEED": str(FIXTURE_VIEWED),
        "DIFFPILOT_APPEARANCE": appearance,
        "DIFFPILOT_REVIEWER": "scripted",
    }
    app = subprocess.Popen([str(binary)], cwd=ROOT, env=env)
    try:
        deadline = time.time() + 60
        wid = 0
        while not wid and time.time() < deadline and app.poll() is None:
            time.sleep(0.5)
            wid = window_id(app.pid)
        if not wid:
            raise RuntimeError("the app showed no window within 60 s (or exited early)")
        time.sleep(3)
        out.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["screencapture", "-x", "-o", "-l", str(wid), str(out)], check=True)
    finally:
        app.terminate()
        app.wait(timeout=10)


def screenshot(crates: list[Crate]) -> int:
    chosen = [c for c in crates if c.package in SCREENSHOT_PACKAGES]
    if not chosen:
        print("No selected crate has a window to capture.", file=sys.stderr)
        return 0
    ensure_fixture()
    if cargo("build", "--release", "-p", APP_PACKAGE) != 0:
        return 1
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    binary = target / "release" / APP_PACKAGE
    for c in chosen:
        for name, appearance in SCENES:
            out = SCREENSHOT_DIR / c.folder / f"{name}.png"
            print(f"== screenshot {c.folder}/{name}", file=sys.stderr)
            try:
                capture_scene(binary, appearance, out)
            except (RuntimeError, subprocess.CalledProcessError) as e:
                print(f"screenshot: {e}. screencapture needs Screen Recording permission.", file=sys.stderr)
                return 1
    return 0


def screenshot_crates(crates: list[Crate]) -> list[str]:
    return [c.folder for c in crates if c.package in SCREENSHOT_PACKAGES]


def related() -> None:
    crates = workspace()
    files = changed_files(upstream())
    touched = touched_crates(crates, files)
    py = [f for f in files if f.endswith(".py")]
    if not touched and not py:
        print("No changes in crates or scripts.")
        return
    print(f"Touched crates: {' '.join(c.folder for c in touched) or 'none'}")
    for c in touched:
        flags = "".join(f" {flag}" for flag in c.test_flags())
        print(f"  cargo test -p {c.package}{flags}")
        print(f"  cargo clippy -p {c.package}{flags} --all-targets -- -D warnings")
    for c in touched:
        if c.package in BENCH_ARGS:
            print(f"  scripts/test.py hotpath {c.folder}")
    if py:
        print("  scripts/test.py lint")
    if screenshot_crates(touched):
        print("After you open the PR:")
        print("  scripts/pr_screenshots.py")


def lint() -> None:
    crates = workspace()
    packages = {c.package for c in crates}
    for table, name in ((BENCH_ARGS, "BENCH_ARGS"), ({p: 1 for p in SCREENSHOT_PACKAGES}, "SCREENSHOT_PACKAGES")):
        for stale in sorted(set(table) - packages):
            raise LintError(f"{name} names {stale}, which is not a workspace member")
    if not (ROOT / "rustfmt.toml").is_file():
        raise LintError("rustfmt.toml is missing")
    done = subprocess.run(
        ["uvx", "ruff", "format", "--check", "scripts", ".githooks"], cwd=ROOT, capture_output=True, text=True
    )
    if done.returncode != 0:
        raise LintError("ruff format check failed:\n" + (done.stdout + done.stderr).strip())


def main(argv: list[str]) -> int:
    if not argv or argv[0] not in COMMANDS:
        print(__doc__.strip(), file=sys.stderr)
        return 64
    command, rest = argv[0], argv[1:]
    try:
        if command == "lint":
            lint()
            print(f"Workspace layout and ruff OK ({len(workspace())} crates)", file=sys.stderr)
            return 0
        if command == "related":
            related()
            return 0
        base = None
        if "--base" in rest:
            i = rest.index("--base")
            base, rest = rest[i + 1], rest[:i] + rest[i + 2 :]
        crates = resolve(workspace(), rest)
        if command == "unit":
            return run_each(crates, "test")
        if command == "clippy":
            return clippy(crates)
        if command == "hotpath":
            return hotpath(crates, base)
        return screenshot(crates)
    except LintError as e:
        print(f"lint: {e}".replace("\n", "\nlint: "), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
