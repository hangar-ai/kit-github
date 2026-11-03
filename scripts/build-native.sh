#!/bin/sh
# Builds the two tabs to WebAssembly and puts the module where hangar-kit.json
# says (`native` of both tabs). Hangar runs the .wasm a kit ships and never
# builds one: commit tabs/github.wasm with the source that made it.
#
# The same sources and the same toolchain (rust-toolchain.toml) give the same
# bytes on any machine: the folders of the checkout and of cargo are written
# as /kit and /cargo in the panic messages the module carries, and the SDK's
# revision is the one native/Cargo.lock holds. CI builds it again and fails
# if tabs/github.wasm differs.
#
# Needs rustup (the target comes with the toolchain file).
set -eu
cd "$(dirname "$0")/.."
root="$(pwd)"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
RUSTFLAGS="--remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$root=/kit" \
  cargo build --manifest-path native/Cargo.toml --release --locked --target wasm32-unknown-unknown
cp native/target/wasm32-unknown-unknown/release/kit_github_tab.wasm tabs/github.wasm
echo "tabs/github.wasm"
