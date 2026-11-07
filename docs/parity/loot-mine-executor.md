# Loot-mine executor presence

Own frozen base `13f70ad482df0a548e1954df8da4b967be3183cb` with a common test-only source-selection patch: actual Game/fresh pinned Java26.3 **129/137 →137/137** complete canonical records. Full pre/post inventories, exact feedback/styles/callbacks, fresh result/success stores and actual relative clock0 remain strict. [Every old expected/actual residual](loot-mine-executor-residuals.md) is retained; fixed has none in137.

## Source admission and cases

The default existing harness owner is independently admitted as a real creative Player (UUID/type/Health20/position), not a console. A separate explicit server source has no entity while that player remains connected. Every case positively checks `execute unless entity @s` in its outer server context; entity cases then use actual `execute as Player` or a fresh UUID-bound ordinary armor stand, with independent type/body/UUID reads. The stand has Health20, NoGravity and an admitted exact position. Sources are not inferred from loot results.

The scene has a loaded stone floor, guarded snow[layers=1], dirt and a real chest. Slot0 is reset and positively read as emerald4 independently for each case. Slot1 diamond3 and slot2 diamond_shovel remain unchanged; the actual shovel stack and its loaded shovels tag are independent positive controls. All tested commands use the explicit diamond_shovel tool input. Source blocks stay unchanged after mine.

| Actual executor | Source block | Java/fixed outcome | Untouched outcome |
| --- | --- | --- | --- |
| Player | Snow | Snowball1, success/result1 | Accepted0, sentinel remains |
| Armor stand | Snow | Snowball1, success/result1 | Accepted0, sentinel remains |
| Server, no entity | Snow | Accepted callback true/0; success1, result0; sentinel remains | Same |
| Player | Dirt | Dirt1 | Same |
| Armor stand | Dirt | Dirt1 | Same |
| Server, no entity | Dirt | Dirt1 | Same |

These are implicit single-result replacements. The accepted-empty server result is distinct from rejected input. No H039 zero-count entry is needed or claimed.

## Existing harness source selector

The bounded fixture uses `source:"console"` on existing `cmd` operations. Omitted source retains the existing owner/player behavior. Java obtains the actual `MinecraftServer.createCommandSourceStack`; `withSource(player.commandSource())` only reuses the existing feedback receiver. Pinned `CommandSourceStack.withSource` preserves entity, names, position, permissions and level. It does not turn null entity into a player. Native uses existing `Game::server_source`, unsuppresses feedback for the same existing capture, runs the command and restores the previous source. The permission-only `as_console` alone would not remove the owner entity.

The source-selector implementation is identical in old/fixed builds (common patch SHA256 `dd422429271e1ab5de0211b3bb42c5d6825b8f9e5c640052de2d6b3d3cdf6d80`). Fresh Java validates the exact137 program before the native pair. Both native builds pass all source admissions. The output observer/schema is unchanged. This is an admitted Overworld command-context fixture, not stdin/RCON delivery, console broadcast or other-dimension parity.

## Production boundary and regression

Pinned `LootCommand.dropBlockLoot` supplies optional THIS_ENTITY from actual `source.getEntity()`. `Game::cmd_loot` now reads existing `source_entity().is_some()` and passes it through the existing generic block-drop presence parameter in both random-sequence branches. That parameter becomes the existing `LootContext.this_entity` Boolean; no new API/context/model field or loot-engine rewrite is added. Stand is not mapped to player identity.

The call retains the same tool, dynamic-empty input, random stream, absent nested sequences/explosion radius and post-evaluation BE attachment. Normal breaking and other consumers are unchanged. The library's existing presence view supports the measured empty entity predicate; this does not supply full entity identity/flags/equipment or establish arbitrary/named entity predicate parity.

A registered Game command regression first admits an actual stand and a real server source. Unconditional dirt succeeds for both, server snow leaves the exact sentinel unchanged, then stand snow fails old at the intended saved item assertion (emerald versus snowball). Fixed passes with unchanged source body/UUID, block state, unrelated slots and world clock. The unit covers stand/server; the actual runtime pair separately covers the real connected player. The existing H035 pot-copy unit also passes fixed. No synthetic player view is used.

All six full stand-body traces match in both builds. Exact absolute startup-clock traces remain raw and unequal; only actual relative0 is canonical. No complete raw-clock or full entity-family claim is made.

## Adjacent patches

H035 #11278's actual BE component attachment and H041 #10400's inline fishing-tool handling are retained. H039 #11258 remains independent and is not imported: when combined, preserve both its command-only keep-empty evaluation/destination filtering and this actual executor-presence carry. Thread presence through its command block-drop context; selecting one wrapper while losing the other behavior is incorrect. Current source retains the existing filter unchanged.

## Reproduce and evidence

```sh
PYTHONPATH=tools python3 tools/test_loot_mine_executor_parity.py --reference-only --work-dir .local/h060/new-java
PYTHONPATH=tools python3 tools/test_loot_mine_executor_parity.py --reference .local/h060/new-java/vanilla.jsonl --binary /absolute/path/to/game --work-dir .local/h060/new-game
```

The public wrapper emits exactly the preserved paired operations. Script source SHA256 `1c791099ad958c8476dab7017ac9376f419878bd129693529fe9e6d9b7025726`; emitted script SHA256 `ac42621bf66c5fd8421839883f2beb865e9831d8ec4cba1645439ca92b569b67`; Java reference SHA256 `871564cfb7fb705830d7e3e0be86b73a5069141e73b9961b83de9a6a5e18962a`. Emitted script/reference bytes are identical across the pair.

All source snapshots, declarations, raw records, logs, binaries, unit artifacts and stage manifests stay under `.local/h060`. The first Java adapter compile attempt passed ServerPlayer instead of its existing CommandSource receiver; it failed before world execution and is preserved as a rejected prerequisite, not a native baseline. Scope is measured THIS_ENTITY presence for loot mine, without normal-break, RNG policy, tool, BE copy, slot-list, connected-player, broader predicate or transport closure.
