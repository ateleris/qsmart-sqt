#!/usr/bin/env bash
# Runs inside the ci build image (ci/Dockerfile); cwd is the repo checkout.
# Tests with coverage (rustfmt and clippy are separate CI jobs). Produces,
# in the workspace:
#   target/nextest/ci/junit.xml  -> published by dorny/test-reporter
#   coverage.json                -> raw llvm-cov report
#   coverage-summary.md          -> appended to the GitHub job summary
# Exits non-zero if any test fails (so the workflow fails too), or if
# COVERAGE_FAIL_UNDER is set and the Orchestrator's line coverage is below it
# (the release gate: measured on every merge, enforced at the tag).
set -uo pipefail

cargo llvm-cov --no-report --workspace nextest --profile ci
status=$?

# Compilation (and nextest's junit) may live in a cached target dir outside the
# workspace (CARGO_TARGET_DIR). Copy the junit into the workspace so that
# dorny/test-reporter — which runs on the runner, not in this container — sees it.
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
JUNIT=target/nextest/ci/junit.xml
if [ "$TARGET_DIR/nextest/ci/junit.xml" != "$JUNIT" ] && [ -f "$TARGET_DIR/nextest/ci/junit.xml" ]; then
  mkdir -p "$(dirname "$JUNIT")"
  cp "$TARGET_DIR/nextest/ci/junit.xml" "$JUNIT"
fi

# nextest omits the `time` attribute on <testsuite> elements (it only sets it on
# the root <testsuites> and each <testcase>). dorny/test-reporter sums per-suite
# times, so without this it reports the total as "NaNms". Backfill each suite's
# time as the sum of its testcases.
if [ -f "$JUNIT" ]; then
  node -e '
    const fs = require("fs");
    const f = process.argv[1];
    let xml = fs.readFileSync(f, "utf8");
    xml = xml.replace(/<testsuite\b([^>]*)>([\s\S]*?)<\/testsuite>/g, (m, attrs, body) => {
      if (/\stime=/.test(attrs)) return m;
      let sum = 0;
      for (const tc of body.matchAll(/<testcase\b[^>]*\stime="([\d.]+)"/g)) sum += parseFloat(tc[1]);
      return `<testsuite${attrs} time="${sum.toFixed(3)}">${body}</testsuite>`;
    });
    fs.writeFileSync(f, xml);
  ' "$JUNIT" || true
fi

# Coverage report + summary (written into the workspace). The Orchestrator
# figure is sqt-reporter alone: the Runner and the shared types are excluded.
ORCHESTRATOR_ONLY='/sqt-(runner|types)/'
cargo llvm-cov report --json --output-path coverage.json || true
cargo llvm-cov report --json --output-path coverage-orchestrator.json \
  --ignore-filename-regex "$ORCHESTRATOR_ONLY" || true
if [ -f coverage.json ] && [ -f coverage-orchestrator.json ]; then
  node -e '
    const row = (label, f) => {
      const t = require(f).data[0].totals.lines;
      return "| " + label + " | " + t.percent.toFixed(1) + "% (" + t.covered + " / " + t.count + " lines) |\n";
    };
    require("fs").writeFileSync("coverage-summary.md",
      "## 📊 Coverage\n\n| Scope | Lines |\n|---|---|\n" +
      row("Workspace", "./coverage.json") +
      row("Orchestrator (sqt-reporter)", "./coverage-orchestrator.json") +
      "\n_Release gate: Orchestrator ≥ 80 % at the tag. Per-test results are in the **Tests** check above._\n");
  ' || true
fi

if [ -n "${COVERAGE_FAIL_UNDER:-}" ]; then
  cargo llvm-cov report --summary-only --ignore-filename-regex "$ORCHESTRATOR_ONLY" \
    --fail-under-lines "$COVERAGE_FAIL_UNDER" || status=1
fi

exit "$status"
