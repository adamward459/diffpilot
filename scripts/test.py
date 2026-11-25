#!/usr/bin/env python3
"""Run diffpilot checks per Cargo crate.

A crate is a workspace member: core (logic), ui-kit (theme, icons, shared components), diff-pane,
review-ui, gpui-app (app shell, sidebar, comparison). Crates are named by package (diffpilot-core) or folder (core). A change in a crate
also touches the crates that depend on it; a change to the root Cargo files touches all crates.

  scripts/test.py unit [crate...] [--force]     cargo test, in one call for all the crates
  scripts/test.py clippy [crate...] [--force]   cargo clippy --all-targets -- -D warnings
  scripts/test.py hotpath [crate...] [--base REF]
                                           run each crate's DIFFPILOT_BENCH=1 mode and compare it with
                                           baselines/bench.json on the PR base ref (or REF); fail on
                                           metrics more than 5% worse
  scripts/test.py baseline [crate...] [--rounds N]
                                           run each bench mode N times and write its part of
                                           baselines/bench.json
  scripts/test.py screenshot [crate...] [--compare [--base REF]]
                                           capture the app window into screenshots/feature/<crate>/;
                                           --compare also diffs each image with the base ref's copy
  scripts/test.py related                  print the commands that match the current changes
  scripts/test.py lint                     check the workspace layout and the formatting of the scripts

unit and clippy skip a crate that already passed with the same inputs: its sources, the sources of
the workspace crates it depends on, the root Cargo files, the toolchain, and the flags. The pass
records live in ~/Library/Caches/diffpilot/passes, so they are shared by all worktrees.

A bench mode prints one JSON object of numeric metrics on stdout; nested objects are flattened to
dotted names. Hotpath compares only the stable ones, the names ending in median_ms or p95_ms.
They are smaller-is-better milliseconds.
"""

from __future__ import annotations

import fcntl
import hashlib
import json
import os
import platform
import re
import statistics
import struct
import subprocess
import sys
import tempfile
import time
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCREENSHOT_DIR = ROOT / "screenshots" / "feature"
BASELINE = ROOT / "baselines" / "bench.json"
FIXTURE_REPO = ROOT / ".fixtures" / "large"
FIXTURE_VIEWED = ROOT / ".fixtures" / "large-viewed.json"
PASSES = Path.home() / "Library" / "Caches" / "diffpilot" / "passes"
ROOT_FILES = ("Cargo.toml", "Cargo.lock", "rustfmt.toml", "rust-toolchain.toml", "clippy.toml", ".cargo/config.toml")
HOTPATH_TOLERANCE = 0.05
# Medians of 5 samples differ by up to 11% on identical code; 3 alternating rounds cut that to ~3%.
HOTPATH_ROUNDS = 3
BASELINE_ROUNDS = 5
# The frame-callback benches stop getting frames while another window covers theirs.
BENCH_TIMEOUT_S = 180
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
COMMANDS = ("unit", "clippy", "hotpath", "baseline", "screenshot", "related", "lint")


class LintError(Exception):
    pass


@dataclass(frozen=True)
class Crate:
    package: str
    folder: str
    deps: tuple[str, ...]
    features: tuple[str, ...] = ()

    def feature_flags(self) -> list[str]:
        return [f"{self.package}/{HEADLESS_FEATURE}"] if HEADLESS_FEATURE in self.features else []


def sh(*cmd: str, cwd: Path = ROOT) -> str:
    return subprocess.run(cmd, cwd=cwd, check=True, capture_output=True, text=True).stdout


def git(*args: str) -> list[str]:
    return [line for line in sh("git", *args).splitlines() if line]


def metadata() -> dict:
    return json.loads(sh("cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"))


def workspace() -> list[Crate]:
    packages = metadata()["packages"]
    members = {p["name"] for p in packages}
    return sorted(
        (
            Crate(
                p["name"],
                Path(p["manifest_path"]).parent.relative_to(ROOT).as_posix(),
                tuple(d["name"] for d in p["dependencies"] if d["name"] in members),
                tuple(p["features"]),
            )
            for p in packages
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
    subprocess.run(["git", "fetch", "-q", "origin", name], check=False, cwd=ROOT, capture_output=True)
    return f"origin/{name}"


def changed_files(base: str) -> list[str]:
    return sorted(set(git("diff", "--name-only", base)) | set(git("ls-files", "-o", "--exclude-standard")))


def closure(crates: list[Crate], start: list[Crate]) -> list[Crate]:
    by_name = {c.package: c for c in crates}
    seen: dict[str, Crate] = {}
    stack = list(start)
    while stack:
        c = stack.pop()
        if c.package not in seen:
            seen[c.package] = c
            stack.extend(by_name[d] for d in c.deps)
    return sorted(seen.values(), key=lambda c: c.folder)


def touched_crates(crates: list[Crate], files: list[str]) -> list[Crate]:
    touched = {c.package for c in crates for f in files if f.startswith(c.folder + "/")}
    if any(f in ROOT_FILES for f in files):
        touched = {c.package for c in crates}
    return [c for c in crates if touched & {d.package for d in closure(crates, [c])}]


def cargo(*args: str, cwd: Path = ROOT, env: dict | None = None) -> int:
    print("+ cargo " + " ".join(args), file=sys.stderr)
    return subprocess.call(["cargo", *args], cwd=cwd, env={**os.environ, **(env or {})})


def select(crates: list[Crate]) -> list[str]:
    args = [arg for c in crates for arg in ("-p", c.package)]
    features = [f for c in crates for f in c.feature_flags()]
    return args + (["--features", ",".join(features)] if features else [])


def inputs_hash(crates: list[Crate], crate: Crate, check: str) -> str:
    folders = [c.folder for c in closure(crates, [crate])]
    files = sorted(set(sh("git", "ls-files", "-z", "-co", "--exclude-standard", "--", *folders).split("\0")) - {""})
    digest = hashlib.sha256()
    digest.update(f"{check}\0{crate.package}\0{' '.join(crate.feature_flags())}\0".encode())
    digest.update(sh("rustc", "-vV").encode())
    for name in [*ROOT_FILES, *files]:
        path = ROOT / name
        digest.update(name.encode() + b"\0")
        digest.update(path.read_bytes() if path.is_file() else b"\0missing")
    return digest.hexdigest()


def pass_record(check: str, crate: Crate, digest: str) -> Path:
    return PASSES / f"{check}-{crate.package}-{digest}"


def unpassed(crates: list[Crate], chosen: list[Crate], check: str, force: bool) -> dict[str, str]:
    todo = {}
    for c in chosen:
        digest = inputs_hash(crates, c, check)
        if not force and pass_record(check, c, digest).exists():
            print(f"  {check} {c.folder}: skipped (passed at {digest[:12]})", file=sys.stderr)
        else:
            todo[c.package] = digest
    return todo


def record_passes(crates: list[Crate], check: str, todo: dict[str, str]) -> None:
    PASSES.mkdir(parents=True, exist_ok=True)
    for c in crates:
        if c.package in todo:
            pass_record(check, c, todo[c.package]).touch()


@contextmanager
def build_dir_lock():
    # Worktrees share one build-dir and a workspace crate gets the same test binary path in each.
    # Cargo frees its own lock before it runs tests, so another worktree could replace the binary.
    meta = metadata()
    build_dir = Path(meta.get("build_directory") or meta["target_directory"])
    build_dir.mkdir(parents=True, exist_ok=True)
    with open(build_dir / ".diffpilot-test.lock", "w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print(f"waiting for another worktree's tests in {build_dir}", file=sys.stderr)
            fcntl.flock(lock, fcntl.LOCK_EX)
        yield


def unit(crates: list[Crate], chosen: list[Crate], force: bool) -> int:
    todo = unpassed(crates, chosen, "unit", force)
    run = [c for c in chosen if c.package in todo]
    if not run:
        return 0
    with build_dir_lock():
        code = cargo("test", "--no-fail-fast", *select(run))
    if code == 0:
        record_passes(crates, "unit", todo)
    return code


def clippy(crates: list[Crate], chosen: list[Crate], force: bool) -> int:
    todo = unpassed(crates, chosen, "clippy", force)
    run = [c for c in chosen if c.package in todo]
    if not run:
        return 0
    code = cargo("clippy", *select(run), "--all-targets", "--", "-D", "warnings")
    if code == 0:
        record_passes(crates, "clippy", todo)
    return code


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
    try:
        done = subprocess.run(
            ["cargo", "run", "--release", "--quiet", *BENCH_ARGS[crate.package]],
            check=False,
            cwd=cwd,
            env={**os.environ, **bench_env()},
            capture_output=True,
            text=True,
            timeout=BENCH_TIMEOUT_S,
        )
    except subprocess.TimeoutExpired:
        raise RuntimeError(
            f"{crate.folder}: the bench stalled for {BENCH_TIMEOUT_S} s. Its window gets no frames while another "
            "window covers it; keep the bench window visible and run again."
        ) from None
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


def build_benches(benched: list[Crate], cwd: Path) -> int:
    return next((code for c in benched if (code := cargo("build", "--release", *BENCH_ARGS[c.package], cwd=cwd))), 0)


def refresh_rate() -> str:
    out = sh("system_profiler", "SPDisplaysDataType", "-json")
    rates = sorted(
        {
            m.group(1)
            for gpu in json.loads(out).get("SPDisplaysDataType", [])
            for display in gpu.get("spdisplays_ndrvs", [])
            if (m := re.search(r"@ ?([\d.]+ ?Hz)", display.get("_spdisplays_resolution", "")))
        }
    )
    return ", ".join(rates) or "unknown"


def host() -> dict[str, str]:
    return {
        "cpu": sh("sysctl", "-n", "machdep.cpu.brand_string").strip(),
        "macos": platform.mac_ver()[0],
        "refresh_rate": refresh_rate(),
        "rustc": sh("rustc", "-V").strip(),
        "profile": "release",
    }


def warn_if_busy(what: str) -> None:
    load, cores = os.getloadavg()[0], os.cpu_count() or 1
    if load > cores / 2:
        print(f"warning: load average {load:.0f} on {cores} cores; {what} timings will be noisy", file=sys.stderr)


def p95(values: list[float]) -> float:
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, round(0.95 * (len(ordered) - 1)))]


def baseline(crates: list[Crate], rounds: int) -> int:
    benched = [c for c in crates if c.package in BENCH_ARGS]
    ensure_fixture()
    warn_if_busy("baseline")
    if code := build_benches(benched, ROOT):
        return code
    samples: dict[str, dict[str, list[float]]] = {c.folder: {} for c in benched}
    for n in range(rounds):
        for c in benched:
            print(f"== baseline {c.folder} round {n + 1}/{rounds}", file=sys.stderr)
            try:
                metrics = bench_once(c, ROOT)
            except RuntimeError as e:
                print(f"baseline: {e}", file=sys.stderr)
                return 1
            for name, value in metrics.items():
                samples[c.folder].setdefault(name, []).append(value)
    now = host()
    old = json.loads(BASELINE.read_text()) if BASELINE.is_file() else {}
    kept = old.get("metrics", {}) if old.get("host") == now else {}
    fresh = {
        folder: {
            name: {"median": round(statistics.median(v), 4), "p95": round(p95(v), 4), "runs": len(v)}
            for name, v in sorted(by_name.items())
        }
        for folder, by_name in samples.items()
    }
    data = {"host": now, "metrics": dict(sorted({**kept, **fresh}.items()))}
    BASELINE.parent.mkdir(exist_ok=True)
    BASELINE.write_text(json.dumps(data, indent=2) + "\n")
    print(f"wrote {BASELINE.relative_to(ROOT)}; commit it with the PR", file=sys.stderr)
    return 0


def committed_baseline(ref: str) -> tuple[dict | None, str]:
    path = BASELINE.relative_to(ROOT).as_posix()
    done = subprocess.run(["git", "show", f"{ref}:{path}"], check=False, cwd=ROOT, capture_output=True, text=True)
    if done.returncode != 0:
        return None, f"{ref} has no {path}"
    data = json.loads(done.stdout)
    now = host()
    differs = [f"{k} {data['host'].get(k)!r} != {v!r}" for k, v in now.items() if data["host"].get(k) != v]
    if differs:
        return None, f"the baseline on {ref} comes from another host or toolchain: {'; '.join(differs)}"
    return data, ""


def compare(folder: str, head: dict[str, list[float]], base: dict[str, float]) -> int:
    regressions = 0
    for name in sorted(head):
        h = statistics.median(head[name])
        if name not in base:
            print(f"  {folder} {name}: {h:.4g} (new, no baseline)", file=sys.stderr)
            continue
        b = base[name]
        change = (h - b) / b if b else 0.0
        worse = change > HOTPATH_TOLERANCE
        regressions += worse
        verdict = f"REGRESSION (limit {HOTPATH_TOLERANCE:.0%})" if worse else "ok"
        print(f"  {folder} {name}: base {b:.4g} -> head {h:.4g} ({change:+.1%}) {verdict}", file=sys.stderr)
    return regressions


def hotpath(crates: list[Crate], base: str | None) -> int:
    benched = [c for c in crates if c.package in BENCH_ARGS]
    if not benched:
        print("No selected crate has a bench mode.", file=sys.stderr)
        return 0
    ensure_fixture()
    warn_if_busy("hotpath")
    ref = resolve_base(base)
    data, why = committed_baseline(ref)
    missing = [c.folder for c in benched if data is not None and c.folder not in data["metrics"]]
    if missing:
        data, why = None, f"the baseline on {ref} has no {', '.join(missing)}"
    if data is None:
        print(f"== hotpath: building the base, because {why}", file=sys.stderr)
        return hotpath_against_build(benched, ref)
    print(f"== hotpath baseline: {BASELINE.relative_to(ROOT)} on {ref}", file=sys.stderr)
    if code := build_benches(benched, ROOT):
        return code
    regressions = 0
    for c in benched:
        samples: dict[str, list[float]] = {}
        for n in range(HOTPATH_ROUNDS):
            print(f"== hotpath {c.folder} round {n + 1}/{HOTPATH_ROUNDS}", file=sys.stderr)
            try:
                metrics = bench_once(c, ROOT)
            except RuntimeError as e:
                print(f"hotpath: {e}", file=sys.stderr)
                return 1
            for name, value in metrics.items():
                samples.setdefault(name, []).append(value)
        base_medians = {name: m["median"] for name, m in data["metrics"].get(c.folder, {}).items()}
        regressions += compare(c.folder, samples, base_medians)
    if regressions:
        print("== hotpath: worse than the baseline; confirming against a build of the base", file=sys.stderr)
        return hotpath_against_build(benched, ref)
    return 0


def base_worktree(ref: str) -> Path:
    common = Path(git("rev-parse", "--path-format=absolute", "--git-common-dir")[0])
    path = common.parent / ".claude" / "worktrees" / "hotpath-base"
    sha = git("rev-parse", ref)[0]
    if path.exists():
        subprocess.run(["git", "-C", str(path), "checkout", "-q", "-f", "--detach", sha], check=True)
    else:
        subprocess.run(["git", "worktree", "add", "-q", "--detach", str(path), sha], cwd=ROOT, check=True)
    return path


def hotpath_against_build(benched: list[Crate], ref: str) -> int:
    path = base_worktree(ref)
    print(f"== hotpath base build: {ref} at {git('rev-parse', '--short', ref)[0]}", file=sys.stderr)
    sides = (("base", path), ("head", ROOT))
    for side, cwd in sides:
        for c in benched:
            code = cargo("build", "--release", *BENCH_ARGS[c.package], cwd=cwd)
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
            for n in range(HOTPATH_ROUNDS):
                for side, cwd in sides:
                    print(f"== hotpath {c.folder} round {n + 1}/{HOTPATH_ROUNDS}: {side}", file=sys.stderr)
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
        base_medians = {name: statistics.median(v) for name, v in samples["base"].items()}
        regressions += compare(c.folder, samples["head"], base_medians)
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
        ["osascript", "-l", "JavaScript", "-e", WINDOW_JXA % pid], check=False, capture_output=True, text=True
    ).stdout.strip()
    return int(out) if out.isdigit() else 0


def capture_scene(binary: Path, appearance: str, out: Path) -> None:
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


def target_dir() -> Path:
    return Path(metadata()["target_directory"])


def screenshot(crates: list[Crate]) -> int:
    chosen = [c for c in crates if c.package in SCREENSHOT_PACKAGES]
    if not chosen:
        print("No selected crate has a window to capture.", file=sys.stderr)
        return 0
    ensure_fixture()
    if cargo("build", "--release", "-p", APP_PACKAGE) != 0:
        return 1
    binary = target_dir() / "release" / APP_PACKAGE
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


@dataclass(frozen=True)
class Bitmap:
    width: int
    height: int
    rows: list[bytes]


def read_bitmap(png: Path) -> Bitmap:
    with tempfile.TemporaryDirectory() as tmp:
        bmp = Path(tmp) / "image.bmp"
        sh("sips", "-s", "format", "bmp", str(png), "--out", str(bmp))
        data = bmp.read_bytes()
    offset, width, height, bits = (
        struct.unpack_from("<I", data, 10)[0],
        *struct.unpack_from("<ii", data, 18),
        struct.unpack_from("<H", data, 28)[0],
    )
    if bits != 32:
        raise RuntimeError(f"{png}: expected 32-bit pixels from sips, got {bits}")
    stride = width * 4
    rows = [data[offset + y * stride : offset + (y + 1) * stride] for y in range(abs(height))]
    return Bitmap(width, abs(height), rows if height < 0 else rows[::-1])


def write_png(bitmap: Bitmap, out: Path) -> None:
    header = struct.pack("<2sIHHI", b"BM", 54 + bitmap.height * bitmap.width * 4, 0, 0, 54)
    info = struct.pack("<IiiHHIIiiII", 40, bitmap.width, -bitmap.height, 1, 32, 0, 0, 2835, 2835, 0, 0)
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        bmp = Path(tmp) / "diff.bmp"
        bmp.write_bytes(header + info + b"".join(bitmap.rows))
        sh("sips", "-s", "format", "png", str(bmp), "--out", str(out))


DIM = bytes(x // 3 for x in range(256))


@dataclass(frozen=True)
class PixelDiff:
    image: str
    changed: int
    total: int
    max_delta: int

    def __str__(self) -> str:
        share = 100 * self.changed / self.total
        return f"{self.image}: {self.changed} pixels changed ({share:.3g}%), max channel delta {self.max_delta}"


def diff_images(old: Bitmap, new: Bitmap, image: str, out: Path) -> PixelDiff | None:
    total = new.width * new.height
    if (old.width, old.height) != (new.width, new.height):
        return PixelDiff(f"{image} (size {old.width}x{old.height} -> {new.width}x{new.height})", total, total, 255)
    if old.rows == new.rows:
        return None
    changed = max_delta = 0
    marked = []
    for a, b in zip(old.rows, new.rows):
        row = bytearray(b.translate(DIM))
        row[3::4] = b"\xff" * new.width
        if a != b:
            for x in range(0, len(a), 4):
                if a[x : x + 4] != b[x : x + 4]:
                    changed += 1
                    max_delta = max(max_delta, *(abs(a[x + i] - b[x + i]) for i in range(4)))
                    row[x : x + 4] = b"\x00\x00\xff\xff"
        marked.append(bytes(row))
    write_png(Bitmap(new.width, new.height, marked), out)
    return PixelDiff(image, changed, total, max_delta)


def compare_screenshots(crates: list[Crate], ref: str) -> list[str]:
    lines = []
    diff_root = target_dir() / "screenshot-diff"
    for c in crates:
        if c.package not in SCREENSHOT_PACKAGES:
            continue
        for shot in sorted((SCREENSHOT_DIR / c.folder).glob("*.png")):
            path = shot.relative_to(ROOT).as_posix()
            done = subprocess.run(["git", "show", f"{ref}:{path}"], check=False, cwd=ROOT, capture_output=True)
            if done.returncode != 0:
                lines.append(f"{path}: new, not on {ref}")
                continue
            with tempfile.NamedTemporaryFile(suffix=".png") as old:
                old.write(done.stdout)
                old.flush()
                out = diff_root / c.folder / shot.name
                found = diff_images(read_bitmap(Path(old.name)), read_bitmap(shot), path, out)
            if found:
                lines.append(f"{found} (diff: {out})")
    return lines


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
    for check in ("unit", "clippy"):
        todo = unpassed(crates, touched, check, force=False)
        if todo:
            print(f"  scripts/test.py {check} {' '.join(c.folder for c in touched if c.package in todo)}")
    for c in touched:
        if c.package in BENCH_ARGS:
            print(f"  scripts/test.py hotpath {c.folder}")
    if py:
        print("  scripts/test.py lint")
    if any(c.package in BENCH_ARGS for c in touched):
        print("Before you open or update the PR (then commit baselines/bench.json):")
        print("  scripts/test.py baseline")
    if screenshot_crates(touched):
        print("After you open or update the PR:")
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
        ["uvx", "ruff", "format", "--check", "scripts", ".githooks"],
        check=False,
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        raise LintError("ruff format check failed:\n" + (done.stdout + done.stderr).strip())


def take_option(rest: list[str], name: str) -> tuple[str | None, list[str]]:
    if name not in rest:
        return None, rest
    i = rest.index(name)
    return rest[i + 1], rest[:i] + rest[i + 2 :]


def take_flag(rest: list[str], name: str) -> tuple[bool, list[str]]:
    return name in rest, [arg for arg in rest if arg != name]


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
        base, rest = take_option(rest, "--base")
        rounds, rest = take_option(rest, "--rounds")
        force, rest = take_flag(rest, "--force")
        compare_flag, rest = take_flag(rest, "--compare")
        crates = workspace()
        chosen = resolve(crates, rest)
        if command == "unit":
            return unit(crates, chosen, force)
        if command == "clippy":
            return clippy(crates, chosen, force)
        if command == "hotpath":
            return hotpath(chosen, base)
        if command == "baseline":
            return baseline(chosen, int(rounds or BASELINE_ROUNDS))
        code = screenshot(chosen)
        if code == 0 and compare_flag:
            ref = resolve_base(base)
            changed = compare_screenshots(chosen, ref)
            print(f"== screenshots changed vs {ref}: {len(changed)}", file=sys.stderr)
            for line in changed:
                print(f"  {line}")
        return code
    except LintError as e:
        print(f"lint: {e}".replace("\n", "\nlint: "), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
