# TO_UPDATE – Required Code Changes

> **Status (2026-07-03): superseded.** U-001…U-009 below are essentially all
> implemented (the metric renames, `model_zip` removal, `api.*`→`runner.*` keys
> are done). The only unimplemented remainders are cosmetic — `SqtConfig` was not
> renamed to `ReporterConfig`, `token_env` is flat not nested, and `src/api/` was
> not renamed to `src/runner_api/`; the architecture now documents the flat/real
> form as the target, so these are candidates to close as "won't do". For the
> current, prioritised backlog see **`REVIEW_FINDINGS.md`**. This file is retained
> for history.

This file tracks changes needed in already-implemented code to align with the
current architecture (`ARCHITECTURE.md`) and API contract (`job-api.openapi.yaml`).
Items here were identified during the IMPLEMENTATION.md alignment pass.

Use this as input when creating the update plan.

---

## U-001 - Remove `model_zip_path` from `SqtConfig` / `ReporterConfig`

**Source task**: T-002 (done)  
**Severity**: High - the field contradicts the architecture; model is Runner-configured at startup.

**What to change**:
- `src/config/schema.rs`: remove `model_zip_path: Option<PathBuf>` field from the config struct
- `src/config/schema.rs` or `validator.rs`: remove any validation logic for that field
- Config YAML fixture files in tests: remove `model_zip_path` entries if present
- Rename config struct from `SqtConfig` to `ReporterConfig` (or `SqtReporterConfig`) to
  match the two-component split
- Rename top-level config key `api.url` / `api.token_env` to `runner.url` / `runner.auth.token_env`
  to match `sqt-reporter.yaml` schema in ARCHITECTURE.md Section 3

---

## U-002 - Remove `--model-zip` CLI flag

**Source task**: T-003 (done)  
**Severity**: High - flag was predicated on `model_zip_path` which is removed.

**What to change**:
- `src/main.rs`: remove `--model-zip <path>` argument from the `clap` definition
- Remove any logic that reads this flag and injects it into config or passes it downstream
- Update CLI help text / usage examples if present

---

## U-003 - Rename metric: `time.execution` → `time.avg_ms`

**Source task**: T-022 (done)  
**Severity**: High - metric key must match `JobMetrics` schema exactly for constraint
evaluation to work.

**What to change**:
- `src/metrics/builtin/execution_time.rs`:
  - `fn name(&self) -> &str { "time.avg_ms" }` (was `"time.execution"`)
  - `inventory::submit! { CollectorFactory { name: "time.avg_ms", ... } }`
- Any test fixtures or test assertions that reference `"time.execution"`
- `src/specs/example.rs` (T-033): any constraint using `"time.execution"` → `"time.avg_ms"`
  (also covered by U-007)

---

## U-004 - Rename metric: `memory.peak_rss` → `memory.peak_rss_kb`

**Source task**: T-023 (done)  
**Severity**: High - metric key must match `JobMetrics` schema.

**What to change**:
- `src/metrics/builtin/memory_rss.rs`:
  - `fn name(&self) -> &str { "memory.peak_rss_kb" }` (was `"memory.peak_rss"`)
  - `inventory::submit! { CollectorFactory { name: "memory.peak_rss_kb", ... } }`
- Any test fixtures or assertions referencing `"memory.peak_rss"`
- `src/specs/example.rs` (T-033): constraint using `"memory.peak_rss"` → `"memory.peak_rss_kb"`
  (also covered by U-007)

---

## U-005 - Rename metric: `memory.leaks` → `memory.leaks_bytes`

**Source task**: T-024 (done)  
**Severity**: High - metric key must match `JobMetrics` schema.

**What to change**:
- `src/metrics/builtin/memory_leaks.rs`:
  - `fn name(&self) -> &str { "memory.leaks_bytes" }` (was `"memory.leaks"`)
  - `inventory::submit! { CollectorFactory { name: "memory.leaks_bytes", ... } }`
- Any test fixtures or assertions referencing `"memory.leaks"`

---

## U-006 - Rename metric: `coverage.line_percent` → `coverage.line_pct`

**Source task**: T-025 (done)  
**Severity**: High - metric key must match `JobMetrics` schema.

**What to change**:
- `src/metrics/builtin/coverage_line.rs`:
  - `fn name(&self) -> &str { "coverage.line_pct" }` (was `"coverage.line_percent"`)
  - `inventory::submit! { CollectorFactory { name: "coverage.line_pct", ... } }`
- Any test fixtures or assertions referencing `"coverage.line_percent"`

---

## U-007 - Update example spec metric names (`src/specs/example.rs`)

**Source task**: T-033 (done)  
**Severity**: Medium - example spec is the template users copy; wrong names here propagate.

**What to change**:
- `src/specs/example.rs`: update any `Constraint::new(...)` calls that use old names:
  - `"time.execution"` → `"time.avg_ms"`
  - `"memory.peak_rss"` → `"memory.peak_rss_kb"`
  - Any other old names from T-022 through T-026

---

## U-008 - Rename `src/api/` module to `src/runner_api/`

**Source task**: T-030 / T-031 (done, flagged for Phase 4 replacement)  
**Severity**: Low - cosmetic but improves clarity about which side this client speaks to.

**What to change**:
- Rename directory `src/api/` → `src/runner_api/`
- Update `mod api;` → `mod runner_api;` in `src/lib.rs` or `src/main.rs`
- Update all `use crate::api::` import paths to `use crate::runner_api::`
- This rename should be done together with T-040/T-041 (Phase 4 type redesign) to avoid
  two separate renames

---

## U-009 - Rename metric: `sonar.critical_issues` → `static.critical_issues`

**Source task**: T-026 (done)  
**Severity**: High - the dev server (T-063) would otherwise need a hidden translation layer
between the collector's name and the `JobMetrics` field the Reporter reads.

**What to change**:
- `src/metrics/builtin/sonar_issues.rs`:
  - `fn name(&self) -> &str { "static.critical_issues" }` (was `"sonar.critical_issues"`)
  - `inventory::submit! { CollectorFactory { name: "static.critical_issues", ... } }`
- Any test fixtures or assertions referencing `"sonar.critical_issues"`
- If a `SonarMajorIssuesCollector` was also implemented, rename it to `"static.major_issues"`

---

## Notes

- **U-001 + U-002** are coupled: removing `model_zip_path` from config and `--model-zip`
  from the CLI should be one atomic change to avoid a broken intermediate state.
- **U-003 through U-006 + U-009** are all metric name renames in Runner-side collector code.
  Do them as one commit - the `debug_assert_eq!` in `build_registry()` will catch any
  name mismatch immediately in tests.
- **U-007** follows from U-003/U-004: the example spec uses the same old names. Fix it in
  the same commit as U-003/U-004.
- **U-008** can be deferred to when T-040/T-041 are implemented (Phase 4), since those tasks
  replace all the types in `src/api/types.rs` anyway - do the rename and the type redesign
  in one pass.
