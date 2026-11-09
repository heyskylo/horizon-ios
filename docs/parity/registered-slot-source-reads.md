# Registered slot-source condition reads (partial)

Part of #13091. One loaded `parity:first_three` definition is admitted through
`minecraft:slot_source` and read by `execute if/unless items/slots`. Its JSON is:

```json
{"type":"minecraft:limit_slots","limit":3,"slot_source":{"type":"minecraft:slot_range","slots":"container.*"}}
```

The existing pack load pass snapshots definitions into `DataStore.slot_sources`.
Candidate function admission and current command registries publish their IDs.
Condition reads normalize identifiers, preserve physical literal range priority,
and evaluate the snapshot through unchanged `inline_slots`. No query reads disk.
Pack order, replacement on reload and skipping invalid JSON follow the existing
load policy. `command_slots`, `command_slot_candidates` and H247/H308 helper
algebra remain unchanged.

The compact fixture uses one enabled pack and a loaded chest with stone5, dirt2,
stone7 and shulker1 (whose nested state is preserved). Named and equivalent inline
sources select three slots, five matching stone items and seven total items.
Physical container controls distinguish fifteen items from twenty-seven slots.
Result/success sentinels, unless complements, missing-ID parse rejection, complete
chest state and a zero-tick observation clock stay strict. The historical 627
checks, 462 rows and 21 replies are separate evidence, not current native proof.

The actual Game regression creates a real pack and chest, verifies the loaded
pack independently, and exercises the real command/callback path. Run it with
prepared resources, isolated homes and a scratch working directory, using the
`registered_slot_source_read_tests` test filter and `--include-ignored`.
Post-baseline native Game controls cover loaded snapshot/reload, pack precedence
and invalid higher-priority JSON. They do not add Java runtime claims.

Shared `ArgType::SlotSource` publication also admits IDs in `/item` arguments.
Those registered consumers remain unsupported: modify/replace/fill/override
target arms select no physical candidates and error before writing; `from` source
arms select nothing and error before target writes. The block-command preservation
control requires parse admission followed by failure and an unchanged complete
chest and clock. No registered write adapter or Player fallback is added.
Entity-slot runtime behavior, registered tags, explicit contexts, contents
adapters, component writes, all registry schemas and lifecycle parity are unclaimed.
