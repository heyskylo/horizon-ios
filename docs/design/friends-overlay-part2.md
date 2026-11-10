# Friends overlay Part 2

> Status: proposed implementation spec. This note is based on `upstream/main` at
> `1453e0e614af4b2d04dd0d228a94162b5d459910` (2026-11-10). It is not a replacement
> for the approved Play with Friends migration spec, issue [#14167](https://github.com/horizon-rust/horizon/issues/14167).

Minecraft 26.3 has two different Friends features which must not be conflated:

- **Play with Friends** is the Horizon host/share feature. Its move out of the engine is
  specified by #14167 and is being delivered by #14334, #14380, #14405, #14411 and #14419.
- **The Friends overlay** is the account social screen: friends, incoming requests, outgoing
  requests, presence, profile actions and friend notifications. This note specifies that
  feature's Part 2 implementation.

The old implementation assembled both concepts in `game/friends.rs` and the old
`Screen::Friends` path. That coupling is the reason the existing overlay PR #14224 is too
large to review as one change. The overlay work below must be built on the new page and host
seams instead of rebasing that PR as a unit.

## Goals

The finished Part 2 overlay will match the observable Minecraft 26.3 behaviour for:

- the Friends and Requests tabs, including loading, empty, error and privacy/account states;
- adding a friend, accepting/declining incoming requests, cancelling outgoing requests and
  removing an existing friend;
- keyboard focus, tab navigation, Ctrl+number/Ctrl+Tab shortcuts, scrolling, clipping,
  tooltips, narration and the confirmation screens used by the vanilla flow;
- background list polling and action-result handling while the overlay is closed;
- friend and system notification timing, ordering, slot occupancy, F1 hiding and the
  Notification Time option;
- friend faces in list rows and notifications, including the default skin before a download
  completes and after a renderer/atlas recovery.

The screen remains a vanilla-looking overlay. It must not become an options-shell page merely
because the common `pages` renderer is used elsewhere. `Place::PauseFriends` belongs to the
Play with Friends host/share page in #14167; it is **not** the account Friends overlay and this
spec must not register the overlay there or replace that page. The overlay keeps its own
vanilla entry/action and centered 220-pixel panel, tabs, list clipping and the appropriate
background. Its exact pause/title registration seam must be agreed with maintainers against the
landed pages API before the UI integration PR; the title-screen icon that opens Join a Friend is
not repurposed by this spec.

## Non-goals and hard boundaries

This spec does **not**:

- move or redesign the Play with Friends host side;
- implement `ShareView`, `Hooks::share`, `Hooks::lan_closed`, `Hooks::control`, the
  `horizon-friends` host move, or the title Join a Friend page;
- delete `Screen::Friends`, delete `game/friends/screen.rs`, or change the host-side portions
  of `game/friends.rs` before #14167's page and crate migration has landed;
- add a second social HTTP client or duplicate the service/cache already in
  `crates/horizon-client-render/src/social.rs`;
- make a live account, screenshot, or local compilation claim in the absence of the required
  environment and evidence;
- carry the 4,090-line/21-file diff from #14224 into a new PR.

# Dependencies and sequencing

The implementation starts only after the relevant upstream seams are available on the base of
the PR:

1. **#14334** supplies the host-side `ShareView` and close event. It is independent of the
   social overlay, but it changes `game/friends.rs`, so overlay PRs must not edit the same host
   code while this step is in flight.
2. **#14380 then #14405** supply `Place::PauseFriends` and the transition from the old screen
   enum to the page/added-page path. The overlay's pause entry is blocked until this chain is
   merged. Do not preserve the old Play with Friends icon as a second Friends overlay entry.
3. **#14411/#14419** move control and host code out of the engine. Overlay work must use the
   resulting registration points and must not restore a `horizon-client -> horizon-friends`
   dependency.
4. **#14430/#14433** define the platform/crate language-file seam. Overlay-visible keys belong
   to the owning crate once that seam is available; no language-specific Rust branches and no
   hardcoded user-facing strings are allowed.
5. **#14439 and pages v2 (#14452 and its follow-ups)** determine the title-screen icon and
   text-field/page contract. The title integration is therefore a later PR, not part of the
   first pause overlay PR.

The order below is the recommended review order. Each PR is based on the previous merged
step (or on the exact dependency SHA named in its body), never on the abandoned #14224 head.

# PR 1: social backend and event contract

**Target size:** approximately 250–450 changed lines, including focused tests.

**Purpose:** make the existing social service observable by a later UI without coupling it to
an individual screen.

**Allowed scope:**

- `crates/horizon-client-render/src/social.rs` and its existing focused tests;
- small public API or fixture changes required to test the service contract.

**Behaviour:**

- The updater continues while the overlay is closed; foreground mode changes only the polling
  interval, not whether the worker exists.
- The first successful list response establishes a baseline and emits no friend-added or
  request event. Later successful responses emit bounded `received`, `sent`, `accepted` and
  `added` events by comparing the cached lists.
- 304 responses, empty responses, failed responses and malformed responses retain the last
  valid lists and do not invent events.
- A successful action schedules the same forced refresh as vanilla. A failed action exposes a
  result for the UI and requests a refresh where vanilla does; it must not clear user input
  before success.
- Presence changes update status data but do not reorder an already populated list. Resorting
  happens when the friend-id list is populated again.
- The event queue is bounded. Overflow drops the oldest friend event, never a system/action
  failure token.

**Not in this PR:** widgets, toast drawing, faces, title/pause integration, or any host/share
state. The PR must be reviewable as a service/data-contract change and must not import UI types
into `social.rs`.

**Proof:** social unit tests cover the baseline, 304/empty/failure retention, each list
transition, bounded overflow, action failure and presence ordering. Run formatting, the
affected crate check and clippy with warnings denied.

# PR 2: persistent friend notifications

**Target size:** approximately 350–500 changed lines. If the existing toast slot integration
cannot stay below the limit, split the slot allocator into a preceding tooling/data PR rather
than widening this one.

**Purpose:** implement friend notifications as client-owned state, so closing or changing the
Friends overlay does not destroy a notification.

**Allowed scope:**

- the existing client toast module and its focused tests;
- the client lifecycle/render seams that update and draw the client-owned queue;
- a small adapter from the PR 1 event contract to the queue.

**Behaviour:**

- System failures use the vanilla system-notification token and merge/update that token rather
  than creating unlimited copies. Friend events do not merge with one another.
- Waiting notifications do not age while all five vanilla 32-pixel slots are occupied. A toast
  receives a contiguous slot range before its 600 ms slide and five-second display timer start.
- Notification Time scales the display duration. F1 hides drawing but continues updating and
  expiring notifications. Existing achievement/tutorial/resource-pack/narrator/title
  occupancy is respected at the integration seam.
- The queue survives overlay close, pause/title transitions and entering a world. The first
  visibility transition is the only transition eligible for the vanilla toast sound; no new
  sound is invented by the Rust adapter.
- System text wraps at the vanilla width and its height is computed from the actual line count.
  Friend text uses the friend toast background and keeps the face inset even while the face is
  still loading.

**Not in this PR:** social HTTP changes beyond the adapter, list widgets, atlas allocation or
host-side `game/friends.rs` changes. The queue's public surface should accept already translated
text and an optional profile id, so it can later be registered by the official Horizon crate.

**Proof:** focused tests cover slot exhaustion/release, system-token reset, queue persistence,
F1, Notification Time and multi-line height. A real rendered check is required before calling
this player-visible PR accepted; code inspection or a blank/offscreen frame is not visual proof.

# PR 3: friend-face atlas and fallback

**Target size:** approximately 250–450 changed lines, including focused tests.

**Purpose:** give friend rows and friend notifications a private, recoverable face cache without
interfering with world-list or Realms face slots.

**Allowed scope:**

- the HUD atlas/face-loading seam and its focused tests;
- the face callback used by the client-owned friend toast queue and the overlay rows.

**Behaviour:**

- Friend faces use dedicated `horizon/friend-icon0` through `horizon/friend-icon31` slots.
  Row faces and notification faces have separate bounded ranges; over-capacity ids use the
  UUID-derived default skin for that frame instead of evicting another feature's texture.
- Atlas/resource rebuild clears friend residency and causes a safe re-upload on the next draw.
  A missing or unresolved skin draws the vanilla default player head and hat layers, not a black
  rectangle or a transparent hole.
- The resolver uses the current service/profile result and never hardcodes a stale URI. Face
  failures are visual fallbacks, not social-list failures.

**Not in this PR:** service polling, list sorting, title/pause entry, or toast timing. Keep the
NUL-bearing `hud.rs` safe: any binary edit must be a reviewed `rb`/`wb` patch with an anchor-count
assertion, and the helper must remain under `.local`.

**Proof:** focused tests cover row/notification range isolation, atlas rebuild recovery,
default-skin selection and over-capacity fallback. A rendered check must show a loading/default
face and a recovered face in both a row and a notification when the real resource environment is
available.

# PR 4: overlay UI integration

This is the only player-facing screen PR family. It is intentionally split again if the page
adapter plus the screen logic would exceed the project’s roughly 500-line rule.

## PR 4a: page shell and pause entry

**Target size:** at most approximately 500 changed lines.

After #14380/#14405 land, integrate the **account Friends overlay** through the entry/action
seam that maintainers approve for it. Do not register it at `Place::PauseFriends`: that place
belongs to the separate Play with Friends host/share page in #14167. Preserve the 26.3 overlay
surface and keep the account overlay's state separate from host/share state in `Game`. The
adapter owns only the approved entry identity, overlay lifecycle and input/draw forwarding; it
must not add `share`, `copy invite code`, `stop sharing` or other host-side actions.

## PR 4b: list and request behaviour

**Target size:** at most approximately 500 changed lines per PR; split the following into a
second UI PR if needed.

Move the overlay's independent state machine into the page/overlay module without touching the
Play with Friends host implementation:

- Friends and Requests tabs, loading/error/empty states and account/privacy confirmation;
- add/edit field, caret blink/scroll-to, send loading and failure retention;
- friend, incoming and outgoing entry actions with clipped hit testing;
- remove/accept/reject/cancel confirmation screens;
- focus path, Tab/Shift+Tab, arrows, Ctrl+number/Ctrl+Tab, Enter/Space, scrolling and tooltips;
- RTL-safe profile rows, profile-name copy and footer link activation;
- transfer notices to PR 2's client-owned queue before the page is destroyed.

The implementation may be a new `friends_overlay.rs` module, but it must not copy the old
4,090-line branch wholesale. Each PR body lists the exact files and the vanilla classes/methods
followed, and says which behavior is deliberately deferred.

**Not in the pause UI PRs:** title-screen Join a Friend, title icon resolver, Play with Friends
host/share, `horizon-ctl friends`, language-file platform design, or a second toast manager.
The title integration follows pages v2 and the approved #14167 step 4 work.

# Vanilla references and implementation evidence

Every implementation PR names the 26.3 classes and methods it follows, rather than citing an
older port. The relevant references include:

- `FriendsOverlayScreen`, `FriendsTab`, `PendingTab`, `AddFriendWidget`,
  `AbstractFriendsEntryContainerWidget`, `TabNavigationBar`, `PopupScreen`,
  `FriendsListConfirmScreen` and `Screen` for layout, input and lifecycle;
- `RemoteFriendListUpdateHandler`, `PlayerSocialManager.runAction/handleResult` and
  `MinecraftServicesFriendsService` for polling, cache and action results;
- `ToastManager`, `ToastInstance`, `SystemToast`, `FriendToast` and
  `PlayerFaceExtractor`/`PlayerSkinRenderCache.getOrDefault` for notification and face rules.

The source used for parity is the installed Minecraft 26.3 decompilation, not a previous Horizon
port. Decompiled files and game resources remain outside the repository under `.local`.

# Verification gate

Every implementation PR must report the exact source SHA, dirty state, command, target and exit
code. The minimum checks are:

```text
cargo fmt --all -- --check
cargo check -p <affected package(s)> --all-targets
cargo clippy -p <affected package(s)> --all-targets -- -D warnings
python3 tools/pr_ready.py --base upstream/main --quick
```

The social/service PR also runs its focused unit tests. The overlay and toast PRs require real
rendered/in-game screenshots at a comparable GUI scale; widget coordinates, compilation and a
blank offscreen frame are not visual acceptance. With maintainer resources and runners available,
run the affected vanilla harness (`python3 tools/parity.py friends_integration`) and report when
it is unavailable rather than marking it passed. No visible game window is started by automated
checks in this worktree.

Known follow-up gaps are tracked explicitly rather than silently folded into these PRs:
scrollbar dragging, horizontal scrolling of overlong status text, the complete narration matrix,
`friend_removed` and toast enter/exit sound parity, unification with the final global
`ToastManager`, and any live-account/relay behavior that cannot be exercised locally.

# Review and publication rule

This document is a proposed design note for maintainer review. Until it is accepted, do not open
implementation PRs for the overlay, do not close or rewrite #14224, and do not force-push its
branch. Once accepted, each implementation PR links this note (or its approved issue) and stays
within one numbered scope above. The old #14224 branch is historical behavior evidence only; it is
not an integration base.
