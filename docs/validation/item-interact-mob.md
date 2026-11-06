# Item fallback after a mob interaction

The pending `item_interact_mob` fragment adds a server hook after normal entity
interactions have passed. It covers native Mob entries, including an acting
guest's exact named target. Armor stands, other real players and dragon-part
interaction tables remain outside this bounded addition. Existing API 29 keeps
its earlier phase and ordinary swing behavior.

Minecraft 26.3 `Player.interactOn` calls the entity's `interact` first and returns
when it consumes the action, before the nonempty item's
`ItemStack.interactLivingEntity` fallback. The new hook follows that ordering.
Existing fallible ordinary handling retains `?`, so an error returns before the
fallback. No artificial error-producing interaction is introduced for testing.

`native_item_interact_mob(entity, kind, stack, swing)` uses the existing replace
contract: first `Some` owns the result, including an earlier mixin answer.
`Some(true)` ends the turn with the normal acting-hand swing; `Some(false)` ends
it without one; `None` passes. The caller reuses the ordinary mob selection and
block-occlusion checks, then requires an alive, nonremoved mob and a nonempty
acting-hand stack. Packet reach validation remains at the normal server entry;
the fallback does not repick another entity or impose a new line-of-sight rule
on an already validated named packet.

The actual prepared-resource Game test installs a process-local trait probe and
checks a real villager merchant menu before the fallback, earlier API 29
ownership, all three results, ordinary Blaze discard without loot or swing,
Panda's early pass, correct offhand stack, unchanged inventories, invalid/dead/
removed/ridden targets, empty and zero-count stacks, local block occlusion,
exact named-target handling and the normal spectator guard. A separate pure
dispatch test verifies first-response ownership and that later hooks never run
after either `Some(false)` or `Some(true)`.

Run with private prepared resources and fresh homes, no window or network
listener:

```sh
cargo test --offline -p horizon-client --lib item_interact_mob \
  -- --include-ignored --nocapture
cargo test --offline -p horizon-mod-api-gen -p horizon-mod-api-gen-tests --lib
cargo clippy --offline -p horizon-client --lib --tests -- -D warnings
cargo fmt --all -- --check
python3 tools/check_public_tree.py
git diff --check
```

Set `HORIZON_TEST_RESOURCES` to private prepared 26.3 resources and set
`HORIZON_HOME`, `HORIZON_LAUNCHER_HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`,
`HORIZON_NO_AUDIO=1` and `HORIZON_LAUNCHER_NO_SELF_UPDATE=1`. The ignored Game test
installs process-wide trait mods, so run its filter in a separate test process.
No Create bundle, third-party decoder, original asset bytes or installed player
world is involved. The hook fragment and generated dispatcher source were
processed with the official `cargo run -p xtask -- mod-api`; global catalog/API
history stamping is deliberately left to the normal upstream process.

Validation: both the actual Game and first-response dispatch tests passed;
the generator's 15 tests and generated-dispatch suite's 12 tests passed.
Full `horizon-client` library/test Clippy passed with warnings denied and no
exceptions, as did workspace formatting, public-tree and diff checks.
