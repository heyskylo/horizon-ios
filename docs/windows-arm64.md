# Windows ARM64

Horizon builds the game and launcher as native Windows ARM64 executables for
`aarch64-pc-windows-msvc`. GitHub Actions builds and checks them on a Windows ARM64 runner.

## Build from source

Use Windows 11 on ARM, or cross-compile from Windows x64. Install stable Rust, the
`aarch64-pc-windows-msvc` Rust target, Visual Studio C++ build tools for ARM64, and a Windows SDK
with ARM64 libraries. Install LLVM/Clang and put `clang.exe` on `PATH`; the `ring` dependency uses
Clang to compile its ARM64 code. The MSVC target also needs the Visual C++ linker and Windows import
libraries.

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

The launcher uses `IsWow64Process2`'s native-machine value, so an x64 launcher running under emulation
on an ARM64 PC selects ARM64 game and launcher downloads. Native-mod builds on Windows ARM64 are not
supported yet: the launcher's bundled Windows compiler is x64-only, and the ARM64 launcher reports
that no matching compiler is available instead of building x64 binaries.
