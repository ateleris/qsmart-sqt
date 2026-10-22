#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Ateleris GmbH
# SPDX-License-Identifier: ESA-PL-permissive-2.4
# Runs inside the ci build image (ci/Dockerfile); cwd is the repo checkout.
# Dependency gates (SRS-CDR-003): licences, bans and sources (cargo-deny,
# deny.toml), security advisories (cargo-audit, RustSec database) and the SPDX
# header of every Rust file. Then writes one CycloneDX SBOM per delivered
# binary into sbom/.
set -euo pipefail

cargo deny --locked check licenses bans sources
cargo audit

# Every Rust source names its licence and copyright holder (SRS-DES-002).
missing=$(grep -rL --include='*.rs' 'SPDX-License-Identifier: ESA-PL-permissive-2.4' sqt-*/src sqt-*/tests || true)
if [ -n "$missing" ]; then
  echo "::error::Rust files without the SPDX header:"
  echo "$missing"
  exit 1
fi

rm -rf sbom && mkdir -p sbom
cargo cyclonedx --format json --spec-version 1.5 \
  --target x86_64-unknown-linux-musl --no-build-deps --describe binaries \
  --license-accept-named ESA-PL-permissive-2.4 -q
for bin in sqt-reporter sqt-runner; do
  mv "$bin/${bin}_bin.cdx.json" "sbom/$bin.cdx.json"
done
# The test fixture binary is not delivered.
find . -name '*.cdx.json' -not -path './sbom/*' -not -path './target/*' -delete
ls -l sbom
