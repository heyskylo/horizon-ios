# Transient client update fields

A mod block entity can own a batch of fields that belongs in the next client
update, but not in its saved contents. Examples include the items used by a
processing particle effect. Repeated block-change notifications may coalesce;
the batch is consumed when the update tag is constructed, not when an update
is requested.

`ModBlockEntity` provides three methods:

- `set_client_update_fields(&mut self, Compound)` replaces the pending batch.
- `clear_client_update_fields(&mut self)` discards it.
- `take_client_update_fields(&self) -> Compound` atomically drains it.

The block entity's owner explicitly extends its API 45
`native_block_entity_update_tag` result with the drained fields. The engine does
not add them automatically. If clients need an explicit empty value on a later
update, the owner supplies that default in its tag. Setting fields does not
schedule an update: use the ordinary block-entity synchronization path for that.

The private `OnceLock<Mutex<Compound>>` is not initialized by an idle entity,
an empty replacement, a clear, or a take. It is safe to consume through the
shared reference available to API 45, including saved chunks encoded by worker
threads. A poisoned lock is recovered with its pending data intact. The fields
are not part of `ModBlockEntityFields`: they are neither saved nor included in
equality, debug output, or revision changes. A clone is a different entity and
starts with no pending batch; a save does not consume the live entity's batch,
and cold decoding starts empty.

Loaded chunks use `ChunkEncoder::body_in` and the live entity map. The block
broadcast path constructs each entity's tag once and sends the encoded packet
to all viewers; an update with no viewers does not construct or consume a tag.
Saved chunks beyond view distance are loaded independently and do not inherit
a live entity's pending effects. These existing boundaries are unchanged.

Validation covers replacement and cancellation, concurrent drains, poison
recovery, clone identity, cold save/decode, loaded-chunk encoding, and a Hub
broadcast to two viewers after a skipped broadcast with no viewers. The tests
exercise the ordinary encoder and broadcaster; the API does not introduce an
additional packet or timing model.

Run the focused tests and strict checks from the workspace root:

```sh
cargo test -p horizon-client-render --lib client_update
cargo test -p horizon-client-lib --lib transient_client
cargo clippy -p horizon-client-render -p horizon-client-lib --all-targets -- -D warnings
```

The first command runs three entity tests; the second runs three save/chunk/Hub
tests. They need no prepared game resources. The Hub test opens an ephemeral
loopback listener. The reusable crate is `horizon-client-lib`, whose source lives
under `crates/horizon-client/src`; selecting the startup `horizon-client` package
does not run these reusable tests.
