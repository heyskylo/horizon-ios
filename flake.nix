{
  description = "Horizon: Minecraft Java Edition 26.3 rewritten in Rust";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAllSystems (pkgs:
        let
          version = "0.2.0";
          pname = "horizon-launcher";

          unwrapped = pkgs.rustPlatform.buildRustPackage {
            pname = "${pname}-unwrapped";
            inherit version;

            src = self;
            cargoRoot = "launcher";
            buildAndTestSubdir = "launcher";
            cargoLock = {
              lockFile = ./launcher/Cargo.lock;
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              libxkbcommon
              wayland
              libx11
              libxcb
              fontconfig
              vulkan-loader
              alsa-lib
            ];

            doCheck = false;
          };

          desktopItem = pkgs.makeDesktopItem {
            name = pname;
            desktopName = "Horizon";
            genericName = "Game launcher";
            comment = "Install, prepare and play Horizon, Minecraft: Java Edition rewritten in Rust";
            exec = pname;
            icon = "horizon";
            categories = [ "Game" ];
            startupWMClass = "horizon-launcher";
          };

          horizonLauncher = pkgs.buildFHSEnv {
            inherit pname version;

            targetPkgs = pkgs: with pkgs; [
              # Windowing, graphics, Vulkan, Wayland, X11
              vulkan-loader
              libglvnd
              wayland
              libxkbcommon
              libx11
              libxcb
              libxcursor
              libxext
              libxi
              libxrandr
              libxrender
              libxxf86vm

              # Audio, udev & dev packages for native builds
              udev
              udev.dev
              alsa-lib
              alsa-lib.dev

              # Desktop integration, fonts, keyring
              fontconfig
              freetype
              dbus
              xdg-utils
              procps

              # Mojang Java runtime dependencies
              zlib
              libxtst

              # Gamemode support
              gamemode

              # Build environment for native mods
              gcc
              glibc.dev
              pkg-config
            ];

            runScript = "${unwrapped}/bin/horizon-launcher";

            profile = ''
              export HORIZON_LAUNCHER_NO_SELF_UPDATE=1
              export HORIZON_LAUNCHER_DESKTOP_EXEC="''${HORIZON_LAUNCHER_DESKTOP_EXEC:-${pname}}"
              export PKG_CONFIG_PATH="/usr/lib/pkgconfig:/usr/share/pkgconfig:''${PKG_CONFIG_PATH:-}"
            '';

            extraInstallCommands = ''
              install -Dm444 ${desktopItem}/share/applications/${pname}.desktop -t $out/share/applications
              if [ -d ${./packaging/icons/hicolor} ]; then
                mkdir -p $out/share/icons/hicolor
                cp -r ${./packaging/icons/hicolor}/. $out/share/icons/hicolor/
              fi
            '';

            passthru = { inherit unwrapped; };

            meta = with pkgs.lib; {
              description = "Launcher for Horizon, Minecraft: Java Edition rewritten in Rust";
              homepage = "https://playhorizon.dev";
              license = licenses.mit;
              platforms = [ "x86_64-linux" "aarch64-linux" ];
              mainProgram = pname;
            };
          };
        in {
          horizon-launcher = horizonLauncher;
          default = horizonLauncher;
        }
      );

      overlays.default = final: prev: {
        horizon-launcher = self.packages.${final.stdenv.hostPlatform.system}.horizon-launcher;
      };

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            coreutils
            findutils
            gnused
            gnugrep
            pkg-config
            rustc
            cargo
            clang
            mold
            python3
          ];
          buildInputs = with pkgs; [
            libxkbcommon
            wayland
            vulkan-loader
            fontconfig
            freetype
            alsa-lib
            openssl
            udev
            libx11
            libxcursor
            libxrandr
            libxi
          ];
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
            libxkbcommon
            wayland
            vulkan-loader
            fontconfig
            freetype
            alsa-lib
            udev
            libx11
            libxcursor
            libxrandr
            libxi
          ]);
        };
      });
    };
}
