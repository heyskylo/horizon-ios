# Inline fishing TOOL context

H041 only supplies the actual optional tool to the existing inline fishing
context. Pinned Java26.3 `LootCommand.dropFishingLoot` inserts TOOL before the
resolved table is evaluated. Its no-tool command endpoint supplies
`ItemStack.EMPTY`; explicit `mainhand`/`offhand` endpoints are separate.

`Items::inline_loot_table` keeps its public signature and no-tool behavior.
The additive `inline_loot_table_with_tool` passes a borrowed stack to the same
`LootContext`/evaluator. Game calls it only with a tool for inline `loot.fish`
when an item or hand was actually supplied. Plain inline loot and omitted tool
keep None, even while the owner holds a rod. The named fishing branch, random
sequence selection, existing random sources and other context fields are
unchanged. No hardcoded item/slot logic in production.

## Actual Game / fresh pinned Java

Frozen base `69fab597a6f0e8a6ce524d38d1b73e599e343034`. Existing command harness,
no new observer or custom fixture pack. Ten deterministic cases:

| Case | Source/tool | Expected saved output |
| --- | --- | --- |
| rod_rod | inline match rod / explicit rod | diamond1 |
| rod_stick | inline match rod / explicit stick | absent |
| stick_rod | inline match stick / explicit rod | absent |
| stick_stick | inline match stick / explicit stick | diamond1 |
| unconditional | unconditioned inline fishing / stick | diamond1 |
| plain_no_tool | plain inline loot, rod condition, owner holds rod | absent |
| omitted_tool | inline fishing, rod condition, tool omitted | absent |
| actual_mainhand | inline fishing, rod condition / mainhand | diamond1 |
| named_shears | builtin blocks/cobweb / shears | cobweb1 |
| named_stick | builtin blocks/cobweb / stick | string1 |

One positively identified, loaded supported chest has ten independently absent
slots. Every case reads its exact pre/post stack, id/count/typed Slot and data
presence, with fresh result/success sentinels. The final complete inventory
retains all six successful outputs in their distinct slots. No prefilled target
or empty-slot replacement policy is being tested (H039 remains separate).
Actual owner UUID/type/Health20/mode/Pos and selected rod id/count, chest
coordinates/state, floor/origin/load and actual relative clock0 are strict.
No observation ticks; no natural hook, luck or modifier claim.

Predeclared **393** compares complete raw records, including all message parts,
styles and callbacks, with no masks/tolerance. **397** additionally records
exactly `initial-owner-body`, `clock-start`, `final-parent-block`, `clock-end`.
All raw bodies and exact absolute clocks remain executable diagnostics. The
fresh Java reference is immutable across each pair; newly generated Java outputs
are also retained, without replacing their raw clocks.

| Program | Execution JSONL SHA-256 |
| --- | --- |
| 393 | `063a36c02e5823da9f083e05b163382662f54a1e0bc2b2f3b28e9c09a00ac3dc` |
| 397 | `f71d2a30d4b2a4d79418c25d2342cc985cea32b4701f302a6cde8d3f947b9edb` |

```sh
python3 tools/test_inline_fishing_tool_parity.py --reference-only --work-dir /tmp/h041-java
python3 tools/test_inline_fishing_tool_parity.py --binary /path/to/old-game --reference /tmp/h041-java/vanilla.jsonl --work-dir /tmp/h041-old
python3 tools/test_inline_fishing_tool_parity.py --binary /path/to/fixed-game --reference /tmp/h041-java/vanilla.jsonl --work-dir /tmp/h041-fixed
```

Use `--full-diagnostic` and its own fresh reference for397. All scripts, raw
JSONL/colored feedback/callbacks, strict reports, immutable executables, hashes,
bytecode/ledger/duplicate review and measured stage times remain in `.local/h041/`.
Unknown starts stay unknown; no game resources/bytecode are committed.

## Regression and retained drafts

The resource-free evaluator/context unit covers explicit None/rod/stick against
rod/stick/unconditioned tables, the legacy no-tool API, a named-table positive
and the exact next random value after each equivalent named/inline evaluation.
Its old-behavior capture uses an API-only extraction retaining the original
`held:None` constructor, then fails empty output versus diamond1. The identical
unit passes after carrying held into the constructor. The actual Game baseline
was compiled from untouched69fab before that extraction; it is not a stubbed
Game/context or synthetic slot answer.

The first357/361 Java draft assumed omitted tool meant mainhand. Java correctly
returned no drop, rejecting that fixture expectation. All source/scripts/raws are
retained; the revised393/397 explicitly tests both omitted and actual mainhand.
No native defect count comes from the rejected draft. A unit compile draft used
the wrong map construction API; its compiler log is retained, not counted as an
old-failing regression. No named-omitted-tool or other route repair is included.

No origin/source/target context expansion, generic evaluator rewrite, H040/RNG
policy, H039 empty-slot, modifier, natural fishing/luck, other consumer, guest,
packet/UI or reopen claim.

## Results and residuals

Actual Game strict pair: **342/393 → 393/393**. The51 old
differences are16 `rod_rod`,17 `stick_stick`,17 `actual_mainhand`, and final
inventory. Every other strict record passes old and fixed. The full diagnostic
is **342/397 → 394/397**; the complete final chest tag now
matches too. No397/397 or whole-owner-body/raw-clock equality claim.

| Reference | SHA-256 |
| --- | --- |
| 393 raw records | `3cf050e40b50c99d24944e3c2237f9fb30533080d678ff98930e00484422ad8d` |
| 397 raw records | `700c42816a36afb1524e58043c0367ebdf11e6f6ad5c1fb6a798b35f29ff6e72` |

Every remaining full-diagnostic row follows. Complete expected/actual text and
callbacks are retained here; colored raw parts remain in the local JSONL/report.
The owner-body differences are outside TOOL and are not repaired or normalized.

<details>
<summary>initial-owner-body</summary>

Java expected:

```text
Player has the following entity data: {Brain: {memories: {}}, post_effects: [], SleepTimer: 0s, Invulnerable: 0b, FallFlying: 0b, PortalCooldown: 0, AbsorptionAmount: 0.0f, abilities: {invulnerable: 1b, mayfly: 1b, instabuild: 1b, walkSpeed: 0.1f, mayBuild: 1b, flying: 0b, flySpeed: 0.05f}, recipeBook: {recipes: ["minecraft:crafting_table"], toBeDisplayed: ["minecraft:crafting_table"]}, DeathTime: 0s, XpSeed: 0, XpTotal: 0, UUID: [I; -1608632253, -450807400, -1786055591, -2146546415], playerGameType: 1, SelectedItem: {count: 1, id: "minecraft:fishing_rod"}, seenCredits: 0b, Motion: [0.0d, 0.0d, 0.0d], Health: 20.0f, foodSaturationLevel: 5.0f, equipment: {}, fall_distance: 0.0d, Air: 300s, OnGround: 1b, Dimension: "minecraft:overworld", Rotation: [0.0f, 0.0f], XpLevel: 0, current_impulse_context_reset_grace_time: 0, warden_spawn_tracker: {warning_level: 0, ticks_since_last_warning: 325, cooldown_ticks: 0}, Score: 0, Pos: [8.5d, 200.0d, 8.5d], previousPlayerGameType: 0, Fire: 0s, XpP: 0.0f, EnderItems: [], attributes: [{id: "minecraft:entity_interaction_range", base: 3.0d}, {id: "minecraft:waypoint_transmit_range", base: 6.0E7d}, {id: "minecraft:block_interaction_range", base: 4.5d}], DataVersion: 5023, foodLevel: 20, foodExhaustionLevel: 0.0f, spawn_extra_particles_on_fall: 0b, HurtTime: 0s, SelectedItemSlot: 0, Inventory: [{count: 1, Slot: 0b, id: "minecraft:fishing_rod"}], foodTickTimer: 0}
callbacks=[[true, 1]]
```

Fixed Game actual:

```text
Player has the following entity data: {post_effects: [], SleepTimer: 0s, Invulnerable: 0b, FallFlying: 0b, PortalCooldown: 0, AbsorptionAmount: 0.0f, abilities: {instabuild: 1b, invulnerable: 1b, mayfly: 1b, walkSpeed: 0.1f, mayBuild: 1b, flying: 0b, flySpeed: 0.05f}, recipeBook: {recipes: ["minecraft:crafting_table"], toBeDisplayed: ["minecraft:crafting_table"]}, DeathTime: 0s, XpSeed: 0, XpTotal: 0, UUID: [I; -1608632253, -450807400, -1786055591, -2146546415], playerGameType: 1, SelectedItem: {count: 1, id: "minecraft:fishing_rod"}, seenCredits: 0b, Motion: [0.0d, 0.0d, 0.0d], Health: 20.0f, foodSaturationLevel: 5.0f, equipment: {}, fall_distance: 0.0d, Air: 300s, OnGround: 0b, Dimension: "minecraft:overworld", Rotation: [0.0f, 0.0f], XpLevel: 0, Score: 0, Pos: [8.5d, 200.0d, 8.5d], previousPlayerGameType: 0, Fire: -20s, EnderItems: [], XpP: 0.0f, attributes: [], DataVersion: 5023, foodExhaustionLevel: 0.0f, foodLevel: 20, HurtTime: 0s, SelectedItemSlot: 0, Inventory: [{count: 1, Slot: 0b, id: "minecraft:fishing_rod"}], foodTickTimer: 0}
callbacks=[[true, 1]]
```

</details>

<details>
<summary>clock-start</summary>

Java expected:

```text
The game time is 313 tick(s)
callbacks=[[true, 313]]
```

Fixed Game actual:

```text
The game time is 0 tick(s)
callbacks=[[true, 0]]
```

</details>

<details>
<summary>clock-end</summary>

Java expected:

```text
The game time is 313 tick(s)
callbacks=[[true, 313]]
```

Fixed Game actual:

```text
The game time is 0 tick(s)
callbacks=[[true, 0]]
```

</details>

Both real relative-clock deltas are0. The ledger infers the factory citation as
a whole `LootCommand.dropFishingLoot` port and flags source-entity/drop callees
not called by that factory. This helper only carries TOOL into the existing
evaluator; no whole-method/class or other-context closure is claimed.

## Local checks

Final production/harness:1105 Game/item tests and26 prepared-content tests pass;
formatting, workspace/server Clippy, doc tests and public-tree checks pass. Python
runs696 tests with four existing macOS tooling failures (ARM installer-fixture
assets and three Bash3.2 optional-archive cases), one skip. The narrow installer
diagnostic is retained. No packaging repair or unexecuted unrelated Rust failure
is folded into this report.

Final main checkpoint remains69fab597a, identical to the frozen base. No source
refresh or proof rerun was needed after unrelated updates. Later changes only
add these evidence notes; final public-tree/whitespace checks pass.
