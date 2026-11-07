# Native block clone items (pending API)

`Game::native_block_clone_item(block, pos, state, &mut stack)` supplies the
equivalent of a block's `getCloneItemStack` before the ordinary Ctrl+Pick path
copies block-entity components. The callback receives the actual state and
position. The first supplied stack owns the result; no response preserves the
existing vanilla clone logic. Empty and unregistered replacements produce no
picked item. A replacement can represent a block with no primary block item.

The supplied stack's components survive. Ctrl+Pick adds the ordinary canonical
block-entity component patch afterward, including native mod payloads; it does
not return early from that path. This differs from the existing cancellable
player-pick event, which controls whether the final result enters the hotbar.

This extension has no global API number. Its generated hook fragment is stamped
after an upstream merge.

## Validation

The focused generated-dispatch regression passes, including first-response
ownership. The explicit prepared-resource `actual_state_factory_lifecycle` Game
fixture passes all nine groups. Its clone group checks actual state-dependent
items, a block with `item: false`, prototype components, Ctrl+Pick native entity
data, explicit empty/unknown refusal and unchanged vanilla fallback. The earlier
eight groups retain their placement, command, piston and synchronization checks.

Run the dispatch case with `cargo test -p horizon-mod-api-gen-tests
block_clone_item_tests`. The ignored Game fixture requires its own process and
private game/launcher/XDG homes, `HORIZON_TEST_RESOURCES=<prepared-26.3-root>`
and a local-only `HORIZON_API_DISCOVERY=http://127.0.0.1:9/` endpoint; run
`cargo test -p horizon-client --lib --no-default-features
actual_state_factory_lifecycle -- --ignored --nocapture`.

Strict no-default-features Clippy passes all targets of `horizon-client`,
`horizon-client-lib`, `horizon-mod-api-gen` and `horizon-mod-api-gen-tests`.
The final tests and Clippy used an unchanged source snapshot. These focused
checks do not claim a full `pr_ready` run, Java input comparison or UI parity.
