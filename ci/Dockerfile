# Build image for ats-sqt CI.
#
# This is NOT a runner image. The self-hosted runner stays thin (agent +
# docker CLI, see ci/runner/) and launches this image per job via
# `docker run` against its dind daemon — the toolchain lives here.
#
# Built on demand by the workflow (`docker build - < ci/Dockerfile`) and
# cached in dind's persistent storage; never pushed to a registry.
FROM rust:1-slim-trixie

# Build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    build-essential \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Node 24 via NodeSource (used by the coverage-summary parse in ci/run-tests.sh)
RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Coverage & test tooling
RUN rustup component add llvm-tools-preview \
    && cargo install cargo-llvm-cov --locked \
    && cargo install cargo-nextest --locked
