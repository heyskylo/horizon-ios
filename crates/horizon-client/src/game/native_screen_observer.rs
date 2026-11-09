//! The screen native mods watch (`screen_opening`, `screen_closing`,
//! `screen_init_post`): what is open this frame, compared with last frame's.
use super::*;

pub(in crate::game) struct ObservedScreen {
    id: &'static str,
    menu: Option<String>,
    title: Option<Text>,
    rect: [f32; 4],
    slots: usize,
    mouse: [f32; 2],
    partial_tick: f32,
}

impl ObservedScreen {
    fn view(&self) -> NativeScreenView<'_> {
        NativeScreenView {
            id: self.id,
            menu: self.menu.as_deref(),
            title: self.title.as_ref(),
            rect: self.rect,
            slots: self.slots,
            mouse: self.mouse,
            partial_tick: self.partial_tick,
        }
    }
}

pub(super) fn observed_overlay_id(overlays: &[(bool, &'static str)]) -> Option<&'static str> {
    overlays.iter().find_map(|(open, id)| open.then_some(*id))
}

impl Game<'_> {
    fn observed_screen_view(&self) -> Option<NativeScreenView<'_>> {
        let book = match &self.book {
            Some(super::books::BookScreen::Edit { signing: true, .. }) => Some("book_sign"),
            Some(super::books::BookScreen::Edit { .. }) => Some("book_edit"),
            Some(super::books::BookScreen::View {
                lectern: Some(_), ..
            }) => Some("lectern"),
            Some(super::books::BookScreen::View { .. }) => Some("book_view"),
            None => None,
        };
        let overlay = observed_overlay_id(&[
            (self.command_prompt.is_some(), "command_prompt"),
            (self.link_prompt.is_some(), "link_prompt"),
            (self.code_of_conduct.is_some(), "code_of_conduct"),
            (self.server_packs.prompt.is_some(), "server_pack_prompt"),
            (self.dialog_open(), "dialog"),
            (book.is_some(), book.unwrap_or("book_view")),
            (self.sign_edit.is_some(), "sign_editor"),
        ]);
        let Some(id) = overlay else {
            return self.screen_view_now();
        };
        let renderer = self.renderer.as_ref()?;
        let batch = HudBatch::new([renderer.config.width, renderer.config.height]);
        Some(NativeScreenView {
            id,
            menu: None,
            title: None,
            rect: [0.0, 0.0, batch.width, batch.height],
            slots: 0,
            mouse: self.native_cursor(),
            partial_tick: self.clock.fraction() as f32,
        })
    }

    pub(in crate::game) fn observe_screen(&mut self) {
        // Free with no native mod: the snapshot clones the title and menu.
        if mod_api::mods().is_empty() {
            return;
        }
        let current = self.observed_screen_view().map(|view| ObservedScreen {
            id: view.id,
            menu: view.menu.map(str::to_owned),
            title: view.title.cloned(),
            rect: view.rect,
            slots: view.slots,
            mouse: view.mouse,
            partial_tick: view.partial_tick,
        });
        let changed = match (&self.observed_screen, &current) {
            (Some(old), Some(new)) => old.id != new.id || old.menu != new.menu,
            (None, None) => false,
            _ => true,
        };
        if changed {
            if let Some(old) = &self.observed_screen {
                self.native_screen_closing(&old.view());
            }
            if let Some(new) = &current {
                self.native_screen_opening(&new.view());
                self.native_screen_init_post(&new.view());
            }
        } else if let (Some(old), Some(new)) = (&self.observed_screen, &current)
            && old.rect != new.rect
        {
            self.native_screen_init_post(&new.view());
        }
        self.observed_screen = current;
    }
}
