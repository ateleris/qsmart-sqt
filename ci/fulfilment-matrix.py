#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Ateleris GmbH
# SPDX-License-Identifier: ESA-PL-permissive-2.4
"""Generate the requirements fulfilment matrix from test metadata.

Every test names the SRS requirement(s) it verifies in a doc-comment line
directly above its test attribute:

    /// Verifies: SRS-FUNC-002, SRS-IF-006
    #[test]
    fn token_env_validation_fires_when_var_missing() { ... }

A test that verifies no SRS requirement says so, with a reason:

    /// Verifies: none (legacy local metric collector, not used by the Orchestrator)

The script fails when a test has no such line or the line is malformed, so
"each test names the SRS requirement it verifies" is enforced, not hoped for.

Optional inputs:
  --junit FILE   nextest JUnit report; adds each test's result to the matrix
  --srs FILE     the SRS StrictDoc file; adds titles and verification methods,
                 lists requirements no test covers, and fails on unknown UIDs

Outputs (each optional): --json FILE, --markdown FILE. With neither, a short
summary is printed.
"""

import argparse
import json
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

CRATES = ["sqt-types", "sqt-reporter", "sqt-runner"]
TEST_ATTR = re.compile(r"^\s*#\[(tokio::)?test\b")
FN = re.compile(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)")
MOD_OPEN = re.compile(r"^(\s*)(?:pub(?:\(\w+\))?\s+)?mod\s+(\w+)\s*\{")
VERIFIES = re.compile(r"^\s*///\s*Verifies:\s*(.*?)\s*$")
UID = re.compile(r"^SRS-[A-Z]+-\d{3}$")
NONE = re.compile(r"^none \((.+)\)$")

# Requirements verified by a CI job rather than by a test function. The job
# is the test; its result is the CI run of the commit or tag.
CI_GATES = {
    "SRS-QUAL-001": ".github/workflows/rust.yml job `clippy`",
    "SRS-QUAL-002": ".github/workflows/rust.yml job `rustfmt`",
    "SRS-CDR-002": ".github/workflows/release.yml (clean build of the tag)",
    "SRS-CDR-003": ".github/workflows/rust.yml, every job, on every push and pull request",
    "SRS-PORT-001": ".github/workflows/rust.yml, every job, on x86-64 Linux (ci/Dockerfile)",
}


def suite_and_prefix(crate_dir: Path, rel: Path) -> tuple[str, str]:
    """nextest suite name and module-path prefix for a source file."""
    crate = crate_dir.name
    if rel.parts[0] == "tests":
        return f"{crate}::{rel.stem}", ""
    has_lib = (crate_dir / "src" / "lib.rs").exists()
    suite = crate if has_lib else f"{crate}::bin/{crate}"
    parts = list(rel.with_suffix("").parts[1:])  # drop "src"
    if parts and parts[-1] in ("mod", "lib", "main"):
        parts.pop()
    return suite, "::".join(parts)


def scan_file(root: Path, crate_dir: Path, path: Path, errors: list) -> list:
    rel = path.relative_to(crate_dir)
    suite, prefix = suite_and_prefix(crate_dir, rel)
    lines = path.read_text().splitlines()
    tests = []
    mods = []  # stack of (indent, name)
    for i, line in enumerate(lines):
        m = MOD_OPEN.match(line)
        if m:
            indent = len(m.group(1))
            while mods and mods[-1][0] >= indent:
                mods.pop()
            mods.append((indent, m.group(2)))
            continue
        if not TEST_ATTR.match(line):
            continue
        j = i + 1
        while j < len(lines) and not FN.match(lines[j]):
            j += 1
        name = FN.match(lines[j]).group(1)
        indent = len(line) - len(line.lstrip())
        scope = [n for (ind, n) in mods if ind < indent]
        test_path = "::".join(p for p in [prefix, *scope, name] if p)
        where = f"{path.relative_to(root)}:{i + 1}"

        # Ignored tests (e.g. the Docker end-to-end test) run in their own CI
        # job, not in the JUnit report of the test job.
        k = i + 1
        ignored = False
        while k < j:
            ignored = ignored or lines[k].strip().startswith("#[ignore")
            k += 1

        # The tag is in the comment/attribute block directly above the attribute.
        tag = None
        k = i - 1
        while k >= 0 and re.match(r"^\s*(///|#\[)", lines[k]):
            v = VERIFIES.match(lines[k])
            if v:
                tag = v.group(1)
                break
            k -= 1

        entry = {"suite": suite, "test": test_path, "file": where, "verifies": [], "none": None,
                 "ignored": ignored}
        if tag is None:
            errors.append(f"{where}: test `{name}` has no `/// Verifies:` line")
        elif n := NONE.match(tag):
            entry["none"] = n.group(1)
        else:
            uids = [u.strip() for u in tag.split(",")]
            bad = [u for u in uids if not UID.match(u)]
            if bad or not uids:
                errors.append(f"{where}: malformed `Verifies:` entry {bad or tag!r}")
            entry["verifies"] = [u for u in uids if UID.match(u)]
        tests.append(entry)
    return tests


def read_junit(path: Path) -> dict:
    results = {}
    for case in ET.parse(path).getroot().iter("testcase"):
        if case.find("failure") is not None or case.find("error") is not None:
            status = "failed"
        elif case.find("skipped") is not None:
            status = "skipped"
        else:
            status = "passed"
        results[(case.get("classname"), case.get("name"))] = status
    return results


def read_srs(path: Path) -> dict:
    text = path.read_text()
    reqs = {}
    for block in re.split(r"\n\[(?:REQUIREMENT|COMPOSITE_REQUIREMENT)\]\n", text)[1:]:
        block = block.split("\n[")[0]
        uid = re.search(r"^UID: (\S+)", block, re.M)
        if not uid:
            continue
        title = re.search(r"^TITLE: (.*)", block, re.M)
        method = re.search(r"^VERIFICATION_METHOD: (.*)", block, re.M)
        reqs[uid.group(1)] = {
            "title": title.group(1).strip() if title else "",
            "method": method.group(1).strip() if method else "",
        }
    return reqs


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", default=".", help="workspace root (default: .)")
    ap.add_argument("--junit", type=Path)
    ap.add_argument("--srs", type=Path)
    ap.add_argument("--json", type=Path)
    ap.add_argument("--markdown", type=Path)
    ap.add_argument("--version", default="", help="SQT version or tag stamped into the output")
    args = ap.parse_args()

    root = Path(args.root).resolve()
    errors: list = []
    tests = []
    for crate in CRATES:
        crate_dir = root / crate
        files = sorted((crate_dir / "src").rglob("*.rs")) + sorted((crate_dir / "tests").glob("*.rs"))
        for f in files:
            tests.extend(scan_file(root, crate_dir, f, errors))

    if args.junit:
        results = read_junit(args.junit)
        for t in tests:
            t["result"] = results.get((t["suite"], t["test"]), "not run")
            if t["result"] == "not run" and t["ignored"]:
                t["result"] = "separate job"
            elif t["result"] == "not run":
                errors.append(f"{t['file']}: `{t['test']}` not found in {args.junit}")

    srs = read_srs(args.srs) if args.srs else {}
    by_uid: dict = {}
    for t in tests:
        for uid in t["verifies"]:
            by_uid.setdefault(uid, []).append(t)
            if srs and uid not in srs:
                errors.append(f"{t['file']}: `{t['test']}` names {uid}, which is not in the SRS")

    requirements = []
    # SRS document order, then any UID the SRS does not know.
    order = list(srs) + sorted((set(by_uid) | set(CI_GATES)) - set(srs), key=uid_sort_key)
    for uid in order:
        verifying = by_uid.get(uid, [])
        info = srs.get(uid, {"title": "", "method": ""})
        requirements.append({
            "uid": uid,
            "title": info["title"],
            "verification_method": info["method"],
            "tests": [f"{t['suite']} {t['test']}" for t in verifying],
            "ci_gate": CI_GATES.get(uid),
            "status": status_of(info["method"], verifying, bool(srs), uid in CI_GATES),
        })

    matrix = {
        "sqt_version": args.version,
        "srs": str(args.srs) if args.srs else None,
        "tests_total": len(tests),
        "tests_without_requirement": sum(1 for t in tests if t["none"]),
        "requirements": requirements,
        "tests": tests,
    }
    if args.json:
        args.json.write_text(json.dumps(matrix, indent=2) + "\n")
    if args.markdown:
        args.markdown.write_text(markdown(matrix, bool(srs), bool(args.junit)))
    if not args.json and not args.markdown:
        covered = sum(1 for r in requirements if r["tests"])
        print(f"{len(tests)} tests, {matrix['tests_without_requirement']} verify no requirement; "
              f"{covered} requirements have at least one test")

    for e in errors:
        print(f"error: {e}", file=sys.stderr)
    return 1 if errors else 0


def uid_sort_key(uid: str):
    m = re.match(r"SRS-([A-Z]+)-(\d+)", uid)
    return (m.group(1), int(m.group(2))) if m else (uid, 0)


def status_of(method: str, verifying: list, have_srs: bool, ci_gate: bool) -> str:
    if have_srs and method.lower() != "test":
        return f"by {method.lower().replace('_', ' ')}" if method else "no verification method"
    if not verifying:
        return "CI gate" if ci_gate else "no test"
    results = {t.get("result") for t in verifying}
    if "failed" in results:
        return "failing"
    if results <= {"passed", "separate job"} and "passed" in results:
        return "verified"
    return "tested"


def markdown(m: dict, have_srs: bool, have_results: bool) -> str:
    out = ["# Requirements fulfilment matrix", ""]
    if m["sqt_version"]:
        out += [f"SQT version: `{m['sqt_version']}`", ""]
    out += [
        f"{m['tests_total']} tests; {m['tests_without_requirement']} verify no SRS requirement.",
        "Generated by `ci/fulfilment-matrix.py` from the `/// Verifies:` line of each test.",
        "",
        "| Requirement | Title | Method | Status | Tests |",
        "|---|---|---|---|---|",
    ]
    for r in m["requirements"]:
        tests = "<br>".join(f"`{t.split(' ', 1)[1]}`" for t in r["tests"]) or r["ci_gate"] or "–"
        out.append(f"| {r['uid']} | {r['title']} | {r['verification_method'] or '–'} | {r['status']} | {tests} |")
    none = [t for t in m["tests"] if t["none"]]
    if none:
        out += ["", "## Tests that verify no SRS requirement", "", "| Test | Reason |", "|---|---|"]
        out += [f"| `{t['test']}` | {t['none']} |" for t in none]
    if have_results:
        failed = [t for t in m["tests"] if t.get("result") == "failed"]
        out += ["", f"Test results from JUnit: {len(failed)} failed."]
    if not have_srs:
        out += ["", "_Generated without the SRS: titles, methods and uncovered requirements are not shown._"]
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    sys.exit(main())
