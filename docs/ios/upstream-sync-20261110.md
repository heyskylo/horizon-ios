# Mobile and upstream integration — 2026-11-10

This integration combines the current mobile fork at `8001edc0b45e1eb5891685e32af871e8ccddf211`,
the saved build 98 changes at `d6625b68d9`, and upstream engine commit
`1b27a37702116f54150048008d8b466021307e53`, followed by the final GitHub refresh at
`5070174aef83be9e9f6653ad8a765f4fbe4b4369` (144 more commits). In total, 2,466
upstream commits are incorporated since the branches diverged. The initial merge had 69
conflicted paths; the final refresh merged without textual conflicts.

## Resolution policy

Compare behavior before choosing an implementation. Use the newer implementation when it
supersedes the same behavior; retain and adapt independent functionality to the new architecture.
The merge includes the full upstream engine update, rather than a selection of mobile fixes.

- Retain the native launcher, Amethyst-style touch controls, joystick appearance and behavior,
  editable locations, gesture handling and input ownership from the current mobile build.
  Their native Swift sources and touch dispatch implementation are unchanged by this merge.
  The older checkout's in-game layout editor is superseded by the current native editor;
  see [touch layout](touch-layout.md). Old source files are not copied over their replacements.
- Move mobile resource-loader ownership and shutdown behavior into upstream's new modules.
  Keep immutable resource generations and leases, cancellation and owned worker retirement.
  Combine these with upstream's bounded skin downloads and resource ordering.
- Use upstream's cosmetics hook/seat architecture while preserving owned requests,
  catalogue state, retry behavior, emote-wheel input, visibility clearing and world cleanup.
  Retiring one seat stops its own workers, not another player's service.
- Use upstream's renderer and frame structures while retaining the mobile stack reduction,
  pipeline ownership and local terrain metadata. Preserve native options and focused-field
  keyboard behavior alongside the new settings shell and radial controller capture.
- Combine bundled native content discovery with the fork's immutable catalogue and pinned
  content leases. Built-in content is enabled by default unless disabled; ordinary mods keep
  their explicit enabled state.
- Keep nonblocking recording shutdown and worker ownership through upstream's recorder trait.
  Keep touch/controller modifier ownership when reconciling hardware keyboard reports.
- Package `horizon-official` on iOS, matching upstream's registered release entry point.
  Forward audio/extras features through `horizon-game` without enabling the desktop controller
  backend. The installed executable is still named `Horizon` and uses the native launcher.
- Repair the final upstream batch's model-loader variable shadowing (a layer-name vector
  shadowed the map that records model ownership), and adapt its new item spill fixture to the
  mobile fork's shared motion resources.
- Retain mobile release routing and workflows. Do not restore the superseded desktop CI/release
  workflows that the mobile fork had deliberately removed.

The layer registry now includes the fork's existing Apple adapters. DNS-SD is a leaf system
resolver binding used by the portable protocol implementation on iOS. Two existing fork
transport dependencies (`horizon-client-lib` to cosmetics cancellation/TLS and friends network
interface discovery) are recorded as migration exceptions; no new production dependency is
introduced for those entries. They remain architectural debt to extract into shared transport
APIs. An unused input-to-DualSense dependency was removed. Release hook registration uses the
upstream layer boundary instead of adding Horizon services back to the engine entry point.

## Verification

Validation runs use scratch homes and game directories, not player saves.

- 190 input crate tests passed.
- 38 targeted game tests passed: cosmetics state/cleanup, pause navigation, content catalogue,
  pinned mod loading and split-screen profile behavior.
- 15 model/death-message tests passed after the final upstream refresh; one prepared-model
  fixture remains ignored. The 16 language-coverage tool tests passed.
- 26 renderer tests passed, with one existing ignored case: bounded downloads, cache generations,
  leases/pruning and mobile settings focus.
- 25 mobile regression probes passed, covering touch gestures, toggles, bindings, handoffs, modifiers, mouse
  ownership, virtual cursor, keyboard focus, native editor and lifecycle behavior.
- 23 iOS packaging/controller tests and 4 world-input capture tests passed against the merged source.
- The iOS release entry point is checked for `aarch64-apple-ios` with `audio,horizon-extras` and
  no default desktop features. Host game/client-library/renderer test targets are checked too.
- Public-tree and layer checks passed. Whitespace checks passed excluding upstream port-map TSV
  rows whose trailing tabs represent empty final fields.

These are source, compile and automated regression results. This integration does not establish
physical-device gameplay, visual parity, Microsoft sign-in or multiplayer runtime equivalence.
No Minecraft Java installation is needed for the controls checks. Prepared-resource gameplay
fixtures remain separate from this verification.
