#!/bin/sh
# Builds one target and packages it as a tarball with a checksum beside it.
#
# `make dist` and the release workflow both go through here, so a tarball you
# build on your own machine is put together exactly like a published one.
#
# usage: scripts/dist.sh <rust target triple>
set -eu

target="${1:?usage: scripts/dist.sh <rust target triple>}"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
name="k9mux-$version-$target"
out=dist

# A release runner starts with no targets but the one it runs on.
if command -v rustup >/dev/null 2>&1; then
    rustup target add "$target" >/dev/null 2>&1 || true
fi

cargo build --release --locked --target "$target"
built="${CARGO_TARGET_DIR:-target}/$target/release/k9mux"

rm -rf "$out/$name"
mkdir -p "$out/$name"
cp "$built" "$out/$name/k9mux"
cp config.example "$out/$name/config.example"
cp -R docs "$out/$name/docs"
for extra in README.md LICENSE LICENSE.md; do
    if [ -f "$extra" ]; then
        cp "$extra" "$out/$name/"
    fi
done

tar -czf "$out/$name.tar.gz" -C "$out" "$name"
rm -rf "$out/$name"

# sha256sum on Linux, shasum on macOS.
cd "$out"
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$name.tar.gz" > "$name.tar.gz.sha256"
else
    shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256"
fi
cat "$name.tar.gz.sha256"
