# Contributing to the SQT

How changes are made, checked and released. The process itself (roles, reviews,
Definition of Done, release plan) is defined in SQT-TN-AP (SQT-PL-0002-ATS);
this file is the practical side of it for this repository.

## Set-up

- Rust toolchain: pinned in `rust-toolchain.toml` (1.96.0, with clippy, rustfmt,
  `llvm-tools-preview` and the `x86_64-unknown-linux-musl` target). `rustup`
  installs it on first use.
- For the static release build: Debian/Ubuntu `musl-tools`.
- For the reference Runner and the end-to-end test: Docker.
- For the test reports used by CI: `cargo-nextest` and `cargo-llvm-cov`, at the
  versions pinned in `ci/Dockerfile`. The CI image (`ci/Dockerfile`) has all of
  the above; `docker build -t ats-sqt-ci - < ci/Dockerfile` gives you the same
  environment locally.

## Branches and pull requests

- `main` holds the releases. Every release tag is on `main`.
- `dev` is the integration branch for the next release. Work happens on short
  feature branches taken from `dev` and merged back through a pull request.
- A pull request is merged when CI is green and a reviewer who is not its
  author has approved it.
- At a release, `dev` is merged into `main` and the tag is placed on `main`.
- A fix to a released version is made on a branch from its tag and merged
  forward into `main` and `dev`.

`main` is protected: no force-push, no deletion, and the checks rustfmt,
clippy, supply-chain and test must pass.

## Commits

- Write the message the way you would explain the change to a colleague: a
  short summary line, then what changed and why. No `feat(x):` style prefixes.
- Commits are signed (SSH signature).
- Every new `.rs` file and CI script starts with the SPDX header; the
  supply-chain job fails without it:

  ```
  // SPDX-FileCopyrightText: 2026 Ateleris GmbH
  // SPDX-License-Identifier: ESA-PL-permissive-2.4
  ```

## Checks to run before pushing

These are the checks CI runs; all of them block a merge.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 ci/fulfilment-matrix.py --requirements ci/requirements.csv
```

- **Lints.** The clippy ruleset (pedantic) is declared once in the root
  `Cargo.toml` under `[workspace.lints]`. `#[allow]` is itself linted: suppress
  a lint only with `#[expect(clippy::..., reason = "...")]`. An expectation
  that no longer fires fails the build, so remove it then.
- **Formatting.** `rustfmt.toml` uses stable options only, so plain
  `cargo fmt` matches CI.
- **End-to-end test** (needs Docker): the Orchestrator against the real
  reference Runner.

  ```bash
  cargo test -p sqt-runner --test e2e_test -- --ignored
  ```

  On a Docker-in-Docker host, set `SQT_E2E_WORKDIR` to a directory that the
  Docker daemon sees at the same path (see the top of
  `sqt-runner/tests/e2e_test.rs`).
- **Supply chain** (`ci/supply-chain.sh`): cargo-deny (licences, bans,
  sources, `deny.toml`), cargo-audit (RustSec advisories), the SPDX header
  check and the CycloneDX SBOM.
- **Coverage.** `ci/run-tests.sh` measures statement coverage on every run. A
  release tag fails if the Orchestrator (`sqt-reporter`) is below 80 % of
  lines.

## Tests name the requirement they verify

Every test states which SRS requirement it verifies, in a comment line
directly above its test attribute:

```rust
/// Verifies: SRS-OPS-002, SRS-OPS-005
#[tokio::test]
async fn most_verbose_level_logs_request_and_response_payloads() { … }
```

A test that verifies no SRS requirement says why:
`/// Verifies: none (reason)`.

`ci/fulfilment-matrix.py` reads these lines and builds the requirements
fulfilment matrix. It fails the job when a test has no such line, when the
line is malformed, or when it names a requirement that is not in the SRS.

Points to keep in mind when you write or review a test:

- The tag is a claim. A test must fail when the requirement it names is
  broken. Check that by breaking the code for a moment and watching the test
  fail.
- A requirement with several clauses needs a test for each clause, not one
  test for the whole requirement.
- Name only the requirements the test really checks. A test that merely passes
  through some code does not verify it.
- Keep tests out of a binary's `main.rs`: the matrix script cannot attribute
  them there. Put the code under test in the library.

### The requirement list

CI has no access to the SRS, which lives in the private documentation
repository. It reads `ci/requirements.csv` instead: every SRS requirement with
its title, verification method and the release the SQT-TN-AP release plan
gives it. The file records the documentation tag and the SHA-256 of the SRS
it was generated from.

Regenerate it whenever the SRS or the release plan gets a new issue:

```bash
git -C ../qsmart-sqt-documentation checkout <docs tag>
python3 ci/requirements-from-srs.py \
  --srs ../qsmart-sqt-documentation/deliverables/editable/SQT-SRS_Software_Requirements_Specification_SRS.sdoc \
  --source "<docs tag> (<commit>)"
```

The release groups are listed in `ci/requirements-from-srs.py`; change them
there together with the release plan.

On every push, the test job's summary shows, per release, how many Test
requirements have a passing test and which are missing. Requirements verified
by Inspection, Analysis or Review of Design are covered by signed notes in the
release's evidence package, not by CI.

## Documentation

- Code documentation is part of the Definition of Done. Every module starts
  with a `//!` description, and every public item has a `///` doc that says
  what it is for and any non-obvious contract (units, error cases, the SRS
  requirement it implements). CI builds the docs with warnings as errors.
- When a change alters behaviour an operator sees, update the Software User
  Manual (SUM) in the documentation repository in the same piece of work. When
  it changes the design, update the ADD and `ARCHITECTURE.md`.
- Keep `IMPLEMENTATION.md` (what is implemented) and `REVIEW_FINDINGS.md` (the
  backlog) current.

## Releasing

A release is cut from `main` once its requirement group is complete (see the
SW plan in `REVIEW_FINDINGS.md`).

1. Merge `dev` into `main` through a pull request.
2. Set the version in the three crates' `Cargo.toml` to the release version.
   `ci/release-build.sh` refuses a tag that does not match.
3. Create a signed, annotated tag on `main` and push it:

   ```bash
   git tag -s v0.2.0 -m "SQT v0.2.0 (SQT-SW-1)"
   git push origin v0.2.0
   ```

4. `.github/workflows/release.yml` runs on the tag:
   - refuses a lightweight or unsigned tag (SRS-CDR-001);
   - reruns every gate, with the coverage gate and the requirements gate: every
     Test requirement planned up to this release must have a passing test;
   - runs the end-to-end test;
   - builds the static binaries, the two container images, the SBOMs and the
     source archive, reproducibly from the tag commit time;
   - writes `SHA256SUMS`, `IMAGES.txt` and the release notes
     (`ci/release-notes.py`);
   - opens a **draft** GitHub release with all of them.
5. Review the draft and publish it. Publishing starts
   `.github/workflows/publish-images.yml`, which pushes the images to GHCR.
6. Record the tag commit, the SHA-256 values and the image IDs in the Software
   Release Document (SRelD) and update the CIDL, in the documentation
   repository.
7. Hold the TRR on the tagged release; it uses the release's test report,
   coverage and fulfilment matrix.

To test the pipeline without a real release, push a throwaway tag such as
`v0.2.0-dryrun.1` on a throwaway branch with the crate versions set to match,
then delete the tag, the draft release and the branch.
