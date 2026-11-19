#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Ateleris GmbH
# SPDX-License-Identifier: ESA-PL-permissive-2.4
"""Write the release notes of a tagged SQT release.

The release pipeline calls this after it has built and checksummed the
assets, so the notes are generated, never written by hand (SQT-TN-AP,
status accounting). They name the release, list what changed since the
previous release tag, summarise the verification run of the tag and list
the delivered assets with their SHA-256 values.

Inputs, all produced earlier in the same release job:
  --changes FILE    `git log --first-parent --merges --format=%s%x00%b%x1e`
                    of the range since the previous release tag
  --matrix FILE     fulfilment-matrix.json (ci/fulfilment-matrix.py)
  --coverage FILE   coverage-orchestrator.json (cargo llvm-cov, sqt-reporter only)
  --sums FILE       dist/SHA256SUMS
  --images FILE     dist/IMAGES.txt
"""

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

PR_MERGE = re.compile(r"^Merge pull request #(\d+) from [^/\s]+/(\S+)$")
TOOLCHAIN = re.compile(r'^channel\s*=\s*"([^"]+)"', re.MULTILINE)
ENGINE = re.compile(r"^# Docker Engine (\S+), (\w+) image store$", re.MULTILINE)
COVERAGE_GATE = 80


def read_changes(path: Path) -> tuple[list, list]:
    """Merged pull requests as (number, title), split into changes and dependency bumps."""
    changes, deps = [], []
    for record in path.read_text().split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        subject, _, body = record.partition("\x00")
        m = PR_MERGE.match(subject.strip())
        if not m:
            continue  # a merge that is not a pull request
        number, branch = int(m.group(1)), m.group(2)
        title = next((line.strip() for line in body.splitlines() if line.strip()), branch)
        (deps if branch.startswith("dependabot/") else changes).append((number, title))
    return changes, deps


def verification(matrix: dict) -> list:
    reqs = matrix["requirements"]
    by_test = [r for r in reqs if r["tests"]]
    statuses = {}
    for r in by_test:
        statuses[r["status"]] = statuses.get(r["status"], 0) + 1
    failing = [r["uid"] for r in reqs if r["status"] == "failing"]
    gates = [r["uid"] for r in reqs if r["ci_gate"] and not r["tests"]]
    none = matrix["tests_without_requirement"]
    lines = [
        f"- {matrix['tests_total']} tests, each naming the SRS requirement it verifies"
        + (f" ({none} name none, with a stated reason)." if none else "."),
        f"- {len(by_test)} SRS requirements are verified by tests: "
        + ", ".join(f"{n} {s}" for s, n in sorted(statuses.items())) + ".",
    ]
    if gates:
        lines.append(f"- Verified by a CI gate of the release run: {', '.join(gates)}.")
    lines.append(f"- Failing requirements: {', '.join(failing) if failing else 'none'}.")
    return lines


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--tag", required=True)
    ap.add_argument("--commit", required=True)
    ap.add_argument("--epoch", type=int, required=True, help="tag commit time (SOURCE_DATE_EPOCH)")
    ap.add_argument("--previous", default="", help="previous release tag; empty for the first release")
    ap.add_argument("--changes", type=Path, required=True)
    ap.add_argument("--matrix", type=Path, required=True)
    ap.add_argument("--coverage", type=Path, required=True)
    ap.add_argument("--sums", type=Path, required=True)
    ap.add_argument("--images", type=Path, required=True)
    ap.add_argument("--toolchain", type=Path, default=Path("rust-toolchain.toml"))
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    version = args.tag.removeprefix("v")
    date = datetime.fromtimestamp(args.epoch, tz=timezone.utc).strftime("%Y-%m-%d")
    m = TOOLCHAIN.search(args.toolchain.read_text())
    if not m:
        print(f"error: no toolchain channel in {args.toolchain}", file=sys.stderr)
        return 1
    toolchain = m.group(1)
    changes, deps = read_changes(args.changes)
    matrix = json.loads(args.matrix.read_text())
    lines_cov = json.loads(args.coverage.read_text())["data"][0]["totals"]["lines"]
    sums = [line.split(None, 1) for line in args.sums.read_text().splitlines() if line.strip()]
    images_text = args.images.read_text()
    images = [line.split() for line in images_text.splitlines()
              if line.strip() and not line.startswith("#")]
    engine = ENGINE.search(images_text)

    out = [
        f"# SQT {args.tag}",
        "",
        f"Software Qualification Toolkit (SQT), release `{args.tag}` of {date}.",
        "",
        f"- Tag: `{args.tag}` (annotated, signed)",
        f"- Commit: `{args.commit}`",
        f"- Toolchain: Rust `{toolchain}` (`rust-toolchain.toml`), target `x86_64-unknown-linux-musl`",
        "- Licence: ESA Software Community Licence Permissive v2.4 (`LICENSE`, `NOTICE`)",
        "",
        "Release content, the tools used, installation and known limitations are recorded "
        "in the Software Release Document (SQT-RP-0002-ATS); the installation procedure is "
        "in the Software User Manual (SQT-MAN-0001-ATS).",
        "",
        "## Changes",
        "",
        f"Pull requests merged since `{args.previous}`:" if args.previous
        else "First release. Pull requests merged up to this tag:",
        "",
    ]
    out += [f"- {title} (#{n})" for n, title in changes] or ["- none"]
    if deps:
        out += ["", "Dependency updates:", ""]
        out += [f"- {title} (#{n})" for n, title in deps]

    out += ["", "## Verification", "", "Every gate ran again on the tag before any asset was built:", ""]
    out += verification(matrix)
    out += [
        f"- Orchestrator (sqt-reporter) line coverage: {lines_cov['percent']:.1f} % "
        f"({lines_cov['covered']} / {lines_cov['count']} lines); release gate ≥ {COVERAGE_GATE} %.",
        "- End-to-end run against the reference Runner, formatting, lints, licence, "
        "advisory and SBOM gates: passed.",
        f"- Full matrix: `fulfilment-matrix-{version}.md` / `.json`.",
        "",
        "## Assets",
        "",
        "| File | SHA-256 |",
        "|---|---|",
    ]
    out += [f"| `{name.lstrip('*')}` | `{digest}` |" for digest, name in sums]
    if images:
        out += ["", "Container images, installed with `docker load -i <archive>`. "
                "`docker image inspect --format '{{.Id}}' <image>` must show one of the "
                "values below: the image ID on a Docker daemon with the same image store "
                "as the build, or the configuration digest on a daemon with the classic "
                "image store (`IMAGES.txt`):", ""]
        for fields in images:
            ref, image_id = fields[0], fields[1]
            line = f"- `{ref}`: image ID `{image_id}`"
            if len(fields) > 2 and fields[2] != image_id:
                line += f", configuration digest `{fields[2]}`"
            out.append(line)
        if engine:
            out += ["", f"Built with Docker Engine {engine.group(1)}, "
                    f"{engine.group(2)} image store."]
    out += [
        "",
        "Verify the downloads with `sha256sum -c SHA256SUMS`. These release notes are "
        "generated from the assets and are not themselves listed in `SHA256SUMS`.",
        "",
    ]
    args.out.write_text("\n".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
