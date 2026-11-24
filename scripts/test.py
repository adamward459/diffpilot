#!/usr/bin/env python3
"""Run diffpilot tests grouped by feature folder.

A feature is a folder diffpilot/Features/<Feature>/. App/ and Shared/ sit next to
Features/ and count as features too. Unit tests mirror the source path in diffpilotTests/
(diffpilotTests/Features/<Feature>/) and e2e tests in diffpilotUITests/.
Test types whose name contains "Performance" are hot path tests; they run under
`hotpath` and never under `unit` or `e2e`. UI test types whose name contains "Screenshot" capture the
feature's screenshots into screenshots/feature/<Feature>/. Shared/ is a feature too,
and a change in it touches every feature.

  scripts/test.py unit [feature...]      run unit tests (all features when none named)
  scripts/test.py e2e [feature...]       run e2e UI tests
  scripts/test.py hotpath [feature...] [--base REF]
                                         run hot path tests here and on the PR base branch
                                         (or REF) and fail on metrics more than 5% worse
  scripts/test.py screenshot [feature...] capture screenshots into screenshots/feature/<Feature>/
  scripts/test.py related                print the test commands that match the current changes
  scripts/test.py lint                   check the folder layout, features.json, that the committed web build is current, and that web/ type checks
"""

from __future__ import annotations

import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import web_stamp  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "features.json"
SOURCE_DIR = "diffpilot"
SHARED = "Shared"
FEATURES_DIR = "Features"
TOP_LEVEL_FEATURES = ("App", SHARED)
KIND_TARGETS = {"unit": "diffpilotTests", "e2e": "diffpilotUITests"}
PERF_MARKER = "Performance"
HOTPATH_TOLERANCE = 0.05
# Medians of 5 launch samples differ by up to 11% on identical code; 15 samples cut that to ~3%.
HOTPATH_ROUNDS = 3
SCREENSHOT_MARKER = "Screenshot"
SCREENSHOT_DIR = ROOT / "screenshots" / "feature"
NA_KINDS = (*KIND_TARGETS, "screenshot")
TOP_LEVEL_TYPE = re.compile(
    r"^(?:(?:public|internal|private|fileprivate|final|open)\s+)*(?:class|struct|actor)\s+(\w+)([^{\n]*)",
    re.M,
)


class LintError(Exception):
    pass


@dataclass(frozen=True)
class TestType:
    target: str
    feature: str
    name: str
    path: str
    body: str

    @property
    def is_perf(self) -> bool:
        return PERF_MARKER in self.name

    @property
    def is_screenshot(self) -> bool:
        return SCREENSHOT_MARKER in self.name

    @property
    def test_id(self) -> str:
        return f"{self.target}/{self.name}"


def git(*args: str) -> list[str]:
    out = subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout
    return [line for line in out.splitlines() if line]


def swift_files() -> list[str]:
    return [f for f in git("ls-files", "-co", "--exclude-standard") if f.endswith(".swift") and (ROOT / f).is_file()]


def upstream() -> str:
    try:
        return git("rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}")[0]
    except subprocess.CalledProcessError:
        return "HEAD"


def changed_files(base: str) -> list[str]:
    scope = [SOURCE_DIR, *KIND_TARGETS.values()]
    return sorted(
        set(git("diff", "--name-only", base, "--", *scope))
        | set(git("ls-files", "-o", "--exclude-standard", "--", *scope))
    )


def folder_of(path: str) -> str | None:
    parts = path.split("/")
    if len(parts) > 3 and parts[1] == FEATURES_DIR:
        return parts[2]
    if len(parts) > 2 and parts[1] in TOP_LEVEL_FEATURES:
        return parts[1]
    return None


def feature_dir(top: str, feature: str) -> str:
    return f"{top}/{feature}/" if feature in TOP_LEVEL_FEATURES else f"{top}/{FEATURES_DIR}/{feature}/"


def features(files: list[str]) -> list[str]:
    return sorted({folder_of(f) for f in files if f.startswith(SOURCE_DIR + "/") and folder_of(f)})


def test_types(files: list[str]) -> list[TestType]:
    found = []
    for f in files:
        target = f.split("/")[0]
        if target not in KIND_TARGETS.values() or not folder_of(f):
            continue
        text = (ROOT / f).read_text(errors="ignore")
        decls = list(TOP_LEVEL_TYPE.finditer(text))
        for i, m in enumerate(decls):
            body = text[m.start() : decls[i + 1].start() if i + 1 < len(decls) else len(text)]
            if "XCTestCase" in m.group(2) or "@Test" in body:
                found.append(TestType(target, folder_of(f), m.group(1), f, body))
    return found


def load_manifest() -> dict:
    manifest = json.loads(MANIFEST.read_text())
    manifest.setdefault("na", {})
    return manifest


def functional_tests(types: list[TestType], feature: str, kind: str) -> list[str]:
    return sorted(
        t.test_id
        for t in types
        if t.feature == feature and t.target == KIND_TARGETS[kind] and not t.is_perf and not t.is_screenshot
    )


def screenshot_tests(types: list[TestType], feature: str) -> list[str]:
    return sorted(t.test_id for t in types if t.feature == feature and t.is_screenshot)


def hotpath_tests(types: list[TestType], feature: str) -> list[str]:
    return sorted(t.test_id for t in types if t.feature == feature and t.is_perf)


def lint(manifest: dict, files: list[str], types: list[TestType]) -> None:
    errors: list[str] = []
    feats = features(files)

    for f in files:
        top = f.split("/")[0]
        if top in (SOURCE_DIR, *KIND_TARGETS.values()):
            folder = folder_of(f)
            if folder is None:
                errors.append(f"{f}: move it into {top}/{FEATURES_DIR}/<Feature>/, or {top}/App/ or {top}/{SHARED}/")
            elif top != SOURCE_DIR and folder not in feats:
                errors.append(f"{f}: no source folder {feature_dir(SOURCE_DIR, folder)} for this test folder")

    for feature, reasons in manifest["na"].items():
        if feature not in feats:
            errors.append(f'features.json na: unknown feature "{feature}"')
            continue
        for kind, reason in reasons.items():
            if kind not in NA_KINDS:
                errors.append(f'features.json na.{feature}: unknown kind "{kind}" (use {", ".join(NA_KINDS)})')
            elif not isinstance(reason, str) or not reason.strip():
                errors.append(f"features.json na.{feature}.{kind}: give a reason")

    for feature in feats:
        for kind, target in KIND_TARGETS.items():
            has_tests = bool(functional_tests(types, feature, kind))
            has_na = kind in manifest["na"].get(feature, {})
            if not has_tests and not has_na:
                errors.append(
                    f"{feature} has no {kind} tests: add them in {feature_dir(target, feature)}, "
                    f"or set na.{feature}.{kind} in features.json with a reason"
                )
            if has_tests and has_na:
                errors.append(f"{feature} has {kind} tests, so remove na.{feature}.{kind} from features.json")

        has_shots = bool(screenshot_tests(types, feature))
        has_na = "screenshot" in manifest["na"].get(feature, {})
        if not has_shots and not has_na:
            errors.append(
                f"{feature} has no screenshot test: add a *{SCREENSHOT_MARKER}UITests type in "
                f"{feature_dir(KIND_TARGETS['e2e'], feature)}, or set na.{feature}.screenshot in features.json with a reason"
            )
        if has_shots and has_na:
            errors.append(f"{feature} has a screenshot test, so remove na.{feature}.screenshot from features.json")

    for t in types:
        if t.is_screenshot and t.target != KIND_TARGETS["e2e"]:
            errors.append(f"{t.path}: {t.name} must live in {KIND_TARGETS['e2e']} to capture the app")

    if "hotpaths" in manifest:
        errors.append(
            "features.json: remove hotpaths; hot path tests now come from *Performance* types in each feature"
        )

    if errors:
        raise LintError("\n".join(errors))


def run_kind(kind: str, names: list[str], base: str | None = None) -> int:
    manifest, files = load_manifest(), swift_files()
    types = test_types(files)
    lint(manifest, files, types)
    feats = features(files)
    unknown = [n for n in names if n not in feats]
    if unknown:
        raise LintError(f"unknown feature(s): {', '.join(unknown)}; features are {', '.join(feats)}")
    names = names or feats
    pickers = {
        "hotpath": lambda n: hotpath_tests(types, n),
        "screenshot": lambda n: screenshot_tests(types, n),
    }
    pick = pickers.get(kind, lambda n: functional_tests(types, n, kind))
    tests = sorted({t for n in names for t in pick(n)})
    if not tests:
        print(f"No {kind} tests for: {' '.join(names)}", file=sys.stderr)
        return 0

    if kind != "unit":
        subprocess.run([sys.executable, str(ROOT / "scripts" / "fixture_repo.py")], check=True)
    print(f"== {kind}: {' '.join(tests)}", file=sys.stderr)
    code, bundle = xcode_test(ROOT, tests, kind)
    if code != 0:
        print(f"Failure details: xcrun xcresulttool get test-results tests --path {bundle}", file=sys.stderr)
    elif kind == "screenshot":
        export_screenshots(bundle, types, names)
    elif kind == "hotpath":
        return compare_with_base(bundle, types, base)
    return code


def xcode_test(cwd: Path, tests: list[str], kind: str) -> tuple[int, Path]:
    args = [f"-only-testing:{t}" for t in tests]
    env = dict(os.environ)
    if kind == "hotpath":
        # Serial, so parallel test clones do not skew the numbers.
        args += ["-parallel-testing-enabled", "NO"]
        # xcodebuild passes TEST_RUNNER_* to the test process without the prefix.
        env["TEST_RUNNER_DIFFPILOT_HOTPATH"] = "1"
    bundle = Path(tempfile.mkdtemp(prefix=f"diffpilot-{kind}-")) / "result.xcresult"
    code = subprocess.run(
        [
            "xcodebuild",
            "test",
            "-quiet",
            "-project",
            "diffpilot.xcodeproj",
            "-scheme",
            "diffpilot",
            "-destination",
            f"platform=macOS,arch={platform.machine()}",
            "-resultBundlePath",
            str(bundle),
            *args,
        ],
        cwd=cwd,
        env=env,
        stderr=subprocess.PIPE,
        text=True,
    )
    noise = re.compile(r"\[MT\] (IDELaunchParametersSnapshot|IDETestOperationsObserverDebug|IDERunDestination)")
    sys.stderr.write("".join(line for line in code.stderr.splitlines(True) if not noise.search(line)))
    return code.returncode, bundle


def read_metrics(bundle: Path) -> dict[tuple[str, str], dict]:
    out = subprocess.run(
        ["xcrun", "xcresulttool", "get", "test-results", "metrics", "--path", str(bundle)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    metrics = {}
    for test in json.loads(out or "[]"):
        for run in test["testRuns"]:
            for m in run["metrics"]:
                if m["measurements"]:
                    metrics[(test["testIdentifier"], m["displayName"])] = {
                        "measurements": m["measurements"],
                        "unit": m.get("unitOfMeasurement", ""),
                        "smaller_is_better": m.get("polarity") != "prefers larger",
                    }
    return metrics


def pool(into: dict, metrics: dict) -> None:
    for key, m in metrics.items():
        into.setdefault(key, {**m, "measurements": []})["measurements"] += m["measurements"]


def resolve_base(explicit: str | None) -> str:
    if explicit:
        return explicit
    try:
        name = subprocess.run(
            ["gh", "pr", "view", "--json", "baseRefName", "-q", ".baseRefName"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        name = "main"
    subprocess.run(["git", "fetch", "-q", "origin", name], cwd=ROOT, capture_output=True)
    return f"origin/{name}"


def base_worktree(ref: str) -> Path:
    common = Path(git("rev-parse", "--path-format=absolute", "--git-common-dir")[0])
    path = common.parent / ".claude" / "worktrees" / "hotpath-base"
    sha = git("rev-parse", ref)[0]
    if path.exists():
        subprocess.run(["git", "-C", str(path), "checkout", "-q", "-f", "--detach", sha], check=True)
    else:
        subprocess.run(["git", "worktree", "add", "-q", "--detach", str(path), sha], cwd=ROOT, check=True)
    return path


def compare_with_base(head_bundle: Path, types: list[TestType], base: str | None) -> int:
    head: dict = {}
    pool(head, read_metrics(head_bundle))
    if not head:
        print("No measured metrics; nothing to compare with the base branch.", file=sys.stderr)
        return 0

    target_of = {t.name: t.target for t in types}
    tests = sorted({f"{target_of[tid.split('/')[0]]}/{tid.split('/')[0]}" for tid, _ in head})
    ref = resolve_base(base)
    path = base_worktree(ref)
    print(f"== hotpath baseline: {ref} at {git('rev-parse', '--short', ref)[0]}", file=sys.stderr)

    baseline: dict = {}
    # Alternate base and head so slow drift on the machine hits both sides equally.
    for round in range(HOTPATH_ROUNDS):
        for side, cwd, into in (("base", path, baseline), ("head", ROOT, head)):
            if side == "head" and round == 0:
                continue
            print(f"== hotpath round {round + 1}/{HOTPATH_ROUNDS}: {side}", file=sys.stderr)
            code, bundle = xcode_test(cwd, tests, "hotpath")
            if code != 0:
                print(f"{side} run failed, so there is nothing to compare: {bundle}", file=sys.stderr)
                return code
            pool(into, read_metrics(bundle))

    regressions = 0
    for key in sorted(head):
        test_id, name = key
        h = statistics.median(head[key]["measurements"])
        unit = head[key]["unit"]
        if key not in baseline:
            print(f"  {test_id} {name}: {h:.4g} {unit} (new, no baseline)", file=sys.stderr)
            continue
        b = statistics.median(baseline[key]["measurements"])
        change = (h - b) / b if b else 0.0
        worse = change > HOTPATH_TOLERANCE if head[key]["smaller_is_better"] else change < -HOTPATH_TOLERANCE
        regressions += worse
        verdict = f"REGRESSION (limit {HOTPATH_TOLERANCE:.0%})" if worse else "ok"
        samples = f"{len(baseline[key]['measurements'])} vs {len(head[key]['measurements'])} samples"
        print(
            f"  {test_id} {name}: base {b:.4g} -> head {h:.4g} {unit} ({change:+.1%}, {samples}) {verdict}",
            file=sys.stderr,
        )
    return 1 if regressions else 0


def export_screenshots(bundle: Path, types: list[TestType], names: list[str]) -> None:
    exported = bundle.parent / "attachments"
    subprocess.run(
        ["xcrun", "xcresulttool", "export", "attachments", "--path", str(bundle), "--output-path", str(exported)],
        check=True,
        capture_output=True,
    )
    feature_of = {t.name: t.feature for t in types if t.is_screenshot}
    for name in names:
        shutil.rmtree(SCREENSHOT_DIR / name, ignore_errors=True)
    for test in json.loads((exported / "manifest.json").read_text()):
        feature = feature_of.get(test["testIdentifier"].split("/")[0])
        for attachment in test["attachments"]:
            if feature is None or not attachment["exportedFileName"].endswith(".png"):
                continue
            target = SCREENSHOT_DIR / feature / f"{attachment_name(attachment)}.png"
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(exported / attachment["exportedFileName"], target)
            print(f"saved {target.relative_to(ROOT)}", file=sys.stderr)


def attachment_name(attachment: dict) -> str:
    return re.sub(r"_\d+_[0-9A-F-]{36}\.png$", "", attachment["suggestedHumanReadableName"])


def touched_features(base: str) -> tuple[set[str], list[str]]:
    """Return (touched features, changed files outside feature folders)."""
    feats = set(features(swift_files()))
    touched: set[str] = set()
    misplaced: list[str] = []
    for f in changed_files(base):
        folder = folder_of(f)
        if folder is None:
            if (ROOT / f).exists():
                misplaced.append(f)
            continue
        touched |= feats if folder == SHARED else {folder} & feats
    return touched, misplaced


def screenshot_features(manifest: dict, touched: set[str]) -> list[str]:
    return [f for f in sorted(touched) if "screenshot" not in manifest["na"].get(f, {})]


def related() -> None:
    manifest = load_manifest()
    types = test_types(swift_files())
    touched, misplaced = touched_features(upstream())
    if not touched and not misplaced:
        print("No changes in source or test folders.")
        return

    print(f"Touched features: {' '.join(sorted(touched)) or 'none'}")
    if misplaced:
        print(f"Not in a feature folder (run scripts/test.py lint): {' '.join(misplaced)}")
    for feature in sorted(touched):
        for kind in KIND_TARGETS:
            reason = manifest["na"].get(feature, {}).get(kind)
            print(f"  # {kind} {feature}: n/a ({reason})" if reason else f"  scripts/test.py {kind} {feature}")
    for feature in sorted(touched):
        if hotpath_tests(types, feature):
            print(f"  scripts/test.py hotpath {feature}")
    if screenshot_features(manifest, touched):
        print("After you open the PR:")
        print("  scripts/pr_screenshots.py")


def web_typecheck() -> None:
    web = ROOT / "web"
    if not (web / "node_modules").is_dir():
        print("web types skipped: run `npm ci` in web/ to type check the page", file=sys.stderr)
        return
    done = subprocess.run(["npm", "run", "--silent", "typecheck"], cwd=web, capture_output=True, text=True)
    if done.returncode != 0:
        raise LintError("web/ does not type check (npm run typecheck):\n" + (done.stdout + done.stderr).strip())


def main(argv: list[str]) -> int:
    if not argv or argv[0] not in ("unit", "e2e", "hotpath", "screenshot", "related", "lint"):
        print(__doc__.strip(), file=sys.stderr)
        return 64
    command, rest = argv[0], argv[1:]
    try:
        if command == "lint":
            files = swift_files()
            lint(load_manifest(), files, test_types(files))
            stale = web_stamp.problems()
            if stale:
                raise LintError("\n".join(stale))
            web_typecheck()
            print("Layout, features.json, web build, and web types OK", file=sys.stderr)
            return 0
        if command == "related":
            related()
            return 0
        base = None
        if "--base" in rest:
            i = rest.index("--base")
            base, rest = rest[i + 1], rest[:i] + rest[i + 2 :]
        return run_kind(command, rest, base)
    except LintError as e:
        print(f"lint: {e}".replace("\n", "\nlint: "), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
