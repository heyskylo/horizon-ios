# Loading `/data` into an existing mod block entity

Changing saved data with `/data merge`, `/data modify` or `/data remove` must
not construct another live mod block entity when its type ID is unchanged.
The command now moves the normally decoded saved compound and item slots into
the existing entity. Its runtime menu data and `ModRevision.same_entity`
identity survive; its content revision changes through ordinary `DerefMut`.
Removing a saved key or inventory replaces that saved state rather than merging
it back from the previous entity. Ordinary dirty marking and client-update
logging still run after the load.

The reference is Minecraft 26.3 `BlockDataAccessor.setData(CompoundTag)`: the
actual client JAR calls `loadWithComponents` on its existing `entity` field,
then `setChanged` and `Level.sendBlockUpdated` with flags 3. No new constructor
or static block-entity load occurs in that command method. This change is scoped
to same-ID mod entities. Existing type-changing behavior, block/type validation,
new-entity creation and persistence decoding remain unchanged; a different ID
or chunk reload still gets a fresh live identity.

The ignored prepared-resource regression
`actual_data_commands_preserve_same_mod_entity_runtime_state` builds a generic
content block and runs real `Game::run_command` operations. It checks:

- Same entity identity, changed content revision, unchanged runtime data and its
  allocation, dirty chunk and queued normal block update.
- Replacement item slots, count and opaque item components through normal NBT
  parsing, with nested typed unknown NBT retained.
- Removal of a nested field, its parent compound, canonical inventory and
  non-inventory opaque `Items`, without resurrection.
- Different-ID replacement and saved/chunk-loaded data have fresh identity and
  no retained runtime data.

Run with private prepared resources and private homes. For example, after setting
`HORIZON_TEST_RESOURCES`, `HORIZON_HOME`, `HORIZON_LAUNCHER_HOME`, `XDG_DATA_HOME`,
`XDG_CONFIG_HOME`, `HORIZON_NO_AUDIO=1` and
`HORIZON_LAUNCHER_NO_SELF_UPDATE=1`:

```sh
cargo test --offline -p horizon-client --lib \
  actual_data_commands_preserve_same_mod_entity_runtime_state \
  -- --ignored --nocapture
cargo clippy --offline -p horizon-client --lib --tests -- -D warnings
cargo fmt --all -- --check
python3 tools/check_public_tree.py
git diff --check
```

This is an actual Game command test without a window, native mod bundle,
third-party decoder or installed-world mutation. It does not claim a new wire
protocol, a live multiplayer run or a general serializer-parity expansion.

Validation on the runtime `31c539b25` base: the actual Game regression passed;
full `horizon-client` library/test Clippy passed with warnings denied and no lint
exceptions. Workspace formatting, public-tree check, diff check and the port
ledger comparison against that base also passed.
