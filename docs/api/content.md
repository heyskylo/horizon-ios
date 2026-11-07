# Content API

Where another launcher puts content in a Horizon game folder, how it turns content on and off, and what it checks
first. It is the install side of the [Launch API](launch.md). The registry's file it installs from is
[mod-index.md](mod-index.md). `horizon-launcher` is the reference implementation. Proposed in
horizon-rust/horizon#10562.

## Overview

| | |
| --- | --- |
| Level | experimental ([stability.md](stability.md)), with the Launch API |
| Covers | the folders and files of each kind under `--game-dir`, turning content on and off, when a change applies, what to check before installing, what a build loads (`content` in `horizon-client --launch-api`) |
| Not yet | an API for a running game (install while it plays); native mods for a launcher that doesn't build Horizon |

## What a build loads

Ask before offering anything: `horizon-client --launch-api` has `content` ([launch-reference.md](launch-reference.md)).
Offer a version only when this build loads it:

| Kind | Loads when | From `content` |
| --- | --- | --- |
| Content mod (`.hmod`) | its manifest `api` is listed | `mod.api` |
| Native mod (`.hnmod`) | the game is rebuilt with it, and its `hooks` is at most this level | `native_mod.hooks`, `native_mod.rebuild` |
| Horizon shader pack (`.hshader`) | its manifest's `schema` and `api` are listed | `shader.schema`, `shader.api` |
| Resource pack | always; outside `format` it is marked too old or too new, as vanilla marks it | `resourcepack.format` |
| Data pack | the same | `datapack.format` |
| Modpack (`.hpack`) | its `format` is this one | `modpack.format` |
| The index | its `schema` is at most this one | `index.schema` |

## Where content goes

Paths are under the game folder (`--game-dir`). Change these files while the game is stopped: it reads them when it
starts and writes them when the player changes a setting.

| Kind | File | Folder | On and off |
| --- | --- | --- | --- |
| Content mod | `<id>.hmod`, or a folder with `horizon-mod.toml` | `horizon-mods/` | `horizon-mods.txt`: a `mod.<id>:true` line turns it on. A mod without one is off |
| Native mod | `<id>.hnmod` | `horizon-native-mods/` | runs only in a game built with it: `mod.<id>:true` chooses it for the next build, `mod.<id>:false` turns a built one off |
| Shader pack | a `.hshader` or a folder with `horizon-shaders.toml`; a `shaders/`-format `.zip` or folder | `shaderpacks/` | `horizon-graphics.txt`: `pickedShaderPack:<file or folder name>`, empty for none. A pack's options are in `shaderpacks/<name>.txt` |
| Resource pack | a `.zip` or a folder with `pack.mcmeta` | `resourcepacks/` | `options.txt`: `resourcePacks:["vanilla","file/<name>"]`, lowest priority first, as vanilla's |
| Data pack | a `.zip` or a folder with `pack.mcmeta` and `data/` | `saves/<world>/datapacks/` | `saves/<world>/datapacks.json`: `{"enabled": [...], "disabled": [...]}`, each `file/<name>` (or `vanilla`). A pack in neither list is on |

- **When a change applies.** Every change above applies at the next start. While the game plays, its own
  screens switch packs, shader packs and assets-only content mods live; a content mod that adds blocks, items,
  entity types or world generation loads only at start ([mods.md](../mods.md)).
- **Native mods** need Horizon rebuilt with them (horizon-mixins, [native-mods.md](../native-mods.md)). A launcher
  that doesn't build Horizon doesn't offer them.
- **A modpack** isn't copied into the game folder. A launcher reads its `horizon-modpack.toml`, installs each mod
  it lists from the index with the checks below, writes `mod.<id>:true` for those with `on` (the default), and
  copies `overrides/` into the game folder. `overrides/` may hold only what [mod-index.md](mod-index.md#hpack)
  lists, within its size limits.

## Before installing a file

The checks `horizon_mods::index::check_file` makes, which [mod-index.md](mod-index.md#before-a-file-is-installed)
specifies. A launcher that installs from the index makes the same ones:

1. **Where it comes from.** An `https://` address of the index's shape: a registry release
   (`https://github.com/horizon-rust/horizon-mods/releases/download/...`) or a release asset of the project's own
   GitHub repository. Anything else is refused.
2. **Size and SHA-256.** The size is at most 64 MiB and equals the entry's, and the SHA-256 is the entry's.
   Check both before the file reaches the game folder.
3. **Signature.** When the project has a minisign `key`, the version's `signature` verifies with it, and a
   version signed with another key is refused. With no key, say the project is unsigned.
4. **Revoked.** A file in `revoked[]` (by `sha256`, or every version of an `id` without one) isn't installed. An
   installed copy is turned off (`mod.<id>:false`, or out of `resourcePacks`), and the player is told the reason.
5. **What the file says.** Its manifest matches the entry (id, version, API), as mod-index.md lists for each kind.

## Stability

Experimental: the folders, the keys and `content` may still change before preview. From preview on, they change only
as [stability.md](stability.md) says.

## Changelog

- 0.1.0 (with Launch API 0.1.0): this page, and `content` in `horizon-client --launch-api`.
