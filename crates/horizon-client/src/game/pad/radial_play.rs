//! The radial menu in play (`horizon-client-input/src/radial.rs` has its rules,
//! `radial_overlay.rs` draws it): what opens and closes it, what the pad's presses do while it is
//! open, and what a commit runs.
use super::{Game, Result, UI_CLICK_VOLUME};
use horizon_client::gamepad::MenuJob;
use horizon_client::gamepad::{self, Button, RadialMode, RadialStick, Rumble};
use horizon_client::pad_glyph::Glyph;
use horizon_client::text::Text;
use horizon_client_input::radial::{self, RadialAction, Slot};
use horizon_client_render::radial_overlay::{self, Dial, Seen, Toggles, View};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// The D-pad's buttons, which settle after the menu closes on a D-pad opener.
const DPAD: [Button; 4] = [
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
];

/// One step of a poll's button changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PollStep {
    /// A release held back from the poll before, until a simulation tick had passed.
    Late(Button),
    Edge(Button, bool),
}

/// The order a poll acts on its changes in: the releases held back first, then this poll's
/// presses and releases. While the radial menu is open its presses come before every release, the
/// held-back ones too: B in the same poll as the opener's release (however late) still cancels.
pub(super) fn poll_order(
    late: Vec<Button>,
    edges: Vec<(Button, bool)>,
    radial_open: bool,
) -> Vec<PollStep> {
    let late = late.into_iter().map(PollStep::Late);
    let edges = edges.into_iter().map(|(b, down)| PollStep::Edge(b, down));
    if !radial_open {
        return late.chain(edges).collect();
    }
    let (presses, releases): (Vec<_>, Vec<_>) =
        edges.partition(|step| matches!(step, PollStep::Edge(_, true)));
    presses.into_iter().chain(late).chain(releases).collect()
}

impl Game<'_> {
    /// Whether the radial menu selects with this stick (0 left, 1 right): while it is open that
    /// stick neither walks nor turns the view.
    pub(in crate::game) fn pad_radial_selects(&self, stick: usize) -> bool {
        match self.settings.controller.radial_stick {
            RadialStick::Right => stick == 1,
            RadialStick::Left => stick == 0,
            RadialStick::Either => true,
        }
    }

    /// Each stick's tilts, from its own deadzone (left, right).
    fn pad_radial_tilts(&self) -> [radial::Tilt; 2] {
        let settings = &self.settings.controller;
        [settings.move_deadzone, settings.look_deadzone].map(radial::Tilt::for_deadzone)
    }

    /// The stick that selects now (0 left, 1 right), where it is, the furthest it leant since the
    /// last poll, and its tilts. With Either, the stick that last leant past the select tilt.
    fn pad_radial_selector(&self) -> (usize, [f32; 2], [f32; 2], radial::Tilt) {
        let index = match self.settings.controller.radial_stick {
            RadialStick::Right => 1,
            RadialStick::Left => 0,
            RadialStick::Either => self.pad.radial_either.unwrap_or(1),
        };
        let stick = [self.pad.state.left, self.pad.state.right][index];
        (
            index,
            stick,
            self.pad.peaks[index],
            self.pad_radial_tilts()[index],
        )
    }

    /// With Either: a stick that has just leant past its select tilt (or did between two polls)
    /// takes over the selecting. While the menu is open, only one that has been at rest since
    /// the open: the walking thumb easing forward on its frozen stick isn't aiming. The stick
    /// that takes over selects at once, whatever the other was waiting for.
    fn pad_radial_follow_either(&mut self) {
        let tilts = self.pad_radial_tilts();
        let sticks = [self.pad.state.left, self.pad.state.right];
        let open = self.pad.radial.is_open();
        for i in 0..2 {
            if radial::stick_length(sticks[i]) < tilts[i].rest {
                self.pad.radial_rested[i] = true;
            }
            let reach =
                radial::stick_length(sticks[i]).max(radial::stick_length(self.pad.peaks[i]));
            let over = reach >= tilts[i].select;
            let may = !open || self.pad.radial_rested[i];
            if over && !self.pad.radial_over[i] && may {
                let before = self.pad.radial_either.replace(i).unwrap_or(1);
                if open && before != i {
                    self.pad.radial.took_over();
                }
            }
            self.pad.radial_over[i] = over;
        }
    }

    /// Screens, the on-screen keyboard and the prompts over gameplay come first. It isn't a
    /// screen: the world goes on behind it, here and on a server.
    pub(in crate::game) fn pad_radial_can_open(&self) -> bool {
        !self.pad_in_menu()
            && !self.pad_keyboard_captures()
            && !self.screen_over_playing()
            && self.player.is_some()
            && self.sleeping.is_none()
            && !self.pause
    }

    /// The opener went down: always on the first page.
    pub(in crate::game) fn pad_radial_open(&mut self) {
        let tilts = self.pad_radial_tilts();
        let sticks = [self.pad.state.left, self.pad.state.right];
        self.pad.radial_rested =
            std::array::from_fn(|i| radial::stick_length(sticks[i]) < tilts[i].rest);
        let (index, stick, _, tilt) = self.pad_radial_selector();
        self.pad.radial.open(index, stick, tilt, Instant::now());
        self.pad.radial_face = self.pad_radial_face();
        self.pad.radial_page_said = false;
        self.pad.radial_said.reset();
    }

    /// The opener came up: held, it commits; in toggle mode the menu stays open, until A, the
    /// opener again or B.
    pub(in crate::game) fn pad_radial_release(&mut self) -> Result<()> {
        if !self.pad.radial.is_open() {
            return Ok(());
        }
        if self.settings.controller.radial_mode == RadialMode::Toggle && !self.pad.radial.latched()
        {
            self.pad.radial.latch();
            return Ok(());
        }
        self.pad_radial_commit(false)
    }

    /// Whether `action` would do anything now. Only what is known here, cheaply and for certain:
    /// the emote wheel opens only with cosmetics on, Quick Actions only when the server gave a
    /// dialog for it, and Social only on a server (`Gui.handleKeybinds`). Everything else on the
    /// list always runs.
    fn pad_radial_runs(&self, action: RadialAction) -> bool {
        self.pad_radial_why(action).is_none()
    }

    /// Why `action` wouldn't do anything now, as a language key for the overlay's hub; `None`
    /// when it runs.
    fn pad_radial_why(&self, action: RadialAction) -> Option<&'static str> {
        match action {
            RadialAction::Emote if !super::super::cosmetics::enabled() => Some(WHY_COSMETICS),
            RadialAction::QuickActions if !self.has_quick_actions() => Some(WHY_QUICK_ACTIONS),
            RadialAction::SocialInteractions if !self.is_multiplayer_server() => Some(WHY_SOCIAL),
            _ => None,
        }
    }

    /// Which of the page's slots would run now: the overlay dims the others. A missing action
    /// never runs.
    pub(in crate::game) fn pad_radial_available(&self) -> [bool; radial::SLOTS] {
        self.pad_radial_available_on(self.pad.radial.page())
    }

    /// `pad_radial_available` on page `page`.
    fn pad_radial_available_on(&self, page: usize) -> [bool; radial::SLOTS] {
        let page = self.settings.controller.radial.page(page);
        page.slots().map(|slot| match slot {
            Slot::Empty => true,
            Slot::Action(action) => self.pad_radial_runs(*action),
            Slot::Missing(_) => false,
        })
    }

    /// A slot that holds something that would run: only those tick as the stick reaches them.
    fn pad_radial_fills(&self, slot: usize) -> bool {
        let page = self.settings.controller.radial.page(self.pad.radial.page());
        !page.slot(slot).is_empty() && self.pad_radial_available()[slot]
    }

    /// Each poll, before its presses: the selecting stick, the D-pad's settle, a second page
    /// tick due; and while it's open, the stick, or the end of it if something took over (a bed,
    /// a death) in a way the context doesn't see.
    pub(in crate::game) fn pad_radial_frame(&mut self) {
        let now = Instant::now();
        self.pad_radial_poll(now);
        let menu = &self.pad.radial;
        let seen = menu.shown(now).then(|| Seen {
            page: menu.page(),
            selected: menu.selected(),
            waiting: menu.waiting(),
        });
        self.pad.radial_look.observe(seen, now);
    }

    /// `pad_radial_frame`'s work, before the overlay's look sees the result.
    fn pad_radial_poll(&mut self, now: Instant) {
        self.pad
            .radial
            .sample([self.pad.state.left, self.pad.state.right], now);
        if self.settings.controller.radial_stick == RadialStick::Either {
            self.pad_radial_follow_either();
        }
        let (_, stick, peak, tilt) = self.pad_radial_selector();
        if let Some((at, rumble)) = self.pad.radial_rumble_due
            && now >= at
        {
            self.pad.radial_rumble_due = None;
            self.pad_rumble(rumble);
        }
        if !self.pad.radial.is_open() {
            return;
        }
        if !self.pad_radial_can_open() {
            self.pad_radial_cancel();
            return;
        }
        let was = self.pad.radial.selected();
        self.pad.radial.update_with_peak(stick, peak, tilt, now);
        // A tick as the stick reaches a slot that would run, so a flick can be felt.
        let selected = self.pad.radial.selected();
        if selected != was && selected.is_some_and(|slot| self.pad_radial_fills(slot)) {
            self.pad_rumble(Rumble::RadialSlot);
        }
        self.pad_radial_narrate(now);
    }

    /// With the narrator on: the page's name once the overlay shows and after each page turn,
    /// and the slot's caption once the selection settles.
    fn pad_radial_narrate(&mut self, now: Instant) {
        if horizon_narrator::status() == horizon_narrator::Status::Off
            || !self.pad.radial.shown(now)
        {
            return;
        }
        let pages = &self.settings.controller.radial;
        let page = self.pad.radial.page();
        if !std::mem::replace(&mut self.pad.radial_page_said, true) {
            let name = horizon_client_render::radial_overlay::page_label(pages.page(page));
            self.say_system_now(&Text::literal(name));
        }
        let at = self.pad.radial.selected().map(|slot| (page, slot));
        if let Some((page, slot)) = self.pad.radial_said.poll(at, now) {
            let slot = pages.page(page).slot(slot);
            let caption = horizon_client_render::radial_overlay::slot_caption(slot)
                .unwrap_or_else(|| horizon_client::lang::get("horizon.radial.empty"));
            self.say_system_now(&Text::literal(caption));
        }
    }

    /// A press while it's open: bumpers turn the page, B and Start cancel; in toggle mode A or
    /// the opener again commits. A trigger cancels and goes on to its own action (`false`: not
    /// used up), so a fight goes on; no other press reaches a mapping.
    pub(in crate::game) fn pad_radial_button(&mut self, button: Button) -> Result<bool> {
        let settings = &self.settings.controller;
        let opener = settings.binding(gamepad::RADIAL);
        let toggle = settings.radial_mode == RadialMode::Toggle;
        let bumpers = [Button::LeftBumper, Button::RightBumper];
        match button {
            // Toggle mode, latched open: the opener again commits.
            b if Some(b) == opener => {
                if self.pad.radial.latched() {
                    self.pad_radial_commit(false)?;
                }
            }
            // The thumb slid off the stick to A: what it had settled on, not what it crossed.
            Button::South if toggle => self.pad_radial_commit(true)?,
            b if bumpers.contains(&b) => {
                // With a bumper for the opener, the other turns the pages on.
                let forward = opener.is_some_and(|o| bumpers.contains(&o)) || b == bumpers[1];
                self.pad_radial_turn(forward);
            }
            Button::East | Button::Start => self.pad_radial_cancel(),
            Button::LeftTrigger | Button::RightTrigger => {
                self.pad_radial_cancel();
                return Ok(false);
            }
            _ => {}
        }
        Ok(true)
    }

    /// A page turn: a tick, and a second one when the pages went round an end.
    fn pad_radial_turn(&mut self, forward: bool) {
        let (_, stick, _, tilt) = self.pad_radial_selector();
        let pages = self.settings.controller.radial.len();
        let Some(wrapped) = self.pad.radial.turn_page(forward, stick, tilt, pages) else {
            return;
        };
        self.pad.radial_look.turned(forward);
        self.pad_rumble(Rumble::RadialPage);
        if wrapped {
            self.pad.radial_rumble_due =
                Some((Instant::now() + radial::WRAP_TICK_GAP, Rumble::RadialWrap));
        }
        self.pad.radial_page_said = false;
        self.pad.radial_said.reset();
    }

    /// The selected action, pressed and let go, once. Every entry acts on its press, and nothing
    /// stays held behind it. One that can't run now does nothing at all, and a release that runs
    /// nothing after something was selected gives a soft buzz.
    /// `by_a`: toggle mode's A, pressed by the thumb that was selecting (`RadialMenu::commit_settled`).
    fn pad_radial_commit(&mut self, by_a: bool) -> Result<()> {
        let page = self.pad.radial.page();
        let aimed = self.pad.radial.aimed();
        let selecting = self.pad_radial_selector().0;
        let now = Instant::now();
        let slot = if by_a {
            self.pad.radial.commit_settled(now)
        } else {
            self.pad.radial.commit(now)
        };
        self.pad_radial_closed(selecting);
        // Social, dimmed in single player, still goes through its key there: vanilla's action
        // bar says it isn't available (`Gui.handleKeybinds`).
        let action = slot
            .and_then(|slot| {
                self.settings
                    .controller
                    .radial
                    .page(page)
                    .slot(slot)
                    .action()
            })
            .filter(|action| {
                self.pad_radial_runs(*action) || *action == RadialAction::SocialInteractions
            });
        let Some(action) = action else {
            if aimed {
                self.pad_rumble(Rumble::RadialNothing);
            }
            return Ok(());
        };
        self.pad.radial_runs.push(action.id());
        if let Some(slot) = slot {
            self.pad.radial_look.ran(slot);
        }
        let shoots = matches!(action, RadialAction::Screenshot | RadialAction::ToggleGui);
        let action = action.action();
        self.audio
            .play("minecraft:ui.button.click", None, UI_CLICK_VOLUME, 1.0);
        // The release runs whatever the press did, so no key stays held.
        let pressed = self.pad_action(action, true);
        let released = self.pad_action(action, false);
        // What opened a screen, takes a screenshot or hides the HUD gets the view to itself at
        // once: no flash or fade over it.
        if shoots || self.pad_in_menu() || self.screen_over_playing() {
            self.pad.radial_look.cut();
        }
        pressed.and(released)
    }

    /// B, Start, a trigger, a key or a click, a takeover the context doesn't see: the menu closes
    /// and nothing runs. (The window's focus is in the context: losing it cancels in
    /// `pad_cancel_inputs`.)
    pub(in crate::game) fn pad_radial_cancel(&mut self) {
        if self.pad.radial.is_open() {
            let selecting = self.pad_radial_selector().0;
            self.pad.radial.cancel();
            self.pad_radial_closed(selecting);
        }
    }

    /// After a commit or a cancel: the stick that was selecting (`selecting`, 0 left, 1 right),
    /// and with Either both, moves the player or the view only once it has been let go; a held drop starts its wait
    /// again rather than throwing the rest of the stack the moment the menu is gone; and on a
    /// D-pad opener the D-pad settles.
    fn pad_radial_closed(&mut self, selecting: usize) {
        let now = Instant::now();
        // With Either neither stick moved anything while it was open: both wait.
        let either = self.settings.controller.radial_stick == RadialStick::Either;
        for i in 0..2 {
            if i != selecting && !either {
                continue;
            }
            let deadzone = [
                self.settings.controller.move_deadzone,
                self.settings.controller.look_deadzone,
            ][i];
            let stick = [self.pad.state.left, self.pad.state.right][i];
            if gamepad::radial(stick, deadzone) != [0.0; 2] {
                self.pad.blocked_sticks[i] = true;
                self.pad.peaks[i] = [0.0; 2];
                match i {
                    0 => self.pad.state.left = [0.0; 2],
                    _ => self.pad.state.right = [0.0; 2],
                }
            }
        }
        if self.pad.held_drop.is_some() {
            let delay = Duration::from_secs_f32(self.settings.controller.repeat_delay);
            self.pad.held_drop = Some(gamepad::HeldDrop::new(now, delay));
        }
        if self
            .settings
            .controller
            .binding(gamepad::RADIAL)
            .is_some_and(|b| DPAD.contains(&b))
        {
            self.pad.radial_settle.closed(now);
        }
    }

    /// A D-pad press in play starting just after the menu closed on a D-pad opener: a thumb
    /// rolling off it. It is ignored, and so is its release; the opener's own isn't.
    pub(in crate::game) fn pad_radial_settling(&self, button: Button) -> bool {
        let opener = self.settings.controller.binding(gamepad::RADIAL);
        let held = opener.is_some_and(|opener| self.pad.state.held(opener));
        DPAD.contains(&button)
            && self
                .pad
                .radial_settle
                .quiet(opener == Some(button), held, Instant::now())
    }

    /// A button came up: the opener's release starts the D-pad's settle again.
    pub(in crate::game) fn pad_radial_button_up(&mut self, button: Button) {
        if self.settings.controller.binding(gamepad::RADIAL) == Some(button) {
            self.pad.radial_settle.opener_up(Instant::now());
        }
    }

    /// A new player entity (spawn, respawn, a server joined, a dimension change): its health is
    /// not compared with the old one's, and the radial menu closes as for a cancel, as a commit
    /// would run on the new body (a death with immediate respawn, the End's exit).
    pub(in crate::game) fn pad_player_replaced(&mut self) {
        self.pad.player_replaced();
        self.pad_radial_cancel();
    }

    pub(in crate::game) fn pad_radial_status(&self) -> Value {
        let menu = &self.pad.radial;
        let pages = &self.settings.controller.radial;
        let page = pages.page(menu.page());
        json!({
            "open": menu.is_open(),
            "latched": menu.latched(),
            "page": menu.page(),
            "pages": pages.len(),
            "name": horizon_client_render::radial_overlay::page_label(page),
            "slots": page.slots().map(|slot| (!slot.is_empty()).then(|| slot.id())),
            "available": self.pad_radial_available(),
            "selected": menu.selected(),
            "aimed": menu.aimed(),
            "shown": menu.shown(Instant::now()),
            "drawn": self.pad_radial_drawn(self.hud_clock()).is_some(),
            // The overlay's glyphs as the player sees them: the bumpers that turn the pages and
            // the button that cancels, by their printed labels.
            "glyphs": {
                "pages": self.pad_radial_page_keys().map(|key| key.map(|glyph| glyph.label())),
                "cancel": self.menu_glyph(MenuJob::Back).label(),
            },
            "waiting": menu.waiting(),
            "selecting": self.pad_radial_selector().0,
            "mode": self.settings.controller.radial_mode.name(),
            "stick": self.settings.controller.radial_stick.name(),
            "runs": self.pad.radial_runs.last,
            "run_count": self.pad.radial_runs.count,
        })
    }

    /// The overlay, over the HUD, in this player's view: while it shows, and while it fades out.
    pub(in crate::game) fn draw_pad_radial(&self, batch: &mut horizon_client::hud::HudBatch) {
        let clock = self.hud_clock();
        let look = &self.pad.radial_look;
        let Some(seen) = self.pad_radial_drawn(clock) else {
            return;
        };
        let pages = &self.settings.controller.radial;
        let page = pages.page(seen.page);
        let why = page.slots().map(|slot| match slot {
            Slot::Action(action) => self.pad_radial_why(*action),
            _ => None,
        });
        let dial = Dial {
            pages,
            seen,
            available: self.pad_radial_available_on(seen.page),
            why,
            toggles: self.pad_radial_toggles(),
            page_keys: self.pad_radial_page_keys(),
            cancel: self.menu_glyph(MenuJob::Back),
            backing: (self.settings.text_background(radial_overlay::CHIP_BACKING) >> 24) as u8,
            room: &self.ui.room,
        };
        let painter = super::super::item_draw::IconPainter {
            game: self,
            face: self.pad.radial_face.as_deref(),
        };
        radial_overlay::draw_overlay(batch, &self.gui.atlas, &painter, &dial, look, clock);
    }

    /// What the overlay draws at `clock`: while it shows, and while it fades out, but never a
    /// fade-out over a screen that came up meanwhile.
    fn pad_radial_drawn(&self, clock: horizon_client::hud_kit::motion::Clock) -> Option<Seen> {
        let look = &self.pad.radial_look;
        if !look.open() && (self.pad_in_menu() || self.screen_over_playing()) {
            return None;
        }
        look.showing(clock)
    }

    /// The overlay is showing (not only fading out): chat lines and the first-person hand give
    /// way to it.
    pub(in crate::game) fn pad_radial_shown(&self) -> bool {
        self.pad.radial_look.open()
    }

    /// The toggles the overlay shows the state of.
    fn pad_radial_toggles(&self) -> Toggles {
        use super::super::perspective::CameraType;
        Toggles {
            view: match self.camera_type {
                CameraType::FirstPerson => View::FirstPerson,
                CameraType::ThirdPersonBack => View::ThirdPersonBack,
                CameraType::ThirdPersonFront => View::ThirdPersonFront,
            },
            hud_hidden: self.ui.hide_gui,
            cinematic: self.smooth_camera,
        }
    }

    /// The bumpers that turn the pages back and on, in this player's glyphs. With a bumper as the
    /// opener, only the other, which turns them on.
    fn pad_radial_page_keys(&self) -> [Option<Glyph>; 2] {
        let opener = self.settings.controller.binding(gamepad::RADIAL);
        let [back, on] = [MenuJob::PagePrev, MenuJob::PageNext];
        match opener {
            Some(Button::LeftBumper) => [None, Some(self.menu_glyph(on))],
            Some(Button::RightBumper) => [None, Some(self.menu_glyph(back))],
            _ => [Some(self.menu_glyph(back)), Some(self.menu_glyph(on))],
        }
    }

    /// The player's own skin, for the Emote slot's face: their entry in the player list, which
    /// single player has too (the window is its server thread's client). Before the entry or its
    /// texture has come, the default skin their id gives, as the player list draws it.
    fn pad_radial_face(&mut self) -> Option<String> {
        let me = self.remote.as_ref().map(|r| r.profile());
        let skin = match (me, self.remote_tab_players()) {
            (Some(me), Some((players, _, _))) => players
                .into_iter()
                .find(|p| p.uuid == me)
                .map(|p| (me, p.skin)),
            _ => None,
        };
        let me = me?;
        Some(match skin {
            Some((me, skin)) => self.face_skin(me, &skin),
            None => horizon_client::tab_list::default_skin(me).to_owned(),
        })
    }
}

/// The hub's reasons a slot can't run (language keys).
const WHY_COSMETICS: &str = "horizon.radial.why.cosmetics";
const WHY_QUICK_ACTIONS: &str = "horizon.radial.why.quick_actions";
const WHY_SOCIAL: &str = "horizon.radial.why.social";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_in_the_poll_of_a_held_back_opener_release_comes_first_while_open() {
        let late = vec![Button::DPadRight];
        let edges = vec![(Button::East, true), (Button::South, false)];
        assert_eq!(
            poll_order(late.clone(), edges.clone(), true),
            [
                PollStep::Edge(Button::East, true),
                PollStep::Late(Button::DPadRight),
                PollStep::Edge(Button::South, false),
            ]
        );
        // Closed, the held-back releases go first, then the changes as they came.
        assert_eq!(
            poll_order(late, edges, false),
            [
                PollStep::Late(Button::DPadRight),
                PollStep::Edge(Button::East, true),
                PollStep::Edge(Button::South, false),
            ]
        );
    }
}
