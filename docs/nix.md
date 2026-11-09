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
