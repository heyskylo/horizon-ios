# Friends overlay Part 2

> Proposed spec, not approved. Updated after a direct local Minecraft 26.3 source review on
> 2026-11-11. Original docs branch base: `1453e0e614af4b2d04dd0d228a94162b5d459910`;
> latest inspected upstream reference: `1405044d0996d37f850f5ab0dec02bab43d4945f`.
> No implementation, game launch or visual acceptance is implied by this document.

This work is the **vanilla account Friends overlay**: account friends, incoming/outgoing
requests, presence and notifications. **Play with Friends** is Horizon's separate invite-code
host/share feature. Its approved migration spec [#14167](https://github.com/horizon-rust/horizon/issues/14167)
is not approval for this overlay.

PR [#14224](https://github.com/horizon-rust/horizon/pull/14224) contains the old combined
implementation (about 4,090 insertions across 21 files). Keep it intact as historical work, not
as the base for new implementation PRs. This proposal does not close, rewrite or supersede that
PR by itself. Do not copy its 2,215-line overlay module into one new PR.

The actionable PR list, estimates, module boundaries and acceptance cases are in
[the source-grounded split checklist](friends-overlay-part2-split.md). That checklist replaces
the earlier single, open-ended "list and request behaviour" PR scope.

## Ownership and entry decisions

Vanilla social service, screens, faces and toast behaviour belong to the **engine**
([layers](../layers.md#engine)), not to `horizon-friends` or `horizon-official`. They must work
without registering Horizon's host/share extension. Vanilla `gui.friends.*` and `options.*`
keys are not the Horizon-only `horizon.friends.*` language files being moved by #14430/#14433.
No engine dependency on a Horizon crate or new `allowed` exception is introduced.

The local vanilla references show that both `TitleScreen.init` and
`PauseScreen.createPauseMenu` call `CommonButtons.friends`, then
`OnlineOptionsScreen.confirmFriendsListEnabled`, then `FriendsOverlayScreen(parent)`.
However, #14167's migration uses those existing Horizon icon slots for Play with Friends and
Join a Friend. **Maintainers must agree how to preserve the vanilla account entry while keeping
those extension entries accessible.** This is a UI integration decision, not a reason to move
the account overlay into the Horizon layer.

Until that decision and the relevant migration steps land:

- Do not replace the host/share `Place::PauseFriends` page with the account overlay.
- Do not silently invent a fifth vanilla pause icon, duplicate an entry, or displace the title
  Join a Friend page. A new extension location requires maintainer agreement.
- Do not implement the account tab/list UI as generic options-page rows; the current main
  `Page`/`Row` API is not a tabbed, scrollable overlay/focus contract.
- Do not edit host/share state or commands in `game/friends.rs`/`game/control.rs`.
- Treat #14334 -> #14380 -> #14405 -> #14411/#14419 and title-side #14439/#14452/#14470 as
  integration dependencies, not as already merged capabilities. Requery before implementation.

Backend, widget and rendering preparations can be designed separately. Their PRs still require
approval of this spec; no public partially implemented account overlay is enabled along the way.

## Observable contract from the local 26.3 sources

Paths below are relative to the decompiled `net/minecraft/client/` root. They are references,
not copied source. Line numbers identify this reviewed local snapshot and may shift on a fresh
decompilation.

| Area | Required behaviour | Reference |
| --- | --- | --- |
| Buttons | Active for non-demo, non-offline-developer mode; this is not simply "has an unexpired session". Account `friendsEnabled()` and list-enabled state are separate gates. | `gui/screens/TitleScreen.java:123-140`; `gui/screens/PauseScreen.java:126-131`; `Minecraft.java:2267-2269` |
| Notification badge | Refresh incoming-request count; no badge when the list is disabled. Use notification/1..5, and more for >=6; draw only when active. | `gui/components/FriendsButton.java:25-60`; `TitleScreen.java:78-85`; `PauseScreen.java:249-271` |
| Global shortcut | Reuse the rebindable Friends action. Toggle from title, pause, no screen, or the current overlay; reject unrelated or input-captured screens and the debug-modifier path. Closing restores the actual parent, including no screen. | `Minecraft.java:2271-2285,2299-2315`; `gui/screens/Screen.java:186-203` |
| Entry consent | When disabled, Turn On sends true/true; only a successful settings result updates local flags and opens the overlay. Turn Off returns to the actual previous screen. Do not substitute the options-toggle code path. | `gui/screens/options/OnlineOptionsScreen.java:53-85,121-149` |
| Lifecycle | Add/remove list listener; clear background focus; init/resize the parent; restore it on outside click or Escape. Try updating presence while the overlay ticks. | `gui/screens/friends/FriendsOverlayScreen.java:65-107,115-143,169-191` |
| Layout | 220-wide content, 110x20 tabs 7 pixels above it, maximum content height h-80, 8-pixel background border. Draw parent with no hover, then blur, then overlay. Height follows the active content layout, not an always-full-height panel. | `FriendsOverlayScreen.java:88-154,223-226` |
| List state | LOADING/error apply to both tabs. Populate on successful list refresh; sort friends by presence then. Later presence updates change text, not order. A fresh populate may reorder even if the IDs stayed the same. | `FriendsOverlayScreen.java:193-242,248-284`; `FriendsTab.applyPresenceUpdate` |
| Action lifecycle | Track pending removals by UUID across repopulation; disable both tabs while an action runs, refresh after completion, and distinguish ordinary result failure from exceptional-future failure. | `FriendsOverlayScreen.java:260-276,295-355`; `PlayerSocialManager.runAction/handleResult` |
| Polling/deltas | Default interval 60 seconds, background x5, worker checks each second. If previous state was not SUCCESS, re-establish a silent baseline, including after an error. Deltas compare FriendDto values, not only IDs. | `gui/screens/social/RemoteFriendListUpdateHandler.java:81-85,152-198,227-234` |
| Notification gate | World notification preference suppresses friend-toast display, not advancing known lists. Specific action failures use one system token; overlay exceptional failures have a separate generic-error path. | `RemoteFriendListUpdateHandler.java:152-198`; `gui/screens/social/PlayerSocialManager.java:160-194`; `FriendsOverlayScreen.java:349-355` |
| Toast lifecycle | One client-owned lifecycle across screens. Update visible entries, release slots, then assign waiting entries; waiting entries do not age. Five contiguous 32-pixel occupancy slots; 600ms slide, 5 fully-visible seconds scaled by Notification Time; F1 hides drawing only. | `gui/components/toasts/ToastManager.java:38-113`; `FriendToast.update`; `ToastManager.ToastInstance` |
| Toast coordinates/sound | Occupancy is ceil(height/32), but Toast.yPos uses firstSlotIndex * toast.height(). First visible toast changing visibility in each update receives in/out sound eligibility; custom getSoundEvent uses a separate dedup path. No friend-specific sound priority is implied. | `gui/components/toasts/Toast.java:35-48`; `ToastManager.java:38-77` |
| System token | addOrUpdate searches visible and waiting entries. reset marks text changed and restarts lastChanged at the next update, not the slide. | `gui/components/toasts/SystemToast.java:67-75,97-100,123-130` |
| Face fallback | Resolve asynchronously; before completion or when unavailable, use the partial profile/default skin and draw head plus hat. | `gui/components/PlayerFaceExtractor`; `renderer/PlayerSkinRenderCache.getOrDefault` |

### Policies that must not be presented as vanilla

- Vanilla ToastManager's waiting deque has no demonstrated 64-entry limit. The old Rust
  64-event/64-toast bounds are implementation policies, not vanilla rules. The old event
  pop-front path can discard action failures, contradicting the previous spec's promise.
  No lossy bound is approved here; any proposed resource bound must describe its visible
  effects and be reviewed explicitly.
- The 32 dedicated face slots, split into 24 row and 8 notification slots, are an old Rust
  atlas strategy, not vanilla's skin-cache capacity. Default rendering and non-interference
  are requirements; the exact capacity/allocation design is reviewable separately.
- UUID-only sort-cache invalidation is not the populate/presence distinction above.
- A shared occupancy bitmap among separate toast queues is not proof of global ordering,
  token lookup or sound arbitration. Do not call that a complete ToastManager port.
- HTTP 304, empty-body, ETag and cooldown semantics require the matching authlib 10.0.77
  service source/bytecode. This round read the client handler, not that service implementation;
  retain existing semantics and do not claim fresh HTTP-cache parity evidence.

## Implementation structure and size gates

Prefer small, cohesive files under `crates/horizon-client/src/friends_overlay/`:
`mod.rs`, `model.rs`, `layout.rs`, `shell.rs`, `add.rs`, `entries.rs`, `requests.rs`,
`links.rs`, `focus.rs` and `faces.rs`. These are proposed responsibility boundaries, not a
requirement to create every file. Keep social data in `horizon-client-render/src/social.rs`;
use a sibling toast module for account notifications rather than embedding it in screen state.

The split checklist names explicit backend/notification/face steps and **eight UI work
packages**, then separate pause/title/global-shortcut integrations. No "remaining core UI"
catch-all PR is allowed. Each package must name its production consumer, dependencies and
non-goals. Preparatory stacked steps may be internally exercised without enabling the public
entry, but must build and be reviewable; no dead-code allowances or advertised partial parity.

Budgets are estimates from old implementation responsibilities, not measured candidate diffs.
Before opening each PR measure additions, deletions, non-test code and hook/glue changes on its
actual base. New code plus integration must remain around 500 lines; if not, return to the spec
with a narrower independently reviewable behaviour. File partitioning alone is not a split.

`title.rs`, `game.rs` and `hud.rs` must not grow when already over 3000 lines. Where no small
existing seam exists, first propose a **separate pure-move PR** with before/after equivalence;
never hide a relocation inside a parity fix. Keep `hud.rs`'s legitimate NUL bytes intact using
Read-before-edit and an anchored binary patch under `.local` if it is eventually modified.

Do not put vanilla behaviour behind the Horizon host/share feature or move vanilla translation
keys into `horizon-friends`. No new machine translations or "reviewed" language flags.

## Validation and evidence

Use the current manifests to choose test targets. On the inspected main,
`horizon-client/src/lib.rs` declares gameplay and `main.rs` is a thin executable. Therefore
`cargo test -p horizon-client --bin horizon-client ...` matching zero tests does not validate
pause routing. Verify a nonzero match count for the actual library/module filter.

For each implementation PR:

- Run formatting, affected-package checks, clippy with warnings denied, focused behavioural
  tests and the current `tools/pr_ready.py --base upstream/main` procedure. Disclose `--quick`
  or missing vanilla artifacts, raw failures and exact source/base SHA. Do not change existing
  tests or declare platform failures baseline failures without a clean-baseline run.
- Use WSL/Linux for Unix-dependent tests if needed, with a separate Linux target directory.
  No visible game window or fast executable is built merely to prepare this checklist.
- Test state transitions with independent expected outcomes. Temporary probes stay under
  `.local`; preserve existing tests. Before publishing code, resolve any required permanent
  harness additions against the user's temporary-test submission preference, not by omitting
  coverage silently.
- Real screenshots at comparable viewport/GUI scale are required for visible changes. Draw
  coordinates, source review and blank offscreen images are not visual acceptance. Live-account
  operations remain unverified until tested in the appropriate environment.
- `friends_integration` currently tests Play with Friends LAN/code/direct/relay behaviour,
  including host and guest. It is an overlap regression guard, **not** an account Friends
  service/UI/toast/face parity oracle. The checklist gives separate acceptance cases; any
  automated account-social oracle or fixture must be scoped and identified explicitly.

Source-review evidence is not runtime proof. The local port-ledger query returned "No ledger
yet"; no ledger check or bytecode oracle pass is claimed. Decompiled source, jars, hashes/logs
and temporary scripts stay outside the tracked tree.

## Maintainer decisions and publication

Before public UI integration, resolve: the vanilla account icon vs #14167 extension entries;
the minimal engine integration seam; any required pure-move prerequisites; and permanent
regression coverage. Generic pages v2 availability does not by itself resolve these questions.

This proposal and the split checklist need maintainer approval. Do not open the implementation
series merely because docs CI passed or `run-parity` was added. Do not close #14224, replace its
history, push the isolated merge result, or change another contributor's branch as part of
preparing the plan. Every later PR links the accepted spec and its exact work-package ID.
