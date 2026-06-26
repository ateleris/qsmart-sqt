#!/usr/bin/env bash
# Build everything needed to run sqt-runner against the sqt-test-binary fixture:
#   1. the sqt-test-binary executable (debug)
#   2. the sqt-run-time:latest Docker image (Ubuntu 24.04 + GNU time)
#
# After this completes, start the runner with:
#   RUST_LOG=info cargo run -p sqt-runner --bin sqt-runner -- -c examples/sqt-runner.test-binary.example.yaml
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IMAGE="sqt-run-time:latest"

echo "==> Building sqt-test-binary (debug)"
cargo build --manifest-path "$REPO_ROOT/Cargo.toml" -p sqt-runner --bin sqt-test-binary

echo "==> Building Docker run image: $IMAGE"
docker build -t "$IMAGE" "$SCRIPT_DIR"

echo
echo "Done. '$IMAGE' is now in your local Docker registry."
echo "Start the runner with:"
echo "  cd $REPO_ROOT"
echo "  RUST_LOG=info cargo run -p sqt-runner --bin sqt-runner -- -c examples/sqt-runner.test-binary.example.yaml"
