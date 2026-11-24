#!/usr/bin/env python3
"""Run diffpilot tests grouped by feature folder.

A feature is a folder diffpilot/Features/<Feature>/. App/ and Shared/ sit next to
Features/ and count as features too. Unit tests mirror the source path in diffpilotTests/
(diffpilotTests/Features/<Feature>/) and e2e tests in diffpilotUITests/.
Test types whose name contains "Performance" are hot path tests; features.json maps
them to source globs. UI test types whose name contains "Screenshot" capture the
feature's screenshots into screenshots/feature/<Feature>/. Shared/ is a feature too,
and a change in it touches every feature.

  scripts/test.py unit [feature...]      run unit tests (all features when none named)
  scripts/test.py e2e [feature...]       run e2e UI tests
  scripts/test.py hotpath [feature...]   run hot path performance tests
  scripts/test.py screenshot [feature...] capture screenshots into screenshots/feature/<Feature>/
  scripts/test.py related                print the test commands that match the current changes
  scripts/test.py lint                   check the folder layout and features.json
"""

from __future__ import annotations

import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from fnmatch import fnmatchcase
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "features.json"
SOURCE_DIR = "diffpilot"
SHARED = "Shared"
FEATURES_DIR = "Features"
TOP_LEVEL_FEATURES = ("App", SHARED)
KIND_TARGETS = {"unit": "diffpilotTests", "e2e": "diffpilotUITests"}
PERF_MARKER = "Performance"
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

    def has_method(self, method: str) -> bool:
        return re.search(rf"\bfunc\s+{re.escape(method)}\b", self.body) is not None


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
    manifest.setdefault("hotpaths", [])
    return manifest


def functional_tests(types: list[TestType], feature: str, kind: str) -> list[str]:
    return sorted(
        t.test_id
        for t in types
        if t.feature == feature and t.target == KIND_TARGETS[kind] and not t.is_perf and not t.is_screenshot
    )


def screenshot_tests(types: list[TestType], feature: str) -> list[str]:
    return sorted(t.test_id for t in types if t.feature == feature and t.is_screenshot)


def hotpath_tests(manifest: dict, feature: str) -> list[str]:
    return [t for h in manifest["hotpaths"] if h["feature"] == feature for t in h["tests"]]


def resolve(types: list[TestType], test_id: str) -> TestType | None:
    target, name, *method = test_id.split("/")
    for t in types:
        if t.target == target and t.name == name and (not method or t.has_method(method[0].removesuffix("()"))):
            return t
    return None


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

    listed: set[str] = set()
    for i, hot in enumerate(manifest["hotpaths"]):
        where = f"features.json hotpaths[{i}]"
        if hot.get("feature") not in feats:
            errors.append(f'{where}: unknown feature "{hot.get("feature")}"')
        if not any(fnmatchcase(f, hot.get("glob", "")) for f in files):
            errors.append(f'{where}: glob "{hot.get("glob")}" matches no file')
        if not hot.get("tests"):
            errors.append(f"{where}: tests must not be empty")
        for test_id in hot.get("tests", []):
            t = resolve(types, test_id)
            if t is None:
                errors.append(f'{where}: test "{test_id}" not found')
            elif not t.is_perf:
                errors.append(
                    f'{where}: "{test_id}" is not a hot path test; its type name must contain "{PERF_MARKER}"'
                )
            elif t.feature != hot.get("feature"):
                errors.append(f'{where}: "{test_id}" lives in {t.path}, not in feature {hot.get("feature")}')
            else:
                listed.add(t.test_id)

    for t in types:
        if t.is_perf and t.test_id not in listed:
            errors.append(f"{t.path}: {t.name} is a hot path test but no hotpaths entry in features.json lists it")

    if errors:
        raise LintError("\n".join(errors))


def run_kind(kind: str, names: list[str]) -> int:
    manifest, files = load_manifest(), swift_files()
    types = test_types(files)
    lint(manifest, files, types)
    feats = features(files)
    unknown = [n for n in names if n not in feats]
    if unknown:
        raise LintError(f"unknown feature(s): {', '.join(unknown)}; features are {', '.join(feats)}")
    names = names or feats
    pickers = {
        "hotpath": lambda n: hotpath_tests(manifest, n),
        "screenshot": lambda n: screenshot_tests(types, n),
    }
    pick = pickers.get(kind, lambda n: functional_tests(types, n, kind))
    tests = sorted({t for n in names for t in pick(n)})
    if not tests:
        print(f"No {kind} tests for: {' '.join(names)}", file=sys.stderr)
        return 0

    args = [f"-only-testing:{t}" for t in tests]
    env = dict(os.environ)
    if kind == "hotpath":
        # Serial, so parallel test clones do not skew the numbers.
        args += ["-parallel-testing-enabled", "NO"]
        # xcodebuild passes TEST_RUNNER_* to the test process without the prefix.
        env["TEST_RUNNER_DIFFPILOT_HOTPATH"] = "1"
    bundle = Path(tempfile.mkdtemp(prefix=f"diffpilot-{kind}-")) / "result.xcresult"
    print(f"== {kind}: {' '.join(tests)}", file=sys.stderr)
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
        cwd=ROOT,
        env=env,
    ).returncode
    if code != 0:
        print(f"Failure details: xcrun xcresulttool get test-results tests --path {bundle}", file=sys.stderr)
    elif kind == "screenshot":
        export_screenshots(bundle, types, names)
    return code


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


def touched_features(base: str) -> tuple[set[str], set[str], list[str]]:
    """Return (touched features, features with a touched hot path, files outside feature folders)."""
    manifest, files = load_manifest(), swift_files()
    types = test_types(files)
    feats = set(features(files))
    touched: set[str] = set()
    hot: set[str] = set()
    misplaced: list[str] = []
    for f in changed_files(base):
        folder = folder_of(f)
        if folder is None:
            if (ROOT / f).exists():
                misplaced.append(f)
            continue
        touched |= feats if folder == SHARED else {folder} & feats
        if f.startswith(SOURCE_DIR + "/"):
            hot |= {h["feature"] for h in manifest["hotpaths"] if fnmatchcase(f, h["glob"])}
        else:
            changed_ids = {t.test_id for t in types if t.path == f}
            hot |= {
                h["feature"]
                for h in manifest["hotpaths"]
                if any("/".join(test.split("/")[:2]) in changed_ids for test in h["tests"])
            }
    return touched, hot, misplaced


def screenshot_features(manifest: dict, touched: set[str]) -> list[str]:
    return [f for f in sorted(touched) if "screenshot" not in manifest["na"].get(f, {})]


def related() -> None:
    manifest = load_manifest()
    touched, hot, misplaced = touched_features(upstream())
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
    for feature in sorted(hot):
        print(f"  scripts/test.py hotpath {feature}")
    if screenshot_features(manifest, touched):
        print("After you open the PR:")
        print("  scripts/pr_screenshots.py")


def main(argv: list[str]) -> int:
    if not argv or argv[0] not in ("unit", "e2e", "hotpath", "screenshot", "related", "lint"):
        print(__doc__.strip(), file=sys.stderr)
        return 64
    command, rest = argv[0], argv[1:]
    try:
        if command == "lint":
            files = swift_files()
            lint(load_manifest(), files, test_types(files))
            print("Layout and features.json OK", file=sys.stderr)
            return 0
        if command == "related":
            related()
            return 0
        return run_kind(command, rest)
    except LintError as e:
        print(f"lint: {e}".replace("\n", "\nlint: "), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
