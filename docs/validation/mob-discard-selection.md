# Immediate discarded-mob visibility

Discarded native mobs now leave command enumeration and ID/UUID lookup
immediately, even while `/tick freeze` keeps their list entries resident.
Passenger save assembly also excludes a discarded living passenger. The
existing root save guard, death-time selector rule, health, damage and death
loot paths are unchanged. No new API or mod-specific condition is introduced.

The source chain is Minecraft26.3 `Entity.discard` → `Entity.remove` →
`Entity.setRemoved` → `PersistentEntitySectionManager.Callback.onRemove`.
That callback synchronously removes section membership and tracked ID/UUID
lookup. `Entity.shouldBeSaved` rejects the discarded removal reason; that
policy also applies when a passenger is written.

The regression uses an actual prepared-resource Game, the normal API37
`native_remove_mob`, actual `/tick freeze`, parsed selectors, ordinary
`execute unless entity` commands (including a UUID target), and normal
`save_every_entity` chunk-save assembly. Positive checks precede removal; no
entity/world tick intervenes. It also checks wrong-dimension rejection, repeated
removal, no lethal damage/loot/XP, retained UUID visibility for a dying but
nonremoved mob, immediate UUID reuse, and exclusion of a discarded passenger
from an otherwise savable vehicle.

`tools/test_mob_discard_parity.py` compares eight observable fields with both
actual client and server26.3 JARs. The Java fixture executes real
`Entity.discard`, persistent-manager registration/lookup/enumeration and save
assembly. It uses a Marker with explicitly initialized ordinary fields, a
controlled storage implementation and callbacks; it does not construct a full
ServerLevel, replace removal methods or tick entities. The Game regression
uses ordinary living Blaze entries. No rendered, acoustic, LAN or full-server
process proof is inferred.

Run in a separate target with prepared resources and JDK25:

```sh
python3 tools/test_mob_discard_parity.py \
  --game-dir /private/path/to/minecraft/26.3 \
  --resources /private/path/to/prepared/26.3
cargo clippy --offline --locked -p horizon-client --lib --tests -- -D warnings
cargo fmt --all -- --check
python3 tools/check_public_tree.py
git diff --check
```

The harness gives Game fresh private homes and writes its logs, compared
outputs, source/JAR hashes and report under `.local/rust-mob-discard-parity`.
Nothing from those proprietary/private inputs is committed.

Remaining boundary: `native_remove_mob` still marks the list entry removed;
normal sweep, leave notifications and tracking cleanup retain their existing
scheduling. This change does not implement synchronous `stopRiding`/passenger
detachment, seat updates or the complete lifecycle of live riders when their
vehicle is discarded. The new save guard prevents a discarded passenger from
being serialized regardless of that transient mount state.

Validation passed: the actual Game regression, all eight compared fields on
both real26.3 JARs, full `horizon-client` library/test Clippy with warnings
denied and no exceptions, workspace formatting, public-tree and diff checks.
