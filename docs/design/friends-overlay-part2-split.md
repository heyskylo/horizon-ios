# Friends overlay Part 2: source-grounded split checklist

Companion to [the proposed spec](friends-overlay-part2.md). **Local proposal, not maintainer
approval or implementation.** Reviewed against local 26.3 sources on 2026-11-11; latest fetched
main was `1405044d0996d37f850f5ab0dec02bab43d4945f`. Requery integration dependencies before
starting code. This checklist does not authorize changing #14224 or another contributor's PR.

## How to use this list

- `V/` means the local decompiled `net/minecraft/client/` reference tree. Names and method/line
  anchors below locate evidence; do not commit that tree or copy Java source into the PR.
- `R/` means `crates/horizon-client/src/`; `S` means
  `crates/horizon-client-render/src/social.rs`. Old Rust line ranges refer to PR #14224 at
  `fab141996690de2816e8758dabb2c13a1fe7ab16`, not the new implementation baseline.
- Size columns are **estimated production-code review budgets**, including hook/glue edits;
  report tests, additions and deletions separately as well. They are not measured candidate
  diffs or permission to exceed ~500 lines. Do not import the old module and call it a move.
- Every step must compile with its real consumer. Internal widget/state slices can be reviewed
  on an explicit stack before public activation; never publish a partially functional entry,
  suppress dead-code warnings, or call an unconnected drawing helper complete.
- New implementation branches start from a recorded main/dependency SHA after spec approval.
  Earlier merged work must not reappear in the next PR's review diff.

## Dependency map

```text
maintainer spec approval
  B1 --------------------------> T2 (event delivery)
  T0 -> T1 --------------------> T2 (client ownership/global toast integration)
  F0 -> F1 --------------------> T1/U4/U5 (optional loaded faces; default fallback first)
  W0 (only if needed) -> U3/U8
  U1 -> U2 -> U3 -> U4 -> U5 -> U6 -> U7 -> U8
  P0 (only if needed: pure moves) -> I1 / I2 / I3 / F1 / T2
  entry ownership decision + landed #14167 interfaces + U1..U8 -> I1/I2/I3
```

The arrows describe prerequisites, not parallel writes to the same files. One integrator owns
shared model/layout contracts. P0, F0, W0 and T0 are conditional preparatory PRs: reuse a
sufficient existing interface instead of introducing a redundant one.

## Backend, notifications and faces

| ID / budget | Deliverable and files | Excludes | Acceptance and evidence |
| --- | --- | --- | --- |
| B1 / 180-350 | Extend `S` with list-update generation/snapshot and typed delta/action-result delivery. Keep an existing worker, not one per overlay. | UI, face downloads, toast drawing, new HTTP cache rules. | `RemoteFriendListUpdateHandler.getUpdateIntervalNanos`, `detectChangesAndShowToast`, `start` (`V/gui/screens/social/RemoteFriendListUpdateHandler.java:81-85,152-198,227-234`): silent initial/recovery baseline, accepted/added/received/sent transitions, no display when muted but known lists advance. Same-id/name changes must follow FriendDto equality. |
| T0 / 120-240 | Correct reusable SystemToast wrapping/height and reset contract in `R/toasts.rs` or a small sibling module, retaining existing consumers. | Friend events and screen state. | `SystemToast` splits at 200px, width max(160,max line)+30, variable height; token reset changes display timer, not slide. Long translated title/message and reset during slide are required cases. |
| T1 / 150-280 | FriendToast rendering/update in `R/toasts/friends.rs` (candidate), using existing Slide and default-face callback. | Global queue ownership and arbitrary caps. | `V/gui/components/toasts/FriendToast.java:35-60,100-102`: 160 width, inset30/7, height10+max(lines,2)*11; sent without profile, other supported kinds with profile. Notification Time and fully-visible duration tested separately from slide. |
| T2 / 250-450 | Connect B1/T1 to **one client-owned toast lifecycle**; update/draw/handoff in new `R/game/friend_notifications.rs` and startup/title adapters. Reuse actual global ownership/slots if available. | A second screen-local ToastManager, host/share state. | `ToastManager.java:38-113`: closed overlay and title->world handoff do not lose entries; slots release before assignment; queued entries do not age; token lookup covers visible+waiting; F1 still expires; visibility/custom sound arbitration remains global. If slot/ownership integration alone exceeds the budget, stop and propose a separate allocator step before this one. |
| F0 / 120-240 | If needed, a separate pure-move/generalized sprite allocation seam supporting current atlas consumers. Candidate small sibling of HUD atlas code. | Friend behaviour or capacity changes in a relocation PR. | Current world/Realms slots and pixels stay identical; `hud.rs` does not grow. Read its binary region first and preserve legitimate NUL; helper under `.local`, never tracked. |
| F1 / 220-400 | Friend face resolver/upload/fallback in `R/friends_overlay/faces.rs`; one shared loader across opens/rows/toasts, bounded GPU residency separate from resolution data. | Fixed32 capacity presented as vanilla, service errors for skin failures. | `V/gui/components/PlayerFaceExtractor` and `V/renderer/PlayerSkinRenderCache`: partial/default profile head+hat while unresolved, later correct skin, reload/device recovery reupload, no world/Realms texture overwrite. Rows beyond GPU capacity still have usable controls. |

**B1 cautions:** the old Rust 64-entry pop-front queue can discard action failures. Vanilla
ToastManager has no demonstrated64 cap. A new lossy policy needs approval and explicit overflow
cases; it is not silently inherited. `PlayerSocialManager.runAction/handleResult` is separate
from an overlay exceptional-future failure. HTTP304/ETag/empty-body/cooldown changes remain out
of B1 until authlib10.0.77 evidence is independently inspected.

**Toast cautions:** use the precise `Toast.yPos` contract (firstSlotIndex * toast.height()) and
ceil(height/32) occupancy; do not assume the former is slotIndex*32. The first visibility
transition in each global update gets in/out sound eligibility, not "the first friend toast"
or "only the first transition in a toast's lifetime". Custom sounds are a separate dedup path.

## Eight UI work packages (no 2,215-line core PR)

All candidate modules below are under `R/friends_overlay/`. Keep `mod.rs` a small coordinator.
These packages port coherent behaviours from vanilla; old ranges help estimate responsibilities,
not justify wholesale cherry-picking. Each reuses preceding types instead of copying the whole
Screen. Test/preview consumers must exercise the actual modules without exposing unfinished UI.

### U1 — layout, row geometry and scrolling (200-350)

- **Files:** `layout.rs`; minimal shared geometry/action identity in `model.rs` if necessary.
- **Old region:** `friends_overlay.rs:358-407,616-653,714-728,774-821,1574-1608,1850-1909`.
- **Reference:** `AbstractFriendsTab`, `AbstractFriendsEntryContainerWidget`,
  `ScrollableLayout` and `AbstractScrollArea.updateScrolling/mouseDragged/mouseScrolled`.
- **Consumer:** shell and list projection use the same rectangles for render, hit tests and
  clipping; no separate host-computed row offsets.
- **Acceptance:**28px entries,17px request headers; wheel only under scroll area at rate10;
  no scrollbar for short content; correct clamping on resize; partly clipped buttons cannot
  click outside the clip; drag across top/bottom follows vanilla incremental delta/bounds.
- **Exclude:** service actions, translation-link hitboxes, generic focus traversal. The old
  absolute-start-offset drag and sending-specific drag gate are not assumed vanilla-equivalent.

### U2 — overlay shell and refresh/presence state (220-380)

- **Files:** `mod.rs`, `shell.rs`, `model.rs` as needed; consume U1 and B1 snapshot generation.
- **Old region:** state/constructor/tick around141-318, `draw_over`1188 onward; do not bring all
  editor/list/notification state into the shell.
- **Reference:** `FriendsOverlayScreen.init/repositionElements/refreshLists/selectTab`,
  `AbstractFriendsTab` and `LoadingDotsWidget`.
- **Acceptance:**220 content width,110x20 tabs at y-27; both loading/error surfaces update
  together; initial Friends selection; active content determines centered height. Listener
  registration/removal cannot multiply on resize/open. Presence-only update does not reorder;
  a new successful populate can reorder even with the same IDs.
- **Exclude:** title/pause entry, consent/settings transaction, add/remove/accept callbacks,
  skin downloader and toast queue ownership. Shell emits typed requests/results to its consumer.

### U3 — add-friend edit field and send transaction (300-450)

- **Files:** `add.rs`; use existing Caret/clipboard helpers, B1 action contract and U2 state.
- **Old region:**409-442,543-550,669-676,730-772,895-907,1085-1174,1253-1351.
- **Reference:** `AddFriendWidget` constructor/state/send paths and `EditBox` behaviour.
- **Acceptance:** maximum32 characters; empty/trimmed name, self/existing friend/pending
  validation; Enter and button use identical send path; selection/paste/caret horizontal
  scrolling/blink; SENDING disables editing and duplicate send; successful result clears input
  and follows vanilla LOADING/refresh transition, failed result preserves it. Profile copy
  includes tooltip and the vanilla RTL profile-row ordering.
- **Exclude:** generic Caret changes hidden in this PR. If an existing helper needs a generic
  fix, W0 first covers its existing consumers; do not duplicate a second editor.

### U4 — friend row, presence projection and removal (250-420)

- **Files:** `entries.rs` with shared row drawing; `model.rs` for UUID-bound actions. Use F1
  when available, otherwise the same default-face path, not a blank texture.
- **Old region:**321-356,912-929,1032-1036,1355-1445,1610-1700.
- **Reference:** `FriendsOverlayScreen.populateLists/applyPresenceUpdate`, `FriendsTab`,
  `FriendEntry.confirmRemoveFriend` and shared entry container.
- **Acceptance:** stable presence-sort on populate, status-only updates without reorder;
  pending removal survives repopulation by UUID; action runs only after removal confirmation;
  both tabs' enablement/loading follows vanilla action start/completion.
- **Identity gate:** at least61 friends plus pending requests; select the61st friend's remove
  action and verify it cannot accept/reject a request. The old `40+index`/`>=100` split collides
  at index60 (`old913,1032-1038`). No slot number is an account identity.
- **Exclude:** full keyboard/narration policy and privacy link handling, assigned to U6/U8.

### U5 — Requests tab and incoming/outgoing transactions (230-400)

- **Files:** `requests.rs`, consuming shared row/clip helpers and UUID/action-kind identity.
- **Old region:**694-709,930-977,1037-1060,1497-1572,1947-1965.
- **Reference:** `PendingTab.updateEntries`, `IncomingEntry`, `OutgoingEntry`.
- **Acceptance:** Requests has **no AddFriendWidget**; incoming/outgoing independently empty,
  both groups populated, total empty, group styling/spacing and incoming-count tab label;
  accept/decline/revoke call correct APIs; loading blocks repeats and failure restores the
  correct row; clipping works with either group partially visible.
- **Identity gate:** refresh/reorder between draw and press must not act on another UUID;
  enough requests to exceed the old u8 range remain operable. Do not invent a confirmation
  screen for every request action; follow the actual entry callbacks.
- **Exclude:** notification generation (B1/T2), consent and rich-link navigation.

### U6 — state messages, rich links and overlong text (240-400)

- **Files:** `links.rs` and small message helpers; share text measurement/hitboxes with draw.
- **Old region:**1066-1083,1447-1495,1702-1747,1911-1945; list empty/footer content in U4.
- **Reference:** `FriendsTab.createManageAccountFooter/showEmpty`, `PendingTab.showEmpty`,
  `AbstractFriendsTab.showError`, `FocusableTextWidget`, `Screen`/entry scrolling text.
- **Acceptance:** all mapped list errors, empty states and300ms loading dots; long translated
  footer wraps and affects scroll height; only actual styled link spans are clickable, including
  wrapped RTL text; cropped spans cannot be clicked; footer activation uses the correct sound.
- **Regression gate:** old substring-based link positioning (`old1732-1736`) is not a styled,
  bidi-aware Component hit-test. Long presence text horizontal scrolling is assigned here,
  not silently left out while claiming complete text parity.
- **Exclude:** performing browser actions; emit a typed link request to U7/host.

### U7 — consent, removal popup and privacy-return stack (250-430)

- **Files:** `confirm.rs` (candidate) and small shell state. Keep entry consent transaction
  separate from settings-menu toggle behaviour.
- **Old region:**enable/confirm fields141-189,611-614,660-686,825-870,1001-1031,
  1152-1163,1753-1847. U4 supplies only a UUID-bound removal intent.
- **Reference:** `OnlineOptionsScreen.confirmFriendsListEnabled/applyFriendSettings`,
  `FriendsListConfirmScreen`, `PopupScreen`, `ConfirmScreen`, `PrivacyConfirmLinkScreen`.
- **Acceptance:** consent Turn On success/failure and Turn Off; only success commits flags and
  opens overlay; remove Cancel never performs remove; modal input does not reach underlying
  rows or trigger outside-close; privacy link Open/Cancel restores the exact confirmation or
  overlay state. Background/title/button sizes and focus follow the correct screen class.
- **Exclude:** unrelated Online options controls/translation expansion and share/invite prompts.

### U8 — focus, nested narration and shared control adapter (300-480)

- **Files:** `focus.rs`, shared typed control description; shell delegates rather than retaining
  duplicated title/game drawing and action-ID switches.
- **Old region:**444-609,982-1000 plus duplicated overlay button/narration loops in
  `R/title.rs` and `R/game/friends.rs` (historical only; do not reintroduce that coupling).
- **Reference:** `TabNavigationBar`, `AbstractFriendsEntryContainerWidget:79-98`, entry
  narration and focusable header/footer/link controls.
- **Acceptance:**Tab/Shift+Tab skips inactive elements; selected-tab bar entry/exit, arrows,
  Ctrl+number/Ctrl+Tab, Enter/Space; focus and scroll bring actual target into view; modal focus
  stays modal. Narration announces tab position, player name, child action and usage; section
  headers, profile text and footer links can be focused as vanilla permits.
- **Regression gate:** tooltip-only generic button names are not nested-entry narration. The
  same descriptions drive draw/hit testing, narrator and controller widget registration; no
  language-specific RTL branch or raw u8 request identity.
- **Exclude:** global Friends shortcut and opening public title/pause entries, assigned below.

## Host adapters: independently reviewable, separate from UI logic

These are engine integration PRs, not Play with Friends host code. Require U1..U8 plus T2 and
an agreed entry-placement decision; remain blocked while the overlapping migration interface
or icon ownership is unsettled. Each must say which public behaviour it enables and must not
ship disabled buttons or placeholder flows as completed parity.

| ID / budget | Allowed files and reference | Acceptance / non-goals |
| --- | --- | --- |
| P0 / <=400 per pure move | If needed, extract existing title/game/HUD responsibilities into sibling modules, before adding overlay behaviour. | No behaviour change; compare before/after existing tests and screenshots where visible. Record move additions/deletions separately. No file already over3000 lines grows; no compression trick or NUL loss. Split multiple relocations rather than mixing them. |
| W0 / 80-200 per generic fix | Existing `horizon-client-render/src/ui.rs` Caret/sprite helpers only when U3/U8 needs a demonstrably missing generic contract. | Check existing editor/sprite consumers as well as overlay. Blink after cursor moves and scroll-to are separate behaviours if their diff needs splitting. No overlay-specific branch in generic widgets. |
| I1 / 180-350 | New `R/game/friends_overlay.rs` adapter; small dispatch replacement in gameplay screen/render/input paths. Reuse pause layout, do not edit the host/share implementation. | `PauseScreen.createPauseMenu`/`FriendsOverlayScreen.extractBackground`: correct demo/offline gate, consent, badge1..5/more, background draw without hover then blur, actual-parent return, pause semantics, listener cleanup, resize; no fifth icon unless separately approved. Dynamic registered pause rows remain intact. |
| I2 / 200-400 | New `R/title/friends_overlay.rs` adapter; minimal title mode/dispatch replacement; P0 first where necessary. | `TitleScreen.init/tick`: same vanilla gate/positioning, badge refresh, consent, Escape/outside restores actual title state, panorama/background blur, no lost notifications during startup/world entry; leave Join by Code reachable through agreed extension entry. Do not add another renderer/skin worker. |
| I3 / 100-220 | Existing rebindable Friends action + global input dispatcher; no hardcoded O check, no duplicate binding. | `Minecraft.handleGlobalKeyPress/toggleFriendsScreen`: null/title/pause/current-overlay routes; reject unrelated screens and focused input capture; debug modifier suppression; demo/offline restriction; toggle closes to actual parent, including no screen. No simulated pause parent for direct in-world opening. |

## Coverage and publication checklist

- [ ] Every reference in an implementation PR was actually reread, including its callers;
  distinguish vanilla requirements, old-port defects and Rust-only resource policies.
- [ ] Per-step reviewed diff stays around500 production lines **with integration**, and full
  additions/deletions are reported. Budget overflow returns to the spec before publication.
- [ ] No broad `allow(dead_code)`, arbitrary new API or public partially implemented overlay.
- [ ] Test package/target/filter yields a nonzero matched count. Gameplay tests use the current
  library declaration; earlier binary filters matching0 are not counted.
- [ ] B1 covers initial/recovery baseline, events and failures; T0/T1/T2 cover variable-height
  layout, token reset, queue saturation, handoff, F1/timing and sound-order arbitration.
- [ ] F1 covers default/loading/loaded/recovered faces and row/toast/world/Realms isolation.
- [ ] UI covers long lists/identity, repopulate vs presence-only updates, clipboard/editing,
  clipped hit-tests, scroll drag, RTL rich links, nested narration and all parent/consent routes.
- [ ] Run affected checks and current pr_ready; exact failures/skips/resources recorded. WSL
  Linux build output stays separate from Windows target. No executable/window launched for docs.
- [ ] `friends_integration` is only a host/guest overlap guard, not account-overlay proof.
  Dedicated account-social fixture/oracle coverage must be agreed and accurately scoped.
- [ ] Preserve existing tests. Archive temporary probes under `.local`; settle required
  permanent regression harness changes before opening implementation PRs.
- [ ] Obtain actual UI screenshots/live-account acceptance where required; a source review,
  fake-service state test or draw-without-panic run is not end-to-end/visual acceptance.
- [ ] No automatic GitHub post, implementation claim, old-PR closure, force-push or update to
  the isolated merge branch from this document-only planning task.
