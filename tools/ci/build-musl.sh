#!/bin/sh
# Called inside rust:alpine3.22 by game-build.yml, without release credentials.
set -eu

apk add --no-cache build-base pkgconf alsa-lib-dev eudev-dev python3 git
git config --global --add safe.directory /work
# Match the stable channel used by the other release jobs, not the image's bundled Rust.
rustup update stable --no-self-update
export RUSTUP_TOOLCHAIN=stable
rustup component add llvm-tools

# Static musl cannot dlopen the graphics/window libraries. Keep audio and controllers,
# and link against Alpine's native libraries rather than Ubuntu's glibc libraries.
export RUSTFLAGS='-C target-feature=-crt-static'
target=x86_64-unknown-linux-musl
bin="target/$target/release"
cargo build --release --locked --target "$target" \
  -p horizon-client -p horizon-client-lib --bin horizon-client --bin horizon-ctl \
  -p horizon-prepare --bin horizon-prepare -p horizon-server --bin horizon-server

set -- "$bin/horizon-client" "$bin/horizon-ctl" "$bin/horizon-prepare" "$bin/horizon-server"
python3 tools/check_imports.py --musl "$@"
python3 tools/check_cpu_baseline.py "$@"

# Even --help must never touch real player data.
export HORIZON_HOME="$(mktemp -d)"
export HORIZON_LAUNCHER_HOME="$(mktemp -d)"
export XDG_DATA_HOME="$(mktemp -d)"
export XDG_CONFIG_HOME="$(mktemp -d)"
export HORIZON_LAUNCHER_NO_SELF_UPDATE=1 HORIZON_NO_AUDIO=1
"$bin/horizon-client" --help > "$HORIZON_HOME/client-help"
grep -q -- '--resources' "$HORIZON_HOME/client-help"
"$bin/horizon-ctl" --help > /dev/null
"$bin/horizon-prepare" --help > "$HORIZON_HOME/prepare-help"
grep -q -- '--minecraft-dir' "$HORIZON_HOME/prepare-help"
for binary in horizon-prepare horizon-server; do
  test "$("$bin/$binary" --version)" = "$binary $HORIZON_VERSION"
done

cargo run --locked --target "$target" -p xtask -- package \
  "$bin/horizon-client" dist/horizon-linux-musl-x86_64
tar -C dist -czf dist/horizon-linux-musl-x86_64.tar.gz horizon-linux-musl-x86_64
