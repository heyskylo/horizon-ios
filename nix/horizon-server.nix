# horizon-server: server.toml, the CLI and the supervisor. It finds the game beside itself, so the
# engine is linked into the same bin/ (crates/horizon-server/src/engine.rs `program_in`).
{
  lib,
  rustPlatform,
  pkg-config,
  callPackage,
  horizon-engine,
  version,
}:
rustPlatform.buildRustPackage {
  pname = "horizon-server";
  inherit version;
  src = callPackage ./source.nix { };
  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package=horizon-server"
    "--bin=horizon-server"
  ];

  nativeBuildInputs = [ pkg-config ];

  doCheck = false;

  postInstall = ''
    ln -s ${lib.getExe horizon-engine} $out/bin/horizon-engine
  '';

  passthru.engine = horizon-engine;

  meta = {
    description = "Host a Minecraft: Java Edition 26.3 server with Horizon, the game rewritten in Rust";
    longDescription = ''
      Holds none of Mojang's files: the server reads the game files you prepare from your own copy of
      Minecraft 26.3 with horizon-prepare (docs/hosting.md).
    '';
    homepage = "https://playhorizon.dev";
    license = lib.licenses.mit;
    mainProgram = "horizon-server";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
