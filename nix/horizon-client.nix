# The game (horizon-client, with sound, controllers and Horizon's extras) and horizon-ctl, the tool
# that drives it (docs/agent-control.md). Start it with `horizon-client --resources DIR`, DIR being
# what horizon-prepare wrote.
{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  callPackage,
  alsa-lib,
  udev,
  vulkan-loader,
  wayland,
  libxkbcommon,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
  libxcb,
  version,
}:
let
  # wgpu and winit open these with dlopen, so the linker never records them.
  dlopened = [
    vulkan-loader
    wayland
    libxkbcommon
    libx11
    libxcursor
    libxi
    libxrandr
    libxcb
  ];
in
rustPlatform.buildRustPackage {
  pname = "horizon-client";
  inherit version;
  src = callPackage ./source.nix { };
  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package=horizon-client"
    "--bin=horizon-client"
    "--package=horizon-client-lib"
    "--bin=horizon-ctl"
  ];

  nativeBuildInputs = [ pkg-config ];
  buildInputs = lib.optionals stdenv.hostPlatform.isLinux [
    alsa-lib
    udev
  ];

  doCheck = false;

  postFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
    patchelf --add-rpath ${lib.makeLibraryPath dlopened} $out/bin/horizon-client
  '';

  meta = {
    description = "Horizon, Minecraft: Java Edition 26.3 rewritten in Rust, and horizon-ctl";
    homepage = "https://playhorizon.dev";
    license = lib.licenses.mit;
    mainProgram = "horizon-client";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
