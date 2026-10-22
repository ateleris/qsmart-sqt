# Build image for ats-sqt CI.
#
# This is NOT a runner image. The self-hosted runner stays thin (agent +
# docker CLI, see ci/runner/) and launches this image per job via
# `docker run` against its dind daemon — the toolchain lives here.
#
# Built on demand by the workflow (`docker build - < ci/Dockerfile`) and
# cached in dind's persistent storage; never pushed to a registry.
# Tag must match the channel in rust-toolchain.toml.
FROM rust:1.96.0-slim-trixie

# Build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    musl-tools \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Node 24 via NodeSource (used by the coverage-summary parse in ci/run-tests.sh)
RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Coverage & test tooling. Versions are pinned for reproducible CI and are
# listed in the release's SRelD; bump them deliberately.
ARG CARGO_LLVM_COV_VERSION=0.9.1
ARG CARGO_NEXTEST_VERSION=0.9.149
RUN rustup component add llvm-tools-preview clippy rustfmt \
    && rustup target add x86_64-unknown-linux-musl \
    && cargo install cargo-llvm-cov --locked --version "$CARGO_LLVM_COV_VERSION" \
    && cargo install cargo-nextest --locked --version "$CARGO_NEXTEST_VERSION"
