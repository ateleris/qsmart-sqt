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

# nextest omits the `time` attribute on <testsuite> elements (it only sets it on
# the root <testsuites> and on each <testcase>). dorny/test-reporter sums the
# per-suite times, so without this it reports the total as "NaNms". Backfill each
# suite's time as the sum of its testcases.
JUNIT=target/nextest/ci/junit.xml
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
