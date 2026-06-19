FROM myoung34/github-runner:latest

# Build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Node 24 via NodeSource
RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Rust via rustup, pinned to the toolchain used in this project
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain 1.95 --profile minimal --no-modify-path \
    && chmod -R a+w /usr/local/rustup /usr/local/cargo

# Coverage & test tooling
RUN rustup component add llvm-tools-preview \
    && cargo install cargo-llvm-cov --locked \
    && cargo install cargo-nextest --locked
