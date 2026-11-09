# Windows ARM64

> This is an upstream **Windows ARM64 reference**, not an iPhone/iPad installation guide. The Windows archives linked below are upstream desktop builds. For the mobile port, use the [mobile README](../README.md), [mobile releases](https://github.com/heyskylo/horizon-ios/releases) and [mobile issue tracker](https://github.com/heyskylo/horizon-ios/issues).

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

Launcher updates always follow the launcher's own process architecture. Game architecture is a
separate choice in Settings on native Windows ARM, detected from the machine even when an x64
launcher runs under emulation. **Automatic** preserves existing behavior: an x64 launcher uses x64,
and an ARM64 launcher prefers ARM64. Either launcher can choose x64 or ARM64 for the game. The
choice is saved and used for installs and updates; when a release has no ARM64 archive, ARM64
selection safely installs the signed x64 fallback. A changed choice installs the selected version
again when automatic updates are on; otherwise use **Install selected build** on the Versions page.
Launching is blocked with an explanation if a manual choice does not match the installed game.
Choosing a game architecture never changes or replaces the launcher binary.

Native-mod compilers remain independent of this setting. The managed compiler currently builds x64
only; an x64 launcher cannot build native mods for an ARM64 game, and the ARM64 launcher has no
matching compiler yet. The launcher refuses mismatched native-mod builds rather than producing a
binary for the wrong game.

On Windows ARM64, `Graphics API: Auto` tries DirectX 12 before Vulkan following a device-loss observed
in testing. Vulkan remains selectable in settings.

The `Windows ARM64 game and launcher` check in `ci.yml` runs only by hand
(`gh workflow run checks --ref <branch>`); the Nightly and launcher releases build these archives on
every run.
