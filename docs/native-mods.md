# Native mods

`native_chat_suggestion_applied(original, start, end, suggestion, input)` reports a suggestion accepted in the open chat by Tab or a mouse click, after the text and caret have changed. `original` is the suggestion list's input before any completion; `start` and `end` are the replaced range in that string, in Java UTF-16 units (end exclusive). They keep referring to `original` when Tab cycles through the list. `suggestion` is the selected replacement text, and `input` is the actual resulting field value, including any length-limit truncation. This is a read-only notification: previews, list navigation, server replies, ordinary typing and command-block editors do not fire it. Trait mods implement `Hooks::chat_suggestion_applied`; mixin bundles use `chat_suggestion_applied`, hook version 1. Bundles need "hooks": @API:chat_suggestion_applied@ (assigned at merge).

API 362 adds `native_compute_fov_modifier(player, base, modifier)` on the client each tick for the local player's FOV scale, after vanilla effects and before zoom and camera easing. `player` contains the calculation inputs and `base` is the original scale. Non-finite modifiers or values outside 0.01..=10 fall back to `base`. It does not affect a spectator camera on a mob. Bundles need "hooks": 362.

The client hook `native_client_packet(packet)` hears each play packet the server sends, decoded (`ClientboundPlayPacket`, `horizon_protocol`'s `play::Clientbound`), just before the game handles it, custom payloads included and in the order they came. It only reads: the game handles the packet as it would without the mod. Packets the network thread handles itself (keep-alives, bundle delimiters, resource packs, cookies, server links, transfers, disconnects, the configuration switch, `minecraft:brand`, far terrain tiles) and the configuration phase are not heard. Bundles need "hooks": 408.

`native_registry_name(registry, id)` (`host.registry_name`) is the name of a protocol id in a registry: `minecraft:item`, `minecraft:data_component_type`, `minecraft:entity_type`, `minecraft:sound_event`, `minecraft:particle_type` and the like. It names what `native_client_packet`'s packets carry as numbers (a `Stack`'s item and components, `AddEntity`'s type, `Sound`, `LevelParticles`). On a client the ids are those of the server it is connected to, so a registry the server synced (enchantments, painting variants) and a mod's own data components, when the server carries them, name too; on a server they are the ids it sends. It answers `None` for an unknown registry, an id out of range or negative, and while no server is connected. Bundles need `"hooks": @API:registry_name@`.

The client hook `native_render_name_tag(entity_id, kind, vanilla_visible, visible, text)` runs at player and mob, armor stand, and item frame name decisions before shared geometry. Set `visible` to override vanilla and `text` to `Some` literal replacement text (NeoForge `RenderNameTagEvent`). Distance and entity availability still apply; the local player's tag is considered only when its own camera and settings gates allow it. Bundles need "hooks": 372.

Native mods are **mixin bundles** that change the game's own Rust source before it is compiled, with
[horizon-mixins](https://github.com/horizon-rust/horizon-mixins) (MIT, by levzzz5154, made for Horizon). A bundle
is a `horizon-mixin.toml` and a few `.rs` helpers that hook into named functions of the game (HEAD, BEFORE_CALL,
RETURN, MODIFY_ARG, OVERWRITE...; the tool's README lists them). A port such as Veinminer is three HEAD hooks and
one module. Start a mod from [horizon-native-mod-template](https://github.com/horizon-rust/horizon-native-mod-template).

Native mods are native code with the player's rights, not sandboxed. They are always on in the launcher: there is no
switch. The first install shows a trust notice, and every build waits behind a red warning; the registry lists one only
after a review (its later releases are checked by machine); and they play in a game of their own, built for the
instance they are installed in.

## Compilation cache

Native builds share `native/target/fast` (Performance and Dev) and `native/target/release`.
After a build finishes, the launcher clears these compiler outputs if their combined size exceeds
30 GiB. The installed game or exported server stays usable; a later changed build recompiles.
`horizon-launcher settings native-cache-gib N` changes the limit (`default` restores 30; `0`
disables automatic cleanup). This is an after-build limit, not a cap on peak compilation space.

`horizon-launcher native-cache` reports its path, size in bytes and limit as JSON.
`horizon-launcher native-cache --clear` frees the compilation cache immediately when builds are idle.
Both use the same process lock as native builds; cleanup also refuses busy Cargo profile locks.
It preserves lock files, refuses symbolic links and Windows junctions, and never targets installed
versions, exported servers, instances, saves, mods, Rust toolchains or downloaded crates.


## Conditional item model properties

A client mod supplies its own namespaced `minecraft:condition` property by implementing
`Hooks::item_model_property`. This hook is the registration: match your property name, answer
through `value`, and leave other names untouched. The resolver calls it when choosing a branch,
not when loading resources. It defaults to false without a provider and skips dispatch entirely
when no trait mods are loaded. Vanilla properties do not enter the hook.

```rust,ignore
fn item_model_property(
    &self,
    _host: &dyn Host,
    event: &mut ItemModelPropertyEvent<'_>,
) {
    if event.property == "example:charged" {
        event.value = event.stack.patch.get("example:charged").and_then(Option::as_ref)
            .and_then(serde_json::Value::as_bool) == Some(true);
    }
}
```

`Host`, `ItemModelPropertyEvent`, and `StackView` are exported by `horizon_client::mod_api`. The stack is borrowed,
including for nested bundle items: `item`, `count`, explicit component `patch` (not merged
defaults), and the runtime bundle selection. Listeners run in mod order; later answers win.
Use `"property": "example:charged"` with both `on_true` and `on_false` in the item's
`minecraft:condition` node. Custom special model renderers are not supplied by this hook.
Bundles need `"hooks": 385` once this pending hook is stamped.

## Item frame rendering

`native_render_item_in_frame(entity_id, pos, stack, canceled)` runs before a visible item frame draws its item or map, including glow and invisible frames. Set `canceled` to hide the item without hiding the frame. Bundles need "hooks": 361.

## Villager farming

Farmers recognize mod blocks made from farmland and `CropBlock` templates, including declarations from
`native_blocks`. A crop block's generated item is its seed: add that item to
`minecraft:villager_plantable_seeds` to let farmers plant its default block state from their inventory.
Untagged items and tagged items without a block are skipped. Crop maturity follows the template's class:
beetroots mature at age 3, ordinary crops at 7; torchflower crops never reach their maximum age 2 while
still crops. Pitcher crops are not `CropBlock`s and are not harvested by this behavior. Farming still
requires a farmer at work and `mob_griefing` enabled.

## Named mob damage

Native code may use `horizon_client::mobs::DamageKind::Named(NamedDamage::new(id, tags))`
for its own damage types. The static namespaced `id` is retained in the ordinary
mob damage record, loot damage source and death-message path. Add that type's JSON
and damage-type tags to the mod's data pack too; player damage reads that registry.
Build the descriptor flags from the loaded tags again after a data-pack reload.

`NamedDamage` exposes common mob combat classifications: `IS_FIRE`,
`BYPASSES_ARMOR`, `NO_KNOCKBACK`, `BYPASSES_INVULNERABILITY`, `BYPASSES_EFFECTS`,
`BYPASSES_RESISTANCE`, `PANIC_CAUSES`, `IS_PROJECTILE`, `IS_EXPLOSION` and `IS_FALL`.
Unset flags are false. Ordinary fire immunity, Fire Resistance, armor, enchantments,
Resistance and knockback still run. The flags do not implement arbitrary additional
damage tags or change vanilla `DamageKind` variants. This compiled API needs a
matching source pin; it is not a serialized protocol enum.

## Compiled particle controllers

`ParticleEngine::controlled(name, position, translucent, controller)` creates a
named sprite particle after the normal camera-distance and particle-setting
checks. Configure its public fields and optional `set_sprite_frame`, then `push`
it into the ordinary engine queue. Missing sprite sets return `None`.

Implement `ParticleController` with owned `Clone + Debug + Send + Sync` state.
Its tick receives the current `ParticleLevel`, and returns a `ParticleUpdate`
with an optional sprite frame and extra normal particle requests. Use
`Particle::advance_age` for ordinary expiry and previous-position interpolation,
or maintain those fields in a source-specific override; `remove` ends it early.
No vanilla movement is applied to a controlled particle. Its own `random` is
separate from the engine/level generators. Choose blended rendering explicitly
for alpha-fading particles; `set_full_bright` controls light without bypassing
normal atlas, geometry or engine population limits. This is an opt-in compiled
API, not executable particle data in a resource pack.
Resource reload discards controlled particles and their pending instances, so
their numeric sprite-set handles cannot refer to a different reloaded set.

## Pinned to a Nightly

A native mod builds on one exact Horizon commit: `[compatibility] horizon_revision` in its `horizon-mixin.toml`,
and `horizon` in the `horizon-native.json` of its registry file (`.hnmod`, `horizon_mods::index::native`). Mods
pin a **Nightly's commit** (or a release's). Every Nightly ships its source and keeps its commit for good as the
tag `nightly-source/<date>-<short>` ([release-flow.md](release-flow.md#nightly-channel)), so a pinned mod still
builds after its Nightly is replaced. When the mods of one build pin different commits, the launcher says which
mod pins which and the commit the build would use, the newest of them, and builds only after the player chooses
**Build anyway** (`native-build --allow-version-mismatch`): such mods often still apply and compile. A failed build
then says the mods were made for other versions. A mod that doesn't fit the build's Horizon doesn't stop the others: when
horizon-mixins refuses it (a function it hooks, the file it adds a module to or a named hook point this Horizon
doesn't have: `E_TARGET_NOT_FOUND`, `E_MODULE`, `E_HOOK_CATALOG`; a module named like one of Horizon's or another
mod's: `E_MODULE_CONFLICT`), or the compiler's errors are in its helpers, the build runs again without it and without
the mods that need it ([dependencies](#dependencies-between-trait-mods)), at most three more compiles. The build's steps
and the Mods page say which mods were left out and why ("X was left out: made for another Horizon version", "…:
clashes with Y", "…: it needs Y, which was left out"); `native-mods.json` keeps them (`left_out`), so the build is up
to date with the same mods turned on, and a new version of a left-out mod asks for a build again. A server engine
(`--server-output`) leaves nothing out: it fails instead. Their horizon-mixins pins may differ too: the build uses the
newest of them and the launcher's own, never one older than a mod was made with. A bundle with no pin builds on
the commit of the installed version the build starts from. A pin chooses the commit even when the player picked another
version: the Mods page then says which mods pinned the commit the build compiled, and **Rebuild on Horizon**
(`native-build --ignore-pins`) builds every mod on the picked version's own commit, where a pinned mod may not
apply or compile. Its later rebuilds keep doing so.

Before a native mod is installed, the registry browser (and `horizon-launcher mods search`) says how it fits the
Horizon the selected instance plays: the commit its native build compiled, else the commit of its release or
Nightly. A mod naming hook points builds there when that commit's `NATIVE_HOOKS_API` is the one it needs or later,
else it asks for a newer Horizon; a pinned mod fits its own commit, else the line says the build moves the game to
that commit, or, when mods turned on pin another, that the build will ask for **Build anyway**. The registry's index
lists each version's `hooks` for this ([api/mod-index.md](api/mod-index.md)).

### Mods on the hook points

A mod that hooks only Horizon's native hook points (`crates/horizon-client/src/game/native_hooks.rs`, with a few
free functions in other crates: [render hook points](#render-hook-points), [items' default
components](#items-default-components), `horizon_world::loot::native_loot_table_load`, [menu slot
rules](#menu-slot-rules); empty functions the game
calls at fixed places, plus helpers, kept stable across releases) can name the version it
needs instead of pinning: `"hooks": 1` in its `.hnmod`'s `horizon-native.json`, the `NATIVE_HOOKS_API` of that
file when it was made. Hook points are only ever added: each addition raises `NATIVE_HOOKS_API`, and a mod builds
on any Horizon whose version is the one it needs or a later one. Its `horizon` is then only the commit it was tested on. The launcher builds it on the commit the other mods
pin, else on the installed version's commit (the game the player plays), after checking that commit's
`NATIVE_HOOKS_API` (read from GitHub) is at least the mod's; when it isn't and nothing pins, it builds on the commit the
mod was tested on. In its own copy of the mod it sets `horizon_revision` to the commit it builds on. Horizon's
own native mods (Zoom, FPS counter, Keystrokes, Minimap; horizon-rust/horizon-mods, `native/<name>/`) are made this way, so any of them build together
with no re-release for each Nightly.

Hook and helper authors use [API fragments](../tools/native-hook-fragments/README.md) to request a number.

The pending `hud_mode` helper fragment adds `Host::hud_mode()` and
`Host::set_hud_mode(HudMode)`. `HudMode::AllVisible` shows the HUD and permits
first-person hands; `NoHud` hides the HUD while permitting hands;
`AllHidden` hides both. Spectator, sleeping, camera and death restrictions,
and `render_hand` cancellation, still apply. `gui_render_post`'s `gui_hidden`
and `client_state().gui_hidden` report the effective HUD visibility.

A mod can keep its own three-state cycle and call `set_hud_mode` from the
existing post-handling `input_key` callback after an F1 press, or from a client
tick callback. No cancellable input hook is required. Without a mod, F1 keeps
its normal all-visible/all-hidden toggle; each F1 toggle clears the independent
hand override. The mode is window UI state, never a saved player setting.
Writes on a server or during a borrowed server guest's turn do nothing.
This fragment receives an API number when stamped after merge.

API 2 adds the item panel beside container screens and its recipe pages (`native_panel_items`,
`native_panel_recipes`, and the hook `native_panel_event` for the player's clicks, searches and keys over items), and
the game's items and recipes (`native_recipes`, `native_known_recipes`, `native_place_recipe`, `native_give`): what a
recipe viewer needs.

The pending `panel_style` helper, `native_panel_style(mod_id, style)`, lets a recipe viewer's panel look like one:
`NativePanelStyle::Viewer` draws its page buttons as square buttons on a bar over the items and its search box with the
viewer's skin, as Horizon's Recipes panel does; `NativePanelStyle::RecipeBook` (the default) keeps the recipe book's
arrows. Only the drawing changes. Bundles need `"hooks": @API:panel_style@`.

API 11 adds `GameLevel::native_send_particles` on authoritative servers. It sends supported
vanilla particles to nearby players. Block particles require a valid block state. Item, trail and
vibration particles are not supported by the current dispatch. Count zero sends one directional
particle. The return value says whether the call was accepted, not whether any player received it. Bundles that use it need `"hooks": 11`.

API 3 adds authoritative world lifecycle events through `native_world_lifecycle`: open ready, open failed,
and final close. Bundles that use these events need `"hooks": 3`.

API 4 adds authoritative server tick events through `native_server_tick`: start (before any level ticks, like NeoForge's `ServerTickEvent.Pre`) and end of each non-paused
server tick, with `world_running`. A frozen world reports false except during a requested step. Bundles that
use these events need `"hooks": 4`.

API 8 adds server commands and players: `native_register_commands` (the commands a mod adds to every player's
command tree) and `native_perform_command` (a player's command on the server), with helpers to answer
([Server commands and players](#server-commands-and-players)). Bundles that use them need `"hooks": 8`.

API 9 adds the acting player's plain items for server mods (count, take, give; [Server commands and
players](#server-commands-and-players)). Bundles that use them need `"hooks": 9`.

API 10 adds server menus: a vanilla chest menu a server mod opens and fills, whose clicks and closing reach the mod
(`native_menu_clicked`, `native_menu_closed`), and `native_exchange_items`, two players' stacks swapped in one checked
step ([Server menus](#server-menus)). Bundles that use them need `"hooks": 10`.

API 12 adds item tooltips through `native_tooltip`: the hovered stack, with its components, and its tooltip's
lines, which a mod can add to, change or take out of, wherever the game shows an item's tooltip. Bundles that use it
need `"hooks": 12`.

API 13 adds using an item, on the server for every player: `native_use_item_on` hears a right click with an item on a
block once the block let it through (`ItemStack.useOn`), and `native_use_item` hears the item's own use, in the air or
after `native_use_item_on` let it through (`Item.use`). Each hears a click once. A mod that claims one swings the hand
instead of the item's use. Bundles that use them need `"hooks": 13`.

API 14 adds breaking and placing a block, on the server for every player: `native_destroy_block` hears a player break a
block once the game allowed it (`ServerPlayerGameMode.destroyBlock`) and may cancel it, which keeps the block and its
drops; `native_place_block` hears a block placed from an item (`BlockItem.place`). To stop a placement, a mod claims
`native_use_item_on`, which runs before it. Bundles that use them need `"hooks": 14`.

API 15 adds block entities for a mod's blocks (`native_block_entity`, `native_block_entity_ticks`,
`native_block_entity_tick`): made when the block is placed, ticked on the server and saved with the chunk. Bundles that use them need `"hooks": 15`.

API 16 adds item slots for those block entities (`native_block_entity_container`): hoppers, comparators and breaking the
block use them as they use vanilla's containers, and `native_block_entity_tick` also gets the slots. Bundles that use them need `"hooks": 16`.

API 17 adds menus for mod containers: the container names one of vanilla's slot-grid or furnace menus, which opens when the block is used,
and `native_block_entity_tick` takes the whole block entity, with the data slots the menu shows. Bundles that use them need `"hooks": 17`.

API 18 adds what a machine's tick reaches: `native_with_mod_block_entity` (change a neighbouring mod block entity, which marks its chunk
and tells comparators when its items changed) and `native_set_block_property` (a property of the block's own state, through the level's
normal update path). Bundles that use them need `"hooks": 18`.

API 19 adds screens a mod draws for its blocks' menus: `native_screen` lays one out (its height, where each container slot is,
where the inventory starts; a slot it leaves out is not in the menu, so shift-clicks skip it) and `native_screen_draw` draws it with the mod's
own GUI textures (`assets/<namespace>/textures/gui`, drawn with `native_sprite`). Bundles that use them need `"hooks": 19`.

API 20 adds buttons, tooltips and item icons on mod screens: `native_screen_click` and `native_screen_tooltip` (the client), `native_menu_button`
(a button press on the server, on the mod block entity) and the helpers `native_item`, `native_screen_title`, `native_data`, `native_block`,
`native_block_property`, `native_insert_item` and `native_cursor`. Bundles that use them need `"hooks": 20`.

API 21 adds what a server mod needs to guard land and move players. `native_right_click_block` hears a player's right
click on a block before the block's own use (a chest, a door, a lever) and the item's, and may cancel it: nothing
happens and the player sees the block as it is. `native_acting_player` says who acts inside a player's hook (UUID,
name, level, position, game mode, health); `native_run_command` runs a command as the server's console (level 4, no
output), so `execute in <dimension> run tp <player> <x> <y> <z>` moves a player across levels; `native_action_bar`
tells a player why, even inside their own hook; and the players
`native_server_players` lists carry their health. Bundles that use them need `"hooks": 21`.

API 22 adds what a server mod needs to keep the world itself out of guarded land. `native_block_spread` hears a
fluid flow or fire spread into a block, `native_piston_move` a piston's push or pull, `native_explosion_detonate` the
blocks an explosion is about to break (a mod takes out those it spares), `native_fill_bucket` a bucket filled or
emptied, and `native_attack_entity` a player's blow on an entity (an item frame, an armor stand, an animal, a
player). Each may cancel, and nothing happens. Bundles that use them need `"hooks": 22`.

API 23 adds `native_level_tick_pre` and `native_level_tick_post`, server-side observations around each
level tick pass. Each receives the dimension ID; neither can cancel the tick. Paused and held passes fire
neither hook, and an error skips the post hook. Bundles that use them need `"hooks": 23`.

API 24 adds `native_set_block`, a server-side `Game` helper that replaces a state in the current dimension through
`Level.setBlock`. It accepts vanilla's neighbour and shape flags and returns whether a block changed; a block with a
block entity gets one. Horizon sends every write to players whatever the flags: `UPDATE_CLIENTS`, `UPDATE_INVISIBLE`
and `UPDATE_IMMEDIATE` change nothing yet. Hooks holding only `GameLevel` cannot call it yet. Bundles that use it need
`"hooks": 24`.

API 25 adds `GameLevel::native_play_sound` for registered vanilla sound events on an authoritative server.
It queues the sound for nearby players in the level's dimension, including during a guest's turn.
Pass a `SOUND_SOURCES` index, position, volume and pitch. The return value says whether the call
was queued, not whether a player heard it. The server discards unknown sound events when it dispatches
the queue. Bundles that use it need `"hooks": 25`.

API 26 adds `Game::native_config_dir()`: the game's shared `config/` folder beside `options.txt` (beside
`server.properties` on a dedicated server), where a mod names its own files, made when missing. It takes no `self`, so
it works from any hook ([mods.md](mods.md#native-mods)). Bundles that use it need `"hooks": 26`.

API 27 adds the player-health HUD rows through `native_health_hud`: every frame the HUD draws them, the
hook hears the layout's inputs (`HealthInputs`) and the screen's GUI size, and a mod that answers with
draws replaces all the rows — armour, hearts, food, air and the heights above the hotbar — for that
frame. Nothing answered draws Horizon's own. Bundles that use it need `"hooks": 27`.

API 28 adds `native_block_in_tag` and `native_item_in_tag` for server tick hooks. They read the world's merged block and item tags, including data packs and content mods. An unknown tag returns false. An empty item stack tests the `minecraft:air` item, as vanilla does. Calls with an inactive or stale tick context return an error. Bundles that use them need `"hooks": 28`.

API 29 adds right-clicking an entity, on the server for every player: `native_interact_entity` hears a right click on
an entity (its id and type, and the stack in the hand) where `Player.interactOn` starts, once per hand. A mod that claims
it swings the hand instead of the entity's and the item's own interaction. Bundles that use it need `"hooks": 29`.

API 30 adds left-clicking a block, on the server for every player: `native_left_click_block` hears the start of a dig
(the block, the face hit) once the game allowed it, before the block is attacked (a note block plays) or broken in
creative. A mod that cancels it keeps the block: on the server it isn't attacked, dug or broken, and the player's client
gets it back. The clicking player's own client still predicts the crack until then.
Bundles that use it need `"hooks": 30`.

API 31 adds a player's server tick: `native_player_tick` runs once a tick for every living player in the world, at the
end of their `ServerPlayer.tick`, with the helpers reading that player. Players still tick while the game is frozen
(`/tick freeze`), as in vanilla. A dead player isn't called, and neither is one killed earlier in the same tick; it
runs again once they respawn. Bundles that use it need `"hooks": 31`.

API 32 adds shape updates a mod can answer through `native_update_shape`: on an authoritative server, each time a
block's state is worked out from a neighbour (`BlockBehaviour.updateShape`): neighbour shape updates after a
`setBlock`, a block fitted to all its neighbours as a piston lands it or a chunk is post-processed, and the blocks at
a feature's edge as it is placed in the live world, such as a tree grown from a sapling
(`StructureTemplate.updateShapeAtEdge`). It gets the dimension, position, state, the neighbour's direction, position
and state, before the block computes it. `state` may not be in the world yet (a piston landing it), so decide from
`state`, not from the block at `pos`. Answer only for your own blocks and leave `new` empty otherwise: a state put
there is the block's new state, applied as vanilla applies the result (air breaks the block, with its drops, on a
neighbour update; post-processing and a feature's edge write air without drops), and the block's own `updateShape`
doesn't run, nor the ticks it would schedule. The level is read-only, as vanilla's `LevelReader` is there; the hook
schedules ticks with its `ticks` through the level's `native_schedule_block_tick(ticks, pos, block, delay)` and
`native_schedule_fluid_tick(ticks, pos, fluid, delay)` (vanilla's `ScheduledTickAccess.scheduleTick`; false for an
unknown name or a position that isn't loaded). It runs often, at least six times per `setBlock` that updates shapes
(more for redstone wire's diagonals and knock-on updates): keep it short. A LAN guest shows vanilla's shape for a
moment, until the server's update arrives. Chunk generation fires none. Bundles that
use it need `"hooks": 32`.

API 33 adds random ticks a mod can handle through `native_random_tick`: on an authoritative server, each block
`ServerLevel.tickChunk` random-ticks (`BlockState.randomTick`), with its dimension, position and state, before the
block handles it. Only blocks that tick randomly fire it (a mod block as its vanilla template block does), once per
draw that lands on one: it runs often, so keep it short. Bone meal's random ticks (`StemBlock.performBonemeal` calls
`randomTick` itself) don't fire it. The hook may change the block with the level's
`native_set_block_property(ticks, pos, key, value, flags)` and set `handled` to skip the block's own random tick, so a
mod crop grows its own way. Without `handled`, the block then takes its own random tick as it is now (if it still
ticks randomly), and lava's fluid random tick runs either way. It needs the level mutably, which only `native_random_tick` and API 34's `native_tick_block` give. It is API 18's `Game::native_set_block_property` with `Level.setBlock`'s flags (`native_hooks::flags`): a
vanilla crop grows with `flags::UPDATE_CLIENTS` (neighbours' shapes update, so an observer fires, but no neighbour
gets `neighborChanged` and comparators aren't told), a furnace sets its `lit` with `flags::UPDATE_ALL` (both). Horizon
sends every write to players whatever the flags: `UPDATE_CLIENTS` (2), `UPDATE_INVISIBLE` (4) and `UPDATE_IMMEDIATE`
(8) change nothing yet. It writes at any loaded position, returns false off an authoritative server (the `Game` one too: a window that isn't a server), for a property or
value the block doesn't have, a position that isn't loaded, or a property that already has the value (nothing is written:
vanilla's `Level.setBlock` returns false for an unchanged state, as `native_set_block` does), and true when the block changed. Elsewhere, `Game::native_set_block_property_with(pos, key, value, flags)` does the same, as a brewing
stand's tick sets its bottles with `flags::UPDATE_CLIENTS`. Bundles that use
them need `"hooks": 33`.

API 34 adds scheduled block ticks a mod can handle through `native_tick_block`: on an authoritative server, each due
scheduled tick of a block that is still there (`ServerLevel.tickBlock` → `BlockState.tick`), with its dimension,
position and state, before the block handles it. The hook may change the block with the level's
`native_set_block_property` and the flags the block's own tick would write with (as in API 33: an observer's tick
uses `flags::UPDATE_CLIENTS`) and set `handled` to skip the block's own tick. Without `handled`, the block then takes
its own tick as it is now, if it is still the tick's block. Bundles that use
it need `"hooks": 34`.

API 35 adds the commands a mod owns on the client: `native_commands` is asked for their names, so the chat box
colours, completes and lists them like the game's own, and `native_command` runs them. A name the game already has
keeps that command. Bundles that use it need `"hooks": 35`.

API 36 adds recipes of a mod's own types, for a machine's crusher or altar. A recipe file whose `type` is outside `minecraft`
(`mymod:crusher`) in `data/<namespace>/recipe/`, in a content mod (`.hmod`) or a world's data pack (not in a native mod's own bundle), stays loaded as it is: `native_recipes_of_type`
gives each one's id and JSON, for the mod to read as it likes, and `native_recipe_ingredient` and `native_recipe_result` read the
usual fields, tags included (a bare id is read as `minecraft:`; items the game doesn't know are left out, and the helper gives nothing if none is left). The recipes are the server's table: on the server thread, a dedicated server or a LAN host; a client, the single-player window included, sees only the content mods' recipes, not the world's data packs'. All three give nothing outside a world. The game doesn't craft them or show them in the recipe book; `/reload` reads a data pack's again.
Bundles that use them need `"hooks": 36`.

API 37 adds [mobs](#mobs) for server mods: spawn one, read the mobs of a level in place, and move, hurt or remove one by its id.
Bundles that use them need `"hooks": 37`.

API 38 adds redstone from a mod's block, on authoritative servers. `native_block_signal` may change the signal a block gives on
a face, weak or direct, and `native_is_signal_source` lets redstone wire connect to it. Both are asked very often, so check the
block first and answer from the mod's own state. When the output changes, and once the block is placed (a direct signal past a conductor isn't read before), `native_update_signal_neighbors(dimension, pos)` makes the blocks around
it read it again; it acts on the level in place and is refused for another dimension. Bundles that use them need `"hooks": 38`.

API 39 adds item behaviour, on the server for every player: `native_hurt_enemy` hears a player's hit land on a mob with the
held stack, `native_inventory_tick` runs each tick for every stack a player carries (armour and off hand included), and
`native_carried_stack` reads and changes that stack, and `native_on_equip` hears an equipment slot take a different item. The helpers read the acting player. Bundles
that use them need `"hooks": 39`.

API 40 adds [mod messages](#mod-messages) between a mod's server and client halves, and hearing players and mobs on the
server for every player: `native_player_died`, `native_mob_spawned`, `native_mob_killed` (a player's melee blow, a sweep or a spear thrust too),
`native_item_dropped` (a throw; not the items scattered when a player dies) and `native_item_picked_up` (`canceled` leaves the stack on the ground). They name a player by UUID. Bundles that use them
need `"hooks": 40`.

API 41 adds bundles' data files (`native_data!`, `native_built_data`; "Data files" below). Bundles that use them need
`"hooks": 41`.

API 42 adds the item entities near a block, for machines that pick up items as a hopper does:
`native_item_entities(dimension, min, max)` lists those whose box meets `min`..`max` (vanilla's
`Level.getEntitiesOfClass(ItemEntity.class, box)`), each with its entity ID, position, stack and pickup delay, and
`native_set_item_entity(dimension, id, stack)` changes one's stack, or removes it when the stack is empty, as a hopper
does with what it takes. Both act on the level in place, as `native_set_block` does. Bundles that use them need
`"hooks": 42`.

API 43 adds `native_level_random(dimension)`, the server level's own random (vanilla's `Level.random`, NeoForge mods'
`level.getRandom()`): a mod draws from the stream vanilla's explosions and block drops use, so its draws shift theirs
as a Java mod's would. It is `None` on a client of a server and for a level that isn't open: the Nether and the End
open when someone first goes there and close after 300 ticks with nobody in them. Bundles
that use it need `"hooks": 43`.

API 44 adds the server starting and stopping, on the single player server thread and on a dedicated server:
`native_server_about_to_start(dedicated)` before the worlds load (NeoForge's `ServerAboutToStartEvent`), and
`native_server_stopping(dedicated)` when an orderly stop begins, with the players still connected and not yet saved
(`ServerStoppingEvent`). Each start is followed by `native_world_lifecycle`'s `WorldOpenReady` or `WorldOpenFailed`,
and each stop by `WorldCloseFinal`; a server whose loop fails reports `WorldCloseFailed` instead of stopping. Bundles
that use them need `"hooks": 44`.

API 45 sends mod block entities to clients. Each mod block has a block entity type of its own, named after it, after
vanilla's; a client makes its copy of the block entity with `native_block_entity`, as the server does.
`native_block_entity_update_tag` gives what clients get of one (`BlockEntity.getUpdateTag`, read from the block entity
alone): it is asked when a chunk holding it is sent and when its block changes, and the client merges it into the
copy's `nbt`. A saved chunk sent past the view distance asks it on its own thread, so read only `entity`. Keep it to what clients show (a machine running, its facing, a tank's level), not a menu's progress:
menus have their own data slots. Bundles that use it need `"hooks": 45`.

Fields meant for the next client update only, not for the save (the items a processing particle shows):
`ModBlockEntity::set_client_update_fields(Compound)` replaces the pending batch, `clear_client_update_fields` drops it
and `take_client_update_fields` drains it. The owner adds the drained fields to its `native_block_entity_update_tag`
result itself; setting them neither schedules an update nor is saved, compared or cloned, and repeated block changes
may coalesce into one update.

`native_block_clone_item(block, pos, state, &mut stack)` answers a block's `getCloneItemStack` for pick block
(Ctrl+Pick) with the real state and position; the first mod that fills the stack owns the result, and no answer keeps
vanilla's. An empty or unregistered stack picks nothing. Ctrl+Pick still adds the block entity's components afterwards.

API 46 adds `native_visuals`: a world-frame hook that appends block-rim and
colored-light requests. Rims trace the client's existing block outline edges,
depth-tested against terrain. Lights add camera-local RGB illumination to the
normal terrain/entity shaders, not gameplay light. Each mod has a budget of its
own: its first 256 rims and eight lights are examined, and its rims draw at most
4096 outline edges, so one mod's requests never crowd out another's (requests
bundles' injections push share one budget). Positions must be within 32 blocks
of the player, values must be finite and bounded, and source chunks must be
loaded. A rim that would pass its mod's edge budget is skipped whole. At most
16 colored lights are rendered in all. Requests are frame-local,
so disabling a native mod or leaving the world clears them without a retained
state. Helpers `native_lit_block_name` and `native_flame_particle` read loaded
blocks and emit a nearby vanilla flame particle; native mods must rate-limit
emission. It also adds `native_emissive`: a world-frame hook for at most 32 pairs of
already-loaded terrain sprites. Transparent pixels in an overlay leave its
base unchanged; opaque pixels are drawn full-bright without changing gameplay
light or the surrounding rock. The source pack must supply the overlay texture
under `assets/<namespace>/textures/emissive/`. When no mod requests overlays,
the terrain shader skips the lookup. Bundles using these hooks need `"hooks": 46`.
When a native mod bundles a Horizon shader pack, naming its folder
`native-<mod-id>` makes the Mods On/Off control pause that pack too (without
changing the player's saved shader selection); other packs are unaffected.

## A bundle

A bundle is a folder with a `horizon-mixin.toml`. Besides its helpers and modules (horizon-mixins' README) it can
have its own crate, data files and a post-build step (horizon-mixins 0.6.0). The test bundle
[`launcher/tests/fixtures/native-crate-demo`](../launcher/tests/fixtures/native-crate-demo) uses all of them:

```
native-crate-demo/
  horizon-mixin.toml
  frame.rs                # helper, compiled into the game
  data/greeting.txt       # the bundle's data: native_data!("crate_demo", "greeting.txt")
  engine/                 # the mod's own crate
    Cargo.toml            # [dependencies] crc32fast = "1"
    data/salt.txt         # the crate's data: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/salt.txt"))
    src/lib.rs
    src/bin/bake.rs       # the post-build step
```

```toml
[mod]
id = "crate_demo"
version = "0.1.0"

[[injection]]
id = "frame"
hook = "frame"            # a name from native-hooks.json, instead of `target`
hook_version = 1
kind = "HEAD"
helper = "frame.rs"
bindings = [{ name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" }]

[[crate]]
id = "engine"
path = "engine"           # holds Cargo.toml
# dependent = "horizon-client"   (the default: the workspace package that depends on it)

[[post_build]]
crate = "engine"
bin = "bake"
args = []
```

### Named hook points

`native-hooks.json` at the root of the source names each hook point and the version of its contract (signature,
when it is called, what it does): `{"name": "hud", "version": 1, "target": "horizon_client::game::native_hooks::Game<'_>::native_hud"}`.
An injection names `hook` and `hook_version` instead of `target`, and horizon-mixins resolves it from the catalog
of the source it applies to, so a hook point can move in the source without breaking the mods that use it. A name
the catalog lacks is `E_HOOK_UNKNOWN` (the error lists the names it has), a version it lacks `E_HOOK_VERSION`. A
contract is never changed, its `mod_api` entry (the trait method's contract) included: an incompatible change adds
a version. Helpers (`native_text`, `native_player`…) have no hook entry; the catalog's `helpers` list names the ones
the generated `Host` offers, and they are kept stable as the hook points are. A test (`catalog_matches_hook_points`) keeps the
catalog and the hook points' sources (`native_hooks.rs`, `gfx/native_render.rs`, `horizon-world`'s `loot.rs`) in step.

### How fast a player digs

`native_break_speed` is NeoForge's `PlayerEvent.BreakSpeed`, at the end of vanilla's `Player.getDestroySpeed`. It is
told the block's state and position, and the speed vanilla gives the player for it: the tool, mining efficiency, Haste
and Mining Fatigue, the block break speed attribute, and the slowdowns under water and off the ground. A mod may change
`speed`. Every mod is called in turn, and `original` stays vanilla's. `BlockBehaviour.getDestroyProgress` then divides
the speed by the block's hardness, and by 30 (or 100 without the right tool), for each tick of the dig. A speed of 0
makes no progress, so the block isn't broken, not even one that breaks at once. It isn't asked for an unbreakable
block (hardness -1) or in creative mode. It fires on the `Game` that works out the dig: in single player, on a LAN or
dedicated host for each guest (the helpers read that player), and in a remote client's own prediction. A mod that
changes the speed alike on both sides keeps the crack in step with the server. It is asked every tick of a dig, on
each side, so it takes `&self` and should only compute. API @API:break_speed@.

### Host-aware weapon damage (API next)

`living_incoming_damage_v2` is an additive hook, like `menu_button_v2`; v1 keeps its signature and firing point.
It receives a mutable `Host`, attacker/victim network ids, damage type, optional source position, victim position
and current view vector, and an optional weapon snapshot. `weapon.enchantments()` exposes enchantment ids and
levels. Change `amount` or return `Flow::Cancel`; the host can read the world and play sounds through its helpers.
It currently fires for player-to-mob melee, sweep and spear hits (including LAN guests), after the victim's invulnerability check (an invulnerable or dead victim fires nothing) and before
v1. Unless canceled, v1 receives the modified amount. It is not a replacement for v1 on
projectiles, player victims, dragons, mob attacks or environmental damage. There is no extra per-hit allocation
without loaded mods. The fragment `living_incoming_damage_v2.json` receives its API number at merge.

### Damage the health takes

`native_living_damage_pre` and `native_living_damage_post` are NeoForge's `LivingDamageEvent.Pre` and `.Post`, at
the end of vanilla's `LivingEntity.actuallyHurt`, for the players, mobs and ender dragon that
`native_living_incoming_damage` hears, with that hook's arguments. Pre comes once armour (worn down by the hit
already), enchantment protection, effects and absorption have taken their part, with the amount about to come off
the health. A mod may change it: a negative or NaN amount takes nothing, an infinite one `Float.MAX_VALUE`. The hit
can't be canceled any more, so its invulnerable time, hurt sound and knockback follow whatever the amount. Post
comes once the health and the combat tracker are done, with what came off the health, 0 when nothing did (absorption
took it all, or a mod made it 0), and only hears. Neither fires when no hit reaches `actuallyHurt` (refused,
canceled by `native_living_incoming_damage`, or within the cooldown and no stronger than the last hit), nor when a
wolf's armour takes the hit. Like `native_living_incoming_damage`, they have no `self` and run on the server thread.
API @API:living_damage@.

### The build's warnings

Before it builds, the launcher reads the mods' `[[injection]]` lists and says three things in `native/build.log` that
horizon-mixins only implies by its order. Two mods injecting at one place — the same function, kind, selector and
ordinal; the places are the manifest's, horizon-mixins is the one that resolves a selector against the source:

```
warning: mods beta, alpha both inject at hook `frame` HEAD; beta runs first (priority descending, mod id ascending).
```

Two mods wrapping one function at its head and its return, which run in that order whatever their priorities:

```
warning: mods alpha (HEAD), beta (RETURN) inject into hook `tick`: HEAD runs on entry, RETURN on return, whatever their priorities.
```

And a `cancellable = true` `HEAD` on a hook point:

```
warning: alpha's `stop` can cancel at hook `frame` HEAD: a Break skips the later injections and the function's body.
```

A raw `target` is not matched against the catalog — whether it names a hook point's function only the source knows —
so the third warning names a `hook`, and `hook = "frame"` and `target = "…::native_frame"` are not told apart. Only
`HEAD` is named: the interior kinds may carry `cancellable` too, but what a `Break` skips there is less than the body.
A mod's `priority` defaults to 0, as horizon-mixins defaults it. Neither warning stops the build.

### A mod's own ingredient kinds, matching at craft time

The load-time list only names items. Matching a concrete stack — its components, usually —
is `native_ingredient_test(kind, data, stack, accept)`: the ingredient's `test` calls it with the
kind and JSON the load-time hook saw, the stack being matched and the verdict, already `true`
(the list named the item). Set `accept` to `false` to refuse the stack (`ICustomIngredient.test`).
The hook may be called many times a craft; the answer must be the same for the same stack.

### The mod's crate

A `[[crate]]` is a Cargo package of the bundle's, staged as `native-crates/<mod>/<crate>/` and added to the
workspace's members and to the `[dependencies]` of its `dependent`, so helpers call it as
`crate_demo_engine::checksum(…)`. Cargo compiles it once and keeps it: editing a helper, or switching another mod
on or off, recompiles `horizon-client` but not the mod's crate, so the heavy code of a big mod belongs there and
only the glue in helpers. A crate can't depend on `horizon-client` (Cargo forbids the cycle) or on any path or git
crate, and has no `Cargo.lock`, `[workspace]` or build of its own. Horizon's own crates (`horizon-core`…) are path
crates too, so they are refused the same way, on purpose for now: a mod names game types through its helpers, which
are compiled into `horizon-client`; a stable API crate for mods' own crates is #3897.

**Dependencies come from Horizon's `Cargo.lock`.** A crate names registry crates by version requirement
(`crc32fast = "1"`, `{ version, features, default-features, optional }`); each must already be in Horizon's lock
file in a version that matches, and builds in exactly that version. The crates that are there only for mods are listed in
`crates/horizon-mod-deps/Cargo.toml` (optional dependencies, with the features mods may turn on; nothing builds them):
`mysql` 28 (blocking MariaDB/MySQL client, `minimal-rust` + `rustls-tls-ring` + `chrono`, no default features), `redis` 1
(blocking, no default features), `tungstenite` 0.30 (websocket over a stream the mod opens, no default features),
`rusty-opus` 1 (Opus encoder and decoder), `quinn` 0.11 (QUIC on tokio, features `runtime-tokio` + `rustls-ring`, no
default features), `rusqlite` 0.41 (SQLite, `bundled` + `cache`, no default features; builds SQLite's C source),
`yaml-rust2` 0.13 (YAML 1.2 reader and writer, no default features, so UTF-8 input only), `gif` 0.14 and `snap` 1
(Snappy). A feature of theirs that turns on a dependency not listed there is refused.
The lock file is the allow list: horizon-mixins
refuses a crate the lock lacks (`E_CRATE_DEPENDENCY`) and writes the mod's crates into the staged lock without
changing any other byte, so the game still builds with `--locked`. The launcher checks it again (every downloaded
package of the staged lock as in Horizon's: name, version, source, checksum), and a feature that turns on a
dependency the lock lacks fails at `cargo fetch --locked` with a message saying so. The build stops with:

```
horizon-mixins failed (exit status: 1). The end of its output, the whole of it in …/native/build.log:
E_CRATE_DEPENDENCY: crate_lock needs rand ^0.9, which isn't in Horizon's Cargo.lock (…/horizon-mixin.toml)
```

### Conditional data

A data file that carries NeoForge's `neoforge:conditions` loads only when every condition holds, and is
silently absent otherwise — as on NeoForge, where packs gate cross-mod recipes on the other mod being
there. The recipe loaders (crafting, the stations, brewing, the recipe book, and the packs' overlays
over them) all answer the gate; `mod_loaded` asks which mods' data is in the root: `minecraft`, always,
and the content mods that are on. `neoforge:true`, `not`, `and` and `or` combine; a condition the game
does not know leaves the file out, so a future shape fails closed. Tags, loot tables and advancements
do not answer the gate yet.

### Recipe filters

A content mod or a data pack can remove recipes, or swap one of their inputs or outputs, without knowing
their ids: a file `data/<ns>/recipe_filter/<name>.json` holds `{"filters": [...]}`. Each filter has match
keys, all of which must hold (a filter with none matches every recipe), and exactly one action:

| Key | Meaning |
|---|---|
| `id` | the recipe id; `*` matches any run of characters (`minecraft:*_planks`) |
| `type` | the recipe's `type`, with the same `*` (`mymod:*`) |
| `input` | an item id or `#tag` among the recipe's inputs |
| `output` | an item id among its outputs (anything under `result`, `results`, `output` or `outputs`) |
| `remove` | `true`: the recipe is not loaded |
| `replace_input` | `{"from": ..., "to": ...}`: every input that is `from` becomes `to` |
| `replace_output` | the same, for the outputs |

```json
{"filters": [
  {"id": "minecraft:*_planks", "remove": true},
  {"type": "mymod:*", "replace_input": {"from": "minecraft:iron_ingot", "to": "#c:ingots/iron"}}
]}
```

Filters run on each recipe after the packs are merged and its load conditions held, so they see the final
set and reach every recipe type, mods' own included: they work on the recipe's JSON, every string outside
`type`, `group`, `category` and the conditions (a `{"tag": ...}` object's tag reads as `#tag`). Every recipe
loader applies them: crafting, the stations, brewing, the recipe book and what players are sent. Files apply
in id order and each file's filters in order; a pack's file replaces a file of the same id, as any data file
does. A filter with an unknown key or not exactly one action is skipped with a warning, so a typo never
matches every recipe.

### A mod's own enchantment effect kinds

A `minecraft:post_attack` entry with `enchanted` attacker and `affected` victim whose `effect.type` is in a
namespace outside `minecraft:` — where the game used to skip it silently — reaches
`native_enchant_effect(enchantment, kind, data, level, victim)` when a player's weapon hits a mob and the entry's
`requirements` pass. `victim` is the mob's id, and the acting player is the one holding the weapon. A bare type
name is minecraft's own. Other components and pairs, a hit on a player, and an entry inside `minecraft:all_of`
aren't told yet. An unclaimed kind does nothing.

### A mod's own consume effects

A consumable's `on_consume_effects` entry of a type in a namespace of its own — where the old loader
dropped it silently — reaches `native_consume_effect(kind, data)` when the item is consumed, on the server
thread, for the acting player. The kind and the JSON the file wrote are there; the game's own effects,
sounds, teleports and drops are open to the hook. An unclaimed kind does nothing.

### A mod's own loot functions and conditions

A loot table's function or condition that names a `type` in a namespace outside `minecraft:` — where the
loot roll used to silently skip it — reaches `native_loot_function(kind, data, ctx, stack)` or
`native_loot_condition(kind, data, ctx, holds)` when the table rolls. The function changes the stack; the
condition says whether it holds. The context brings the entity, the damage, the player who killed it, the
tool, the block, the luck, the biome and a seed from the roll's random. An unclaimed function leaves the
stack, an unclaimed condition fails, as unknown kinds do; a bare type name is minecraft's own.

The tables the items engine rolls (block drops, entity drops, `loot_table`, `interact_loot` and the
built-in tables) answer both hooks, with the tool and the block in the context. The function hook there
gets the stack as the main engine's (`LootStack`): its item, count, damage, enchantments (a book's stored
ones), potion, ominous bottle amplifier, stew effects and the components the engine keeps as saved, as
their JSON; what it changes comes back to the engine's stack, and a component it adds arrives with the
drop. A function that asks for a map's structure search (`map`/`exploration`) changes nothing there: the
items engine has no level to search. In the items engine's context the `entity`, `damage`, `player` and
`biome` fields are `None` and `luck` is 0; in the main loot engine's context the `tool` and `block` fields
are `None`.

Both engines' contexts carry `place`, where the roll happens: `origin` (a broken block's centre, a dead mob's
position), `block_entity` (the broken block's entity as it was before the break, saved with its id, position and
`Items`) and `level` (the dimension, the game time, the time of day, the rain and the thunder, for conditions like
vanilla's `time_check` and `weather_check`). The game sets it for a block a player or an explosion breaks and for a
mob's death loot; on other rolls `place` is `None`.

### Recipe book category declarations (first slice of #5501)

The pending `recipe_book_categories` API fragment adds a declaration hook and a
lookup helper. It has no API number until stamping; do not require a guessed
hooks version. Trait mods import `RecipeBookCategory` from `horizon_client::mod_api`
and append declarations in `Hooks::recipe_book_categories`:

```rust,ignore
event.categories.push(RecipeBookCategory {
    id: "mymod:crushing".into(),
    recipe_type: "mymod:crusher".into(),
    icon: "mymod:crusher".into(),
});
```

Mixin bundles use `native_recipe_book_categories(categories)` at the same hook
point. `host.recipe_book_category("mymod:crusher")` (the mixin helper is
`native_recipe_book_category`) returns the validated declaration, including its
icon, or `None`. It does not grant recipes. Declarations are requested lazily at
first lookup, cached for that recipe table, and invalidated on a new session,
server recipe-content replacement or server `/reload`. No lookup means no hook
call or per-frame work. A client validates against its own custom recipes unless
`native_datapack_sync` has supplied the server's type (API 120).

Every field must be a fully namespaced resource id. Category ids and recipe types
must be outside `minecraft:`; the type must occur in the current custom recipe
table. The icon must name a registered, non-air item. Invalid ids, unknown types,
missing/unknown icons, and duplicate category ids or recipe types are logged and
skipped, never panicked on. The first valid declaration wins each id and type;
an invalid declaration does not reserve either. This slice supports one category
per type, not per-recipe category selectors.

**Not yet book UI support:** custom recipes still have no book displays or
unlock/display-id synchronization. This slice deliberately leaves vanilla tabs,
search, layouts and unlock behavior unchanged. Adding custom tabs, displaying and
searching recipes there, and unlocking them like vanilla remain follow-up work
for #5501; a declaration alone will not make a recipe visible.

### A mod's own ingredient kinds

A recipe's ingredient that names a type in a namespace outside `minecraft:` reaches
`native_ingredient(kind, data, items)` at load time. Fill `items` with the item ids the
ingredient matches (item ids only: components are not checked; a match-time hook follows
separately). Duplicates are removed keeping first-seen order. The hook may be called more
than once per recipe; the answer must be the same. An unclaimed kind leaves the list empty
and the recipe fails, as an unknown kind does.

### Data files

A bundle's `data/` folder (any files) is staged as `native-data/<mod>/` at the root of the workspace. A helper or
module compiles a file in with `horizon_client::native_data!("<mod id>", "path/in/data")` (`&'static [u8; N]`,
`crate::native_data!` inside the lib), since hook points API 41. A mod's crate keeps its own data in its folder and
reads it with `include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/data/…"))`. The registry's `.hnmod` may carry
both (`mixins/<id>/data/…`, `mixins/<id>/<crate>/data/…`).

Both are compiled in: neither folder is beside the game when it runs. A file the game reads at run time is one a
post-build step writes into `HORIZON_NATIVE_DATA_DIR` (below), copying it from `HORIZON_NATIVE_BUNDLE_DATA_DIR` or
making it; the game finds it with `native_built_data("<mod>")` (None only when the game can't tell its own folder).

**What a `.hnmod` holds.** Only files under `mixins/<id>/`: `.rs` and `.toml` files, the data folders above, and the
text files a crate folder carries (`README*`, `LICENSE*`, `COPYING*`, `NOTICE*`, `.gitignore`, in the bundle's
folder or a crate's), so a crate that compiles its README in builds the same from the registry as from a folder. At
most 512 files and 64 MiB unpacked; anything else refuses the bundle. A local folder build has no such limits.

**One `.hmod` for content and code.** A mod's content and its native code ship as one `.hmod`, and what is inside
decides what happens: only `horizon-mod.toml` with `assets/` and `data/` is a content mod, nothing compiles; with
`horizon-native.json` and `mixins/<id>/` (a bundle as above) it is a native mod, built into the game. Both may be in
one file: `horizon-mod.toml`, `assets/` and `data/` are its content, checked as a content mod's
(`horizon_mods::Content`) and left out of the build, and the two manifests name the same mod id. A `.hmod` with
native code is never a WebAssembly mod: one carrying `mod.wasm` is refused with
`horizon_mods::index::native::WASM_GONE`. An `.hnmod` (code only) keeps loading as before. The registry keeps
publishing native mods as `.hnmod` until launchers and the game read native `.hmod`s. Every rule of the file is in
[api/mod-files.md](api/mod-files.md).

### Post-build steps

A `[[post_build]]` names a bin of one of the mod's crates. After the game compiles, the launcher builds it with the
same profile into the same `native/target/`, then runs it (at low priority, in mod order) in the new build's version
folder with:

| Variable | What |
|---|---|
| `HORIZON_NATIVE_MOD` | the mod's id |
| `HORIZON_NATIVE_VERSION_DIR` | the new build's version folder (also the working directory) |
| `HORIZON_NATIVE_DATA_DIR` | `<version folder>/native-data/<mod>/`, created: what the game reads at run time with `native_built_data("<mod>")` (or `horizon_client::native_data::built`) |
| `HORIZON_NATIVE_CACHE_DIR` | `<launcher data>/native/cache/<mod>/`, created, kept from one build to the next |
| `HORIZON_NATIVE_BUNDLE_DATA_DIR` | the bundle's own `data/` as staged for the build (`native/stage/native-data/<mod>/`); read it, don't write it |

Its output goes to `native/build.log`. A `native-post-build.json` that can't be read, or names a mod id that isn't
one, stops the build instead of skipping the steps. When a step fails, so does the build ("crate_demo's post-build
step failed …") and the version folder is removed. It runs on the player's computer like the rest of the mod's code: only the
launcher runs it, never horizon-mixins.

API 47 adds `native_entity_join_level(dimension, entity, kind, pos, loaded_from_disk, canceled)`, an entity joining a
server level (NeoForge's `EntityJoinLevelEvent`, vanilla's `PersistentEntitySectionManager.addEntity`):
`loaded_from_disk` is true when it comes back with its chunk, false for a fresh add (a drop, an orb, `/summon`, an
arrival through a portal into another level). The entity is already in the level's lists when the hook runs; `canceled` takes it back
out, and a `/summon` of it fails with vanilla's "duplicate UUIDs" message. It tells every kind in the game's entity
lists (items, orbs, mobs, boats, minecarts, armor stands, primed TNT, falling blocks, projectiles, rockets, tridents),
once each, in creation order, so check `kind`. A drop, an orb, a mob, a `/summon` and a chunk's entities are told at
once; other gameplay spawns when the next tick starts, and a cancelled one is removed then. Bundles that use it need `"hooks": 47`.

API 48 adds [a block's items](#a-blocks-items) for server mods, what a pipe or a machine needs to pull from other blocks: `native_container_slots`, `native_extract_item`
and `native_move_items`, and `native_dimension` for a machine's tick to name its level. `native_insert_item` now goes one item at a time, so it no longer overfills a slot,
leaves a client's copy of the world alone, and can't reach a machine's own slots during its tick, where what went in was lost. Bundles that use the new helpers need `"hooks": 48`.

API 49 adds what a server mod needs for a public server's screens: `native_sidebar(uuid, sidebar)` shows one
player a sidebar of up to 15 lines (a text, and one where the score would be) that only their client holds (the
server's scoreboard never sees it; a mod sends it again when they rejoin), or takes it away with `None`;
`native_tab_list(uuid, header, footer)` sets the lines over and under their Tab list; `native_tick_stats()` reads the
server's tick rate and average tick time in ms (`/tick query`'s); and `native_sleep_ignored(uuid, ignored)` leaves a
player out of `players_sleeping_percentage`'s count until they are in bed (players away from the keyboard), outside a
player's hook only. The players `native_server_players` lists carry where they look (`yaw`, `pitch`). Bundles that
use them need `"hooks": 49`.

API 50 adds damage a mod can change or cancel through `native_living_incoming_damage` (NeoForge's
`LivingIncomingDamageEvent`): where `LivingEntity.hurtServer` starts, once its guards passed (invulnerable, dead or
dying, fire with fire resistance; Horizon's mobs don't check the fire resistance effect there yet), with the hurt
entity's id and type (`minecraft:player` for a player), the damage type (`minecraft:fall`), the source's causing
entity (`causing`, vanilla's `DamageSource.getEntity`: a skeleton for its arrow) and direct entity (`direct`,
`getDirectEntity`: the arrow; the attacker itself in melee), and the amount. The entity ids are 0 for none, and for
the sources listed below that Horizon doesn't track yet. A mod may change the amount, or set `canceled`: nothing of
`hurtServer` itself happens then (no damage, hurt sound, knockback, invulnerable time, statistics or criteria), and
the caller acts as on a refused hit, so a blast still pushes a mob whose hit was canceled, as vanilla pushes one that
refused it. A canceled hit on the ender dragon still counts as landed, as vanilla's `EnderDragon.hurt` returns true
whatever `reallyHurt` did: the arrow is spent and the melee effects run. A ghast's canceled hit by a fireball it was
sent back counts as landed (`Ghast.hurtServer`). A negative amount hurts by 0 and a NaN or infinite one by
`Float.MAX_VALUE`, as vanilla clamps them. It fires for the players, mobs and the ender dragon the game hurts. It has
no `self`: the game is in the middle of the hit. It runs on the thread that ticks the world (the server thread), so
state a mod shares with its render thread hooks needs synchronising. `/kill` and
`native_kill_mob` don't reach it yet: they set the health to 0 instead of hurting with `minecraft:generic_kill` as
vanilla does.

A player's damage type is the source's own. A mob's and the dragon's come from the kind of damage Horizon keeps for
the hit, which doesn't tell every vanilla type apart yet:

- A trident's hit on a mob is `minecraft:arrow`, not `minecraft:trident`
- A wither skull's hit on a mob is `minecraft:magic`, not `minecraft:wither_skull`
- Evoker fangs on a mob are `minecraft:magic`, not `minecraft:indirect_magic`
- A fireball nobody shot is `minecraft:fireball`, not `minecraft:unattributed_fireball`
- A shulker bullet's hit has no `minecraft:mob_projectile`
- A bed or respawn anchor blast on a mob is `minecraft:explosion`, not `minecraft:bad_respawn_point`
- A blast from TNT a player lit is `minecraft:explosion` on a mob, not `minecraft:player_explosion`
- A bed or respawn anchor blast on the dragon is `minecraft:explosion`, not `minecraft:bad_respawn_point`
- A blast a mob caused (an end crystal a skeleton shot, a creeper's) is `minecraft:explosion` on the dragon, not
  `minecraft:player_explosion`

The causing and direct entities are 0 for now in these cases:

- A player's direct entity, always
- A player's causing entity, except for a mob that shot the projectile, the mob `/damage` names, an evoker that summoned
  the fangs, a slime or magma cube that touched the player, and a mob that exploded (a sulfur cube's is guessed until
  its blast names its source)
- A mob's causing and direct entities when a player is behind the hit (melee, arrows, tridents, blasts, TNT a player
  lit)
- A mob's direct entity when it isn't the causing one (a projectile, a blast, a potion)
- A falling block on a mob, whose causing and direct entity is the block in vanilla
- The dragon's direct entity for a projectile or a blast
- The dragon's causing player when it isn't the acting one on a LAN guest's turn
- `/damage` on a mob `by` an entity with a damage type that isn't melee: the direct entity is 0, not that entity

And `/damage` on a mob `by` one entity `from` another with a melee damage type gives the `from` entity as the direct
one, where vanilla gives the `by` one.

Bundles that use it need `"hooks": 50`.

API 51 adds `native_menu_button_v2`: a mod container's menu button (`AbstractContainerMenu.clickMenuButton(Player, int)`) with the clicking player's cursor stack, read and written, so a widget outside the slots can take or give what the player carries (a tank taking a bucket). It fires right after `native_menu_button`, on the server, for single player and LAN guests alike. Bundles need "hooks": 51.

API 52 adds `native_block_entity_sync(pos)`, which sends the block and its update tag to clients again
(`Level.sendBlockUpdated`): with the level's block changes, to the players who see its chunk, once per position
however often it is called in a tick. A change made in a block entity's tick reaches clients at the start of the
next tick, as in vanilla. Call it when what clients show changes, not for every tick's progress: a menu's data slots
carry that. Bundles that use it need `"hooks": 52`.

API 53 adds `native_entity_leave_level(dimension, entity, kind)`, an entity leaving a server level (NeoForge's
`EntityLeaveLevelEvent`, vanilla's `onTrackingEnd`): removed for any reason or its chunk unloading, not the entities
still loaded when the server stops. It only hears. It tells item entities, experience orbs and mobs (`kind` is the mob type) so far; check `kind`.
Bundles that use it need `"hooks": 53`.

API 54 adds fluid tanks on a mod block's block entity, and the fluid helpers that read and change them, in millibuckets (a
bucket is a thousand). `native_block_entity_fluids` declares the tanks, as `native_block_entity_container` declares the slots: each tank's capacity, the
tanks each face reaches, and which a fill may reach and a drain may take; the tanks' contents live in the entity's saved compound under
`fluids`, so they survive restarts, and the block's own tick reads them there. `native_fluid_tanks` reads them through a side, `native_fluid_insert`
fills (first into tanks holding the same fluid, then into empty ones) and answers what did not fit, `native_fluid_extract` drains and answers what it
took; both also work a cauldron, which gives and takes whole thirds of a bucket and never more than was asked (less than its top third gives nothing), a lava one only whole, and powder snow none, as it is no fluid. Water and lava
(`minecraft:water`, `minecraft:lava`) are the fluids; a tank holds one at a time. Bundles that use them need `"hooks": 54`.

API 55 adds moving a block's fluids to another block. `native_move_fluids(dimension, from, from_face, to, to_face, max)`
moves up to `max` millibuckets between two blocks, one fluid, and what the target won't take goes back into the
source, past the fill sides' leave: the source's tanks just gave the fluid up, so they have the room, and a
cauldron levels back up by the thirds it gave, which a drain hands out whole. None where `native_fluid_tanks`
is none at the source, where nothing drainable is, where the target holds no fluids, and within one block.
Bundles that use it need "hooks": 55.

API 56 adds what the mob effects of a content mod ([mods.md](mods.md)) do, named after vanilla's `MobEffect` methods,
or NeoForge's event where it has one. On the server, each tick a player or a mob has one,
`native_should_apply_effect_tick_this_tick` says whether it acts (vanilla's base answer is no) and
`native_apply_effect_tick` acts, and may end it, before it counts down, as in `MobEffectInstance.tickServer`. These
two take `&self`: during a mob's tick the mob world borrows the game, so `&mut self` would alias that borrow, and the
game doesn't hold the mobs being ticked, nor the player's effects. `native_on_effect_added` hears it given to an
entity that didn't have it, and `native_mob_effect_added` (NeoForge's `MobEffectEvent.Added`) every time it is given.
A mob is its `Entity.getId`; a player is None, with their state swapped in (`native_acting_player`). Vanilla's effects
never reach them. Bundles that use them need `"hooks": 56`.

API 57 adds chunks and players coming and going, on the server: `native_chunk_load` (with whether the chunk is new), `native_chunk_unload`, and
`native_player_logged_in` and `native_player_logged_out`, which come in pairs however a player leaves, the server stopping included. Bundles that use them need `"hooks": 57`.

API 58 adds [the renderer's hook points](#render-hook-points): `native_world_drawn`, called once the world is drawn with its colour, depth, matrices and entity geometry, and
`native_render_needs`, where a mod asks for the entity runs and the world's depth without a shader pack. Bundles that use them need `"hooks": 58`.

API 59 adds [entities](#entities) for server mods: `native_summon(dimension, kind, pos, nbt)` spawns any entity from its compound
as `/summon` does, and `native_move_entity`, `native_set_display` (a display's transformation, eased over ticks) and
`native_remove_entity` move, pose and remove displays, markers and interactions: models that move, from a mod's own
item models. Bundles that use them need `"hooks": 59`.

`native_summon_with_uuid(dimension, kind, pos, nbt, uuid)` is `native_summon` with a UUID the mod picks for the root (the
entity answers to it in `native_entity_by_uuid` afterwards); nothing is spawned, and None comes back, when an entity of any
loaded level or a server's player already holds it or a passenger's `UUID` in `nbt` is taken (`/summon`'s "duplicate
UUIDs"). Passengers in `nbt` are spawned and mounted as in `/summon`; mount and dismount are `native_mount`,
`native_dismount` and `native_eject_passengers`, and an item frame's `ItemRotation` and a hanging entity's `Facing` change
through `native_entity_merge_nbt` (`/data merge entity`, which refuses players). Bundles need
`"hooks": @API:summon_with_uuid@`.

API 60 adds `native_item_bar(stack, bar)`: the small bar `GuiGraphicsExtractor.itemDecorations` draws under an item,
wherever the game shows one. `bar` arrives holding
vanilla's choice (the damage bar, or a bundle's fullness, already applied); set it to a width `0..=13` and an
`rgb` colour, or to `None` to hide it. Asked each frame a stack is drawn. Client-side (NeoForge has no event for
this; vanilla's `Item.isBarVisible`/`getBarWidth`/`getBarColor`). Bundles that use it need `"hooks": 60`.

API 61 adds mods' own sound events in `native_play_sound`: an event of another namespace than `minecraft` (`mymod:machine_hum`) is sent to players by
name, as vanilla sends a sound that isn't in its registry, and they hear it when their resources define it (a content mod's `assets/mymod/sounds.json`
and its `.ogg` files). `minecraft:` events must still be registered. Bundles that play their own sounds need `"hooks": 61`.

API 62 adds nicknames for server mods: `native_set_nickname(uuid, Option<&str>)` shows a name in place of a player's in chat (their messages, `/me`, `/msg`) and the Tab list, the real name on hover and everywhere else; `native_text_allowed(text)` asks the server's chat filter. Both outside a player's hook. Bundles need "hooks": 62.

API 63 adds context-free block and item tag lookups from server hooks (`native_server_block_in_tag`, `native_server_item_in_tag`). They return `NotServer` on a client and `TagsUnavailable` before the world loads its tags. An unknown tag returns false; an empty stack tests `minecraft:air`. Bundles need "hooks": 63.

API 64 adds a mod block's comparator output, as vanilla's `BlockBehaviour.hasAnalogOutputSignal` and `getAnalogOutputSignal`: `native_has_analog_output` says a block of yours has one,
and `native_analog_output` gives it (0 to 15) for its state and position. A state change with neighbour updates tells the comparators itself; when the output
changes otherwise, `native_update_output_neighbors(dimension, pos)` makes them read it again (`Level.updateNeighbourForOutputSignal`). Bundles need `"hooks": 64`.

API 65 adds `native_component_types(types)` runs once before content loads. Append namespaced component IDs; duplicates, invalid IDs and the `minecraft` namespace stop startup. `native_component`, `native_set_component` and `native_remove_component` read, set and remove registered components, including item defaults. Values use NBT and must fit the network reader limits. Bundles that use them need `"hooks": 65`.

API 66 adds [explosions](#explosions) for server mods: `native_explode(dimension, center, radius, fire, blast)` sets one off as
`Level.explode` does, with `NativeBlast::Keep`, `Block` or `Tnt` for what it does to blocks. Bundles that use it need
`"hooks": 66`.

API 67 adds dispenser item behaviours (`native_dispense_item`): a server hook receives the dimension, dispenser position, facing and selected stack, before any vanilla behaviour, for mod or vanilla items. Leave its answer `None` for vanilla, or answer `Some((remainder, event))`: a `None` remainder empties the slot, `Some(stack)` writes that remainder without consuming again, and `event` is the level event played with the smoke (1000 click, 1001 fail, 0 nothing, as `NOOP`). Answer `Some((native_dispense_default(dimension, pos, facing, stack.clone()), 1000))` to drop one item normally, including items with a special vanilla behaviour. Droppers are unaffected. Bundles need "hooks": 67.

API 68 adds image tooltip components: `native_tooltip_component(stack, size)` answers finite positive width/height in GUI pixels, and `native_tooltip_component_draw(stack, batch, origin, size)` draws that component with `native_sprite`, `native_fill` and `native_text`. The stack supplies its data. Space and background are reserved after the text and any vanilla bundle component; the component width participates in tooltip positioning. Neither hook changes ordinary tooltips without an answer. Client-side. Bundles need "hooks": 68.

API 69 adds [advancement triggers](#advancement-triggers) of a mod's own: `native_trigger_criterion(uuid, trigger, test)` fires a trigger
an advancement of a content mod or a data pack names (`mymod:pipe_connected`) for one player, as a NeoForge mod's
`SimpleCriterionTrigger.trigger`. Bundles that use it need `"hooks": 69`.

API 70 adds where a mod block can stay, on an authoritative server. `native_can_survive(dimension, pos, state, survives)` answers
`BlockState.canSurvive` before the block does: as it is placed (`BlockItem.canPlace`: `Some(false)` refuses the spot),
from the shape update of a block whose template checks it (a plant, a torch, a sign: `Some(false)` breaks it with its
drops when a neighbour changes), and wherever else vanilla asks. `state` may not be in the world yet, so decide from
`state`. `native_may_place_on(dimension, state, ground, ground_pos, may)` answers `VegetationBlock.mayPlaceOn` for a
mod plant (a block whose template is a vanilla plant, such as a crop): the blocks it can stand on, which its
template's `canSurvive` asks, so it is refused elsewhere and pops off when its ground changes. Answer only for your
own blocks and leave the answer empty otherwise; the level is read-only. Both run on every shape update of such a
block: keep them short. Bundles that use them need
`"hooks": 70`.

API 71 adds drawing mod block entities on the client. `native_block_entity_submit(pos, block, state, entity, draw)` runs once a frame
for each mod block entity in view, as vanilla's `BlockEntityRenderer.submit` does (within 64 blocks, in the frustum,
its chunk faded in), with the copy the server sent (`native_block_entity_update_tag`). It draws into the entity batch
under `draw`'s pose stack (`push`, `pop`, `translate`, `rotate_about`, `scale`, in blocks from the block's corner, as
vanilla's `PoseStack`): `native_draw_quad` (four corners and a sprite's UVs), `native_draw_cube` (a model element's box,
its faces baked as a model's are; `CubeFace` is in `super::native_block_entity_draw`) and `native_draw_block` (a block
state's model, as `submitBlockModel`: a moving part is often a block of the mod's own). Sprites are the atlas's:
`<ns>:block/...` and `block/...`, and a content mod's `assets/<ns>/textures/entity/` as `<ns>:entity/...`. Culling uses the bounds
`native_block_entity_render_bounding_box` gives (NeoForge's `getRenderBoundingBox`): the block with a block of margin
unless the mod widens them, or never culled when a bound is infinite. Both run every frame for every one in view: keep
them cheap. Bundles that use them need `"hooks": 71`.

API 72 adds bone meal on a mod block, the `BonemealableBlock` three questions a block nothing in Horizon knows one for
answers itself: `native_is_valid_bonemeal_target(state, valid)` says whether the block takes bone meal (`Some(false)` refuses it even where its template takes it, `None` leaves the
template's; asked on both sides, as vanilla asks it, so a client predicts the use); `native_is_bonemeal_success(state, success)` is
`isBonemealSuccess`, drawn from `native_level_random()` to keep vanilla's order; and `native_perform_bonemeal(state)`
is the growth itself, with `native_set_block`/`native_set_block_property`. The meal is used up and the growth
particles and sound follow as vanilla's do, whether or not the roll grows the block, from a player's bone meal, a
dispenser's and, on a crop, a villager's. A block that answers nothing keeps the `BonemealableBlock` its template gives it.
Authoritative servers only, except the first question. Bundles that use it need `"hooks": 72`.

API 73 adds mod-block entity contacts: `native_block_step_on`, `native_block_entity_inside` and `native_block_fall_on`. Each receives the server dimension, entity id/type, block state and position. The landing hook also receives fall distance and mutable calculated damage; zero prevents fall hurt and its generic fall sounds. Creative and fall-immune entities notify with zero and retain immunity. Invalid non-finite damage changes are ignored; negative damage is clamped to zero. Hooks run synchronously without `self`, like `native_living_incoming_damage`. Bundles need "hooks": 73.

API 74 adds a consumable's effect of a mod's own kind. The item's `on_consume_effects` entry of a type in a namespace of its own — where the old loader dropped it silently — reaches `native_consume_effect(kind, data)` when the
item is consumed, on the server thread, for the acting player. The kind and the JSON the file wrote are there;
the game's own effects, sounds, teleports and drops are open to the hook. An unclaimed kind does nothing.
The hook sees the item's own consumable component, not a patch on the stack; a bare vanilla name and an unknown `minecraft:` kind are the loader's, not the hook's. Bundles that use it need "hooks": 74.

API 75 adds [loot table hooks](#loot-tables), for changing loot a mod didn't make. `native_loot_table_load(id, table)`, in `horizon-world`'s `loot.rs`, gets each loot table's JSON as it loads, after the data packs' (NeoForge's `LootTableLoadEvent`, Fabric's `LootTableEvents.MODIFY`): add pools or entries, change or replace it, and chests, block and mob drops, fishing and the rest then roll the changed table. It runs wherever tables load, a client too. `native_loot_drops(context, drops)` gets the items a broken block or a dead mob is about to drop, to add, change or take out (global loot modifiers, `MODIFY_DROPS`), with `NativeLootDrops`: the table rolled, the dimension, the origin, the block or the mob, the player who broke or killed it, the tool and an explosion's radius. It hears blocks a player, an explosion or a mob breaks, not other breaks such as leaves decaying. Bundles need `"hooks": 75`.

API 76 adds `native_state_for_placement(state, context, fluid, answer)`: the state a block is about to be placed in
(`Block.getStateForPlacement(BlockPlaceContext)`), `state` the block's own default state. Answer only for your own
blocks: `answer` set to `Some(Some(state))` places that state (a state of that block, as vanilla sets properties on its own
`defaultBlockState`; another block's is ignored), `Some(None)` refuses the placement (vanilla's `null`:
the block goes nowhere and the stack is kept), and left empty the block's own placement runs. `context` is vanilla's
`BlockPlaceContext`: the clicked face, the placer's rotation, the clicked position, whether the clicked block is
being replaced and the item in hand. `fluid` is the fluid in the position the block would go in, which a
waterlogged block reads. It is asked for every placer that goes through vanilla's placement (a player's
`BlockItem.place`, a dispenser's `DirectionalPlaceContext`), and the answer still takes its survival and entity
obstruction checks. `native_state_with_property(state, key, value)` builds the state from `state` and one of its
properties, its own declared ones included. Authoritative servers only. Bundles that use it need `"hooks": 76`.

API 77 adds a `minecraft:post_attack` enchantment effect of a mod's own kind, on a mob a player's weapon hit: `native_enchant_effect` (see
[A mod's own enchantment effect kinds](#a-mods-own-enchantment-effect-kinds)). Bundles need "hooks": 77.

API 78 adds `native_screen_render_post`, the point of `ScreenEvent.Render.Post`: a native mod draws over the open screen after it and its tooltips, given its id, the server-opened menu's vanilla type (`menu`) and title, its container panel, its slot count, the mouse in GUI pixels and the partial tick (`NativeScreenView`). Every container, station and mod window draws on the inventory screen and so reports `inventory`: tell them apart by `menu` and `title` (None on a window the client opened) and by `slots`. It does not fire on the title screen, nor for the prompts, books and sign editors drawn over gameplay after it. Bundles that use it need "hooks": 78.

API 79 adds `native_modify_default_components(item, components)`, in `horizon-world`'s `item_defaults.rs`: an item's default
components in `items.json`'s form, for any item, vanilla's included (NeoForge's `ModifyDefaultComponentsEvent`). Insert
or replace a component to set it (`"minecraft:max_stack_size": 16`), remove it to take it away. The item table,
loot, enchanting and item models all read the changed defaults, and stacks are saved and sent against them as
against vanilla's. A `minecraft:max_stack_size` above vanilla's 99 is kept, up to `i32::MAX`. Asked for every item
each time the game reads the defaults: answer the same each time. Client and
server each ask their own mods, so both need the mod. Bundles that use it need `"hooks": 79`.

API 80 adds the open screen hears of a mouse button going down (`native_screen_mouse_button_pressed_pre`) or up (`native_screen_mouse_button_released_pre`) before it acts, as vanilla's `ScreenEvent.MouseButtonPressed.Pre` and `.MouseButtonReleased.Pre`: the screen, the GLFW button (0 left, 1 right, 2 middle, 3 back, 4 forward; a button past those is the platform's own number, which differs between systems) and GLFW's modifier bits (shift 1, control 2, alt 4, super 8), with the pointer in `screen.mouse`. A cancelled press goes no further (no slot, screen button, drag or hotbar key); cancel a release only when its press was cancelled, or a drag it ended stays held. They fire on the window only, never on the title screen itself (Options and the pack list opened from it do fire), the connect and loading screens, the prompts, books and sign editors drawn over gameplay, or while a world loads or saves, and never for a gamepad or keyboard-focus click. Bundles need "hooks": 80.

API 81 adds [falling blocks](#falling-blocks) for server mods: `native_spawn_falling_block(dimension, state, pos, velocity, falling)`
throws a block that flies, lands and places itself as sand does, for debris and collapsing structures. Bundles that
use it need `"hooks": 81`.

API 82 adds camera shake for client mods: `native_camera_shake(partial, shake)` asks each frame for turns in degrees and an
offset in blocks, which add up across mods and multiply into the view after its bobbing. Horizon scales the sum by
Distortion Effects and bounds it to 10 degrees and half a block on each axis. Bundles that use it need
`"hooks": 82`.

API 83 adds client sounds that wait: `native_play_local_sound(sound, at, volume, pitch, delay)` plays a sound event, vanilla's or
the mod's own from its `assets/<id>/sounds.json`, for this player only and up to a minute later, so a client mod
can time a blast's sound to its shock wave. Bundles that use it need `"hooks": 83`.

API 84 adds values for a mod's own shader pack: `native_shader_params(partial, params)` asks each frame for eight `vec4`s that
every stage of a Horizon shader pack reads as `frame.mods`, so a mod can tell the pack it bundles where and when
something happened (an explosion's centre and age for a cloud drawn in a post pass). Bundles that use it need
`"hooks": 84`.

API 85 adds menus of content mods' own menu types (vanilla's `MenuType`): a mod container's `menu` may name a menu type its content mod declares in `data/<id>/horizon/menu/` with as many slots ([mods.md](mods.md#menu-types)). It opens when the block is used, its data slots are the block entity's `data`, and a Horizon client shows it through the mod's `native_screen`. Bundles that use it need `"hooks": 85`.

API 86 adds `native_block_highlight(pos, state, partial, canceled)` runs before building the eligible targeted block's selection outline, after hidden-HUD and game-mode restrictions. Set `canceled` to suppress only that outline; default behavior is unchanged otherwise. Position is block coordinates and state is the current block state ID. No entity outlines are affected. Bundles need "hooks": 86.

API 87 adds `native_render_hand(hand, stack, partial, swing, equip, canceled)` runs before each visible first-person hand: `Hand::Main` or `Hand::Off`, the displayed stack (None for an empty arm), partial tick, swing progress and inverse equip height including swap animation scale. Set `canceled` to skip the default geometry for that hand (arms and maps included). It runs after the existing spectator, hidden-HUD, sleep and camera gates. It only hides: the hook has no way to draw a replacement. Bundles need "hooks": 87.

`native_render_hand_pose` runs after cancellation for each visible first-person hand, including maps and the empty main-hand arm (the same path as `RenderArmEvent`). Add to `x`, `y`, `z` in blocks and `pitch`, `yaw`, `roll` in degrees, and multiply `scale` to compose with other mods. The pose is applied in hand view space before the ordinary hand and item poses, in translation, pitch, yaw, roll, scale order. Non-finite or out-of-range components are ignored individually: offsets must be within -16..=16 blocks, angles within -3600..=3600 degrees and scale within 0.01..=100. Bundles need "hooks": 369.

`native_screen_opening(screen)`, `native_screen_init_post(screen)` and `native_screen_closing(screen)` observe screens once per frame with `NativeScreenView`, including sign editors, book edit/sign/view and lectern screens, dialogs and prompts. Overlay views use the full window as their rectangle and have no menu or title; anvil rename remains part of its inventory screen. Dialog states share the `dialog` id. Screens opened and closed within one frame are missed; cancelling an opening is not possible because there is no single transition point. Bundles need "hooks": 367.

API 88 adds `native_client_resources_reloaded()` fires on the render thread after successful client resource-pack application: the atlas, models, renderer and client caches already use the new resources. It covers selection/F3+T and server-pack reloads. Failed staging or application and initial startup do not fire it. This is a completion notification, not a reload-listener registration or staging API. Bundles need "hooks": 88.

API 89 adds `native_item_decorations(stack, x, y, batch)` appends custom GUI drawing after the default bar, cooldown shade and count have been painted for an item icon. The stack is the displayed stack, x/y are its icon's top-left in GUI units, the units the batch takes (no item-local translation remains active). Existing GUI helpers can draw badges, text and overlays. The shared path covers the hotbar, carried stacks, menu slots, recipe book and item tooltip icons, and the item icons of the advancement, beacon, stats and trading screens. `native_item_bar` still controls the bar separately; this hook does not suppress vanilla decorations. Bundles need "hooks": 89.

API 90 adds using an item over time, as vanilla's `Item.getUseDuration`, `getUseAnimation` and `finishUsingItem` and NeoForge's `LivingEntityUseItemEvent.Start` and `.Finish`. `native_use_duration` answers a stack's use time: a mod's own item with no use of vanilla's that answers above 0 is used over time on a right click, until it is let go or the time runs out, on single player and LAN alike. `native_use_animation` picks the first person pose of a use (eat, drink, bow, trident, brush), and `native_can_eat` decides whether a food can be eaten now. `native_living_entity_use_item_start` may change a use's duration or refuse it, on the server and where the window's client predicts the use, and on the server `native_living_entity_use_item_finish` may replace what the hand holds once the time ran out (consumables, potions and mods' items). Both take the using entity, None for a player, so mobs can fire them later, and the hand in use. Change the world only where this game runs the world, not on a server's client. Bundles that use them need `"hooks": 90`.

API 91 adds items and block state properties for drawing mod block entities: in `native_block_entity_submit`, `native_draw_item(draw,
stack, display, owner_yaw, seed)` draws an item stack under `draw`'s pose as a campfire's or a shelf's
(`ItemModelResolver.updateForTopItem` and `ItemStackRenderState.submit`, in the model's `display` transform; with `owner_yaw`, the block
entity is the item's owner, as a shelf is, so a compass on it points from there). The client's copy of a block entity has
no item slots: send the stacks you draw in `native_block_entity_update_tag` and read them from `entity.nbt`.
`native_block_state_property(state, key)` reads the block's
`facing` from the hook's `state` without a world lookup. Bundles that use them need `"hooks": 91`.

API 92 adds the world's clock for client mods: `native_clock()` is the shown world's game time and time of day in ticks (`i64`; the day time counts on past 24000), none on the title screen. Bundles need `"hooks": 92`.

API 93 adds the open screen hears of the wheel (`native_screen_mouse_scrolled_pre`) before it scrolls, as vanilla's `ScreenEvent.MouseScrolled.Pre`: the screen, the scroll's x and y as the game's scroll handler scales them (a line times the wheel sensitivity: with Discrete Scrolling on, a notch is ±1, its sign only; a touchpad's pixels are divided by 30), GLFW's modifier bits (shift 1, control 2, alt 4, super 8) and the pointer in `screen.mouse`. A cancelled turn scrolls nothing: no bundle under the pointer, no list, slider or scrollbar, and no hotbar slot. It fires for the window's wheel (horizon-ctl's `scroll-at` included), never on the title screen itself (Options and the pack list opened from it do fire), the connect and loading screens, the prompts, books and sign editors drawn over gameplay, or while a world loads or saves, and never for a wheel a gamepad or the keyboard's focus sends. Bundles need "hooks": 93.

API 94 adds keeping a mod block entity's geometry: `draw.keep()` in `native_block_entity_submit` keeps what that call drew, and the
block is drawn the same again without calling `native_block_entity_submit` or `native_block_entity_render_bounding_box`
until its state, its block entity (what the server sent), its light or the resource packs change, so idle machines cost
no call into the mod each frame. Keep only geometry that reads none of `partial`, `game_time`, the blocks around it or other state of yours:
kept geometry stands still (an enchanted item's glint, a clock or compass drawn with `native_draw_item`).
Bundles that use it need `"hooks": 94`.

API 95 adds any command a player types: `native_player_command(player, name, command, cancel, replies)` is heard before the command is looked at (NeoForge's `CommandEvent`), without its slash, for vanilla's commands, the mods' own and commands the player may not run, once for each command a player sends, on a server. Also each line of a quiet command batch. A mod sets `canceled` to stop it: nothing runs and nothing is answered but the lines it pushes to `replies`, which reach the player as system chat. Bundles that use it need `"hooks": 95`.

API 96 adds `native_neighbor_signal(dimension, pos)`, the strongest redstone signal reaching a block from its neighbours (`SignalGetter.getBestNeighborSignal`, 0 to 15), as a dispenser or a piston reads it: what a mod's explosive, launcher or machine needs to go off or switch on from its block entity's tick. 0 on a client and for another dimension. Bundles that use it need "hooks": 96.

API 97 adds `native_light(pos)` (block and sky light, 0 to 15, both 0 where the chunk isn't loaded), `native_weather()` (rain and thunder levels, 0 in the Nether and the End), `native_day_time()`, `native_world_height()` (min y and height) and `native_max_stack_size(item)`: what a mod's solar panel, wind turbine, quarry or own container needs without reaching into Horizon's internals. Bundles that use them need "hooks": 97.

API 98 adds `native_give_stack(uuid, stack)`: gives a whole stack (damage, name, enchantments kept) to any player from a server hook, as `Inventory.add` places it, and returns what did not fit. Bundles need "hooks": 98.

API 99 adds a mod's command typed on the dedicated server's console: `native_perform_console_command(command, ran)` hears a command a mod registered with `native_register_commands`, without its slash, from the console (permission level 4, no player: `native_acting_player` is None), and a mod sets `ran`. `native_feedback` answers on the console. A name no mod registered goes to vanilla's dispatcher as before; the single player owner's console is not covered. Bundles that use it need `"hooks": 99`.

API 100 adds player-use drops to `native_loot_drops`: shears on a sheep, a mooshroom, a snow golem or a bogged, a brush on an
armadillo or a suspicious block, carving a pumpkin, picking sweet berries or cave vines, and a player taking an item frame or
its item now pass through it too, with `NativeLootDrops::used` set and the player named. Bundles that read `used` need
`"hooks": 100`.

API 101 adds `native_experience_orbs(dimension, min, max)`, the experience orbs in a box; `native_give_experience(uuid, points)`, points given to any player as an orb they touch gives them (Mending first); and `native_give_levels(uuid, levels)`, levels given or, negative, taken. Bundles need "hooks": 101.

API 102 adds lead drops to `native_loot_drops`: untying a lead or cutting it with shears (`used`), and the lead a mob a
player killed while leashed drops on its next tick, naming the killer. Bundles that rely on them need
`"hooks": 102`.

API 103 adds native mod particle types to `native_send_particles`: `ParticleOptions::named("mymod:spark")` sends a particle of the mod's own type, drawn by Horizon clients from the sprite set in the mod's `assets/<namespace>/particles/<path>.json` (`"textures": ["mymod:spark/1", ...]`, resolved as `particle/<texture>` in the atlas). The name must be namespaced; a client without that file draws nothing for it, vanilla clients (whose brand isn't Horizon's) aren't sent it, and the call still returns true. The type behaves as vanilla's `SimpleParticleType` with a sprite set: a plain billboard, no tint, 20 to 40 ticks, its motion the game's default spread (the speed sets its direction, not how fast it goes). Bundles that use it need `"hooks": 103`.

A mod particle may opt into declarative behaviour in that same particle JSON (no new hook API):

```json
{
  "textures": ["mymod:spark"],
  "horizon:behaviour": {
    "velocity": [0.0, 0.15, 0.0],
    "velocity_jitter": [0.02, 0.0, 0.02],
    "animation": "loop",
    "frame_ticks": 3,
    "gravity": 0.5,
    "friction": 0.96,
    "lifetime": 40,
    "size": [0.1, 0.0],
    "alpha": [1.0, 0.0]
  }
}
```

`animation` chooses how the particle walks through its sprites: `age` (first to last over its
lifetime), `loop` (cycles every `frame_ticks` ticks, 1..200, only valid with `loop`), `random` (one random
sprite, kept) or `first` (the first sprite, kept). Without `animation` the particle keeps the sprite it was
spawned with, as before.

Every field is optional. Omit `horizon:behaviour` (or use `{}`) to keep the existing behaviour,
including its random draws, unchanged. `velocity` replaces the initial randomized velocity with
an exact vector in blocks/tick, plus the velocity the spawn call passed; omit it to retain the spawn's default spread. `velocity_jitter` adds a random
offset per axis, uniform in `-j..=j` (0..16 each, drawn from the particle's seeded generator, so the same
seed gives the same particle); declaring it alone starts from zero plus the spawn velocity. Each component must
be finite and within -16..16. `gravity` is the vanilla multiplier: each tick subtracts
`0.04 * gravity` from vertical velocity before moving (negative rises; range -100..100).
After movement, `friction` multiplies all velocity components (0..1); normal block collisions
and the vanilla on-ground horizontal multiplier of 0.7 still apply. Omitted gravity remains 0,
omitted friction remains the engine default (0.98), and omitted lifetime remains 20..39 ticks.
`lifetime` is an integer from 1 to 12000; removal uses the existing base tick: it moves that many
times and is removed on the following tick.

`size` and `alpha` are `[start, end]` linear curves over `age / lifetime`, clamped to their
endpoints. Size is billboard half-width/half-height in blocks (0..16), not a multiplier;
alpha is opacity (0..1). The start value applies immediately at spawn, then the curves update
once per tick, reaching the end after `lifetime` motion ticks. Omitted size keeps the random
spawn size; omitted alpha stays 1. Fading particles use the blended particle layer.
Malformed declarations, unknown behaviour fields, non-finite or out-of-range numbers reject the
whole behaviour object with a log warning, falling back to the existing particle behaviour;
the sprite set still loads. Vanilla particle JSONs are unaffected. Resource reloads apply new
declarations to newly spawned particles; live particles keep their spawn-time values.
The engine copies fixed-size numeric data at spawn and performs no new allocations per particle
tick. This first slice supports only linear size/opacity curves and constant gravity/friction;
there is no per-particle code hook.


API 104 adds the open screen hears of a key press (`native_screen_key_pressed_pre`) and of each character it types (`native_screen_char_typed_pre`), as vanilla's `ScreenEvent.KeyPressed.Pre` and `.CharacterTyped.Pre`: the screen, vanilla's key name (`key.keyboard.e`, as `native_key` hands it over; a key with no name fires nothing), whether the press is an auto-repeat, the character, GLFW's modifier bits (shift 1, control 2, alt 4, super 8), and `screen.mouse` for the pointer. A cancelled press goes no further — no focus move, no Escape-close, no inventory or hotbar key, no typing by the key itself, no `native_key` — while its release still arrives, so a `native_key` listener sees that release without its press, and the characters no mod cancelled still reach the text box, as vanilla's separate `charTyped` comes for a cancelled `keyPressed` too. A cancelled character drops only that character and never reaches a text box; control characters (Enter, Backspace, Tab, Delete, Escape, a Control letter) are keys and fire nothing. A key's characters fire after the key has acted, as GLFW's char callback comes after its key callback, so a key that closes the screen (the inventory key) reports no character (the closed screen hears none) and one that moves the focus types into the box it moved to. The input method's committed text fires the character hook too. They fire for the window and the control socket's `key` and `type` (not its `\b` backspace), after the fullscreen, screenshot, debug and narrator keys, never on the title screen itself (Options and the pack list opened from it do fire), the connect and loading screens, the prompts, books and sign editors drawn over gameplay, or while a world loads or saves. Bundles need "hooks": 104.

API 105 adds `native_push_player([x, y, z])` on a client, added to the local player's velocity as an explosion's knockback is, and `native_set_flying_speed(player, speed)` for server mods, a player's creative flying speed, kept across respawns and game mode changes and saved with the player as vanilla's `abilities.flySpeed` is. Bundles need `"hooks": 105`.

API 106 adds `native_data_pack_files(files)`, a native mod's own pack files in the data the server loads: while the tables are assembled (a world's first open, and again on every `/reload`), push `(path, bytes)` and they join that load. `path` is the overlay's key, `<kind>/<namespace>:<path>` without `.json`, over the kinds the tables read: `recipe/mymod:thing`, `tags/mymod:block/thing`, `loot_table/…`, `advancement/…`, `trade_set/…`, `villager_trade/…`, `dialog/…`; a file that isn't JSON is skipped with a warning. Asked once per load on both paths, so it is where a mod recomputes them from its config or the world. Contributed files sit below the world's own data packs: a pack's file for the same path still overrides a mod's, and a later contribution overrides an earlier one. Bundles that use it need "hooks": 106.

API 107 adds biome colours for client mods, for seasons: `native_biome_colors(biome, colors)` changes a biome's grass, foliage, dry
foliage and water tints, asked for every biome when a mod calls `native_refresh_biome_colors()`, which rebuilds the
loaded terrain with them. Bundles that use it need `"hooks": 107`.

API 108 adds `native_server_data_loaded(reload)` fires once the server's data tables are built (NeoForge's `TagsUpdatedEvent` with `SERVER_DATA_LOAD`): the recipes, tags, loot and advancements of the world's data packs and of `native_data_pack_files` are what the helpers read from then on. `reload` is false on a world's first open, before its chunks and players are loaded, and true after `/reload`, once the players were sent the new data. It is where a mod drops caches derived from the data and rebinds what depends on it. Server-side, the single player's integrated server included; a client connected to a server never fires it. Bundles need "hooks": 108.

API 109 adds `native_compute_fov(partial, camera, degrees)` receives the computed world FOV after settings, ordinary FOV effects and zoom, before projection/Nausea. `camera` is its world position and `partial` the interpolation fraction. Mods can narrow or widen the angle; only finite values in 1..=179 degrees are accepted, otherwise the original computed angle is used. It runs for world rendering and near-plane overlays, not just once per frame. Hand/HUD FOV and the title panorama keep their existing paths. Bundles need "hooks": 109.

`native_movement_input(forward, strafe, jump, sneak)` runs on the client each local player tick after input is read and before it is applied. Mods may change all four values; positive strafe is left. The server receives direction flags, not scalar magnitudes, so remote servers may correct modified movement. Bundles need "hooks": 360.

API 110 adds mobs drawn by client mods: `native_mob_submit(mob, draw, drawn)` is asked each frame for every mob in view but players,
with its id, kind, custom name and the frame's body and head turns and walk cycle; a mod draws a model of its own with the
block entity drawing helpers under `draw`'s pose (at the feet, turned with the body) and sets `drawn` to leave out the vanilla
model. Bundles that use it need `"hooks": 110`.

API 111 adds a screen of a client mod's own: `native_mod_screen_open(mod_id)` opens one over the world (every mod's own screen has the `NativeScreenView.id` `mod`: check `native_mod_screen()` is yours; it doesn't pause a single-player game; it opens only on the window's own client, over the world with no overlay up; a gamepad can close it but not click on it), the screen hooks fire over it as over any other, so the mod draws it in `native_screen_render_post` and takes its clicks, scroll, keys and typing from them, and Escape closes it (`native_mod_screen()` says whose is open, `native_mod_screen_close(mod_id)` closes it); and two drawing helpers beside `native_fill`: `native_quad(batch, points, argb)` and `native_item_model(batch, item, model, at, size)`, an icon drawn with another `item_model`. Bundles need `"hooks": 111`.

API 112 adds `native_camera_distance(partial, mirrored, entity, distance)` receives the final scaled maximum distance for a third-person camera, before the existing eight visual-shape collision rays shorten it. `entity` is the camera entity view, including optional living scale/distance and mount scale/distance; this covers player and spectator-mob cameras. `mirrored` identifies front-facing third person. Only finite values in 0..=1024 blocks are accepted, keeping mod requests within the collision-ray budget; other values retain the original distance. First-person views and title panoramas do not fire it. Bundles need "hooks": 112.

API 113 adds `native_compute_fog_color(partial, camera, fog_type, rgb)` adjusts the final world fog RGB after normal biome, weather and effect calculation and before GPU upload. Camera is its world position; fog_type distinguishes atmosphere, water, lava and powder snow. Only finite components in 0..=1 are accepted; invalid requests retain the original RGB. Alpha and all distance fields are unchanged. Title panoramas skip this hook. Bundles need "hooks": 113.

API 114 adds `native_mouse_button_pre(button, pressed, modifiers, canceled)` sees real button events on any screen, before normal dispatch. Button and modifiers use the screen hooks' GLFW encoding. Physical button bookkeeping already ran. Cancellation skips normal dispatch and Post. Accepted events call `native_mouse_button_post(button, pressed, modifiers)` after dispatch, including screen handlers' early returns. Synthetic pad/focus clicks skip both hooks. Existing screen-specific hooks and native_key notifications remain. Bundles need "hooks": 114.

API 115 adds a player respawning: `native_player_respawned(player, name)` is heard once at the end of `PlayerList.respawn`, with the player placed and their health, food and experience reset (NeoForge's `PlayerRespawnEvent`), on a server, with their state swapped in so the helpers reach them. Bundles that use it need `"hooks": 115`.

API 116 adds a client mod's view and items: `native_raycast(distance)` and `native_view_hit()` (the first block along the view, up to 1024 blocks, across the distant terrain past the loaded chunks), and `native_claim_items(mod_id, ids)`: a right-click with one of those items is the mod's alone, the server never hears of it, and `native_poll(mod_id)` gives `UseStart` and `UseStop` with the stack (its custom name and data as SNBT) and the ticks it was held. Bundles need `"hooks": 116`.

API 117 adds `native_render_fog(partial, camera, fog_type, fog)` adjusts final FogData after normal environment/effect/render-distance calculation and Horizon's fog option, before GPU upload. Title panoramas skip it. The whole request is accepted only when color components are finite in 0..=1, all distances are finite, environmental and render-distance ends exceed their starts, and sky/cloud ends are nonnegative. Negative near planes remain valid (water/lava). Invalid requests retain the original whole fog. Bundles need "hooks": 117.

API 118 adds `native_screen_key_released_pre(screen, key, modifiers, canceled)` runs before a named key's release reaches an open screen, with the same screen exclusions and GLFW modifier encoding as the existing key-press hook. It does not fire for presses/repeats or unnamed keys. Cancel bypasses screen handling and its legacy native_key notification, but physical key removal and mapped keyboard/toggle releases still happen to prevent stuck inputs. Mouse mappings retain their existing gameplay-only gate. Bundles need "hooks": 118.

API 119 adds music for client mods: `native_music(music, volume)` changes the track the music manager plays and its volume each
tick, and `native_music_layers(layers)` lists looping tracks played over it, each easing to its own volume (stems that
grow fuller). Bundles that use them need `"hooks": 119`.

API 120 adds a server's recipes reach its Horizon clients, as NeoForge's `OnDatapackSyncEvent.sendRecipes` and `RecipesReceivedEvent`: on the server, `native_datapack_sync(recipe_types)` takes the recipe types (`minecraft:smelting`, `mymod:crusher`) a mod's client half needs, asked once per load, the same types for every player (NeoForge asks per joining player); Horizon sends their final recipes (data packs merged, load conditions held) to every Horizon client when it joins and after each `/reload`, in payloads within the clientbound cap, and nothing to a vanilla client. On the client the recipe helpers (`native_recipes`, `native_recipes_of_type`) then give the server's recipes of those types in place of the window's own, single player included, and `native_recipes_received(recipe_types)` says they came. No type asked: nothing is sent and nothing changes; none asked after a load that asked some: the clients go back to their own recipes and `native_recipes_received` comes with no types. Bundles need "hooks": 120.

API 121 adds `native_input_key(event, modifiers)` runs after a real window keyboard event finishes handling, including shortcuts, screen consumption and repeats. event is the original winit::event::KeyEvent with physical/logical key, location, text, state and repeat; modifiers use the screen hooks' GLFW bits, captured before handling. Not cancellable. Foreign-window and OS synthetic focus events skip it. Window callback only: pad/focus/control-socket input retains its existing native_key route. The legacy native_key hook is unchanged. Bundles need "hooks": 121.

API 122 adds `native_mouse_scrolling(scroll, accumulated, buttons, cursor, modifiers, canceled)` fires with a player in gameplay and no open screen/prompt/sleeping UI, once at least one whole scroll step accumulates and before hotbar/spectator handling. scroll is x/y after discrete/sensitivity scaling (touchpad pixels normalized by 30), accumulated is the whole x/y pair, buttons are physical left/middle/right, cursor is the GUI position, and modifiers use GLFW bits. Cancel skips gameplay scrolling; accumulated whole steps remain consumed. Synthetic input skips the new hook. The legacy vertical native_wheel still runs first and can consume the event; screen-scroll hooks are unchanged. Bundles need "hooks": 122.

API 123 adds `native_screen_mouse_dragged_pre(screen, button, delta, modifiers, canceled)` runs before screen drag handlers for real nonzero pointer movements with a held button. The most recently pressed button is active until it releases; focus loss and splitscreen handoff clear it. Same screen exclusions and GLFW button/modifier encoding as the other screen input hooks. screen.mouse is the new GUI position; delta is movement in GUI pixels. Cancel skips screen drag handlers while retaining the new cursor position. Synthetic input and hover-only moves skip it. Bundles need "hooks": 123.

API 124 adds `native_mod_container_viewers(dimension, pos, viewers, previous)` is heard once each time the number of players with the mod container at `pos` open changes (vanilla's `ContainerOpenersCounter`, as a barrel keeps it): `previous` 0 is `onOpen`, `viewers` 0 is `onClose`, so a block can show itself open while `viewers` is not 0. It is counted every tick from the players' open menus, so a player who leaves without closing (a lost connection, death, another dimension, out of reach, the block gone) is heard too. On a server, with no player's state swapped in. Bundles that use it need `"hooks": 124`.

API 125 adds knockback a mod can change or cancel through `native_living_knockback` (NeoForge's
`LivingKnockBackEvent`): at the start of `LivingEntity.knockback`, on the raw strength, before the entity's knockback resistance takes its
share and its motion changes, with the pushed entity's id and type (`minecraft:player` for a player), the `strength`, the direction
(`x`, `z`; vanilla points it at the source of a hit, the attacker's side, and the entity is pushed the other way) and a
`canceled` flag. A mod may change the three numbers or set `canceled`, and the push follows: canceled, the entity's motion is
left alone. A `strength` of 0 or less after the hook is no push, and a direction too short to tell draws a random one
after it, as in vanilla; the resistance scales the strength the hook leaves, so a full knockback resistance still stops the push. A value a mod
leaves that is not a finite number is put back as it was. `damage_type` (`minecraft:player_attack`), `causing`
(`DamageSource.getEntity`) and `direct` (`getDirectEntity`) are the push's damage source, named as
`native_living_incoming_damage` names a hit's, with the same entity ids left at 0 (a goat's ram pushes with
`minecraft:mob_attack` and hurts with `minecraft:mob_attack_no_aggro`, as in vanilla). It fires for the players and mobs the
game knocks back: a hit's own push, a sprinting or Knockback hit, a sweep, a spear, a shield's push on its holder and a
goat's ram. It does not fire for a sulfur cube with a block that a hit with a causing entity knocks back
(`SulfurCube.knockback` does not call `super` there), for the pushes vanilla doesn't make with `LivingEntity.knockback` (the
ender dragon's wings, Punch arrows, explosions, wind charges, a mace smash's area, a hoglin's throw), nor yet for the ones
Horizon makes another way: a mob's melee push from its attack knockback or a Knockback weapon, an armor stand's and the
ender dragon's. It has no `self`: the game is in the middle of the hit. It runs on the thread that ticks the world (the
server thread), so state a mod shares with its render thread hooks needs
synchronising.

Bundles that use it need `"hooks": 125`.

API 126 adds `native_edit_blocks(dimension, edit)` for server mods: runs up columns, from a height or a heightmap's, of block states and filters written as commands write them, made at once straight into the chunks of the level being ticked with what `fill ... strict` does for block entities; the chunks changed go to the players whole. Bundles need `"hooks": 126`.

API 127 adds column reads for server mods, in the level being ticked: `native_surface_heights(dimension, columns, heightmap)` and `native_read_columns(dimension, columns, heightmap, (below, above))`, the blocks round each column's height as a palette and indexes with the biome's grass, foliage and water tints. Bundles need `"hooks": 127`.

API 128 adds `native_experience_orb_can_merge(dimension, into, from, into_value, from_value, can_merge)` is asked for each pair of orbs whose boxes meet, when an orb spawns (`ExperienceOrb.tryMergeToExisting`; `from` is 0, the new orb is no entity yet) and every 20 ticks as an orb scans (`scanForMerges`); `can_merge` holds vanilla's `canMerge` answer, which a mod may flip either way. `native_experience_orb_merge(dimension, into, from, from_value, canceled)` follows a yes and may cancel it: both orbs stay, and a spawning orb tries the next orb in reach, then spawns on its own. A merged orb counts as `count` orbs of the keeper's value. `native_set_experience_orb(dimension, id, value, count, age)` writes an orb's `value`, `count` and `age` (each optional; false for a `value` or `count` of 0 or an orb that isn't there). Bundles need "hooks": 128.

API 129 adds `native_block_entity` gives a block a block entity whatever its template: a mod's stone casing or thin part no longer needs a chest's or furnace's template. The engine removes it when its block is broken or replaced, no piston moves it, and a mod that draws it (`native_block_entity_submit`) is asked for it. Bundles need "hooks": 129.

API 130 adds slot rules a mod decides in code for its content menu types: `native_menu_slot_rules(menu, slot, stack, may_place, max_stack)`, in `horizon-client`'s `menu.rs`, is given the type's own answer (false on one of its `results`, [mods.md](mods.md#menu-types), true elsewhere, and 99) and may change both, as a NeoForge mod overrides `Slot.mayPlace` and `Slot.getMaxStackSize`. A mod menu type's clicks run on the vanilla menu engine (`menu::Kind::Mod`). Bundles need `"hooks": 130`.

API 131 adds an item's use tick and release, as vanilla's `Item.onUseTick` and `releaseUsing` and NeoForge's `LivingEntityUseItemEvent.Tick` and `.Stop`. Each tick of a use, `native_living_entity_use_item_tick` may change the ticks left: at 0 or less, or on a cancel, the server finishes the use, as NeoForge's code does, and an item used on release stays frozen until let go. `native_on_use_tick` is what a mod's own item does then (cook, charge). When the player lets go early, `native_living_entity_use_item_stop` may skip the release, and `native_release_using` acts with the ticks left (a bow-like item shoots by its charge); setting `used` starts the item's cooldown. All four run on both sides, as vanilla's do, and take the using entity, None for a player; the tick and stop take the hand too. Change the world only where this game runs the world, not on a server's client. Bundles that use them need `"hooks": 131`.

API 132 adds `native_inventory(uuid, kind)`: a player's inventory (`Player`: 41 slots, main, armour, off hand; `Ender`: 27), live when they are on the server, else from their file, never while they log in or are dead; `native_move_stack((from, kind, slot), expected, (to, kind))`: one stack moved, checked against `expected`, where it fits whole, or nothing moves (at most one side a file, written first); `native_send_map(uuid, map_id, colors)`: a 128 x 128 map picture sent to a player, for a filled map with that `map_id`. What an operator's `/invsee` and a price chart on a map need. Bundles need "hooks": 132.

API 133 adds a loot table's function and condition of a mod's own kind. The entry that names a `type` in a namespace outside `minecraft:` — where the loot roll used to silently skip it — reaches `native_loot_function(kind, data, ctx, stack)` or `native_loot_condition(kind, data, ctx, holds)` when the table rolls. The context brings the entity, the damage, the player, the tool, the block, the luck, the biome and a seed from the roll's random. Both hooks answer in the tables the items engine rolls (block drops, entity drops, loot_table, interact_loot), where entity, damage, player and biome are None; the function hook there gets the stack as the main engine's (its item, count, damage, enchantments, potion and saved components) and takes back what it changes, but a function that asks for a map's structure search changes nothing there. An unclaimed function leaves the stack, an unclaimed condition fails, as unknown kinds do. Bundles that use them need "hooks": 133.

API 134 adds a server's levels opening and closing, on the single player server thread and on a dedicated server: `native_level_load(dimension)` when a level opens, before any of its chunks (NeoForge's `LevelEvent.Load`), and `native_level_unload(dimension)` once it has closed, after its last save (`LevelEvent.Unload`). The level the world opens in comes before `WorldOpenReady`; the Nether and the End open when someone first goes there and close after 300 ticks with nobody in them. Every level still open closes when the server stops, before `WorldCloseFinal`, or when its open or its loop fails, so each load has one unload. Bundles that use them need "hooks": 134.

API 135 adds the client's level made and dropped, on the window: `native_client_level_load(dimension)` when the client makes a level, on joining a server (single player's own included) and on changing dimension (NeoForge's `LevelEvent.Load` from the `ClientLevel` constructor), and `native_client_level_unload(dimension)` when it drops one, replaced by another dimension's (told after the new one's load, as `Minecraft.setLevel` does) or on leaving the server (`LevelEvent.Unload`). Each load has one unload, except when the game itself quits. Bundles that use them need "hooks": 135.

API 136 adds a projectile's impact, which a mod can cancel, through `native_projectile_impact` (NeoForge's `ProjectileImpactEvent`): it fires when a projectile has found what it meets, before the hit acts, with the projectile's id, type (`minecraft:arrow`) and owner, the level, and what it hit: an entity (its id and type) or a block (its position and the face's normal) and the point. Set `canceled` and the impact does not happen: no damage, knockback, sound or block reaction, an arrow does not stick, a pearl does not teleport, a potion or a fireball does not burst. The projectile flies on and is not discarded, so a mod that cancels decides what becomes of it. It covers arrows, tridents, thrown items, fireballs, wither skulls, wind charges, llama spit and shulker bullets, on the single player server thread and on a dedicated server. It does not fire for a breeze's deflection, a hit on a wind charge, fireworks or fishing bobbers. A mod hooks `native_hears_projectile_impacts(hears)` too and sets `hears`: without it the server keeps no copy of its flying projectiles and the impact hook is not called. Bundles that use it need "hooks": 136.

API 137 adds an entity getting on or off a vehicle, which a mod can cancel, through `native_entity_mount` (NeoForge's `EntityMountEvent`): the rider's id and type (`minecraft:player`, `minecraft:zombie`), the vehicle's id and type (`minecraft:oak_boat`, `minecraft:horse`), whether it is mounting, and a `canceled` flag. Set `canceled` and the rider does not board, or keeps riding; a `/ride` that is canceled fails. It is told for a player boarding a boat, minecart, cushion or mount (pig, strider, horse and its kin, llama, nautilus, happy ghast) and getting off by their own input, `/ride`, a stab, a buck, a teleport or spectator mode, and for the mobs Horizon seats on a vehicle (jockeys, raiders on a ravager, `/ride`) and lets off one with `/ride` or a stab. It is not told for a rider whose vehicle is gone or dies, nor for riders loaded with a save. On the single player server thread and on a dedicated server. Bundles that use it need "hooks": 137.

API 138 adds `native_interaction_key(button, key_mapping, hand, swing_hand, canceled)` fires while playing when an interaction key is about to act on the crosshair: button 0 the attack key (`key.attack`; when an attack is allowed, and each tick a held key keeps digging a block), 1 the use key (`key.use`; once for each hand tried, main first, before the hand's target and item), 2 pick block (`key.pickItem`; only with a block or entity aimed at). Set `canceled` to skip the action; a cancelled use ends the click, so the other hand gets no turn. A cancelled attack or use still swings `hand` unless the mod clears `swing_hand` (true on entry). A single player's window and a server's player (any window that plays on a server). Bundles need "hooks": 138.

API 139 adds `native_gui_layer_pre(layer, batch, canceled)` and `native_gui_layer_post(layer, batch)` fire around each vanilla HUD layer Horizon draws, in its paint order, so a mod places its own drawing below a layer (in `pre`) or above it (in `post`), NeoForge's `RegisterGuiLayersEvent` registerBelow/registerAbove, and hides a layer by setting `canceled` in `pre` (`RenderGuiLayerEvent.Pre`). Layer ids: `minecraft:crosshair`, `hotbar`, `effects`, `boss_overlay`, `vehicle_health` (riding a mount), `player_health` (health, armor, food and air rows, one pass), `contextual_info_bar` (the jump bar, locator bar or experience bar with the level number), `selected_item_name`, `sleep_overlay`, `overlay_message` (action bar), `title`, `scoreboard_sidebar`, `subtitle_overlay`, `chat` (the unfocused lines) and `tab_list`. F1 hides the layers it hides in vanilla, and then nothing fires for them. The camera overlays (drawn by the screen-effects pass) and the debug screen are not layers. A hidden layer still runs its game logic (the health rows' blink, the bubble pop sound) and `post` still fires. Bundles need "hooks": 139.

API 140 adds `native_map_decorations(map, decorations)` is asked each time a map is drawn (held, in an item frame, or on the cartography table's screen) with `NativeMapInfo` (id, centre block, scale, dimension, locked, whether it is in a frame); push `NativeMapDecoration { sprite, x, y, rot, shown_on_frame }` (an atlas sprite, a position in half map pixels from -128 to 127, a rotation 0 to 15, and whether it shows in frames and on the screen like vanilla's markers) and it is drawn after the vanilla decorations with the same pose; a sprite the atlas lacks is left out. Nothing is saved in the map and vanilla's decorations are not passed in. This is the client half of NeoForge's `MapDecorationType`; types registered on the server and saved in map data need code-side registration. Bundles need "hooks": 140.

API 141 adds a player's client starting and stopping tracking an entity, on the single player server thread and on a dedicated server: `native_player_start_tracking(player, name, entity, uuid, kind)` when the entity is sent to the player (NeoForge's `PlayerEvent.StartTracking`, vanilla's `ServerEntity.addPairing`) and `native_player_stop_tracking(...)` when it stops (`PlayerEvent.StopTracking`, `removePairing`): out of range, gone from the level, or the player left, changed level or respawned. They name the entity by ID, UUID and type and run with no player's state swapped in, so a mod sends what the player needs with `native_net_send_to_player`. Each start has one stop. Bundles that use them need "hooks": 141.

API 142 adds an energy buffer on a mod block's block entity (`Capabilities.Energy.BLOCK`), and the helpers that read and change it. `native_block_entity_energy` declares the buffer as `SimpleEnergyHandler` is built: its capacity, the most one insert and one extract moves, and the faces a fill may come through and a drain may leave by. The amount lives in the entity's saved compound as the int `energy`, so it survives restarts and the block's own tick reads it there. `native_energy(dimension, pos, face)` reads it through a side; `native_energy_insert(pos, face, amount, simulate)` and `native_energy_extract(dimension, pos, face, amount, simulate)` answer the amount moved (`simulate` changes nothing); `native_move_energy(dimension, from, from_face, to, to_face, max)` is `EnergyHandlerUtil.move` between two blocks and answers the amount moved. Bundles need "hooks": 142.

API 143 adds data maps (`RegisterDataMapTypesEvent`). `native_data_map_types(types)` pushes the types a mod registers: an `id` (`mod_id:name`, not in `minecraft`) for a `registry`, `minecraft:item` or `minecraft:block` so far; asked once per load of the server's data (a world's first open and every `/reload`). The values come from `data/<mod_id>/data_maps/<registry path>/<name>.json` in the world's data packs and from files a mod pushes through `native_data_pack_files` under the key `data_maps/<mod_id>:<registry path>/<name>`: `{"replace": false, "values": {"minecraft:coal": ..., "#minecraft:logs": ...}, "remove": [...]}`, an entry's value being the data itself or `{"value": ..., "replace": false}`; `replace` clears what earlier files set, a tag gives every entry in it the value, `remove` applies after the file's own values, and a newer value overrides an older one. `native_data_map(registry, type, entry)` reads an entry's value (the JSON the file held) and `native_data_map_entries(registry, type)` lists them. Server side only and not synced to clients; mergers, conditions, other registries and mod datapack registries are not here yet. Bundles need "hooks": 143.

`native_living_fall(entity, kind, distance, multiplier, cancel)` corresponds to NeoForge's `LivingFallEvent` at `LivingEntity.causeFallDamage`. Mods may change the distance and damage multiplier or cancel fall handling; cancellation does not reverse physical movement. This free function receives the entity ID and type and cannot call game helpers. Player landings, mounted player falls and non-equine mob falls are wired. Equines use their own fall-damage override rather than `LivingEntity.causeFallDamage`. Bundles need `"hooks": 357`.

`native_living_jump(entity, kind)` listens for NeoForge's `LivingEvent.LivingJumpEvent` after a living entity jumps. It receives the entity ID and type, and cannot cancel or change the jump. The player motion and mob ground-jump paths, including cubes and ridden horses and camels, are wired; whether every client-side prediction path reaches them remains unverified. Bundles need `"hooks": 359`.

`native_living_heal(entity, amount, cancel)` corresponds to NeoForge's `LivingHealEvent` at `LivingEntity.heal`. The amount can be changed and cancellation prevents the heal. It covers player effect, food and peaceful regeneration healing and the wired mob healing paths. Other healing paths and client-side coverage remain unverified. `entity` is the affected player or mob ID. Bundles need `"hooks": 358`.

`native_living_drown(entity, kind, is_drowning, damage_amount, bubble_count, cancel)` asks on the server when a player or generic-air mob cannot breathe and has no air (NeoForge's `LivingDrownEvent`, `LivingEntity.baseTick`). Mods may change the drowning decision, damage and bubble count, or cancel. Damage at or below zero does not hurt. The normal eight bubbles use the entity event; other positive counts are sent in a particle packet, and nonpositive counts suppress bubbles. Bundles need "hooks": 356.

API 144 adds a player's or mob's breathing, on the single player server thread and on a dedicated server: `native_living_breathe(entity, kind, can_breathe, consume, refill)` every tick for a player and for a mob with the generic air logic (NeoForge's `LivingBreatheEvent`, vanilla's `LivingEntity.baseTick`), with Horizon's answers as the defaults: set `can_breathe` and how much air `consume` takes or `refill` gives (up to 300); at -20 the entity is set to 0 air and takes 2 drowning damage. It is a free function in `horizon_client::breathing`, so it answers and cannot call helpers. Axolotls and `WaterAnimal`s keep their own air logic. Bundles that use it need "hooks": 144.

API 145 adds blocks and items a native mod declares in code, as NeoForge's `DeferredRegister.Blocks` and `.Items` drained by `RegisterEvent`, instead of one content file each: `native_blocks(blocks)` and `native_items(items)`, asked once at start on every side, take `horizon_mods::content::Block` and `Item`, the declarations a content mod's `data/<id>/horizon/block/` and `item/` files hold (a vanilla template and what differs; `item: true` gives a block its item). Ids are in the mod's namespace. They join the content mods' blocks and items, inside the content mod whose id is that namespace when it is on (its models, textures, loot and recipes), else on their own with the template's look, so their network ids, saves and the joining client's content check are the files' (docs/mods.md). They pass the files' checks; a namespace whose declarations don't is logged and left out. Bundles that use them need `"hooks": 145`.

`Block.state_light` optionally takes a `content::StateLight` (`StateValues<u8>`), selecting emission
from 0 to 15 by state-property tuples, exactly as the [content declaration](mods.md) does. It overrides
the scalar `BlockProperties.light` only when present. The content compiler validates and resolves
every state once; native, content-only, dedicated-server and client declarations use the same emitted
state facts and ordinary light propagation, without a runtime callback.

`Item.place_block: Option<String>` binds an item to an ordinary `BlockItem` placement of a vanilla
block or a block in the item's namespace, including blocks with `item: false`. It uses the same
[content field](mods.md) on clients and servers. Components still come from the item template;
the placement context, consumption and statistics retain the alias item's ID. It does not inherit
specialized placement-item behavior from the component template.

Prepared-resource regression tests explicitly exercise this boundary:
`cargo test -p horizon-client-lib --lib place_block_aliases_keep_ -- --ignored` checks both level
contexts; `HORIZON_TEST_RESOURCES=<prepared-root> cargo test -p horizon-client --lib
place_block_alias_survival_ -- --ignored` checks ordinary survival consumption, adventure refusal
and dry client prediction. These native tests do not require a renderer or game window.

API 146 adds entities ticking, through `native_entity_tick_pre` (NeoForge's `EntityTickEvent.Pre`) and `native_entity_tick_post` (`EntityTickEvent.Post`), with the entity's id and type (`minecraft:zombie`, `minecraft:item`) and the level. Set `canceled` in the Pre and the entity doesn't tick this tick (and its Post isn't told). Told for the mobs, item entities, minecarts and boats that tick in an entity-ticking chunk, on the single player server thread and on a dedicated server; not for a player's own boat or minecart, projectiles, orbs, primed TNT, falling blocks or armor stands yet (check `kind`: later APIs add kinds). Mobs and items hear every Pre before the family's first tick and every Post after its last; minecarts and boats hear theirs around each one's tick. A mod that hooks them also hooks `native_entity_tick_listened(listened)` and sets `*listened = true`: otherwise no entity's tick is told. It runs each tick for every ticking entity, so keep it cheap. Bundles that use them need "hooks": 146.

API 147 adds a fluid handler on items (`Capabilities.FluidHandler.ITEM`). `native_item_fluid_tank(item, capacity)` makes a mod item a container with a capacity in millibuckets; what it holds lives in the stack's `minecraft:custom_data` under `horizon_fluid` (`Fluid`, `Amount`), so it is saved, sent to Horizon clients and stacks only with equal contents. `native_item_fluid(stack)` reads one item of a stack (vanilla's buckets are built in: empty, water and lava, a whole bucket or nothing); `native_item_fluid_fill(stack, fluid, amount)` and `native_item_fluid_drain(stack, max)` answer how much one item takes or gives and the single item it becomes, as `IFluidHandler.fill`/`drain` through an `ItemAccess`; the stack given is not changed, so the caller shrinks it by one and hands out the result. They work on any side. Bundles need "hooks": 147.

API 148 adds a mob's or an armor stand's equipment changing, on the single player server thread and on a dedicated server: `native_living_equipment_change(entity, kind, slot, from, to)` once for each slot (`mainhand`, `offhand`, `feet`, `legs`, `chest`, `head`, `body`, `saddle`) that holds another stack than the mods last heard (NeoForge's `LivingEquipmentChangeEvent`, vanilla's `LivingEntity.collectEquipmentChanges`). A mob new to the level counts from empty on its first tick. A mod that hooks it also hooks `native_living_equipment_listened(listened)` and sets `*listened = true`: otherwise the game does not read every mob's equipment each tick, and only armor stands are told. Players keep `native_on_equip`. It only hears. Bundles that use it need "hooks": 148.

API 149 adds `native_player_submit(player, who, draw, drawn)` fires once a frame for each other player in view and for this window's player in third person (not while invisible), with the `native_mob_submit` frame (`NativeMobRender`, a `BlockEntityDraw` at the feet turned with the body) and `who` (profile name, whether it is the local player); set `drawn` to leave out the vanilla model, armour, held items, cape and other layers (`RenderPlayerEvent.Pre`). `native_mob_layer(mob, who, draw)` fires once a frame after the vanilla model and layers of every mob and player in view that a mod did not draw itself, and what the mod adds in that frame is drawn with it (`EntityRenderersEvent.AddLayers`, `RenderLivingEvent.Post`); `who` is set for a player. Geometry attaches to the entity's frame, not to the model's bones: head yaw, pitch and the walk cycle are in `NativeMobRender`. A mod that hooks either also hooks `native_player_render_listened(listened)` and sets `*listened = true`: otherwise neither is asked. Bundles need "hooks": 149.

API 150 adds a player finishing a sign: `native_sign_changed(player, name, dimension, pos, front, lines, cancel)` is heard when a player sends the text they wrote on a sign they were editing, right before it goes on the sign, once the checks have passed, on a server, with the player's state swapped in (`native_tell` reaches them). A mod may rewrite `lines` (four lines of plain text) or set `cancel` so the sign keeps its old text; a waxed sign is not told. Bundles that use it need `"hooks": 150.

API 151 adds models no blockstate names, as NeoForge's `ModelEvent.RegisterStandalone`: `native_standalone_models(models)`, asked once at start, takes the ids of models a block entity or a mob draws as parts (`mymod:block/shaft_half`, from `assets/mymod/models/block/shaft_half.json`), and every resource load, a pack reload included, bakes them beside the block models. `native_draw_model(draw, model, tint)` draws one under `draw`'s pose in `native_block_entity_submit` or `native_mob_submit`, as `native_draw_block` draws a block's model, with `tint` (0xRRGGBB) on its tinted faces. A model that doesn't read is logged and left out, and drawing it gives false; no block has to be registered just to bake a part's model. Bundles that use them need `"hooks": 151`.

API 152 adds a player opening and closing any menu on a server, on the single player server thread and on a dedicated server: `native_player_container_open(player, name, dimension, menu, pos)` and `native_player_container_close(...)` (NeoForge's `PlayerContainerEvent.Open` and `.Close`), with the menu type's name and, for a block's menu, its position. They run with the player's state swapped in. Unlike the viewer count of a mod container (`native_mod_container_viewers`) they name the player and cover every menu: chests, furnaces, crafting tables, trades, a mod's own. A menu is heard once a tick, so one opened and closed inside a tick is not; each open has one close, also when the player leaves. Bundles that use them need "hooks": 152.

API 153 adds a player starting or stopping flight: `native_player_toggle_flight(player, name, flying, cancel)` is heard once for each change a player's client makes to its flying (vanilla honours it only for a player who may fly), on a server, with the player's state swapped in. A mod sets `cancel` to refuse it: the player keeps the state they had and the server sends their abilities back, so the client lands or flies again. Only what a client asks for is heard, not the flight a game-mode change starts or ends, and not a LAN host's own player. Bundles that use it need `"hooks": 153`.

API 165 adds `native_physical_window_size()`, the window's width and height in physical pixels, independent of GUI scale. It returns no size on a server or when no window exists. Bundles that use it need `"hooks": 165`.


API 154 adds a player changing dimension: `native_player_changed_dimension(player, name, from, to)` is heard once per move of a server's player into another level, when the move has ended (a portal's exit found and the player standing there, a command's teleport landed, a respawn in another level placed for good), with `from` and `to` as dimension ids (`minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`) and the player's state swapped in so that `native_tell` and the other helpers reach them. A move within a level is not heard. Bundles that use it need `"hooks": 154`.

API 155 adds A mod that hears `native_experience_orb_pickup` sets `listened` in `native_experience_orb_pickup_listened` (a HEAD injection); `native_experience_orb_pickup(player, dimension, orb, value, count, canceled)` then runs before a server player takes an orb. Canceling leaves the orb and the pickup delay unchanged; otherwise the value is clamped to 1..=2,147,483,647 and the count to at least one, then one count is taken. `native_mending_repair(player, stack, experience, repair)` sees the damaged Mending item picked at random and can change the durability repair proposed for it, which is capped at its current damage. Turning a positive proposal into zero leaves the experience unspent (a zero vanilla proposal keeps vanilla's behaviour); if the picked stack is removed, changed except for its damage, or no longer eligible, the repair is skipped and the experience remains. The repair also decides how much experience is left for further repairs. It runs for `native_give_experience` too. Both run on the integrated and the dedicated server, guests included. Bundles need "hooks": 155.

API 156 adds where a loot roll happens, for a mod's own loot kinds: `ctx.place` in `native_loot_function` and
`native_loot_condition` brings `LootContextParams.ORIGIN` (a broken block's centre, a dead mob's position), `BLOCK_ENTITY` (the broken
block's entity as it was, saved with its id, position and `Items`) and the level (its dimension, game time, time of day, rain and
thunder), on a block broken by a player or an explosion and on a mob's death loot; None on the other rolls. The game sets it
around a roll with `with_native_loot_place`. Bundles that use it need `"hooks": 156`.

API 157 adds how a mod block burns, and whether it makes a campfire over it a signal fire, on an authoritative server. `native_fire_odds(dimension, state, odds)` answers
`FireBlock`'s flammability table (`FireBlock.setFlammable(block, igniteOdds, burnOdds)`) before vanilla's table: put
`(ignite, burn)` in `odds` for your own blocks. Ignite odds above zero let fire stand beside the block and spread
to the air around it (planks 5, wool 30); burn odds are how fast fire destroys it (planks 20, wool 60); `(0, 0)` never
burns; keep both within vanilla's 0 to 100. A waterlogged state never burns and isn't asked. It runs hundreds of times per fire tick: keep it short.
`native_smoke_source(dimension, state, smoke)` answers `CampfireBlock.isSmokeSource` for the block under a campfire,
as the campfire is placed or that block changes: `Some(true)` makes the campfire a signal fire, as a hay bale does.
Answer only for your own blocks and leave the answer empty otherwise; the level is read-only. The deprecated in-frame
path fires neither. Bundles that use them need `"hooks": 157`.

API 158 adds a server player's game mode about to change: `native_player_game_mode_changed(player, name, from, to, cancel)` is heard before the change (NeoForge's `PlayerChangeGameModeEvent`) for the `/gamemode` command, the F3+F4 switcher and a hardcore world's respawn into spectator, once for each change to a different mode (0 survival, 1 creative, 2 adventure, 3 spectator), on a server; not for the default mode a player joins with. A mod sets `cancel` to keep the old mode. Bundles that use it need `"hooks": 158`.

API 159 adds the server's difficulty changed, from one of `peaceful`, `easy`, `normal`, `hard` to another, through `native_difficulty_changed` (NeoForge's `DifficultyChangeEvent`). It is vanilla's `MinecraftServer.setDifficulty`, which `/difficulty`, a dedicated server's management and a test environment's difficulty all go through, and it is told only when the difficulty is another one. It only hears: the change has already happened. Bundles that use it need "hooks": 159.

API 160 adds a player's fishing rod: `native_player_fish(player, name, state, cancel)` is heard for the cast (`state` 0), a fish biting (1), the reel-in with a catch (2), the reel-in with nothing on the line (3) and the reel-in of a bobber hooked to an entity or item (4), for a player of a server (Bukkit's `PlayerFishEvent`; NeoForge's `ItemFishedEvent` fires later, once the loot is rolled). A mod sets `cancel` to stop a cast, make the fish get away, or leave the bobber out on a reel-in. The player's state is swapped in. Bundles that use it need `"hooks": 160`.

API 161 adds a player took a crafting grid's result, through `native_item_crafted` (NeoForge's `PlayerEvent.ItemCraftedEvent`): the player's uuid, the recipe the grid matched (`minecraft:oak_planks`) and the result stack it took, with its components. It only hears; the take has already happened. It fires for the crafting grid of an inventory or a crafting table, on the integrated server and on a dedicated server, guests included, and not for a stonecutter's, a smithing table's or a furnace's result. Bundles that use it need "hooks": 161.

API 162 adds a player's item losing durability: `native_item_damaged(player, name, item, amount, cancel)` is heard where `ItemStack.hurtAndBreak` wears the item of a player of a server (Bukkit's `PlayerItemDamageEvent`), after Unbreaking, with the points about to be lost. A mod changes `amount` or sets `cancel` for no wear. The player's state is swapped in. Bundles that use it need `"hooks": 162`.

API 163 adds `native_server_event(name, uuid, message)` posts an `announce` event to a dedicated server's event stream (`GET /api/v1/events`); horizon-server's Discord bridge relays it, in gold, when `[discord] events` lists `announce` (off by default). False elsewhere and for an empty message. Bundles that use it need "hooks": 163.

API 164 adds `native_state_block(state)` names the block a state belongs to (`BlockState.getBlock()`), on a server hook's level and on the game: `native_state_for_placement` is asked for every block placed, so a mod compares it with its own block to answer only for it. Bundles that use it need "hooks": 164.

API 165 adds the helper `native_physical_window_size()`, which returns the window's width and height in physical pixels, independent of GUI scale. It returns no size on a server or when no window exists. Bundles that use it need `"hooks": 165`.

API 166 adds a recipe ingredient of a mod's own kind. The ingredient entry that names a type in a namespace outside minecraft: reaches native_ingredient(kind, data, items) at load time. Fill items with the item ids the ingredient matches (item ids only: components are not checked; a match-time hook follows separately). Duplicates are removed keeping first-seen order. An unclaimed kind leaves the list empty and the recipe fails. The hook may be called more than once per recipe; the answer must be the same. Bundles that use it need "hooks": 166.

API 167 adds lowering the frame rate limit: `native_framerate_limit(focused, minimized, in_world, limit)` is asked each frame with the limit Horizon is about to use (Max Framerate after the inactivity throttle and Battery Mode, 260 for unlimited), whether the window has focus, whether it is minimised and whether a world is open. A mod may lower `limit`, for example to throttle a window in the background as Dynamic FPS does; Horizon keeps the lower of its own limit and the mod's and ignores values below 1. Bundles that use it need `"hooks": 167`.

API 168 adds a player swapping their main and off hand items: `native_player_swap_hands(player, name, cancel)` is heard before the swap (Bukkit's `PlayerSwapHandItemsEvent`; a swap from an open inventory is not heard), for a player of a server who is not a spectator, with their state swapped in. A mod sets `cancel` to keep the items where they are; the player's client gets its slots back. Bundles that use it need `"hooks": 168`.

API 169 adds a player's food level changing: `native_food_level_changed(player, name, from, to, cancel)` is heard for a player of a server when a turn of theirs changed it (eating, hunger and saturation effects, exhaustion, starving; Bukkit's `FoodLevelChangeEvent`), before the new level reaches them. A mod changes `to` or sets `cancel` to leave the level at `from`. Bundles that use it need `"hooks": 169`.

API 170 adds a server player teleported: `native_player_teleported(player, name, from, to, dimension)` is heard after `/tp` or `/teleport` has moved a player inside their level or into another one (NeoForge's `EntityTeleportEvent`), with their state swapped in. `from` and `to` are their feet before and after, `dimension` the level they are in now. It cannot stop the move; ender pearls, chorus fruit and a spectator's teleport menu are not heard yet, portals and respawns are not teleports here. Bundles that use it need `"hooks": 170`.

API 171 adds the command tree a server sends a player: `native_player_commands_sent(player, name, names)` is heard when a player joins and when their permission level changes, with the root command names their client would get. For a player already on their state is swapped in. A mod removes names and that client's tree is sent without them (its chat box neither completes nor colours them); a typed command still runs, `native_player_command` stops it. Bundles that use it need `"hooks": 171`.

API 172 adds a joining player the mods may refuse: `native_player_login(player, name, cancel, reason)` is heard once for each player a server has accepted (name, whitelist, bans and player limit checked), before their saved state is loaded and before they are placed in a level (NeoForge's `PlayerNegotiationEvent`). A mod sets `cancel` to disconnect them with `reason` ("Disconnected" when it stays unset); no join message and no `native_player_logged_in` follow. Bundles that use it need `"hooks": 172`.

API 173 adds a server player's sprint: `native_player_sprint(player, name, sprinting)` is heard once each time a player's client starts or stops sprinting (the sprint key, a double tap forward, letting go) and the server's player has taken it, with their state swapped in. It only hears what the client asks: a sprint that ends by itself (knockback, hunger, a wall) is not reported. Bundles that use it need `"hooks": 173`.

API 174 adds a resource pack answer: `native_resource_pack_status(player, name, status)` is heard when a server player's client answers a resource pack in play (`status` is `ServerboundResourcePackPacket.Action`'s number: 0 loaded, 1 declined, 2 failed download, 3 accepted, 4 downloaded, 5 invalid URL, 6 failed reload, 7 discarded), with their state swapped in. The pack offered during the configuration is answered before the player has joined and is not heard. Bundles that use it need `"hooks": 174`.

API 175 adds a player taking an item from or putting one on an armor stand: `native_armor_stand_manipulate(player, name, stand_id, slot, cancel)` is heard after the stand's disabled-slot checks and before anything moves, with the stand's entity id and the slot the click reached (`mainhand`, `offhand`, `feet`, `legs`, `chest`, `head`), on a server, with the player's state swapped in. A mod sets `cancel` and neither the stand nor the player's hand changes. Bundles that use it need `"hooks": 175.

API 176 adds a player opening one of the game's own menus: `native_container_opened(player, name, kind, pos, cancel)` is heard when a chest, furnace, crafting table, villager, anvil, lectern, horse inventory... is about to open (the job of NeoForge's `PlayerContainerEvent.Open`, but before the menu opens), after the checks that refuse it, with the menu type and the block's position, on a server, with the player's state swapped in. A mod sets `cancel` and nothing opens. Menus a mod opens (`native_open_menu`) are not heard. Bundles that use it need `"hooks": 176`.

API 177 adds a creative player setting a slot: `native_creative_slot(player, name, slot, item, count, cancel)` is heard when the creative screen sets an inventory slot, or throws a stack out (a slot below 0), after the packet passed vanilla's checks and before the slot changes, on a server, with the player's state swapped in. A mod sets `cancel` and the packet is ignored. Bundles that use it need `"hooks": 177`.

API 178 adds a boat or minecart moving: `native_vehicle_moved(player, kind, from, to)` is heard once a tick for each boat or minecart that carries a server player (and for one a server player left, for ten seconds after it stood still), with the vehicle's type and its position before and after (`VehicleMoveEvent`), on a server, with the rider's state swapped in. `player` is the rider's UUID, none for a vehicle that carries nobody now. It only hears. Bundles that use it need `"hooks": 178`.

API 179 adds a server player emptying a bucket: `native_bucket_empty(player, name, pos, fluid, cancel)` is heard right before the fluid is placed, once the player may build there. A mod sets `cancel` and nothing is poured. The player's state is swapped in, so the helpers reach them. `native_fill_bucket` hears the same click without the player and the fluid; dispensers and cauldrons are not heard. Bundles that use it need `"hooks": 179`.

API 180 adds a server player placing a hanging entity: `native_hanging_place(player, name, kind, pos, cancel)` is heard for an item frame, a glow item frame, a painting and a leash knot (a lead tied to a fence that has none), once it fits and before it is placed or the item used. A mod sets `cancel` and nothing is placed. The player's state is swapped in, so `native_tell` answers them. Bundles that use it need `"hooks": 180`.

API 181 adds TNT about to be primed: `native_tnt_primed(player, pos, cancel)` is heard before a flint and steel, a fire charge, fire, a redstone signal, an arrow or a dispenser lights the TNT at `pos`, with the player whose click lit it (None when no player did). A mod sets `cancel` and it is not primed: the block stays, or is put back when fire or a signal had already taken it out. The clicking player's state is swapped in. TNT set off by a blast, a TNT minecart and a sulfur cube are not asked. Bundles that use it need `"hooks": 181`.

API 182 adds a totem saving a player: `native_player_totem(player, name)` is heard for a server player whose hurt would have killed them and whose totem fired (`LivingEntity.checkTotemDeathProtection`, Bukkit's `EntityResurrectEvent`), just before the totem is used up. Listen only. The player's state is swapped in. Bundles that use it need `"hooks": 182`.

API 183 adds `native_anvil_damaged(dimension, pos, from, to)` runs after an anvil at `pos` changes from `from` to `to`; `to` is air when it broke. Bundles need "hooks": 183.

API 184 adds `native_beacon_state_change(dimension, pos, active)` is called after the beacon at `pos` gains or loses its complete beam pyramid; `active` says whether it is active now. Bundles need "hooks": 184.

API 185 adds A bell is about to ring: `native_bell_ring(dimension, player, pos, cancel)` is asked before the bell event and sound. Set `cancel` to stop the ring and its raider reveal. `player` is the acting server player when available. Bundles need "hooks": 185.

API 186 adds `native_block_break_progress_update(player, name, pos, progress)` is called when a guest's crack animation at `pos` reaches a new stage from 0 through 9. Bundles need "hooks": 186.

API 187 adds Fire about to burn a block: `native_block_burn(dimension, pos, cancel)` is heard when fire's burnout check is about to remove the block at `pos`. Set `cancel` to keep it. Bundles need "hooks": 187.

API 188 adds A campfire finishes cooking `item` into `result` at `pos`, before dropping the result. Bundles need "hooks": 188.

API 189 adds A dispenser equips `item` on the first eligible player, mob or armor stand in front of its block at `pos`, after the item is consumed and the equip sound and game event are applied. `target` identifies the kind of entity equipped. Bundles need "hooks": 189.

API 190 adds A block with a vanilla experience drop is about to award its points at `pos`; `block` is its identifier. Change `experience` to adjust the amount before the orbs are awarded. Bundles need "hooks": 190.

API 191 adds `native_block_fade(dimension, pos, from, to, cancel)` runs when ice or snow melts, or coral dies. `from` and `to` are block state IDs. Set `cancel` to keep the old block. Bundles need "hooks": 191.

API 192 adds `native_block_failed_dispense(dimension, pos, item)` is called when a dispenser at `pos` fails to dispense `item`. Bundles need "hooks": 192.

API 193 adds `native_block_form(dimension, pos, from, to, cancel)` runs when precipitation is about to place ice or snow, or add a snow layer at `pos`. `from` and `to` are block state IDs. Set `cancel` to keep the old state. Bundles need "hooks": 193.

API 194 adds Fire about to be placed: `native_block_ignite(dimension, player, pos, cause, cancel)` is heard for fire spread, lava, lightning and dispenser flint-and-steel. Set `cancel` to leave the position unchanged. Direct player flint-and-steel/fire-charge placement is not covered. Bundles need "hooks": 194.

API 195 adds `native_block_lock_check(player, name, pos, locked)` runs when a guest checks the container at `pos`; `locked` starts with vanilla's result and may be changed. The guest's state is not swapped in. Bundles need "hooks": 195.

API 196 adds A sculk sensor, calibrated sensor or shrieker received vibration event `event` at `pos`; `block` is its identifier. Set `cancel` to suppress the block's response. Bundles need "hooks": 196.

API 197 adds A dispenser sheared entity `entity` of kind `kind` at the block in front of the dispenser at `pos`, after the shearing effects. Bundles need "hooks": 197.

API 198 adds A brewing stand finished brewing: `native_brewing_complete(pos)` is heard after the bottles at `pos` received their brewed contents. Server-side. Bundles need "hooks": 198.

API 199 adds A brewing stand consumed fuel: `native_brewing_fuel(pos, fuel)` is heard after the stand at `pos` consumed the item `fuel` and gained brewing fuel. Server-side. Bundles need "hooks": 199.

API 200 adds A brewing stand starts brewing after it consumes fuel and sets its timer. Bundles need "hooks": 200.

API 201 adds A system broadcast line is about to be logged and sent to every player: `native_broadcast_message(line, cancel)` lets a mod rewrite it or set `cancel` to drop it. Bundles need "hooks": 201.

API 202 adds An item was placed on a campfire and began cooking. Bundles need "hooks": 202.

API 203 adds A cauldron's fill level is about to change: `native_cauldron_level_changed(dimension, player, pos, from_level, to_level, reason, cancel)` is asked for rain, snow, dripstone and extinguishing. Set `cancel` to leave it as it was. `from_level` and `to_level` use 0 for empty and 1 to 3 for filled levels. `player` is the acting server player when available. Bucket, bottle and dyeing interactions are not covered yet. Bundles need "hooks": 203.

API 204 adds A player adds a compostable item to a composter at `pos`; the layer has been applied. Bundles need "hooks": 204.

API 205 adds A crafter at `pos` completed `recipe`; item consumption and output handling have finished. Bundles need "hooks": 205.

API 206 adds A player used an item in `minecraft:creeper_igniters` on a creeper, after it entered its ignited state. Bundles need "hooks": 206.

API 207 adds Lightning struck a creeper and set its charged state; the hook reports the creeper id and strike position. Bundles need "hooks": 207.

API 208 adds The first Ender Dragon fight is about to place its egg at `pos`: `native_dragon_egg_form(dimension, pos, cancel)` runs on `GameLevel`. Set `cancel` to leave the block unchanged. Bundles need "hooks": 208.

API 209 adds A player applied an enchantment: `native_enchant_item(player, name, pos, item, levels)` is heard after the player at `pos` applied it, with the finished `item` and levels charged. The guest's state is not swapped in. Bundles need "hooks": 209.

API 210 adds Saved entity records for a chunk finished loading into the server world: `native_entities_load(dimension, chunk, count)` reports the dimension, chunk coordinates and number of records read. Bundles need "hooks": 210.

API 211 adds Entities in a chunk left the active world and were queued for saving: `native_entities_unload(dimension, chunk, count)` reports the dimension, chunk coordinates and number of records. Bundles need "hooks": 211.

API 212 adds A zombie or raider finished breaking the door at `pos`; the mob id and entity type are reported after the door is removed. Bundles need "hooks": 212.

API 213 adds Two mobs completed a breeding action: `native_entity_breed(parent, partner, child)` reports both parents and the baby id when one was created; egg laying has no child id. Bundles need "hooks": 213.

API 214 adds A mob changed the block at `pos`; the hook reports the mob id and kind, and the old and new block names after the new state was applied. Bundles need "hooks": 214.

API 215 adds A bee was stored in a beehive or bee nest at `pos`; the hook reports the bee id and kind and the block name, after the bee was removed from the level. Bundles need "hooks": 215.

API 216 adds A mob entered love mode after a player interaction; the hook reports its entity id, kind and feeder UUID when known. Bundles need "hooks": 216.

API 217 adds A server entity finished its portal wait and is beginning to leave through the portal block at `pos`: `native_entity_portal(dimension, entity, pos)` reports the entity's network id, source dimension and entry block. The transfer destination follows the existing portal rules. Bundles need "hooks": 217.

API 218 adds A guest player's synchronized pose changed: `native_entity_pose_change(player, name, from, to)` reports the old and new pose names. Bundles need "hooks": 218.

API 219 adds A player fired an arrow from a bow or crossbow: `native_entity_shoot_bow(player, name, projectile, force)` reports the arrow id and shooting power after the completed shot. Bundles need "hooks": 219.

API 220 adds A mob became tamed after its owner and persistence were set: `native_entity_tame(entity, kind, owner)` reports the entity type and owner UUID when known. Bundles need "hooks": 220.

API 221 adds A server entity completed a same-level teleport command: `native_entity_teleport(dimension, entity, from, to)` reports its network id and the coordinates before and after the move. Bundles need "hooks": 221.

API 222 adds A guest player's fall flying state changed after the server accepted or ended a glide: `native_entity_toggle_glide(player, name, gliding)`. Bundles need "hooks": 222.

API 223 adds A mob conversion completed through Horizon’s shared `Mob.convertTo` path; the hook reports the removed entity and its replacement ids and kinds. Bundles need "hooks": 223.

API 224 adds An experience bottle broke, after its water splash and experience orbs were created; the hook reports the impact position and awarded experience. Bundles need "hooks": 224.

API 225 adds A firework rocket completed its damage, particles and explosion effects; the hook reports its entity id, position and number of burst shapes. Bundles need "hooks": 225.

API 226 adds `native_fluid_level_change(dimension, pos, fluid, from, to, cancel)` is called when flowing water or lava at `pos` is about to change level; levels run from 0 through 8. Set `cancel` to keep the old level. Bundles need "hooks": 226.

API 227 adds A furnace consumes `fuel` and begins burning at `pos` (`native_furnace_burn`). Bundles need "hooks": 227.

API 228 adds A server player takes `count` items of `item` from a furnace at `pos` (`native_furnace_extract`). Bundles need "hooks": 228.

API 229 adds A furnace finishes smelting `item` into `result` at `pos` (`native_furnace_smelt`). Bundles need "hooks": 229.

API 230 adds A furnace begins cooking `item` at `pos` when its cooking timer first advances (`native_furnace_start_smelt`). Bundles need "hooks": 230.

API 231 adds A generic world game event fired at `pos`: `native_game_event(event, pos)` reports its registered event id and position, whether or not vibration listeners are nearby. Bundles need "hooks": 231.

API 232 adds A hopper is about to pick up a dropped stack: `native_hopper_pickup_item(dimension, pos, item, count, cancel)` names its position and stack. Set `cancel` to leave the item in the world. Bundles need "hooks": 232.

API 233 adds `native_inventory_block_start(player, name, pos, cancel)` runs before a guest opens an inventory supplied by a block at `pos`. Set `cancel` to leave it closed; the guest's state is not swapped in. Bundles need "hooks": 233.

API 234 adds A dropped item entity was successfully added to the server level: `native_item_spawn(entity, item, count, pos)` reports its entity id, item type, stack count and spawn position. Bundles need "hooks": 234.

API 235 adds `native_leaves_decay(dimension, pos, cancel)` is called when a non-persistent leaf block at distance 7 is about to decay. Set `cancel` to keep it. Bundles need "hooks": 235.

API 236 adds A weather, Channeling-trident or `/summon` lightning bolt is about to be created: `native_lightning_strike(player, pos, cause, cancel)`. Set `cancel` to prevent the bolt and its effects. Bundles need "hooks": 236.

API 237 adds `native_moisture_change(dimension, pos, from, to, cancel)` runs when farmland at `pos` is about to change moisture from `from` to `to`. Set `cancel` to keep the old moisture. Bundles need "hooks": 237.

API 238 adds A server player put a plant into a flower pot: `native_player_flower_pot_manipulate(player, name, pos, plant)` is heard after the potted state is stored and the item consumed. Bundles need "hooks": 238.

API 239 adds A server player inserted a book into a lectern: `native_player_insert_lectern_book(player, name, pos, book)` is heard after the block entity and inventory changed. Bundles need "hooks": 239.

API 240 adds A server player changed an item frame: `native_player_item_frame_change(player, name, entity_id, action, item, rotation)` is heard after its contents and comparator output changed. Bundles need "hooks": 240.

API 241 adds An orb repaired an item through Mending: `native_player_item_mend(player, name, item, repair, experience)` is heard after its damage changed. Bundles need "hooks": 241.

API 242 adds A server player changed a lectern page: `native_player_lectern_page_change(player, name, pos, page)` is heard after the page and redstone pulse changed. Bundles need "hooks": 242.

API 243 adds A server player took a book from a lectern: `native_player_take_lectern_book(player, name, pos, book)` is heard after its block entity changed. Bundles need "hooks": 243.

API 244 adds `native_spawn_change(pos, angle)` is told after `/setworldspawn` moves the world's spawn point, with its block position and rotation. Bundles need "hooks": 244.

API 245 adds `native_world_border_bounds_changed()` is told after `/worldborder set` or `/worldborder add` changes the border size or starts its timed size change. It hears successful commands only. Bundles need "hooks": 245.

API 246 adds `native_world_border_center_changed()` is told after `/worldborder center` moves the border center. It hears successful commands only. Bundles need "hooks": 246.

API 247 adds `native_world_game_rule_change(player, name, rule, value)` is told after an operator changes a world game rule through the server's game rule screen, with its final value. Bundles need "hooks": 247.

API 248 adds A note block is about to sound: `native_note_play(dimension, player, pos, instrument, note, cancel)` is asked when punched, used or powered. Set `cancel` to silence it; a canceled tuning interaction leaves the note unchanged. `player` is the acting server player when available. Bundles need "hooks": 248.

API 249 adds A server player completed an advancement after progress was recorded; `advancement` is its identifier, such as `minecraft:story/mine_stone`. Helpers looking up the guest through `self.lan` may find nothing during packet handling. Bundles need "hooks": 249.

API 250 adds A server player's main-hand punch swing and attack-strength reset have happened: `native_player_attack_cooldown_reset(player, name)`. Helpers looking up the player through `self.lan` may find nothing during a guest packet. Bundles need "hooks": 250.

API 251 adds A server player about to enter a bed after its sleep checks pass: `native_player_bed_enter(player, name, pos, cancel)` hears the bed head position. Set `cancel` to keep the player out. The player's state is swapped in, though helpers that look them up through `self.lan` may find nothing during a guest packet. Bundles need "hooks": 251.

API 252 adds A server player woke or stood up from the bed at `pos`, after their sleeping state changed. Helpers looking up the guest through `self.lan` may find nothing during packet handling. Bundles need "hooks": 252.

API 253 adds A server player caught a bucketable mob: `native_player_bucket_entity(player, name, entity_id, entity)` runs after the mob was removed and the filled bucket was put in the player's hand or inventory. `entity` is the mob species id. Bundles need "hooks": 253.

API 254 adds A server player's selected beacon effects were stored: `native_player_change_beacon_effect(player, name, pos, primary, secondary)`. Empty effect ids mean none is selected. Bundles need "hooks": 254.

API 255 adds A server player's client option changed their dominant hand; `native_player_change_main_hand(player, name, left)` reports whether the left hand is dominant. Bundles need "hooks": 255.

API 256 adds A server player counts as deeply asleep after 100 ticks in a bed: `native_player_deep_sleep(player, name)` fires once at that point. Helpers looking the guest up through `self.lan` may find nothing during its tick. Bundles need "hooks": 256.

API 257 adds A server player book-and-quill packet is about to be applied. `pages` is its page count and `signing` says it has a title; set `cancel` to drop it. Helpers looking up the guest through `self.lan` may find nothing during packet handling. Bundles need "hooks": 257.

API 258 adds A server player threw an egg after its projectile was launched and its use applied. Bundles need "hooks": 258.

API 259 adds A server player used a firework rocket to boost while gliding, after the rocket was launched and consumed. Bundles need "hooks": 259.

API 260 adds A server player's experience points from an orb pickup: `native_player_exp_change(player, name, amount)` is called after mending has taken its share and before the remaining points are added. Change `amount` to change the points added; negative values become zero. The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing while a guest packet is handled. Command experience changes use another path and are not heard. Bundles need "hooks": 260.

API 261 adds A server player's XP orb pickup cooldown changed, after its new value is stored (`native_player_exp_cooldown_change(player, name, ticks)`). Bundles need "hooks": 261.

API 262 adds An item cooldown about to start for a server player: `native_player_item_cooldown(player, name, item, ticks, cancel)` is called for use cooldowns and for a shield disabled by a hit. `item` is the item id and `ticks` its duration. Set `cancel` to skip the cooldown; shield disable still stops the use and plays its configured sound. The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing while a guest packet is handled. Bundles need "hooks": 262.

API 263 adds A server player selected a different hotbar slot: `native_player_item_held(player, name, from_slot, to_slot, cancel)` is called after the slot range check and before the selected slot changes. Set `cancel` to keep the old slot; the client is sent that slot again. The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing while a guest packet is handled. Bundles need "hooks": 263.

API 264 adds A server player changed their client language option (`native_player_locale_change(player, name, locale)`), after the new locale is stored. Bundles need "hooks": 264.

API 265 adds A server player selected loom pattern button `pattern`, after the menu accepted it: `native_player_loom_pattern_select(player, name, pattern)`. Bundles need "hooks": 265.

API 266 adds A server player filled an empty map: `native_player_map_filled(player, name, map_id)` fires after the filled map and its saved map data were created. Guest lookup helpers may find nothing during packet handling. Bundles need "hooks": 266.

API 267 adds A server player named a mob with a name tag, after its name and persistence changed: `native_player_name_entity(player, name, entity_id, entity_name)`. Bundles need "hooks": 267.

API 268 adds A server player opened a sign editor at `pos`, for the front or back side, after the editor state was made: `native_player_open_sign(player, name, pos, front)`. Bundles need "hooks": 268.

API 269 adds A server player's middle-click pick resolved a block or entity's pick stack and is about to put it in the hotbar: `native_player_pick_item(player, name, from_entity, pos, entity_id, cancel)` receives block coordinates and entity_id zero for a block, or zero coordinates and the entity network id for an entity. A pick with no result is not heard. Set `cancel` to leave the inventory and client unchanged. The player's state is swapped in, though helpers that look them up through `self.lan` may find nothing while a guest packet is handled. Bundles need "hooks": 269.

API 270 adds A server player picked up an arrow after it was removed and its pickup was applied: `native_player_pickup_arrow(player, name, entity_id, item)` gives the pickup item id. Bundles need "hooks": 270.

API 271 adds A server player about to travel through a Nether portal, End portal or End gateway: `native_player_portal(player, name, from_dimension, to_dimension, cause, cancel)` supplies the source and target dimension ids; `cause` is `nether_portal`, `end_portal` or `end_gateway`. Set `cancel` to leave the player where they are. The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing while a guest tick is handled. Bundles need "hooks": 271.

API 272 adds A server player requests a known recipe be placed in their crafting grid through the recipe book (`native_player_recipe_book_click(player, name, recipe, all)`). Bundles need "hooks": 272.

API 273 adds A recipe about to become known to a server player: `native_player_recipe_discovered(player, name, recipe, cancel)` is heard for advancement rewards, knowledge books and `/recipe give`. Set `cancel` to keep it locked; the client is sent the matching removal. The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing while a guest packet is handled. Bundles need "hooks": 273.

API 274 adds A server player launches with a riptide trident: `native_player_riptide(player, name)` fires after the velocity and spin are applied. The player's state is swapped in; helpers that look them up through `self.lan` may find nothing during a guest packet. Bundles need "hooks": 274.

API 275 adds A server player about to shear a sheep, mooshroom, snow golem or bogged. `off_hand` says which hand holds the shears; set `cancel` to leave the mob and tool unchanged. Bundles need "hooks": 275.

API 276 adds A server player's shield was disabled by a blocked melee hit: `native_player_shield_disable(player, name, ticks)` fires immediately before the shield cooldown is set. Bundles need "hooks": 276.

API 277 adds A server player's respawn point about to change: `native_player_spawn_point_set(player, name, dimension, pos, cause, cancel)` is heard for `bed`, `respawn_anchor`, `command` and `death_reset`; `pos` is the block position. Set `cancel` to keep the old point. A canceled `death_reset` keeps the old point even if its bed or anchor is gone. The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing while a guest packet is handled. Bundles need "hooks": 277.

API 278 adds A server player selected stonecutter recipe button `recipe`, after the menu accepted it: `native_player_stonecutter_recipe_select(player, name, recipe)`. Bundles need "hooks": 278.

API 279 adds A spectator's camera returned to their own player after they stopped watching an entity: `native_player_stop_spectating_entity(player, name, entity_id)` gives the former target. Bundles need "hooks": 279.

API 280 adds A server player swapped their main hand with an equipment slot: `native_player_swap_with_equipment_slot(player, name, slot)` fires after both hands changed; `slot` is currently `offhand`. Guest lookup helpers may find nothing during packet handling. Bundles need "hooks": 280.

API 281 adds A player is about to travel through an End gateway (`native_player_teleport_end_gateway(player, name, gateway, cancel)`). Set `cancel` to leave them at the entrance without starting the gateway cooldown. Bundles need "hooks": 281.

API 282 adds A server player used a golden dandelion on an ageable mob: `native_player_toggle_entity_age_lock(player, name, entity_id, locked)` fires after the lock and item use changed. Bundles need "hooks": 282.

API 283 adds A server player completed a villager or wandering trader trade, after the merchant menu applied the trade: `native_player_trade(player, name, entity_id)` gives the merchant's network id. Bundles need "hooks": 283.

API 284 adds A server player's action removed an entity's leash: `native_player_unleash_entity(player, name, entity_id)` fires after the leash state is cleared. Bundles need "hooks": 284.

API 285 adds The server synced changed motion to a player: `native_player_velocity(player, name, velocity)` fires with velocity in blocks per tick. Guest lookup helpers may find nothing during packet handling. Bundles need "hooks": 285.

API 286 adds `native_portal_create(dimension, pos)` is told after a Nether portal for dimension travel or the End arrival platform has been built, with its dimension and lower corner or platform origin. Bundles need "hooks": 286.

API 287 adds A thrown splash or lingering potion finished applying its hit effects; the hook reports the potion item and impact position. Experience bottles are excluded. Bundles need "hooks": 287.

API 288 adds An anvil result was prepared: `native_prepare_anvil(player, name, pos, result, cost)` is called for a server player at `pos`; change the result or level cost before it is sent to the menu. Guest state is not swapped in. Bundles need "hooks": 288.

API 289 adds A grindstone result was prepared: `native_prepare_grindstone(player, name, pos, result)` is called for a server player at `pos`; change the result before it is sent to the menu. Guest state is not swapped in. Bundles need "hooks": 289.

API 290 adds A crafting-grid result was prepared: `native_prepare_item_craft(player, name, pos, result)` is called for the inventory grid (None) or a table at `pos`; change the result before it is sent to the menu. Guest state is not swapped in. Bundles need "hooks": 290.

API 291 adds Enchanting offers were calculated: `native_prepare_item_enchant(player, name, pos, costs)` is called for a server player at `pos`; change the three costs before they are sent to the menu. Guest state is not swapped in. Bundles need "hooks": 291.

API 292 adds A smithing result was prepared: `native_prepare_smithing(player, name, pos, result)` is called for a server player at `pos`; change the result before it is sent to the menu. Guest state is not swapped in. Bundles need "hooks": 292.

API 293 adds A projectile was added to the server level: `native_projectile_launch(player, name, entity, kind)` reports the entity id and type, plus the player launcher when there is one. Bundles need "hooks": 293.

API 294 adds A raid wave has spawned at its position, with its raid id, wave number and raider count (riders' mounts not counted). Bundles need "hooks": 294.

API 295 adds A raid is triggered or reaches `victory`, `loss` or `stopped`: `native_raid_state_change(raid, pos, state)` reports its id, center and new state. Bundles need "hooks": 295.

API 296 adds `native_sculk_bloom(dimension, pos)` is called after a sculk catalyst at `pos` takes experience and enters the blooming state. Bundles need "hooks": 296.

API 297 adds `native_sponge_absorb(dimension, pos)` is called after a sponge at `pos` has absorbed water and become wet. Bundles need "hooks": 297.

API 298 adds A guest's server command completion request was evaluated: `native_tab_complete(player, name, command, range, suggestions)` lets a mod add or filter the suggestions; `range` is the replaced byte range. The guest's state is not swapped in, so helpers that look through `self.lan` may find nothing while its packet is handled. Bundles need "hooks": 298.

API 299 adds `native_target_hit(dimension, pos, strength)` is called after a projectile hits the target block at `pos` and it takes the reported redstone output strength. Bundles need "hooks": 299.

API 300 adds The server's `/weather` command is about to change thunder: `native_thunder_change(dimension, thundering, cancel)`. Set `cancel` and the previous thunder state and timer remain. Bundles need "hooks": 300.

API 301 adds The overworld clock is about to jump after sleep or a `/time` command: `native_time_skip(dimension, from, to, cause, cancel)`. Set `cancel` to keep the old time. Bundles need "hooks": 301.

API 302 adds A player selected a trade: `native_trade_select(player, name, villager, index)` is heard after the trading menu updates; `villager` is the entity id when known. The player's state is not swapped in. Bundles need "hooks": 302.

API 303 adds `native_vault_change_state(dimension, pos, state)` is called after a vault at `pos` changes to `state` (`inactive`, `active`, `unlocking`, or `ejecting`). Bundles need "hooks": 303.

API 304 adds The server's `/weather` command is about to change its rain and thunder parameters: `native_weather_change(dimension, raining, thundering, cancel)`. Set `cancel` and the prior weather and timers remain. Bundles need "hooks": 304.

API 305 adds The dedicated server's whitelist changed state: `native_whitelist_state_update(enabled)` reports its final on/off state, after `/whitelist on` or `/whitelist off` from a player or console. Bundles need "hooks": 305.

API 306 adds The server completed a world save cycle: `native_world_saved(wait)` reports whether chunk writes were flushed before returning. Bundles need "hooks": 306.

API 307 adds energy held by items (`Capabilities.Energy.ITEM`). `native_item_energy_buffer(item, buffer)` gives a mod item a buffer as `SimpleEnergyHandler` is built: its capacity and the most one insert and one extract move (a capacity under 1 leaves it out; the limits are clamped to 0..=capacity). The amount lives in the stack's `minecraft:custom_data` as the int `horizon_energy`, so it is saved and sent with the stack; an empty item carries nothing, so it stacks with a new one. `native_item_energy(stack)` reads one item of a stack (amount, capacity, limits); `native_item_energy_insert(stack, amount)` and `native_item_energy_extract(stack, amount)` answer the amount one item takes or gives and the single item it becomes, as `EnergyHandler.insert`/`extract` through an `ItemAccess`; the stack given is not changed, so the caller shrinks it by one and hands out the result, or leaves the result unused to simulate. They work on any side. Bundles need "hooks": 307.

API 308 adds the time players wake at after sleeping, as NeoForge's `SleepFinishedTimeEvent`: `native_sleep_finished_time(dimension, current, new_time)` runs on the server once enough players have slept and the `advance_time` rule is on, before the clock moves. `new_time` comes in as the next morning and a mod may change it; a time before `current` is refused. `native_time_skip` then hears the jump to the chosen time. Bundles need "hooks": 308.

API 309 adds a shift-click in a mod container's menu routed by the mod, `AbstractContainerMenu.quickMoveStack`: `native_menu_quick_move(pos, entity, container_slots, slot, stack, handled)` runs on the server before the game's own routing, for a mod container on a vanilla menu type or its own menu type. The mod takes what it moves out of `stack` and sets `handled`; the slot keeps the rest and the client gets the menu again. Bundles need "hooks": 309.

API 310 adds a server player's trade with a villager or a wandering trader, as NeoForge's `TradeWithVillagerEvent`: `native_trade_with_villager(merchant, kind, offer)` is heard once per trade made, after its statistic and `villager_trade` trigger, with the trader's entity id and type and the `MerchantOffer` as it is after the trade. Listen only; the player's state is swapped in. Bundles need "hooks": 310.

API 406 adds `native_merchant_offer_traded(merchant, kind, offer)`, asked once per trade made with the live `MerchantOffer` in the state the trade left it in. A mod may change `uses`, `max_uses`, `demand`, `special_price_diff`, `price_multiplier` and `xp` (Bukkit's `PlayerTradeEvent#getTrade`, `Merchant` recipes): the next click and the offers sent to the player see the change, so a mod can keep a trade from running out or from getting dearer. The unchanged `trade_with_villager` still hears the offer as the trade left it. A shift-click's trades are asked for after the click. Bundles need "hooks": 406.

API 311 adds a server player's `teleport_randomly` consume effect (chorus fruit) is about to try a spot: `native_chorus_fruit_teleport(player, name, from, target, cancel)`, once per attempt (up to 16). A mod may move `target`; the game still looks down from it for ground to land on. `cancel` ends the effect with no teleport and no more attempts. Foxes are not heard. Bundles need "hooks": 311.

API 312 adds a server player's ender pearl landed: `native_ender_pearl_teleport(player, name, pearl, from, target, damage, cancel)` runs before the endermite roll and the teleport. A mod may move `target` or change the fall `damage` (5; 0 or less is none); `cancel` keeps the player where they are, without the endermite roll, damage or sound, and the pearl is used up either way. Bundles need "hooks": 312.

API 313 adds drawing an item stack on a screen: `native_item_stack(batch, stack, at, size)` draws its model, glint, damage bar, cooldown and mod decorations, but not its count. It draws nothing and returns false for an unknown item or an empty stack (air, or a count of 0). Bundles need "hooks": 313.

API 314 adds hiding a player (a vanish): `native_hide_player(uuid, level)` hides that player, from the next tick, from the players below permission `level` (None shows them again): they leave those players' Tab list and world (their clients stop tracking them, as they do a spectator) and those players don't read them joining or leaving. Players of `level` or more still see them; a player made an operator sees them again. It is kept by UUID, for a player who is off too, until a mod sets it again or the server stops. Server only; false on a client. Bundles need "hooks": 314.

API 315 adds a player's item breaking: `native_player_destroy_item(player, name, item, slot)` is heard for a server player whose tool, shield, rod or armour piece ran out of durability (`LivingEntity.onEquippedItemBroken`, Bukkit's `PlayerItemBreakEvent`), with the item that broke and the slot it was in (`mainhand`, `offhand`, `head`, `chest`, `legs`, `feet`, `body`, `saddle`); armour has a slot rather than a hand. `native_item_damaged` is the wear, this is only the break. Listen only. The player's state is swapped in. Not heard for a creative player or for the host of a LAN world. Bundles that use it need `"hooks": 315`.

API 316 adds A player's damaging melee hit on a living target: `native_critical_hit(target_kind, vanilla_critical, critical, damage_multiplier)` may force or deny the critical hit and change its multiplier (1.5 for a vanilla crit, else 1.0); the sweep, sound and particles follow the decided result. Not fired for a dragon part or a hit that does no damage. Bundles need "hooks": 316.

API 317 adds attachment reads that never insert the default: `AttachmentType::has` says whether a holder carries a value, saved or set,
even before it decodes (NeoForge's `hasData`), and `AttachmentType::get_existing` returns `None` instead of the default
(NeoForge's `getExistingData`). Neither adds anything to the holder's saved data. Bundles that use them need `"hooks": 317`.

API 318 adds A player releases a bow: `native_arrow_loose(stack, has_ammo, charge, canceled)` may change held-tick `charge` or cancel the shot; negative charge also prevents it. Unlike `native_living_entity_use_item_stop`, it does not skip the whole item release; `native_entity_shoot_bow` reports the spawned arrow afterward with read-only force. Fires on the Game running the bow release, including LAN guests; a remote client's own release path, crossbows, and ArrowNockEvent are not covered. Bundles need "hooks": 318.

API 319 adds an item frame or painting about to break: `native_hanging_break(player, kind, pos, cancel)` is heard before a server player's hit, a blast or the loss of its wall breaks an item frame, a glow item frame or a painting, with the hitting player (None for a blast or a lost wall). A mod sets `cancel` and it stays. The hitting player's state is swapped in. A hit that only takes a frame's item out, the game's own player and `/kill` are not asked. Bundles that use it need `"hooks": 319`.

API 320 adds A criterion of a server player's advancement was granted or revoked after its progress changed and before any completion rewards, except the `minecraft:tick` criteria granted on a network player's first tick or a reload, which are heard after that tick's rewards and packets; `advancement` is its identifier, such as `minecraft:story/mine_stone`, `criterion` the criterion name and `granted` is false for a revoke. A grant or revoke that changes nothing is not heard. Helpers looking up the guest through `self.lan` may find nothing during packet handling. Bundles need "hooks": 320.

API 321 adds A statistic about to be awarded to a server player: `native_player_stat_award(player, name, stat_type, stat, value, cancel)`, with `stat_type` such as `minecraft:mined` or `minecraft:custom`, `stat` such as `minecraft:stone` or `minecraft:walk_one_cm`, and `value` the amount added. Set `cancel` to award nothing: the count and its scoreboard criteria stay as they were. Helpers looking up the guest through `self.lan` may find nothing during packet handling. Bundles need "hooks": 321.

API 322 adds a player's data copied to their new player: `native_player_clone(player, name, was_death)` is heard from `ServerPlayer.restoreFrom` once for each death respawn (`was_death` true) and for each exit from the End after the credits (`was_death` false), after the retained data is copied and before the new player is placed (NeoForge's `PlayerEvent.Clone`). The uuid stays the same, so `player` names both the old and the new player. It cannot change or stop anything; no player's state is swapped in. Bundles that use it need `"hooks": 322`.

API 323 adds a player loaded from the world's save: `native_player_load_from_file(player, name, directory)` is heard once for each join (the owner of an integrated server too), after the player's `.dat` is read and applied and before they are placed in a level (NeoForge's `PlayerEvent.LoadFromFile`, `PlayerList.loadPlayerData`). `directory` is the world's player directory (`players/data`); a mod reads its own file there, named `<uuid>.<suffix>`. It cannot change or stop anything; no player's state is swapped in. Bundles that use it need `"hooks": 323`.

API 324 adds a player saved to the world's store: `native_player_save_to_file(player, name, directory)` is heard once for each save of a placed player (when they leave or the world closes, when the dedicated server saves everyone, and for an integrated server's owner when the world saves them; LAN guests are not heard on integrated autosaves) after their data is written (NeoForge's `PlayerEvent.SaveToFile`, `PlayerList.save`). `directory` is the world's player directory (`players/data`); write your own file there, named `<uuid>.<suffix>`, never the player's `.dat`. The player may be leaving, so no player's state is swapped in. It cannot change or stop anything. Bundles that use it need `"hooks": 324`.

API 325 adds `native_item_expire(entity, stack, extra_life)`, a dropped item reaching its lifetime on the server (NeoForge's `ItemExpireEvent`, vanilla `ItemEntity.tick`): every mod may set or add to `extra_life`, in ticks from zero; a positive total keeps the item that much longer, zero or less lets it expire. Extensions are capped at 32766 ticks. Bundles need "hooks": 325.

API 326 adds the title's own screens to the screen hooks. `NativeScreenView.id` gains one id per title state (`title`, `world_select`, `world_create`, `world_create_flat`, `world_create_buffet`, `world_edit`, `world_backup`, `world_backup_confirm`, `world_optimize_confirm`, `world_optimize`, `low_disk_space`, `world_delete`, `world_recover`, `world_recovered`, `error`, `multiplayer`, `add_server`, `edit_server`, `direct_connect`, `server_delete`, `friends_join`, `disconnected`, `multiplayer_warning`, `onboarding`, `ban_notice`), with no `menu`, no `title` and no `slots`, and the whole window as `rect`. `native_screen_render_post` fires after the title draws its screen (before its toasts), and the `_pre` input hooks — mouse buttons, drag, wheel, key press and release, character typed — fire on it and can cancel it as on any screen, so a mod draws its own button or browser over Create New World and takes the clicks. A dialog, a prompt or a server's pack prompt over the title keeps them away, and typing still goes to the title's own boxes (the key's text stays on the key there, as on Options, so `native_screen_char_typed_pre` does not fire from the keyboard on the title's own screens; the control socket's `type` now types into them too). `native_instance_dir()` is the instance folder, where `options.txt`, `saves/`, `resourcepacks/`, `shaderpacks/` and `horizon-mods/` are; a world's `datapacks/` is in that world's folder. Bundles that use them need `"hooks": 326`.

API 327 adds A player took an anvil's result: `native_anvil_craft_post(player, name, pos, craft)` (NeoForge's `AnvilCraftEvent.Post`) is heard after the levels, the inputs and the anvil wear are done, so a broken anvil is already air. `craft` holds the inputs and the result as they were before. Not heard when `native_anvil_craft_pre` cancelled. Bundles need "hooks": 327.

API 328 adds A player took an anvil's result: `native_anvil_craft_pre(player, name, pos, craft, cancel)` (NeoForge's `AnvilCraftEvent.Pre`) runs before the levels are charged, the inputs used up and the anvil worn. `craft` holds the left and right inputs and the result as they were before. Set `cancel` to refuse the take and skip the matching `native_anvil_craft_post`: the result stays in its slot and the inputs, levels, inventory and cursor are as before the click. Bundles need "hooks": 328.

API 329 adds A server player stopped digging `pos` before the block broke: `native_block_damage_abort(player, name, pos)` is heard after the server clears the dig and sends the block state back to the client. The player's state is not swapped in. Bundles need "hooks": 329.

API 330 adds A mob is about to drop `count` of `item` at `pos`: `native_entity_drop_item(entity, item, count, pos)`. Bundles need "hooks": 330.

API 331 adds A mob's leash was removed, before any lead drop is handled: `native_entity_unleash(entity, kind)`. Bundles need "hooks": 331.

API 332 adds A server explosion is about to begin at `center` with `radius` (`native_explosion_prime`); the hook is listen-only and the explosion proceeds as usual. Bundles need "hooks": 332.

API 333 adds A hopper selects a container while searching for items: `native_hopper_inventory_search(hopper, container, entity)` identifies the hopper and the selected block container or container entity. Bundles need "hooks": 333.

API 334 adds A hopper is about to move one item between containers: `native_inventory_move_item(source, destination, item, cancel)` runs before the source slot is changed, a composter's bone meal included; a chest boat or minecart is reported at the block it is in. Set `cancel` to keep the item in its source slot. Bundles need "hooks": 334.

API 335 adds A dropped item entity reached its 6000-tick lifetime and is about to be removed: `native_item_despawn(entity, item, count, pos)`. Bundles need "hooks": 335.

API 336 adds A boat or minecart joined the level at `pos`, after a player or dispenser placed it: `native_vehicle_create(kind, pos)` is listen-only. Bundles need "hooks": 336.

API 337 adds A player is about to damage a boat or minecart: `native_vehicle_damage(kind, damage, cancel)` may change the damage or cancel it before the vehicle's damage and wobble change; the attack still completes. Bundles need "hooks": 337.

API 338 adds A boat or minecart is about to be destroyed at `pos`: `native_vehicle_destroy(kind, pos)` is heard just before it is removed; TNT minecarts that prime instead are not heard. Bundles need "hooks": 338.

API 339 adds A server player earned one criterion of an advancement, after its progress was recorded: `native_player_advancement_criterion_grant(player, name, advancement, criterion)`. Bundles need "hooks": 339.

API 340 adds A guest's arm swing: `native_player_arm_swing(player, name, hand)` is called after the server handles the guest's `Punch` packet and resets attack strength. The current packet swings `mainhand`; the guest's state is swapped in. Bundles need "hooks": 340.

API 341 adds A server player selected the item at `index` in a bundle in menu `slot`: `native_player_bundle_item_select(player, name, slot, index)`. Bundles need "hooks": 341.

API 342 adds A server player's client finished loading the level around them after joining, respawning or changing dimension: `native_player_client_loaded_world(player, name)`. Bundles need "hooks": 342.

API 343 adds A guest's player input flags: `native_player_input(player, name, flags)` is called after a loaded guest's `PlayerInput` packet is applied to the hub. `flags` is the packet's bit field; the guest's state is swapped in. Bundles need "hooks": 343.

API 344 adds A guest jumped: `native_player_jump(player, name)` is called after accepted movement made the server player jump from the ground, once the guest's movement simulation finished. The guest's state is swapped in. Bundles need "hooks": 344.

API 345 adds A guest was kicked from the server: `native_player_kicked(player, name, reason)` is called after the hub records the rendered reason and sends the disconnect. The guest's state is swapped in. Bundles need "hooks": 345.

API 346 adds A LAN guest launched a snowball, egg, ender pearl, potion or trident: `native_player_launch_projectile(player, name, item)` runs after the projectile is created. Bundles need "hooks": 346.

API 347 adds A server player's experience level changed: `native_player_level_change(player, name, from, to)` is called after experience points or levels change their level, including orb points. The player's state is swapped in when they are a guest. Bundles need "hooks": 347.

API 348 adds A server player released a drawn bow with `native_player_ready_arrow(player, name, arrow)`: the name of the selected projectile item. Bundles need "hooks": 348.

API 349 adds A server player's recipe book open or filter setting changed: `native_player_recipe_book_settings_change(player, name, book, open, filtering)`, where `book` is its protocol id. Bundles need "hooks": 349.

API 350 adds A server player released the use-item key before the item finished being used: `native_player_stop_using_item(player, name)`. Bundles need "hooks": 350.

API 351 adds A guest's player input changed sneaking: `native_player_toggle_sneak(player, name, sneaking)` is called after the hub applies the new state, only when it changes. The guest's state is swapped in. Bundles need "hooks": 351.

API 352 adds A hopper begins a transfer attempt: `native_prepare_hopper_transfer(pos)` runs after its cooldown and enabled checks, before it pushes or pulls items. Bundles need "hooks": 352.

API 353 adds A vault is about to eject a loot item: `native_vault_loot(pos, item)` supplies the vault position and item identifier before it spawns. Bundles need "hooks": 353.

API 354 adds A timed world border size change finished: `native_world_border_bounds_change_finish()` fires after the border reaches its final size. Bundles need "hooks": 354.

API 355 adds `Game::native_mod_image(width, height, rgba)`: a mod's own RGBA image, uploaded into the HUD atlas slot `horizon/modimage` (1024 by 1024) and drawn with `native_sprite`, whose source rectangle crops and stretches it, so a map or a schematic view needs no texture of its own. The slot is reserved only in a game built with native mods, so a game without mods pays none of its 4 MiB: one whose source horizon-mixins changed (its `.horizon-mixins-stage.json` names a mod; the launcher's builds and a bundle applied by hand or by a mod's own script alike), one with trait mods installed, or one whose `native-mods.json` lists mods. False for an image that is not `width * height` pixels of four bytes, is larger than the slot, or when there is no slot. Each upload is a texture patch of the image, so send one when the image changed, not every frame.
API 356 adds a living entity drowning (`native_living_drown`), with changeable drowning and damage, a read-only bubble count and cancellation. Bundles need "hooks": 356.

API 357 adds a living entity's fall (`native_living_fall`), with changeable distance and multiplier and cancellation. Bundles need "hooks": 357.

API 358 adds a living entity healing (`native_living_heal`), with a changeable amount and cancellation. Bundles need "hooks": 358.

API 359 adds a living entity jumping (`native_living_jump`). Bundles need "hooks": 359.

API 360 adds `native_movement_input(forward, strafe, jump, sneak)` lets client mods change local movement after input is read and before it is applied. Positive strafe is left; servers receive direction flags rather than scalar magnitudes. Bundles need hooks API 360.

API 361 adds a cancellable item frame item render hook (`native_render_item_in_frame`). Bundles need "hooks": 361.

API 362 adds a per-tick player FOV modifier hook (`native_compute_fov_modifier`). Bundles need "hooks": 362.

API 363 adds Read the most recently extracted world camera with `native_camera()` (none before a world frame): position, interpolated eye height, partial tick, yaw, pitch, final FOV, near/far planes, dimension and column-major matrices. The view is camera-relative and includes bob and spin; the projection uses reversed Z. Earlier callbacks can see the previous frame. `native_block_collision(pos)` returns block-local boxes using the empty collision context; unloaded blocks and collision errors yield no boxes. Bundles need "hooks": 363.

API 364 adds the render hook point `native_hand_drawn(hand)`, after the first-person hand and held item are drawn, with the
frame's target, the hand's quads and its clip matrix (`HandDrawn`). Bundles need `"hooks": 364`.

API 365 adds `native_screen_closing` receives a NativeScreenView, observed at most once per frame, including sign, book, dialog and prompt overlays; cancelling an opening is not possible because transitions do not share a single choke point. Bundles need hooks version 365.

API 366 adds `native_screen_init_post` receives a NativeScreenView, observed at most once per frame, including sign, book, dialog and prompt overlays; cancelling an opening is not possible because transitions do not share a single choke point. Bundles need hooks version 366.

API 367 adds `native_screen_opening` receives a NativeScreenView, observed at most once per frame, including sign, book, dialog and prompt overlays; cancelling an opening is not possible because transitions do not share a single choke point. Bundles need hooks version 367.

API 368 adds tooltip box colours (`native_tooltip_color`): the background and the two border colours, the colour half of what NeoForge 26.3 `RenderTooltipEvent.Texture` decides. Its sprite textures are not offered: Horizon draws these boxes as solid fills. Bundles need "hooks": 368.

API 369 adds `native_render_hand_pose` adjusts the first-person hand, item, maps and empty main-hand arm. Bundles need "hooks": 369.

API 370 adds items held by items (`Capabilities.Item.ITEM`): backpacks, storage cells, toolboxes. `native_item_container(item, slots)` gives a mod item its slots (clamped to 1..=256). The contents live in the stack's `minecraft:container`, as a shulker box item keeps them, so they are saved and sent with the stack; an empty item carries nothing, so it stacks with a new one. `native_item_container_slots(stack)` reads one item of a stack, empty slots included; `native_item_container_insert(stack, item)` puts in as much of `item` as fits, topping up equal stacks first and then the first empty slots (`SimpleContainer.addItem`'s order), no slot past its limit (99, or the item's own when less), and never a shulker box (`Item.canFitInsideContainerItems`); `native_item_container_extract(stack, slot, max)` takes up to `max` from a slot. Both answer what moved and the single item it becomes, through an `ItemAccess` as NeoForge's handler acts; the stack given is not changed, so the caller shrinks it by one and hands out the result, or leaves the result unused to simulate. They work on any side. Bundles need "hooks": 370.

API 371 adds `WorldDrawn::motion` in `native_world_drawn`: which mob, player or dropped item each run of the frame's entity
quads is, by its entity id, recorded every frame, so a mod can match an entity's vertices with last frame's. Bundles
that read it need `"hooks": 371`.

API 372 adds a name tag hook (native_render_name_tag), API 372.

API 373 adds `EntityRef` and `LevelRef`, handles a mod keeps and calls `Host` through: `host.entity(id)` and `host.level(dimension)` make them, and `entity.position(host)`, `block_position`, `rotation`, `kind`, `uuid` and `level` read an entity. Bundles need "hooks": 373.

API 374 adds A server explosion calculates an entity's added velocity: `native_explosion_knockback(dimension, center, blocks, entity, knockback_velocity)` can replace it. Bundles need "hooks": 374.

API 375 adds A player is about to nock a bow: `native_arrow_nock(player, name, bow, hand, has_ammo, cancel)` is asked where `BowItem.use` would start using the item. `has_ammo` is whether `getProjectile` found ammunition (creative always has). Set `cancel` to make the use fail, as `InteractionResult.FAIL`. Crossbows and tridents do not ask. Bundles need "hooks": 375.

API 376 adds The ammunition `LivingEntity.getProjectile(weapon)` returns for a player or a mob (skeleton, illusioner, pillager, piglin): `native_living_get_projectile(player, name, entity, kind, weapon, projectile)` hands every mod the event with `projectile` set to the vanilla ammunition (the held or inventory stack, a mob's held projectile or a new arrow, a creative player's default arrow, empty when a survival player has none). A mod reads it, and sets or empties it. It is asked for the bow's start and release, the crossbow's start and draw, a skeleton's or illusioner's shot, a pillager's or piglin's crossbow load, and the client's prediction of the use. A replaced stack is not taken from the inventory: the hook owns it. A mob's shot uses the stack's arrow item and effect; an emptied stack leaves a mob's crossbow uncharged. `entity` is the mob's id, empty for a player. Bundles need "hooks": 376.

API 377 adds a mod's own ingredient kind matching a concrete stack at craft time. The ingredient's test calls native_ingredient_test(kind, data, stack, accept) with the kind and JSON the load-time hook saw, the stack being matched and the verdict, already true (the load-time list named the item). Set accept to false to refuse the stack - its components are the usual reason (ICustomIngredient.test). The hook may be called many times a craft; the answer must be the same for the same stack. Bundles that use it need "hooks": 377.

API 378 adds Declare a category id, custom recipe type and icon item with `native_recipe_book_categories`; read the validated mapping with `native_recipe_book_category`. This first slice does not add book tabs, displays or unlock synchronization. Bundles need hooks version 378.

API 379 adds `entity.velocity(host)`, `set_velocity`, `on_ground`, `bounding_box`, `eye_height`, `eye_position`, `view_vector` and `distance_to_sqr`. Bundles need "hooks": 379.

API 380 adds [block textures](#block-textures) for client mods: `native_block_sprites(blocks)` says what each block draws on each side (each face's sprite, place, UV map and tint source), `native_texture_sprites(textures)` a texture's sprite, `native_block_sprites_version()` changes when the resources load anew, and `WorldDrawn::block_sprites` holds the arrays with a nearest and the terrain's sampler. Bundles need "hooks": 380.

API 381 adds `level.block_state(host, pos)`, `block`, `block_property`, `is_loaded`, `height`, `game_time` and `day_time` read any level the server has loaded. Bundles need "hooks": 381.

API 382 adds `LivingRef`, made with `host.living(entity)`: `living.equipment(host, EquipmentSlot)`, `hand_item(host, Hand)` and `set_equipment(host, slot, stack)`. Bundles need "hooks": 382.

API 383 adds living_knockback_v2: raw strength before resistance, attacker network id (0 if unknown), the attacker's held main-hand and off-hand stacks (a player's selected and off-hand stacks, a mob's main hand; none when unknown), and armor-stand sweeps. The unchanged v1 hook runs first on its existing paths; cancellation from either wins. Bundles need hooks API 383.

API 384 adds `native_living_incoming_damage_v2`: a host-aware damage hook for a player's melee, sweep and spear hits on mobs, including LAN guests. It supplies the attacker/victim network ids, damage type, optional source position, victim position/view vector and the weapon snapshot (read `ItemStack::enchantments()` for ids and levels). Mods can change damage, cancel delivery and use Host helpers, including sounds. It fires after the victim's invulnerability check (an invulnerable or dead victim fires nothing) and before v1; v1 then sees the changed amount unless v2 canceled. Projectiles, player victims, dragons, mob attacks and environmental damage still only fire v1. No extra work or allocation when no mods are loaded. This is additive, like `menu_button_v2`, not a signature change to v1. Bundles need "hooks": 384.

API 385 adds per-stack conditional item model properties (`native_item_model_property`). Bundles need "hooks": 385.

API 386 adds `entity.nbt(host)`, `merge_nbt`, `tags`, `add_tag`, `remove_tag` and `name`. Bundles need "hooks": 386.

API 387 adds On the authoritative server, not in client prediction, a player tunes a note block: `native_note_block_change(context, new_note, cancel)`. The context holds the dimension, position, instrument and old note. `new_note` starts as vanilla's cycled note; values outside 0–24 keep vanilla's. Cancel leaves the note unchanged and prevents playback. Bundles need "hooks": 387.

API 388 adds `native_potential_spawns(dimension, category, pos, entries, canceled)`, run on the server when natural spawning builds its possible spawn list (NeoForge's `LevelEvent.PotentialSpawns`, vanilla's `NaturalSpawner.mobsAt`). Mods may add or remove entries; canceling empties the list. Bundles need "hooks": 388.

API 389 adds `native_village_siege(dimension, player, attempted_pos, canceled)`, run on the server when a village siege finds a spawn position (NeoForge's `VillageSiegeEvent`, vanilla's `VillageSiege.tryToSetupSiege`). `player` is the player's UUID, `attempted_pos` is the found position, and the siege is the dimension's singleton. Set `canceled` to stop the siege. Bundles need "hooks": 389.

API 390 adds a mob that converted into another type, as NeoForge's `LivingConversionEvent.Post`: `native_living_conversion_post(entity, kind, converted, converted_kind)` runs on the server after the new mob joined. Bundles need "hooks": 390.

API 391 adds a mob about to convert into another type, as NeoForge's `LivingConversionEvent.Pre`: `native_living_conversion_pre(entity, kind, outcome, cancel)` runs on the server before `Mob.convertTo` changes anything. Cancelling keeps the mob as it is. Bundles need "hooks": 391.

API 392 adds a living entity about to die, as NeoForge's `LivingDeathEvent`: `native_living_death(entity, kind, damage_type, attacker, cancel)` runs on the server when a mob's or a server player's health has run out, before the death runs (loot, experience, death message). Cancelling keeps the entity alive at 1 health. Bundles need "hooks": 392.

API 393 adds the problem that stops a player sleeping, as NeoForge's `CanPlayerSleepEvent`: `native_can_player_sleep(player, name, pos, problem)` runs on the server once vanilla's checks have found their first problem (`too_far_away`, `obstructed`, `not_possible_now`, `not_safe`) or none. A mod may set `None` to let the player sleep or a name to refuse (`other_problem` refuses without a message). `native_player_bed_enter` still fires after it and may cancel. Bundles need "hooks": 393.

API 394 adds experience points a player is about to gain, as NeoForge's `PlayerXpEvent.XpChange`: `native_player_xp_change(player, name, amount, cancel)` runs on the server at `Player.giveExperiencePoints`: orb pickups (after mending), advancement rewards and `/xp add ... points`. A mod may change `amount` or cancel. `native_player_exp_change` stays: it hears orb pickups only, and cannot cancel. Bundles need "hooks": 394.

API 395 adds two animals breeding, as NeoForge's `BabyEntitySpawnEvent`: `native_baby_entity_spawn(parent, partner, player, kind, cancel)` runs on the server before the baby is made. `player` is the feeder's UUID (0 for none). Set `cancel` for no baby; the parents still rest and lose their love, but there is no baby, experience, statistic or advancement. Not fired for turtles, frogs, sniffers or villagers. Bundles need "hooks": 395.

API 396 adds a slime or magma cube about to split into smaller ones on death, as NeoForge's `MobSplitEvent`: `native_mob_split(parent, kind, size, child_size, count, cancel)` runs on the server for a parent above size 1 after vanilla drew the child count. A mod may change `count` (0 to 64) or set `cancel` to spawn no children. Bundles need "hooks": 396.

API 397 adds mod config files, loaded and reloaded (`ModConfigEvent`). `native_register_configs` lists a mod's configs: `NativeConfig::new(mod_id, kind, default)`, a file `config/<mod id>-<kind>.toml` (`.file(..)` names another inside `config/`) written from `default` when missing. `Client` configs load on the window when the game starts, `Common` once per process (the window, or a dedicated server), `Server` on the server before `native_server_about_to_start` for each world, read from the world's `serverconfig/<file>` when there is one. `native_config_loading` hands over the file's text (`NativeConfigFile`: mod id, kind, file, path, text); the mod parses it, and checks `mod_id`, since every mod hears every config. The game looks at the files' modification times once a second and fires `native_config_reloading` with changed text; the same text saved again fires nothing. `native_config_unloading` drops a server's configs when it stops. Bundles need "hooks": 397.

API 398 adds `NativeServerPlayer.entity_id` is the network entity id the player has this session (0 when not known), so hooks that only give entity ids (`living_incoming_damage`, `living_knockback`) can be matched to a player. Bundles need "hooks": 398.

API 399 adds A player's chat message is about to be sent to everyone: `native_player_chat(player, name, message, replies, cancel)` is heard once for each message a player sends, after the secure-chat chain and the chat filter took it and before anyone sees it. Set `cancel` to drop it for everybody; push lines to `replies` and they go to the sender as system chat (the sender is not told otherwise). The text cannot be changed, a signed message would no longer verify. Bundles need "hooks": 399.

API 400 adds a mod's own data folders (`AddServerReloadListenersEvent`). `native_add_server_reload_listeners` lists a mod's listeners: `NativeReloadListener::json(id, folder)`, the `.json` files of `data/<namespace>/<folder>/` in every namespace, as vanilla's `SimpleJsonResourceReloadListener` reads its folder. On every data load of a server (a world opening, `/reload`), before `native_server_data_loaded`, `native_server_reload_listener_apply` hands over each listener's files: `<namespace>:<path>` (without `.json`) and the parsed JSON, sorted by id, from the resource root (vanilla's and content mods' data) and the world's enabled data packs, the top pack's file for each id. A file that isn't JSON is logged and left out; a folder that leaves `data/<namespace>/` is skipped. Every mod hears every listener, so check `listener.id`. Bundles need "hooks": 400.

API 401 adds a mod menu type opened with extra data (NeoForge's `openMenu` with an extra-data writer). When a mod block opens its content mod's menu type for a player, `native_menu_extra_data(player, menu, block, pos, data)` asks on the server for bytes of the mod's own (up to 32 KiB; the first mod to set `data` wins). A Horizon client gets them with the block's position just before the menu opens; while the mod's screen is open, `native_screen_pos()` gives the block the server named (not the last clicked one) and `native_screen_extra_data()` the bytes, empty when the mod wrote none. Bundles need "hooks": 401.

API 402 adds `native_camera_pose(partial, position, yaw, pitch)` (client) runs every world frame after vanilla placed the camera (first or third person, a spectated mob, a minecart), with its position and its yaw and pitch in degrees. Set them to draw the world from anywhere: a cinematic, replay or film camera. The picture, culling, the sound's listener and what faces the camera (particles, item sprites, block entity text) follow it; the player, where they aim, the first-person hand (cancel it with `native_render_hand`) and which chunks load stay the player's. Values that aren't finite keep vanilla's. The field of view is `native_compute_fov`'s. Not called on the title screen's panorama. Bundles need "hooks": 402.

API 403 adds A chat line from the server is about to show, once Chat Delay lets it through: `native_client_chat_received(system, overlay, sender, chat_type, message, cancel)`, NeoForge's `ClientChatReceivedEvent`, for system messages (`overlay` for the action bar), player chat and disguised chat. `sender` is 0 for system and disguised messages; `chat_type` is the chat type's registry name. Replace `message` to show another line, or set `cancel` to hide it. Bundles need "hooks": 403.

API 404 adds `native_send_chat(message)` sends chat, or a command with a leading `/`, as the chat screen would, to any server; an error when not in a server's play state, for an empty message, or when the Chat option blocks it. Bundles need "hooks": 404.

API 405 adds a dying player's inventory to `native_loot_drops`: after vanishing-curse items are gone and before the rest is thrown, naming the player (`entity` the player's id and `minecraft:player`, no table). A mod that takes every stack out of `drops` keeps the inventory. Not heard with `keep_inventory`. Bundles that rely on it need `"hooks": 405`.

API 406 adds a trade's live offer, for the mods to change: `native_merchant_offer_traded(merchant, kind, offer)` is asked once per trade made with the live `MerchantOffer` as the trade left it (`uses` counted). A mod may change `uses`, `max_uses`, `demand`, `special_price_diff`, `price_multiplier` and `xp`; the next click and the offers sent to the player see it. Bundles need "hooks": 406.

API 407 adds `NativeServerPlayer.address` is the IP address the player connects from, without the port (behind a Velocity proxy, the address the proxy forwards; empty when not known), for per-IP limits and sessions. Bundles need "hooks": 407.

API 408 adds reading the server's play packets on the client (`native_client_packet`). Bundles need "hooks": 408.

API 409 adds `native_send_packet(packet)` sends any serverbound play packet (`ServerboundPlayPacket`), a plugin channel's custom payload included, to any server, unchecked; an error when not in a server's play state. Bundles need "hooks": 409.

API 410 adds `native_gui_render_post(batch, gui_hidden, focused)` draws after the complete normal GUI batch: screens, prompts, toasts, debug overlays, pad cursor and IME included. It fires during gameplay, menus and the title, even with F1; loading/file-upgrade and resource-reload overlays do not fire it. The read-only host's `camera()` is the current frame's world camera (None on the panorama), including bob/spin, FOV and projection. `batch` uses GUI pixels; `gui_hidden` reports F1 and `focused` reports window focus so the mod can choose when its indicator is visible. The existing `hud` and `screen_render_post` hooks retain their drawing order and visibility rules. Bundles need "hooks": 410.

API 411 adds Client-only Host snapshots of the window, connection, entities, interpolated model heads and profile skins; visual rays and block occlusion facts; atlas GUI painting, nine-slice widgets and tooltips; persisted mod key changes and a local action bar. client_start runs before connecting (a window need not exist yet), client_disconnecting before a real connection is discarded (not dimension travel), client_stopping once before final save/exit. menu_background can change a normal menu blur request (None disables it; finite 1..64 only). Bundles need "hooks": 411.

API 412 adds `native_force_chunk(owner, chunk, hold)` holds a chunk of the level in place loaded and ticking for `owner`, as NeoForge's `TicketController.forceChunk`; `native_forced_chunks(owner)` lists what it holds. Tickets aren't saved. Bundles need "hooks": 412.

API 413 adds `native_force_chunk_in(owner, dimension, chunk, hold)`, `native_force_chunk` for a named level, which opens and ticks a level not open yet, so a mod can pre-generate the Nether and the End with nobody there. Bundles need "hooks": 413.

API 414 adds a held world through `native_world_hold`: once a frame a client mod can hold the terrain's picture, all of it or
only inside circles; sections on screen that change are meshed and uploaded but wait beside the ones drawn, and
letting go shows them all on one frame ([Holding the world's picture](#holding-the-worlds-picture)). Bundles need
`"hooks": 414`.

API 415 adds a world warp through `native_world_warp`: once a frame a client mod may give WGSL (`world_warp(pos, section) -> Warped`, and optionally `world_fragment(f) -> Painted` after a line `// [fragments]`) that every terrain vertex, far terrain's tiles included, and entity vertex goes through, with sixteen floats it reads as `warp.params`. While it is on the frame culls nothing by where it was. A frame without one costs nothing more ([World warps](#world-warps)). Bundles need "hooks": 415.

## What the launcher does

**Developing a mod?** Run `horizon-launcher settings native-profile dev` once, or pick "Dev build" in the Mods page's
"Native build" row. `dev` builds the game crate unoptimized: a mod edit rebuilds about 6× faster than with
`performance`, while game logic runs up to about twice as slow. Use it while developing, not for playing;
`performance` stays the default for players and releases (step 5 below).

In the window: install a native mod from the Mods page, into the instance picked on the left ("Installs into
{instance}", shown even with one instance). The first one shows a trust notice once (native mods aren't sandboxed; the
first build downloads Rust and can take 20 to 30 minutes; later builds are faster); `mods install` prints it too. Each
installed native mod's row says "Installed in {instance}" and has **Open folder** (`<instance>/horizon-native-mods/`).
The installed list has a **Native build of {instance}** row for the instance picked on the left, never another's:

- a badge: **Not built** (the version it plays isn't a `native-*` build), **Needs rebuild** (the `mods` in its
  `native-mods.json` differ from those turned on, by id and version) or **Up to date**, with **Build** for the first
  two;
- for a native build, "Plays native-…, built on Horizon {version}" (the `base` of its `native-mods.json`);
- **Rebuild on Horizon {version}** when a newer release or Nightly is installed (`native_mods::update_for`). Build
  stays on the Horizon the instance plays; only that button moves it, and its warning says the build starts from scratch
  and takes longer.

The instance list on the left shows such an instance as "Native build · Nightly 10-28" (its base). The launcher names
the mods, warns in red, and builds only from "I trust these mods: build"; the row shows each step and the error if
one fails, on the instance being built only; one build runs at a time. From the command line:
`horizon-launcher native-build [FOLDER] [--instance NAME] [--update] --trust-native-code`. FOLDER is a bundle, an
unpacked `.hnmod`, or a folder of them; without it, the native mods turned on in the instance. It is not an MCP tool:
trusting native code is the player's decision, never an agent's.

1. **Rust.** The launcher's own toolchain ([`rust_toolchain`](../launcher/src/rust_toolchain.rs)): Rust 1.99.1's
   official `rustc`, `cargo` and `rust-std` (and `rust-mingw` on Windows), each pinned by SHA-256, downloaded once
   from static.rust-lang.org into `<launcher data>/rust/`, with its own Cargo home `<launcher data>/cargo/`. No
   rustup, nothing on the system; the player's own Rust is never used. It ships for macOS (Apple Silicon and Intel), Linux
   x86-64, Windows x64 and Windows ARM64. The managed compiler follows the launcher's process architecture. An x64
   launcher running under ARM64 emulation keeps the existing x64 Rust and MinGW toolchain. The game architecture
   setting does not switch the compiler or enable cross-compilation. Windows x64 uses `x86_64-pc-windows-gnu` and
   WinLibs' GCC 16.2.0 on MinGW-w64 14.0.0 (MSVCRT), with its SHA-pinned `mingw64/` folder on `PATH`.

   On Windows ARM64, Rust 1.99.1's native host tools use `aarch64-pc-windows-gnullvm`; the launcher also downloads
   a SHA-pinned LLVM-MinGW 23.1.5 toolchain with UCRT. Rust's `aarch64-pc-windows-msvc` release target cannot be
   reused as a self-contained player toolchain: it needs the MSVC linker and C libraries, while the launcher must
   bring its own C compiler and linker for dependencies such as `ring` and `hidapi` and for Windows imports. The
   selected game's source is rebuilt in full for `aarch64-pc-windows-gnullvm`; the existing MSVC executable is never
   linked into the output. Rust and C code therefore use one Windows ARM64 calling convention and ABI. The
   executable's PE machine is checked before installation. LLVM-MinGW's ARM64 `windres` and archive tools are
   selected for Windows icon resources; the generic resource compiler can emit x64 objects on an ARM64 host.
   Prebuilt MSVC static libraries in a native mod are not compatible. The output uses UCRT, which is included with
   Windows 10 and later. Rust's unwinder is statically linked because Windows does not include LLVM-MinGW's
   `libunwind.dll`; the built game imports only Windows system DLLs.

   Before compiling, the launcher checks that the selected game's installed executable matches its managed compiler.
   The Windows ARM64 launcher builds a selected ARM64 game, and the x64 launcher builds a selected x64 game,
   including when it runs under ARM64 emulation. A selection that does not match the launcher's process architecture
   is rejected, and an ARM64 selection that has only installed the x64 fallback is rejected by the PE check. Install
   a matching game build before building native mods. ARM64EC executables are unsupported. ARM64 Cargo artifacts
   and build records have their own cache namespace; the existing x64 cache layout is unchanged. The linker
   is the system's on macOS (the Command Line Tools: the launcher asks for `xcode-select --install`) and Linux (`cc`).
   On Linux the game also builds against the system's ALSA (`alsa-sys`) and libudev (`libudev-sys`, `hidapi`), found
   with `pkg-config`, whose development files desktop distributions don't install. Before downloading anything the
   launcher checks for `cc`, the C library's headers (`cc` compiles an include of `stdint.h`), `pkg-config`, `alsa`
   and `libudev` and names what is missing, with the install command for the distribution in `/etc/os-release`
   (Debian, Ubuntu and Mint: `sudo apt install build-essential pkg-config libasound2-dev libudev-dev`). SteamOS has
   GCC without the C library's headers, and a read-only system: native builds don't work there yet.
   On Windows, Smart App Control lets only signed programs run, and a build runs programs it has just compiled
   (build scripts, horizon-mixins, the game) and some of the toolchain's own (`dlltool.exe`), which Windows refuses
   with os error 4551 (#5911). No build setting avoids it. A build that hits os error 4551 leads its error with what
   to do: turn it off in Windows Security > App & browser control > Smart App Control settings. Nothing is checked
   before the build: with Smart App Control on, a build whose programs already ran before it was turned on succeeds.
   `horizon-launcher rust status|install|remove`.
2. **horizon-mixins**, at the commit the mods name (their `.hnmod`'s `mixins`), else `native_mods::MIXINS_REV`,
   built once with that cargo (`cargo install --locked --git … --rev …`) into `native/tools/<commit>/`.
3. **The source** of the pinned commit: GitHub's archive of it, once per commit (`native/source/<commit>/`, about
   63 MB unpacked), with the commit in `.horizon-revision`. Never the player's own checkout. Links in the
   archive are left out: the tool refuses them and nothing needs them to build.
4. **Mixins.** The bundles are copied first, so the build and its key use that copy whatever happens to the
   folder meanwhile. `horizon-mixins apply` stages the modded source; the launcher copies only the files that
   changed into `native/stage/`, so Cargo, which goes by file times, recompiles only the crates the mods touch.
   Each file written gets the current time, never the archive's (the commit's) or the copy's original: on
   Windows and macOS a copy keeps it, and a file older than the last build reads as already compiled, so
   switching Nightlies linked the previous one's crates (#10421). A stage from before that fix is written again
   once (`native/stage.stamped` marks it).
   With mods' crates, their dependencies are checked against Horizon's `Cargo.lock` ("The mod's crate").
5. **Compile**: `cargo build --locked --profile <fast|release> [--config ...] [--features horizon-client/mods] -p
   horizon-client`, into `native/target/`, with one of three build profiles:

   | Profile | Cargo profile | Use |
   |---|---|---|
   | `performance` (the default) | `fast`, the workspace's own: `codegen-units = 16`, incremental, and `lto = false`, Cargo's default, which is thin-local LTO across one crate's codegen units, none across crates | The everyday build for playing with mods: a rebuild recompiles and links only what changed |
   | `dev` | `fast` with `horizon-client` at opt-level 0 (`--config profile.fast.package.horizon-client.opt-level=0`) | Developing a mod: the quickest rebuild after an edit, but the game runs about twice as slow, so iterate with it rather than play |
   | `release` | `release`, the release's own (game-build.yml: thin LTO, one codegen unit) | Slowest to build, slightly faster game |

   With `dev`, an edit to a function the machine sample's mixin injects into `game/native_hooks.rs` compiled
   about 6× faster than with `performance`, and a tick took up to about twice as long. `dev` lowers only `horizon-client`, so an edit in another workspace
   crate (`horizon-client-lib`, `-render`, `-world`) still compiles at opt-level 3, as with `performance`. The
   launcher sets it with `--config` (`native_mods::Profile`), so the workspace's own `[profile.fast]` builds stay
   as they are. The Mods page's "Native build" row, `horizon-launcher settings native-profile performance|dev|release`
   or `native-build --profile ...` pick one (the setting's older value `fast` still means `performance`, and an older
   settings file's "Release build" switch still picks `release`).

   A revision whose `Cargo.toml` has no `fast` builds with `release`; `native-mods.json` says which (`profile`).
   Each profile is in the build's key, so their builds never mix and may all stay. `performance` and `dev` share
   `native/target/fast/`: every crate but `horizon-client` is built with the same settings, so switching between
   them recompiles only `horizon-client`, and Cargo keeps both builds of it, so switching back finds the other
   still there. `release` has its own folder (`native/target/release/`). The first build in a folder builds every
   crate once (1½ to 3½ minutes measured through the launcher, depending on the machine); later builds reuse them.
   Post-build steps build with the same
   profile, reusing its crates. The mods feature is passed while the revision still has it. Cargo downloads the locked crates from crates.io (Horizon has no git
   dependencies). The compiler and the horizon-mixins build run at the lowest priority (`rust_toolchain::low_priority`:
   nice 19 and the idle I/O class on Linux, nice 19 on macOS, below-normal priority on Windows), so the desktop and a
   game in play stay smooth; on an otherwise idle computer they still use every core. The log is `native/build.log`;
   a failure shows its last lines (a horizon-mixins diagnostic in one line).
6. **Post-build**: each mod's post-build program is built; it runs in the new version folder once that is copied
   (next step), before the version counts as installed ("Post-build steps").
7. **Install**: a new version, `native-<commit>-<key>`, a copy of the starting version's folder (resources
   included, cloned on APFS, Btrfs and ReFS) with the built game in place of its own, and a `native-mods.json`
   saying which release or Nightly it started from (`base`) and which commit, mods, horizon-mixins and Rust made
   it. The game lists its `mods` with their versions in a crash report's details (`Native Mods: id version, …`;
   they stay in the local report, not in crash telemetry). The instance the mods are installed in plays it, the default **Horizon** instance included: the build
   pins its version, as picking a version does. Updates of the launcher's version leave a pinned instance where
   it is, and their clean-up never removes a native build. An instance built before this (the **Native mods**
   instance the default home's mods used to go to) keeps its worlds and its build.

### Build log

The full log of a build is `native/build.log`, beside the build's own folders under the launcher's data
folder (`native_mods::log_path`). Each new attempt replaces it. Compiler output and the final result
stay in the log, including failures before compilation or while selecting or exporting the built game.

The launcher has a **Build log** action at the top right. It is there whenever a log exists, also right
after the launcher starts, and it always opens the latest build's log: the next build replaces it. The
panel shows a bounded preview of the end of the log and says when it truncated; **Copy log** copies the
whole file, **Open full log** opens it outside the launcher, **Show in folder** reveals it, and
**Refresh** reads it again (also after a build finishes). The panel is a snapshot read on open and
refresh, not a live stream: a running build's later output appears only on the next refresh. Nothing is
uploaded; the log stays on the player's computer.

## Rebuilds stay on the instance's Horizon

A build starts from the version the instance plays (`native_mods::base_for`). When that is a native build, the
next one compiles the same commit (its `native-mods.json`'s `horizon`) from the same game files, unless a mod pins
another, so a rebuild after a mod changes or is switched off compiles only what changed. A newer Horizon is taken
only when the player asks: `native-build --update` starts from the newest installed release or Nightly (a full
rebuild); `native_mods::update_for` says which one, if any, the window can offer.

The compiler runs as the release's build does, whatever the player's own Rust setup: the launcher's toolchain
drops the variables that change compiler flags or the target (`RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`,
`RUSTDOCFLAGS`, `CARGO_INCREMENTAL`, `CARGO_PROFILE_*`, `CARGO_BUILD_*` but `CARGO_BUILD_JOBS`, `CARGO_TARGET_*`,
rustup's and the wrappers'), and starts Cargo from the disk's root with `--manifest-path`. Cargo reads `.cargo/config.toml` from
the folder it starts in and every one above it, whatever `CARGO_HOME` says, so from the launcher's data folder it
would read the player's `~/.cargo/config.toml`, whose `[profile]`, `[build]` and `[target]` settings beat the
workspace and the environment; no switch turns that search off. The source's own `.cargo/config.toml`, if it
has one, is passed with `--config`.

Every path the build's tools see — the toolchain, the Cargo home, `native/` and the tools' temp dir (TMPDIR,
TMP and TEMP, where gcc, the assembler, `ld` and `cargo install` put their scratch files) — comes from
`backend::build_dir`: the launcher's data folder, resolved (never through a link, which horizon-mixins
refuses). On Windows, when that path is not spelled in ASCII — a Cyrillic user name in `%APPDATA%`, which
MinGW-w64's tools can't open — it is a real folder under `ProgramData` named for it (`horizon-launcher-<id>`),
and only when that can't be created the data folder's short (8.3) name. A short name alone is not enough
(#6009): rustc canonicalizes its own path to find its sysroot, which spells the long name again for the
linker. A data folder that is itself a junction to a drive with an ASCII path resolves to that and needs
neither.

## Install from GitHub

An author can ship a native mod without a registry pull request per version: the player installs it from a
GitHub link, on the Mods page ("Add from GitHub") or with `horizon-launcher mods add-github <url>
[--mod ID | --all] [--instance NAME] --trust-native-code`. The link is `https://github.com/<owner>/<repo>`
(its latest release), `.../releases/tag/<tag>`, `.../tree/<ref>` or `.../commit/<sha>`.

- **Resolved to something exact.** GitHub's public API (no sign-in; a rate limit is said plainly, and
  `HORIZON_GITHUB_TOKEN` raises it) turns the link into a release's `.hnmod` file or a **commit SHA**, never a
  branch that moves. A release with a `.hnmod` asset installs that file (its SHA-256 is compared with the one GitHub
  lists). With no release or no such asset, the launcher fetches
  `codeload.github.com/<owner>/<repo>/tar.gz/<sha>` and looks in it for mods by the rules of
  `native-build <folder>`: a folder with `horizon-mixin.toml`, or an unpacked `.hnmod` (`horizon-native.json`).
  Several mods: they are listed, and the player picks one or installs all. A source folder must pin the Horizon commit
  (`[compatibility] horizon_revision`); the launcher packs its Rust, TOML and `data/` files into a `.hnmod`.
  A release that ships only a content mod (`.hmod`) says "not supported yet": installing `.hmod` files from a link
  is not built.
- **Checked like the registry's files.** The `.hnmod` is read as `horizon_mods::index::native::Bundle::read` reads a
  registry file (only Rust, TOML and data under `mixins/<id>/`, size limits, nothing run) before it is installed.
- **Said first.** Before anything is downloaded: "not reviewed by the Horizon registry; native mods run with full
  access to this computer; read the source", with a link to the exact commit. The command needs
  `--trust-native-code`, as `native-build` does. It is not an MCP tool: trusting native code is the player's
  decision, never an agent's.
- **Origin kept.** `horizon-native-mods/<id>.origin.json` beside the `.hnmod`: owner/repo, release tag (for a
  release), commit, asset name and the SHA-256 of the installed file. `mods list` shows "from GitHub: owner/repo @
  tag" (or the commit's first 7 digits).
- **Updates.** `horizon-launcher mods update-github [ID] [--check]` compares a mod from a release with the
  repository's latest release, and a mod from a commit with the head of the default branch; a newer one is
  installed with the same warning and `--trust-native-code`. `mods remove <id>` removes it. A mod already
  installed from the registry (or by hand) is never replaced.

Installing turns Native mods on, as any native mod does; then build as usual.

## Cache and disk

The key hashes the commit, the horizon-mixins commit, Rust's version, the profile and features, and every file of
every bundle. The same set on the same commit is never built twice. A change gives a new build, and
`native/target/` stays, so a rebuild compiles only `horizon-client` and what the mods touched.

**Incremental rebuilds.** With `performance` and `dev`, Cargo recompiles only the changed parts of `horizon-client`, not the whole
crate with LTO. Patched source is cached separately by its mixin inputs and pins (the three most
recent snapshots are kept), so editing only a trait crate skips the mixins pass. This holds because
the stage (`native/stage/`) is one folder for every build and the launcher writes
only the files whose content changed (Cargo goes by file times), switching mods on the same commit reuses the same
`native/target/`, and the launcher removes `CARGO_INCREMENTAL` and `CARGO_BUILD_INCREMENTAL` from the build's
environment. `release` has no incremental compilation (the release profile's own).

**Build status.** Before it starts, the build says which case it is, with the profile: "Up to date (no build needed)",
"Incremental rebuild: mods X, Y" (or "Horizon commit a to b", "horizon-mixins"; compared with the last build in that
profile's folder, kept in `native/built.json`) or "First build: the whole game (several minutes)". While compiling it shows the
crate, crates done of the total (Cargo's own progress), and the time taken; afterwards "Built in 42 s (performance,
incremental)". The window shows it in the Native mods row, `native-build` prints each step with its time on standard
error; `native/build.log` keeps Cargo's output as before (without the progress bar).

Measured 2026-10-30 on Linux (x86_64 desktop), Zoom 2.0.0 and FPS Counter 2.0.0 on Nightly `6f8c7a0`, the `fast`
profile, the launcher at nice 19 with `CARGO_BUILD_JOBS=10` (an empty launcher home: Rust, the crates and the
source downloaded first):

| What | Time |
|---|---|
| First build: Rust, horizon-mixins, the whole game (384 crates) | 4 min 29 s in all; 3 min 6 s of compiling the game; `native/target/` about 2 GB |
| One mod switched off, rebuild | 56 s in all, 51 s of compiling (only `horizon-client`) |
| That mod switched on again | under 1 s: the kept build plays again |
| Nothing changed, rebuild | 1 s, nothing compiled |

The test bundle `native-crate-demo` (its own crate on `crc32fast`, data files, the `frame` hook by name, a
post-build step), measured the same way on 2026-10-30 (Rust already downloaded):

| What | Time | Compiled |
|---|---|---|
| First build (horizon-mixins, the crates, the whole game) | 3 min 27 s; 2 min 24 s compiling the game | 343 crates |
| A helper's code changed | 48 s (43 s compiling) | `horizon-client` only: the mod's crate is kept |
| The mod's crate changed | 11 s (6 s compiling) | `crate-demo-engine`, then `horizon-client` |
| Nothing changed | under 1 s | nothing |

The same set with the `release` profile took 7 min 6 s for the first build and 2 min 34 s per rebuild (2 min 28 s of
compiling `horizon-client`, mostly its link-time optimization). In play, a `fast` game drew the same frames per
second as the Nightly's own `release` build (vsync-bound, about 112 FPS at 120 Hz), with about 0.1 ms more CPU per
frame (0.57 to 0.68 ms against 0.51 to 0.54 ms).

Measured before builds used `fast`, with the `release` profile, on 2026-10-28 on the Mac mini (M4 Pro, 14 cores) with Veinminer 0.3.0 on Nightly `83f15fa`,
`CARGO_BUILD_JOBS=8` while other agents' builds ran (upper bounds):

| What | Size, time |
|---|---|
| Rust 1.99.1 (`<launcher data>/rust/`) | 453 MB; 58 s to download and unpack |
| horizon-mixins (`native/tools/`) | 7 MB; built once in 19 s |
| Crates in the launcher's Cargo home (`<launcher data>/cargo/`) | 289 MB |
| Source of one commit, and the stage | 64 MB each |
| First build: the whole game | 9 min 10 s of compiling, 9 min 41 s in all; `native/target/` 1.5 GB |
| Rebuild after a helper's code changed | 2 min 46 s of compiling (only `horizon-client`), 2 min 53 s in all |
| A change horizon-mixins ignores (a comment) | nothing compiled; 31 s, mostly the version folder's copy |

Measured 2026-10-29 on Windows 11 (Ryzen 7 9800X3D, 16 threads), the same mod and Nightly, an empty `native/target/`
(horizon-mixins, the source and the crates were already downloaded):

| What | Size, time |
|---|---|
| Rust 1.99.1 and MinGW-w64 (`<launcher data>/rust/`) | 1.7 GB, MinGW-w64 875 MB of it |
| Crates in the launcher's Cargo home | 657 MB |
| First build: the whole game | 6 min 57 s of compiling, 8 min 3 s in all; `native/target/` 2.0 GB |

Measured 2026-10-28 on Linux (an x86_64 server with no Rust of its own), Zoom on Horizon `1149c92`, 3 jobs while CI
ran: `rust install` in 11 s (592 MB), the first build in 12 min 9 s. The built game ran Zoom's proof (under `xvfb-run`:
the game needs a display even with `--offscreen`).

The native builds an instance plays are kept, and the three newest no instance plays (`native_mods::prune`, by when
each was built or last used): switching a mod off and on again plays the kept build again without compiling.
`native/target/` stays for the next one.

## Trust

- **A notice, then a warning.** The first native install shows a trust notice once. The command needs
  `--trust-native-code`; the window
  shows a warning in red before every build: the mods run with full access to the computer, and their ids and
  versions. The Mods page says who made each one, and "Not made or endorsed by the Horizon project" for any mod not
  Horizon's own.
- **The registry** (horizon-rust/horizon-mods) lists them as their own kind, `native-mod`, only after a maintainer
  has read the source and the repository they come from. Its entry names the GitHub repository (`repo`); each
  release of it with a `.hnmod` asset is a version once the bundle passes the registry's checks, with no further
  review (nothing is built or run by the registry; the player's launcher builds it behind its warning). The `.hnmod` contains `.rs` and `.toml` files and may contain data files under
  `mixins/<id>/data/` or a crate's `data/`. The launcher checks the bundle without running anything. Native code is trusted.
  The file is pinned by SHA-256 like every registry file, and the version, game and pins are the bundle's own
  `horizon-native.json`. `revoked.toml` takes a bad release down.
  A content mod's entry may turn `native-mod` once (never back): its `.hmod` versions stay published and
  are checked as `.hmod`s, the index lists only its versions with native code, and the launcher's update keeps the
  player's `.hmod` (the game runs it until a native build has the mod, then skips it); removing the mod removes both.
- **Isolation of data.** Native mods run only in the instance they are installed in, with its worlds, options and
  logs (the default Horizon instance's, when they are installed there); other instances play their own versions.
- What this does not do: sandbox the compiled game. horizon-mixins' checks stop mistakes, not a malicious helper
  ([security notes](https://github.com/horizon-rust/horizon-mixins/blob/main/docs/security.md)).

## Multiplayer

- **Single player and LAN host:** the mods run in the host's game and its integrated server, so a server-side
  hook applies to every player of the world.
- **Joining a server:** only client-side hooks do anything; the protocol is unchanged.
- **Dedicated server:** native mods run in a standalone engine export built with `--server-output`, not in an
  instance build (`--server-output` conflicts with `--instance` and needs an explicit mod `FOLDER`). See
  [Standalone server export](#standalone-server-export).

### Standalone server export

`horizon-launcher native-build FOLDER --server-output DIR --version TAG --trust-native-code [--profile PROFILE] [--allow-version-mismatch]`

Builds a standalone dedicated-server engine into `DIR`, which must not exist yet. `FOLDER` (a mod folder or a
folder of them) is required; no instance is read, selected, edited or played, and no launcher config is written.
It reuses the pinned toolchain, source and horizon-mixins and compiles `--no-default-features` (no audio or
controllers), keeping the mods feature when the revision has it. `--version` pins the Horizon release/Nightly the
export reuses (explicit for reproducibility). The export holds the built `horizon-engine` executable, prepared resources,
`native-data/` and `native-mods.json`, and prints its absolute path. It runs on the same OS/architecture as the
host only (no cross compilation), needs a prepared installed launcher base release, and survives cache pruning
because it is copied outside the managed folders. On Windows use the printed `horizon-engine.exe` path.

Point a server at it:

    horizon-server --dir SERVER config set engine.path=/absolute/export/horizon-engine
    horizon-server --dir SERVER restart

Updates do not rebuild a pinned `engine.path`: build a new export folder, set `engine.path` to it, restart, and keep
the old folder for manual rollback. The supervisor does not auto-update or rebuild a pinned engine path, and there
is no compatibility handshake yet (shared server build crate / native config / updates / handshake are follow-up
#6304, not implemented). Native mods run unsandboxed: only build code you have read and trust.
`--trust-native-code` records the operator's decision; an agent must not supply that consent.

### Developer acceptance workflow

Use a disposable server folder and fresh homes when developing:

```sh
SCRATCH=$(mktemp -d)
export HORIZON_HOME="$SCRATCH/game" HORIZON_LAUNCHER_HOME="$SCRATCH/launcher"
export XDG_DATA_HOME="$SCRATCH/data" XDG_CONFIG_HOME="$SCRATCH/config"
export HORIZON_NO_AUDIO=1 HORIZON_LAUNCHER_NO_SELF_UPDATE=1
export HORIZON_LAUNCHER_TRASH="$SCRATCH/trash"
export HORIZON_LAUNCHER_CLIPBOARD="$SCRATCH/clipboard"
export HORIZON_LAUNCHER_DESKTOP="$SCRATCH/desktop"
```

Install and prepare the base release in that scratch launcher home first, then test your own licensed bundle:

1. `horizon-launcher native-build bundle/ --server-output $ENGINE --version TAG --trust-native-code` (scratch homes,
   `HORIZON_NO_AUDIO=1`, `HORIZON_LAUNCHER_NO_SELF_UPDATE=1`).
2. `horizon-server --dir SERVER config set engine.path=$ENGINE/horizon-engine`; `horizon-server --dir SERVER run`.
3. Join with the same mod's client half where the hook needs one; test lifecycle, tick, command and net hooks.
4. Headless compatibility: the mod's server code must not require an audio device; run the server audio-free (`HORIZON_NO_AUDIO=1`).

## Left to do

- A list of every native build, to delete the ones no instance needs.
- A prebuilt horizon-mixins from its CI instead of building it on the player's computer.

## Named hook catalog (draft)

`native-hooks.json` maps stable hook names and contract versions to the current
source targets. A contract includes the signature, call timing and behavior.
Keep an existing contract or add a new version when it changes incompatibly.
The catalog does not cover helper methods or arbitrary private game code.
Injections name them with `hook` and `hook_version` ("Named hook points" above); raw
targets and the `hooks` metadata continue to work as before.

### Knockback hook versions

`living_knockback_v2` keeps `living_knockback` version 1 unchanged, as `menu_button_v2` does for
its older hook: v1's function signature is a target mixin mods attach to, so it cannot gain
arguments. Both already receive the raw strength before knockback resistance (v1 did before
v2 existed). V2 adds three things: the attacker's network entity id (falling back to a known
last hurt-by mob, or 0 when unknown), what the attacker holds (`attacker_main` and
`attacker_off`: a player's selected and off-hand stacks, a mob's main hand; none when unknown,
recorded only while a native mod is loaded), and armor-stand sweeps. V2 runs after v1 on
their shared paths; cancellation from either prevents motion, so a trait mod that listens to
both sees each hit twice and should pick one. Mob melee, player hits, sweeps and spears use the
shared knockback path; direct velocity pushes (explosions, Punch arrows, hoglin throws) are not
`LivingEntity.knockback` and do not fire it. The fragment assigns the new API number when
stamped; the existing hook stays version 1.

### Reading server blocks (draft)

A native server tick hook can call `native_server_block` with its context, a dimension id and a block position. It returns the block name and state properties from a loaded chunk. It does not load a chunk or open a dimension. Reads outside that hook, from a stale context, from unavailable dimensions or outside the world bounds return an error.

### Neighbour observations (draft)

`native_neighbor_changed` observes each authoritative server `neighborChanged` before the block handles it. It supplies the dimension, position, current state ID, changed block ID and moved flag. Resolve IDs with the level's registries during the hook; the hook does not load chunks or change the update queue.

### Server block changes (draft)

`native_block_changed` observes a committed authoritative server `Level.setBlock` change before block side effects and neighbour updates. It supplies the dimension, position and old and new state IDs. It does not observe chunk loading or direct storage writes that bypass `Level.setBlock`.

### Server commands and players

For mods that run where the world is simulated: a dedicated server, a LAN host, single player's server thread
(`crates/horizon-client/src/game/native_server.rs`). The SMP mod of the official server is built on them
([smp.md](smp.md)).

- **`native_register_commands(commands)`**: push a `NativeCommand` per usage, asked once when a world opens to
  players. A usage is words, then arguments `<name:kind>`, optional ones last in brackets:
  `NativeCommand::new("pay <player:player> <amount:integer>")`, `NativeCommand::op("eco set <player:player>
  <amount:integer>", 3)` for operators of level 3. Kinds: `word`, `text` (the rest of the line, last), `integer`,
  `number`, `boolean`, `player`, `item`, `block_pos`. The usages of one command merge into one tree, which every
  player receives after vanilla's (filtered by their level, as vanilla's is), so their chat box completes and colours
  it. A name vanilla has, or a usage that doesn't parse, is left out with a warning in the log.
- **`native_refresh_commands()`** (`host.refresh_commands()` for trait mods, API @API:refresh_commands@): a helper for
  a mod whose commands change while the server runs (a plugin host that reloads a plugin, a mod turned on at run time).
  At the start of the next server tick every mod's `native_register_commands` is asked again, the commands players may
  run (`native_perform_command`, with their levels) are rebuilt, and every player on the server is sent their tree
  again, after `native_player_commands_sent`, as vanilla sends `Commands.sendCommands` to each player after `/reload`.
  Call it after you change what you register; nothing happens where no world is open to players, and a server whose
  mods never call it is unchanged.
- **`native_armor_stand_manipulate(player, name, stand_id, slot, cancel)`**: a player takes an item from an armor stand or
  puts one on it, heard after the stand's disabled-slot checks and before anything moves. `slot` is `mainhand`,
  `offhand`, `feet`, `legs`, `chest` or `head`. Set `cancel` and nothing changes (the click is spent). The player's
  state is swapped in; `player` is their UUID. Not told of a click that changes nothing; breaking or blasting a stand and
  a dispenser armouring one are other ways its equipment moves and are not covered.
- **`native_vehicle_moved(player, kind, from, to)`**: a boat or minecart that carries a server player (`player`, its UUID)
  moved since the last tick, heard once a tick after the move with its positions before and after. The rider's state is
  swapped in, so `native_acting_player`, `native_tell` and the item helpers reach them. A vehicle a player rode and left is
  still told of, with no `player` and nobody swapped in, until it has stood still for ten seconds. Mounts (horses...) and
  vehicles nobody ever rode are not covered. It only hears and costs one position compare a tick for each watched vehicle.
- **`native_container_opened(player, name, kind, pos, cancel)`**: a player is about to open one of the game's own menus
  (a chest, furnace, crafting table, villager, anvil, lectern, a horse's inventory...). `kind` is the menu type
  (`minecraft:generic_9x3`, `minecraft:merchant`...), `pos` the block it belongs to, none for a villager, a mount or a
  vehicle's chest. Heard after the checks that refuse the menu (a locked chest), before the open menu closes. Set
  `cancel` and nothing opens. The player's state is swapped in, so `native_acting_player`, `native_tell` and the item
  helpers reach them. Menus a mod opens (`native_open_menu`) are not heard.
- **`native_creative_slot(player, name, slot, item, count, cancel)`**: a creative-mode player sets a slot from the creative
  screen (`slot` is the inventory menu's slot; below 0 is a stack thrown out of the screen), with the `item` (`minecraft:air`
  for an emptied slot) and `count`. Heard after vanilla's checks, before the slot changes. Set `cancel` and the packet is
  ignored. The player's state is swapped in. Stacks a creative player gets other ways (pick block, `/give`) are not heard.
- **`native_player_game_mode_changed(player, name, from, to, cancel)`**: a server player's game mode is about to change
  (0 survival, 1 creative, 2 adventure, 3 spectator, as `NativeServerPlayer.game_mode`): the `/gamemode` command, the
  F3+F4 switcher's packet and a hardcore world's respawn into spectator, once per change to a different mode. The mode a
  player joins with (`defaultgamemode`) is not heard. Set `cancel` and the player keeps their mode. Inside the command and
  the switcher the player's state is swapped in; for the hardcore respawn it is not (use the UUID with the helpers that
  take one).
- **`native_player_changed_dimension(player, name, from, to)`**: a player changed dimension, once per move
  (`from` and `to` are ids such as `minecraft:overworld` and `minecraft:the_nether`), heard when the move has ended:
  the portal's exit is found and the player stands there, a command's teleport has landed, or a respawn in another
  level than the death's is placed for good. `to` is the level in place and the player's state is swapped in (as in
  `native_perform_command`), so `native_acting_player` and `native_tell` reach them. Heard for Nether and End portals
  (also riding, and leaving the End as its winner), `/execute in ... run tp` and any command teleporting into another
  level, and the death respawn. Not heard: a move within a level (`/tp`, an End gateway), a player who leaves before
  the move ends, a respawn in the dimension of the death, and anything outside a dedicated server.
- **`native_player_toggle_flight(player, name, flying, cancel)`**: a player's client started (`flying`) or stopped
  flying, once per change, for a player who may fly (creative or spectator), on a server. The player's state is swapped
  in, so `native_tell` and the other helpers reach them. Set `cancel` to refuse it: the player keeps the state they
  had and gets their abilities back, so the client lands or flies again. Only what a client asks for is heard: not the
  flight a game-mode change starts or ends, and not a LAN host's own player.
- **`native_player_item_held(player, name, from_slot, to_slot, cancel)`**: a server player selected a different
  hotbar slot, after the slot range check and before it changes. The player's state is not swapped in, so helpers
  that look them up through `self.lan` may find nothing while a guest packet is being handled. Set `cancel` to
  keep the old slot; the client is sent that slot again.
- **`native_furnace_burn(pos, fuel)`**: a furnace consumed `fuel` and started burning at `pos`.
- **`native_furnace_smelt(pos, item, result)`**: a furnace finished smelting `item` into `result` at `pos`.
- **`native_player_exp_change(player, name, amount)`**: a server player's experience points from an orb pickup are
  about to be added, after mending has taken its share. Change `amount` to change the points added; negative values
  become zero. The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing
  while a guest packet is being handled. Command experience changes use another path and are not heard.
- **`native_player_item_cooldown(player, name, item, ticks, cancel)`**: an item cooldown is about to start for a
  server player, including a used item's cooldown and a shield disabled by a hit. `item` is the item id and `ticks`
  its duration. Set `cancel` to skip the cooldown; shield disable still stops the use and plays its configured sound.
  The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing while a guest
  packet is being handled.
- **`native_player_recipe_discovered(player, name, recipe, cancel)`**: a recipe id is about to become known to a
  server player through an advancement reward, a knowledge book or `/recipe give`. Set `cancel` to keep it locked;
  the client is sent the matching removal. The player's state is not swapped in, so helpers that look them up through
  `self.lan` may find nothing while a guest packet is being handled.
- **`native_player_spawn_point_set(player, name, dimension, pos, cause, cancel)`**: a server player's respawn point
  is about to change. `cause` is `bed`, `respawn_anchor`, `command` or `death_reset`; `pos` is its block position.
  Set `cancel` to keep the old point. A canceled `death_reset` keeps the old point even if its bed or anchor is gone.
  The player's state is not swapped in, so helpers that look them up through `self.lan` may find nothing while a guest
  packet is being handled.
- **`native_player_portal(player, name, from_dimension, to_dimension, cause, cancel)`**: a server player is about
  to travel through a Nether portal, End portal or End gateway; `cause` is `nether_portal`, `end_portal` or
  `end_gateway`. Set `cancel` to leave the player where they are. The player's state is not swapped in, so helpers that
  look them up through `self.lan` may find nothing while a guest tick is handled.
- **`native_block_damage_abort(player, name, pos)`**: a server player stopped digging `pos` before it broke. It is
  heard after the server clears the dig and sends the block state back to the client. The player's state is not
  swapped in, so helpers that look them up through `self.lan` may find nothing while a guest packet is handled.
- **`native_player_pick_item(player, name, from_entity, pos, entity_id, cancel)`**: a server player's middle-click
  pick resolved a block or entity's pick stack and is about to put it in the hotbar. For a block, `pos` is its
  position and `entity_id` is zero; for an entity, `from_entity` is true, `pos` is `[0, 0, 0]` and `entity_id`
  is its network id. The player's state is swapped in, though helpers that look them up through `self.lan` may
  find nothing while a guest packet is handled. Set `cancel` to leave the inventory and client unchanged. A
  pick with no result is not heard.
- **`native_sign_changed(player, name, dimension, pos, front, lines, cancel)`**: a player finished a sign they were
  editing, heard once every check has passed (editing that sign, in reach, not waxed, the chat filter) and right before
  the text goes on it. `lines` holds the four lines as written, formatting codes still in; rewrite them and that is
  what the sign gets (the chat filter does not look at it again), or set `cancel` and it keeps its old text. The
  player's state is swapped in, so `native_tell` reaches them. Not told: the host's own editing on a LAN or
  single-player world, and changes that are no text edit (dye, glow ink, wax, `/data`, `/setblock`).
- **`native_player_client_loaded_world(player, name)`**: the server heard that a guest's client finished loading
  the level around them after joining, respawning or changing dimension. The player's state is not swapped in, so
  helpers that look them up through `self.lan` may find nothing while the guest packet is handled.
- **`native_player_stop_using_item(player, name)`**: a server player released the use-item key before the item
  finished being used. The player's state is not swapped in, so helpers that look them up through `self.lan` may
  find nothing during packet handling.
- **`native_player_recipe_book_settings_change(player, name, book, open, filtering)`**: a server player's recipe
  book open or filter setting changed; `book` is the book type's protocol id. The player's state is not swapped in.
- **`native_player_advancement_criterion_grant(player, name, advancement, criterion)`**: a server player earned
  one advancement criterion, after its progress was recorded.
- **`native_player_command(player, name, command, canceled, replies)`**: any command a player typed (without the
  slash), vanilla's too and commands the player may not run, heard before anything looks at it, and each line of a
  quiet command batch. Set `canceled` and nothing runs; push lines to `replies` and the player gets them as system chat
  (a quiet batch drops them). The player's state is not swapped in, and `native_tell` and the other helpers that take a
  UUID reach nobody inside this hook: answer through `replies`. Commands may carry secrets (`/msg`, a login plugin's
  password), so don't log them carelessly.
- **`native_perform_console_command(command, ran)`**: a registered command typed on the dedicated server's console
  (without its slash), run as the server at level 4: no player, so `native_acting_player()` is None and the player-only
  helpers give nothing. The mod parses the line and sets `ran`; `native_feedback` answers on the console. A name no mod
  registered goes to vanilla's dispatcher as before. The single player owner's console (`horizon-ctl command`, the MCP
  command tool) is a player's: it reaches `native_perform_command` with the owner, at level 4.
- **`native_player_respawned(player, name)`**: a dead player came back, once per respawn, on a server: after
  `PlayerList.respawn` reset their health, food and experience and placed them at their bed, anchor or the world spawn
  (NeoForge's `PlayerRespawnEvent`). The player's state is swapped in for the call, as for their other actions, so
  `native_player`, `native_give_items` and the other helpers reach them. A respawn at an anchor in another dimension, or
  at the world spawn's free column, may still move them on a later tick, once that level has loaded round them; the
  End-exit respawn (after the credits) does not fire it.
- **`native_player_login(player, name, cancel, reason)`**: a player the server has accepted (their name, the whitelist,
  bans and the player limit are checked) and who holds a place, heard before their saved state is loaded and before they
  are placed in a level (NeoForge's `PlayerNegotiationEvent`). Set `cancel` and they are disconnected with `reason` (a
  plain "Disconnected" when it stays `None`); no join message and no `native_player_logged_in` follow. It runs after the
  client's configuration, so the client sees the refusal as an in-game disconnect screen. The player is not placed yet,
  so no player's state is swapped in and `native_tell` reaches nobody: the refusal text is the answer. Players the
  server refuses itself (name, whitelist, bans, a full server, the proxy's forwarding) are not heard.
- **`native_player_teleported(player, name, from, to, dimension)`**: a server player a command (`/tp`, `/teleport`,
  also what a mod's `run_command` reaches) moved, inside their level or into another; `from` and `to` are their feet
  before and after and `dimension` the level they are in now. Heard after the move has reached the player's state and
  their client (into another level: when that level is entered, before it has loaded around them), so it cannot stop it
  (NeoForge's `EntityTeleportEvent`). The player's state is swapped in, so `native_tell` and the acting-player helpers
  work. Ender pearls and chorus fruit have their own hooks below; not heard yet: a spectator's teleport menu; portals
  and respawns are not teleports here.
- **`native_ender_pearl_teleport(player, name, pearl, from, target, damage, cancel)`**: a server player's ender pearl
  landed and is about to teleport them (NeoForge's `EntityTeleportEvent.EnderPearl`, `ThrownEnderpearl.onHit`).
  `pearl` is the pearl's entity id, `from` their feet, `target` where the pearl started its last tick and `damage` the
  fall damage they then take (5). A mod may move `target` or change `damage` (0 or less: none). Canceling keeps the
  player where they are: no endermite roll, no teleport, no damage, no sound; the pearl is used up either way. Mobs'
  pearls are not heard. Bundles need `"hooks": 312`.
- **`native_chorus_fruit_teleport(player, name, from, target, cancel)`**: a server player whose `teleport_randomly`
  consume effect (chorus fruit) is about to try a spot (NeoForge's `EntityTeleportEvent.ChorusFruit`,
  `TeleportRandomlyConsumeEffect.apply`): once per attempt, up to 16, after they stopped riding. A mod may move
  `target` (its y is kept inside the level's height); the game still looks down from it for ground to land on, inside
  the border. Canceling ends the effect: no
  teleport and no more attempts. Foxes eating chorus fruit are not heard. Bundles need
  `"hooks": 311`.
- **`native_player_clone(player, name, was_death)`**: a player's data was copied to their new player (NeoForge's
  `PlayerEvent.Clone`, from `ServerPlayer.restoreFrom`): after a death respawn (`was_death` true) and when a player
  leaves the End after the credits (`was_death` false, vanilla's `keepAllPlayerData`). It is heard after the copy and
  before the new player is placed, once each, on a server. The uuid stays the same, so `player` names the old and the
  new player alike; no player's state is swapped in, so the helpers that look a player up may find nothing. It cannot
  stop anything.
- **`native_player_load_from_file(player, name, directory)`**: a joining player's saved state was read and applied
  (NeoForge's `PlayerEvent.LoadFromFile`, `PlayerList.loadPlayerData`), before they are placed in a level: once for each
  join on a server, for an integrated server's owner (loaded from the file `level.dat` names, else their own) as for
  anyone else. `directory` is the world's `players/data` folder, created if missing; read your own file there, named `<uuid>.<suffix>`. It
  is heard whether or not the player was in the world before (a new player has no file). No player's state is swapped
  in. It cannot change or stop anything.
- **`native_player_save_to_file(player, name, directory)`**: a placed player's data was saved (NeoForge's
  `PlayerEvent.SaveToFile`, `PlayerList.save`): when they leave or the world closes, when the dedicated server saves
  everyone, and for an integrated server's owner when the world saves their state into `level.dat`. `directory` is the
  world's `players/data` folder, created if missing; write your own file there, named `<uuid>.<suffix>`, and never the player's `.dat`.
  The player may be leaving, so no player's state is swapped in. It cannot change or stop anything.
- **`native_player_commands_sent(player, name, names)`**: the command tree a player is sent, when they join and when
  their level changes (`/op`, `/deop`, the LAN settings). `names` are the root commands their client would get, vanilla's
  they may use and the mods'; remove a name and that client's tree lacks it, so its chat box neither completes nor
  colours it. This hides the command from the client only, a typed one still runs (stop it with
  `native_player_command`). For a player already on, their state is swapped in and `native_tell` works; a joining
  player has no world yet, so nothing can be told them. The tree the hub re-sends by itself (after a respawn or a change
  of level) reuses the last answer.
- **`native_player_sprint(player, name, sprinting)`**: a server player's client started or stopped sprinting (the sprint
  key, a double tap forward, letting go), once for each change, after the server's player took it. It only hears. It
  reports what the client asks, not what the server does: a sprint that ends by itself (a hit's knockback, hunger, a
  wall) is not heard. The player's state is swapped in, so `native_tell` and the acting-player helpers work.
- **`native_resource_pack_status(player, name, status)`**: a server player's client answered a resource pack while
  playing (`ServerboundResourcePackPacket`): `status` is `Action`'s number (0 loaded, 1 declined, 2 failed download,
  3 accepted, 4 downloaded, 5 invalid URL, 6 failed reload, 7 discarded). It only hears. The pack a dedicated server
  offers during the configuration is answered before the player has joined and is not heard. The player's state is
  swapped in, so `native_tell` works.
- **`native_projectile_hit(owner, kind, hit_entity, pos, cancel)`**: a projectile a server player launched (arrow,
  snowball, egg, thrown trident, ender pearl, ...) hit an entity (`hit_entity`) or a block at `pos`, heard before any
  of its effects (NeoForge's `ProjectileImpactEvent`). Set `cancel` and it does nothing: it vanishes, a thrown trident
  flies on through. The shooter's state is swapped in, so `native_tell` answers them. Mobs' and the host's
  projectiles are not heard, nor are fishing bobbers and firework rockets.
- **`native_tnt_primed(player, pos, cancel)`**: TNT at `pos` is about to be lit by a flint and steel or fire charge
  (`player` is whoever clicked, whose state is swapped in), fire, a redstone signal, an arrow or a dispenser (`player` is
  None). Set `cancel` and it is not primed: the block stays, or is put back if fire or a signal had already taken it
  out. TNT set off by a blast, a TNT minecart and a sulfur cube are not asked; `native_explosion_detonate` can spare
  the blocks of a blast.
- **`native_hanging_place(player, name, kind, pos, cancel)`**: a server player places an item frame, a glow item
  frame, a painting or a leash knot (tying a lead to a fence that has none; `pos` is the fence), once it fits and
  before it is placed or the item used. Set `cancel` and nothing happens; the item stays in the hand. The player's
  state is swapped in, so `native_tell` answers them. `/summon` and a lead tied to a mob are not heard.
- **`native_hanging_break(player, kind, pos, cancel)`**: an item frame, a glow item frame or a painting (`kind`)
  hanging in `pos` is about to break: a server player's hit (`player`, whose state is swapped in), a blast, or the
  wall it hangs on gone (`player` None for both), before anything drops or sounds. Set `cancel` and it stays as it
  was. A hit that only takes a frame's item out, the game's own player and `/kill` are not asked.
- **`native_bucket_empty(player, name, pos, fluid, cancel)`**: a server player empties a bucket (`fluid` is
  `minecraft:water`, `minecraft:lava` or `minecraft:powder_snow`; fish and axolotl buckets pour water) into `pos`,
  right before the fluid is placed. Set `cancel` and nothing is poured. The player's state is swapped in, so
  `native_tell` answers them. `native_fill_bucket` hears the same click without the player and the fluid; dispensers
  and cauldrons are not heard. A fluid or mob bucket is also heard at the block beside the clicked face whenever that
  block is not `pos`, since a block that refuses the fluid passes it there; the fluid may still go into `pos`.
- **`native_player_bed_enter(player, name, pos, cancel)`**: a server player passed the sleep checks and is about to
  enter the bed at its head position. Set `cancel` to keep them out; helpers that look them up through `self.lan` may
  find nothing while a guest packet is handled.
- **`native_player_bed_enter(player, name, pos, cancel)`**: a server player passed the sleep checks and is about to
  enter the bed at its head position. Set `cancel` to keep them out; helpers that look them up through `self.lan` may
  find nothing while a guest packet is handled.
- **`native_player_bed_leave(player, name, pos)`**: A server player woke or stood up from a bed, after their sleeping state changed. The head position is `pos`; helpers looking up the guest through `self.lan` may find nothing during guest packet handling.
- **`native_player_edit_book(player, name, pages, signing, cancel)`**: A server player book-and-quill edit or signing packet is about to be applied. `pages` is its page count and `signing` says it has a title. Set `cancel` to drop the packet.
- **`native_player_shear_entity(player, name, entity_id, off_hand, cancel)`**: A server player is about to shear a sheep, mooshroom, snow golem or bogged. The hook gives the mob entity id and whether the shears are in the off hand. Set `cancel` to leave the mob and shears unchanged.
- **`native_player_advancement_done(player, name, advancement)`**: A server player completed an advancement after its progress was recorded. `advancement` is its identifier, such as `minecraft:story/mine_stone`; helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_advancement_progress(player, name, advancement, criterion, granted)`**: A criterion of a server player's advancement was granted or revoked, after its progress changed and before any completion rewards, except the `minecraft:tick` criteria granted on a network player's first tick or a reload, which are heard after that tick's rewards and packets. `advancement` is its identifier, such as `minecraft:story/mine_stone`, and `granted` is false for a revoke. A grant or revoke that changes nothing is not heard; helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_stat_award(player, name, stat_type, stat, value, cancel)`**: A statistic is about to be awarded to a server player, before its count and scoreboard criteria change. `stat_type` and `stat` are ids such as `minecraft:mined` and `minecraft:stone`, or `minecraft:custom` and `minecraft:walk_one_cm`, and `value` is the amount. Set `cancel` to award nothing. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_riptide(player, name)`**: a server player launched with a riptide trident, after its velocity and spin are applied. The player's state is swapped in, though helpers that look them up through `self.lan` may find nothing during a guest packet.
- **`native_player_fish(player, name, state, cancel)`**: a player's fishing rod, on a server: `state` 0 the cast, 1 a fish
  biting, 2 the reel-in of a bite (the catch), 3 the reel-in with nothing on the line, 4 the reel-in of a bobber hooked
  to an entity or item. Set `cancel` and nothing is thrown (0), the fish gets away and the wait starts over (1), or the
  bobber stays out with no wear on the rod (2 to 4). The bobber ending by itself (too far, rod put away, owner dead) is
  not heard, nor is the host of a LAN world's rod. The player's state is swapped in, so `native_acting_player`,
  `native_tell` and the item helpers reach them.
- **`native_block_dispense_armor(dimension, pos, item, target)`**: a dispenser equipped `item` on the first eligible player, mob or armor stand in front of its block at `pos`. The equip, item consumption, sound and game event have completed; `target` names the kind of entity equipped.
- **`native_block_exp(dimension, pos, block, experience)`**: a block with a vanilla experience drop is about to award its points at `pos`. Change `experience` before the orbs are awarded.
- **`native_block_shear_entity(dimension, pos, entity, kind)`**: a dispenser sheared the entity of `kind` and id `entity` at the block in front of the dispenser at `pos`, after applying the shearing effects.
- **`native_block_receive_game(dimension, pos, block, event, cancel)`**: a sculk sensor, calibrated sensor or shrieker received the vibration `event`. Set `cancel` to suppress its response.
- **`native_brewing_start(pos, ingredient)`**: a brewing stand at `pos` starts its timer for `ingredient`, after it consumes fuel. Server-side.
- **`native_campfire_start(pos, item)`**: `item` was placed on the campfire at `pos` and began cooking. Server-side.
- **`native_block_cook(pos, item, result)`**: a campfire finishes cooking `item` into `result` at `pos`, before the result is dropped. Server-side.
- **`native_compost_item(pos, item)`**: a player adds a compostable item to the composter at `pos`, after its layer is applied. Server-side.
- **`native_crafter_craft(pos, recipe)`**: a crafter completed `recipe` at `pos`, after consuming its ingredients and handling its output. Server-side.
- **`native_player_attack_cooldown_reset(player, name)`**: a server player's punch was handled: the main-hand swing and attack-strength reset have both happened. The player's state is not swapped in, so helpers looking them up through `self.lan` may find nothing during a guest packet.
- **`native_player_bucket_entity(player, name, entity_id, entity)`**: a server player caught a bucketable mob after the mob was removed and the filled bucket was put in the player's hand or inventory. `entity` is the mob species id.
- **`native_player_change_beacon_effect(player, name, pos, primary, secondary)`**: a server player's beacon selection was stored. Effect values are ids, empty when none is selected.
- **`native_player_deep_sleep(player, name)`**: a server player reached 100 ticks asleep in a bed and now counts as deeply asleep for the night-skip check. Helpers looking up the guest through `self.lan` may find nothing during its tick.
- **`native_player_change_main_hand(player, name, left)`**: a server player's client options changed their dominant hand; `left` is true for left-handed. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_egg_throw(player, name)`**: a server player threw an egg after its projectile was launched and its use applied.
- **`native_player_elytra_boost(player, name)`**: a server player used a firework rocket to boost while gliding, after the rocket was launched and consumed.
- **`native_player_flower_pot_manipulate(player, name, pos, plant)`**: a server player put `plant` into the flower pot at `pos`, after its state changed and the item was consumed. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_insert_lectern_book(player, name, pos, book)`**: a server player put `book` on the lectern at `pos`, after the block entity and inventory changed. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_take_lectern_book(player, name, pos, book)`**: a server player removed `book` from the lectern at `pos`, after its block entity changed. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_lectern_page_change(player, name, pos, page)`**: a server player changed the lectern at `pos` to zero-based `page`, after its page and redstone pulse changed. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_item_frame_change(player, name, entity_id, action, item, rotation)`**: a server player changed a frame (`put`, `rotate`, `remove_item` or `break`), after its contents and comparator output changed. `item` is empty when none remains. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_item_mend(player, name, item, repair, experience)`**: an orb actually repaired `repair` durability on `item` for a server player, after the item changed. `experience` is the orb value. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_player_trade(player, name, entity_id)`**: a server player completed a villager or wandering trader trade, after the merchant applied it. `entity_id` is the merchant network id. Helpers looking up the guest through `self.lan` may find nothing during packet handling.
- **`native_trade_with_villager(merchant, kind, offer)`**: NeoForge's `TradeWithVillagerEvent`, heard once per trade a
  server player makes with a villager or a wandering trader (a shift-click that trades several times is heard for each),
  after the trade's statistic and `villager_trade` trigger. `merchant` is the trader's entity id, `kind` its type
  (`minecraft:villager` or `minecraft:wandering_trader`), and `offer` the `MerchantOffer` as it was right after the
  trade: its costs, `result`, `uses` (this trade counted), `max_uses` and `xp`. Listen only, heard once the whole click
  is done (its payments taken; NeoForge fires during each take). The player's state is swapped in, so `native_acting_player` names
  them. Unlike `native_player_trade`, which is heard once per click with no offer.
- **`native_player_shield_disable(player, name, ticks)`**: a blocked melee hit disabled the server player’s shield for `ticks` ticks. It fires just before the cooldown is set; guest lookup helpers may not find the guest during packet handling.
- **`native_player_loom_pattern_select(player, name, pattern)`**: a server player selected loom pattern button `pattern`, after the menu accepted it. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_name_entity(player, name, entity_id, entity_name)`**: a server player used a named name tag on a mob, after it became persistent and was named. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_open_sign(player, name, pos, front)`**: a server player opened the sign editor at `pos` for the front or back side, after the editor state was made. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_pickup_arrow(player, name, entity_id, item)`**: a server player picked up an arrow after it was removed and its pickup was applied. `item` is the pickup item id. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_stonecutter_recipe_select(player, name, recipe)`**: a server player selected stonecutter recipe button `recipe`, after the menu accepted it. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_swap_with_equipment_slot(player, name, slot)`**: a server player swapped their main hand with `slot` (currently `offhand`), after both hands changed. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_toggle_entity_age_lock(player, name, entity_id, locked)`**: a server player used a golden dandelion on an ageable mob, after its age lock changed and the item was used.
- **`native_player_unleash_entity(player, name, entity_id)`**: a server player's action removed an entity's leash, after its leash state was cleared. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_velocity(player, name, velocity)`**: the server synced changed motion to a player, with `velocity` in blocks per tick. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_stop_spectating_entity(player, name, entity_id)`**: a spectator's camera returned to their own player after they stopped watching `entity_id`.
- **`native_player_map_filled(player, name, map_id)`**: a server player filled an empty map, after the filled map and its saved map data were created. Guest lookup helpers may not find the guest during packet handling.
- **`native_player_teleport_end_gateway(player, name, gateway, cancel)`**: a player is about to travel through the End gateway at `gateway`. Set `cancel` to leave them at the entrance; the gateway does not start its cooldown. The player is not swapped in, so helpers may find no guest through `self.lan`.
- **`native_player_recipe_book_click(player, name, recipe, all)`**: a server player requests a known recipe be placed in their crafting grid through the recipe book. `all` is true for the shift click form. The player is not swapped in, so helpers may find no guest through `self.lan`.
- **`native_player_exp_cooldown_change(player, name, ticks)`**: the player’s XP orb pickup cooldown changed, after its new value is stored. This includes the two tick delay after an orb is picked up and each tick it counts down. The player is not swapped in, so helpers may find no guest through `self.lan`.
- **`native_player_locale_change(player, name, locale)`**: a server player changed their client language option. It fires after the new locale is stored. The player is not swapped in, so helpers may find no guest through `self.lan`.
- **`native_entity_enter_block(entity, kind, pos, block)`**: a bee was stored in the beehive or nest at `pos`.
- **`native_entity_change_block(entity, kind, pos, from, to)`**: a mob changed a block, after the new state was applied.
- **`native_entity_break_door(entity, kind, pos)`**: a mob finished breaking a door at `pos`, after the block was removed.
- **`native_creeper_power(entity, lightning)`**: a lightning strike charged the creeper.
- **`native_creeper_ignite(entity, player, item)`**: a player used a creeper igniter on the creeper, after it entered its ignited state.
- **`native_exp_bottle(pos, experience)`**: an experience bottle broke and its orbs were created.
- **`native_firework_explode(entity, pos, bursts)`**: a firework rocket completed its damage and explosion effects.
- **`native_potion_splash(item, pos)`**: a thrown splash or lingering potion finished applying its hit effects at `pos`. Experience bottles are excluded.
- **`native_entity_transform(entity, kind, transformed, transformed_kind)`**: a mob conversion completed through the shared `Mob.convertTo` path.
- **`native_entity_enter_love_mode(entity, kind, player)`**: a mob entered love mode after a player interaction; `player` is the feeder UUID when known.
- **`native_entity_teleport(dimension, entity, from, to)`**: a server entity completed a same-level teleport command. `entity` is its network id; `from` and `to` are coordinates in `dimension`. This reports the move after it happens.
- **`native_entity_breed(parent, partner, child)`**: two mobs completed a breeding action. `child` is absent when the vanilla action lays an egg instead of creating a baby.
- **`native_entity_tame(entity, kind, owner)`**: a mob became tamed after its owner and persistent state were set. `owner` is the server player's UUID when known.
- **`native_item_spawn(entity, item, count, pos)`**: a dropped item entity was successfully added to the server level. The count is its stack size and `pos` is its spawn position.
- **`native_entity_pose_change(player, name, from, to)`**: a guest player's synchronized pose changed. Pose names are `STANDING`, `CROUCHING`, `SWIMMING`, `FALL_FLYING`, `SPIN_ATTACK` or `SLEEPING`.
- **`native_entity_toggle_glide(player, name, gliding)`**: a guest player's fall flying state changed after the server accepted or ended a glide.
- **`native_entity_shoot_bow(player, name, projectile, force)`**: a player fired an arrow from a bow or crossbow, after the arrow entered the level. `force` is the shooting power. This reports the completed shot.
- **`native_arrow_nock(player, name, bow, hand, has_ammo, cancel)`**: a player is about to start using a bow, where `BowItem.use` runs on the server (NeoForge's `ArrowNockEvent`). `has_ammo` is whether `getProjectile` found ammunition, always true in creative. Set `cancel` to fail the use: the bow is not drawn. Crossbows and tridents do not ask, and the client's own prediction does not.
- **`native_living_get_projectile(player, name, entity, kind, weapon, projectile)`**: the ammunition `LivingEntity.getProjectile(weapon)` returns (NeoForge's `LivingGetProjectileEvent`). Every mod gets the event with `projectile` set to the vanilla ammunition (the stack found in the hands or inventory, a mob's held projectile or a new arrow, a creative player's default arrow, or empty) and may change or empty it. It is asked on the bow's start and release, the crossbow's start and draw, the client's prediction of those starts, a skeleton's or illusioner's shot, and a pillager's or piglin's crossbow load (`entity` and `kind` name the mob, `player` is empty); a mob's arrow takes the replaced arrow's item and effect, a mob's crossbow also fires a replaced firework rocket, and an emptied stack leaves its crossbow uncharged and silent. A replaced stack is not taken from the inventory: the hook owns it.
- **`native_projectile_launch(player, name, entity, kind)`**: a projectile was added to the server level. `kind` is its entity type; `player` and `name` identify a player launcher, and are `None` and empty for other sources.
- **`native_entity_portal(dimension, entity, pos)`**: a server entity finished its portal wait and is beginning to leave through the portal block at `pos`. The destination follows the existing portal rules; the hook reports the transfer without changing it.
- **`native_furnace_extract(player, name, pos, item, count)`**: a server player takes furnace output at `pos`, after the output is removed; the player state is not swapped in.
- **`native_furnace_start_smelt(pos, item)`**: a furnace at `pos` begins cooking `item`, when its cooking timer first advances. Server-side.
- **`native_brewing_complete(pos)`**: a brewing stand finished brewing at `pos`, after the bottle contents changed.
- **`native_brewing_fuel(pos, fuel)`**: a brewing stand at `pos` consumed `fuel` and gained brewing fuel. Server-side.
- **`native_enchant_item(player, name, pos, item, levels)`**: a player applied an enchantment at `pos`; `item` is the finished item and `levels` the levels charged. Guest state is not swapped in.
- **`native_prepare_item_enchant(player, name, pos, costs)`**: enchanting offers were calculated at `pos`; change the three offer costs before they are sent to the player's menu. Guest state is not swapped in.
- **`native_prepare_anvil(player, name, pos, result, cost)`**: an anvil result was prepared for a player at `pos`; change the result or level cost before it is sent to their menu. Guest state is not swapped in.
- **`native_anvil_update(player, name, left, right, item_name, vanilla_output, vanilla_xp_cost, vanilla_material_cost, output, xp_cost, material_cost, canceled)`**: NeoForge's `AnvilUpdateEvent`, where `AnvilMenu.createResult` ends, each time the inputs or the rename box's `item_name` change while the left slot holds an item. `right` is none when that slot is empty, and `item_name` is none until the client sent one (empty clears a custom name). `vanilla_output`, `vanilla_xp_cost` and `vanilla_material_cost` are vanilla's result. `output`, `xp_cost` (the level cost) and `material_cost` (how many of `right` the take uses) start as vanilla's, and every mod in turn may change them. Set `canceled` to leave no result, with both costs at 0; later mods aren't called. A negative level cost becomes 0, and a result that costs 0 levels can't be taken. A material cost of 0 or less, or above what `right` holds, takes the whole stack (none when the anvil only renames). It fires on both sides: on a host (single player's server thread, a LAN or dedicated host) for each player's menu, before `native_prepare_anvil`, and in the window's own menu, whose predicted take then charges what the station shows. The window works its result out again after each click in the menu, so it may ask about the same inputs again: a mod should answer from what it is told. Guest state is not swapped in.
- **`native_anvil_craft_pre(player, name, pos, craft, cancel)`**: a player took an anvil's result at `pos` (NeoForge's `AnvilCraftEvent.Pre`), before the levels are charged, the inputs used up and the anvil worn. `craft` has the left and right inputs and the result as they were before. Set `cancel` to refuse the take and skip `native_anvil_craft_post`: the result stays in its slot and the inputs, levels, inventory and cursor are as before the click. Fires on the server only; the window's predicted take does not call it.
- **`native_anvil_craft_post(player, name, pos, craft)`**: the levels, inputs and anvil wear of that take are done (NeoForge's `AnvilCraftEvent.Post`); a broken anvil is already air. `craft` is the same as in `native_anvil_craft_pre`. Not called when it cancelled.
- **`native_prepare_grindstone(player, name, pos, result)`**: a grindstone result was prepared for a player at `pos`; change it before it is sent to their menu. Guest state is not swapped in.
- **`native_prepare_smithing(player, name, pos, result)`**: a smithing result was prepared for a player at `pos`; change it before it is sent to their menu. Guest state is not swapped in.
- **`native_prepare_item_craft(player, name, pos, result)`**: a crafting-grid result was prepared; `pos` is the crafting table, or None for the player's inventory grid. Change it before it is sent to the menu. Guest state is not swapped in.
- **`native_trade_select(player, name, villager, index)`**: a player selected offer `index` in a villager's trading menu; `villager` is its entity id when known. The menu has updated and the player's state is not swapped in.
- **`native_hopper_pickup_item(dimension, pos, item, count, cancel)`**: a hopper at `pos` is about to pick up a dropped stack; set `cancel` to leave it in the world.
- **`native_weather_change(dimension, raining, thundering, cancel)`**: the server's `/weather` command is about to set
  its final rain and thunder states in `dimension`. Set `cancel` to keep the previous weather parameters, including
  their timers.
- **`native_thunder_change(dimension, thundering, cancel)`**: the server's `/weather` command is about to change
  thunder. Set `cancel` to keep the previous thunder state and timer.
- **`native_lightning_strike(player, pos, cause, cancel)`**: a weather, Channeling-trident, or `/summon` lightning bolt
  is about to be created. Set `cancel` to prevent the bolt and its effects; `cause` names the creation path.
- **`native_time_skip(dimension, from, to, cause, cancel)`**: the server clock is about to jump after enough players
  sleep or through `/time set`, `/time add`, or `/time set <marker>`. Set `cancel` to keep the old time.
- **`native_sleep_finished_time(dimension, current, new_time)`**: enough players slept through the night and the
  `advance_time` rule is on (NeoForge's `SleepFinishedTimeEvent`). `new_time` comes in as the next morning's day time;
  set another to wake everyone then. A time before `current` is refused and the morning stays; the check runs once after
  every mod, so a later mod sees the value an earlier one set. It runs before `native_time_skip`, which hears the jump
  to the time chosen here and may still cancel it; the players wake either way. Bundles need
  `"hooks": 308`.
- **`native_world_border_bounds_changed()`**: `/worldborder set` or `/worldborder add` changed the border size or started a timed size change. Bundles need `"hooks": 245`.
- **`native_world_border_center_changed()`**: `/worldborder center` moved the border center. It hears successful commands only. Bundles need `"hooks": 246`.
- **`native_world_game_rule_change(player, name, rule, value)`**: an operator changed a game rule through the server game rules screen; it is heard after the final value is applied. Bundles need `"hooks": 247`.
- **`native_spawn_change(pos, angle)`**: `/setworldspawn` moved the world's spawn point, after its position and rotation are applied. Bundles need `"hooks": 244`.
- **`native_portal_create(dimension, pos)`**: a Nether portal built for dimension travel or the End arrival platform was completed in `dimension`; `pos` is its lower corner or platform origin. Bundles need `"hooks": 286`.
- **`native_raid_spawn_wave(raid, pos, wave, raiders)`**: a raid wave is about to be spawned at `pos`; `raiders` is the count Horizon will spawn.
- **`native_raid_state_change(raid, pos, state)`**: a raid at `pos` was triggered or changed to `victory`, `loss`, or `stopped`.
- **`native_world_saved(wait)`**: the server completed its world save cycle; `wait` is true when chunk writes were flushed before returning.
- **`native_entities_load(dimension, chunk, count)`**: saved entity records for `chunk` finished loading into `dimension`.
- **`native_entities_unload(dimension, chunk, count)`**: entities in `chunk` left the active world and were queued for saving.
- **`native_game_event(event, pos)`**: a generic world game event was fired at `pos`, including events with no nearby vibration listeners.
- **`native_tab_complete(player, name, command, range, suggestions)`**: a guest's server completion request was evaluated; mods may add or filter suggestions. `range` is the replaced byte range. Guest state is not swapped in, and helpers that look through `self.lan` may find nothing while the guest packet is handled.
- **`native_broadcast_message(line, cancel)`**: a system broadcast line is about to be logged and sent to every player. A mod may change `line` or set `cancel` to drop the broadcast.
- **`native_whitelist_state_update(enabled)`**: the dedicated server's player whitelist was enabled or disabled.
- **`native_player_bundle_item_select(player, name, slot, index)`**: a server player selected an item in a bundle;
  `slot` is the packet's menu slot and `index` is the chosen item index.
- **`native_player_ready_arrow(player, name, arrow)`**: a server player released a drawn bow, with `arrow` naming
  the selected projectile item. The guest's state is not swapped in while the packet is handled.
- **`native_prepare_hopper_transfer(pos)`**: a hopper at `pos` begins a transfer attempt, after its cooldown and enabled checks and before pushing or pulling items.
- **`native_inventory_move_item(source, destination, item, cancel)`**: a hopper is about to move one item between containers (a composter's bone meal too). Set `cancel` to leave it in the source slot. A chest boat or minecart is reported at the block it is in.
- **`native_hopper_inventory_search(hopper, container, entity)`**: a hopper selects a block container or container entity while searching for items.
- **`native_vault_loot(pos, item)`**: a vault is about to eject a loot item, before the item entity spawns.
- **`native_world_border_bounds_change_finish()`**: a timed world border size change finished on the server, after the border reached its final size.
- **`native_player_launch_projectile(player, name, item)`**: a server player launched a snowball, egg, ender pearl,
  potion or trident. It fires after the projectile is created and only for guest players in a LAN world.
- **`native_entity_unleash(entity, kind)`**: a mob's leash was removed, before any lead drop is handled.
- **`native_entity_drop_item(entity, item, count, pos)`**: a mob is about to drop `count` of `item` at `pos`.
- **`native_item_despawn(entity, item, count, pos)`**: a dropped item entity reached its 6000-tick lifetime and is
  about to be removed.
- **`native_explosion_prime(center, radius)`**: a server explosion is about to begin at `center` with `radius`.
  It is listen-only, so the explosion proceeds as usual; it also fires on the single-player server.
- **`native_vehicle_damage(kind, damage, cancel)`**: a player is about to damage a boat or minecart. Change `damage`
  or set `cancel` to leave its damage and wobble unchanged; the attack still completes. `kind` is `boat` or the
  minecart type. Server-side.
- **`native_vehicle_destroy(kind, pos)`**: a boat or minecart is about to be destroyed at `pos`. TNT minecarts that
  prime instead of breaking are not heard. Server-side.
- **`native_vehicle_create(kind, pos)`**: a boat or minecart joined the level at `pos`, after being placed by a
  player or dispenser. Server-side.
- **`native_player_toggle_sneak(player, name, sneaking)`**: a guest's player input changed sneaking, after the hub applied the state. The guest's state is swapped in; only actual changes are reported.
- **`native_player_input(player, name, flags)`**: a loaded guest sent a `PlayerInput` packet, after its flags were applied to the hub. `flags` is the packet's bit field, and the guest's state is swapped in.
- **`native_player_arm_swing(player, name, hand)`**: a guest's arm swing was handled and their attack strength reset. The packet currently swings `mainhand`; the guest's state is swapped in.
- **`native_player_jump(player, name)`**: accepted guest movement made the server player jump from the ground, after movement simulation finishes. The guest's state is swapped in.
- **`native_player_kicked(player, name, reason)`**: a guest was kicked after the hub recorded the rendered disconnect reason and sent the disconnect. The guest's state is swapped in, unless the guest was already removed in the same poll (a kick and the disconnect arriving together): the hook still fires, without it.
- **`native_player_level_change(player, name, from, to)`**: a server player's experience change altered their level, after the new level was applied. The guest's state is swapped in when the player is a guest.

#### Server hooks for plugin ports: console and log

- **`native_console_line(source, line, cancel, replies)`**: a line the dedicated server's console is about to run, before
  any command sees it (vanilla's, the dedicated-only ones such as `stop`, and `native_perform_console_command`'s).
  `source` is `console` for a line typed on the console or sent by `horizon-server` as the console (its console, `cmd`,
  the panel, the `save-off` around its backups) and `rcon` for an RCON command; `line` is as given, a leading slash
  included. Set `cancel` and it does not run. Push to `replies` to answer: each line is logged as the console's
  feedback is, so it also reaches the RCON client or the `horizon-server` caller. There is no player (the console is
  level 4). An audit log's `ServerCommandEvent`/`RemoteServerCommandEvent`. Bundles need `"hooks": @API:console_line@`.
- **`native_log_line(level, thread, message, line)`** (`horizon_client_lib::native_client_hooks`): a message was
  written to `logs/latest.log`, so a console bridge needs no tailing of the file. `level` is `INFO`, `WARN` or `ERROR`,
  `thread` the thread the line names, `message` the text and `line` what the file got (`[HH:mm:ss] [thread/LEVEL]:
  message`). It runs on the log's own writer thread after the file is written, never on the server or render thread,
  so a slow mod holds up no tick; it has no `Game`, and a line a mod logs from it comes back to it later (don't log
  each line you hear). Lines the flood limit leaves out are not heard, and a mod that panics in it is not called again.
  Bundles need `"hooks": @API:log_line@`.

#### Server hooks for plugin ports: connection

What a proxy-fronted backend, a plugin channel or a login gate needs, which plugin ports reached with raw injections
before (#10461).

- **`native_handshake(address, host, intent, cancel, reason)`**: a free function in `horizon_client_lib::lan::conn`,
  heard on the connection's own thread for each network connection's handshake (`ClientIntentionPacket`), before the
  server acts on `intent` (1 status, 2 login, 3 transfer). `address` is the peer's (behind a proxy, the proxy's; the
  forwarded address comes later, with the login), `host` the address the client says it connects to, with whatever a
  proxy put after it (Floodgate's `\0^Floodgate^...` data). A mod may rewrite `host` (later mods see the new one; the
  server itself reads nothing from it), or set `cancel` to refuse the connection: a login or a transfer is disconnected
  with `reason` (a plain "Disconnected" if unset), a status ping is closed unanswered. The legacy `0xFE` ping and an
  integrated server's own player are not heard. There is no `Game` on this thread, so no helper works and `native_tell`
  reaches nobody; keep the hook short, the connection waits for it. Bundles need `"hooks": @API:handshake@`.
  With proxy forwarding on (`forwarding.secret`, offline mode) the server reads the handshake's host up to 32767
  characters, as Paper does behind a proxy, so a Floodgate player's long data reaches the hook; without forwarding the
  limit stays vanilla's 255.
- **`native_custom_payload_from_client(player, channel, data)`**: a player's plugin channel message
  (`ServerboundCustomPayloadPacket` in play) on any channel that isn't Horizon's own (`horizon:mod_net`,
  `horizon:quiet_commands`, `horizon:lod`), which vanilla drops: a Simple Voice Chat client's `voicechat:*` requests,
  a proxy's or a client mod's channels. `data` is the client's bytes, checked by nothing. It runs in the tick after the
  message arrived, in arrival order, with the hub back and no player's state swapped in, so every helper works; it is
  heard from a dead player too. Every guest is heard (a single player world's own player sends none). The client side,
  a window's own plugin channels, is #6774. Bundles need `"hooks": @API:custom_payload_from_client@`.
- Helper **`native_send_custom_payload_to_player(player, channel, data)`** (`host.send_custom_payload_to_player`):
  sends `data` on a plugin channel (`namespace:path`, not Horizon's own `horizon:` ones, at most 1 MiB as
  `ClientboundCustomPayloadPacket` takes) to one player's client, a vanilla client included, as Bukkit's
  `Player.sendPluginMessage`. It is queued and leaves at the tick's end, so it works from any server hook, a player's
  own included; `Ok` means queued, and a player who isn't on by then gets nothing. Unlike `native_net_send_to_player`
  nothing caps or frames it. An error on a client, and for a bad channel or size. Bundles need
  `"hooks": @API:send_custom_payload_to_player@`.
- Helper **`native_player_permission(player)`** (`host.player_permission`): the permission level of a player who is on
  the server or joining it, 0 to 4, an operator's from `ops.json` above 0 (`PlayerList.getProfilePermissions`). It also
  works inside `native_player_login`, for the player that hook hears (their level is set by then), so a login gate that
  lets operators through needs no `ops.json` of its own. Inside a player's hook, with the hub out of reach, only that
  player is found; None for a player who isn't on, and on a client. Bundles need `"hooks": @API:player_permission@`.

#### Server hooks for plugin ports: players

- **`native_player_logging_out(player, name)`**: a player is leaving (lost connection, kick, quit), heard while they are
  still in their level and listed in `native_server_players`, before their menu closes and they are saved and removed
  (Bukkit's `PlayerQuitEvent`). Their state is not swapped in, so the helpers that take a UUID and `native_run_command`
  reach them. A death a mod causes here (`kill <name>`) happens at once where they stand, with its death message, drops
  and `native_player_died`, and they are saved dead. Once per placed player who leaves, before `native_player_logged_out`.
  Not heard when the server stops (`native_player_logged_out` tells then, with them still listed).
- **`native_player_hurt_by_player(attacker, attacker_entity, victim, victim_entity, source, damage_type, amount, canceled)`**:
  one player hurting another, both named by UUID and network entity id. `source` is `melee`, `sweep` or the projectile's
  entity type (`minecraft:arrow`, `minecraft:spectral_arrow`, `minecraft:snowball`, `minecraft:egg`, a mod's thrown type).
  Heard where `native_living_incoming_damage` is (the victim's `hurtServer` past its guards: the `pvp` rule and the teams
  allow it, the victim is not creative, a spectator or dead; a hit in the hurt cooldown is heard too) and just before it.
  Change `amount`, or set `canceled` and nothing of the hurt happens; v1 then hears nothing of it. A snowball or egg does
  no damage and vanilla's `Player.hurtServer` stops on zero: it is heard there with amount 0, and what a mod sets changes
  nothing. No `self`, like v1. Not heard: thorns, Channeling's lightning, fire, potion and tipped-arrow effects, thrown
  tridents (they hurt no players yet).
- **`native_player_spawn_location(player, name, dimension, position, rotation)`**: a joining player is about to be placed
  (Spigot's `PlayerSpawnLocationEvent`): after `native_player_login` and their saved data (or a newcomer's spawn), before
  the level and the world around them are chosen and sent. Change the dimension, feet position or yaw/pitch and they
  appear there, with no teleport afterwards; an unknown dimension or a non-finite value leaves that part as it was.
  No state is swapped in.
- **`native_item_picked_up_v2(player, stack, thrower, position, canceled)`**: an additive version of
  `native_item_picked_up` with the whole `ItemStack` (its components: enchantments, unbreakable, attribute modifiers, a
  custom name...), the item entity's thrower UUID (none for block and mob drops) and its position. Heard just before v1,
  which keeps its signature and firing point and hears nothing of a pickup canceled here.
- **`native_item_dropped_v2(player, stack, cancel)`**: an additive, cancellable version of `native_item_dropped`
  (NeoForge's `ItemTossEvent`) with the whole `ItemStack` thrown, heard just before v1, which keeps its signature and
  firing point. Set `cancel` and nothing is thrown: the stack goes back into the player's inventory (the held slot
  first; only what doesn't fit is thrown, unheard), and v1 hears nothing of it.
- **`native_creative_slot_v2(player, name, slot, stack, cancel)`**: an additive version of `native_creative_slot` with
  the whole `ItemStack` put in the slot (none for an emptied one), heard just before v1, which keeps its signature and
  firing point and hears nothing of a packet canceled here. The player's state is swapped in, as for v1.

#### Server hooks for plugin ports: world

Helpers for server mods that work on the levels as a whole (a pre-generator, a web map, a view distance tuner), on a
server only.

- **`native_force_chunk_in(owner, dimension, [x, z], hold)`** (`native_chunk_tickets.rs`): `native_force_chunk`
  ([Chunk tickets](#chunk-tickets)) for the level `dimension` (`minecraft:the_nether`), whichever level is in place. A
  level nobody is in, or one not open yet, opens at the next server tick and ticks while a ticket holds it, so a
  pre-generator walks the Nether and the End from any server hook (a console command, `native_level_tick_pre` of the
  Overworld) with nobody there. False for a dimension other than the Overworld, the Nether and the End. API
  413.
- **`native_world_border(dimension)`** (`native_world_border.rs`): the level's world border as the server enforces it,
  `[min_x, min_z, max_x, max_z]` in blocks (the previous size while it moves), for a web map's border line or a
  pre-generator's `worldborder` selection. Vanilla 26.3 keeps a border per level (`ServerLevel.getWorldBorder`, each
  level's `data/minecraft/world_border.dat`, `/worldborder` changing the level it runs in); Horizon keeps one for every
  level today, so the three levels answer the same bounds until that is fixed, and the helper then answers each level's
  own. None for another dimension. API @API:world_border@.
- **`native_set_player_view_distance(uuid, distance)`** (`native_view_distance.rs`): limits one player's view distance
  to `distance` chunks (2 to 32), None gives it the server's again. The player's distance is the smallest of the
  server's, what its client asked for and the limit; only that player is told its radius (`SetChunkCacheRadius`), and
  from its next view update it is sent, keeps and tracks chunks and entities within it, the others as before. The
  server still loads its own view distance round every player, so the limit can only lower a player's distance (raise
  the server's and limit the others to give one player more). It lasts until the player leaves.
  `native_player_view_distance(uuid)` is the distance the player has now. Both answer false/None for a player not online
  and inside a guest's packet hook, where the hub is taken out: call them from a tick or console hook. API
  @API:player_view_distance@.
- **`native_set_player_fake(uuid, fake)`** (`native_view_distance.rs`): marks the player as a fake player, a mod's bot
  with no client, or not. A fake player keeps only a small area loaded and ticking around it (its view distance plus
  one, at most the server's plus one), its packets are cut to the few a bot needs (position, entity motion, the chunk
  batch end, keep-alive, ping, combat kill and respawn), and it does not count for natural mob spawning. The mark lasts
  until the player leaves. False off a server, for a player not online, and inside a guest's packet hook: call it from a
  tick or console hook. API @API:set_player_fake@.
- **`native_expect_fake_player(name, fake)`** (`native_view_distance.rs`): says that a player named `name` (any case)
  will join as a fake player, or not any more. The player is fake from its first packet on, so its join sends and
  builds nothing a client-less player cannot read (no login records, recipe book, command tree, player list or
  advancements). False off a server. API @API:expect_fake_player@.
- **`native_is_player_fake(uuid)`** (`native_view_distance.rs`): whether the player is a fake player, a mod's bot with no
  client (marked by `native_set_player_fake`, or announced by name with `native_expect_fake_player`, which makes it fake from
  its first packet on). False off a server and for a player not online; for a join hook that gets only a uuid. API
  @API:is_player_fake@.
- **`native_chunk_snapshot(dimension, [x, z])`** (`native_chunk_snapshot.rs`): a `NativeChunkSnapshot` of a loaded
  chunk of a level that is open (in place or with nobody in it), None for a chunk not loaded (nothing is loaded for
  it). It shares the chunk's sections until the level changes them, so it is cheap to take on the server thread and
  can be read later on another one. It reads `block_state(x, y, z)` (a `BlockStateId`, for `native_state_block` and the
  state helpers), `surface(x, z)` (`WORLD_SURFACE`), `noise_biome(qx, qy, qz)` and `biome_cell(x, y, z)` (the quart
  cell vanilla's fuzzy biome zoom reads for a block, which can lie in a neighbouring chunk), `height()` and
  `has_ceiling()`, in the level's block coordinates; outside the chunk it answers None. To draw it:
  `native_block_map_color(state)` is `(MapColor id, MapColor.col)`, `native_biome_name(biome)` the biome's name and
  `native_biome_tint(biome, x, z)` its grass (with the dark forest and swamp modifiers), foliage, dry foliage and water
  colours (`BiomeColors`, after `native_biome_colors`, not blended). API @API:chunk_snapshot@.

#### Server hooks for plugin ports: permission nodes

Permission nodes beyond operator levels: a permissions mod (a LuckPerms port) answers, every other mod and the
server's command tree ask. Without such a mod every answer is the operator level, as before.

- **`native_permission_check(player, node, default_level, result)`**: whether a player has the permission node `node`
  (`essentials.tpa`, `minecraft.command.gamemode`), as Bukkit's `Permissible.hasPermission` and NeoForge's
  `PermissionAPI.getPermission` ask it. A permissions mod sets `result`; the first mod that sets it decides. Left None,
  the answer is the player's operator level against `default_level` (0 lets everyone). It is asked through `&self`
  (a provider reads what it knows) and often with the hub out of reach, for players on the server, joining or unknown
  to it, so answer from the mod's own data. Server-side. Bundles need `"hooks": @API:permission_check@`.
- Helper **`native_has_permission(player, node, default_level)`** (`host.has_permission`): asks
  `native_permission_check`, and when no mod answers, whether the player's operator level is `default_level` or above.
  The level is the server's for a player who is on or joining, the acting player's inside their hook; anyone else, and
  every player on a client, counts as level 0. Bundles need `"hooks": @API:permission_check@`.
- Helper **`native_player_permission_value(player, node)`** (`host.player_permission_value`): the node as the
  permissions mods set it, three-valued as Bukkit's `isPermissionSet` against `hasPermission`: Some(true), Some(false)
  (a mod set it to false, which is not the same as unset), or None when no mod answers `native_permission_check` (asked
  with a `default_level` of 0, no operator fallback). A mod applies its own default and parent-node inheritance on None.
  The operator level is `native_player_permission(player)`. Works for players on the server or not. Server-side.
  Bundles need `"hooks": @API:permission_value@`.
- **`native_player_commands_sent_v2(player, name, names, names_hidden)`**: an additive version of
  `native_player_commands_sent`, heard right after v1 at the same times (join, a change of level, a resend) with
  `names` as v1 left it. `names_hidden` lists the root commands an operator of level 4 would get that this player's
  level hides (`gamemode`, `op`, a mod's command above their level). Move one into `names` and the player's tree has it
  with everything under it, as an operator of its level gets it (Paper's `PlayerCommandSendEvent` after a permission
  check passed), so their client completes and highlights it; take a name out of `names` to hide it, as with v1. Only
  the tree changes: running a granted vanilla command still needs its level (`native_player_command_level`). Mods are
  asked in order and share both lists. Bundles need `"hooks": @API:player_commands_sent_v2@`.
- **`NativeCommand::permission(node)`** (`NativeCommand::new("tpa <player:player>").permission("essentials.tpa")`): a
  mod's command that a permission node decides, as Bukkit's `plugin.yml` `permission:`. Before the server puts it in a
  player's tree and before it runs a player's typed one, it asks `native_has_permission(player, node, level)` with
  the command's level as the default, so a permissions mod grants it to a player below that level or refuses it to an
  operator; without one the level decides, as for a command without a node. `native_player_commands_sent_v2` then
  hears a command its node refuses in `names_hidden` and one it grants in `names`. A command's node is the first one
  its usages give. A dedicated server's console runs every registered command without asking. Bundles need
  `"hooks": @API:command_permission@`.
- **`native_player_command_level(player, name, command, level)`**: the operator level one typed command of a player
  runs at, before the server checks it (Paper's `VanillaCommandWrapper`, which asks `minecraft.command.<name>` instead
  of the level). `level` comes in as the player's own; raise it to grant this command (`/gamemode` at 2 for a player of
  level 0) or lower it to refuse one; above 4 counts as 4. `command` is the line without its slash. The command then
  runs at that level: a vanilla one through its own checks, a mod's against its level (one with a permission node asks
  the node, with this level as the fallback). Heard after `native_player_command` for the commands vanilla's and the
  mods' dispatch run; not for the chat commands the server answers itself (`/me`, `/msg` and its aliases, `/say` from
  a player allowed it), `/list`, the chat filter's and Horizon's `/tempban` commands, nor a mod's quiet commands, and a LAN world without Allow Commands still refuses its guests' vanilla
  commands. The hub is out of reach and no player's state is swapped in: answer from the mod's own data. Server-side.
  Bundles need `"hooks": @API:player_command_level@`.
- Helper **`native_resend_commands(player)`** (`host.resend_commands`): sends a player their command tree again, with
  `native_player_commands_sent`, its v2 and the permission nodes of the mods' commands asked anew, as Bukkit's
  `Player.updateCommands`: after a permissions mod changed what they may use (a `/lp` edit, a sync from another
  server). The tree goes out in the next server tick, so it works from any server hook, the player's own included
  (calling it from the commands-sent hooks themselves would send the tree every tick). True when the player is on;
  false for anyone else and on a client. Bundles need `"hooks": @API:resend_commands@`.

#### Running commands from mods

- `native_run_command_captured(line) -> NativeCommandResult`: `native_run_command` with the answer kept (vanilla's
  `CommandSourceStack.withCallback` on a source that keeps what it is sent). `line` (a leading slash allowed) runs as
  the console; a command a mod registered goes to the mods (`native_perform_console_command`), anything else to
  vanilla's. `ran` says it ran without failing, `result` is vanilla's result value (the sum of the successful results,
  one per fork of `execute as @a run ...`; 0 for a failed `execute if entity`; 1 for a mod's command a mod ran), and
  `output` holds the lines it answered with, which reach no one else, operators included. Where `native_run_command`
  runs (no player swapped in); inside a player's hook nothing runs. Bundles need `"hooks": @API:run_command_captured@`.
- `native_run_command_as(player, line) -> NativeCommandResult`: `line` runs as that player typing it would (Bukkit's
  `Player.performCommand`): their source, operator level and position, so `/gamemode creative` for a level-0 player
  is refused with vanilla's error; a command a mod registered goes to the mods when their level allows it. The answer
  is sent to them as their own command's, and the same lines come back in `output`. `native_player_command` doesn't
  hear it. Where `native_run_command` runs, or inside that player's own hook (`native_perform_command`: the lines
  join its feedback); for anyone else, or a player not on the server, nothing runs. Bundles need
  `"hooks": @API:run_command_as@`.
- `native_run_mod_command(line) -> bool`: `native_run_command` that reaches the mods' commands too (Bukkit's
  `Bukkit.dispatchCommand` from the console): a command a mod registered goes to the mods
  (`native_perform_console_command`), anything else to vanilla's. True when a mod took it or vanilla's ran without
  failing; no output. Bundles need `"hooks": @API:run_mod_command@`.
- `native_command_info(label) -> Option<NativeCommandInfo>`: the server's command `label` names (Bukkit's
  `CommandMap.getCommand`), a leading slash allowed: one a mod registered, else vanilla's with an alias resolved (`tp`
  gives `teleport`, whose `aliases` hold `tp`). `usages` are the mod's registered usages, or vanilla's one line as
  `/help` gives it; `level` is the operator level a player needs, `vanilla` whether vanilla has it. None when no
  command has that name. Bundles need `"hooks": @API:command_info@`.
- Argument kinds that ask the server, for `native_register_commands`' usages: `word?` (a word) and `text?` (the rest
  of the line, last) are sent with vanilla's `ask_server` suggestions, so the player's client asks the server for
  their completions instead of completing nothing; `player_name` is a word that does the same, any name accepted
  (offline ones too, unlike `player`), and the server offers the online players' names matching it.
  **`native_tab_complete_v2(player, name, label, arguments, range, suggestions)`** hears every completion request
  right after `native_tab_complete`, the command split as Bukkit's `TabCompleter` gets it: `label` is its first word,
  `arguments` the words after it split at each space, the last the one being typed (empty after a space). For a
  command a mod registered, `range` is that word, and for `player_name` `suggestions` already holds the names; push
  yours over `range`. No player's state is swapped in and the server's players are out of reach
  (`native_server_players` gives none). Bundles need `"hooks": @API:tab_complete_v2@`.

#### Server hooks for plugin ports: Movement and teleports

Hooks before players and projectiles move, so a port can stop or redirect a move instead of sending the player back a
tick later with a command.

- **`native_player_move(player, name, from, to, yaw, pitch, on_ground, cancel)`**: a server player's position or look
  packet that changed either and passed the server's checks (moved too quickly, moved wrongly, the anticheat), before
  it is applied (Bukkit's `PlayerMoveEvent`). `from` and `to` are the feet before and after. Set `cancel` and the
  player is sent back to `from`; change `to` and they are put there instead (not checked against the blocks) and their
  client gets a teleport there. Not heard: riders, sleepers, moves while a teleport waits for its answer, teleports.
  Once a move, inside the packet's handling: keep it cheap. No `self`, no state swapped in. Server-side.
- **`native_player_teleport(player, name, cause, from_dimension, from, to_dimension, to, relative, cancel)`**: a server
  player is about to be teleported, before their client hears it (Bukkit's `PlayerTeleportEvent`, NeoForge's
  `EntityTeleportEvent`). `cause` is `command`, `ender_pearl`, `chorus_fruit`, `spectate` (the spectator teleport
  menu), `dismount` or `unknown` (another server move of the player). `to_dimension` differs from `from_dimension` only
  for a `command`, `spectate` or `unknown` (a mod's `native_teleport_player`) teleport into another level. Change `to` and they go there instead; set `cancel` and
  they stay where they are. Within a level `relative` is the position packet's flags: a mod may set or clear the look and motion bits (8, 16,
  32, 64, 128, 256; 0x1f8 keeps a pearl's speed as 26.2 did), the position bits are ignored; into another level it is
  not used. Not heard: portals and End gateways (their own hooks), respawns, a rider its vehicle carries, waking from a
  bed, a move the server's checks send back, the spectator camera. No `self`, no state swapped in. Server-side.
- **`native_player_respawn_position(player, name, dimension, pos, yaw, pitch)`**: a dead player is about to respawn
  (NeoForge's `PlayerRespawnPositionEvent`, Bukkit's `PlayerRespawnEvent.setRespawnLocation`), before they are placed:
  their respawn point (a bed or anchor block's top, a forced point, else the world spawn) and the look they get (0, 0).
  Left alone, the point is resolved as vanilla's is (the spot beside the bed, the world spawn's free column). Change the
  dimension or position and they are placed exactly there, no bed or anchor used; an unknown dimension or a non-finite
  value leaves that part as it was. On a LAN host's world the level stays the Overworld. No state swapped in.
- **`native_trample(dimension, entity, kind, pos, fall_distance, cancel)`**: a player or mob (`entity` its network id,
  `kind` its type) that fell `fall_distance` onto the farmland at `pos` is about to turn it to dirt, past vanilla's
  chance and checks (NeoForge's `FarmlandTrampleEvent`, Bukkit's `PlayerInteractEvent` `PHYSICAL` and
  `EntityInteractEvent`). Set `cancel` and the farmland and its crop stay; the fall damage is taken either way. Pressure
  plates and tripwires are `native_player_physical_trigger`. No `self`. Server-side.
- **`native_projectile_world_border(dimension, entity, kind, pos, velocity, collide)`**: a thrown projectile (pearl,
  snowball, egg, potion...) inside the world border whose next step would end outside it, before 26.3 stops it on the
  border like a wall (MC-310476). A mod may change `pos` and `velocity`, or set `collide` false and it flies through
  this tick as before 26.3. Arrows and tridents, which Horizon does not stop at the border, are not heard. No `self`.
  Server-side.

#### Server hooks for plugin ports: Inventories

- **`native_player_launch_projectile_v2(player, name, stack, cancel)`**: a guest is about to throw `stack` (a snowball,
  an egg, an ender pearl, a mod's thrown item), heard where `ProjectileItem.use` creates the projectile, before it
  exists (Bukkit's `ProjectileLaunchEvent` for a player). Set `cancel` and nothing is thrown, used or put on cooldown,
  and `native_player_launch_projectile` (v1, which keeps its signature and firing point) hears nothing of it; the
  client, which took the item at once, gets its inventory back. The guest's state is swapped in. Not heard: bows,
  crossbows, tridents, splash and lingering potions, experience bottles, wind charges and fireworks.
- **`native_inventory_click(player, name, window, slot, button, mode, cancel)`**: a guest's click in their own
  inventory (`window` 0) or a vanilla menu they have open (a chest, a furnace, a crafting table, a station, a merchant,
  a mount), heard before it is applied (Bukkit's `InventoryClickEvent`), with the packet's `slot` (the window's
  numbering, -999 outside), `button` and `mode` (`ClickType`'s id: 0 pickup, 1 quick move, 2 swap, 3 clone, 4 throw,
  6 pickup all). Drags (mode 5) are heard once, at their end, by `native_inventory_drag`; a mod's own menus are `native_menu_clicked`'s; spectators are not
  heard. Set `cancel` and nothing moves: the click is handled as one that does nothing, so the client's predicted
  slots and cursor are answered with the server's. No state is swapped in; the helpers that take a UUID reach them.
- **`native_inventory_drag(player, name, window, kind, slots, cancel)`**: a guest's drag (quick craft) in their own
  inventory or a vanilla menu ends, heard before it is applied (Bukkit's `InventoryDragEvent`): `kind` 0 spreads the
  cursor evenly (left button), 1 puts one in each slot (right), 2 clones (middle, creative), and `slots` are the window
  slots the client dragged over, in order (the menu still skips those that cannot take the stack). Set `cancel` and
  nothing moves: the drag ends empty and the client's predicted slots and cursor are answered with the server's.
- **`native_block_place(player, name, stack, pos, state, cancel)`**: a guest is about to place `stack` as the block
  `state` at `pos`, heard where `BlockItem.place` has found the position and the state and checked that the block
  survives there unobstructed, before anything is set (Bukkit's `BlockPlaceEvent`, NeoForge's
  `BlockEvent.EntityPlaceEvent`). Set `cancel` and nothing is placed or used (a failed placement, so the hand does not
  swing); the client sees the blocks and its inventory as they are, and `native_place_block` hears nothing. The guest's
  state is swapped in. `state` is `getStateForPlacement`'s, before the stack's `block_state` component; finding it asks
  `native_state_for_placement` and `native_can_survive` once more. Unlike claiming `native_use_item_on`, the block's
  own use (a door, a chest) and the item's other uses are left alone.
- Helper `native_set_inventory(uuid, kind, slots, selected) -> Result<(), String>`: replaces a player's whole
  inventory (`Player`: 41 slots in `native_inventory`'s numbering) or ender chest (`Ender`: 27), as Bukkit's
  `PlayerInventory.setContents` does: live when they are on the server, else in their file, never while they log in,
  are dead or while a player's state is swapped in (call it from `native_server_tick`). For `Player`,
  `selected` (0..9) becomes the held hotbar slot, the cursor and the 2x2 crafting grid are emptied, and a player on
  the server is sent the whole inventory at once. Err for a wrong number of slots.
- Helpers `native_set_inventory_slot(uuid, kind, slot, expected, new) -> Result<Option<ItemStack>, String>` and
  `native_take_stacks(uuid, slots) -> Result<Vec<ItemStack>, String>`, checked like `native_move_stack` and live or in
  the file like it: the first puts `new` in one slot (armour and the off hand too), which must still hold exactly
  `expected`, and returns what was there (Bukkit's `setItem`); the second takes the stacks out of the given `Player`
  slots, each of which must still hold exactly its stack, all or none (a stake, a trade, a fine with enchanted gear).
  A player's screen follows with the tick's sync.
- **`native_player_inventory_changed(player, stacks)`**: a player's inventory changed, heard at the moment vanilla's
  `InventoryChangeTrigger` is checked (the per-tick check found another item or count in a slot), before any criterion
  is looked at and whether or not any advancement uses `inventory_changed`. `stacks` are the 41 slots in
  `native_inventory`'s `Player` numbering: 0..36 the main inventory (0..9 the hotbar), 36..40 the armour (feet, legs,
  chest, head), 40 the off hand. It also fires at a player's first check after joining, so a mod sees what they log in
  with. The player's state is swapped in. Heard for the single player, LAN guests and a dedicated server's players,
  never on a client of a remote server: what quest, collection and "obtain an item" mods need instead of polling
  every inventory. Bundles need `"hooks": @API:player_inventory_changed@`.

#### Server hooks for plugin ports: login and profiles

What a skin, login or profile mod needs, which plugin ports reached with raw injections or their own `usercache.json`
parsing before (#10461).

- **`native_player_pre_login(player, name, address, defer)`**: a free function in `horizon_client_lib::lan::conn`,
  heard on the connection's own thread for a player logging in, once the session service or the proxy has named them
  and before the server's own checks (Bukkit's `AsyncPlayerPreLoginEvent`). `address` is theirs (behind a proxy, the
  forwarded one). A mod that needs time, a permissions or inventory load from a database, or may refuse, pushes a
  `Receiver<Result<(), Text>>` onto `defer` and answers on it from any thread: `Ok` lets the player on, `Err` refuses
  them with that reason. The login waits for every answer, 20 s at most in all (a vanilla client gives up after 30 s
  of silence); an answer that doesn't come by then, or whose sender is dropped, refuses with a plain "Disconnected".
  Mods' loads run side by side. The integrated server's own player is not heard, and there is no `Game` on this
  thread. Bundles need `"hooks": @API:player_pre_login@`.
- **`native_player_profile(player, name, properties)`**: a free function in `horizon_client_lib::lan::conn`, heard on
  the connection's own thread once the server has let a player in (name, whitelist, bans and player limit checked) and
  before it sends `LoginFinished` (Velocity's `GameProfileRequestEvent`). `properties` are the profile's, as the session
  service (online mode) or the proxy (forwarding) gave them, none in offline mode. A mod may change, add or remove them,
  within `GameProfile`'s limits (names up to 64 characters, signatures up to 1024): the player's own client and every
  player list get the new ones, so a skin mod sets a stored `textures` here before anyone sees the old one. The
  integrated server's own player is not heard. There is no `Game` on this thread, so no helper works; the hook may block
  for a lookup, the login waits for it (a vanilla client gives up after 30 s of silence). Bundles need
  `"hooks": @API:player_profile@`.

- Helpers **`native_set_profile_property(player, name, value)`** (`host.set_profile_property`) and
  **`native_refresh_player(player)`** (`host.refresh_player`): a skin changed while the player is on (Paper's
  `Player.setPlayerProfile`, then `hidePlayer`/`showPlayer`). The first sets the property `name` of an online player's
  profile to `value` (its value and signature, None unsigned) or removes it with None; every player list entry of
  theirs carries it from then on, but nothing is sent. False on a client and past `GameProfile`'s limits (64, 32767
  and 1024 characters). Where the hub is out of reach (inside a player's own hook) the change applies at the tick's
  end. The second shows the player again, at the end of their level's pass: everyone who sees them gets their list
  entry anew and their entity again, and the player is respawned in place into the level they are in, all state kept,
  so their own client draws the new skin too (a dead player only to the others). Both work from any server hook,
  `native_server_tick` included; true does not mean the player was found. Bundles need
  `"hooks": @API:profile_refresh@`.

- Helpers **`native_player_textures(uuid)`** (`host.player_textures`) and **`native_player_head(uuid)`**
  (`host.player_head`): the `textures` property of a player who is on, its value and signature as their player list
  entry carries it (Paper's `Player.getPlayerProfile`), and a `minecraft:player_head` stack of any player (Bukkit's
  `SkullMeta.setOwningPlayer`) whose `minecraft:profile` holds their id, plus their name and skin when they are on,
  so it shows the skin at once (a profile of an id alone the client resolves itself). Where the hub is out of reach
  (inside a player's own hook) the textures are None and the head carries the id only. Bundles need
  `"hooks": @API:player_textures@`.

- Helpers **`native_profile_by_name(name)`** (`host.profile_by_name`) and **`native_profile_by_uuid(uuid)`**
  (`host.profile_by_uuid`): a player the server knows, by name in any case (their id and name as written) or by id
  (their name): one who is on, else one in a dedicated server's profile cache, `usercache.json` (vanilla's
  `GameProfileCache`, Bukkit's `getOfflinePlayer` without its network lookup). They ask no profile service and make
  up no offline player; None when nobody is found. Bundles need `"hooks": @API:profile_cache@`.

#### Server hooks for plugin ports: player state

Helpers that read a player's state and write it back, what a mod keeping players in step across servers (a HuskSync
port) needs. They reach the server's own players, as `native_server_players` lists them: inside a player's turn (their
hook, their command) only that player, outside one (`native_server_tick`, `native_menu_clicked`) everyone on. A write
goes through the player's turn as `/effect` and `/attribute` do, and their client follows with the tick's sync. Nothing
calls them without a mod.

- Helper **`native_player_compound(uuid)`**: the player's compound as the server would save it now
  (`players/data/<uuid>.dat`, vanilla's `ServerPlayer.saveWithoutId`), with every key Horizon keeps without modelling
  it. None inside a player's turn (their live state is out of their record) and for a player not on.
  Bundles need `"hooks": @API:player_compound@`.
- Helpers **`native_player_food(uuid)`** and **`native_set_player_status(uuid, health, food, xp)`**: the food level,
  saturation and exhaustion (`FoodData`); and health, food `(level, saturation, exhaustion)` and experience
  `(level, progress, total)` set, each when given, as Bukkit's `setHealth`, `setFoodLevel`, `setSaturation`,
  `setExhaustion`, `setLevel`, `setExp` and `setTotalExperience`. Health is capped at the player's maximum; 0 or less
  kills them as `/kill` does, with the death message "<player> died" (Bukkit's `setHealth(0)`). The mod's food is not
  heard by `native_food_level_changed`, except inside the player's own turn. False for a player not reached, a dead one,
  a value that is not a number, a negative level or total, or a progress outside 0 to 1.
  Bundles need `"hooks": @API:player_status@`.
- Helpers **`native_effects(uuid)`**, **`native_add_effect(uuid, effect)`**, **`native_remove_effect(uuid, id)`** and
  **`native_set_effects(uuid, effects)`**: a player's effects as `NativeEffect { id, amplifier, duration, ambient,
  particles }` (`MobEffectInstance`; duration -1 is infinite); one given as `LivingEntity.addEffect` gives it (Bukkit's
  `addPotionEffect`: a weaker or shorter one of the kind waits under it), one taken off as `/effect clear` takes it, or
  all replaced (a saved player's potion effects restored). The client gets `UpdateMobEffect` with the ambient and
  particle flags. Horizon keeps no hidden-icon flag on an effect, so the icon always shows. Unknown effects and a
  duration of 0 or below -1 are refused. Bundles need `"hooks": @API:player_effects@`.
- Helpers **`native_attribute(uuid, attribute)`**, **`native_set_attribute_base(uuid, attribute, base)`** and
  **`native_set_attributes(uuid, saved)`**: an attribute's base value and its value with every modifier (Bukkit's
  `AttributeInstance.getBaseValue` and `getValue`); its base set as `/attribute .. base set` does; a saved
  `attributes` list (the compound's) loaded as `AttributeMap.load` reads a player's file. A lower `max_health` caps the
  health, and the client gets `UpdateAttributes`. An id without a namespace is `minecraft:`'s.
  Bundles need `"hooks": @API:player_attributes@`.
- Helpers **`native_player_persistent_data(uuid)`** and **`native_set_player_persistent_data(uuid, data)`**: the
  compound mods keep on a player (NeoForge's `Entity.getPersistentData()`, Bukkit's `PersistentDataContainer`), saved in
  their file as `NeoForgeData`; an empty compound when there is none, and
  setting an empty one takes it away. Mods share it: keep your keys under your namespace.
  Bundles need `"hooks": @API:player_persistent_data@`.

#### Server hooks for plugin ports: Moving players and vehicles

Helpers that move a server player or a vehicle from a mod, so a port no longer runs `execute in .. run tp` from the
console (which waits for the next tick and does nothing inside a player's hook).

- Helper **`native_teleport_player(uuid, dimension, pos, yaw, pitch)`** (`host.teleport_player`): teleports a server
  player to `pos` in `dimension` (`minecraft:the_nether`), facing `yaw` and `pitch`, as `/tp` does (Bukkit's
  `Player.teleport`): they leave their vehicle, lose their motion and fall, and their client is told. Callable from any
  server hook. Within their level it is done at once when they are the acting player or nobody's state is swapped in
  (a tick hook); inside another player's hook it is done after the players' packets of the server tick (the same tick
  when the hook heard a packet). Into another level it happens when the tick's level teleports run, as a command's.
  True when done or queued; false for a player not on the server or dead (where that can be seen), another dimension
  than the three, a position outside `/tp`'s bounds, an angle that is not finite, and on a client. Server-side.
  Bundles need `"hooks": @API:teleport_player@`.
- Helper **`native_teleport_vehicle(dimension, id, pos)`** (`host.teleport_vehicle`): teleports the boat, minecart or
  mob a rider sits on (a horse, a pig) with entity id `id`, of the level in place, to `pos` with its riders, as `/tp`
  does a vehicle (Bukkit's `Vehicle.teleport`): they stay seated and move by the same offset, and a server player riding
  it is told, as their client drives the vehicle itself. Only where nobody's state is swapped in (a tick hook); inside
  `native_vehicle_moved_v2` change `to` instead. False inside a player's hook, for no such vehicle here, another
  dimension, a position outside `/tp`'s bounds, and on a client. Server-side. Bundles need
  `"hooks": @API:teleport_vehicle@`.
- **`native_vehicle_moved_v2(player, dimension, vehicle, kind, from, to, cancel)`**: an additive version of
  `native_vehicle_moved`, heard right after v1 at the same times and with the same state swapped in, and also for a mob
  a server player rides (a horse, a pig, a strider), which v1 does not hear. `vehicle` is the entity id and `dimension`
  its level. Set `cancel` and the vehicle is put back at `from` with its riders; change `to` and it is put there instead
  (not checked against the blocks; a value that is not finite is ignored), as Bukkit's `VehicleMoveEvent` with
  `Vehicle.teleport`. Either way its riders' clients are told, and the next move is heard from there. Mods are asked in
  order and share `to` and `cancel`. Server-side. Bundles need `"hooks": @API:vehicle_moved_v2@`.
- Helpers **`native_mount(entity, vehicle)`**, **`native_dismount(entity)`** and **`native_eject_passengers(vehicle)`**
  (`host.mount`, `host.dismount`, `host.eject_passengers`), by entity id in the level in place: seat a server player or
  a mob on a boat, minecart or mob, as `/ride <entity> mount <vehicle>` does with its checks (neither rides already, no
  loop, a free seat, minecarts carry players only); take one off what it rides, as `/ride <entity> dismount`, landing
  beside the vehicle; or take every server player and mob off a vehicle (Bukkit's `addPassenger`, `leaveVehicle`,
  `eject`). `native_entity_mount` may refuse each. Inside a player's hook only the acting player and mobs are reached.
  True when it rides, got off, or one got off; false otherwise and on a client. Server-side. Bundles need
  `"hooks": @API:mount@`.
- Helper **`native_set_flight(uuid, allow, flying)`** (`host.set_flight`): lets a server player fly in any game mode
  (`allow`) or not, and sets whether they fly now (`flying`), as Bukkit's `Player.setAllowFlight` and `setFlying`
  (`Abilities.mayfly` and `flying`); their client is told, and the server's flying check no longer kicks them. A player
  who may not fly does not fly; creative and spectator players fly whatever `allow` is. Kept until their game mode
  changes, they respawn or they leave (vanilla sets it for the mode then); not saved. Done at once where nobody's state
  is swapped in, else after the players' packets of the server tick. True when done or queued; false for a player not
  on the server (where that can be seen) and on a client. Server-side. Bundles need `"hooks": @API:set_flight@`.
- Helper **`native_respawn_player(uuid)`** (`host.respawn_player`): respawns a dead server player as their client's
  Respawn click does (`ClientCommand` `PERFORM_RESPAWN`; Spigot's `player.spigot().respawn()`): at their bed, anchor or
  the world spawn, with the respawn hooks, without waiting for the click. Done after the players' packets of the server
  tick (the same tick from a packet's hook or the tick's start, else the next). True when queued; false for a player
  seen not on the server or alive (inside another player's hook nobody else can be seen: it is queued and does nothing
  for a living player), and on a client. Server-side. Bundles need `"hooks": @API:respawn_player@`.

#### Server hooks for plugin ports: Reading players

What a server mod reads about a player besides their position and health. These helpers find the acting player
(whose state a player's hook swaps in) and every player the hub holds; inside another player's hook, where the hub
is out of reach, only the acting player is found and the others read as None. Server-side: on a client they give
None.

- **`NativeServerPlayer`**, as `native_server_players()` and `native_acting_player()` give it, also carries
  `latency` (the round-trip in ms, as the player list shows it), `protocol`, `xp_level` and `xp_progress`,
  `on_ground`, the abilities the client is told (`flying`, `may_fly`, `invulnerable`, `walk_speed`, `fly_speed`),
  `sneaking`, `eye_height` in the current pose (1.62 standing, 1.27 crouching), `selected_slot`, and the `nickname`
  and `hidden` level server mods set (Bukkit's `getPing`, `isOnGround`, `getAllowFlight`, `isFlying`,
  `isSneaking`, `getEyeHeight`, `getDisplayName`). For the acting player, `latency`, `nickname`, `hidden` and
  `address` read 0 or None while the hub is out of reach. Bundles need `"hooks": @API:server_player_state@`.
- **`NativeServerPlayer.fake`** is true for a fake player, a mod's bot with no client (`native_set_player_fake`), so a
  mod can leave out what only a client would show. Bundles need `"hooks": @API:server_player_fake@`.
- Helper **`native_player_client(uuid) -> Option<NativeClientInfo>`** (`host.player_client`): the client's brand,
  language, the view distance it asks for, chat visibility, main hand and particle setting, the chunks the server
  sent it and its wire bytes in and out (Paper's `getClientBrandName`, `locale`, `getClientViewDistance`). It
  reads the hub, so it is None inside a player's hook while the hub is out of reach, the acting player's too.
  Bundles need `"hooks": @API:player_client@`.
- Helpers **`native_player_stats(uuid)`**, **`native_player_stat(uuid, stat_type, key)`** and
  **`native_set_player_stats(uuid, stats)`** (`host.player_stats`, ...): the player's statistics as vanilla's
  `players/stats/<uuid>.json` holds them (`{"stats": {"minecraft:custom": {"minecraft:jump": 3}}, "DataVersion":
  ..}`), one counter of them (0 when never counted; `custom`, `jump` mean `minecraft:custom`,
  `minecraft:jump`), and their replacement from that JSON (a HuskSync restore). Replaced statistics are saved with
  the player and all go to the client at its next request; scoreboard objectives that count one are not told.
  **`native_player_advancements(uuid)`** and **`native_set_player_advancements(uuid, advancements)`** do the same
  for `players/advancements/<uuid>.json` (each advancement's criteria with their dates, and `done`); advancements
  this server lacks are dropped as when the file loads, and the client gets every advancement's progress again
  with the player's next tick (one newly done shows its toast, as a grant would, with no rewards). The setters say whether the player was found. Bundles need
  `"hooks": @API:player_records@`.
- Helpers **`native_max_players()`** (`max-players`, as `/list` shows it; None while the hub is out of reach),
  **`native_server_dir()`** (a dedicated server's folder, absolute; None elsewhere) and **`native_log_file()`** (the
  `logs/latest.log` this run writes, absolute; None when it logs to the terminal only), as Bukkit's
  `Server.getMaxPlayers` and a plugin's server folder. Bundles need `"hooks": @API:server_files@`.
- Helpers **`native_held_stack(uuid)`** and **`native_player_equipment(uuid, slot)`** (`host.held_stack`,
  `host.player_equipment`): the whole stack, components included, in a player's main hand or `EquipmentSlot`, from
  any server hook and for any player, not only the acting one (Bukkit's `getEquipment().getItemInMainHand()` and
  `getItem(slot)`). None for an empty slot and for `Body` and `Saddle`. Bundles need `"hooks": @API:player_equipment@`.

#### Server hooks for plugin ports: per-player feedback

What Bukkit's and Adventure's `Player` methods send one player, which plugin ports built as raw packets through
`native_send` before: a sound, particles, a title, boss bars and the compass target. Only
that player is sent anything; the world, its `/bossbar` bars and the other players are untouched. NeoForge has no
per-player helpers for these (a NeoForge mod sends `ServerPlayer.connection.send` itself), so none is named.

- Helper **`native_play_sound_to(uuid, sound, category, at, volume, pitch)`** (`host.play_sound_to`): a
  `ClientboundSoundPacket` to that player alone, as Bukkit's `Player.playSound`, at `at` in their level or, with None,
  where they stand when it leaves. `category` is a `SoundSource` name (`master`, `music`, `record`, `weather`, `block`,
  `hostile`, `neutral`, `player`, `ambient`, `voice`, `ui`). As for `GameLevel::native_play_sound`, an unknown
  `minecraft:` event is dropped and another namespace's event goes by name (a client hears it if its resources define
  it). No range applies: their client fades it with distance as any sound. It leaves at the tick's end, so it works
  from any server hook, a player's own included; true means queued. False on a client, for a bad name, category,
  position, volume or pitch, and for a player who isn't on (inside another player's hook, where the players are out of
  reach, that player then gets nothing). Bundles need `"hooks": @API:play_sound_to@`.
- Helper **`native_send_particles_to(uuid, options, state, at, spread, speed, count, force)`**
  (`host.send_particles_to`): particles for that player alone, as vanilla's `ServerLevel.sendParticles(player, ...)`
  (`/particle ... normal|force <player>`) and Bukkit's `Player.spawnParticle`: sent when `at` is within 32 blocks of
  the centre of their block, 512 with `force`, which also sets the packet's `overrideLimiter` (the client draws them
  whatever its Particles option says). Options and their checks are `GameLevel::native_send_particles`'s: block
  particles need `state`, item, trail and vibration particles aren't sent, a mod's own type
  (`ParticleOptions::named`) reaches only a Horizon client. Queued as `native_play_sound_to`, with the same answers.
  Bundles need `"hooks": @API:send_particles_to@`.
- Helper **`native_title(uuid, title, subtitle, fade_in, stay, fade_out)`** (`host.title`): a title and subtitle in
  the middle of that player's screen, as `/title` and Adventure's `showTitle`: the times (in ticks; vanilla's default
  is 10, 70, 20), the subtitle and the title, in that order, replacing a title shown before; an empty subtitle shows
  none. Sent at once, as `native_tell`: also inside a player's hook for the acting player, whose packets go after the
  hook. False on a client, for a negative time, and for a player who isn't on or can't be reached from here (another
  player inside a player's hook). Bundles need `"hooks": @API:title@`.
- Helper **`native_boss_bar(uuid, id, bar)`** (`host.boss_bar`): shows that player the boss bar `id` (a UUID the mod
  picks; a random one keeps clear of the world's) as a `NativeBossBar` (`name`, `progress` 0 to 1, `color` and
  `overlay` as `BossBarColor` and `BossBarOverlay` ordinals, the `darken`, `music` and `fog` flags), changes it, or
  takes it away with None, as Bukkit's `BossBar.addPlayer`/`removePlayer`. The game's boss bar update sends it with the
  world's bars (`ServerBossEvent`'s packets: a change is one packet, a new bar is added under the mod's earlier ones,
  after the `/bossbar` bars and before a boss's). By UUID: kept for a player who is off, who sees it again when they
  come back, until a mod takes it away or the server stops. Works from any server hook. False on a client, and for a
  progress outside 0 to 1 or a colour or overlay `BossEvent` hasn't. Bundles need `"hooks": @API:boss_bar@`.
- Helper **`native_set_compass_target(uuid, target)`** (`host.set_compass_target`): points that player's compass at
  `Some((dimension, pos))`, or at the world spawn again with None, as Bukkit's `Player.setCompassTarget`. The
  `ClientboundSetDefaultSpawnPositionPacket` they are sent carries the target in place of the world spawn, now and
  every time the server sends the world spawn again (a respawn, a trip to another level), until a mod sets it again or
  they leave. A compass points there only in that dimension (elsewhere it spins, as for the world spawn; a Horizon client so far
  keeps only an Overworld target, `remote.rs`); a lodestone
  compass and a recovery compass keep their own targets. Queued as `native_play_sound_to`, with the same answers; also
  false for a dimension that isn't a namespaced id or a position a block position can't carry. Bundles need
  `"hooks": @API:compass_target@`.

#### Server hooks for plugin ports: packets

What packet-level plugins (ProtocolLib, PacketEvents, TAB), plugin channel listeners, resource pack plugins and
`/kick` replacements need, which plugin ports reached with raw packets and injections before.

- **`native_server_packet(player, packet, cancel)`**: each play packet the server is about to send one guest's
  client, decoded (`ClientboundPlayPacket`), in the order they leave; a broadcast is heard once for each player it
  reaches, so a mod can answer each viewer differently (ProtocolLib's and PacketEvents' outgoing listeners; there is no
  NeoForge or Bukkit event). Change `packet` and that client gets the changed packet, encoded again; set `cancel` and
  it gets nothing. It is heard only after a mod called the helper **`native_watch_server_packets()`** (any thread,
  any time; it cannot be switched off again): until then the server neither decodes nor copies what it sends, and each
  packet costs one atomic load. Not heard: the configuration phase, the disconnect a kick sends, bytes that don't decode
  (they go out unchanged, as does every packet no mod changed), and a player who isn't a guest (single player's own). It
  runs where the packet is sent, on whichever server thread sends it, with no `self` and no player's state swapped in, once for
  each packet and player: keep it cheap. Bundles need `"hooks": @API:server_packet@`.
- **`native_entity_data_to(viewer, entity, values)`**: the typed shortcut for one packet, the entity data
  (`ClientboundSetEntityDataPacket`) the server is about to send the guest `viewer` for the entity with network id
  `entity`. `values` are its `(index, serializer id, value)` entries (`EntityDataValue`, `SynchedEntityData.DataValue`)
  in the order sent; change them and only that viewer gets the change, so one viewer can see a player glow (OR `0x40`
  into the `Byte` of entry 0) or another custom name, while the server and every other viewer keep the real values.
  Heard just before `native_server_packet` hears the same packet, with the changed values. Switched on by the helper
  **`native_watch_entity_data()`**, which decodes this packet alone (the other packets keep going out undecoded).
  Every entity's data reaches a client in this packet, a newly tracked one's first values included; an entity whose
  values are all defaults is tracked without one, as in vanilla, so there is nothing to change until a value does.
  The same threads and limits as `native_server_packet`. Bundles need `"hooks": @API:entity_data_to@`.
- **`native_player_register_channels(player, name, channels, register)`**: a guest's client registered plugin channels
  (`minecraft:register`, `register` true) or unregistered them (`minecraft:unregister`), NUL-separated names as Bukkit's
  and Fabric's clients and proxies send them; vanilla reads neither message (Bukkit's `PlayerRegisterChannelEvent` and
  `PlayerUnregisterChannelEvent`). `channels` are the names that changed, after Horizon applied them. What a client
  registered during its configuration, before it joined, is heard once as one registration right after
  `native_player_logged_in`, with the hub in place; a message in play is heard as it is handled, from a dead player too,
  with the player's state swapped in, so `native_tell` answers them, and then goes on to wherever it went before. Horizon
  keeps at most 128 names of at most 256 bytes, printable ASCII with a `:`; a name past that, or one that changes
  nothing, is not heard. The helper **`native_player_channels(player)`** lists the channels a player has, in the order
  registered (Bukkit's `Player.getListeningPluginChannels`); it reads the hub, so it is empty inside a player's own
  hook. Bundles need `"hooks": @API:player_channels@`.
- Helpers **`native_send_resource_pack(player, id, url, sha1, prompt, required)`** and
  **`native_pop_resource_pack(player, id)`**: a mod's own resource packs for one player's client, as Bukkit's
  `Player.addResourcePack` and `removeResourcePack(s)`. The push (`ClientboundResourcePackPushPacket`) carries the
  mod's own `id`, a UUID it picks, so it can take that pack away again and tell its answers from other packs' with
  `native_resource_pack_status_v2`; `sha1` is the pack's lowercase hex hash (empty: unchecked), `prompt` an extra line
  on the client's prompt, `required` how the prompt words it. A pack whose id the client already has is replaced.
  As in vanilla, a player who declines is kicked only when the server's own pack (`require-resource-pack`) is
  required, whatever pack they declined; anything else is the mod's to decide. `native_pop_resource_pack(player, None)`
  takes away every server pack the client has. Both send as `native_send` does (from `native_server_tick` and from the
  player's own hooks) and are false when the player isn't on, on a client, and for an empty URL, a URL past 32767
  characters or a hash past 40. Bundles need `"hooks": @API:resource_packs@`.
- **`native_resource_pack_status_v2(player, name, pack, status)`**: an additive version of
  `native_resource_pack_status` with the answered pack's id (`ServerboundResourcePackPacket.id`, Bukkit's
  `PlayerResourcePackStatusEvent.getID`), so a mod hears its own pack's answers apart from every other pack's;
  `status` is the same `Action` number. In play it is heard just before v1, with the player's state swapped in. The
  answers to the server's own pack (`resource-pack` in `server.properties`), given during the configuration and never
  heard by v1, are heard here in order once the player has logged in, right after `native_player_logged_in`, with the
  hub in place; a player disconnected for declining a required pack never logs in. v1 keeps its signature and firing
  point. It only hears. Bundles need `"hooks": @API:resource_pack_status_v2@`.
- Helper **`native_kick(player, reason)`**: disconnects a player with `reason`, a whole `Text` in its colours and
  translations (Adventure's `Player.kick(Component)`; `/kick` takes one line of plain text), as `/kick` does: the
  client shows it, the log's "lost connection" line names it and `native_player_kicked` hears it. The kick happens at
  the tick's end, with the mod messages, so it works from any server hook, the kicked player's own included. True when
  it is queued; false on a client, in a world nobody else can join, and for a player who isn't on when the hub can tell
  (a player gone by the tick's end isn't kicked either). Bundles need `"hooks": @API:kick@`.

#### Server hooks for plugin ports: vanish and per-viewer visuals

What a vanish, a per-player hologram or a Tab list plugin needs, which plugin ports reached with raw packets or
injections before.

- **`native_living_change_target(dimension, entity, kind, target, target_mob, cancel)`**: a mob's target changed
  (NeoForge's `LivingChangeTargetEvent`, Bukkit's `EntityTargetEvent`): mob `entity` of type `kind` now targets the
  player `target` (a UUID) or the mob `target_mob`, or lets go (both None). Heard once per change, after the mob's
  tick that made it (its target goals or brain; a creative player it drops), or, when another mob's tick changed it
  (`HurtByTargetGoal`'s alert), once every mob has ticked. Set `cancel` and the mob keeps the target it had, and what
  its tick did to the new one (a melee hit, a sting, a sonic boom) is not dealt; it may pick the same target on a
  later tick, which is heard again, so a vanish mod cancels every time the target is a vanished player. A target that
  is neither a player nor a mob is not heard. No `self` (the mobs are out of the game while they tick), so a mod
  decides from its own state. Server-side. Bundles need `"hooks": @API:living_change_target@`.
- Helper **`native_hide_entity_from(viewer, entity, hidden)`** (`host.hide_entity_from`): hides the entity with
  network id `entity` (a mob, a display, another player's `NativeServerPlayer.entity_id`) from the player `viewer`
  alone, or shows it again, as Bukkit's `Player.hideEntity`/`showEntity` (`hidePlayer` for a player). From the next
  tick the viewer's client stops tracking it, so it disappears there, and once shown it is paired again as it is then;
  a hidden player also leaves the viewer's Tab list, and the viewer doesn't read them leave. Everyone else still sees
  it; `native_hide_player` stays the way to hide a player from permission levels. Kept while the viewer is on, for that
  id: forgotten when they leave, and a player who rejoins has another id (hide them again from
  `native_player_logged_in`). Outside a player's hook; false there, when the viewer isn't on and for their own entity.
  Bundles need `"hooks": @API:hide_entity_from@`.
- Helpers **`native_client_entity_spawn(viewer, kind, pos, yaw, nbt)`**, **`native_client_entity_set(viewer, id, nbt)`**,
  **`native_client_entity_move(viewer, id, pos, yaw)`**, **`native_client_entity_remove(viewer, ids)`** and
  **`native_reserve_entity_id()`** (`host.client_entity_spawn`...): an entity shown to one player's client alone, a
  hologram or a board only its owner sees (Bukkit has no API for it; plugins send the packets). Spawn sends the add and
  its synched data, encoded from `nbt` as `/summon` reads it: all of a text, item or block display's (`text`,
  `background`, `billboard`, `transformation`, `item`, `block_state`, `teleport_duration`...), and for any type
  `CustomName`, `CustomNameVisible`, `Invisible`, `Glowing`, `Silent` and `NoGravity`; other types' own data is not
  read, and a player is refused. It returns the entity's id, one no server entity takes. No level holds it: it is not
  simulated, saved, tracked or seen by anyone else, and the client forgets it when the player changes dimension or
  leaves. `set` gives it the data `nbt` gives its type, what `nbt` leaves out back at its default (pass the whole of what
  it shows); `move` teleports it (a display eases over its `teleport_duration`); `remove` takes away those of `ids` that
  are this viewer's client-side entities and leaves other ids alone. `native_reserve_entity_id()` is an id from the
  server's own counter for a mod's own packets. Outside a player's hook; None or false there, when the viewer isn't on,
  for an unknown type and for an id that isn't theirs. Bundles need `"hooks": @API:client_entities@`.
- Helper **`native_set_list_name(uuid, name)`** (`host.set_list_name`): styled text (colours, a rank prefix) for that
  player in everyone's Tab list in place of their nickname or name, as Bukkit's `Player.setPlayerListName`; None shows
  the nickname or name again. Only the Tab list (chat keeps `native_set_display_name`'s or the nickname). It is sent at
  once and with every entry the lists get of them later (they join, a vanish ends, `native_hide_entity_from` shows them
  again), and kept by UUID until a mod sets it again or the server stops, also if they leave and come back, so a Tab
  plugin needs no per-tick diff. Outside a player's hook; false there and when they aren't on. Bundles need
  `"hooks": @API:set_list_name@`.
- Helper **`native_set_display_name(uuid, name)`** (`host.set_display_name`): styled text for that player in chat in
  place of their nickname or name, as Bukkit's `Player.setDisplayName`: their chat messages, `/me`, `/msg` both ways
  and the lines that they joined or left; None shows the nickname or name again. Their team's prefix, suffix and colour
  still go around it; hovering it gives the real name, which `/tell` is suggested with and which stays in the log,
  commands and selectors. The Tab list keeps `native_set_list_name`'s or the nickname. Kept by UUID until a mod sets
  it again or the server stops. Outside a player's hook; false there and when they aren't on. Bundles need
  `"hooks": @API:set_display_name@`.
- Helper **`native_set_name_tag(uuid, tag)`** (`host.set_name_tag`): a plain name over that player's head for everyone
  else in place of theirs, a nickname above the head; None shows their name again. A client puts a player's profile
  name there, so the tag is the profile name of their list entries: 1 to 16 ASCII letters, digits or symbols, no
  spaces, no colours (a team's formatting applies only while the team lists the shown name). The other players'
  clients get their entry and entity again at once (a blink), and their Tab list shows the tag unless a list name or
  nickname covers it; their own client reads it from their next join. Chat, commands, selectors, the log and the
  server's events keep the real name. Kept by UUID until a mod sets it again or the server stops. Outside a player's
  hook; false there, when they aren't on and for a tag that isn't one. Bundles need `"hooks": @API:set_name_tag@`.

#### Server hooks for plugin ports: NPCs

Helpers for server mods that spawn and drive NPCs (an NPC library, guards that fight, a stage that places actors),
in `crates/horizon-client/src/game/native_npcs.rs`, on a server only (on a client they do nothing and the readers give
nothing). Each names its level by `dimension` (`minecraft:the_nether`) and reaches any level the server has loaded,
not only the one in place: that level is put in place for the call and the one that was is put back, as `execute in`
does, so they work from `native_server_tick` too. A level the server hasn't loaded is refused, and a level nobody is
in has only the chunks something holds loaded. While a level ticks its mobs they are out of it, so from a hook inside
that tick the other mobs of that level read as gone.

- **`native_spawn_mob(dimension, kind, pos, &NativeSpawn)`** and **`native_summon(dimension, kind, pos, nbt)`**
  ([Mobs](#mobs), [Entities](#entities)) spawn into any loaded level, as `execute in <dimension> run summon` does,
  where they were refused for any level but the one in place. Their other refusals stay. API @API:npc_any_level@.
- **`native_entity_by_uuid(uuid)`** finds the entity with that UUID in any loaded level, as Bukkit's
  `Server.getEntity(UUID)` does: `(dimension, id, kind)`, so a stage plugin that keeps its actors by UUID reaches them
  wherever they are, without `/tp`, `/data` or `/kill` by UUID on the console. A server's player is found too
  (`minecraft:player`, in its level), except while a guest's packet is handled, where the hub is taken out.
  **`native_merge_entity_nbt(dimension, id, nbt)`** is `native_entity_merge_nbt` ([Entity and level
  handles](#entity-and-level-handles)) for any loaded level, as `execute in <dimension> run data merge entity` does,
  with the same refusals (a player, a kind Horizon can't reload yet, data that changes nothing). API
  @API:npc_entity_by_uuid@.
- **`native_level_markers(dimension)`** lists the `minecraft:marker` entities of a loaded level as `(id, position,
  data)`: the `data` compound is what `/data get entity <marker> data` shows (empty for a marker saved without data).
  It reads only the level's list of kept entities (displays, interactions, markers), not every entity, so it is the
  cheap way to find markers used as anchors, beside filtering `native_level_entities`. Empty on a client and for a
  level not loaded. Bundles need `"hooks": @API:level_markers@`.
- **`native_set_entity_rotation(dimension, id, yaw, pitch, head_yaw)`** turns an entity without moving it, as
  `/rotate` does (`Entity.forceSetRotation`): its yaw and pitch in degrees (pitch clamped to -90..90), with its body and
  head; `Some(head_yaw)` then turns a mob's head alone (`setYHeadRot`), so an NPC looks over its shoulder. It takes
  mobs (mannequins too), armor stands, boats, minecarts, evoker fangs, and displays and the other entities Horizon
  keeps without simulating them; a player is turned by teleporting them. A mob whose AI runs turns back by its own
  look and body control on its next tick: for one that looks around, use `native_mob_look_at` each tick, or spawn it
  with `no_ai`. False for no such entity, another kind, or an angle that isn't finite. API @API:npc_entity_rotation@.
- **`native_mob_navigate(dimension, mob, to, reach, speed)`** walks a mob with the engine's pathfinding, as
  `PathNavigation.moveTo(x, y, z, reach, speed)` does: round walls and through doors as its own navigation allows (a
  walker's target is the ground below `to`), done within `reach` blocks, at `speed` times its movement speed. True
  when a path was found within the mob's follow range. **`native_mob_follow(dimension, mob, target, speed)`** walks it
  to where an entity of its level stands now (`moveTo(entity, speed)`; a player by their entity id): call it again as
  the target moves, as vanilla's follow and attack goals do. **`native_mob_stop(dimension, mob)`** stops the walk
  (`stop`), **`native_mob_navigating(dimension, mob)`** says whether one is in progress (`isInProgress`: false once
  it arrived, got stuck or was stopped), and **`native_mob_look_at(dimension, mob, at)`** turns its head towards a
  point at its own head speeds for its next tick (`LookControl.setLookAt`), its body following. These drive the mob
  as vanilla's goals do, so its own goals may take the navigation or the look over on a later tick: an NPC library
  asks again each tick, or gives its mobs goals of their own (`native_register_goals`). A mob with `no_ai` keeps a path
  but doesn't walk it, and doesn't turn its head (`native_set_entity_rotation` turns it). False for no such living mob
  or a value that isn't finite. API @API:npc_mob_navigation@.
- **`native_mob_attack(dimension, mob, target)`** has a mob strike an entity with its own attack, as
  `Mob.doHurtTarget` does: its attack damage, held weapon and enchantments, knockback, the species' extras (a cave
  spider's poison, a husk's hunger) and its attack sound, dealt as any mob's hit is, so the damage hooks hear it.
  `target` is a living mob or a server's player of the same level, by entity id; the player is hit as when a mob that
  sees them strikes in its tick, whichever player the mob sees. There is no reach or sight check and no swing, as in
  vanilla, where the attack goal checks and swings first. True when the blow was dealt (whether it hurt, a shield or
  creative mode aside, is the damage hooks' to tell); false for no such living mob or target, the mob itself, and a
  player while a guest's packet is handled, where the hub is taken out. API @API:npc_mob_attack@.

#### Server hooks for plugin ports: damage, hands, fishing and redstone

`native_living_incoming_damage_v3(victim, victim_uuid, kind, attacker, attacker_uuid, direct, damage_type, amount,
cancel)` is `native_living_incoming_damage` with who the hit names (NeoForge's `LivingIncomingDamageEvent`, Bukkit's
`EntityDamageEvent`). It fires everywhere v1 fires (players, mobs and the dragon; melee, projectiles, the environment),
right after v1 with the amount v1 left: change `amount` or set `cancel`, which does what a v1 cancel does; a hit v1
canceled is not asked. `victim_uuid` is a player victim's UUID (a player is known from their first turn on the server;
a mob's UUID is looked up by its id). `attacker` is the causing entity where v1 has 0 for it: a mob's melee names the
mob, a player's melee the player, and a source naming no entity falls back to the synchronous attack in progress, as
`native_living_knockback_v2` does; `attacker_uuid` is a player attacker's UUID and `direct` the direct entity (the
arrow). Each is none when Horizon does not know it, and an attacker that is the victim itself is none. No `self`, like
v1: the victim is borrowed out of the game during the hit, so the entity helpers can't be called here; a mod keeps
what it needs and acts in its next tick hook. Bukkit's damage modifier breakdown (armour, blocking, resistance,
absorption as separate values) is not offered: the reductions are applied after this hook. Bundles need "hooks":
@API:living_incoming_damage_v3@.

`native_interact_entity_v2(entity, kind, hand, stack, cancel)` is `native_interact_entity` with the hand that clicks
(`Hand::Main` or `Hand::Off`, NeoForge's `PlayerInteractEvent.EntityInteract.getHand`). It is asked right after the
unchanged v1 hook, once per hand vanilla tries: the main hand first, the off hand only when the main hand's turn did
nothing. Setting `cancel` claims the click, as v1's `claimed` does. A mod that claims the main hand's click hears one
event per click, as Bukkit's `PlayerInteractEntityEvent` for `EquipmentSlot.HAND`. A click v1 claimed is not asked
again. Bundles need "hooks": @API:interact_entity_v2@.

`native_right_click_block_v2(pos, face, hand, cancel)` is `native_right_click_block` with the hand that clicks
(NeoForge's `PlayerInteractEvent.RightClickBlock.getHand`), asked right after the unchanged v1 hook, once per hand
tried. A canceled click does what a v1 cancel does (neither the block nor the item acts, the hand doesn't swing, the
other hand gets no turn); a click v1 canceled is not asked again. A protection mod refuses an off-hand flint and steel
or bone meal by its hand. Bundles need "hooks": @API:right_click_block_v2@.

`native_player_fish_v2(player, name, state, hand, cancel)` is `native_player_fish` with the rod's hand: the cast and the
reel-ins (states 0 and 2 to 4) name the hand the rod is used with, as Bukkit's `PlayerFishEvent.getHand`; a bite
(state 1) names the hand holding the rod, the main hand first. It is asked right after v1 with the same `cancel`; one
v1 set is not asked again. A stasis or power rod in the off hand is seen, and read from that hand's stack. Bundles
need "hooks": @API:player_fish_v2@.

`native_fishing_hook_landed(player, hook, dimension, block, face)` hears a server player's fishing bobber (`hook`, its
entity id) touching a block, as Bukkit's `ProjectileHitEvent` for a `FishHook`: once each tick the flying bobber meets
a block on its way (`face` the side it meets), and once each tick it lies on one (on the ground as the tick begins or
ends: the block under its centre, `face` `[0, 1, 0]`). It is heard at the end of the bobber's tick with the bobber in
place, so a mod that powers the button or plate it rests on can also take it away with `native_remove_entity`. A
bobber in water, one hooked to an entity and a LAN host's own bobber are not heard. It only hears: the bobber moves as
it would. The player's state is swapped in, the other players are out of reach. Bundles need "hooks":
@API:fishing_hook_landed@.

`native_fishing_medium(dimension, hook, position, owner, medium)` lets a bobber fish outside water: lava, the void
below a skyblock island, a cloud. Each tick a bobber that is flying or floating, not in water and not hooked to an
entity asks the mods whether its spot (`position`, its feet) is a fishing medium; `owner` is where the player who cast
it stands. The first mod to answer `Some(NativeFishingMedium)` makes it one, for this tick:

- `surface`: the height it floats at. In the block of the surface or below, the bobber is in the medium as it would
  be in a water block: a flying bobber stops there, then bobs on the surface, waits for a fish, bites and is reeled in
  as in still water. Above it, it falls as usual.
- `loot_table`: what a catch from there rolls, with the fishing context vanilla's table gets (luck, the open-water
  flag, which is true in a medium). None rolls `minecraft:gameplay/fishing`. The bobber keeps the last medium's table
  until it lands in water.
- `lure_ticks`: the wait before a fish comes, drawn from `min..=max` and less the rod's Lure (vanilla draws 100 to 600
  ticks). None keeps vanilla's.

It is asked on the server, with the owner's state swapped in, and on every client that draws the bobber (`owner` None
for another player's whom the client doesn't see), so that both move it alike, as vanilla ticks the hook on both
sides: answer from the world and `owner` alone, the same on both. A mod that isn't on a client leaves it predicting a
fall that the server's updates pull back. `horizon-ctl status` shows the hook's `loot_table`. Water is never asked
about, and no answer keeps vanilla. Bundles need "hooks": @API:fishing_medium@.

`native_remove_entity(dimension, id)` also removes projectiles (arrows, tridents, snowballs, eggs, ender pearls,
fireballs, wind charges, llama spit, shulker bullets, firework rockets) as `/kill` does, and a server player's fishing
bobber, as `Entity.discard` (Bukkit's `Entity.remove`): an arrow cleanup, or a power rod's bobber taken once its
button is held. A bobber can be removed for the player in place (inside their hooks, `native_fishing_hook_landed`
included), and outside a guest's turn for any player of the level; it ends as a bobber that goes by itself, and the
rod takes no wear. False for another dimension or no such entity. Bundles need "hooks": @API:remove_projectiles@.

`native_block_redstone(dimension, pos, old, new)` (a `GameLevel` hook) is asked when a button, lever or pressure plate
is about to change its signal from `old` to `new`, 0 to 15, as Bukkit's `BlockRedstoneEvent`: a button pressed by a
hand or an arrow or let go by its release tick, a lever pulled, a plate's weight checked. Change `new` and the
component takes that signal (clamped to 0 to 15; an on-or-off component is on for anything above 0). One equal to
`old` keeps it as it is, without a sound: a held button's release is scheduled again and asked again, a plate held on
is checked again, a lever stays. A mod holds a power rod's button or plate on with no 1-tick gap. Not heard yet:
redstone wire, torches, repeaters, comparators, observers, detector rails, tripwire hooks, daylight detectors and the
other sources. Server-side. Bundles need "hooks": @API:block_redstone@.

`native_hurt_entity(dimension, target, amount, damage_type, attacker)` hurts a mob or a player of the level `dimension`
by `amount` of `damage_type` (`minecraft:mob_attack`), from `attacker` (a mob's or a player's entity id, the causing
and direct entity, as vanilla's `damageSources().mobAttack(attacker)`; none for no entity). It goes the way
`/damage <target> <amount> <damage_type> by <attacker>` goes: armour, effects, invulnerability time, knockback away from
the attacker, death and its message, and the damage hooks hear it. It returns whether the target was hurt: false for
an invulnerable, dead or creative target, another dimension, an entity that is not a mob or a player, an attacker that
isn't there, or an amount that is not a finite number. An NPC's own hit needs no `damage` command and no UUID lookup.
Bundles need "hooks": @API:hurt_entity@.


#### Server hooks for plugin ports: Menus, items and maps

- Helper `native_item_from_snbt(snbt) -> Option<ItemStack>`: the stack an SNBT item compound stands for, as `/give`
  and the data packs write one (`{id:"minecraft:diamond_sword",count:1,components:{"minecraft:damage":5}}`), read as
  vanilla's `TagParser` and `ItemStack.CODEC` do (Bukkit's `ItemFactory.createItemStack`). None when the text is not
  one compound (whitespace around it aside), for air, and for an item the world does not have. `ItemStack::from_nbt`
  reads a stack from a compound without asking the registry, and `mod_api::parse_snbt` is the SNBT reader itself (the
  tag and the bytes it read), so a mod that keeps items as text needs neither the game's internal `item_data` module
  nor a `summon` command to make them. Bundles need `"hooks": @API:item_from_snbt@`.
- Helpers `native_new_map(dimension, colors) -> Option<i32>` and `native_paint_map(map_id, colors) -> bool`: maps a
  server mod draws that the world keeps (Bukkit's `Server.createMap` and a `MapCanvas` drawn for everyone). `colors`
  is the 128 x 128 picture, row by row, each byte a `MapColor` id times 4 plus its shade, as `native_send_map` takes.
  `native_new_map` makes a locked map (as a cartography table locks one: no exploring draws over it, no player
  markers) of the level `dimension`, scale 0, under the world's next map id, so it never meets an id the game hands
  out; give a `minecraft:filled_map` with that `minecraft:map_id`. `native_paint_map` replaces the whole picture of a
  map the world made or saved (false otherwise); an unlocked map is still drawn over where its holders explore. Both
  are saved with the world's maps (`data/minecraft/maps/<id>.dat`) and sent like vanilla map data to whoever carries
  the map or sees it framed, players who join later included, so the mod never resends them. None and false on a
  client of a server. Bundles need `"hooks": @API:new_map@`.
- Helper `native_open_menu_with_input(uuid, rows, title, items, input) -> Option<i32>`: a server menu (see "Server
  menus") whose slots in the range `input` hold items as a chest's do (Bukkit's custom inventory with storage slots):
  the player puts stacks in and takes them out by vanilla's click rules (pickup, split, shift-click from their
  inventory, number keys, drags, double-click), while the menu's other slots stay the mod's to show and their clicks
  still reach `native_menu_clicked`, moving nothing. `items` fills both; `native_set_menu_items` changes only the shown
  slots. None for a player not here, or `input` empty or past the rows' slots. Helpers `native_menu_contents(uuid)`
  (every slot of the player's open server menu, input slots included; None while none is open) and
  `native_menu_cursor(uuid)` (the stack on the cursor; inside `native_menu_clicked`, the one the player held as they
  made that click, so a kit editor can copy it into a slot).
- **`native_menu_input_closed(uuid, window, slots)`**: a menu with input slots closed, for any reason (the player, the
  server, a mod, another menu, death, leaving), heard before `native_menu_closed` for the same window. `slots` are
  its input slots as the player left them: take what the mod keeps, and what stays goes back to the player as
  vanilla's `AbstractContainerMenu.clearContainer` gives a crafting grid back (into the inventory, the rest dropped;
  all dropped at them when they left), so no item is lost or made. It runs where the menu closed, so the hub may be
  out of the game's hands and a player's state swapped in: answer through `slots`. Bundles need
  `"hooks": @API:menu_input@`.
- Helper `native_open_inventory_menu(viewer, rows, title, (target, kind), layout) -> Option<i32>`: another player's
  inventory (`Player`, 41 slots in `native_inventory`'s numbering) or ender chest (`Ender`, 27) in a server menu, for
  an operator's `/invsee` (Bukkit's `openInventory(target.getInventory())`). Menu slot `k` shows the target's slot
  `layout[k]`; a `None` (or a slot past the layout) stays the mod's to show with `native_set_menu_items`, its clicks
  heard by `native_menu_clicked`. Clicks on the shown slots move the target's own items by vanilla's click rules, as in
  a chest (pickup, split, shift-click, number keys, drags, double-click, throw), so a split stack really splits; an
  armour or off-hand slot takes any stack, as that Bukkit view does. A target on the server is read before each click
  and each tick and changed in the same step, so their own screen follows with the tick's sync and nothing is copied
  or lost; a target away is their file (`players/data/<uuid>.dat`), read again before each click and written after one
  that changes it (a click whose write fails is taken back whole). The menu closes once the target leaves, dies or,
  away, comes back. None for a viewer not here, the target being the viewer, logging in, dead or never seen here, a
  layout longer than the rows' slots, a slot past the inventory or shown twice, and inside a player's turn. A mixin
  helper, not on `Host` (it takes `NativeInventoryKind`). Bundles need `"hooks": @API:inventory_menu@`.

#### Server hooks for plugin ports: world generation

For a mod that generates terrain of its own (a custom world generator, a far lands border, a void world with climate
biomes) in some chunks or all of them, as Bukkit's `ChunkGenerator` does.

- **`native_set_biomes(dimension, entries)`** (`native_biome_colors/native_biome_edit.rs`): sets biome cells in the loaded chunks of the
  level being ticked, as `/fillbiome` does. `entries` is a list of `([qx, qy, qz], biome id)`: a cell is 4x4x4 blocks
  and its position is the block coordinate shifted right by 2 (`qy` counts from world height 0, so it may be
  negative). The columns changed are saved and their biomes sent to the players who have them
  (`ChunkMap.resendBiomesForChunks`). Returns `Ok(cells changed)`: a cell that already holds the biome, a cell in a
  chunk not loaded and a cell outside the column's height are left out. Returns `Err` with nothing changed for a
  `dimension` that is not the level being ticked, a client of a server, or a biome id the level doesn't have
  (`minecraft:plains`, or a data pack's own). API @API:set_biomes@.
- **`native_world_seed()`** (`native_world_seed.rs`): the world's seed as `/seed` prints it (`WorldOptions.seed`,
  Bukkit's `World.getSeed`), the one every level generates from, for a preview that draws the terrain a chunk will get.
  None off a server: a client only knows a hash of its server's seed. API @API:world_seed@.
- **`native_chunk_generate(dimension, chunk, seed, flat, chunk_out, generate)`**
  (`horizon_world::worldgen::native_generate`): chunk `[x, z]` of the level `dimension` is about to get its terrain,
  after its biomes and structure starts and before the noise (Bukkit's `ChunkGenerator.generateNoise`, vanilla's
  `NOISE` status); `flat` says the level is superflat. To build it, set its blocks with `chunk_out.set_block(x, y, z,
  state)` (all air until then; `chunk_out.registry()` names the states) and set `generate` to `NativeGenerate::Replaced
  { surface, carvers, structures, decorations }`: vanilla then adds only what the flags ask for, the dimension's surface
  rule, its carvers (on a superflat level too), the structures, the biomes' features (Bukkit's `shouldGenerateSurface`,
  `shouldGenerateCaves`, `shouldGenerateStructures`, `shouldGenerateDecorations`). Left at `Vanilla` with no block set,
  the chunk is vanilla's, byte for byte; `chunk_out.set_biomes(pick)` (what decoration, the saved chunk and the players
  see; the surface rule and structure placement keep reading the biome source) and `chunk_out.clear_structures()` work
  with either answer. Mods are asked in turn. It runs on the generation workers, for several chunks at once on several
  threads and never on the server thread, so no `Game` helper works there; a chunk can be asked again (a proto chunk
  saved unfinished, the neighbours of a finished one, the far terrain's decoration), so answer the same each time. Also
  where single player and the far terrain generate. API @API:chunk_generate@.
- **`native_lod_column(dimension, column, seed, flat, column_out)`** (`horizon_world::worldgen::native_generate`): the
  far terrain samples block column `[x, z]` where no chunk is generated yet, from vanilla's noise and surface rule
  (`GeneratorWorker::lod_grid`, `surface_sample`). A mod whose `native_chunk_generate` builds that chunk calls
  `column_out.replace(spans, biome)` with the column's blocks, runs `(lowest y, highest y + 1, state)` in any order, and
  the biome its top shows (None keeps vanilla's): the far terrain then draws them, its top, the water right over it and
  the spans a cliff shows, until the chunk itself is generated. Left alone, the column is vanilla's. It is asked for each
  far column, many per tile, on the far terrain's workers, several at once: answer from the terrain function alone,
  without building the chunk. Like vanilla's, a far column shows no carver's caves. API @API:lod_column@.

#### Server hooks for plugin ports: diagnostics

What a diagnostics or profiler port (Bukkit's `getTPS`, spark, PacketEvents' counters) reads. Each costs nothing
while no mod asks for it.

- **`native_tick_times()`** and **`native_tps()`**: the last 100 server tick durations in nanoseconds, laid out as
  vanilla's `MinecraftServer.getTickTimesNanos` (the tick numbered `n` at index `n % 100`, 0 where none was recorded
  yet; sort a copy for percentiles, `native_tick_stats` has the average), and the ticks a second over the last one,
  five and fifteen minutes (Bukkit's `Server.getTPS`), measured between the ends of the recorded ticks and averaged
  once a second as a load average is. They start at 20; an idle server reads 20, a sprint more and a server that
  falls behind less. Bundles need `"hooks": @API:tick_times@`.
- **`native_level_stats(dimension)`**, **`native_level_entities(dimension)`** and
  **`native_level_block_entities(dimension)`**: what an open level of a server holds (`minecraft:the_nether`; None or
  empty on a client and for a level that isn't open), as Bukkit's `World.getLoadedChunks`, `getForceLoadedChunks`,
  `getEntities` and `Chunk.getTileEntities` give it. `NativeLevelStats` counts the loaded chunks, the forced ones
  (`/forceload` and `native_force_chunk`), the block entities and those that tick, and the entities (players
  included) and block entities by type, the most first. The entity list gives each entity's id (what events and
  `native_entity` take), type and position; the block entity list each one's position and type: the vanilla block
  entity type of its block (`minecraft:barrel`, `minecraft:mob_spawner`), else the block's name. A level not in place
  is swapped in for the count and back, as `/execute in` does. The players are the hub's, so none are counted inside
  a player's own hook, where the hub is taken out. Each walks every entity or block entity: call them once a second
  or less, not every tick. Bundles need `"hooks": @API:level_stats@`.
- **`native_server_packets(player, packets, outbound)`** and **`native_count_packets(on)`**: the play packets each
  player's connection carries, by type, as PacketEvents' send and receive events count them. Nothing is kept until a
  mod calls `native_count_packets(true)` (one switch for the server, shared by every mod; off drops what is kept, and
  a packet costs one relaxed load); while on, once a server tick after `ServerTickEnd` each player's packets since the
  last tick arrive, those sent (`outbound`) and those received, each as (packet id, body bytes) in order. The id is
  `horizon_protocol::ids::play`'s as the server sent or read it (before an older client's renumbering; keep-alives
  and the disconnect included), the bytes are id and payload before compression and encryption. A tick stalled for
  long keeps at most 65536 packets each way. Bundles need `"hooks": @API:server_packets@`.
- **`native_server_tick_timed(wall_ms, cpu_ms, spans)`**, **`native_spans(on)`**, **`native_mob_cost_sampling(on)`**
  and **`native_take_mob_costs()`**: a profiler by tick step instead of by stack, as spark's. After each pass of the
  server tick (paused ones too) while its spans are recorded, the hook gets the pass's wall time, the server thread's
  CPU time in it (None where the system doesn't tell) and its spans (`TickSpan`: name, wall and CPU milliseconds,
  depth; summed by name in the order first seen, `tick_mobs` inside `tick`). A dedicated server records them anyway
  (its slow-tick log), so there the hook fires every tick; elsewhere it fires while `native_spans(true)`,
  `HORIZON_SPIKES` or a performance report wants them: one switch shared by all, on while anyone wants it, which costs
  two clock reads a step on every thread. `native_mob_cost_sampling(true)` times every mob's own tick (riders inside)
  by level and type until it is turned off, apart from `horizon-report`'s sampling, so both can run;
  `native_take_mob_costs()` takes the sums (`NativeMobCost`), costliest first. For a lighter profile, sample one tick
  in N: turn it on in `ServerTickStart` and off after. Call both from server hooks: the sampling belongs to the
  server thread. Bundles need `"hooks": @API:server_tick_timed@`.

- **`native_perform_command(player, command, ran)`**: a player typed a registered command (without its slash) and
  their level allows it, whether the world allows commands or not. The mod parses the line and sets `ran`. The
  player's state is swapped in for the call, as for their other actions, so `native_player` reads them; a
  registered command no mod takes answers as vanilla's unknown command.
- **`native_player_swap_hands(player, name, cancel)`**: a player pressed the swap-hands key (not a spectator), heard
  before the main and off hand items swap. Set `cancel` and they stay where they are; the player's client, which swapped
  them at once, gets its slots back. The player's state is swapped in, so `native_acting_player`, `native_tell` and the item
  helpers reach them. Not heard: the same swap from an open inventory (hovering a slot and pressing the key) and the host of a LAN world.
- Helpers: `native_server_players()` (uuid, name, level, dimension, position and game mode of each player on);
  `native_tell(uuid, text)`, a system line to one player, also the acting player inside their hook;
  `native_broadcast(text)`, to every player and the server's
  log; `native_server_event(name, uuid, message)`, an `announce` event on a dedicated server's event stream, which the Discord
  bridge relays when `[discord] events` lists `announce`; `native_feedback(text)`, the command's answer to the player whose command it is; `native_world_dir()`, the
  open world's folder, where a server mod keeps its data in a folder of its own so the world's backups carry it.
- **`native_item_damaged(player, name, item, amount, cancel)`**: an item of a player is about to lose durability, on a
  server (`ItemStack.hurtAndBreak`: tools, armour, shield, rod, elytra), after Unbreaking has had its say. Change
  `amount` (0 leaves the item whole) or set `cancel` for no wear. Not heard for a creative player or when Unbreaking
  saved the item, nor for a mob's armour, a dispenser's tool or the host of a LAN world. The player's state is swapped
  in, so `native_acting_player`, `native_tell` and the item helpers reach them.
- Items (API 9), of the acting player (inside `native_perform_command`, whose player is swapped in): `native_held()`
  (the main hand's item, count, and whether it is plain), `native_count_items(item)`, `native_carried_items()`,
  `native_take_items(item, count)` (the main hand first, then as `Inventory.add` orders slots),
  `native_room_for(item)` and `native_give_items(item, count)` (only where it fits: nothing is dropped). They look at
  the 36 inventory slots and the off hand, never the armour, and only at **plain** stacks: no damage and no
  component (name, enchantment...), so a shop never takes a worn or enchanted item for a new one. Each returns how
  many it counted, took or gave; the player's screen follows as for any change to their inventory.
- `native_give_stack(uuid, stack)`, a whole stack (damage, name, enchantments... kept) to any player from any server
  hook, as `Inventory.add` places it (onto equal stacks, then empty inventory slots); what did not fit comes back, all
  of it for a player not here, dead, or swapped out by another player's hook. Nothing is dropped: an auto-pickup
  leaves the rest on the ground, a machine keeps it.
- Experience: `native_experience_orbs(dimension, min, max)`, the orbs in a box (id, position, `value`, and `count`
  for merged orbs), read before a mod cancels an orb in `native_entity_join_level`; and `native_give_experience(uuid,
  points)`, points given to any player as touching an orb gives them: Mending repairs their damaged items first, the
  rest goes to their experience, with the level-up sound; and `native_give_levels(uuid, levels)`, levels added or,
  negative, taken off (`Player.giveExperienceLevels`, as the anvil and the enchanting table charge; below 0 the level
  and points are 0; no level-up sound). Both return false for a player not here, dead, or swapped out by another
  player's hook, and where the server's player list is busy (some world ticks hold it): give first, and cancel the orb
  only when the give returned true. A merged orb is worth `value` × `count`.
- **`native_player_totem(player, name)`**: a totem of undying saved a player whose hurt would have killed them, on a server
  (`checkTotemDeathProtection`), heard just before it is used up. Listen only: the health state machine decides before a
  mod is asked, so a mod cannot veto it. The player's state is swapped in, so `native_acting_player`, `native_tell` and the
  item helpers reach them. Not heard: the host of a LAN world.
- API 21: `native_acting_player()`, the player whose state is swapped in (inside `native_perform_command`,
  `native_right_click_block`, `native_use_item_on`, `native_destroy_block`...; None elsewhere);
  `native_run_command(line)`, a command run as the server's console (level 4, no output), only where no player is
  swapped in (`native_tick`, a menu hook): a teleport to another level (`execute in minecraft:the_nether run tp Alex 0
  70 0`) lands at the next tick's start. `native_server_players()` and the acting player carry `health` (0 once dead).
  `native_action_bar(uuid, text)`, a line over a player's hotbar, which also reaches the acting player inside their
  hook.
- **`native_food_level_changed(player, name, from, to, cancel)`**: the food level of a player is about to change, on a
  server (eating, hunger and saturation effects, exhaustion, starving), before the new level reaches them. Change `to`
  (0 to 20) or set `cancel` to leave it at `from`, saturation included. It is heard once for what one turn of the
  player changed, not for the full bar a respawn gives, the food a player joins with, or the host of a LAN world. The
  player's state is swapped in, so `native_acting_player`, `native_tell` and the item helpers reach them.
- **`native_right_click_block(pos, face, canceled)`** (API 21): a player's right click on a block, before the block's
  own use and the item's (`ServerPlayerGameMode.useItemOn`), once per hand. A mod that sets `canceled` stops both,
  without a swing; the player's client is sent the block back. With `native_destroy_block` it keeps players out of
  claimed land.
- **`native_block_spread(dimension, kind, from, to, canceled)`** and **`native_piston_move(dimension, piston,
  direction, push, destroy, canceled)`** (API 22, on `GameLevel`, like the block observers): a fluid flowing or fire
  spreading (`kind` `fluid` or `fire`) from `from` into `to`, and a piston moving blocks once its structure resolved.
  Canceled, the block at `to` stays as it is, and the piston moves nothing. Fluids ask on every flow: answer from
  memory, fast.
- **`native_block_ignite(dimension, player, pos, cause, cancel)`** (on `GameLevel`): fire is about to be placed.
  It covers fire spread, lava's random fire placement, lightning strikes, and dispenser flint-and-steel. Set `cancel`
  to leave the position unchanged. Direct player flint-and-steel/fire-charge placement is not covered.
- **`native_block_burn(dimension, pos, cancel)`** (on `GameLevel`): fire's burnout check is about to remove the block at
  `pos`. Set `cancel` to keep it.
- **`native_dragon_egg_form(dimension, pos, cancel)`** (on `GameLevel`): the first Ender Dragon fight is about to place
  its egg at `pos`. Set `cancel` to leave the block unchanged.
- **`native_fluid_level_change(dimension, pos, fluid, from, to, cancel)`** (on `GameLevel`): flowing `water`, `lava`
  or a mod fluid (by its id, `ruby:honey`) at `pos` is about to change level. Levels run from 0 through 8. Set `cancel` to keep the old level.
- **`native_leaves_decay(dimension, pos, cancel)`** (on `GameLevel`): a non-persistent leaf block at distance 7 is
  about to decay. Set `cancel` to keep it.
- **`native_inventory_block_start(player, name, pos, cancel)`**: a guest is about to open an inventory supplied by a
  block at `pos`. Set `cancel` to leave it closed. The guest's state is not swapped in.
- **`native_moisture_change(dimension, pos, from, to, cancel)`** (on `GameLevel`): farmland at `pos` is about to
  change moisture. Set `cancel` to keep its old moisture.
- **`native_sculk_bloom(dimension, pos)`**: after a sculk catalyst at `pos` takes experience, its block has entered
  the blooming state.
- **`native_sponge_absorb(dimension, pos)`** (on `GameLevel`): a sponge at `pos` has absorbed water and become wet.
- **`native_target_hit(dimension, pos, strength)`**: a projectile hit the target block at `pos`, which now has the
  reported redstone output `strength`.
- **`native_vault_change_state(dimension, pos, state)`**: the vault block at `pos` changed its state to `state`
  (`inactive`, `active`, `unlocking`, or `ejecting`).
- **`native_block_failed_dispense(dimension, pos, item)`**: a dispenser at `pos` failed to dispense `item`.
- **`native_beacon_state_change(dimension, pos, active)`**: the beacon at `pos` gained or lost its complete beam
  pyramid; `active` says whether it is active now.
- **`native_anvil_damaged(dimension, pos, from, to)`**: the anvil at `pos` took damage, changing from `from` to
  `to`; `to` is air when it broke.
- **`native_block_break_progress_update(player, name, pos, progress)`**: a guest's crack animation at `pos` reached
  progress stage 0 through 9.
- **`native_block_lock_check(player, name, pos, locked)`**: a guest checks the lock on the container at `pos`;
  `locked` starts with vanilla's result and may be changed. The guest's state is not swapped in.
- **`native_block_fade(dimension, pos, from, to, cancel)`** (on `GameLevel`): ice or snow melts, or coral dies. `from`
  and `to` are block state IDs. Set `cancel` to keep the old block.
- **`native_block_form(dimension, pos, from, to, cancel)`** (on `GameLevel`): precipitation is about to place ice or
  snow, or add a snow layer at `pos`. `from` and `to` are block state IDs. Set `cancel` to keep the old state.
- **`native_note_play(dimension, player, pos, instrument, note, cancel)`** (on `GameLevel`): a note block is about to
  sound when punched, used, or powered. Set `cancel` to silence it; when used to tune it, the note remains unchanged.
- **`native_bell_ring(dimension, player, pos, cancel)`** (on `GameLevel`): a bell is about to ring. Set `cancel` to
  stop the ring and its raider reveal.
- **`native_player_physical_trigger(dimension, player, pos, block, cancel)`** (on `GameLevel`): a player stands in a
  pressure plate (wooden, stone, weighted) or a tripwire `block` at `pos` and would set it off (Bukkit's
  `PlayerInteractEvent` with `Action.PHYSICAL`). Asked each time the block counts what is inside it, before the
  effect, for every player in it. Set `cancel` and that player is not counted: the plate stays unpowered and the wire
  untripped for them, while another player or mob still sets it off. Farmland and turtle eggs a player lands on are
  `native_trample` and `native_entity_change_block_v2`; redstone ore, big dripleaf and sculk are not heard yet.
- **`native_cauldron_level_changed(dimension, player, pos, from_level, to_level, reason, cancel)`** (on `GameLevel`):
  precipitation, dripstone, or a burning player being extinguished is about to change a cauldron's level. Set `cancel`
  to leave it as it was.
- **`native_random_tick(ticks, dimension, pos, state, handled)`** (API 33, on `GameLevel`): a block `tickChunk`
  random-ticks, before its own `randomTick`. Change the block with the level's `native_set_block_property(ticks, pos,
  key, value, flags)`; set `handled` to skip the block's own random tick.
- **`native_tick_block(ticks, dimension, pos, state, handled)`** (API 34, on `GameLevel`): a due scheduled tick of a block
  that is still there (`ServerLevel.tickBlock`), before its own `tick`; the same contract as `native_random_tick`.
- **`native_explosion_detonate(dimension, center, blocks)`** (API 22): the positions an explosion is about to break,
  before any is hit; take out the ones to spare. Entities are hurt as before.
- **`native_explosion_knockback(dimension, center, blocks, entity, knockback_velocity)`**: a server explosion's
  calculated velocity, before it is added to the entity. Replace `knockback_velocity` to change the push;
  `entity` is None for a player, whom `native_acting_player` names. The player packet uses the same replacement. Wind bursts are heard too.
  Bundles need `"hooks": 374`.
- **`native_fill_bucket(stack, pos, canceled)`** and **`native_attack_entity(entity, kind, pos, canceled)`** (API 22):
  a player's bucket at a block, and their blow on an entity, with the acting player swapped in. Canceled, the bucket
  does nothing and the blow does no harm. A fluid or mob bucket also asks about the block beside the clicked face
  whenever that block is not its target, since a block that refuses the fluid passes it there; the fluid may still
  stay in the target.

#### Server hooks for plugin ports: player bots

Helpers for server mods whose bots are server players driven by the mod (a fake client that sends its own moves, as a
PvP bot plugin's), in `crates/horizon-client/src/game/native_bots.rs`, on a server only. Nothing runs unless a mod
calls them, and neither changes the player or the level. API @API:player_bots@.

- **`native_simulate_player_move(uuid, NativePlayerInput) -> Option<NativeSimulatedMove>`** plays one tick of the
  server player's own movement from a bot's keys, with the client movement code the server's movement prediction
  replays a client tick with (`LocalPlayer.aiStep` and `Player.travel`): `forward` and `strafe` (-1..1; a value in
  between is a stick's partial push), `jump`, `sneak` (slower, held at edges), `sprint` (only with a forward input),
  `yaw` and `pitch` in degrees. Gravity, friction, block speed factors, stepping up, swimming and climbing follow. It
  starts where the player is now, with `velocity` when given, else the server's copy: the server doesn't keep a bot's
  momentum, so pass back the `velocity` of the last answer each tick. The answer is the `position`, `velocity`,
  `on_ground`, `horizontal_collision` and `sprinting` after the tick; the player stays where they are until the mod
  moves them (its bot's move packet, `native_teleport_player`). Entities don't block it (a boat, a shulker). None off
  a server, for a player in another level than the one in place, a guest out of reach inside another player's hook
  (the hub is taken out), dead, a spectator or riding, and for an input that is not a number.
- **`native_collision_boxes(dimension, aabb) -> Vec<[f64; 6]>`** lists the block collision boxes that share volume
  with `aabb` (`[min_x, min_y, min_z, max_x, max_y, max_z]`) in the level `dimension`, in world space, as
  `Level.getBlockCollisions(null, aabb)` gives them under the empty collision context (a stair gives two boxes, a
  fence its post): for a bot's own collision checks and pathfinding. Entities and the world border aren't in it.
  Empty off a server, for another level than the one in place, a box whose corners are not numbers or out of order,
  and a box over about a million blocks.

### Mobs

API 37, `crates/horizon-client/src/game/native_mobs.rs`: helpers for server mods, for the level in place, on a server only (on a
client they do nothing, and the readers give nothing). Each takes the `dimension` (`minecraft:overworld`) of the level it means, as
`native_set_block` does, and is refused when another level is in place. The per-level tick hooks (API 23) run with a defined level in
place; `native_server_tick` does not define one, so name the dimension and expect a refusal there.

- `native_spawn_mob(dimension, kind, pos, &NativeSpawn)` spawns a mob and returns its id, in any loaded level since API
  @API:npc_any_level@ ([NPCs](#server-hooks-for-plugin-ports-npcs)). `NativeSpawn` holds an optional name and health, and the
  `no_ai`, `invisible`, `silent`, `invulnerable` and `no_gravity` flags; its default is a plain mob. The mob is finalized as a natural spawn
  is, and kept when its chunk unloads. Nothing is spawned outside `/summon`'s bounds, in a chunk that isn't loaded, or a monster on peaceful.
- `native_mob(dimension, id)` and `native_mobs(dimension)` read the living mobs: id, dimension, kind, position, yaw, health and maximum health.
- `native_teleport_mob(dimension, id, pos)` (as `/tp`: it dismounts and drops its path), `native_push_mob(dimension, id, velocity)` and
  `native_set_mob_health(dimension, id, health)` change one, and answer false for no such mob or a value that isn't finite. Health must be
  above 0.
- `native_kill_mob(dimension, id)` kills as `/kill` does, with its loot; `native_remove_mob(dimension, id)` takes the mob away with no death
  and no loot (it is not saved either).
- While the game ticks its mobs they are out of the level: from a hook inside that tick, a spawned mob joins after the tick, and the others
  read as gone.

### Mob goals

**`horizon_client_lib::mobs::ai::native_register_goals(mob)`** (vanilla's `Mob.registerGoals`; a trait mod's
`Hooks::register_goals(event)`, with `event.mob`) is asked for every new mob, after its species added its goals and before its
saved data is read. `mob.ai.goals` is vanilla's `goalSelector`, `mob.ai.targets` its `targetSelector`. A mod's living type
([mods.md](mods.md)) comes with its template's goals, a zombie's attacks, strolls and targets: clear them to replace them, as a
class whose `registerGoals` skips `super`'s, or add to them. A villager acts through its brain, which goals don't replace:
a type from the villager template still works, sleeps and trades as a villager, next to the goals a mod adds. For a
neutral mob whose goals are all the mod's, copy `minecraft:wandering_trader`. A goal is a
Rust type implementing
`horizon_client::mobs::goals::Goal` (a bundle compiles into the game, so nothing is loaded at run time): its `flags` (`MOVE`,
`LOOK`, `JUMP`, `TARGET`), `can_use`, `can_continue`, `start`, `tick` and `stop` run by priority and flag as vanilla's
`GoalSelector` runs them. The vanilla goals in that module (`Float`, `Stroll`, `LookAtPlayer`, `MeleeAttack`, `NearestTarget`
and the rest) are there to reuse.

```rust
use horizon_client::mobs::{Mob, goals::{self, Cx, Goal}};
use horizon_client::mod_api::{Hooks, RegisterGoalsEvent};

/// Walks the mob back to its post when it is more than 4 blocks away.
#[derive(Clone, Debug)]
struct ToPost { post: [f64; 3] }
impl Goal for ToPost {
    fn flags(&self) -> u8 { goals::MOVE }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        let [x, y, z] = self.post;
        let (dx, dy, dz) = (m.position.x - x, m.position.y - y, m.position.z - z);
        dx * dx + dy * dy + dz * dz > 16.0
    }
    fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool { !m.ai.nav.is_done() }
    fn start(&mut self, m: &mut Mob, cx: &mut Cx) { m.nav_move_to(cx.w, self.post, 1.0); }
    fn box_clone(&self) -> Box<dyn Goal> { Box::new(self.clone()) }
}

struct Guards;
impl Hooks for Guards {
    fn register_goals(&self, event: &mut RegisterGoalsEvent<'_>) {
        if event.mob.kind.id == "example:guard" {
            let ai = &mut event.mob.ai;
            ai.goals.clear();
            ai.targets.clear();
            ai.goals.add(0, goals::Float);
            ai.goals.add(1, ToPost { post: [0.5, 64.0, 0.5] });
        }
    }
}
```

From a goal, `mob.nav_move_to(cx.w, [x, y, z], speed)` walks the mob with the engine's pathfinding (`create_path_to` and
`nav_move_to_path` to check a path first), and `mob.ai.mov.set_wanted(x, y, z, speed)` steps it toward a point, for a mod with
pathfinding of its own. `goals::Float` keeps the navigation floating, as FloatGoal's constructor does. Each mob gets its own
goals, so a goal's fields are that mob's state while it lives; state that is saved goes in an attachment on
`Holder::Entity(mob.id)` ([Server attachments](#server-attachments)), from a hook with a host such as `native_entity_tick_pre`.
The hook is asked on a client too, for its copies of a server's mobs; goals tick only where the mob's AI runs.

### Loot tables

Changing loot a mod didn't make, as NeoForge's `LootTableLoadEvent` and global loot modifiers and Fabric's
`LootTableEvents.MODIFY` and `MODIFY_DROPS` do. A mod's own loot function and condition kinds are #5451.

- **`horizon_world::loot::native_loot_table_load(id, table)`**: each table's JSON as it loads, after the data packs'. Push a pool
  onto `table["pools"]` or an entry onto a pool's `entries`; chests, block and mob drops, fishing and the rest then roll the
  changed table. Keep a replaced table's `random_sequence`: a mob's table without one drops nothing. It runs wherever tables load,
  a client too, so make the same change each time; a `/reload` may keep the tables already loaded. It is a free function in
  `crates/horizon-world/src/loot.rs`, so a helper injected there compiles inside `horizon-world`, without `crate::game`.
- **`native_loot_drops(context, drops)`**: the items a broken block or a dead mob is about to drop, after its tables rolled (a
  mob's worn, held and carried items and inventory too, not an enderman's carried block or a copper golem's preserved equipment).
  Add, change or take out stacks; empty ones don't drop. `NativeLootDrops` says what dropped them: its own loot table (empty when
  none rolled), the dimension, the origin, the block's position and state or the mob's id and type, the player who broke or killed
  it (a mob names one only while it remembers the player's hit, 100 ticks), the tool and an explosion's radius. It hears blocks
  that a player, an explosion or a mob breaks, not other breaks (leaves decaying, a torch losing its wall, a piston,
  `/setblock ... destroy`), nor a creative break or a tool that can't harvest the block. A block's stacks get its block entity's data (a
  shulker box's items) after the hook, and a container's contents spill on their own, not through it. A player's use drops
  through it too, with `used` set and that player named: shears on a sheep, a mooshroom, a snow golem or a bogged (its shearing
  table), a brush on an armadillo (`minecraft:brush/armadillo`) or a suspicious block (its item, no table), shears carving a
  pumpkin, picking sweet berries or cave vines (their harvest table), and a player's hit taking an item frame or its item (the
  frame's entity, no table). A dispenser's shears and an explosion's frames aren't told. Leads: untying one or cutting it with
  shears is a use (the leashed mob, no table), and a mob a player killed while leashed drops its lead on its next tick as a call
  of its own, naming the killer (`used` false, no table). A lead that snaps or loses its holder names no player and isn't told.
  A dying player's inventory is a call of its own: after vanishing-curse items are gone and before the rest is thrown from
  the player's eyes, with `entity` the player's id and `minecraft:player`, `player` them, no table and `used` false (take every
  stack out of `drops` to keep the inventory for a grave or a soulbound rule). It is not heard with `keep_inventory` on.

### A block's items

Helpers for a server mod, by the rules hoppers follow: a furnace's sides, a double chest's two halves, a mod's `ModContainer` with its `faces`, `inputs` and `outputs`. `face` is the side of the block being reached, so a pipe above a chest uses `Direction::Up`. Those that read or take name the level in place by its `dimension`, and another one is refused. A client's copy of the world is refused.

- **`native_container_slots(dimension, pos, face)`:** the slots that side reaches, with each one's number, stack and `can_take`. None where no container is: another dimension, a chunk that isn't loaded, a block without slots, a composter, or the mod block entity ticking now, which has its items in hand.
- **`native_extract_item(dimension, pos, face, slot, count)`:** takes up to `count` from a slot through that side. The items are the mod's to place then, so to hand them on use `native_move_items`.
- **`native_move_items(dimension, from, from_face, to, to_face, item, max)`:** moves up to `max` items, one by one, and returns how many. An item the target refuses stays in its slot, so none is lost. `item` (`minecraft:coal`) takes only that item. It answers 0 within one container.
- **`native_insert_item(pos, face, stack)`** (API 20): puts a stack in the level in place, and returns what didn't fit.
- **`native_dimension()`:** the level in place (`minecraft:overworld`), for a hook that isn't told it, such as `native_block_entity_tick`.

During a machine's own tick its block entity is out of the world, so these answer as if no container were there: it has its slots in hand, in the `items` it's given. A composter takes one item a call, and these don't empty it: a hopper does. Container entities (minecarts, chest boats) aren't reached yet.

### Entities

`crates/horizon-client/src/game/native_entities.rs`: helpers for server mods, for the level in place, on a server only. Each
names the level it means by its `dimension` (`minecraft:overworld`), as `native_set_block` does.

- `native_summon(dimension, kind, pos, nbt)` spawns an entity of `kind` from its compound, as `/summon <kind> <pos> <nbt>`
  does (passengers included), and returns its id, in any loaded level since API @API:npc_any_level@
  ([NPCs](#server-hooks-for-plugin-ports-npcs)). None on a client, for a level not loaded, a position outside `/summon`'s
  bounds, a chunk that isn't loaded, a kind Horizon can't load, or one a mod kept out with `native_entity_join_level`.
- `native_move_entity(dimension, id, pos, yaw, pitch)` moves an entity Horizon keeps without simulating it (a display, a
  marker, an interaction); a display eases there over its teleport duration. Mobs move with `native_teleport_mob`.
- `native_set_display(dimension, id, &NativeDisplay)` sets a `block_display`, `item_display` or `text_display`'s
  transformation (translation, left rotation, scale, right rotation; quaternions `[x, y, z, w]`), the ticks players see it
  ease over (`interpolation`) and its teleport duration (`teleport`, 0 to 59). Each call restarts the ease on clients.
- `native_remove_entity(dimension, id)` removes such an entity as `/kill` does. Mobs go with `native_remove_mob`.
  Since API @API:remove_projectiles@ it also removes projectiles and server players' fishing bobbers.

A model that moves (a turning gun barrel, a thrown projectile) is an `item_display` showing an item whose model comes from
the mod's `.hmod` assets: summon it once, then move and pose it from a tick hook. Easing between ticks happens on each
client, so a call a tick looks smooth.

### Entity and level handles

`crates/horizon-client/src/game/native_entity_refs.rs` (#7387): an `EntityRef` names one entity of the open world and a
`LevelRef` one level. They are small `Copy` values a mod keeps, and their methods take the host, so a mod can hold
several at once: `host.entity(id)` makes one from the id an event carries, `host.level("minecraft:the_nether")` one from
a dimension.

- `entity.position(host)`, `block_position`, `rotation` (yaw and pitch), `kind` (`minecraft:zombie`), `uuid` (`&mut`:
  an entity with no UUID yet gets its own) and `level` read the entity.
- `entity.velocity(host)` (a player's is the move its client last reported), `set_velocity(host, v)` (not for players,
  whose client moves them), `on_ground`, `bounding_box` (lowest and highest corner), `eye_height`, `eye_position`,
  `view_vector` and `distance_to_sqr(host, pos)` read and move it.
- `level.block_state(host, pos)` (`minecraft:oak_stairs[facing=east,…]`, as `/setblock` takes it), `block` (the name
  alone), `block_property(host, pos, key)`, `is_loaded`, `height` (lowest `y` and height), `game_time` and `day_time`
  read any level the server has loaded, not only the one in place. A block in a chunk that isn't loaded reads as `None`.
- `level.seed(host)`, `difficulty` (`"normal"`, as `/difficulty` names it), `game_rule(host, "keep_inventory")` (a
  `GameRuleValue`, `Bool` or `Int`; None for an unknown rule), `weather` (rain and thunder levels, 0.0 to 1.0; both 0
  where the level has no weather), `is_raining` and `is_thundering` read its world settings.
- `level.set_day_time(host, ticks)` is `/time set` on the overworld clock (mods hear `time_skip` with cause `mod` and can
  keep the old time), `set_game_rule(host, "advance_time", GameRuleValue::Bool(false))` is `/gamerule` (`advance_time` and
  `advance_weather` freeze and unfreeze the day clock and the weather cycle), and `set_weather(host, clear_time,
  weather_time, raining, thundering)` is `/weather` (`ServerLevel.setWeatherParameters`; mods hear `weather_change` and
  `thunder_change` and can keep the old state). They return false for a negative time, an unknown rule or a value of the
  wrong kind or range, a level without weather or not the one in place, a client, a handle of an earlier open, or when a
  mod kept the old state. Bundles need `"hooks": @API:level_world_setters@`.
- `host.living(entity)` gives a `LivingRef` for a mob, a player, an armor stand or the dragon (vanilla's
  `LivingEntity`); `living.entity()` goes back. `living.equipment(host, EquipmentSlot::Head)` and `hand_item(host,
  Hand::Main)` read its equipment (an empty slot reads as `None`); `set_equipment(host, slot, stack)` puts a stack in a
  slot as `/item replace entity` does (None empties it; false where the entity has no such slot: a player has no body
  or saddle slot, while a mob has every slot, head and body apart; an air stack empties the slot). A mob's saddle can't be set yet, and a mob's off hand and
  body slot keep only the item, not the stack's count and components.
- `entity.nbt(host)` and `merge_nbt(host, compound)` read and change its data as `/data get` and `/data merge entity`
  do (merging works on mobs, dropped items, armor stands and item frames; never on players, as in vanilla);
  `tags`, `add_tag`, `remove_tag` work as `/tag` does, and `name` is its name as plain text.
- An `EntityRef` holds the world's open (`NativeWorldEpoch`) and the entity's id, which no other entity of that open
  gets. A handle of an earlier open, or of an entity that is gone or moved to another level (it gets a new id there, as
  in vanilla), reads as `None`.
- The entities are those of the level in place (the one a hook runs in) and the server's players, whatever their level.

On a client (API @API:client_entity_refs@), `host.entity(id)` and these reads work against the entities the server shows
the window: the window's own player (its id is the login's, once the server's first teleport made the player), other
players, mobs, armor stands and the rest the server added. A handle belongs to one client level: after a login or a
change of dimension (a respawn in the same level keeps it, as vanilla keeps its `ClientLevel`), the old one reads as
`None`, as does a server-side handle used on the client and the other way round. A client knows only what the server synced, so:

| Read on a client | What it gives |
|---|---|
| `position`, `block_position` | Where the window shows the entity (it eases toward the server's moves); for the own player, where it is. |
| `rotation` | Yaw and pitch as synced; the own player's view. |
| `kind`, `uuid` | The type and UUID the server added the entity with; a player's UUID is its profile's. |
| `level` | The client's level. `LevelRef` reads stay server-only and return `None`. |
| `name` | The custom name the server synced (`Entity.DATA_CUSTOM_NAME`: an armor stand's or a mob's), a player's profile name, else the type's name. |
| `velocity` | The motion the server last sent (`Entity.lerpMotion`); the own player's own velocity. |
| `on_ground`, `bounding_box`, `eye_height`, `eye_position`, `view_vector`, `distance_to_sqr` | From the client's copy, as on a server. |
| `living.equipment`, `hand_item` | What `ClientboundSetEquipmentPacket` sent (an armor stand's six slots, a mob's or another player's), the own player's own inventory. A mob's off hand and body slot keep the item only. |
| `nbt`, `tags` | `None`: the server sends neither saved data nor tags. |
| `set_velocity`, `merge_nbt`, `add_tag`, `remove_tag`, `living.set_equipment` | Refused (`false`): a client's copy changes only when the server says so. |

An entity the client tracks without a list of its own (a display, an interaction, a marker, a leash knot) has a handle
too, but reads only its `kind`, `uuid`, `level` and `position` (where the server last put it).

`host.menu_stacks()` (client, API @API:client_entity_refs@) gives the stacks of the menu the server opened for the window,
with their components, by the server's slot number (the numbers `ContainerSetSlot` and `ContainerClick` carry): a
chest's own slots, then the player's inventory, then the hotbar; an empty slot is `None`. Where `NativeScreenView.slots`
counts the slots, this lists them. It is `None` on a server, while no server-opened menu is up (the window's own
inventory, creative) and for a menu whose slots the client doesn't lay out (a lectern).

`host.menu_slot_roles()` (client, `native_menu_slot_roles`, API @API:slot_roles@) says what each of those slots is for,
in the same numbering, so an inventory mod (drags, the wheel, sorting, quick-stack) works in any menu without a list
of menu type ids: `NativeSlotRole::Hotbar` and `Inventory` (the player's), `Equipment` (armour, the off hand, a
mount's saddle and body armour), `Storage` (slots that only hold items: chests, barrels, shulker boxes, ender chests,
hoppers, dispensers, droppers, a mount's chest, a content mod menu type's `links`), `CraftingGrid` (the inventory's
2x2, the crafting table's and the crafter's 3x3, a mod menu type's `crafting` grid), `Result` (crafting, furnace and
workstation results, a mod menu type's `results` and crafting result) and `Input` (any other slot: a furnace's input
and fuel, a workstation's inputs, a mod menu type's slots its JSON gives no role). It is `None` where `menu_stacks` is.
The enum is `#[non_exhaustive]`: treat a role you don't know as `Input`. An injection mod, which clicks by Horizon's
own slot ids (`Game::slots`), asks `Game::native_slot_roles()` for `(slot, role)` pairs of the open menu screen
instead; Horizon's sort buttons (the extras) use it too.

Each read is a `Host` method too (`host.entity_position(entity)`): a helper whose catalog entry names a `receiver`
(`entity`, `level`) also becomes a method of that handle.

### Entities a client sees

`crates/horizon-client/src/game/native_client_services.rs`: lists for a client mod that draws the creatures, players
and dropped items around the player, such as a radar, name tags or a spectator's entity ESP. They read the client's own copy of what
the server shows the window, so they work the same in single player, on a LAN and on a server. Each is empty (or
`None`) on a server. They are snapshots: call them each frame and keep what you need.

- `native_client_entities()` (API 411) lists the mobs and other players the client knows, dying ones included (`alive`
  false). The window's own player is never in it (`native_player()` gives it), nor are the ender dragon, armor stands,
  vehicles, projectiles, displays and the other entities Horizon keeps outside its mob list. Each
  `NativeClientEntity` has the entity's `id` (the id `native_entity(id)` takes, for its custom name and the other
  handle reads), `position` and `previous_position`, `bounds`, `height`, `head_yaw`, `head_pitch`, `alive`,
  `invisible` (the flag or the effect), and for a player its `player_uuid` and `player_name`. From API
  @API:client_entity_list@ it also has:

  | Field | What it is |
  |---|---|
  | `kind` | The entity type, `minecraft:zombie`. |
  | `yaw` | `Entity.getYRot` in degrees, 0 facing south, 90 west. |
  | `baby` | `isBaby`. |
  | `spectator` | A player in spectator mode, whom vanilla draws only to spectators. |
  | `texture` | The texture vanilla's renderer selects for its body (`EntityRenderer.getTextureLocation`): the variant's, the baby's (`entity/cow/cow_temperate_baby`), and for a player its skin (`skins/<hash>` once downloaded, else the default skin for its UUID, `entity/player/slim/efe`). A pack's random textures and custom entity models are not followed. |

  `entity.position_at(state.world_partial_tick)` (with `native_client_state()`) is where the frame draws it,
  `Entity.getPosition(partialTick)`.
- `native_client_items()` (API @API:client_entity_list@) lists the dropped items the client shows, as
  `NativeClientItem`: `id`, `stack`, `position`, `previous_position` and `position_at`. `native_item_entities` stays
  server-only.
- `native_client_texture(name)` (API @API:client_entity_list@) gives a texture's pixels as `TexturePixels` by the name
  `texture` or `NativeClientProfile::skin` gives: a downloaded skin while the client holds it in memory (64x64), else
  `native_client_resources().texture(name)` ([Block states and resource files](#block-states-and-resource-files)), the
  resource packs' texture of that name. It decodes on each call, so cache the result by name, and clear the cache in
  `native_client_resources_reloaded`: other packs can change the pixels under a name. It is `None` for a texture it
  can't read and for a skin still downloading: ask again later.
- A player's skin by UUID: for a player in the level, find it in `native_client_entities()` by `player_uuid` and read
  its `texture`. `native_client_profiles()` (API 411) covers the player list instead, in the level or not: each
  entry's `uuid`, `name` and `skin` (the default one until the download is ready). `native_client_texture(name)` gives
  either's pixels.

A loop over them, from a mixin helper bound to the game:

```rust
let (Some(state), Some(me)) = (game.native_client_state(), game.native_player()) else { return };
for e in game.native_client_entities().iter().filter(|e| e.alive && !e.invisible && !e.spectator) {
    let [x, _, z] = e.position_at(state.world_partial_tick);
    let face = e.texture.as_deref().and_then(|t| game.native_client_texture(t)); // cache it by name
    // draw at (x - me.x, z - me.z)
}
for item in game.native_client_items() {
    let [x, _, z] = item.position_at(state.world_partial_tick);
}
```

### Mobs drawn by a mod

`crates/horizon-client/src/game/native_mob_draw.rs`: `native_mob_submit(mob, draw, drawn)` (client) is asked once a frame for
every mob in view but players (vanilla's `EntityRenderer.submit`). Set `drawn` and the mob's vanilla model, armour and held
items are left out (NeoForge's `RenderLivingEvent.Pre`, cancelled); its name tag, shadow, flames and glowing outline still draw.

- `mob` (`NativeMobRender`) is the frame's `LivingEntityRenderState`: the id, kind, custom name and `baby`, the partial tick,
  `age` (ticks lived plus the partial tick), `body_yaw`, `head_yaw` (from the body) and `pitch` in degrees, the walk cycle
  (`walk_position`, `walk_speed`: a vanilla quadruped's legs swing `cos(walk_position * 0.6662) * 1.4 * walk_speed` radians), `hurt_time`,
  `death_time` and `on_ground`.
- `draw` is a block entity's frame (`native_block_entity_submit`): draw with `native_draw_cube`, `native_draw_quad`,
  `native_draw_block` and `native_draw_item` under its pose stack, which starts at the mob's feet turned with its body: +z
  is the way it faces, +x its left, +y up, in blocks. Turn a head or swing a leg with `rotate_about` and a pivot. It holds
  the mob's light.
- A mod's own creature is an ordinary mob (`native_spawn_mob`, API 37) drawn this way: a quiet villager, an allay or a bat
  gives it movement, sounds and saving, and the mod gives it a model and animations of its own. Tell yours apart by the
  ids the server half sends with mod messages (API 40), or by its custom name.
- Players have their own hook, `native_player_submit(player, who, draw, drawn)` (`RenderPlayerEvent.Pre`): once a frame for
  each other player in view and for this window's player in third person, with the same frame and `who` (the profile name,
  and `local`). `drawn` leaves out the vanilla model, armour, held items, cape and other layers.
- `native_mob_layer(mob, who, draw)` (`EntityRenderersEvent.AddLayers`, `RenderLivingEvent.Post`) adds to every mob and player
  after its vanilla model and layers, in the same frame (not for one a mod drew itself); `who` is set for a player. The
  geometry hangs from the entity's frame, not from a model bone: place a hat from `head_yaw`, `pitch` and the entity type's
  eye height.
- A model built in code, as a NeoForge mod's `createBodyLayer()` builds it, is an entity model layer (API
  @API:model_layers@). `native_layer_definitions(layers)` (`EntityRenderersEvent.RegisterLayerDefinitions`, asked once at
  start) takes a mod's layers by id, `<mod id>:<model>#<layer>`. Each is a `LayerDefinition`: a `MeshDefinition` whose
  named parts hold `CubeListBuilder` boxes at a `PartPose`, on a texture of a given size. A box takes its faces from the
  texture as vanilla's do (`tex_offs`, `mirror`), and a `CubeDeformation` grows it over the same texels.
  `native_bake_layer(layer)` (`EntityModelSet.bakeLayer`) bakes one of them, or a vanilla layer by its
  `entity-models.json` name (`minecraft:zombie#main`), into a `ModelPart` tree. Bake it once and keep it. Each frame,
  `reset_all_poses` and set the parts' `x_rot`, `y_rot`, `z_rot` and place, as `setupAnim` does, then draw the tree
  with `native_draw_model_part(draw, part, sprite, tint)` (`ModelPart.render`). A mob's model is in the model's own
  frame, +y down with its front towards -z: turn the mob's frame into it first, as `LivingEntityRenderer` does.

  ```rust
  fn layer_definitions(&self, event: &mut LayerDefinitionsEvent<'_>) {
      let mut mesh = MeshDefinition::new();
      let root = mesh.root();
      let body = CubeListBuilder::create().tex_offs(0, 16).add_box([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0]);
      let body = mesh.add_or_replace_child(root, "body", body, PartPose::offset(0.0, 0.0, 0.0));
      let head = CubeListBuilder::create().tex_offs(0, 0).add_box([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0]);
      mesh.add_or_replace_child(body, "head", head, PartPose::ZERO);
      event.layers.push(("mymod:golem#main".into(), LayerDefinition::create(mesh, 64, 64)));
  }
  // In mob_submit, with `model` from host.bake_layer("mymod:golem#main"):
  draw.scale([1.0, -1.0, -1.0]);
  draw.translate([0.0, -1.501, 0.0]);
  host.draw_model_part(draw, &model, "mymod:entity/golem", 0xFFFFFF);
  ```

### Advancement triggers

`crates/horizon-client/src/game/native_criteria.rs`: a mod's own criteria triggers, as a NeoForge mod registers a
`CriterionTrigger` and calls its `SimpleCriterionTrigger.trigger`. The advancement is an ordinary file of a content mod
or a world's data pack (`data/mymod/advancement/connect_a_pipe.json`) whose criterion names the mod's trigger and
whatever conditions the mod reads:

```json
{
  "criteria": {
    "copper_pipe": {
      "trigger": "mymod:pipe_connected",
      "conditions": {"pipe": {"items": "#c:ingots/copper"}}
    }
  }
}
```

- `native_trigger_criterion(uuid, trigger, test)` fires it for one player, on a server only. Each criterion with that
  trigger the player hasn't been granted, of an advancement not done yet, goes to `test`, then to its `player`
  predicate, as vanilla tests one. Those that pass are granted; an advancement they complete is awarded as by the game's
  own triggers: the toast, the chat line, the rewards and the player's saved progress.
- `test` gets a `NativeCriterion`: its `conditions` (`Null` when it has none), and `item(key, stack)`, which tests
  `stack` against the item predicate under `key` as the game's own triggers read one: `items` (an id, a list or a
  `#tag`), `count`, the `potion_contents` and `damage` components, and the `enchantments` and `damage` predicates.
  Other components and predicates aren't checked. A missing key passes.
- It reaches the acting player inside their hook, and any player on the server, in their own level, only where no
  player is swapped in (`native_tick`, a menu hook), as `native_run_command`. True means the player was reached, not
  that anything was granted. It returns false otherwise, when the player isn't on, and for a `minecraft:` trigger or a
  name without a namespace: the game fires vanilla's triggers from its own events.

### Explosions

`crates/horizon-client/src/game/native_explosions.rs`: a helper for server mods, for the level in place, on a server only. It
names the level it means by its `dimension` (`minecraft:overworld`), as `native_set_block` does, and is refused when another
level is in place.

- `native_explode(dimension, center, radius, fire, blast)` sets off an explosion as `Level.explode` with no source entity
  does: blocks break (`native_explosion_detonate` sees them first and can spare some), entities are hurt and pushed, `fire`
  lights a third of the air above solid blocks, and nearby players hear and see it. `blast` is `NativeBlast::Keep` (no
  block breaks), `Block` (drops by `block_explosion_drop_decay`) or `Tnt` (by `tnt_explosion_drop_decay`). It answers false,
  doing nothing, on a client, for another dimension, a chunk that isn't loaded, or a radius outside `0 < radius <= 32`
  (TNT is 4, vanilla's largest is 7).
- A blast set off inside `native_explosion_detonate` runs that hook again, so a mod that chains blasts keeps its own guard.

### Camera shake

`native_camera_shake(partial, shake)` (client) asks every frame for the camera shake a mod wants: `pitch`, `yaw` and `roll`
in degrees and an `offset` in blocks along the camera's right, up and back. Add to `shake` rather than set it, so two
mods' shakes add up. Horizon reads it only while playing, scales the sum by Distortion Effects (a player who turns
that to 0% sees no shake), bounds it to 10 degrees and half a block on each axis, and multiplies it into the view
after the hurt tilt and view bobbing. A mod keeps its own shake's time and decay, from `partial` and its tick hooks;
it moves only what the player sees, not where they are or aim.

### A player's motion

`crates/horizon-client/src/game/native_motion.rs`: a push on a client, a flying speed on a server.

- `native_push_player([x, y, z])` (client) adds a push to the player's own velocity, in blocks a tick, as an
  explosion's knockback does: the player's own game moves them. The server didn't send it, so on a LAN or dedicated
  server the anticheat's movement check sees a move it can't explain (logged only unless the server sets it to send
  players back); for a push the server should stand behind, have the server send it. Each component is
  cut to 4 either way; an upward push lifts the player off the ground. A spectator, no player, or a value that isn't
  a number is left alone. A server mod that wants a client pushed sends its client half a message (API 40).
- `native_set_flying_speed(player, speed)` (server) sets `player`'s creative flying speed: vanilla's is 0.05, `None`
  puts it back, and it is cut to 0 to 50 (1000 times vanilla's). The player's client is told at once, a respawn and
  a game mode change keep it, and it is saved with the player as vanilla's `abilities.flySpeed` is. Past about 0.45
  (sprinting) a player flies more than 10 blocks a tick: anyone but the single-player owner then meets the server's
  moved-too-quickly check, which can send them back, as vanilla's does. It answers false for a player not on this server,
  on a client, and inside a player's hook (call it from a tick hook).

### Health HUD rows a mod draws

A mod that answers `native_health_hud` (API 27) draws the health, armor, food and air rows itself. Two client helpers
give it what Horizon's own rows use, without reading `Game`'s fields:

- `native_health_hud_state()` is Horizon's `HealthHud`. Its `left_height` and `right_height`, as they stand when the hook
  returns, place the selected item's name and the action bar above the rows, as vanilla's `Hud.leftHeight` and
  `rightHeight` do: set them when your rows take more or less room than vanilla's. They keep last frame's values until
  something sets them. To keep some of Horizon's rows (food and air, say), call its `layout(inputs, wi, hi)` with the
  hook's arguments and keep the draws you want; it also sets both heights. `layout` advances the rows' frame state (the
  hearts' blink, the air bubbles' pop sound, which the game plays after the hook), so call it at most once a frame.
- `native_local_attribute(name)` is the current value, base plus modifiers, of an attribute of this client's player
  (`minecraft:armor_toughness`, which vanilla doesn't draw), as `LivingEntity.getAttributeValue`. None with no player
  or for an id the player has no attribute for.

Bundles need `"hooks": @API:health_hud_state@`.

### What a client HUD reads

A minimap, a coordinates line or a waypoint mod reads the player and the session through helpers, not `Game`'s fields.
They answer the same on a server's client, a LAN guest and single player's window, which is a client of its own server
thread:

- `native_player()` is the player's feet this tick, their yaw and pitch as the mouse turned them this frame, and their
  dimension. `native_camera()` is the frame's interpolated camera, and `native_clock()` the time of day.
- `native_client_state()` (None on a server) has, besides `gui_hidden` (F1):
  - `screen`, what covers the world: `Playing` (nothing), `Chat`, `Death` (the death screen) or `Other` (the pause
    menu, an inventory or container, a sign, a book, a dialog or prompt, the loading screen, the title's screens). Xaero's
    minimap draws on `Playing` and `Chat` only.
  - `debug_screen`: F3's debug screen is showing.
  - `world`, the world joined, for a mod's data per world: `NativeWorldId::Singleplayer(folder)` is the save's folder in
    `saves/` (its LAN host and splitscreen seats too), `Server(address)` the address as the player joined it
    (`mc.example.com:25565`). A friend's world is `friend:` and what its invite shows anyone, its host's addresses and
    its relay nameplate, never the invite's secret; the host's code is new each time it shares, so this names that
    sharing only, as a LAN world's port does. None before a world is joined. A later Horizon may add kinds, so match
    it with a `_` arm.
  - `sleeping_pos`: the head of the bed the player sleeps in, as the server says (`LivingEntity.getSleepingPos`); None
    while awake. `native_block(pos)` names the bed.
- `native_biome(pos)` is the biome at a block (`minecraft:plains`, a data pack's own), as `Level.getBiome` picks it
  through `BiomeManager`'s zoom; None where that chunk isn't loaded.
- `native_data_dir(mod_id)` is the mod's own folder, `horizon-mods/.horizon-mod-data/<id>/` in the instance; the helper
  doesn't make it. Every world shares it: keep a world's files in a folder named from `world`.

Bundles need `"hooks": @API:session_state@`.

### Toasts a mod hides

`native_toast_add(kind, id, cancel)` (client) is NeoForge's `ToastAddEvent`: it is heard once for each new toast, just
before it is queued, so a mod can drop recipe toasts, tutorial hints or the advancement toasts of a large pack. `kind`
and `id` say which:

- `advancement`: the advancement id; `recipe`: the first recipe id of the toast;
- `tutorial`: the hint, `move`, `look`, `find_tree`, `punch_tree`, `open_inventory`, `craft_planks` or `socialInteractions`;
- `system`: `narrator_toggle`, `pack_load_failure`, `pack_download` (a server pack batch's progress), `low_disk_space`,
  `chunk_load_failure` or `chunk_save_failure`;
- `now_playing`: an empty id, each time the music toast shows a new song;
- `mod`: a mod's own toast (`native_add_toast`, below), with the id the mod gave it.

Set `cancel` and the toast isn't queued, shown or sounded. As in vanilla, an update to a toast already queued or shown
is not an add: more recipes joining the recipe toast, or a system toast taking new text (`SystemToast.addOrUpdate`).
A cancelled tutorial hint isn't asked again while it stays the current hint; a cancelled system toast is asked again at
its next update. The startup menu's narrator toast is not told: it shows before the game's `Host` exists.
Bundles need `"hooks": @API:toast_add@`.

### Toasts a mod shows

`native_push_voice(mod_id, source, samples, sample_rate, at)` (client, `host.push_voice`) plays a
mod's own audio, such as a proximity voice chat's: mono samples from -1 to 1 at `sample_rate` hertz.
`source` is the mod's own id for a speaker (a player's UUID, say): each source's blocks play one
after another, in the order they were pushed, while different sources play at the same time. `at`
is where they were spoken, for direction and a linear fade to silence at 48 blocks; none plays them
centred. A source holds at most two seconds, and a burst past that is refused rather than delayed;
at most 64 sources play at once. False when nothing was queued: on a server, without an audio
device, with nothing to play or past a cap. Bundles need `"hooks": @API:push_voice@`.

`native_add_toast(id, icon, title, description, challenge)` (client, `host.add_toast`) queues a toast of the mod's own
in the five slots the advancement, recipe and system toasts share, drawn as vanilla's `AdvancementToast`: the item
`icon` at 16 pixels (an unknown item draws none), `title` where the advancement toast's frame label goes ("Advancement
Made!"), yellow, and `description` under it in white. A description longer than one line is wrapped at 125 pixels and
fades in after the title, as an advancement's long title does. With `challenge` the title is pink and the challenge
sound plays as the toast leaves the queue. It stays up as long as an advancement toast (5 seconds times the
notification time setting). `native_toast_add` hears it first as kind `mod` with the mod's `id` (`mymod:quest/first`),
so a pack can hide them. True when queued; false on a server, without a window and when cancelled. A quest mod's
server half tells the player's client half (mod messages) and the client half calls it. Bundles need
`"hooks": @API:add_toast@`.

### The client's tick

`native_client_tick_pre()` and `native_client_tick_post()` (client) are NeoForge's `ClientTickEvent.Pre` and `.Post`:
they bracket each client tick of the window, the top and the end of vanilla's `Minecraft.tick`. The window ticks 20
times a second on the title screen, while paused and in a world alike, so a mod checks for itself whether a world is
shown (`native_clock()` is None on the title screen). A frame that falls behind runs the ticks it owes back to back,
each one bracketed. A server never fires them, single player's own included (it ticks on its own thread): server work
goes in `native_server_tick`. A tick that errors ends without its `post`. Bundles need `"hooks": @API:client_tick@`.

### Values for a mod's shader pack

`native_shader_params(partial, params)` (client) asks every frame, while a Horizon shader pack is on, for eight
`[f32; 4]` that every stage of the pack reads as `frame.mods` (`array<vec4<f32>, 8>`, docs/api/shaders-reference.md). What
they mean is the mod's own: a mod that bundles its pack as `native-<mod id>` writes the slots that pack reads, such as an
explosion's centre (camera-relative, from `frame.camera_position`) and its age in seconds, for a cloud or a shock wave
ray marched in a post pass against `scene_depth`. A value that isn't finite reads as 0, and all are 0 when no mod sets
them. The mod's server half can send what happened to each client half with mod messages (API 40).

### Biome colours

`crates/horizon-client/src/game/native_biome_colors.rs`: the grass, leaf and water colours a client mod changes while the
game runs, for seasons (a golden autumn, a pale winter) or a world that greens as it heals.

- `native_biome_colors(biome, colors)` (client) changes a biome's `BiomeColors`: `grass`, `foliage` (leaves, vines),
  `dry_foliage` (leaf litter) and `water`, 0xRRGGBB, starting from the biome file and the colormaps. The dark forest's
  modifier still darkens the grass set here; the swamp's noise colours replace it.
- It is asked for every biome when a mod calls `native_refresh_biome_colors()`. When the colours differ from those in use,
  every loaded section and the far terrain are rebuilt with them, as changing Biome Blend does. Call it when your colours
  change and once when the player joins, not every tick: a rebuild takes the workers a few seconds. A slow fade is a
  call a day, not a frame.
- Block-breaking and falling-leaf particles keep the biome's own colours for now.
- A season's mood in a shader pack (warmer grading, a lower sun, more haze) comes from the same mod through
  `native_shader_params` (`frame.mods`); its server half can send the season to each client with mod messages.

### Logging in and out

`native_client_player_network_logging_in()` (client) is NeoForge's `ClientPlayerNetworkEvent.LoggingIn`: the window's
player logs in to a server, single player's own included. It fires once per login packet
(`ClientPacketListener.handleLogin`), after the new client level (`native_client_level_load`). Respawns and dimension
changes don't log in again; a server that sends its player back through configuration logs it in again, with no logout
between. Horizon makes the player with the server's first position, so `native_player()` is None here, but the
connection is up: `native_send_packet` reaches the server. The counterpart is `native_client_disconnecting()`,
NeoForge's `ClientPlayerNetworkEvent.LoggingOut`. It is told before a connection is discarded, after
`native_client_level_unload`, and only when a connection ends: NeoForge's documentation says its event may also fire as
a new integrated server is created, and Horizon's doesn't. A mod keeps its per-server state between the two: caches, a
handshake with its server half, a welcome message. Bundles need `"hooks": @API:client_player_network_logging_in@`.

### The local player's combat, on the client

API @API:client_combat@, `crates/horizon-client/src/game/native_hooks.rs`: what the window sees of its own player's
fights, on any server (a vanilla one, a plugin server, single player's own thread). The server hooks
(`native_attack_entity`, `native_living_incoming_damage`) run only where Horizon is the server, so a client mod such as
a battle-music or combat-HUD mod listens to these instead.

- `native_client_attack(target, player)`: the local player attacked entity `target`, right after the attack packet left
  (`MultiPlayerGameMode.attack`, the same point vanilla swings the arm). `player` is whether `target` is another
  player. Whether the hit lands is the server's to say: a hit that lands comes back as that entity's hurt animation.
- `native_client_hurt(attacker, player)`: the server says the local player was hurt (`ClientboundDamageEventPacket`,
  the packet that tilts the camera and plays the hurt sound). `attacker` is the entity that caused it when the server
  names one (the shooter of an arrow, not the arrow; none for falling, lava or a cactus), `player` whether this client
  knows it as a player. An attacker out of this client's tracking range is still named, as not a player.

Both are only told: nothing to answer or cancel. Client only. Bundles need `"hooks": @API:client_combat@`.

### Sounds a client mod plays

`native_play_local_sound(sound, at, volume, pitch, delay)` (client) plays sound event `sound` at `at` for this player only,
`delay` ticks from now (0 to 1200). The event is vanilla's (`minecraft:entity.generic.explode`) or the mod's own: a content
mod's `assets/<id>/sounds.json` and `assets/<id>/sounds/*.ogg` load with the rest of its assets. A volume past 1 carries
farther, as vanilla's attenuation does. A client half that hears from its server half (mod messages, API 40) where
something happened can time the sound to the distance, such as a blast heard when its shock wave reaches the player:
`delay = distance / speed`. It answers false on a server and for a value out of range; an event the resources lack
plays nothing.

`native_muffle_sounds(amount, lowpass, ticks, exempt)` (client, `host.muffle_sounds`) muffles the game's sounds for a
while, as ears ringing after a close blast hear them: `amount` (0 to 1) of every sound's volume is taken away and
`lowpass` (0 to 1) dulls it, from untouched to a 300 Hz low-pass. Both start in full and fade back in a straight line
over `ticks` client ticks, held while the game is paused. The sound events in `exempt` (`mymod:ear.ringing`, at most
64) play clearly through it, those already playing too, so the mod's own ringing stays sharp; everything else is
muffled, sounds that were already playing, music and the interface's clicks included. A new call replaces the muffle
in place, and `ticks` 0 ends it at once. The muffle belongs to the sound output, so in splitscreen every player hears
it. Nothing changes while no mod calls it. It answers false, changing nothing, on a server, for an amount or strength
outside 0 to 1, and for an exempt event that isn't `namespace:path`.

```rust,ignore
// A close blast: half the volume gone and the world dulled, back to normal over 4 seconds.
game.native_muffle_sounds(0.5, 0.9, 80, &["ironfall:ear.ringing"]);
game.native_play_local_sound("ironfall:ear.ringing", eye, 1.0, 1.0, 0);
```

Bundles need `"hooks": @API:muffle_sounds@`.

### Fireworks a client mod draws

`native_client_fireworks(at, velocity, explosions)` (client, `host.client_fireworks`) draws a firework's explosions at
`at` in this client's world as `ClientLevel.createFireworks` does when a rocket explodes (vanilla's client-only
`FireworkParticles.Starter`): each `FireworkExplosion` in turn, two ticks apart, with its `shape` (`SmallBall`,
`LargeBall`, `Star`, `Creeper`, `Burst`), `colors` and `fade` (RGB, as the `firework_explosion` component's), `trail`
and `twinkle`, the flash, and the blast (large for three or more explosions or a large ball, far past 16 blocks) and
twinkle sounds. No explosions puffs a little smoke, as vanilla's. `velocity` is a rocket's, which the sparks carry.
Only the picture and the sound: there is no rocket entity, no damage or knockback, nothing on the server, and other
players see nothing. A mod that celebrates with them has its server half tell each nearby client half (mod messages)
to draw it. `FireworkExplosion` and `FireworkShape` are in `mod_api`. True when queued; false on a server, without a
client player or level, for non-finite numbers and for more than 64 explosions. Bundles need
`"hooks": @API:client_fireworks@`.

```rust,ignore
use horizon_client::mod_api::{FireworkExplosion, FireworkShape};
let star = FireworkExplosion {
    shape: FireworkShape::Star, colors: vec![0xFFD700], fade: vec![0xFFFFFF], trail: true, twinkle: false,
};
host.client_fireworks([x, y + 3.0, z], [0.0, 0.0, 0.0], &[star]);
```

### Fluid currents

`native_fluid_flow(dimension, pos)` is `FluidState.getFlow(level, pos)`: the normalised current at a position, the
push an item there gets. It is zero for still water and for no fluid, and none where the chunk isn't loaded or the
dimension isn't available (a client has only the level it is in), so a water wheel on a chunk edge can keep its last
speed rather than stop.

### A mod's own fluids in tanks

Tanks (`native_block_entity_fluids`) take a mod's own fluids as they take water and lava: any id in a namespace other
than `minecraft` (`create:chocolate`). `native_fluid_insert`, `native_fluid_extract` and `native_move_fluids` move them,
and `native_fluid_tanks` reads what a side reaches (`Host::fluid_tanks`). Equal ids merge in a tank; variants of a fluid,
a potion's kinds, are ids of their own (`create:potion/healing`). The mod draws its fluid in its block entities. A
fluid a content mod declares (`data/<id>/horizon/fluid/`, [mods.md](mods.md#content-mods-api-02)) also has network ids
and may be named in fluid tags; tanks still take undeclared ids as before. A declared fluid with a `block` flows in the
world as that block (`native_fluid_level_change` gets its id, `ruby:honey`, where vanilla's are `water` and `lava`); a
cauldron takes only water and lava. Its `bucket` pours and picks it up as vanilla's do, so `native_bucket_empty` hears it with its id.

A tank may refuse a fluid: `native_block_entity_accepts_fluid(block, tank, fluid, cancel)` (NeoForge's
`IFluidHandler.isFluidValid`) is asked on the server before `native_fluid_insert` fills a fillable tank, in both its
passes (same fluid first, then empty), and before `native_move_fluids` counts a tank's room. Set `cancel` to refuse the
fluid there, so a water tank takes no `create:chocolate` and a mod removed later leaves no fluid id the tank cannot use.
Every tank takes every fluid unless a mod says otherwise, and a cauldron, which holds only water and lava, never asks:

```rust,ignore
fn block_entity_accepts_fluid(
    &self,
    _host: &dyn Host,
    event: &mut BlockEntityAcceptsFluidEvent<'_>,
) -> Flow {
    if event.block == "mymod:water_tank" && event.fluid != "minecraft:water" {
        Flow::Cancel
    } else {
        Flow::Continue
    }
}
```

### Falling blocks

`crates/horizon-client/src/game/native_falling.rs`: a helper for server mods, for the level in place, on a server only. It
names the level it means by its `dimension` (`minecraft:overworld`), as `native_set_block` does.

- `native_spawn_falling_block(dimension, state, pos, velocity, falling)` throws a falling block of `state` from `pos` (its
  bottom centre) at `velocity` in blocks a tick, each axis cut to 3.9. It flies, lands and places itself as sand does, or
  drops as an item where it can't be placed. `NativeFalling` holds `drop_item` (default true) and `hurts` (hurts what it
  lands on as an anvil does, 2 a block fallen and 40 at most; default false).
- The block at `pos` stays: take it away first with `native_set_block` to make a block fall out of the world, as an
  explosion's debris or a collapsing tower would.
- It answers false, throwing nothing, on a client, for another dimension, air or an unknown state, a chunk that isn't
  loaded, a value that isn't finite, or with 1024 falling blocks in the level already.

### Bulk block edits

`crates/horizon-client/src/game/native_world_edit.rs`: a helper for server mods, for the level in place, on a server only.
It names the level it means by its `dimension` (`minecraft:overworld`), as `native_set_block` does.

- `native_edit_blocks(dimension, edit)` makes many blocks at once: `NativeBlockEdit` holds a `palette` of block states and
  a list of `filters` (block predicates, `#minecraft:logs`), both written as commands write them, and `runs`, each
  `height` blocks up column (`x`, `z`) from `y`, set to palette entry `block`, only where filter `only` passes the block
  there. With `surface` (`NativeHeightmap`: `WorldSurface`, `MotionBlocking`, `MotionBlockingNoLeaves` or `OceanFloor`),
  `y` counts from that height of the column, read once a column before anything changes. Runs are made in order, each
  seeing the changes of those before it.
- The blocks go straight into the chunks, a section packed once however many of its blocks change, as `fill ... strict`
  does: no neighbour is told, nothing drops, and no block updates or ticks; light is worked out again for the chunks
  changed, and they go to the players whole. A block replaced by another loses its block entity, and a new block gets
  the one `native_set_block` would give it (a mod block's own, API 15, included).
- It answers the number of blocks changed. Runs in chunks that aren't loaded, and the parts of runs outside the level's
  height, are left out. It answers `Err`, changing nothing, on a client, for another dimension, a palette entry or filter
  that doesn't read or carries NBT (`chest{...}`: refused, set states only), a run naming a palette entry or filter past
  the end, or more than 65536 filters.
- Block entity data: `NativeBlockEdit.block_entities` lists `NativeBlockNbt { x, y, z, nbt }` entries (SNBT), loaded after
  every run as `/setblock ... {nbt}` loads it, so a chest placed by a run keeps the `Items` given. An entry whose position
  holds no block entity, or whose `id` names another type (`chest` reads as `minecraft:chest`), is skipped and the block
  stays as the runs made it; NBT that doesn't read is an `Err`, changing nothing. Palette entries still carry no NBT. The
  data is loaded before any neighbour is told (`neighbour_updates`), and heightmaps, light and the sections' emptiness
  follow the blocks changed. `NativeBlockEdit` gains fields as the API grows (`block_entities` in API
  @API:edit_blocks_nbt@): build it with `..Default::default()`, as a struct literal naming every field stops compiling.
- `native_set_block_entity(dimension, pos, nbt)` loads SNBT into the block entity at `pos` the same way and answers
  whether it was accepted (false: another dimension, a client, a chunk not loaded, no block entity there, NBT that isn't
  a compound, or an `id` that isn't the entity's type). `native_block_entity_nbt(dimension, pos)` answers its saved NBT as
  SNBT (`/data get block`), None where there is none. API @API:edit_blocks_nbt@.

- `native_may_build(player, name, dimension, pos, action)` asks whether a player may edit one block, for edits that skip
  the normal place and break path (this one included): `action` is `place` or `break`, any other is refused. It asks
  every mod's `native_can_build(player, name, dimension, pos, action, cancel)` hook, a region-protection mod's chance to
  say no; any mod setting `cancel` denies the edit, and with no such mod it is allowed. Nothing asks it for you: a mod
  that edits for a player asks it for the positions it is about to change, and leaves out those it is refused.
  Bundles need "hooks": @API:can_build@.

### Light and resends for raw section writes

`crates/horizon-client/src/game/native_world_edit.rs`: more helpers for server mods that write sections themselves (a paste,
an edit of their own) and must get the result to the players. Each names the level by its `dimension`, as
`native_edit_blocks` does, and acts on the level being ticked only (0 or empty for another, and on a client).

- `native_relight_chunks(dimension, chunks)` lights the loaded chunks `chunks` (`[x, z]`, block coordinates `>> 4`) and
  the loaded chunks beside them again, whole and at once, from the blocks of each 3x3, as the light jobs
  `native_edit_blocks` queues do later. A chunk without all eight neighbours loaded is left out; a job already out for a
  chunk is dropped. The light that changed is saved and sent to the players for whom the chunk is on the edge of what
  they track; send the others the chunk (`native_resend_chunk`). Returns how many chunks were lit.
- `native_chunk_trackers(dimension, chunk)` gives the UUIDs of the players who have been sent the chunk, in join order
  (vanilla's `ChunkMap.getPlayers(pos, false)`).
- `native_resend_chunk(dimension, chunk, player)` sends the chunk whole again with its light, block entities and biomes
  as they are now, to every player who has it or only to `player` (a UUID, or none) when that player has it. Returns how
  many players were sent it. Relight first.

Bundles need `"hooks": @API:chunk_relight_resend@`.

### Machines

`crates/horizon-client/src/game/native_machines.rs`: helpers for a mod's machines on a server, in the level in place.
These are what Create's drill, saw, harvester and deployer need.

- `native_machine_destroy_block(pos, tool, collect)` (`Host::destroy_block`) is `Level.destroyBlock(pos, true)` with
  the loot rolled for `tool` and no player (`Block.dropResources(state, level, pos, blockEntity, null, tool)`).
  It shows the break particles and plays the sound, and the block entity's contents go into the drops as a player's
  break puts them. An ore gives experience as with no tool, and the block's fluid takes its place. Mods' loot changes
  (`native_loot_drops`) apply. With `collect`, the drops are returned instead of popped.
- `native_set_break_progress(breaker, pos, progress)` is `ServerLevel.destroyBlockProgress`: a crack, stage 0 to 9, for
  the players within 64 blocks. Pass a negative id of your own, the same each time; any other stage removes the crack.
- `native_use_as_player(stack, eye, yaw, pitch, target, sneaking)` is NeoForge's `FakePlayer`. It uses `stack` on a
  block (`useItemOn`), in the air (`useItem`) or on an entity (`interactOn`). The fake player is in survival and holds
  nothing else. It has no connection, so what it opens closes again before the call returns, through the same paths a
  player's close takes: a lid opens and shuts, a trader is free for the next player, and the click still counts as used.
  Its stats and advancements count for no one.
  The answer (`NativeFakeUse`) says whether the use did something, and what the fake player holds and carries
  afterwards. Call it from a block entity's tick, not from inside a player's hook: while a player acts, it answers
  `None`.

### Breaking as a player

`crates/horizon-client/src/game/native_player_break.rs`: `native_player_break_block(uuid, pos, into_inventory)`
(`Host::player_break_block`) breaks a block as the player `uuid` breaks it, for vein miners and area tools such as
FTB Ultimine, which calls `ServerPlayerGameMode.destroyBlock` from NeoForge's `BlockEvent.BreakEvent`.

- It runs the game's own player break, so the player's checks hold (spectator, adventure `can_break`, an item that
  can't break blocks, game master blocks, a dedicated server's spawn protection), `native_destroy_block` hears it
  and can cancel it, and the drops are rolled for the player's held item and its enchantments. The item wears as it
  would, the mined statistic, exhaustion and experience count, and the other players see the particles and hear
  the sound. The player's own client is left out of the break event (`Block.playerWillDestroy`), as in vanilla.
- With `into_inventory`, the block's own drops go to the player's inventory and what doesn't fit is dropped from
  them. What else the break sets loose (a torch losing its support, a container's contents) drops in the world.
- The answer is whether the block was removed. False, with nothing broken, for air, an unloaded block, a block a
  survival player can't break (bedrock), a player who isn't online or is dead, another player than the one whose
  action is running, and off a server. Reach and line of sight are not checked: that is the mod's choice.
- Call it from the player's `native_destroy_block` (they act there), or with any online player's UUID from a hook
  where no player acts, such as `native_server_tick`. Inside `native_destroy_block`, each block the mod breaks is
  heard by that hook too: keep a guard (an `AtomicBool`) so those breaks don't start more veins.

### Column reads

`crates/horizon-client/src/game/native_world_columns.rs`: helpers for server mods, for the level in place, on a server
only, that read many columns in one call. They name the level they mean by its `dimension` (`minecraft:overworld`), as
`native_set_block` does, and answer `Err` with the reason, reading nothing, on a client, for another dimension or for
a read too big, so a mod can tell "ask for less" from "wrong level". They run on the server tick: read a big area in
pieces.

- `native_surface_heights(dimension, columns, heightmap)` answers each column's height on `heightmap` (`NativeHeightmap`,
  as `native_edit_blocks` reads it: one above the highest block that counts), `None` for a column whose chunk isn't
  loaded; at most 65,536 columns a call.
- `native_read_columns(dimension, columns, heightmap, (below, above))` reads, for each column, the blocks from `below`
  under its height to `above` - 1 over it, bottom first: `NativeColumnBlocks` holds the heights (`bases`), the block
  names seen (`palette`), the blocks as indexes into it, a column after another, and three biome tints a column (grass,
  foliage and water, at the column's top). Blocks outside the level's height, and every block of a column not loaded,
  read as air (its tints as 0). `below + above` must be 1 to the level's height, and the read at most 1,048,576 blocks.
  The tints are the biome's own at the column's top, not blended over the biome blend radius as the game draws them, so
  near a biome border they can differ from what players see by a few blocks.

### Client world snapshots

`crates/horizon-client/src/game/native_client_world.rs`: helpers for client mods that read the world in bulk on their
own threads (a minimap baking tiles, a world map export, a shader pack's CPU side). They read the client world, what
the window shows: on a client of a server, the chunks the server sent. API @API:client_world_snapshots@.

- `native_client_chunks(from, to)` takes the loaded chunks from chunk `from` to chunk `to` (`[x, z]`, both included, in
  either order) as `NativeClientChunks`, which is `Send` and `Sync`. It shares the chunks with the world, so taking or
  cloning one copies no blocks. It reads by block position: `block(pos)` (a `BlockStateId`; air outside the world's
  height and in columns not loaded), `biome(pos)` (a `NativeBiomeId`, one a 4 by 4 by 4 cell, `y` clamped to the build
  height; none where not loaded), `loaded([x, z])`, `section_top([x, z])` (one above the highest 16-block section of the
  column's chunk that isn't all air: the column is air from there up) and `height()` (the world's build height, from its
  lowest y to one above its highest; `native_world_height` gives the lowest y and the height instead). `Err`, taking
  nothing, on a server or for an area of more than 66,049 chunks (the square a render distance of 128 loads).
- `native_client_chunks_changed(&snapshot)` names the chunks of the snapshot's area, in no set order, whose state in the
  world differs from the snapshot's now: written to since it was taken (blocks, biomes or light, even a write that left
  them the same), or loaded or unloaded where the snapshot had them the other way. Empty means the snapshot's blocks and
  biomes are still the world's. A snapshot keeps its chunks as they were, so the world copies the section it edits in a
  held chunk: drop snapshots you no longer read.
- `native_client_biomes()` gives the biome table the terrain is drawn with (`NativeClientBiomes`, `Send` and `Sync`), a
  mod's `native_biome_colors` included, none on a server. `name(id)` is the biome's name (`minecraft:plains`) and
  `colors(id, [x, z])` its `BiomeColors` at a column: grass as drawn there (the dark forest's darkening, the swamp's
  noise), foliage, dry foliage and water, before the biome blend the terrain smooths them with.
  `native_client_biomes_changed(&biomes)` says when the terrain is drawn with another table (a mod recoloured the biomes
  with `native_refresh_biome_colors`, the resources loaded again, another world opened): what was tinted from the old
  one is out of date even where no chunk changed.

A minimap takes a snapshot of the chunks a tile can see, bakes the tile from it on its worker, and keeps the snapshot
and the biome table with the tile. Every so often it asks `native_client_chunks_changed` for the tiles in view and
bakes again those with a changed chunk, and all of them when `native_client_biomes_changed` says so.

### Structure templates

`crates/horizon-client/src/game/native_structures.rs`: a helper for server mods that reads a structure template without
placing it, for a builder that places it a block at a time (a colony's builder, a schematic cannon). API
@API:structure_template@.

- `native_structure_template(id, at, rotation, mirror)` (`Host::structure_template`) finds template `id` (`mymod:hut`)
  as vanilla's `StructureTemplateManager.get` does: the world's saved structures (`generated/<namespace>/structures/`,
  what a structure block saves), then the game's and the content mods' (`data/<namespace>/structure/`). It answers what
  `/place template id` would place at `at` with that `Rotation` and `Mirror`: `NativeStructureTemplate` holds the
  template's `size` after the rotation and its `blocks`. Each block has its position in the level, its state after the
  mirror and the rotation, as `native_set_block` takes it, and the block entity data the template saved with it (`nbt`:
  a chest's items, a sign's text), for the mod to apply.
- The blocks come in the order `StructureTemplate.placeInWorld` places them: full blocks, then the others, then those
  with a block entity, each group by y, then x, then z. Of a template with several palettes, it is the one
  `/place template` picks at `at`. No processor runs, nothing is placed, and the template's entities are left out.
- It answers `None` on a client, for an id that doesn't parse or has a `.` or `..` part, and for a template the game
  doesn't have.

A builder reads the template once when it starts and keeps it. Then, each time it places a block, it walks there (its
goals, [Mob goals](#mob-goals)) and calls `native_set_block(dimension, pos, state, 3)`, skipping blocks that are already
right. It keeps how far it got in an attachment on the builder, so the build goes on after a save.

### Chunk tickets

`crates/horizon-client/src/game/native_chunk_tickets.rs`: `native_force_chunk(owner, [x, z], hold)` holds a chunk of the
level in place for `owner` (`create:train/<uuid>`), as NeoForge's `TicketController.forceChunk` does. The chunk stays
held while any owner holds it. It is kept as a `/forceload`ed chunk is: it is loaded with the ring around it, it ticks,
and its level stays open. `/forceload` neither lists nor releases it. `native_forced_chunks(owner)` lists what an owner
holds. Tickets are not saved: keep what you hold (a level attachment) and hold it again in `native_level_load`.

Where this differs from NeoForge, and its limits today:

- The ticket goes to the level in place, with no level argument, and every ticket ticks. Hold and release from a hook
  tied to that level (`native_level_load`, a block entity's tick), not from a server-wide one such as
  `native_lifecycle_server_tick_start`, where the level in place is whichever ticked last.
- The Nether and the End open only when something goes there, so their `native_level_load` doesn't run at start: a
  ticket held there before a restart stays released until the level opens again. `/forceload` has the same limit.
- A dedicated server that pauses when empty (`pause-when-empty-seconds`) stops ticking held chunks too.
- `owner` is any namespaced string, not tied to the calling mod, and `native_forced_chunks` needs it exactly: keep the
  owners you hold for, or a chunk stays held until the server stops.
- A chunk past the world's edge (beyond 30,000,000 blocks, as `/forceload` refuses) is refused. Each ticket keeps a
  5x5 ring loaded and the forced set is scanned per column: keep to what a machine needs (dozens, not thousands).

Asking for a chunk without waiting for it (Paper's `getChunkAtAsync` with a plugin ticket), API @API:chunk_load_async@:

- `native_chunk_load_request(owner, dimension, [x, z])` takes a ticket as `native_force_chunk_in` does and returns at
  once; the chunk comes in over the next ticks, loaded or generated, and stays loaded and ticking until released. True
  when the request is new.
- `native_chunk_load_state(owner, dimension, [x, z])` is `NativeChunkLoad::Pending` while it comes in,
  `Loaded` once it is loaded, `Failed` when `owner` holds no request for it (refused, or released). Loads nothing.
- `native_chunk_is_loaded(dimension, [x, z])` says whether a chunk is loaded now, in place or in a level open with
  nobody in it. It loads and generates nothing and does not open a level.
- `native_chunk_load_release(owner, dimension, [x, z])` lets go of the request; the chunk unloads once nothing else
  holds it. A request is a ticket, so `native_forced_chunks(owner)` lists it and the limits above apply.

### Mod messages

API 40, `crates/horizon-client/src/game/native_net.rs`: bytes between a mod's server half and its client half, on the
one custom payload channel `horizon:mod_net`. A vanilla client never gets them, and a vanilla server isn't sent any.

- `native_net_send_to_player(player, channel, data)` on the server and `native_net_send_to_server(channel, data)` on the
  client send one. The channel is `mod_id:name`: the id `[a-z0-9_]`, the name `[a-z0-9_./-]`, each 1 to 64 characters;
  `minecraft` and `horizon` are the game's. The data is at most 30 000 bytes, and 32 messages a tick go each way per
  player. Ok means accepted: a player who left doesn't get it. Sending to the server is an error until the player has
  joined.
- `native_net_from_client(player, channel, data)` and `native_net_from_server(channel, data)` hear them, all mods on every
  message: check the channel. What a client sent is checked by nothing, so treat it as a player's own input.
- A player's message reaches the server's hook in the tick after it arrives, when the helpers can reach every player;
  a message to a player leaves at the end of the tick, from any server hook.

### Scenes in screens

`native_draw_scene(batch, rect, blocks, view)` (client, `crates/horizon-client/src/game/native_scene.rs`) draws a small
scene into a box of a screen: block states at positions of the scene's own (`NativeSceneBlock`), each with its model and,
given a mod block entity, its drawing through `native_block_entity_submit`, `spin` parts included. `NativeSceneView`
sets the point at the box's middle, the turn (`yaw`, counterclockwise seen from above; at 0 the scene is seen from the
north), the tilt of its top towards the viewer (`pitch`) and the pixels per block (`scale`). The scene has a depth of its
own and is cut to the box. Draw it from the screen's drawing hook; Create's Ponder steps through its scenes this way.

Limits: each call collects every block's models again and calls `native_block_entity_submit` for every block entity
(`keep` has no effect here), so keep scenes small. The hook gets the scene's position as `pos`, with no world around it:
what it reads of the world at that position is unrelated. Only a mod's block entity draws; a chest or a sign shows its
block model only. `spin` parts follow the world's game time (and stop under `/tick freeze`): a scene with its own
timeline turns parts through the pose instead. Faces are shaded and culled as in the world.

### Chat and commands a client mod sends

API 404, `crates/horizon-client/src/game/native_net.rs`: `native_send_chat(message)` sends what the player
could type in chat, to any server, a vanilla one included: `ClientPacketListener.sendChat` and `sendCommand`. A leading
`/` sends the rest as a command, signed where its arguments are messages; anything else goes as signed chat. The message
is trimmed first, as the chat screen trims it. It isn't added to the chat history, and no client chat event fires
(vanilla's `ClientChatEvent` comes only from the chat screen). It is an error when the player isn't in a server's play
state (not connected, not joined yet or being reconfigured), when the trimmed message is empty, and when the Chat option
blocks it: Commands Only blocks chat, Hidden blocks chat and commands. The server's own spam limits apply as they do to
typed chat. Bundles need `"hooks": 404`.

`native_open_chat(text)` (API @API:open_chat@, same file) opens the chat screen with `text` typed in and the cursor at
its end, as T and / open it: a command macro can fill in `/tp ` for the player to finish. Nothing is sent: the player
edits and sends it, or presses Escape, and it goes into the chat history then like anything typed. The box keeps its
256-character limit and its character filter. It is an error with no player in a world, and while another screen
(the chat itself included) is open. Bundles need `"hooks": @API:open_chat@`.

While the chat is open, `native_chat_input()` (API @API:chat_input@, `crates/horizon-client/src/game/chat.rs`) gives its
text and cursor (counted in characters), `None` when it's closed, and `native_set_chat_input(text, cursor)` replaces
them, as an edit: completions refresh as they do for typing, so a mod that turns `:smile:` into an emoji as it's typed
runs from `native_screen_char_typed_pre` and writes the result back. The cursor is clamped to the text. Only what typing
lets in reaches the box (no control characters, DEL or section sign), within its 256-character limit. Writing is an
error while the chat isn't open. Bundles need `"hooks": @API:chat_input@`.

### Packets a client mod sends

API 409, `crates/horizon-client/src/game/native_net.rs`: `native_send_packet(packet)` sends one
serverbound play packet (`mod_api::ServerboundPlayPacket`, Horizon's `play::Serverbound`) to any server, a vanilla one
included. The usual one is a plugin channel's `ServerboundPlayPacket::CustomPayload`, for a server plugin or a Java mod
that listens on that channel; its data is at most 32767 bytes (`ServerboundCustomPayloadPacket.MAX_PAYLOAD_SIZE`), past
which a server disconnects. The packet goes out as it is: nothing counts it, and the game doesn't apply it on its side,
so a packet that needs a sequence number, a signature or the client's own state (a block placed, a slot clicked, signed
chat) is the mod's to get right. It is an error when the player isn't in a server's play state (not connected, not
joined yet or being reconfigured), and for a custom payload over the limit. Mod messages between the halves of one
Horizon mod stay with `native_net_send_to_server`. Bundles need `"hooks": 409`.

### Screenshots a client mod takes

API @API:take_screenshot@, `crates/horizon-client/src/game/native_client_services.rs`: `native_take_screenshot()` takes a
screenshot as F2 does (`ScreenShot.grab`): of the next frame, at the game's resolution times Screenshot Quality, into
`screenshots/` with vanilla's file name, with the same "Saved screenshot as" line in chat. It returns the path (a string) of the file it will
write, which exists once that frame is drawn. It is an error without a window to draw (a server, an offscreen run with
no renderer), and while another screenshot waits for the frame (F2's, an agent's or the world icon's): call it again
next frame. A timed or repeated screenshot mod calls it from its tick. Bundles need `"hooks": @API:take_screenshot@`.

### Server menus

API 10, `crates/horizon-client/src/game/native_menus.rs`: menus a server mod builds from ordinary items, which a
stock client shows as a chest (`generic_9x1` to `generic_9x6`); nothing is installed on the player's side.

- `native_open_menu(uuid, rows, title, items)` opens one in place of the player's open menu (its window id);
  `native_set_menu_items(uuid, items)` changes what it shows; `native_close_menu(uuid)` closes it.
- **No click moves anything.** Each reaches `native_menu_clicked(uuid, window, slot, button, mode)`, and the client
  is sent the menu, its inventory and its cursor as they were. `slot` counts the menu's own slots first, then the
  player's inventory below it: `native_menu_inventory_slot(size, slot)` gives the inventory slot (0..9 the hotbar).
- `native_menu_closed(uuid, window)`: the player closed it, another menu opened, or the player left.
- `native_player_stacks(uuid)`: the player's 36 inventory slots, to show copies or offer them.
- `native_exchange_items(a, a_offer, b, b_offer)`: each offer is inventory slots, each with the exact stack it must
  still hold. Both offers leave their slots and go to the other player where they fit whole (onto matching stacks,
  then empty slots), or nothing moves and the error says why (an offer that changed, no room, a player gone or
  dead). A trade on these menus leaves the offered stacks in their owners' inventories until this one step, so
  closing the menu, leaving or dying moves nothing.

The menu hooks run with no player's state swapped in; `native_player_stacks` and `native_exchange_items` refuse a
player whose state is (inside `native_perform_command`): call them from a menu hook or `native_server_tick`.

### Text, turning parts and copycats in block entities

- `native_draw_text(draw, text, at, color, glowing, outline)` draws a `Text` under a block entity's pose, as a sign
  draws its lines. One unit is one font pixel and y points down, so scale the pose by `[1/64, -1/64, 1/64]` as a sign
  does. Each span keeps its own colour, and glowing text is full bright. It returns the width, for centring. Only the
  spans' text and colour are drawn, in the default font: no bold, italic or other font yet. Clamp text read from NBT,
  since an outlined glyph costs nine quads, and widen `native_block_entity_render_bounding_box` for a long line, which
  the one-block default bounds cull while it is still on screen.
- `BlockEntityDraw::spin(axis, pivot, degrees_per_tick, phase)` makes what is drawn next turn by itself, and `still`
  ends it. The game works out the angle from the game time each frame (`game_time * degrees_per_tick + phase`), so a
  new speed needs a new phase to carry on from the current angle. With `keep`, a shaft turning at a steady speed
  is drawn again with no call of `native_block_entity_submit` until its block entity changes. This stands in for
  Flywheel's rotating instances, on the CPU.
- `native_block_face_sprite(state, side)` names the texture a block state shows on a side, as `native_draw_cube`
  takes it (`block/oak_planks`: vanilla's without the namespace). With it, a copycat block draws its material's textures on its own shape and keeps the result.
- Connected textures (casings, framed glass) need no code. A content mod ships OptiFine CTM rules under
  `assets/<id>/optifine/ctm/`, and the mesher reads them as it reads a resource pack's.

### Render hook points

`crates/horizon-client-render/src/gfx/native_render.rs`: the renderer's own hook points, free functions rather than
`Game` methods, which a bundle targets as `horizon_client_render::gfx::native_render::native_<name>`. They run on the
render thread with the frame's wgpu objects; what a mod draws there shows in that frame.

- `native_world_drawn(world)`: the world is drawn (sky, terrain, entities, particles, weather and the world post pass),
  the first-person hand, the HUD and screens not yet. `WorldDrawn` holds the device, queue and encoder, the frame's
  colour target (copy it out before sampling it), the world's reversed depth and its size, the camera's position and
  its matrices with and without TAA's jitter, the frame's camera-relative entity quads with their runs by entity, the
  entity atlas, the shared quad index buffer and the block sprite arrays ([block textures](#block-textures)). Not
  called on the title screen's panorama. `motion` says which mob, player or dropped item each run of the entity quads
  is, by its entity id, so a mod can match an entity's vertices with last frame's (a motion blur's own motion
  vectors); `spans` only name the type.
- `native_hand_drawn(hand)`: the first-person hand and held item are drawn, after `native_world_drawn` and before
  entity outlines, post effects, screen effects and the HUD, on every frame `native_world_drawn` runs. `HandDrawn`
  holds the device, queue and encoder, the frame's colour target with the hand in it, the hand's camera-relative
  quads (empty when no hand drew: third person, F1, a shader pack's own hand), the hand's clip matrix (its own field
  of view and the view bob, no jitter), the camera, the entity atlas and the quad index buffer. The hand's depth isn't
  kept: draw its quads into a depth of your own. A motion blur can match the hand's quads with last frame's for the
  swing, attack, equip and bob motion, and blur the picture with the hand in it.
- `native_render_needs(needs)`: set `entity_spans` to have the entity geometry split into runs by entity without a
  shader pack (each run's id is stable from frame to frame: a hash of the entity type's name, a block entity's state),
  and `world_depth` to keep the world pass's depth when nothing else needs it. Set what you need and leave the rest:
  several mods can ask. It's asked as each frame is built, so keep it cheap.
- `native_world_warp(warp)`: give the frame a [world warp](#world-warps), WGSL every terrain and entity vertex goes
  through. Asked once a frame before the world is drawn (not on the title screen's panorama); the last mod to set
  one draws.

### World warps

A world warp bends the drawn world itself: every terrain vertex (far terrain's tiles too) and entity vertex goes
through a mod's WGSL, so what moves is the real geometry, with its light and depth, seen from any side. Set
`WorldWarp` in `native_world_warp`:

- `source`: the WGSL, an `Arc<str>` (keep one and hand out clones: a source that compares equal to the last one is
  not built again). `None`, the default, is no warp.
- `on`: whether this frame draws with it. A source given while `on` is false is still built, so give it a moment
  before it's needed: building takes a while the first time (logged as "a mod's world warp built in N ms").
- `params`: sixteen floats the WGSL reads as `warp.params`, four `vec4<f32>`. Set them every frame.

The source has `world_warp`, and may go on after a line `// [fragments]` with `world_fragment` for the terrain's
pixels:

```wgsl
// pos: the vertex from the camera. section: the corner of the section or far tile it belongs to, from the camera
// (a vertex on the edge between two sections says by itself which it lies on no better than its position does);
// for an entity, its own position; for a block entity, its section's corner.
fn world_warp(pos: vec3<f32>, section: vec3<f32>) -> Warped {
    // where the world's point p is from the camera, exact far from the origin:
    // (p - warp.camera.xyz) - warp.camera_rest.xyz; warp.camera.w is the frame's partial tick
    let lift = sin((pos.x + warp.camera.x) * 0.1 + warp.params[0].x) * warp.params[0].y;
    // Warped: pos, where to draw it (from the camera); tint, what its colour is multiplied by
    // (terrain only: red under 0 marks the vertex's faces cut)
    return Warped(pos + vec3(0.0, lift, 0.0), vec3(1.0));
}
// [fragments]
fn world_fragment(f: Fragment) -> Painted {
    // Fragment: home (where the point really is, from the camera: world_warp's pos), at (where it is drawn), uv and
    // layer (sprite(f.uv, f.layer) is the block's texture), color (biome tint, light, shading and the warp's tint),
    // light (sky and block, 0 to 15), cut (over 0 where a tint had red under 0), front (false on a face's back)
    if f.cut > 0.0 || !f.front { return Painted(vec4(0.0, 0.0, 0.0, -1.0), 1.0); }  // alpha under 0: left out
    return Painted(sprite(f.uv, f.layer) * f.color, 1.0);  // Painted: color, and fog (how much of the fog it takes)
}
```

Without a `world_fragment` the terrain is drawn as the game draws it, less the faces a tint cut. The source goes into
copies of the game's terrain and entity shaders (`crates/horizon-client-render/src/gfx/world_warp.rs`), so prefix
what else it declares to keep clear of their names. What follows `// [fragments]` goes into the terrain's only:
entities have no pixel stage of the warp. A source that doesn't compile is logged ("a mod's world warp did not
compile", with naga's message) and the frame draws without it until the source changes.

While a warp draws, the warp's pipelines don't cull back faces and the frame leaves nothing out by where it was:
frustum, cave and occlusion culling (CPU and GPU) and the culling of section faces turned away are off for it, since
the warp may bring any of it into view. That makes the frame cost more: turn the warp off when it's done. A warped
frame also keeps to the plain per-section draws: flat draws, the visibility buffer, improved transparency, the water
mirror, and a Horizon shader pack's material stage and `[displacement]` give way to it for that frame (the pack's sky
and post stages still run; its shadows are of the unwarped world). A shader pack that draws the terrain itself (Iris
and OptiFine format) isn't warped: the frame draws without the warp, logged once. TAA reprojects by the camera alone,
so a warp that moves fast smears as other moving things do. Warped: terrain (solid and translucent), entities, items,
particles, block entities, entity shadows and name tags. Not warped: the hand, sky, clouds, weather, block outlines and
breaking cracks, lightning, and what mods draw themselves (world primitives, the world canvas, native models and
fluids). A frame without a warp costs nothing more: the game's own shaders hold only comments where a warp goes in,
and nothing of the warp's is made until a mod gives one.

`cargo run -p horizon-client-render --example world_warp_check -- <dir>` (with throwaway `HORIZON_HOME`,
`HORIZON_LAUNCHER_HOME`, `XDG_DATA_HOME` and `XDG_CONFIG_HOME`) draws a warp through the renderer offscreen.

### World primitives

`native_world_primitives(partial, camera, primitives)` (the trait's `Hooks::world_primitives`) runs once a frame while
the world is drawn, on the client, not on the title screen. A mod adds to `primitives`
(`horizon_client::world_primitives::WorldPrimitives`) in world coordinates, with ARGB colours:

- `line(from, to, color, width)`: a line `width` pixels wide;
- `box_outline(min, max, color, width)` and `filled_box(min, max, color)`;
- `beam(base, height, color)`: a beacon's beam from the centre of its foot, rotating and scrolling as a beacon's; the
  colour's alpha scales the beacon's own;
- `text(at, text, color, scale)`: text in the game's font facing the camera, centred on `at`, a name tag's size at
  scale 1; `.scale_with_distance()` keeps the size on screen it has 8 blocks away, `.background()` puts a name tag's
  background behind it.

Each returns the primitive, which `.through_walls()` makes seen through walls (no depth test); the others are hidden
by the world in front of them. Horizon draws them with its own pipelines after the entities and before the
first-person hand, into the world's targets: its reversed depth, its resolution (Resolution Scale) and TAA's jitter.
They blend over the world and write no depth. A mod never needs wgpu for them.

```rust,ignore
fn world_primitives(&self, _host: &dyn Host, event: &mut WorldPrimitivesEvent<'_>) {
    let home = [100.0, 64.0, -20.0];
    event.primitives.beam(home, 256, 0xFF55_FFFF);
    event.primitives.text([100.5, 66.0, -19.5], "Home", 0xFFFF_FFFF, 1.0)
        .through_walls()
        .scale_with_distance();
}
```

### Holding the world's picture

`native_world_hold(hold)` (the trait's `Hooks::world_hold`) is asked once a frame while a world is shown, on the
client, just before the frame's terrain uploads. A mod that calls `hold.hold()` keeps the terrain drawn as it stands
while the world changes under it; one that stops asking lets go, and every change shows on that frame. Use it to make a
change too big to go in at a stroke (thousands of sections) appear at once: the edits go in over seconds at whatever
pace suits them, the game lights, meshes and uploads their sections as ever, and letting go only swaps which meshes are
drawn, with nothing left to upload.

`hold.hold_within(x, z, radius)` holds only the sections whose middle lies in that circle (block coordinates; call it
again for more, at most 64 a frame). Smaller circles than last frame's let go of what they leave on that frame, and
changes outside them show as ever from then on: shrink them over a few frames to let a change spread across the land.
Several mods add up: `hold()` from any of them holds everything, and circles join. A circle that isn't finite or has a
negative radius is left out.

```rust,ignore
fn world_hold(&self, _host: &dyn Host, event: &mut WorldHoldEvent<'_>) {
    if self.crater_going_in() {
        event.hold.hold_within(120.0, -40.0, 300.0);
    }
}
```

- Only the terrain's sections are held, and only those already on screen: terrain streaming in during a hold shows as
  ever. In single player, far terrain waits too for the columns the server changes beyond the view inside the hold,
  and takes them in when it lets go of them, so they show a moment after the sections (far terrain summarizes them
  again). Far terrain a multiplayer server sends, entities, block entities drawn as entities, particles, light,
  collisions and the blocks themselves change at once, so the land a player sees may not be the land they stand on.
- Both the old and the new mesh of each waiting section are in memory. A hold is meant for seconds and is bounded: it
  lets go by itself after 30 seconds, past 1 GiB of waiting meshes and when memory for meshes runs out, showing what
  waited early rather than losing it; ended so, it stays off until no mod has asked for a frame. Leaving the world,
  another dimension, a shader pack that lays terrain out again and chunks unloading drop what waits with the sections.
- A section still being meshed when the hold lets go shows a frame or so later: give the game a moment between the
  last edit and letting go.
- Each player's game is asked for itself (splitscreen: one hold per player). The debug census lists
  `held_section_vertices`.

Bundles need `"hooks": 414`.

### Block textures

`crates/horizon-client/src/game/native_block_sprites.rs` and `WorldDrawn::block_sprites`: for a client mod drawing
blocks in its own pass (flying debris as voxels, say) with the player's own block textures, resource packs included,
as the terrain samples them. API 380.

- `WorldDrawn::block_sprites` (`BlockSpriteArrays`) holds the block sprite arrays as `D2Array` views: the terrain's
  eight, a 1-layer stand-in past the ones in use, each with the mip levels the Mipmap Levels option leaves, animated
  layers at the frame shown. Each texture is a whole square layer of `size` texels, its UVs 0 to 1 across it,
  gamma-encoded as the world uses it. `split` layers go to an array (2048 on most GPUs, fewer where the GPU holds
  fewer): every vanilla texture is usually in array 0. `nearest` samples texels sharp (nearest, repeating, the
  nearest mip level), `filtered` is the terrain's own sampler.
- `native_block_sprites(blocks)` says, for each block (`minecraft:grass_block`, `oak_log[axis=x]`; properties left
  out take the default), what its model draws on each side, in `Direction` order (down, up, north, south, west,
  east): its faces in the order drawn, each with its `sprite` (`array`, `layer`, `animated`, `cutout`), its `rect`
  on the side and `inset` from the side's plane (a whole face is 0, 0, 1, 1 and 0), its `uv` map, its `tint` source
  and whether it is `translucent`; the model's `particle` sprite; whether it is a plain `cube`. None for a block the
  game doesn't know. A side's axes run along x then z for down and up, x then y for north and south, z then y for
  west and east, from the block's corner nearest 0, 0, 0 (`block_sprites::side_axes`); the point (a, b) of a side
  shows the sprite at `(uv[0], uv[1]) + a * (uv[2], uv[3]) + b * (uv[4], uv[5])`, rotations and mirrors included
  (for a plain block's north side, `u = 1 - x, v = 1 - y`). A tinted face is multiplied by its source's colour:
  `Grass`, `Foliage`, `DryFoliage` and `Water` are the biome's where the block is, `Fixed` a constant (spruce
  leaves, redstone at its power). A grass block's side is two faces, the side and then its grey overlay tinted with
  grass. The model is the state's first random variant; faces that don't lie flat on a side (a flower's crossed
  planes) are left out, and the `particle` sprite stands for them. Fluids give their still texture on every side.
- `native_texture_sprites(textures)`: a texture's sprite by name (`block/stone`), none for one no model uses.
- `native_block_sprites_version()` changes each time the resources load (a resource pack turned on or off moves the
  layers, and the arrays are new): ask again then.

Look blocks up once and keep what you need (a few words a block for your own shader), not every frame.

### Block states and resource files

`crates/horizon-client/src/game/native_block_sprites.rs`, `horizon_client::block_states` and
`horizon_client::client_resources`: for a client mod working out what every block looks like (a map's colours, a
voxel renderer's table) or reading textures, on a thread of its own. `native_block_states()` returns a copy to keep,
and `native_client_resources()` a handle that reads the files on disk at each call. Both are cheap to clone, `Send` and
`Sync`: fetch them on the game's thread, then use them anywhere. Both follow the resources loaded at the time, so ask
again when `native_block_sprites_version()` changes (`native_client_resources_reloaded` hears the same reloads, but not
the first load). A reload rebuilds the tree's files in place, so while it runs a read can miss a file or get the new
pack's before the version changes: read again after it does.

- `native_block_states()` (`BlockStates`) holds every block state by its id (`BlockStateId`, as the world stores
  blocks): `len()`, `get(id)` and `iter()` give each state's `BlockStateFacts`:
  - `name`: its block, without properties (`minecraft:grass_block`);
  - `air`: air, cave air or void air (`BlockState.isAir`);
  - `invisible`: no model draws it (render shape `INVISIBLE`: air, barriers, light, and water and lava, which are drawn
    as fluids);
  - `fluid`: `BlockState.getFluidState` (`FluidState`, whose `kind()` is a `FluidKind`), a waterlogged block's water
    and kelp's included;
  - `waterlogged`: its `waterlogged` property is true;
  - `emission`: the light it gives, 0 to 15;
  - `outline`: `BlockState.getShape`, the outline the cursor picks, as block-local `[min_x, min_y, min_z, max_x,
    max_y, max_z]` boxes (`[0, 0, 0, 1, 0.5, 1]` for a bottom slab, none for air), without flowers' random offset;
  - `map_color`: `MapColor.col` as `0xRRGGBB`, its colour on a vanilla map, none for `MapColor.NONE`;
  - `collides`: whether it can collide at all (false for a state whose collision shape is always empty: flowers,
    water). `native_block_collision(pos)` gives a placed block's boxes.

  `sprites(id)` says what the state's model draws on each side, as `native_block_sprites` does for a block by name,
  and `texture(sprite)` names the texture behind any of its sprites (`block/grass_block_top`: vanilla's without their
  namespace, a mod's with it), the name `native_texture_sprites` and `native_draw_cube` take.
- `native_client_resources()` (`ClientResources`) reads the resources the game loaded as one tree: vanilla's, with the
  resource packs turned on (a higher pack over a lower one) and the content mods' files over it. `read(path)` gives a
  file's bytes by its path in the tree (`assets/minecraft/textures/entity/cow/cow_temperate.png`, a texture's
  `.png.mcmeta`), only for plain names under `assets/`; `texture(name)` the decoded pixels of a texture by name
  (`block/stone`, `entity/cow/cow_temperate`, `mymod:item/gear`), as `TexturePixels`: `width` by `height` RGBA, rows
  top first, as its PNG stores them. An animated texture is its frames one under another. Both are none for a file
  that isn't there.

A face's colour on a map, say: `states.sprites(id)` for its top, `states.texture(face.sprite)` for its name, and
`resources.texture(name)` for its pixels, averaged as the mod likes. Many states share a texture: decode each one
once.

### Menu extra data

A content mod's menu type opened by its block can carry bytes of the mod's own to the screen, as NeoForge's
`openMenu(provider, extraDataWriter)` does for a menu type made with `IMenuTypeExtension.create`: a machine's tier,
mode, owner, or the recipes it can run. On the server, `native_menu_extra_data(player, menu, block, pos, data)` is
asked when block `block` at `pos` opens menu type `menu` for `player`, after `native_inventory_block_start` let it
open. Set `data` to your bytes, only for your own blocks: the first mod to set it wins. Up to 32 KiB; more is dropped
with a warning in the log, and the menu opens without it.

A Horizon client gets the block's position and the bytes in a `horizon:menu_extra` payload just before the menu's
`OpenScreen`, so its screen is the block the server opened, not the block the player clicked last. While the mod's
screen is open (`native_screen_draw`, `native_screen_click`, `native_screen_tooltip`), `native_screen_pos()` gives that
block and `native_screen_extra_data()` the bytes (empty when the mod wrote none); both are None with no mod screen
open, and inside `native_screen` itself, whose slot layout the server asks for too and so can't depend on them. The
data slots still carry what changes while the menu is open; the extra data is sent once, when it opens. Works the same
on single player's server thread, for LAN guests and on a dedicated server. Bundles need "hooks":
401.

### Menu slot rules

`crates/horizon-client/src/menu.rs`: `native_menu_slot_rules(menu, slot, stack, may_place, max_stack)`, a free function
a bundle targets as `horizon_client::menu::native_menu_slot_rules`. A content mod menu type's container slot `slot`
(from 0) is asked whether it takes `stack` (`Slot.mayPlace`) and how many it holds (`Slot.getMaxStackSize`), as a
NeoForge mod's own `Slot` overrides them. `may_place` and `max_stack` come in with the type's own answer (false on one
of its `results` ([mods.md](mods.md#menu-types)), true elsewhere, and 99): change them for your own menu types.
`max_stack` is kept to 1 to 99, and the item's own stack size still caps it. It runs inside a click, a drag or a
shift-click (several times for one), with no level or player at hand: answer from the stack, and fast. It is asked
only for a server's players (single player's server thread, LAN guests, a dedicated server), and vanilla's menus
never ask it. As in vanilla (`AbstractContainerMenu.moveItemStackTo`), a
shift-click tops up a slot (not a `results` one) that already holds the same item without asking `may_place`, only `max_stack`: to keep a
slot from filling that way, lower `max_stack`.

### Recipe sources

`native_recipe_sources(player, menu, pos, sources)` is asked on the server when the recipe book or a recipe viewer
(`native_place_recipe`, `native_transfer_recipe`) fills the crafting grid of a mod menu type (`crafting`,
[mods.md](mods.md#menu-types)) open at `pos`. Push block positions onto `sources` and the placement may also take
ingredients from the containers there, as Tinkers' Construct's crafting station takes them from the chests beside it:

```rust
fn recipe_sources(&self, _host: &mut dyn Host, event: &mut RecipeSourcesEvent<'_>) {
    if event.menu == "mystation:station" {
        let [x, y, z] = event.pos;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            event.sources.push([x + dx, y, z + dz]);
        }
    }
}
```

Chests, barrels, shulker boxes, hoppers, dispensers, droppers and mod containers (their container slots) in the
player's level count. Anything else, the menu's own block and repeats are passed by, and at most 64 are used. The
placement counts and takes from the player's inventory first, then the sources in the order given, with vanilla's
`ServerPlaceRecipe` matching. What the grid held goes back to the player as vanilla's does. A changed source is saved
and its comparators update. Every mod adds to the same list. With no answer the placement is vanilla's, from the
inventory alone. A recipe viewer's craftable marks are still worked out on the client from the player's inventory.

### Loot odds

`native_loot_odds(table, tool)` (`host.loot_odds`) works out what one roll of the world's loot table `table` gives, its
data packs merged: one `NativeLootOdds` per item, with its chance to come at least once, the fewest one drop gives and
the most the table gives at once, and the conditions on its way it could not judge, taken as met and named
(`needs a brush`, `without a brush`, `in certain weather`). Weights, rolls (constant, uniform and binomial),
alternatives, groups, sequences, nested tables, `random_chance` and set counts count; luck and enchantments are none
(`table_bonus` and `random_chance_with_enchanted_bonus` take their unenchanted chances). With `tool`
(`minecraft:brush`), `match_tool` conditions are judged with that item in hand, so a block table brushed shows only
what a brush sifts out. Conditions are taken as independent of each other. None when there is no such table.

### Recipe viewers: lookups and categories

A mod that draws items on screens of its own (a quest book, a guide) can open whatever recipe viewer is installed
without depending on it. `native_show_recipes(stack, lookup)` (client, `host.show_recipes`) asks for `stack`'s recipes
(`NativeRecipeLookup::Recipes`, a left click), its uses (`Uses`, a right click), or for what a key that went down over it
does in the viewer (`Key("key.keyboard.a")`), so the viewer's own recipe, uses and bookmark keys work there as over its
item list, however the player bound them. It only queues the ask, so a screen hook with `&dyn Host` may call it. The
viewer hears it at the start of the next frame:

```rust
fn recipe_lookup(&self, host: &mut dyn Host, event: &mut RecipeLookupEvent<'_>) -> Flow {
    match event.lookup {
        NativeRecipeLookup::Recipes => open_recipes(host, event.stack),
        NativeRecipeLookup::Uses => open_uses(host, event.stack),
        NativeRecipeLookup::Key(key) if is_my_key(host, key) => act(host, key, event.stack),
        _ => return Flow::Continue,
    }
    Flow::Cancel // handled: no other viewer hears it
}
```

With no viewer installed nothing happens, and the asking mod works as before.

`native_recipe_categories(categories)` (client) collects the mods' own recipe categories for a viewer: a fishing spot,
a sieve, a machine. A `NativeRecipeCategory` has an id, a title and a subtitle in the player's language, an icon, its
catalysts (the items or blocks that do it) and its recipes, each of input slots to outputs (`NativeChanceOutput`: the
stack, the chance it comes each time, the count range, notes) with notes of its own. A viewer gets them with
`native_collect_recipe_categories()` and gets them again when `native_recipe_categories_version()` moves; a mod moves
it with `native_recipe_categories_changed()`, as when its server's data reaches it. The categories come from the
client, so a mod whose data the server owns (loot tables a data pack may change) works them out on the server and sends
them to its client half with its own messages, on join and after `/reload`.

### Menu links

`native_menu_links(player, menu, pos, links)` is asked on the server each tick while `player` has a mod menu type with
`links` ([mods.md](mods.md#menu-types)) open at `pos`. Push block positions onto `links` and the menu's linked slots
show the containers there, their slots in order, as real slots of the menu: a crafting station's side panel showing
the chest beside it, with a tab per neighbour, is a button (`native_screen_click`, `native_menu_button`) that changes
what the mod answers for that player:

```rust
fn menu_links(&self, _host: &mut dyn Host, event: &mut MenuLinksEvent<'_>) {
    if event.menu == "mystation:station" {
        let [x, y, z] = event.pos;
        event.links.push([x + 1, y, z]);
    }
}
```

Chests, barrels, shulker boxes, hoppers, dispensers, droppers and mod containers (their container slots) in the
player's level count; anything else, the menu's own block and repeats are passed by, and at most 64 are used. A
double chest is its two halves: push both, the first half first, as its own menu orders them. Linked slots past the
containers' slots hold nothing and are left out of clicks, as a hidden slot is. Every mod adds to the same list, and
with no answer the linked slots are empty. Clicks run on the server's menu engine as on the block's own slots, so
nothing is copied: what a player takes out of a linked slot leaves the chest in the same step. The menu does not check
the player's reach to a linked container: keep links to the blocks beside the menu's own. Guests, single player's
server thread and dedicated servers work alike; the deprecated in-frame mode does not show links.

### What a mod's container takes

`native_container_accepts(pos, entity, slot, stack, face, cancel)` is a hook (NeoForge's `IItemHandler.isItemValid`).
Hoppers, droppers and `native_insert_item` ask it before they put an item into a slot that the block's `ModContainer`
lets them fill. Set `cancel` to refuse the item there: a millstone takes only what it has a recipe for. Players' clicks
in a menu are `native_menu_slot_rules`' instead.

### Shift-click in a mod container

`native_menu_quick_move(pos, entity, container_slots, slot, stack, handled)` is a mod container's
`AbstractContainerMenu.quickMoveStack`, the method a Java menu overrides to say where a shift-clicked stack goes. It is
asked on the server before the game's own routing, for the mod block entity at `pos` whose menu a player shift-clicked
with the left or right button, on a vanilla menu type (`minecraft:generic_9x1`...) or on the mod's own menu type. `slot`
is the menu's index: below `container_slots` the block's own slot (`entity.items[slot]`), from there the player's
inventory, its 27 main slots, then the hotbar. `stack` is a copy of what that slot holds. Take what you move out of
`stack` (into `entity.items`, or into another block with `native_insert_item`) and set `handled`: the slot holds what
is left of `stack` (whatever you wrote to that slot of `entity.items`), the game's routing is skipped, and the player's
client gets the whole menu again. Unhandled, `entity.items` goes back to what it held and the game moves the stack as
for any container; a later mod sees what an earlier one did to `stack`. If the mod removes its own block, the menu
closes and the player's inventory is sent again. This is how a storage terminal
whose items live in other blocks, or a screen that hides the container's slots, takes a shift-click from the
inventory, and how a machine sends fuel to its fuel slot and ores to its input.

Not asked for an empty slot, a slot the mod's screen hides, another click type, a spectator or a dead player, a menu
no longer valid, or a shift-click during a drag (it only ends the drag, as in vanilla). It is asked once per click, where vanilla asks `quickMoveStack` again while the same item keeps moving: move
all you want in one call. The clicking player's state is swapped in (`native_acting_player` names them), and `entity`
is out of the world during the call, as in `native_menu_button`. Only a server's players (single player's server
thread, LAN guests, a dedicated server) shift-click this way; a native mod's own menus (`native_open_menu`) get their
clicks already.

### Items' default components

`crates/horizon-world/src/item_defaults.rs`: `native_modify_default_components(item, components)`, a free function a
bundle targets as `horizon_world::item_defaults::native_modify_default_components` (NeoForge's
`ModifyDefaultComponentsEvent`). `components` is the item's entry in `items.json`, vanilla's default-component report:
insert or replace a key to set a component (`"minecraft:max_stack_size": 16`), remove it to take the component away.
Any item can be changed, vanilla's included. Write values as vanilla's codec writes them: nothing decodes them with it.
Each table reads the fields it understands and takes a default for a field it can't read, but keeps the component, so a
malformed `minecraft:food` still makes the item edible, with no nutrition. A stack size above vanilla's 99 is kept: the
data pack's own is held to `ExtraCodecs.intRange(1, 99)`, a mod's to 1 to `i32::MAX` (the count the wire and the save
carry), and a value that is not a number leaves the data pack's own. Keep a stack above 99 to a server whose clients run
the mod: a vanilla client decodes the count with vanilla's 1 to 99 and cannot show it.

- Every table that reads items' defaults reads them through it: the item table, loot, enchanting, item models,
  banners, pot sherds, item rarity in chat. Stacks are saved and sent as their difference from the changed defaults, as from vanilla's.
- It's asked for every item each time the defaults are read, from any thread, several times as resources load
  and again when a world's data packs load. Answer the same each time, from the item alone.
- Client and server each apply their own mods, so both need the mod. A client without it (a vanilla Java client, or
  Horizon without the mod) fills in vanilla's defaults for the same stacks: a stone stack the server caps at 16 still
  looks like it stacks to 64 there.
- The creative search tab matches vanilla's exported tooltip text, so a changed `item_name` is not searchable yet.

## Trait mods (draft)

`cargo horizon-mod new <path> --template native --horizon <commit>` writes a trait mod to start from: the bundle's
`horizon-mixin.toml`, pinned to that Horizon commit, and an `[api]` crate whose `hooks()` greets each player who
joins. Its files are what a bundle may carry ([api/mod-files.md](api/mod-files.md)), so the launcher builds the folder
as it is (`native-build <path>`) and packs it into a `.hnmod`.

A bundle declares its trait mod package by its path inside the bundle:

```toml
[api]
path = "engine"
```

The package may use only crates and versions in Horizon's `Cargo.lock`, like mixin crates.
The folder contains a Rust package with a `horizon-client` dependency. Its library exports
`pub fn hooks() -> Box<dyn horizon_client::mod_api::Hooks>`. The launcher links this package
into a generated game executable and calls `hooks()` in dependency order (a mod after the mods it
depends on, else in bundle order). The package is not a
`[[crate]]`: it depends on the game instead of the game depending on it. A bundle with no
injections, state, bridges, modules, mixin crates or post-build steps skips horizon-mixins.

The package may depend on the bundle's own `[[crate]]`s by path, a sibling folder of the package in the bundle
(`shared = { path = "../shared" }` from `engine/`, where `shared` is a `[[crate]]` with `path = "shared"`). The
launcher points that dependency at the crate's staged copy, `native-crates/<mod>/<crate>`, so the crate is built once
with the game, as a `[[crate]]`'s own is. Any other path is refused, since only the package's folder is copied.
A crate used this way follows the same rules as a `[[crate]]`: no `horizon-client`, and only crates in Horizon's
`Cargo.lock`.

Turning a trait mod off in Options > Mods takes effect at once: the game skips its `Hooks` from the next dispatch on,
on the server thread and in the window alike, and turning it on again resumes it, with no rebuild while it is still in
the build. It stays loaded and keeps what it holds; what it registered at start (blocks, items, component types)
stays registered, and its `RenderHooks` keep drawing until a restart. Mixin code is not switched: it checks
`native_on` itself, as before ([design/mod-hotload.md](design/mod-hotload.md)).

### Dependencies between trait mods

A mod can publish functions and statics from its `[api]` package for another mod to call:

```toml
[dependencies]
base_mod = "^1"
extra_mod = { version = "^2", optional = true }
```

Requirements use Cargo semver syntax and are checked against each installed bundle's `[mod] version`,
not its Cargo package version. The dependent and every installed dependency need an `[api]` package.
Missing required dependencies, incompatible versions and cycles stop the build with the mods named.
An absent optional dependency is permitted; an installed optional dependency must match its requirement.

The launcher adds a path dependency using the mod ID as the Cargo alias and the target's actual Cargo
package name. In Rust, replace hyphens in the alias with underscores. Optional dependencies also get a
Cargo feature named exactly as their mod ID, enabled only when that mod is installed. Guard calls with
`#[cfg(feature = "extra_mod")]`. Do not define these dependencies or features yourself in Cargo.toml.
Public statics must use appropriate synchronization for shared and reentrant hooks; private state in
another mod's hook object is not exposed.

Trait dispatch follows dependency order, with ties kept in bundle order. Mixin injection order remains
horizon-mixins' priority, mod ID, then injection ID; dependency order does not change injection order.

A panic in a trait mod's hook adds `in native mod <id>, hook <hook>` to the panic's message, in the crash report
and in the server thread's log line, with the bundle's `[mod] id` (the generated executable passes the ids to
`mod_api::name_mods`; a build that doesn't gives the mod's place in dispatch order, `#0` first). The game still
stops as before. Render hooks and trait injections are not named yet.

A bundle with an `[api]` package may declare its own injection in the trait:

```toml
[[injection]]
id = "before-tick"
target = "horizon_client::game::example"
kind = "HEAD"
trait_method = "before_tick"
bindings = [{ name = "amount", ty = "&mut i32", value = "&mut *amount" }]
```

The generated trait method is `inject_<mod_id>_<trait_method>` (hyphens in the mod id become
underscores). Its event, as a catalog hook's, is that name in CamelCase plus `Event`
(`InjectExampleBeforeTickEvent`), with the bindings as fields under the same rules. Implement it on the object
returned by that bundle's `hooks()`; it is called only for that bundle's object. Types must be
public and available from both the target and `horizon_client::mod_api`. The kinds are HEAD listen,
or HEAD with `cancellable = true` for a unit-returning target (that method returns `Flow`, and
`Flow::Cancel` returns early from the target), and RETURN, MODIFY_ARG and MODIFY_EXPRESSION_VALUE.
For those three, the binding whose `value` is `$value` (its type is the returned, argument or
expression type, as horizon-mixins requires) becomes a plain event field. The method changes it in
place and returns nothing; the changed value is what the target returns, passes or uses. The selector,
`require`, `ordinal` and `argument` are horizon-mixins' own fields. Before the mods are installed, the
value passes through unchanged. A borrowed `$value` works only where Rust's elision ties the result to
one input. `cancellable` is HEAD-only. Other kinds are rejected. Do not specify `helper` on these entries.

```toml
[[injection]]
id = "points"
target = "horizon_client::game::example"
kind = "MODIFY_ARG"
selector = "self.give_points"
require = 1
ordinal = 0
argument = 0
trait_method = "points"
bindings = [{ name = "points", ty = "i32", value = "$value" }]
```

```rust
fn inject_example_points(&self, event: &mut InjectExamplePointsEvent) {
    event.points *= 2;
}
```

Trait injections can target only functions in `horizon-client`;
ordinary mixin helpers in other crates continue to work as before. The generated helper has the same priority and ordering as an
ordinary helper of that mod: priority descending, mod id then injection id. It is not automatically
ordered after other mixins. Catalog hooks retain the mixin-before-trait order below.

The launcher and catalog build scripts share `horizon-mod-api-gen` for trait, event and helper generation.
For game sources with that build dependency, the launcher stages declarations in
`.horizon-mod-api-injections.json`; Cargo consumes them through `HORIZON_MOD_API_INJECTIONS` without
editing generated Rust in the source tree. The declarations stay with cached mixin output. Older released
sources without that build dependency still receive the same declarations in their committed generated
trait. Neither path requires rustfmt during a player build.

A linked trait mod implements `horizon_client::mod_api::Hooks`. Mixin injections run first.
An injection that returns early skips trait mods. A claimed cancellation also skips trait mods.
Otherwise, trait mods run in their supplied order. The first `Cancel` stops dispatch.
For replacement hooks, the first `Some` wins. Collection hooks call every mod. Receiverless hooks have no `Host`.

Each hook method takes its host, where it has one, and one event:

```rust
fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
    if event.kind == "minecraft:pig" {
        event.amount *= 0.5;
    }
    Flow::Continue
}
```

The event is a generated `#[non_exhaustive]` struct named after the catalog hook, in CamelCase plus `Event`, with
the hook point's arguments as public fields: values as values and borrowed ones as references (`event.kind`). A
number or `bool` a mod may change is a plain field, written back once every mod has run, even after a `Cancel`;
anything else a mod may change, a game struct or a list, is a `&mut` field. A cancellation or replacement is still
the return value; a collection hook's list is a field (`event.lines`). One event goes through every mod, so each
sees the changes before it. Because no mod can build or match the whole
struct, a later version can add a field without breaking any mod, as NeoForge's event objects do; the cost is that
a mod's own tests drive a hook through its dispatch function ([Testing a trait mod](#testing-a-trait-mod)).

Hook methods take `&self` and require `Send + Sync`. The process-wide mod list is installed once, like mixin module statics. One mod instance serves both
the window's game and the server-thread game. A hook can also fire again inside
the same mod: for example, `set_block` fires `block_changed`. Mods keep mutable
state with interior mutability so these calls can share the instance safely. Data-load
hooks can run on any loading thread; item default changes must be identical on every side.

The trait has every hook in `native-hooks.json` and the pending fragments ([Adding a hook](#adding-a-hook)).
Level hooks use access-specific hosts rather than `Host`. `LevelHost` provides
`block_state` through `LevelAccess::block_state`, client-side detection and
`state_with_property`. `LevelRandomHost` provides vanilla's `Level.random` so
mod draws come in vanilla's order. `LevelTicksHost` adds scheduling through
`native_schedule_block_tick` and `native_schedule_fluid_tick`, while
`LevelWriteHost` adds random access, `native_set_block_property`, `native_play_sound` and
`native_send_particles`. Whole-block writes wait for a
later slice that creates block entities, as vanilla's `setBlock` does. The game owns
the tick queue; mods do not pass it back to these helpers.
For frame hooks, implement `RenderHooks` and return it from `Hooks::render`. The ordered
renderer list is installed once alongside the mod list. `render_needs` collects requests;
`world_drawn` observes the world pass and can encode drawing before the hand and HUD. Both
run on the render thread while the frame is built, before any deferred GPU submission.
The renderer module re-exports the game's wgpu types.

### The typed API from mixin code

A helper is compiled inside its target, as a [Sponge Mixin](https://github.com/SpongePowered/Mixin/wiki) handler is merged
into its target class, so it can use the typed API. On a `Game` method, bind the game and coerce it to `dyn Host`:

```toml
[[injection]]
id = "count"
target = "horizon_client::game::Game<'_>::tick_mobs"
kind = "HEAD"
helper = "count.rs"
bindings = [{ name = "game", ty = "&mut crate::game::Game<'_>", value = "&mut *self" }]
```

```rust
fn count(game: &mut crate::game::Game<'_>) {
    let host: &mut dyn horizon_client::mod_api::Host = game;
    let holder = horizon_client::mod_api::Holder::Level;
    if let Ok(old) = crate::game::example::COUNTER.get(&mut *host, holder) {
        let _ = crate::game::example::COUNTER.set(host, holder, old + 1);
    }
}
```

`COUNTER` is the `AttachmentType<i32>` of [Server attachments](#server-attachments), kept in a `[[module]]` of the
bundle (horizon-mixins' README): one with `file = "crates/horizon-client/src/game.rs"` and `name = "example"` is
`crate::game::example`. `Game` is not an `AttachmentHost` itself, so the `dyn Host` coercion is the step that matters;
any `Host` helper works on `host` too.

On a `GameLevel` method that takes `&mut self` (`native_random_tick`, `native_tick_block`, `native_perform_bonemeal`,
`native_is_bonemeal_success`), bind `{ name = "level", ty = "&mut dyn crate::mod_api::LevelRandomHost", value = "&mut *self" }`
for attachments and `random()`. Scheduling and `set_block_property` stay on the `LevelTicksHost` and `LevelWriteHost` the
hook points pass; `is_bonemeal_success` gets only a `LevelRandomHost`.

A trait injection gives the mod's `[api]` crate the same `Host` at that method. The `[api]` crate depends on
horizon-client's library, not on the game executable that helpers and modules are compiled into, so it declares its
own `AttachmentType<i32>` with the same `namespace` and `name`: the pair is the key, so both statics read one value.

```toml
[[injection]]
id = "count"
target = "horizon_client::game::Game<'_>::tick_mobs"
kind = "HEAD"
trait_method = "count"
bindings = [{ name = "host", ty = "&mut dyn crate::mod_api::Host", value = "&mut *self" }]
```

```rust
fn inject_example_count(&self, event: &mut InjectExampleCountEvent<'_>) {
    let _ = COUNTER.get(&mut *event.host, Holder::Level);
}
```

The `[api]` crate reads `event.host` at any `Game` method horizon-mixins can inject into at HEAD (its README lists the
limits). `hook` and `hook_version` work in place of `target`.

The helper runs in every `Game` that calls its target, and attachments answer `MissingHolder` outside the server's game,
as they do in hooks.

A catalog hook's trait method, event and return are part of its `hook_version` contract ([Named hook
points](#named-hook-points), [Mods on the hook points](#mods-on-the-hook-points)). Later Horizons only add event fields
(`#[non_exhaustive]`), trait methods with default bodies, and `Host` methods. So a mod whose `[api]` package uses only catalog
hooks (trait methods, or injections by `hook`) and `Host` can set `"hooks"` in its `.hnmod`'s `horizon-native.json` to the
`NATIVE_HOOKS_API` it was made on.
A raw `target` like `tick_mobs` is not a hook point and may change in any commit: such a mod pins `horizon_revision` and
leaves `hooks` out.

### Testing a trait mod

Every `Host` method defaults to a panic naming it (`not implemented: Host::tell`), so a test double implements
only what its hook calls, and an unexpected call fails loudly. Drive the hook through
`horizon_client::mod_api::dispatch_<hook>(&mut host, &mods, args…)`, which builds the event as the game does.
If a `Host` method returns a game value a test can't build, a double can return `None` instead, as with
`acting_player() -> Option<NativeServerPlayer>`. If the dispatch function itself takes an unconstructible value,
such as `dispatch_perform_command`'s `&NativeServerPlayer`, test the logic beneath the hook instead. The level
hosts are small enough to implement whole. NeoForge's GameTest is the in-game counterpart.

```rust
struct Stone;
impl Host for Stone {
    fn block(&self, _pos: [i32; 3]) -> String {
        "minecraft:stone".to_string()
    }
}
let mods = [Box::new(MyMod) as Box<dyn Hooks>];
let mut claimed = false;
dispatch_use_item_on(&mut Stone, &mods, &stack, [0, 64, 0], [0, 1, 0], &mut claimed);
assert!(claimed);
```

### Item fallback on native mobs

`native_item_interact_mob(entity, kind, stack, swing)` is a pending fragment for
an item's fallback after a native Mob's ordinary interaction passes. Existing
`native_interact_entity` (API 29), the entity's own handlers and built-in item
actions retain priority; errors propagate before the fallback. This is the
`ItemStack.interactLivingEntity` part of `Player.interactOn`, currently limited
to native Mob entries. Armor stands, other real players and dragon-part tables
are not covered by this hook.

The caller reuses normal mob selection: an already validated packet names its
exact entity ID; a local crosshair must reach the mob before a blocking shape.
The target must still be alive and not removed, and the acting hand must hold a
nonempty item. `stack` is that hand's item snapshot; a hook changes inventories
through ordinary Host helpers when needed. First `Some(true)` owns the turn and
swings once, `Some(false)` owns it without a swing, and `None` passes to the next
mod. An earlier mixin result also owns the output. No item count or world change
is performed merely because a hook answers. The fragment is unstamped; it adds
no manually assigned global API number.

### Adding a hook

A new hook needs its fragment in `tools/native-hook-fragments/` with its `mod_api` entry (for example
`{"contract": "cancel", "out": "cancel"}`), its `native_*` function with the placeholder body
`let _ = (every, parameter);` (all its parameters, in order), and a run of `cargo run -p xtask -- mod-api`.
That command only replaces the placeholder in `native_hooks.rs` with the `dispatch_<hook>` call; commit
that source change and the fragment. The `horizon-mod-api-gen` build dependency generates the hook traits,
`Host` methods and dispatch code into Cargo's `OUT_DIR` when the game builds, including release builds.
These generated Rust files are not kept in the source tree or committed, and no manual regeneration is
needed after a merge. The one exception is the renderer's (`horizon-client-render`, an engine crate, which
may not depend on the generator: [layers.md](layers.md)): its `RenderHooks` trait and dispatch are committed
in `gfx/native_render/generated.rs`. A render hook's change rewrites that file when you run
`cargo run -p xtask -- mod-api`, and the game's build fails while it is out of date. Every hook needs a `mod_api` entry: the build fails naming any hook without one.
A body that is neither the
placeholder nor already calls the dispatch is an error, and the generator never changes it. Hooks that are
free functions outside `Game` are wired by hand: those crates can't see the mod list, so each calls a callback
the executable installs once, such as `NativeLoadHooks` in `horizon-world` and `NativeClientHooks` in the
client library (`menu_slot_rules`, `living_breathe`, `ingredient`). A build reports a placeholder still to
fill instead of changing hand-written source; run `cargo run -p xtask -- mod-api` to fill it. Mixin injections
go at the head of the body, before the dispatch, so they still run first.

### Server attachments

A mod can declare an `AttachmentType<T>` once and call its `get`, `set`, `remove`,
`has` and `get_existing` methods with the hook's host and a `Holder`. `T` is
`Clone + Send + Sync + 'static`; `get` returns a clone and inserts the default on
first access. `has` and `get_existing` read without inserting (NeoForge's `hasData`
and `getExistingData`): `has` says whether a value is stored, saved or set, even
before it decodes, and `get_existing` returns `None` instead of the default. Neither
adds anything to the holder's saved data. `set` runs the optional codec before
changing the value, returning an `AttachmentError` if it cannot encode.
Keys are the pair `(namespace, name)`; a key already decoded as another Rust type is
an error. On a server, a missing holder is an error, not a new phantom holder; a client
keeps a copy for any holder it is asked about (see the sync paragraph below).
`AttachmentError` distinguishes `MissingHolder`, `TypeMismatch`, `Decode`, `Encode`,
`InvalidNamespace` (a malformed saved compound), `InvalidCopyPolicy` (death-copy
without a codec) and `InvalidSyncPolicy` (sync without a codec).

```rust
use horizon_client::mod_api::{AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Tag};

static COUNTER: AttachmentType<i32> = AttachmentType {
    namespace: "example", name: "counter", default: || 0,
    codec: Some(AttachmentCodec {
        encode: |n| Ok([("count".into(), Tag::Int(*n))].into()),
        decode: |data: &Compound| match data.get(&"count".into()) {
            Some(Tag::Int(n)) => Ok(*n), _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};
// Inside a server hook, or mixin code, with `host: &mut dyn Host`:
if !COUNTER.has(host, Holder::Chunk([0, 0]))? { /* chunk carries no counter yet */ }
let old = COUNTER.get(host, Holder::Chunk([0, 0]))?;
COUNTER.set(host, Holder::Chunk([0, 0]), old + 1)?;
Ok::<(), AttachmentError>(())
```

For mixin code, see [The typed API from mixin code](#the-typed-api-from-mixin-code).

Use `Holder::Level`, `Holder::Chunk([x, z])`, `Holder::BlockEntity([x, y, z])`,
`Holder::Entity(id)`, or `Holder::Player(uuid)`. Entity IDs cover every entity in
the level apart from players: mobs, items, experience orbs, projectiles, tridents,
falling blocks, primed TNT, fireworks, boats, minecarts, armor stands, cushions, item
frames, paintings, End crystals, the ender dragon, ominous item spawners and the
server's `OtherEntity` records. An entity keeps its UUID through saves, so its saved
attachments load back with it; an orb saved with a `count` above one loads as several
orbs, and only the first keeps the attachments. Player UUIDs cover online guests and
the acting player. Mutable level
hosts can access their level, loaded chunks and block entities; read-only level hosts
cannot access attachments. Unserialized values disappear on unload or restart. A saved
value is stored under `horizon:attachments`, then the mod namespace and attachment name,
as `{value: <compound>, copy_on_death: 1b, sync: 1b}` (a flag is omitted when false).
Unknown mods' saved entries survive load and save without being decoded. Level
attachments use the dimension's `data/minecraft/horizon_attachments.dat` SavedData
file, not `level.dat`.

A type with `sync: true` sends its value to Horizon clients when `get` creates the
default, on `set` and `remove`, and on demand with `AttachmentType::sync_data(host,
holder)`; changes made in one tick are sent once, at its end. The recipients are the
players who track the holder: guests who were sent a chunk for a chunk or block entity,
who track an entity for an entity, the player and those tracking them for a player, and
everyone in the level for the level. A client also gets all of a holder's synced values
when it starts seeing it: a chunk sent (with its block entities), a level joined or
re-entered, an entity tracked, and its own player. With no native
mod loaded nothing is sent. The wire form is the codec's compound, so `sync` requires
a codec.

Only Horizon clients are sent values. A message with a namespace or name over 256
characters, a value that cannot be written or a payload past 1 MiB is not sent.

On a client, `Host::attachments` returns a copy of what the server synced, which `get`
decodes; `set` and `remove` change only that copy, which is dropped when the chunk,
entity or player goes. A client keeps no copy for the level hosts (`LevelRandomHost`):
they stay server-only there, though their changes on a server are synced.

`copy_on_death` requires a codec. Player respawn copies opted-in entries via their
encoded representation, including raw entries of an absent mod; other entries are
removed. Living-entity conversion is not supported. This follows the default supplier,
optional server-side persistence, player-respawn option and `sync` with a `StreamCodec`
(with `syncData`) of NeoForge 26.3's public `AttachmentType.Builder` contract, and
`IAttachmentHolder`'s `hasData`/`getExistingData`, but not its holder-aware supplier,
custom copy handler, conditional serialization, the `sendToPlayer` predicate,
`AttachmentSyncHandler` partial updates or a wire codec apart from the disk codec.

`Host` exposes the documented `Game` helpers listed under `helpers` in
`native-hooks.json`, without their `native_` prefix. Each keeps the helper's
receiver and documentation; a helper without `self` (`config_dir`) takes
`&self` in `Host`. Level services stay on the level hosts above, except `play_sound` and `send_particles`, which the table below carries for a block entity's own tick.
A helper's `receiver` also exposes it on that handle. Receivers come from public
`*Ref` structs in `mod_api.rs`: for example, a public `BlockEntityRef` struct registers
`block_entity`. Adding a handle requires no separate generator registration.

Some helpers reach `Host` in another form, because their own signatures can't
go through `dyn Host` (`native_host_bridge.rs`):

| `Host` method | For | What it does |
|---|---|---|
| `mod_block_entity(pos)`, `change_mod_block_entity(pos, &mut change)` | `with_mod_block_entity` | Reads or changes a neighbouring mod block entity. On a client, `mod_block_entity` returns what the server sent (`native_block_entity_update_tag`), so a client mod can read another block's data. On a client, `change_mod_block_entity` changes only the local copy, and the next update tag from the server replaces what it changed: change it on the server. |
| `trigger_criterion(uuid, trigger, &test)` | `trigger_criterion` | The same call, with the test as `&dyn Fn`. |
| `play_sound`, `send_particles` | `LevelWriteHost`'s methods | Work in the level in place, from any server hook, such as a block entity's tick. |
| `block_name_in_tag(block, tag)` | `server_block_in_tag` | Takes a block name instead of a state. |

`equipment(slot)` is the player's own equipment on a client and the acting player's on a server.
`sneaking()` is `isSecondaryUseActive` in a use hook. `draw_model`, `item_entities`, `level_random`,
`server_item_in_tag`, `component`, `set_component` and `remove_component` are the helpers themselves.

The following documented `Game` helpers are not in the trait yet:

- `spawn_mob`, `mob`, `mobs`, `block_in_tag`, `item_in_tag`,
  `server_block_in_tag`, `server_block`,
  `set_display`, `sidebar`, `experience_orbs`,
  `player`, `panel_recipes`, `recipes`, `spawn_falling_block`, `explode`
  and `container_slots` use gameplay-private types.
- `with_mod_block_entity` and `trigger_criterion` are generic or take
  `impl Trait`, so their signatures cannot be called through `dyn Host`;
  the forms above stand in for them.

The following catalog hooks are not in the trait yet:

- Newly added upstream hooks `player_command`, `perform_console_command`, `item_decorations`,
  `client_resources_reloaded`, `camera_shake`, `screen_key_pressed_pre`, `screen_char_typed_pre`,
  `screen_mouse_scrolled_pre`, `use_duration`, `use_animation`, `can_eat`,
  `living_entity_use_item_start`, `living_entity_use_item_finish`, `render_hand`, `shader_params`
  and `block_highlight` will join a later slice.
- Newer upstream hooks `biome_colors`, `camera_distance`, `compute_fog_color`, `compute_fov`,
  `data_pack_files`, `mob_submit`, `mouse_button_pre`, `mouse_button_post`, `player_respawned`
  and `server_data_loaded` will join a later slice.

The client `tooltip_color` hook changes the background and top and bottom border gradient colours as mutable ARGB `u32` values; it does not change text colours. It covers solid-fill item tooltips (including creative, recipe book, chat, trading and bundle-selected item names), screen text boxes, and native panel tooltips. The stack is `None` for text-only boxes, including recipe-book item lines where the stack is discarded before drawing. Sprite-based screen tooltips (including options and title screens) use a separate renderer and are not covered; no texture hook is offered.

# Camera and collision reads

The `native_camera()` helper returns the most recently extracted world frame's camera, or none before one exists. Its position, interpolated eye height, partial tick, angles, final FOV, near/far planes and dimension accompany column-major view and projection matrices. The view is camera-relative and includes bob and spin; projection uses reversed Z. Earlier callbacks in a frame can see the previous frame. `native_block_collision(pos)` returns block-local `[min_x, min_y, min_z, max_x, max_y, max_z]` boxes with an empty collision context; unloaded blocks and collision errors give no boxes.

Entity-sensitive static shapes belong to the bundled content declarations
([entity-context shapes](mods.md#entity-context-shape-declarations)). They compile
at resource load and are selected by the captured entity class in normal
movement and selection. They are not callbacks into a native mod from physics.
Empty-context native reads retain the declared default shape. These declarations
cannot depend on the level, neighbouring blocks, position or block-entity data.

### Wind burst block protection

The pending `wind_burst` hook receives `dimension`, `center` and mutable `blocks`
just before wind-charge, breeze-charge or Wind Burst mace block interactions.
Remove positions to keep their doors, levers, candles and other reactive blocks
unchanged. The list has already been shuffled; entity knockback and the shuffle's
random draws are unaffected. A breeze blocked by `mob_griefing` does not call the hook.
This is separate from `explosion_detonate`, and does not cancel the burst's sound,
particles or entity effects. The pending fragment assigns its API number at merge.

### Advancement displays

The pending `advancement_display` helper (`host.advancement_display`) returns an `AdvancementDisplay` for an advancement id in the loaded tree: `title` and `description` as `Text` (the components as the datapack wrote them), `frame` as `task`, `goal` or `challenge`, `announce_to_chat` and `hidden`. It is None for an unknown id or one without a display. The helper loads the tree on first use, so it is for server-side reads, not hot paths. The pending fragment assigns its API number at merge (`@API:advancement_display@`).
