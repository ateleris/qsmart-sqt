#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Ateleris GmbH
# SPDX-License-Identifier: ESA-PL-permissive-2.4
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

images=(
    "Dockerfile.run        sqt-run:latest"
    "Dockerfile.valgrind   sqt-valgrind:latest"
    "Dockerfile.coverage   sqt-coverage:latest"
    "Dockerfile.sonar      sqt-sonar:latest"
    "Dockerfile.static     sqt-static:latest"
)

for entry in "${images[@]}"; do
    read -r dockerfile tag <<< "$entry"
    echo "--- Building $tag from $dockerfile ---"
    docker build -t "$tag" -f "$SCRIPT_DIR/$dockerfile" "$SCRIPT_DIR"
    echo ""
done

echo "All images built:"
for entry in "${images[@]}"; do
    read -r _ tag <<< "$entry"
    echo "  $tag"
done
