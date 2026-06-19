#!/usr/bin/env bash
# Runs inside the ci build image (ci/Dockerfile); cwd is the repo checkout.
# Produces:
#   target/nextest/ci/junit.xml  -> published by dorny/test-reporter
#   coverage.json                -> raw llvm-cov report
#   coverage-summary.md          -> appended to the GitHub job summary
# Exits non-zero if any test fails (so the workflow fails too).
set -uo pipefail

cargo llvm-cov --no-report --workspace nextest --profile ci
status=$?

cargo llvm-cov report --json --output-path coverage.json || true

if [ -f coverage.json ]; then
  node -e '
    const d = require("./coverage.json");
    const t = d.data[0].totals.lines;
    require("fs").writeFileSync("coverage-summary.md",
      "## 📊 Coverage\n\n| Metric | Value |\n|---|---|\n| Lines | " +
      t.percent.toFixed(1) + "% (" + t.covered + " / " + t.count + " lines) |\n" +
      "\n_Per-test results are in the **Tests** check above._\n");
  ' || true
fi

exit "$status"
