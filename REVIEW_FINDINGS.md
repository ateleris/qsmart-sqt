# SQT Review Findings & TODO

**Review date**: 2026-07-03
**Basis**: workspace code vs. `ARCHITECTURE.md`, `IMPLEMENTATION.md`, and the SRS
of the time (71 requirements).
**State at review**: 279 tests passing, `cargo clippy --workspace` clean.
**Status update**: 2026-10-15, against `main` at `b4ed836` and the SRS
SQT-SP-0001-ATS Issue 1 Rev 2 in
[`qsmart-sqt-documentation`](https://github.com/ateleris/qsmart-sqt-documentation)
(74 requirements; SRS-IF-004, -005 and -009 moved to SRS-FUNC-018, -019 and -020).
Fixed items are marked **Fixed** with the commit and pull request; everything not
marked is still open.

This is the actionable backlog from the review. Each item has an ID, severity,
location, the SRS/contract reference it touches, and a fix direction. Items are
ordered by severity. The doc-drift items were **fixed in this pass** (see the last
section); everything above that is outstanding code work or a decision.

Related: `ARCHITECTURE.md` (target design, refreshed), `IMPLEMENTATION.md`
(current state). The older U-001…U-009 list (`TO_UPDATE.md`) was superseded by
this file and removed on 2026-10-15; its remainders are D-3 below.

---

## Critical — fix before SQT can gate a qualification

### F-C1 — Unauthenticated command injection via `input` (security)
**Fixed** in `847f4e5` (#8): every shell element and the stdin target are single-quoted.

`sqt-runner/src/tools/run_binary.rs:119` interpolates the client-supplied
`JobRequest.input` into a shell command run via `sh -c`
(`format!("{} < {inp}", …)`). `shell_join` (`:131-143`) only quotes on space/quote,
so `;`, `|`, `$`, backticks pass through; `Arg` mode is likewise vulnerable. The
input's parent dir is bind-mounted read-write into the container
(`run_binary.rs:53-57`), and no route enforces auth. A POST with
`{"input":"/tmp/x; touch /tmp/pwned"}` runs arbitrary commands.
**Fix**: use the correct `shell_quote` already present at `executor/docker.rs:385`
(or pass argv without a shell); validate/normalise `input` to a real path under an
allowed root. Pairs with F-C2.

### F-C2 — Runner enforces no bearer auth (security, SRS-IF-006)
**Fixed** in `847f4e5` (#8): optional `auth.token_env`; bearer middleware on every route except `/health`, 401 on failure.

No auth middleware on any route; `sqt-runner/src/handlers.rs:146` has an explicit
`TODO(auth)`. The OpenAPI declares `bearerAuth` globally and 401s; the runner never
returns 401. The reporter sends and redacts a token correctly — only the server
side is missing. This also makes the reporter's preflight auth check vacuous.
**Fix**: add token config to `ServerConfig`, an axum middleware that checks
`Authorization: Bearer` on all routes except `/health`, and 401 `ErrorResponse` on
failure.

### F-C3 — `time.avg_ms` measures Docker overhead, not the binary (SRS-FUNC-005)
**Fixed** in `64d4576` (#10): `time.avg_ms` from the inner GNU time elapsed line; overhead reported as `harness.docker_overhead_ms`.

`run_binary.rs:96` reports `output.wall_time` (the whole `docker run` duration).
`/usr/bin/time -v` runs inside the container and its "Elapsed (wall clock) time"
line is captured but discarded (only Max RSS / CPU% parsed at `:89-90`). The
primary latency metric is inflated by container create/start/teardown.
**Fix** (also review item C3, "report both"): parse the inner elapsed line for
`time.avg_ms`; add an additive `JobMetrics` diagnostic field for
harness/Docker overhead = outer wall-clock − inner elapsed, plus a matching
reporter constraint name. `JobMetrics` has `deny_unknown_fields`, so this is a
deliberate schema addition (SRS-DATA-001 additive change).

### F-C4 — All-INFRA_ERROR run exits 0 (SRS-OPS-001, SRS-OPS-006)
**Fixed** in `1dc6a1d` (#9): `Summary::is_gate_failure()` covers infra errors.

`sqt-reporter/src/main.rs:170` exits non-zero only when `summary.failed > 0`, but
`report.rs:148` computes `failed = total − passed − infra_errors`. A run where
every spec is INFRA_ERROR (none FAIL) exits 0 — a broken CI gate.
**Fix**: exit non-zero when `failed > 0 || infra_errors > 0` (one line).

---

## High — SRS / contract conformance

### F-H1 — Runner never returns `output`; ML features dead end-to-end
**Fixed** in `cad966f` (#14): stdout captured as the job output with checksum and `result_consistent`.

`sqt-runner/src/handlers.rs:232` hardcodes `output: None`; the executor captures no
inference outputs. The reporter-side precision (SRS-FUNC-016), accuracy
(SRS-FUNC-017, marked **Active** in the SRS), and `result_consistent`
(SRS-REL-004) logic is implemented and tested but can never fire against the
shipped runner. When `precision_reference` is set, the fall-through failure message
is even misleading ("reference_outputs not provided", `orchestrator/mod.rs:388`).
**Fix**: have the Docker backend capture the binary's stdout/declared output file
into `JobOutput.outputs` (+ checksum + `result_consistent` across repetitions);
correct the reporter's fall-through message.

### F-H2 — OpenAPI `model` vs code `use_case` (SRS-IF-001)
**Mostly fixed** in `2e0fc77` (#12): `use_case` throughout, `batch_size` removed, drift test `sqt-types/tests/openapi_contract.rs`. Still open: the `JobInput` union versus the free-form `input` the code accepts is documented as a pending contract decision.

`job-api.openapi.yaml` requires `model` on `JobRequest`/`JobStatusResponse`; all
code (sqt-types, runner, reporter) and `ARCHITECTURE.md` use `use_case`. `input`
is a further mismatch (contract discriminated union vs. bare path string on both
sides); `batch_size` is in the contract but not in `JobRequest`.
**Fix**: regenerate the OpenAPI file from the real DTOs (or vice-versa) and add a
contract test that fails on drift.

### F-H3 — Concurrency unimplemented (SRS-PERF-001/002/003)
**Fixed** in `fa71128` (#15): permits held across submit → poll, concurrent polling, poll-interval config wired, wall-clock envelope test.

`orchestrator/mod.rs:213-215` acquires and immediately drops the semaphore permit
around each single submit inside a sequential loop → `max_concurrent_jobs` bounds
nothing; polling is also sequential. `poll_interval_initial_secs` /
`poll_interval_max_secs` are parsed (`config/schema.rs:37-40`) but never passed to
`ApiClient` (cadence hardcoded 1s→30s, `api/client.rs:260-263`). Harmless today
only because the runner runs jobs serially.
**Fix**: submit all jobs holding permits across their lifetime, poll concurrently
(one task per job), and wire the poll-interval config. Add a test for the
wall-clock envelope.

### F-H4 — Closed metric/constraint vocabulary vs SRS-FUNC-006 (decision)
**Decided** at the ADD review for PDR: SRS-FUNC-006 stands and the ADD designs an open namespace (C-ORC-05, LLR-019). Implementation in SQT-SW-1, issue #17. See D-1.

`JobMetrics` `deny_unknown_fields` + `Constraint::from_name` hardcoded list means a
new project metric requires editing `sqt-types` **and** `sqt-reporter`. Also
`ml.mse`/`ml.mae` are computed but have no constraint variant, and
`output.result_consistent` cannot be declared as a constraint (SRS-REL-004).
**Decision needed** — see Decisions section.

### F-H5 — No LICENSE / attribution (SRS-DES-002)
**Fixed** in `25be6ca` (#13): ESA-PL Permissive v2.4 `LICENSE`, workspace licence and authors metadata, root README.

No `LICENSE` file, no `license`/`authors` in any `Cargo.toml`, no README at the
workspace root. SOW requires ESA Software Community Licence Permissive v2.4.
**Fix**: add LICENSE + license metadata + attribution + a root README.

---

## Medium — robustness & fidelity

- **F-M1** Silent zero-metrics: valgrind exit code ignored, well-formed XML with no
  leak records → `memory.leaks_bytes = 0` (`tools/valgrind.rs:49-73`); cppcheck on a
  binary dir with no source → `static.warnings = 0` (`tools/static_analysis.rs`).
  Both read as passing when nothing was measured. Fix: check tool exit codes;
  distinguish "clean" from "did not run".
- **F-M2** Submit-path failure aborts the whole session (`orchestrator/mod.rs:214`
  `?`), and a non-conformant submit response does the same — should be per-job
  INFRA_ERROR (SRS-IF-007/FUNC-020). **Fixed** in `fa71128` (#15): a submit
  failure is now an INFRA_ERROR of that job only.
- **F-M3** Server job failures mislabeled: `wait_for_job` bails on `Failed`
  (`client.rs:269`), so the `RUNNER_JOB_FAILED` branch is unreachable and failures
  surface as `TRANSPORT_ERROR`; the structured `details` field is dropped
  (SRS-IF-003/REL-003).
- **F-M4** Evidence not run-scoped; JSON report only written with `--output`
  (`main.rs:133`) though SRS-FUNC-009 says "shall produce"; successive runs
  overwrite `report.pdf`/`qdp.zip`/`traceability.*` (SRS-OPS-004). Fix: per-run
  timestamped subdir; always write the JSON report.
- **F-M5** Runner resource leaks: per-job workspace dirs never cleaned up; job
  store grows unbounded with no eviction / queue-depth cap (`handlers.rs`,
  `store.rs`).
- **F-M6** Runner ignores `repetitions` / `warmup_repetitions` / `mode` — binary
  runs once, `mode` always echoed `latency` (`handlers.rs:221`). `time.avg_ms` is a
  single sample despite the name.
- **F-M7** No default use-case fan-out (SRS-FUNC-019): `use_case` is required, so an
  undeclared workload is a hard error rather than one-job-per-type.
- **F-M8** Traceability matrix lacks provenance (SRS-DATA-003 wants it on *every*
  output) and omits actual constraint values; `evidence_paths` holds evidence-map
  values, not artefact paths.
- **F-M9** Unit reconciliation is load-time only and aborts the run on an
  irreconcilable unit (`specs/loader.rs:137,146`) instead of failing that
  constraint with a message (SRS-FUNC-004); the evaluator itself is unit-blind.
- **F-M10** `cpu.avg_normalized_pct` is raw GNU-time "Percent of CPU" (can exceed
  100%), not normalised as the field name and AD-015 promise
  (`run_binary.rs:90` → `executor/docker.rs:358`).
- **F-M11** `GET /api/v1/docs` drops `attachments` and raw `.sdoc` text after
  parsing — verbatim baseline evidence never reaches the report/QDP (SRS-FUNC-018);
  a docs-fetch failure silently degrades to "empty" rather than being distinguished.

---

## Low

- rustfmt CI gate is advisory (`continue-on-error: true`) — contradicts
  SRS-QUAL-002 / SRS-CDR-003 "any gate blocks". Make it blocking.
- No release tags exist; SRS-CDR-001 (tagged immutable releases) and the
  tag-traceable version in SRS-DATA-003 are currently unsatisfiable.
- No infrastructure-failure end-to-end test (SRS-VAL-001); INFRA_ERROR is only
  unit-tested.
- No bounded-memory strategy for large payloads (SRS-RES-003) — everything is
  buffered whole.
- SIGTERM handler does not stop the submission loop or cancel in-flight jobs
  server-side (SRS-OPS-008 partial); no cancel operation exists in the API.
- `sqt-reporter/src/metrics/` legacy collector tree is unwired dead code — delete
  to keep the qualification surface honest (relevant to SRS-PORT-002).
- `queue_position` hardcoded to 1 (`handlers.rs:148,215`) though jobs genuinely
  queue behind the runner's `Semaphore(1)`.

---

## Decisions needed (not obviously code-vs-doc)

### D-1 — Metric/constraint extensibility (SRS-FUNC-006 vs. F-H4)
**Decided: option 2.** The ADD review for PDR kept SRS-FUNC-006 and the open-namespace
design; implementation in SQT-SW-1 (issue #17). The options are kept below for the
record.

The code deliberately uses a closed vocabulary (`deny_unknown_fields` +
`Constraint::from_name`). Options:
1. **Accept & reword SRS-FUNC-006** to "extensible by recompiling the open-source
   workspace" (matches AD-012's compile-time-only stance). Cheapest.
2. **Add an open extension namespace**: a `custom.*` metric map on `JobMetrics`
   (kept separate from the strict known fields) plus a generic constraint kind, so
   a project can add a metric without editing the core enums.
Recommendation: decide with the SRS owner; (1) if extensibility is genuinely
out-of-scope for v1, (2) if third-party metrics are a real near-term need.

### D-2 — Runner-side concurrency (SRS-PERF-001)
The reporter must submit+poll concurrently (F-H3) regardless. Separately decide
whether the reference runner stays single-job (`Semaphore(1)`, valid per the SRS)
or gains bounded parallelism; document the chosen bound.

### D-3 — Config key naming remnants (from `TO_UPDATE.md`)
`SqtConfig` was never renamed to `ReporterConfig`, and `token_env` is flat rather
than nested under `runner.auth`. The architecture now documents the flat form as
the target, so this is cosmetic — close U-001's remainder and U-008 (`api/` →
`runner_api/`) as "won't do" unless there's a reason.

---

## Documentation drift — FIXED in the 2026-07-03 pass

These were the doc/code mismatches found in the review; they were corrected while
producing this file:

- `ARCHITECTURE.md` §3 configs rewritten to the real `backend:`/`docker:` runner
  schema and the minimal reporter schema (removed the three fictional runner
  shapes and the nonexistent `ecss_level`/`reports.formats`/`ci_integration`/
  `logging` reporter sections); §3.2 validation rewritten to the real behaviour.
- §8 rewritten from the fictional `Target` trait to the real
  `ExecutionBackend`/`LifecycleBackend` abstraction + `backend:` factory; §8.5
  Docker section corrected to the per-tool-image model.
- §9 report section corrected: JSON/PDF(`printpdf`)/terminal/traceability/QDP are
  the real outputs; HTML marked deferred; Typst template code removed.
- §10 metric-extensibility rewritten (real Docker tool pipeline + backend seam);
  the closed-vocabulary tension is now documented as §10.2.
- §11 tech stack + project structure replaced with the real 3-crate workspace and
  actual dependencies; decision log AD-006/AD-010/AD-023 annotated.
- `IMPLEMENTATION.md` rewritten from the stale reporter-only plan to a current
  three-crate state description.

The OpenAPI `model`/`use_case` mismatch (F-H2) has since been fixed in the
contract file itself (`2e0fc77`).

## Documentation update — 2026-10-15

`ARCHITECTURE.md`, `IMPLEMENTATION.md` and this file were aligned with the PDR
documents: the moved requirement IDs, the term *workload type* (AD-025), the
API-profile qualification of SRS-FUNC-021 to SRS-FUNC-023 (`ARCHITECTURE.md` §4.7,
AD-028), the open-metric-namespace decision (§10.2, AD-029), and the status of the
findings above. `TO_UPDATE.md`, superseded since 2026-07-03, was removed.
