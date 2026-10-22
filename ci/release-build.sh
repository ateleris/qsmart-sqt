#!/usr/bin/env bash
# Runs inside the ci build image (ci/Dockerfile); cwd is the repo checkout.
# Builds the static release binaries and packages them into dist/:
#   sqt-reporter-<version>-x86_64-linux-musl.tar.gz   binary + LICENSE + README.md
#   sqt-runner-<version>-x86_64-linux-musl.tar.gz     binary + LICENSE + README.md
#   sqt-reporter-<version>.cdx.json, sqt-runner-<version>.cdx.json   (from sbom/)
#   bin/sqt-reporter, bin/sqt-runner                  build context for the images
#
# Usage: ci/release-build.sh <version>   (version without the leading "v")
# Expects ci/supply-chain.sh to have written sbom/. SOURCE_DATE_EPOCH (the tag
# commit time) fixes the archive timestamps so the tarballs are reproducible.
set -euo pipefail

VERSION="${1:?usage: release-build.sh <version>}"
TARGET=x86_64-unknown-linux-musl
: "${SOURCE_DATE_EPOCH:?set SOURCE_DATE_EPOCH to the tag commit time}"

# The tag must match the crate versions.
for crate in sqt-types sqt-reporter sqt-runner; do
  v=$(sed -n 's/^version = "\(.*\)"/\1/p' "$crate/Cargo.toml" | head -1)
  if [ "$v" != "$VERSION" ]; then
    echo "::error::$crate is version $v, tag says $VERSION" >&2
    exit 1
  fi
done

# Keep build paths out of the binaries.
export RUSTFLAGS="--remap-path-prefix=$PWD=/build --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
cargo build --locked --release --target "$TARGET" --bin sqt-reporter --bin sqt-runner

TARGET_DIR="${CARGO_TARGET_DIR:-target}"
rm -rf dist && mkdir -p dist/bin
for bin in sqt-reporter sqt-runner; do
  install -m 0755 "$TARGET_DIR/$TARGET/release/$bin" "dist/bin/$bin"
  # A dynamically linked binary lists its libraries as "name => path".
  if ldd "dist/bin/$bin" 2>&1 | grep -q '=>'; then
    echo "::error::$bin is dynamically linked:" >&2
    ldd "dist/bin/$bin" >&2
    exit 1
  fi

  stage="dist/stage/$bin-$VERSION"
  mkdir -p "$stage"
  cp "dist/bin/$bin" LICENSE README.md "$stage/"
  tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
    -C dist/stage -cf - "$bin-$VERSION" | gzip -n -9 > "dist/$bin-$VERSION-x86_64-linux-musl.tar.gz"

  cp "sbom/$bin.cdx.json" "dist/$bin-$VERSION.cdx.json"
done
cp LICENSE dist/bin/
rm -rf dist/stage
# This runs as root in the container; the image step runs on the runner host.
chmod -R a+rwX dist
ls -l dist dist/bin
