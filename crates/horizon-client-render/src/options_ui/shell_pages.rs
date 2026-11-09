//! The pages Options shows on the settings shell (`settings_shell`): which shell each page
//! opens, what a shell's outputs do here, and the radial menu's pages (`radial_editor`), whose
//! name entry is drawn here in place of a shell, and whose rows of Controller Settings step here.
use super::{Event, OptionsUi, Page, tr};
use crate::{
    gamepad,
    hud::{HudAtlas, HudBatch},
    radial_editor::Named,
    settings::{Opt, Settings},
    settings_shell as shell,
    text::Text,
};

impl OptionsUi {
    /// The open page's screen on the settings shell. This, with `shell_mut`, is all a screen
    /// needs here: drawing, clicks, the wheel, a stick's scrolling, keys and typing all go
    /// through them, so no input path can miss one.
    pub(super) fn shell(&self) -> Option<&dyn shell::Hosted> {
        match self.page() {
            Page::Extras => Some(&self.extras),
            page if Self::graphics_page(page).is_some() => Some(&self.graphics),
            Page::ControllerRadial => Some(&self.radial),
            Page::ControllerRadialPick => Some(&self.radial_pick),
            page if Self::controller_page(page).is_some() => Some(&self.controller),
            page if Self::shader_page(page).is_some() => Some(&self.shader),
            _ => None,
        }
    }
    pub(super) fn shell_mut(&mut self) -> Option<&mut dyn shell::Hosted> {
        self.graphics.screen.output = self.output;
        match self.page() {
            Page::Extras => Some(&mut self.extras),
            page if Self::graphics_page(page).is_some() => Some(&mut self.graphics),
            Page::ControllerRadial => Some(&mut self.radial),
            Page::ControllerRadialPick => Some(&mut self.radial_pick),
            page if Self::controller_page(page).is_some() => Some(&mut self.controller),
            page if Self::shader_page(page).is_some() => Some(&mut self.shader),
            _ => None,
        }
    }
    /// Where the open shell screen asked the focus to open (a radial slot's list, on the slot's
    /// action), for a controller's focus to land there.
    pub fn shell_opened_focus(&self) -> Option<[f32; 4]> {
        self.shell()?.opened_focus()
    }
    /// The clicks until it is set back are a controller's Left or Right on a row: a choice of a
    /// settings shell steps, rather than opens.
    pub fn pad_clicks_step(&mut self, on: bool) {
        if on {
            if let Some(shell) = self.shell_mut() {
                shell.clicks_step(true);
            }
            return;
        }
        // Every shell: the click may have changed the page.
        let shells: [&mut dyn shell::Hosted; 6] = [
            &mut self.extras,
            &mut self.graphics,
            &mut self.controller,
            &mut self.radial,
            &mut self.radial_pick,
            &mut self.shader,
        ];
        for shell in shells {
            shell.clicks_step(false);
        }
    }
    /// The controller's focus landed there.
    pub fn clear_shell_opened_focus(&mut self) {
        if let Some(shell) = self.shell_mut() {
            shell.clear_opened_focus();
        }
    }
    /// The window's size, for the graphics and controller screens' rows that step an option
    /// (`Opt::cycle`, which a fullscreen resolution needs): before each input that can press one.
    pub(super) fn shell_framebuffers(&mut self) {
        self.graphics.screen.framebuffer = self.framebuffer;
        self.controller.screen.framebuffer = self.framebuffer;
    }
    /// Who and what Controller Settings is about: the game sets it before each draw.
    pub fn set_controller_context(&mut self, context: crate::controller_ui::Context) {
        self.controller.screen.context = context;
    }
    /// The binding the screen waits on: a controller button's, else a key's.
    pub(super) fn waiting(&self) -> Option<shell::Capture> {
        let pad = self.selected_pad_action.map(shell::Capture::Pad);
        pad.or(self.selected_key.map(shell::Capture::Key))
    }
    /// What a shell screen's outputs do to these options.
    pub(super) fn shell_events(
        &mut self,
        s: &mut Settings,
        outputs: Vec<shell::Output>,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        for output in outputs {
            match output {
                shell::Output::Event(event) => events.push(event),
                shell::Output::Bind(shell::Capture::Key(name)) => self.selected_key = Some(name),
                shell::Output::Bind(shell::Capture::Pad(action)) => {
                    self.selected_pad_action = Some(action)
                }
                shell::Output::Push(page) => {
                    if page == Page::HdrCalibration {
                        self.calibration =
                            Some(crate::hdr_calibration::Calibration::new(s, &self.output));
                    }
                    self.push(page);
                    // A slot's list, for the slot, opening on what it holds.
                    if page == Page::ControllerRadialPick
                        && let Some(at) = self.radial.screen.picking.take()
                    {
                        let row = self.radial_pick.screen.open_on(at, &s.controller.radial);
                        self.radial_pick.focus_soon(row);
                    }
                }
                shell::Output::Done => {
                    // Back from a slot's list, the focus is on the slot.
                    let slot = (self.page() == Page::ControllerRadialPick)
                        .then(|| self.radial_pick.screen.at());
                    events.extend(self.back(s));
                    if let Some((page, slot)) = slot {
                        self.radial
                            .focus_soon(crate::radial_editor::RadialRow::Slot(page, slot));
                    }
                }
            }
        }
        // A pick on a slot's list is a change of the radial page's, which its Undo takes back.
        if self.radial_pick.screen.take_picked() {
            self.radial.changed_elsewhere();
        }
        // A radial page added or deleted: the one to show now.
        if let Some(page) = self.radial.screen.take_shown() {
            self.radial.show(page);
        }
        events
    }
    /// What a press on a radial page's name does here.
    pub(super) fn name_events(
        &mut self,
        named: crate::radial_editor::Named,
        s: &mut Settings,
    ) -> Vec<Event> {
        use crate::radial_editor::Named;
        let mut events = vec![Event::ButtonClick];
        match named {
            Named::Nothing => return Vec::new(),
            Named::Done => {
                let entry = self.radial.screen.naming.take();
                if entry.is_some_and(|entry| entry.apply(&mut s.controller.radial)) {
                    self.radial.changed_elsewhere();
                }
                events.push(Event::Changed);
                events.extend(self.back(s));
            }
            Named::Back => events.extend(self.back(s)),
        }
        events
    }
    /// Where the shader screen opens for `page`, if it is one of its pages.
    pub(super) fn shader_page(page: Page) -> Option<crate::shader_ui::Tab> {
        match page {
            Page::ShaderPacks => Some(crate::shader_ui::Tab::Packs),
            Page::ShaderOptions => Some(crate::shader_ui::Tab::Screen(0)),
            _ => None,
        }
    }
    /// Where the controller screen opens for `page`, if it is one of its pages.
    pub(super) fn controller_page(page: Page) -> Option<crate::controller_ui::Page> {
        match page {
            Page::Controller => Some(crate::controller_ui::Page::General),
            Page::PadBinds => Some(crate::controller_ui::Page::Bindings),
            _ => None,
        }
    }
    /// Whether the controller screen is open: a controller button pressed now may bind.
    pub fn controller_open(&self) -> bool {
        Self::controller_page(self.page()).is_some()
    }
    /// The radial screens' glyphs this frame, and the name of a radial page being typed, drawn
    /// in place of a shell: whether it was.
    pub(super) fn draw_radial_name(
        &mut self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        cursor: (f32, f32),
        in_world: bool,
    ) -> bool {
        self.radial.screen.glyphs = self.pad_glyphs;
        if self.page() == Page::ControllerRadialName
            && let Some(entry) = &mut self.radial.screen.naming
        {
            entry.draw(batch, atlas, cursor, in_world);
            return true;
        }
        false
    }
    /// A click on a radial page's name entry (`ignored`: a drag or a right click).
    pub(super) fn click_radial_name(
        &mut self,
        s: &mut Settings,
        atlas: &HudAtlas,
        at: (f32, f32),
        size: (f32, f32),
        ignored: bool,
    ) -> Vec<Event> {
        let entry = self.radial.screen.naming.as_mut();
        let named = match entry.filter(|_| !ignored) {
            Some(entry) => entry.click(atlas, at, size),
            None => Named::Nothing,
        };
        self.name_events(named, s)
    }
    /// Whether a radial page's name is being typed.
    pub(super) fn radial_name_typing(&self) -> bool {
        self.radial
            .screen
            .naming
            .as_ref()
            .is_some_and(|n| n.typing())
    }
    /// Text typed into a radial page's name.
    pub(super) fn type_radial_name(&mut self, text: &str) {
        if let Some(entry) = &mut self.radial.screen.naming {
            entry.type_text(text);
        }
    }
    /// An edit shortcut in a radial page's name: whether it did something.
    pub(super) fn radial_name_shortcut(
        &mut self,
        shortcut: crate::clipboard::EditShortcut,
        clipboard: &mut dyn crate::clipboard::Clipboard,
    ) -> bool {
        let entry = self.radial.screen.naming.as_mut();
        entry.is_some_and(|entry| entry.shortcut(shortcut, clipboard))
    }
    /// A key on a radial page's name entry.
    pub(super) fn radial_name_key(
        &mut self,
        s: &mut Settings,
        key: winit::keyboard::KeyCode,
        text: Option<&str>,
    ) -> Vec<Event> {
        let named = match &mut self.radial.screen.naming {
            Some(entry) => entry.key(key, text),
            // Nothing to name: Escape still leaves.
            None if key == winit::keyboard::KeyCode::Escape => Named::Back,
            None => Named::Nothing,
        };
        self.name_events(named, s)
    }
}

impl Opt {
    /// The radial menu's rows of Controller Settings: their value.
    pub(super) fn radial_text(self, s: &Settings) -> Text {
        match self {
            Opt::PadRadialMode => tr(match s.controller.radial_mode {
                gamepad::RadialMode::Hold => "options.key.hold",
                gamepad::RadialMode::Toggle => "options.key.toggle",
            }),
            Opt::PadRadialStick => tr(match s.controller.radial_stick {
                gamepad::RadialStick::Right => "horizon.options.radial_stick.right",
                gamepad::RadialStick::Left => "horizon.options.radial_stick.left",
                gamepad::RadialStick::Either => "horizon.options.radial_stick.either",
            }),
            _ => Text::empty(),
        }
    }
    /// The radial menu's rows of Controller Settings: a step, back or on.
    pub(super) fn radial_step(self, s: &mut Settings, back: bool) {
        let next = |n: u8, count: u8| {
            if back {
                (n + count - 1) % count
            } else {
                (n + 1) % count
            }
        };
        match self {
            Opt::PadRadialMode => {
                let all = gamepad::RadialMode::ALL;
                let i = all
                    .iter()
                    .position(|m| *m == s.controller.radial_mode)
                    .unwrap_or(0) as u8;
                s.controller.radial_mode = all[usize::from(next(i, all.len() as u8))];
            }
            Opt::PadRadialStick => {
                let all = gamepad::RadialStick::ALL;
                let i = all
                    .iter()
                    .position(|k| *k == s.controller.radial_stick)
                    .unwrap_or(0) as u8;
                // An opener under the thumb that now selects goes unbound.
                s.controller
                    .set_radial_stick(all[usize::from(next(i, all.len() as u8))]);
            }
            _ => {}
        }
    }
}
