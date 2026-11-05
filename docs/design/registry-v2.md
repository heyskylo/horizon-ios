# Mod registry v2

Every kind can be submitted, every project has a real page (description, categories, tags, icon, screenshots), and
installed projects update themselves. Asked by Ludovic on 2026-11-05. This note fixes the schema, the kinds, where
the launcher installs each one, the category lists and the rollout. The format itself stays in
[docs/api/mod-index.md](../api/mod-index.md), updated by the PRs that change it.

## Kinds

| Kind | File | What it is | The registry's check | The launcher installs it in |
|---|---|---|---|---|
| `mod` | `.hmod` | Content mod (and sandboxed code) | manifest + content (today) | `mods/<id>.hmod` |
| `native-mod` | `.hnmod` | Native source mod | bundle + pins (today) | the native build; rebuilt after an update |
| `modpack` | `.hpack` | Registry mods played together | manifest + its mods in the index (today) | its own profile |
| `resourcepack` | `.zip` | `pack.mcmeta` + `assets/` | plain paths + `pack.mcmeta` (today) | `resourcepacks/<file>` |
| `shader` | `.hshader` | Horizon's WGSL shader pack | `horizon_shader_api::check` (today) | `shaderpacks/<file>` |
| `shaderpack` | `.zip` | **new**: a `shaders/`-folder pack (the OptiFine/Iris format Horizon loads with `horizon-shaders`) | plain paths, a `shaders/` folder at the root | `shaderpacks/<file>` |
| `datapack` | `.zip` | **new**: `pack.mcmeta` + `data/` | plain paths, `pack.mcmeta` with a `pack` object, a `data/` folder; its pack format | a world the player picks: `<world>/datapacks/<file>` |

- Why a separate `shaderpack` kind rather than `.zip` files under `shader`: launchers already released check every
  version's extension against its kind and refuse the whole index on a mismatch. A new kind name is read as
  `Kind::Other` by them: not offered, nothing refused. The same holds for `datapack`.
- A `shaderpack` is accepted only when the submitter is the pack's author and its licence allows redistribution. The
  form asks for both as a ticked statement; the reviewer checks the licence and the author against the source. Ports
  of someone else's pack are refused (Mojang rules and [MOD-POLICY.md] in horizon-mods apply as for every kind).
- Data packs: the game discovers `<world>/datapacks/*.zip` and turns them on at world open and on `/reload`
  (`datapack::discover`). The launcher already installs Modrinth data packs into a chosen world
  (`browser::installed::install_datapack`); registry data packs go through the same picker.

## Page fields

New fields, in the entry (`mods/<id>.toml`) and in the index's projects, under the same names:

```toml
categories = ["magic", "mobs"]              # 1 to 3, from the kind's list below
tags = ["bosses", "dungeons"]               # optional, at most 10: [a-z0-9][a-z0-9-]{0,23}, no repeats
screenshots = ["screenshots/my_mod/1.png"]  # optional, at most 8: screenshots/<id>/<name>.png|.jpg in the registry, or https://
```

- **description**: Markdown, required, at least 50 characters once trimmed, at most 10,000.
- **icon**: required. A PNG, square, 64 to 1024 pixels, at most 256 KiB, in `icons/<id>.png` (or `https://`).
- **screenshots**: PNG or JPEG, 320×180 to 3840×2160 pixels, at most 1 MiB each, stored in the registry
  repository like icons and served from its Pages site; the index lists their `https://` addresses in order.
- **tags**: free-form, lowercase. Tags naming gambling (`casino`, `gambling`, `slots`, `lottery`, `betting`...) are
  refused, as no category names it (Mojang's Usage Guidelines).
- Old readers are unaffected: the index has no `deny_unknown_fields`, and `SCHEMA` stays 1. The entry has
  `deny_unknown_fields`, hence the rollout below.

## Categories

One fixed list per kind, Modrinth's, minus `cursed`:

- `mod`, `native-mod`, `datapack`: adventure, decoration, economy, equipment, food, game-mechanics, library, magic,
  management, minigame, mobs, optimization, social, storage, technology, transportation, utility, worldgen.
- `modpack`: adventure, challenging, combat, kitchen-sink, lightweight, magic, multiplayer, optimization, quests,
  technology.
- `resourcepack`: 8x, 16x, 32x, 48x, 64x, 128x, 256x, 512x (resolution); audio, blocks, combat, core-shaders,
  decoration, entities, environment, equipment, fonts, gui, items, locale, models, realistic, simplistic, themed,
  tweaks, utility, vanilla-like.
- `shader`, `shaderpack`: cartoon, fantasy, realistic, semi-realistic, vanilla-like (style); atmosphere, bloom,
  colored-lighting, foliage, path-tracing, pbr, reflections, shadows (features); potato, low, medium, high,
  screenshot (performance).

They live in `horizon_mods::index::categories(kind)`; the site and the launcher read them from the index's projects
and show the kind's list as filters.

## Submitting and editing

- The form (playhorizon.dev/mods/submit) offers every kind, requires description, 1 to 3 categories and an icon (an
  upload: the service writes `icons/<id>.png` on the submission branch), and takes up to 8 screenshot uploads
  (`screenshots/<id>/<n>.<ext>`). The service checks sizes, types and dimensions before anything reaches GitHub; the
  registry's CI checks them again.
- **Edit project**: an owner (by `owner_ids`, never by login alone) changes the page without a new version:
  `POST /api/mods/edit`, page fields only (name, summary, description, authors, licence, source, categories, tags,
  icon, screenshots). The service opens a pull request `edit/<id>-<time>` labelled `owner-edit`; `registry review`
  refuses an edit PR that changes versions, owners, keys or the kind.

## Updates

- The launcher updates installed registry projects by default (setting "Update registry mods automatically", on).
  It checks when it starts and before play, and installs each project's `Index::installable` version.
- It never updates: a mod inside a modpack profile (the pack pins it by SHA-256: the pack updates as a whole, when
  the player asks); a native mod when the native build is pinned to a Nightly; a data pack (it lives in a world: the
  player updates it); anything across Minecraft versions.
- An update never breaks a world: a content mod's new version that drops a block, item or entity id the installed
  one has is held for a manual update with a warning; the replaced file is kept beside it until the next start so a
  failed install rolls back; native mods are rebuilt after an update before play.

## Rollout

The `cargo horizon-mod` pinned by horizon-mods (`HORIZON_REV`) refuses unknown entry fields (#9688), so:

1. horizon: the new fields as optional (serde defaults), the two kinds, the checks of what is present, the category
   lists and docs/api/mod-index.md. Nothing is required yet.
2. horizon-mods: `gh variable set HORIZON_REV` to that merge.
3. In parallel: the service (uploads, Edit project), the site (form, pages, filters, screenshots), the launcher
   (kinds, filters, screenshots, auto-update), and the backfill of every existing entry (description, categories,
   tags, icon) in batches of horizon-mods PRs. The service deploys from horizon main on merge, so its PR merges after
   step 2.
4. horizon: description, categories and icon required in `check_fields`; bump `HORIZON_REV` again; the form
   requires them. One issue in horizon-mods tells each owner what was filled in for them and how to change it.

Progress: `agent-state/registry-v2.md` in the orchestration repo.

[MOD-POLICY.md]: https://github.com/horizon-rust/horizon-mods/blob/main/MOD-POLICY.md
