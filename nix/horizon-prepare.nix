# horizon-prepare: builds the game files Horizon reads from the player's own Minecraft 26.3 and the
# server jar they downloaded themselves. Nothing of Mojang's is fetched or packaged here.
#
# It works in a "root": research-lock.json (the pinned jars' hashes), the exporters' Java sources it
# compiles against those jars, and a writable .local/ for its scratch files. The store is read-only, so
# the package keeps that root in share/horizon/prepare and bin/horizon-prepare (horizon-prepare.sh)
# copies it somewhere writable unless --root is given. The exporters need a JDK of the pinned Java
# version (25): the default JAVA_HOME.
{
  lib,
  stdenv,
  rustPlatform,
  callPackage,
  coreutils,
  jdk25_headless,
  version,
}:
rustPlatform.buildRustPackage {
  pname = "horizon-prepare";
  inherit version;
  src = callPackage ./source.nix { };
  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = [
    "--package=horizon-prepare"
    "--bin=horizon-prepare"
  ];

  doCheck = false;

  postInstall = ''
    root=$out/share/horizon/prepare
    install -Dm444 research-lock.json -t $root
    install -Dm444 crates/horizon-prepare/src/exporters.rs -t $root/crates/horizon-prepare/src
    mkdir -p $root/mods/horizon/src/test
    cp -r mods/horizon/src/test/java $root/mods/horizon/src/test/

    mkdir -p $out/libexec
    mv $out/bin/horizon-prepare $out/libexec/horizon-prepare
    substitute ${./horizon-prepare.sh} $out/bin/horizon-prepare \
      --subst-var out \
      --subst-var-by shell ${stdenv.shell} \
      --subst-var-by coreutils ${coreutils} \
      --subst-var-by java ${jdk25_headless}
    chmod +x $out/bin/horizon-prepare
  '';

  passthru.java = jdk25_headless;

  meta = {
    description = "Prepare Horizon's game files from your own copy of Minecraft: Java Edition 26.3";
    homepage = "https://playhorizon.dev";
    license = lib.licenses.mit;
    mainProgram = "horizon-prepare";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
