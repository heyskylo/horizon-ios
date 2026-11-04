# Windows ARM64

Horizon builds the game and launcher as native Windows ARM64 executables for
`aarch64-pc-windows-msvc`. GitHub Actions builds and checks them on a Windows ARM64 runner.

## Build from source

Use Windows 11 on ARM, or cross-compile from Windows x64. Install stable Rust, the
`aarch64-pc-windows-msvc` Rust target, Visual Studio C++ build tools for ARM64, and a Windows SDK
with ARM64 libraries. Install LLVM/Clang and put `clang.exe` on `PATH`; the `ring` dependency uses
Clang to compile its ARM64 code. The GitHub-hosted ARM64 runner includes LLVM, and CI checks that
`clang` is available. The MSVC target also needs the Visual C++ linker and Windows import libraries.

For standalone release-compatible executables, link the Visual C++ runtime statically, as the release
workflows do:

```powershell
$env:RUSTFLAGS = '-C target-feature=+crt-static'
```

To run `horizon-prepare compile-exporters`, install JDK 25 and Python 3.10+; that command compiles
the Java exporters used by the game.

```powershell
rustup target add aarch64-pc-windows-msvc
cargo build --release --locked --target aarch64-pc-windows-msvc `
  -p horizon-client -p horizon-client-lib --bin horizon-client --bin horizon-ctl `
  -p horizon-prepare --bin horizon-prepare -p horizon-server --bin horizon-server
```

The launcher has a separate workspace:

```powershell
cd launcher
cargo test --release --locked --target aarch64-pc-windows-msvc --lib
cargo build --release --locked --target aarch64-pc-windows-msvc
```

## Releases and limitations

The game archive, `horizon-windows-arm64.zip`, is attached to the [Nightly release](https://github.com/horizon-rust/horizon/releases/tag/nightly)
and listed in its signed `SHA256SUMS`. The launcher archive, `horizon-launcher-windows-arm64.zip`,
and `horizon-launcher-setup-arm64.exe` are attached to the [launcher releases](https://github.com/horizon-rust/horizon/releases)
and listed in their signed `SHA256SUMS`.

An ARM64 launcher updates from ARM64 releases. An existing x64 launcher running under emulation
continues to update as x64, while selecting the native ARM64 game archive when a release has one.
Older releases without an ARM64 archive fall back to the x64 game archive. On Windows ARM64,
`Graphics API: Auto` tries DirectX 12 before Vulkan following a device-loss observed in testing;
Vulkan remains selectable in settings.

The bundled Windows native-mod compiler is x64-only. An x64 launcher can use it under emulation;
the native ARM64 launcher currently reports that no matching compiler is available. Native-mod
builds from the ARM64 launcher are not supported yet.
