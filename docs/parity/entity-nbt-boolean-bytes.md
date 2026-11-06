# Entity NBT boolean bytes

H045 changes only `entity_nbt::flag`: use the existing NBT `int_value` conversion
and then keep the low byte, comparing that byte with zero. The public function
signature, generic `number()`, boxed-number helpers, field writers, species
getters and pickup/AI policies are unchanged.

Pinned Java 26.3 `TagValueInput.getBooleanOr` calls `NumericTag.byteValue`.
Integral tags keep their low byte. FloatTag/DoubleTag call `Mth.floor`, mask 255
and narrow to Byte. This version's `Mth.floor` is `(int)Math.floor(value)`, with
saturating casts, matching existing `nbt_path::int_value`. Narrowing a Long to
Int before Byte preserves its low bits, including values above Double's exact
integer precision. The boxed-number conversion in `datafix::dynamic` truncates
floating fractions and is a different boundary.

## Frozen proof and build prerequisite

Base: `d3f799f5d51df0e18f57bf5bb35b01c320b18fdb`.
Unchanged d3 cannot build: existing raid and four older synthetic unit calls
omit the newly required fifth `mob_to_nbt` argument. Those E0061 logs remain
rejected build prerequisites, not parity failures.

Both actual Game builds use the same authorized five-call compatibility patch,
SHA-256 `d43a4f3bd9e844f286c861ce28072ce72da085c3a83e1eb041b94e71ce4f7aab`:
the raid observer uses existing actual `Persist::uuid_of`; two SpellTicks,
animal-integer and pickup synthetic tests use empty resolvers because they have
no referenced live identities. No assertions/model values, UUID policy or raid
behavior changed. PR #10544 subsequently landed all five repairs. The published
branch retains those upstream repairs without duplicate compatibility changes.
The frozen common patch remains byte-identical after the converter edit; no
runtime pair is claimed against the later publication base.

## Live command contract

Driver SHA-256: `73968da787ed30e7178b4bb080cdc1f203692c4c47c45bfc3724ad33cf88f1c1`.
Fresh Java reference SHA-256:
`ac676ebe38788a43c6eb1f72ff4744e53a63d0961417898434f71e806a98804c`.

`tools/test_entity_nbt_boolean_parity.py` uses the existing actual Game command
harness and fresh pinned Java. It declares 26 inputs before capture: absence;
Byte 0/1/-1; Short/Int/Long wrapping; a Long above 2^53; finite Float/Double
fraction signs, wrap boundaries and overflow; numeric/non-numeric strings and
a compound. Every case creates a distinct typed UUID-bound pig with Health 10,
exact position, canonical NoGravity/persistence, a loaded stone floor and zero
observation ticks. Complete body and absolute-clock traces remain raw.

All 274 declared records compare as complete raw JSON (messages/styles,
callbacks and fresh stores), without masks/tolerances. NoAI false omission and
pickup false's always-written Byte 0 are separate strict observations. Actual
identity/health/position, load/geometry and zero relative-clock admissions pass.
No arbitrary AI/pickup behavior or whole-body/raw-clock equality is claimed.

The baseline with the common compatibility patch matches 196/274; fixed Game
matches 274/274 with the same script and fresh Java reference bytes. Its 78
residuals belong to 13 numeric-input cases, each with 6 affected observations;
they are not 78 independent defects. Default, canonical Byte, nonnumeric and
positive/wrapping controls remain executable and raw.

The resource-free regression calls actual Mob decode and save, checks modeled
NoAI/pickup values and both no-raw and retained-raw encoding. It keeps NoAI's
false omission, pickup's explicit false Byte, and unrelated raw data. Its old
assertion fails at Short 256 (true versus required false).

## Caller audit at the frozen base

There are 71 in-file calls and 6 qualified external calls (excluding the separate
fox bitflag helper). These are entity NBT booleans, not item-component BOOL or
boxed-number consumers. Live runtime coverage is bounded to the pig fields
above; the audit is not an all-species/lifecycle parity claim.

| Caller group | Existing model/boundary | Runtime coverage |
| --- | --- | --- |
| Mob loader | NoAI, gravity, visibility, persistence, pickup and species flags | Pig NoAI/pickup matrix; other fields/callers untested live |
| Entity-specific loaders | Item/orb, projectile/arrow/trident, frame/stand flags | Source audit only; their raw/default policies remain unchanged |
| Game persistence/minecart/data writes | Loaded NoGravity flags | Source audit only |
| Unmodeled command damage | Saved Invulnerable flag | Source audit only |
| Nautilus loader | Sitting flag | Source audit only |

Existing Allay/SulfurCube pickup getter overrides are unchanged. AgeLocked,
from-bucket, tame and other species logic is not modified. Canonical numeric
Byte 0/1/-1 and absent/nonnumeric behavior are retained by the conversion.

Run with prepared resources, fresh output directories and an explicit binary:

```sh
python3 tools/test_entity_nbt_boolean_parity.py --reference-only \
  --work-dir .local/entity-boolean-reference
python3 tools/test_entity_nbt_boolean_parity.py --binary /path/to/horizon-client \
  --work-dir .local/entity-boolean-native --reference .local/entity-boolean-reference/vanilla.jsonl
```

Each native invocation reruns Java and requires the exact immutable reference
bytes. Complete programs, reference/native records, raw traces, rejected builds
and common-patch manifests remain local evidence; no observer was added.
