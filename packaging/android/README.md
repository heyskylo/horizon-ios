# Horizon on Android

The game itself, as a NativeActivity: no Java, no Gradle.

- [`crates/horizon-android`](../../crates/horizon-android) builds the shared gameplay library
  (`horizon-client/src/lib.rs`, `entry.rs`, `game.rs`, `game/`) as `libhorizon_android.so`.
  It depends on `horizon-client-lib` for reusable client systems, just as the desktop/iOS
  gameplay package does. Its own workspace avoids building Android in host-only checks.
- `crates/horizon-client/src/android.rs` is `android_main`: the working directory and `HOME` both become the app's
  external files folder (`Android/data/dev.horizon.game/files`: saves, `resources/`, imported archives, and
  `.local/share/horizon` for logs and settings), then `main` runs as anywhere else.
- `AndroidManifest.xml`: `dev.horizon.game`, Android 8 (API 26) or later, landscape, Vulkan.

`.github/workflows/android.yml` builds, packs (aapt2), aligns, signs and uploads `Horizon.apk`. By hand, with an NDK:

```sh
bin=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
export CC_aarch64_linux_android=$bin/aarch64-linux-android26-clang AR_aarch64_linux_android=$bin/llvm-ar
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$CC_aarch64_linux_android
cargo build --release --manifest-path crates/horizon-android/Cargo.toml --target aarch64-linux-android
```

## Game files

The .apk carries no Mojang assets. As on iOS, a game with none waits on the import screen and asks for the ZIP
from `horizon-client --pack-resources` over Wi-Fi (see [../ios/README.md](../ios/README.md)), or take the cable:
`adb push Horizon-26.3.zip /sdcard/Android/data/dev.horizon.game/files/` and relaunch.

## Reading what happened

Horizon's own log lines and panics go to logcat: `adb logcat -s RustStdoutStderr`. The whole log is
`adb pull /sdcard/Android/data/dev.horizon.game/files/.local/share/horizon/logs/latest.log`.

## Not done yet

- Leaving the app (home button, screen off) takes the window away; the game does not rebuild its surface, so
  coming back may show nothing or crash. Relaunch.
- No soft keyboard for text fields, no system clipboard (the game's own copy buffer only), no controllers
  (gilrs has no Android backend), no mods.
- Touch controls are the iOS ones (`game/touch.rs`), on from the start.
