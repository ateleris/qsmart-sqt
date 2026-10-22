#!/usr/bin/env bash
# Runs inside the ci build image (ci/Dockerfile); cwd is the repo checkout.
# Dependency gates (SRS-CDR-003): licences, bans and sources (cargo-deny,
# deny.toml) and security advisories (cargo-audit, RustSec database). Then
# writes one CycloneDX SBOM per delivered binary into sbom/.
set -euo pipefail

cargo deny --locked check licenses bans sources
cargo audit

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
