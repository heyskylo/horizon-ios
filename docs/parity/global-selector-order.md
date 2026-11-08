# Unbounded arbitrary selector order: joined-family slice

H268 is **partial**. Issue #12199 covers only the measured global route; spatial
section ordering is unclosed. No result is compared as a set.

## Current defect and bounded repair

At base `b13dbf0687b545431172466ab4b91d19c3976405`, `Game::select` gathered
`level_candidates` by entity family, filtered, and then applied the limit.
Official 26.3 `EntityLookup` uses an insertion-ordered ID map. The existing native
join traversal was available but unused by selectors.

The new branch is exactly:

```rust
s.kind == 'e' && matches!(s.sort, Sort::Arbitrary) && !s.world_limited()
```

`world_limited()` includes distance, explicit position and delta options. This is
a conservative unbounded slice. The existing checked join traversal supplies
candidate order; a temporary occurrence-count map is never iterated for order.
Every nonwalk fallback occurrence is retained in its original relative order.
A failed join traversal returns the whole original candidate list, not a prefix.
Filtering and limiting remain in their existing order.

`level_candidates` itself and its native-diagnostic consumer are unchanged.
Bounded spatial, nearest, furthest, random, self, player selectors and direct
name/UUID routes keep their existing candidate paths. Player source context,
random draws, tick/save traversal, storage and mount policy are unchanged.

## Frozen actual command proof

One scene creates six UUID-bound actors in this chronology:

| Label | Actual kind | UUID final word | Position |
|---|---|---:|---|
| a | Pig |41|17.5,200,16.25|
| b | Armor stand |7|15.75,200,15.5|
| c | Item, stone count 1 |93|15.5,200,17|
| d | Pig |2|17,200,15.75|
| e | Armor stand |68|18.5,200,16.5|
| f | Item, stone count 1 |11|14,200,16.75|

The nonmonotonic UUIDs do not define order. Real summon receipts, fresh stores,
cumulative counts, actual identity/type/position/motion/NoGravity, living Health,
Pig NoAI, saved item state, loaded floor and exact relative clock 0 independently
admit the scene. No Health predicate is invented for item entities.

Actual `execute as ... data modify ... append from entity @s UUID` commands build
ordered UUID-array lists. Complete lists, actual indexed UUID elements, full
feedback/styles/callbacks and independent limit/count stores remain strict.

| Query | Fresh Java | Old Game | Fixed Game |
|---|---|---|---|
| Global arbitrary |a,b,c,d,e,f|a,d,c,f,b,e|a,b,c,d,e,f|
| Global limit2 |a,b|a,d|a,b|
| Global limit6 |a,b,c,d,e,f|a,d,c,f,b,e|a,b,c,d,e,f|
| Global exclude stand, limit3 |a,c,d|a,d,c|a,c,d|
| Spatial distance..3 |b,c,f,d,a,e|a,d,c,f,b,e|a,d,c,f,b,e|
| Spatial distance..3, limit2 |b,c|a,d|a,d|
| Nearest, distinct distances |b,d,c,a,f,e|b,d,c,a,f,e|b,d,c,a,f,e|
| Furthest, distinct distances |e,f,a,c,d,b|e,f,a,c,d,b|e,f,a,c,d,b|

SAME236 program bytes and one fresh Java reference are frozen. Global-slice
records improve **195/210→210/210**, closing 15 assigned list/index differences;
all 185 admission/control records pass. All 12 spatial rows remain strict/public:
**3/12 unchanged**, nine unclosed differences. Full comparison is
**206/236→221/236**; six predeclared pig-body/absolute-epoch differences remain.
All 26 native diagnostic JSONL lines (12 spatial + 14 raw body/epoch observations)
are byte-identical before/after. No masking or output normalization is applied.
`--strict-all` executes the entire retained comparison and remains red.

The actual Game regression uses registered summons and commands, verifies real
increasing join stamps and authoritative identities, and checks full/limited/
filtered ordered stored UUID arrays. Its original module is byte-identical:
meaningful old 1 failed / 0 ignored, 7.97s, fixed 1 passed / 0 ignored, 8.13s. The fixed normal
build took 1m27s. Immutable normal/unit binaries and hashes are retained locally.

## Reach and explicit limits

The shared join walk reaches Mob, Item, Boat, Cart, ArmorStand, TNT, FallingBlock,
Orb, Projectile, Rocket and Trident lists. Runtime claims cover only the admitted
Pig/Stand/Item actors above. Player and nonwalk families, including Cushion,
remain unproved; fallback membership is preserved, not advertised as Java global
order. H030 bounds, H135 visibility, H267 tags and H269-H271 predicate adapters
remain separate. No all-selector, cross-level runtime, packet, restart or
random-sort parity claim follows.

Official bounded lookup uses accessible sorted section keys and each section's
member order. Section-crossing movement removes and appends membership; global
lookup does not reorder on movement. Current join stamps do not model that
section lifecycle. No section model or move/reinsert runtime claim was added.
Native Stand/Item removal erases the entry and a fresh load gets a new join stamp;
Mob kill versus discard retains its existing policy. Taking/restoring lists keeps
existing stamps. This lifecycle audit is source evidence, not a re-add capture.

## Reproduction and retained evidence

Use `tools/test_selector_global_order_parity.py --reference-only --work-dir <new>`
for independent Java admission, then `--binary <immutable-game> --reference-dir
<original-reference> --work-dir <new>` for the pair. Every capture retains a
separate fresh Java result while comparison uses the original reference bytes.

Local `data/h268/evidence` retains full raw bodies/styles/orders, bytecode and
ownership audits, source/ref/ELF seals and phase times (unknown times are null).
The initial 236 Java draft used integer X/Z arguments and invalid centering
premises; it is retained invalid, with no native run for that draft. Explicit
finite decimals corrected only fixture inputs. The first unit stopped at a
multi-chunk floor prerequisite because the factory populated only one actual
World chunk; source/log/ELF are retained invalid. Only unit chunk setup was
corrected before the meaningful failure; no runtime/normal/ledger repeat was
needed. A resume disk preflight stopped before compilation. Only exact inactive
owned cache aliases were retired after archive/SHA/fuser checks; all proofs and
peer files remain intact. No pending production code was imported.
