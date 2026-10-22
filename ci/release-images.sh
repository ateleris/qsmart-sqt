#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Ateleris GmbH
# SPDX-License-Identifier: ESA-PL-permissive-2.4
# Runs on the CI runner (needs the docker CLI); cwd is the repo checkout.
# Builds the two release images from the binaries in dist/bin (written by
# ci/release-build.sh) and saves each as a gzip'd OCI tarball in dist/:
#   sqt-reporter-<version>-image.tar.gz   FROM scratch, static sqt-reporter
#   sqt-runner-<version>-image.tar.gz     FROM docker:cli, static sqt-runner
# dist/IMAGES.txt records each image ID (its content digest) for the SRelD.
#
# Usage: ci/release-images.sh <version>
set -euo pipefail

VERSION="${1:?usage: release-images.sh <version>}"
: "${SOURCE_DATE_EPOCH:?set SOURCE_DATE_EPOCH to the tag commit time}"
CREATED=$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ)
REVISION=$(git rev-parse HEAD)

: > dist/IMAGES.txt
for bin in sqt-reporter sqt-runner; do
  tag="$bin:$VERSION"
  docker build -f "release/Dockerfile.$bin" -t "$tag" \
    --label org.opencontainers.image.title="$bin" \
    --label org.opencontainers.image.version="$VERSION" \
    --label org.opencontainers.image.revision="$REVISION" \
    --label org.opencontainers.image.created="$CREATED" \
    --label org.opencontainers.image.source="https://github.com/ateleris/qsmart-sqt" \
    --label org.opencontainers.image.licenses="ESA-PL-permissive-2.4" \
    dist/bin
  docker save "$tag" | gzip -n -9 > "dist/$bin-$VERSION-image.tar.gz"
  echo "$tag $(docker image inspect --format '{{.Id}}' "$tag")" >> dist/IMAGES.txt
done
cat dist/IMAGES.txt
