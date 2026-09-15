#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Prerequisites: rustup target add x86_64-unknown-linux-musl x86_64-pc-windows-gnu
# Debian/Ubuntu: apt-get install musl-tools gcc-mingw-w64-x86-64-posix
version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
name="codex-prompt-history"
mkdir -p dist

cargo build --locked --release --target x86_64-unknown-linux-musl
cargo build --locked --release --target x86_64-pc-windows-gnu

for platform in linux windows; do
    package="$name-v$version-$platform-amd64"
    stage=$(mktemp -d)
    trap 'rm -rf "$stage"' EXIT
    mkdir -p "$stage/$package"
    cp README.md "$stage/$package/"
    if [[ "$platform" == linux ]]; then
        cp "target/x86_64-unknown-linux-musl/release/$name" "$stage/$package/"
        tar -czf "dist/$package.tar.gz" -C "$stage" "$package"
    else
        cp "target/x86_64-pc-windows-gnu/release/$name.exe" "$stage/$package/"
        python3 - "$stage" "$package" "$PWD/dist/$package.zip" <<'PY'
import pathlib, sys, zipfile
stage, package, output = sys.argv[1:]
with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
    for path in sorted((pathlib.Path(stage) / package).iterdir()):
        archive.write(path, path.relative_to(stage))
PY
    fi
    rm -rf "$stage"
    trap - EXIT
done
(
    cd dist
    sha256sum "$name-v$version-linux-amd64.tar.gz" "$name-v$version-windows-amd64.zip" > SHA256SUMS
)
echo "Release archives and SHA256SUMS are in dist/"
