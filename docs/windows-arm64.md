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

Each launcher keeps its own architecture: an x64 launcher on an ARM64 PC (under emulation) updates
itself from the x64 archive and installs the x64 game, so an update never moves a working setup to
another build. `horizon-launcher-setup-arm64.exe` installs the ARM64 launcher, which installs
`horizon-windows-arm64.zip` and falls back to `horizon-windows-x64.zip` (under emulation) for a release
without an ARM64 build. Native-mod builds need the x64 launcher for now: the bundled Windows compiler is
x64-only, and the ARM64 launcher reports that no matching compiler is available instead of building x64
binaries.

On Windows ARM64, `Graphics API: Auto` tries DirectX 12 before Vulkan following a device-loss observed
in testing. Vulkan remains selectable in settings.

The `Windows ARM64 game and launcher` check in `ci.yml` runs only by hand
(`gh workflow run checks --ref <branch>`); the Nightly and launcher releases build these archives on
every run.
