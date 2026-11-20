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
**Status update**: 2026-11-19, after the release of `v0.1.0` (SQT-SW-0) at
`6c5b2bb`. Items fixed for the release are marked; the follow-ups the release
left are in "SQT-SW-0 release follow-ups" below.

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
  distinguish "clean" from "did not run". **Fixed** in the runner-robustness
  PR: valgrind runs with `--leak-check=full` (without it the XML had no leak
  records at all) and its report must show a finished run; cppcheck must exit 0
  and find source files; lcov must exit 0. Otherwise the tool's metric is
  `null`, which fails a constraint on it (SRS-REL-002), and the Runner log
  says why. The contract has no per-metric diagnostics field to carry the
  reason to the client.
- **F-M2** Submit-path failure aborts the whole session (`orchestrator/mod.rs:214`
  `?`), and a non-conformant submit response does the same — should be per-job
  INFRA_ERROR (SRS-IF-007/FUNC-020). **Fixed** in `fa71128` (#15): a submit
  failure is now an INFRA_ERROR of that job only.
- **F-M3** Server job failures mislabeled: `wait_for_job` bails on `Failed`
  (`client.rs:269`), so the `RUNNER_JOB_FAILED` branch is unreachable and failures
  surface as `TRANSPORT_ERROR`; the structured `details` field is dropped
  (SRS-IF-003/REL-003). **Fixed** in #35: `wait_for_job` returns the failed job,
  which is recorded as `RUNNER_JOB_FAILED` with the Runner's error message.
- **F-M4** Evidence not run-scoped; JSON report only written with `--output`
  (`main.rs:133`) though SRS-FUNC-009 says "shall produce"; successive runs
  overwrite `report.pdf`/`qdp.zip`/`traceability.*` (SRS-OPS-004). Fix: per-run
  timestamped subdir; always write the JSON report. **Fixed** in #22 (landed via
  #28): every run writes into `runs/<run_id>/`, the JSON report always, with an
  evidence manifest.
- **F-M5** Runner resource leaks: per-job workspace dirs never cleaned up; job
  store grows unbounded with no eviction / queue-depth cap (`handlers.rs`,
  `store.rs`). **Fixed** in the runner-robustness PR: a job's working
  directory is removed when it finishes (`keep_job_workspaces: true` keeps
  it), and only the newest `max_finished_jobs` finished jobs (default 1000)
  stay in the store; an evicted job answers 404 `JOB_NOT_FOUND`. Waiting jobs
  are not capped.
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
  **Fixed** in #21 (landed via #28): an irreconcilable unit fails that constraint
  only.
- **F-M10** `cpu.avg_normalized_pct` is raw GNU-time "Percent of CPU" (can exceed
  100%), not normalised as the field name and AD-015 promise
  (`run_binary.rs:90` → `executor/docker.rs:358`). **Fixed** in the
  runner-robustness PR: divided by the core count (the configured
  `hardware.cores`, else the CPUs available to the Runner) and clamped to
  0..100, as the contract defines it.
- **F-M11** `GET /api/v1/docs` drops `attachments` and raw `.sdoc` text after
  parsing — verbatim baseline evidence never reaches the report/QDP (SRS-FUNC-018);
  a docs-fetch failure silently degrades to "empty" rather than being distinguished.
  **Fixed** in #24: documents and attachments are kept verbatim under
  `evidence/discovery/`; a failed fetch aborts the run before any job is submitted.

---

## Low

- rustfmt CI gate is advisory (`continue-on-error: true`) — contradicts
  SRS-QUAL-002 / SRS-CDR-003 "any gate blocks". Make it blocking. **Fixed** in
  #19; `main` is protected with rustfmt, clippy, supply-chain and test required.
- No release tags exist; SRS-CDR-001 (tagged immutable releases) and the
  tag-traceable version in SRS-DATA-003 are currently unsatisfiable. **Fixed**:
  `v0.1.0`, annotated and signed, released 2026-11-19.
- No infrastructure-failure end-to-end test (SRS-VAL-001); INFRA_ERROR is only
  unit-tested. **Fixed** in #26: `sqt-runner/tests/e2e_test.rs` runs pass, fail and
  infrastructure error against the real reference Runner, in CI and at every tag.
- No bounded-memory strategy for large payloads (SRS-RES-003) — everything is
  buffered whole.
- SIGTERM handler does not stop the submission loop or cancel in-flight jobs
  server-side (SRS-OPS-008 partial); no cancel operation exists in the API.
- `sqt-reporter/src/metrics/` legacy collector tree is unwired dead code — delete
  to keep the qualification surface honest (relevant to SRS-PORT-002). **Fixed**
  in #20 (landed via #28); only the target descriptor remains in `metrics/`.
- `queue_position` hardcoded to 1 (`handlers.rs:148,215`) though jobs genuinely
  queue behind the runner's `Semaphore(1)`. **Fixed** in the runner-robustness
  PR: the place among the waiting jobs, in order of submission.

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

### D-4 — Level-C objectives for SRS-GEN-002
SRS-GEN-002 asks the SQT to enable qualification of the ML-L to ECSS level C.
At SQT-SW-0 it is inspected as "partly fulfilled, as planned", because no list
of the level-C objectives the SQT must cover exists yet. Derive that list from
SoW Annex A R06 and negotiation minutes Annex 1 Item 5, so later releases can
verify GEN-002 against it instead of by judgement.

---

## SQT-SW-0 release follow-ups (2026-11-19)

Left open by the `v0.1.0` release. The documents named here are in
[`qsmart-sqt-documentation`](https://github.com/ateleris/qsmart-sqt-documentation).

- **R-1** Sign-offs for SQT-SW-0. The TRR record
  (`doc/reviews/SQT-SW-0_TRR.md`) needs its date, participants, go/no-go and
  signature. The inspection and analysis notes (docs PR #6) need sign-off by a
  non-author: someone other than Manuel Stutz for CDR-001, DES-001, DES-002,
  DES-003 and PORT-002; someone other than Laszlo Etesi for GEN-001 and GEN-002.
- **R-2** The document cover still says "All rights reserved. Proprietary and
  confidential.", although the SoW (Appendix 1, §3.4) distributes SQT-SW-N
  including its documentation under ESA-PL Permissive v2.4 (SRS-DES-002). The
  fix is docs PR #7; merge it after the PDR and reissue the documents at their
  next issue. **Fixed** in docs #7 (merged 2026-11-19); the documents carry the
  new line from their next issue.
- **R-3** The CIDL does not list the software release. It is generated from
  `project_config.yml`, and `external_cis` has no field for a tag or commit.
  Either add SQT-SW-0 there with the tag and commit in the title, or extend the
  generator in `baseline`. SQT-TN-AP records source as "commit identifier;
  release tag" in the CIDL. **Fixed** in docs #8: SQT-SW-0 is an external CI
  with tag and commit in its title. Its section heading reads "SW - SW" until
  `SW` is added to `DOC_TYPE_REGISTRY` in the baseline.
- **R-4** `ci/release-notes.py` says each image must load with the ID in
  `IMAGES.txt`. That only holds on Docker's containerd image store; the classic
  store shows the config digest. The SRelD I1R1 lists both IDs. Fix the wording,
  or write both IDs into `IMAGES.txt`. **Fixed** in the `image-records` PR:
  `IMAGES.txt` records both digests and the notes explain which one applies.
- **R-5** `IMAGES.txt` of `v0.1.0` does not record the Docker Engine version of
  the build. The line that adds it is in PR #31, which is not merged. Take it
  over independently of GHCR. **Fixed** in the `image-records` PR, with the
  image store of the build.
- **R-6** Publishing the images to GHCR. The old PR #31 was closed: it was
  based on `main` from before the 2026-11-19 rewrite. Its workflow is carried
  over, adapted to the three-field `IMAGES.txt`, in #36 (merged). Still
  needed before it can push: an org admin grants `ateleris/qsmart-sqt` write
  access to container packages (GHCR answered `denied` on 2026-10-30 although
  the login succeeded), and the packages are made public. Not needed for
  delivery: the images are release assets.
- **R-7** cargo-audit warnings at `v0.1.0`, allowed by policy: `chrono`
  unmaintained (RUSTSEC-2026-0543) and `unsafe-libyaml` unmaintained
  (RUSTSEC-2024-0449), both in the delivered binaries; `rustix` unsound in
  `xattr` (RUSTSEC-2026-0486), test dependency only through `tempfile`. Look for
  maintained replacements before SQT-SW-1.
- **R-8** The SRS defines the `Generic` allocation as "constrains the
  Orchestrator-Runner API contract itself" (SRS-GEN-001 and the attribute
  table), but 13 of the 21 `Generic` requirements constrain the SQT or the
  project as a whole: GEN-001, GEN-002, FUNC-005, FUNC-022, DES-002, DES-003,
  QUAL-001, QUAL-002, QUAL-003, CDR-003, CDR-004, DATA-002, VAL-001. Proposal:
  widen the definition to "the Runner API contract, or the SQT and its delivery
  as a whole". This is an SRS change, so it goes to the ESA Technical Officer
  and the ML-L role at the SQT-SW-1 release planning (SQT-TN-AP, change
  control).
- **R-9** Known limitations of `v0.1.0` carried into SQT-SW-1, as recorded in the
  SRelD: FUNC-019 fan-out (F-M7), `repetitions` not sent (IF-002, F-M6), OpenAPI
  range and pattern validation (IF-007), Runner-declared metrics (FUNC-006,
  issue #17, D-1).
- **R-10** SRS-IF-010 (encrypted transport) is in the SRS but in no release of
  the SQT-TN-AP release plan, which predates it. It is also not implemented:
  the Orchestrator verifies TLS certificates (rustls), but it would send the
  bearer token over plain `http://` to any host, where IF-010 allows that only
  for the local host. Assign it at the SW-1 release planning; the fix is a
  configuration check that rejects a token with a non-loopback `http://` URL.

---

## SQT-SW-1 plan (2026-11-19)

SQT-SW-1 is `v0.2.0`, reviewed at the project-level CDR (about T0+8M, date to
be agreed with ESA after the PDR). Its SRS group per SQT-TN-AP: FUNC-005, 006,
010..015, 021..023; PERF-001..004; REL-001..004; OPS-001..008; HF-001, 002;
DATA-001..003; RES-001..003; MAINT-001, 002 (37 requirements). Delivery focus:
metric collectors, traceability matrix, verification-activity triggering,
operator guidance, performance and reliability; the fulfilment matrix becomes
authoritative.

State on `dev` at `88db294`, from the test metadata against SRS I1R2 (a
requirement "has tests" when a test names it):

| State | Requirements |
|---|---|
| Has tests (20) | FUNC-005, 010; PERF-001..004; REL-001..004; OPS-001, 005, 006, 008; DATA-001..003; RES-001 |
| Implemented, no test names it | OPS-002 (verbosity), OPS-003 (end-of-run summary), OPS-007 (pre-flight), RES-002 (no implicit network access), HF-001 (self-discoverable interface). **Done** in #38 (P-1). |
| Feature work | FUNC-006 (Runner-declared metrics, issue #17, D-1); FUNC-011, 012, 013 (configuration-driven run, operator guidance, no manual steps); FUNC-014, 015 (verification activities through the Runner API, needs a contract change); FUNC-021..023 (API profile, representative workloads, profile coverage); RES-003 (bounded memory); HF-002 (end-user operability) |
| Verified by a note | Analysis: FUNC-006, MAINT-001, MAINT-002. Inspection: FUNC-015, OPS-004 |

Correction: an earlier version of this table listed FUNC-012 and FUNC-016 as
having no verification method. Both are Test in the SRS; the matrix script
misread them (a statement line starting with `[LINK:` ended the requirement
block early). Fixed together with P-2.

Steps, in this order:

- **P-1** Quick wins: write or tag the tests for OPS-002, OPS-003, OPS-007,
  RES-002 and HF-001.
- **P-2** Make the fulfilment matrix authoritative: run `ci/fulfilment-matrix.py`
  with `--srs` in CI, against the SRS of the docs repository at a pinned tag,
  so every requirement without a passing test shows up on each push.
  **Done** in the `p2-requirements-list` PR, with a requirement list instead of
  the SRS: the docs repository is private, and a release must build from a
  clean clone without secrets (SRS-CDR-002). `ci/requirements.csv` holds UID,
  title, verification method and planned release, generated from the SRS and
  the SQT-TN-AP release plan by `ci/requirements-from-srs.py`; regenerate it at
  every new SRS or plan issue. Every push reports per release in the job
  summary; a release tag fails unless every Test requirement planned up to that
  release has a passing test or CI gate.
- **P-3** Carry-overs and robustness: IF-002 `repetitions` together with the
  Runner repetition loop (F-M6); FUNC-019 fan-out (F-M7); F-M1, F-M5, F-M10;
  SIGTERM cancelling in-flight jobs (OPS-008); `queue_position`; R-7.
- **P-4** Features, after the PDR outcome and the release planning: FUNC-006
  (#17); FUNC-014/015 with the contract change; FUNC-021..023; FUNC-011, 013;
  RES-003; HF-002.
- **P-5** Notes for the analysis and inspection requirements above, signed off
  by a non-author.

Process for SQT-SW-1, not code:

- Release planning with ESA and the ML-L role (SQT-TN-AP rolling outlook).
  SRS change requests to raise there: R-8 (`Generic` allocation). Also assign
  SRS-IF-010 to a release (R-10).
- TRR at the start and the end of the release; lightweight CDR.
- ADD and SUM kept current with each feature (Definition of Done).
- Branching: work now goes to `dev` and reaches `main` at a release, with the
  release tag on `main`. SQT-TN-AP describes trunk-based development (feature
  branches into `main`), so describe the `dev` branch at its next issue.
  Protect `dev` like `main` if PRs into it should require green checks.

Afterwards, SQT-SW-2 (`v1.0.0`, QR): FUNC-016, 017 (implemented, tested),
CDR-004 (complete SUM and documentation), VAL-001, 002, and end-to-end QDP
generation on GR740 and GR765/NOEL-V emulation through target Runners.

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
