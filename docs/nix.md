# Nix and NixOS

The repository is a flake (`flake.nix`, and the game's packages in `nix/`). It builds Horizon from source with nixpkgs'
`rustPlatform` and holds none of Mojang's files: the game files still come from `horizon-prepare` and your own
copy of Minecraft 26.3, as everywhere else ([hosting.md](hosting.md), step 2).

| Package | What | Systems |
|---|---|---|
| `horizon-server` | `horizon-server`, with `horizon-engine` (the game without sound or controllers) beside it | Linux, macOS (Apple Silicon) |
| `horizon-engine` | The server build of the game alone | Linux, macOS |
| `horizon-prepare` | `horizon-prepare`, with a JDK 25 for the exporters it compiles | Linux, macOS |
| `horizon-client` | The game and `horizon-ctl` | Linux, macOS |
| `horizon-launcher` | The launcher in an FHS environment (it builds native mods with Cargo) | Linux |

```sh
nix run github:horizon-rust/horizon#horizon-server -- --version
nix profile install github:horizon-rust/horizon#horizon-server
```

`overlays.default` adds them to `pkgs`; on Linux, `nix develop` gives the build environment of the workspace.

## Preparing the game files

`horizon-prepare` works in a writable folder (its pinned hashes, the exporters' sources, scratch files). The
Nix wrapper copies one to `$HORIZON_PREPARE_ROOT`, by default `~/.local/share/horizon-nix/prepare`, unless you
pass `--root`. Write the files where you want them with `--output`:

```sh
horizon-prepare --prepare --server-jar ~/Downloads/server.jar --output ~/horizon/26.3
```

## A server on NixOS: `services.horizon-server`

```nix
{
  inputs.horizon.url = "github:horizon-rust/horizon";

  outputs = { nixpkgs, horizon, ... }: {
    nixosConfigurations.box = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        horizon.nixosModules.horizon-server
        {
          services.horizon-server = {
            enable = true;
            eula = true;                      # you agree to https://aka.ms/MinecraftEULA
            openFirewall = true;              # port 25565 by default
            resources = "/srv/horizon/26.3";  # the folder horizon-prepare wrote, copied here
            init = { world = "survival"; players = [ "Alex" ]; op = "Alex"; };
            settings = { "world.difficulty" = "hard"; "server.max_players" = 10; };
          };
        }
      ];
    };
  };
}
```

- The server folder is `/var/lib/horizon-server` (`dataDir`), owned by the `horizon-server` user. The first
  start writes `server.toml` from `init`, as `horizon-server init` would. Every start then sets `port`,
  `resources` and `settings` with `horizon-server config set`, which checks them and keeps the rest of the
  file: the settings you don't name stay yours to edit.
- Give paths as strings, not Nix paths: a Nix path would copy the game files into the world-readable store.
- The service runs `horizon-server run` and stops it with SIGTERM, which saves the world first. Manage it as
  that user: `sudo -u horizon-server horizon-server --dir /var/lib/horizon-server status` (or `cmd`, `logs`,
  `backup`...).
- Nix decides the release: the update check is off (`HORIZON_NO_UPDATE_CHECK=1`) and `horizon-server update`
  refuses, since this isn't an install.sh install.
- The unit is hardened (read-only system, no home folders, no capabilities, its own `/tmp` and devices). A
  `resources` folder under `/home` isn't visible to it: keep it elsewhere, such as `/srv`.

### Preparing on the server

Instead of `resources`, `prepare` runs `horizon-prepare` before the server starts, from files you put on the
machine yourself: the 26.3 `server.jar` and a copy of the official launcher's game folder with 26.3 played
once (`versions/26.3`, `libraries`, `assets`), readable by the `horizon-server` user.

```nix
services.horizon-server.prepare = {
  enable = true;
  serverJar = "/srv/horizon/server.jar";
  minecraftDir = "/srv/horizon/minecraft";
};
```

It writes `/var/lib/horizon-server/game-files`, and runs again only when the `horizon-prepare` package or
those paths change. The `horizon-server-prepare` unit has no network at all: nothing is ever downloaded.
