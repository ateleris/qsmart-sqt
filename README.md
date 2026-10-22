# SQT — Software Qualification Toolkit

SQT qualifies a Machine Learning Library (ML-L) for use on space hardware. It
runs a binary-under-test, measures it (execution time, memory, CPU, leaks,
coverage, static analysis, …), checks the results against
requirement-derived constraints, and produces a qualification report and
evidence bundle.

Developed by Ateleris in the context of the ESA Q-SMART project.

## Workspace layout

A Cargo workspace (Rust, edition 2024) of three crates:

| Crate | Role |
|-------|------|
| [`sqt-types`](sqt-types) | Shared DTOs — the `job-api.openapi.yaml` HTTP contract expressed in Rust. |
| [`sqt-runner`](sqt-runner) | Reference **SQT Runner**: an `axum` HTTP server that executes the binary-under-test in Docker tool containers and measures it. |
| [`sqt-reporter`](sqt-reporter) | The client CLI (the **Orchestrator**): loads config + specs, submits/polls jobs, evaluates constraints, and writes the report + traceability + evidence bundle. |

The two components communicate over the HTTP contract defined in
[`job-api.openapi.yaml`](job-api.openapi.yaml). `sqt-runner` is a *reference*
implementation; a real qualification may target another runner (e.g. actual
flight hardware) that speaks the same API.

## Requirements

Two distinct layers:

1. **Requirements on SQT itself** (`SRS-*`) — defined in the project SRS
   (StrictDoc). Compliance is tracked in [`REVIEW_FINDINGS.md`](REVIEW_FINDINGS.md).
2. **Requirements the ML-L must satisfy** (`REQ-ML-L-*`) — defined per run as
   **test specs**: YAML files (see [`examples/specs`](examples/specs)) mapping a
   requirement UID to a workload `use_case` and a list of metric constraints
   (e.g. `time.avg_ms <= 5s`).

## Quickstart

Build and test:

```
cargo build --workspace
cargo test --workspace
```

The toolchain is pinned in `rust-toolchain.toml`; rustup installs it on first
use. TLS is pure Rust (rustls), so no OpenSSL or other system library is needed.

Static release binaries (self-contained, no runtime dependencies) are built for
musl. The only package needed besides rustup is a musl C compiler, used by
`ring` (on Debian/Ubuntu: `apt install musl-tools`):

```
cargo build --locked --release --target x86_64-unknown-linux-musl
```

Run a qualification (the runner needs Docker):

```
# 1. Start the reference runner against a config describing the binary + images
cargo run -p sqt-runner -- -c examples/sqt-runner.example.yaml

# 2. (optional) Validate config, connectivity, auth, and specs without running jobs
cargo run -p sqt-reporter -- preflight -c examples/sqt-reporter.example.yaml

# 3. Run the session; writes report.json/report.pdf/traceability.*/qdp.zip + evidence
cargo run -p sqt-reporter -- run -c examples/sqt-reporter.example.yaml -o report.json
```

The reporter's exit code gates CI: `0` all-pass, `1` any failed or
infrastructure-errored spec, `2` interrupted.

## Documentation

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — target design.
- [`IMPLEMENTATION.md`](IMPLEMENTATION.md) — current implementation state.
- [`job-api.openapi.yaml`](job-api.openapi.yaml) — the runner ↔ reporter contract.

Additional strictdoc project at https://github.com/ateleris/qsmart-sqt-documentation.  
Can be linked in:
```bash
cd documentation
git clone git@github.com:ateleris/qsmart-sqt-documentation.git
```

## Licence

Copyright © Ateleris.

Licensed under the **European Space Agency Public License (ESA-PL) Permissive –
v2.4** (SPDX identifier `ESA-PL-permissive-2.4`). See [`LICENSE`](LICENSE) for the
full text.
