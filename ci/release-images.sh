#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Ateleris GmbH
# SPDX-License-Identifier: ESA-PL-permissive-2.4
# Runs on the CI runner (needs the docker CLI); cwd is the repo checkout.
# Builds the two release images from the binaries in dist/bin (written by
# ci/release-build.sh) and saves each as a gzip'd OCI tarball in dist/:
#   sqt-reporter-<version>-image.tar.gz   FROM scratch, static sqt-reporter
#   sqt-runner-<version>-image.tar.gz     FROM docker:cli, static sqt-runner
# dist/IMAGES.txt records, for the SRelD, the Docker Engine and image store of
# the build and two digests per image: the image ID this daemon reports, and the
# image configuration digest. `docker image inspect --format '{{.Id}}'` shows the
# first on a daemon with the containerd image store and the second on a daemon
# with the classic store, so a recipient can check a loaded image either way.
#
# Usage: ci/release-images.sh <version>
set -euo pipefail

VERSION="${1:?usage: release-images.sh <version>}"
: "${SOURCE_DATE_EPOCH:?set SOURCE_DATE_EPOCH to the tag commit time}"
CREATED=$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ)
REVISION=$(git rev-parse HEAD)

store=classic
if docker info --format '{{json .DriverStatus}}' | grep -q 'io.containerd.snapshotter'; then
  store=containerd
fi
{
  echo "# Docker Engine $(docker version --format '{{.Server.Version}}'), $store image store"
  echo "# <image> <image ID on the build daemon> <configuration digest>"
} > dist/IMAGES.txt
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
  archive="dist/$bin-$VERSION-image.tar.gz"
  docker save "$tag" | gzip -n -9 > "$archive"
  # The configuration digest is named in the archive's manifest.json.
  config=$(gzip -dc "$archive" | tar -xO manifest.json \
    | sed -n 's|.*"Config":"blobs/sha256/\([0-9a-f]\{64\}\)".*|\1|p')
  if [ -z "$config" ]; then
    echo "::error::no configuration digest in the manifest of $archive" >&2
    exit 1
  fi
  echo "$tag $(docker image inspect --format '{{.Id}}' "$tag") sha256:$config" >> dist/IMAGES.txt
done
cat dist/IMAGES.txt
