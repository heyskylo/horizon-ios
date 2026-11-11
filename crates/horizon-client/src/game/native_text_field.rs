//! Focused existing text boxes, without exposing a mutable UI or queued writes.
use super::*;
use crate::mod_api;
use horizon_client::{
    screen_draw::{Measure, TextField},
    ui::EditBox,
};

/// The field before a character is inserted. Cursor and selection anchor count
/// Unicode scalar characters, not UTF-8 bytes or UTF-16 code units. Host filters
/// and limits are read-only and apply to every replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeTextField {
    pub value: String,
    pub cursor: usize,
    pub highlight: usize,
    max_length: usize,
    multiline: bool,
}
impl NativeTextField {
    pub fn max_length(&self) -> usize {
        self.max_length
    }
    pub fn multiline(&self) -> bool {
        self.multiline
    }
    fn from_edit(edit: &EditBox) -> Self {
        Self {
            value: edit.value.clone(),
            cursor: edit.cursor,
            highlight: edit.highlight,
            max_length: edit.max_length,
            multiline: edit.multiline,
        }
    }
    fn from_text(edit: &TextField) -> Self {
        Self {
            value: edit.value.clone(),
            cursor: edit.cursor,
            highlight: edit.highlight,
            max_length: edit.max_length,
            multiline: false,
        }
    }
    fn apply_edit(&self, edit: &mut EditBox) {
        if self.value != edit.value {
            edit.set_cursor(0, false);
            edit.highlight = edit.value.chars().count();
            edit.insert_text(&self.value);
        }
        if self.cursor != edit.cursor || self.highlight != edit.highlight {
            edit.set_cursor(self.cursor, false);
            edit.highlight = self.highlight.min(edit.value.chars().count());
        }
    }
    fn apply_text(&self, edit: &mut TextField, measure: &Measure<'_>) {
        if self.value != edit.value {
            edit.move_cursor_to(measure, 0, false);
            edit.highlight = edit.value.chars().count();
            edit.insert_text(measure, &self.value);
        }
        if self.cursor != edit.cursor || self.highlight != edit.highlight {
            edit.move_cursor_to(measure, self.highlight, false);
            edit.move_cursor_to(measure, self.cursor, true);
        }
    }
}

fn edit_box(edit: &mut EditBox, replacement: Option<&NativeTextField>) -> Option<NativeTextField> {
    if !edit.focused {
        return None;
    }
    if let Some(field) = replacement {
        field.apply_edit(edit);
    }
    Some(NativeTextField::from_edit(edit))
}

impl Game<'_> {
    /// Specialized book/sign/dialog/rule, options and numeric editors retain
    /// their own validators; this hook does not expose those editors yet.
    fn focused_native_text_field(
        &mut self,
        replacement: Option<&NativeTextField>,
    ) -> Option<NativeTextField> {
        if !self.screen_hooks_open() {
            return None;
        }
        if control::title_owns_text(&self.screen, self.title.is_some()) {
            let mut result = None;
            self.title
                .as_mut()?
                .edit_focused_text_field(|edit| result = edit_box(edit, replacement));
            return result;
        }
        match self.screen {
            Screen::Chat | Screen::CommandBlock => {
                let measure = Measure(&self.gui.atlas.font);
                let commands = horizon_client::chat::Commands {
                    dispatcher: chat::chat_tree(&self.remote),
                    registries: &self.ui.registries,
                };
                let screen = if self.screen == Screen::Chat {
                    self.ui.chat_screen.as_mut()?
                } else {
                    &mut self.command_block_screen.as_mut()?.command
                };
                if !screen.input.focused {
                    return None;
                }
                let result = NativeTextField::from_text(&screen.input);
                if let Some(field) = replacement {
                    screen.edit_input(&measure, &commands, |edit| field.apply_text(edit, &measure));
                }
                Some(result)
            }
            Screen::Creative if self.creative_screen.tab().search && !self.creative_ignore_text => {
                let result = edit_box(&mut self.creative_screen.search, replacement);
                if replacement.is_some() {
                    self.creative_screen.scroll = 0.0;
                }
                result
            }
            Screen::Social => {
                let result = edit_box(&mut self.social.search, replacement);
                if replacement.is_some() {
                    self.social.scroll = 0.0;
                }
                result
            }
            Screen::DebugOptions => edit_box(&mut self.debug_options.as_mut()?.search, replacement),
            Screen::Inventory if self.mod_browser_typing() => {
                let result = edit_box(self.panel.search.as_mut()?, replacement);
                if replacement.is_some() {
                    self.mod_browser_text(None, false);
                }
                result
            }
            Screen::Inventory if self.recipe_search_focused() => {
                edit_box(self.recipe_ui.search.as_mut()?, replacement)
            }
            Screen::Inventory if self.station_typing() => {
                let result = edit_box(&mut self.station.as_mut()?.rename, replacement);
                if replacement.is_some() {
                    self.station_text(None, false);
                }
                result
            }
            _ => None,
        }
    }
    // Hookable source bodies cannot contain macros; diagnostics stay here.
    pub(super) fn report_text_input_result(result: Result<()>) {
        if let Err(error) = result {
            log::warn!("Horizon: text input: {error}");
        }
    }
    pub(super) fn type_text_field_input(&mut self, text: &str) -> Option<Result<()>> {
        if mod_api::mods().is_empty() {
            return None;
        }
        try_field_input(self, text)
    }
}

trait FieldTarget {
    fn owns_input(&self) -> bool;
    fn old_hook(&mut self, character: char) -> bool;
    fn field(&mut self, replacement: Option<&NativeTextField>) -> Option<NativeTextField>;
    fn hook(&mut self, field: &mut NativeTextField, character: char) -> bool;
    fn insert(&mut self, character: char) -> Result<()>;
}
fn try_field_input(target: &mut impl FieldTarget, text: &str) -> Option<Result<()>> {
    if !target.owns_input() || target.field(None).is_none() {
        return None;
    }
    Some(type_characters(target, text))
}
fn type_characters(target: &mut impl FieldTarget, text: &str) -> Result<()> {
    for character in text.chars() {
        if !target.owns_input() {
            return Ok(());
        }
        if !character.is_control() && target.old_hook(character) {
            continue;
        }
        let canceled =
            if let Some(mut field) = target.field(None).filter(|_| !character.is_control()) {
                let original = field.clone();
                let canceled = target.hook(&mut field, character);
                if field != original {
                    target.field(Some(&field));
                }
                canceled
            } else {
                false
            };
        if !canceled && target.owns_input() {
            target.insert(character)?;
        }
    }
    Ok(())
}
impl FieldTarget for Game<'_> {
    fn owns_input(&self) -> bool {
        self.screen_hooks_open()
    }
    fn old_hook(&mut self, character: char) -> bool {
        let Some(screen) = self.screen_view_now() else {
            return false;
        };
        let mut canceled = false;
        self.native_screen_char_typed_pre(
            &screen,
            character,
            super::glfw_modifiers(self.modifiers),
            &mut canceled,
        );
        canceled
    }
    fn field(&mut self, replacement: Option<&NativeTextField>) -> Option<NativeTextField> {
        self.focused_native_text_field(replacement)
    }
    fn hook(&mut self, field: &mut NativeTextField, character: char) -> bool {
        let Some(screen) = self.screen_view_now() else {
            return false;
        };
        let mut canceled = false;
        self.native_screen_text_field_char_typed_pre(
            &screen,
            field,
            character,
            super::glfw_modifiers(self.modifiers),
            &mut canceled,
        );
        canceled
    }
    fn insert(&mut self, character: char) -> Result<()> {
        self.type_chars_unhooked(&character.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_api::{
        Flow, Hooks, Host, NativeScreenView, ScreenCharTypedPreEvent,
        ScreenTextFieldCharTypedPreEvent,
    };
    use std::sync::{Arc, Mutex};

    struct HostStub;
    impl Host for HostStub {}
    struct Replacement(Arc<Mutex<Vec<String>>>);
    impl Hooks for Replacement {
        fn screen_char_typed_pre(
            &self,
            _: &dyn Host,
            event: &mut ScreenCharTypedPreEvent<'_>,
        ) -> Flow {
            self.0.lock().unwrap().push(format!("old:{}", event.c));
            if event.c == '?' {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn screen_text_field_char_typed_pre(
            &self,
            _: &dyn Host,
            event: &mut ScreenTextFieldCharTypedPreEvent<'_>,
        ) -> Flow {
            self.0.lock().unwrap().push(format!(
                "field:{}:{}:{}:{}",
                event.character, event.field.value, event.field.cursor, event.field.highlight
            ));
            if event.character == '!' {
                event.field.value = "a\u{1f600}b".into();
                event.field.cursor = 2;
                event.field.highlight = 2;
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }
    fn screen() -> NativeScreenView<'static> {
        NativeScreenView {
            id: "direct_connect",
            menu: None,
            title: None,
            rect: [0.0; 4],
            slots: 0,
            mouse: [0.0; 2],
            partial_tick: 0.0,
        }
    }
    struct TitleTarget {
        title: horizon_client::title::Title,
        mods: Vec<Box<dyn Hooks>>,
    }
    impl FieldTarget for TitleTarget {
        fn owns_input(&self) -> bool {
            true
        }
        fn old_hook(&mut self, character: char) -> bool {
            let mut canceled = false;
            mod_api::dispatch_screen_char_typed_pre(
                &HostStub,
                &self.mods,
                &screen(),
                character,
                0,
                &mut canceled,
            );
            canceled
        }
        fn field(&mut self, replacement: Option<&NativeTextField>) -> Option<NativeTextField> {
            let mut value = None;
            self.title
                .edit_focused_text_field(|edit| value = edit_box(edit, replacement));
            value
        }
        fn hook(&mut self, field: &mut NativeTextField, character: char) -> bool {
            let mut canceled = false;
            mod_api::dispatch_screen_text_field_char_typed_pre(
                &HostStub,
                &self.mods,
                &screen(),
                field,
                character,
                0,
                &mut canceled,
            );
            canceled
        }
        fn insert(&mut self, character: char) -> Result<()> {
            self.title.type_text(&character.to_string());
            Ok(())
        }
    }
    #[test]
    fn title_pipeline_keeps_old_cancellation_and_observes_each_real_insertion() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut title = horizon_client::title::Title::new(
            std::path::PathBuf::from("/nonexistent-native-field-fixture/saves"),
            std::path::Path::new("/nonexistent-native-field-fixture"),
        );
        title.mode =
            horizon_client::title::Mode::Multiplayer(horizon_client::multiplayer::Screen::Direct {
                address: "a\u{1f600}b".into(),
            });
        title.edit_focused_text_field(|edit| {
            edit.set_cursor(1, false);
            edit.highlight = 2;
        });
        let mut target = TitleTarget {
            title,
            mods: vec![Box::new(Replacement(events.clone()))],
        };
        try_field_input(&mut target, "x?!").unwrap().unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            [
                "old:x",
                "field:x:a\u{1f600}b:1:2",
                "old:?",
                "old:!",
                "field:!:axb:2:2"
            ]
        );
        let field = target.field(None).unwrap();
        assert_eq!(
            (field.value.as_str(), field.cursor, field.highlight),
            ("a\u{1f600}b", 2, 2)
        );
        target.title.mode = horizon_client::title::Mode::Main;
        events.lock().unwrap().clear();
        assert!(try_field_input(&mut target, "x").is_none());
        assert!(
            events.lock().unwrap().is_empty(),
            "unfocused input is left to the original handler"
        );
    }
    #[test]
    fn edit_box_replacements_keep_actual_limits_filtering_and_scalar_selection() {
        let mut edit = EditBox::new(0.0, 0.0, 100.0, 20.0);
        edit.max_length = 4;
        edit.set_value("a\u{1f600}b");
        let mut field = NativeTextField::from_edit(&edit);
        assert_eq!((field.max_length(), field.multiline()), (4, false));
        field.value = "a\n§\u{1f600}bcdef".into();
        field.cursor = usize::MAX;
        field.highlight = 1;
        field.max_length = usize::MAX;
        field.multiline = true;
        field.apply_edit(&mut edit);
        // maxLength is 4 UTF-16 code units: a, b and the emoji's two.
        assert_eq!(
            (edit.value.as_str(), edit.cursor, edit.highlight),
            ("a\u{1f600}b", 3, 1)
        );
        assert_eq!((edit.max_length, edit.multiline), (4, false));
        edit.focused = false;
        assert!(edit_box(&mut edit, None).is_none());
    }
    #[test]
    fn multiline_replacements_use_the_actual_edit_box_filter() {
        let mut edit = EditBox::new(0.0, 0.0, 100.0, 20.0);
        edit.multiline = true;
        edit.max_length = 5;
        let mut field = NativeTextField::from_edit(&edit);
        field.value = "a\nb§\0cdef".into();
        field.cursor = usize::MAX;
        field.highlight = usize::MAX;
        field.apply_edit(&mut edit);
        assert_eq!(edit.value, "a\nbcd");
        assert_eq!((edit.cursor, edit.highlight), (5, 5));
    }
    #[test]
    fn overlay_ownership_leaves_the_real_chat_and_callbacks_untouched() {
        use horizon_client::{
            chat::{ChatAbilities, ChatScreen, ChatVisibility, Commands},
            font::Font,
            hud::Rect,
        };
        let font = Font::load(
            std::path::Path::new("/nonexistent-native-overlay-font"),
            &Default::default(),
            &[],
            (1, 1),
            (0, 0),
            Rect {
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
                w: 1,
                h: 1,
            },
        );
        let registries = horizon_client::command::Registries::default();
        let commands = Commands {
            dispatcher: horizon_client::command::vanilla::game_dispatcher(),
            registries: &registries,
        };
        struct CoveredChat {
            chat: ChatScreen,
            calls: usize,
        }
        impl FieldTarget for CoveredChat {
            fn owns_input(&self) -> bool {
                false
            }
            fn old_hook(&mut self, _: char) -> bool {
                self.calls += 1;
                false
            }
            fn field(&mut self, _: Option<&NativeTextField>) -> Option<NativeTextField> {
                self.calls += 1;
                Some(NativeTextField::from_text(&self.chat.input))
            }
            fn hook(&mut self, _: &mut NativeTextField, _: char) -> bool {
                self.calls += 1;
                false
            }
            fn insert(&mut self, _: char) -> Result<()> {
                self.calls += 1;
                panic!("covered chat must not receive insertion")
            }
        }
        let mut target = CoveredChat {
            chat: ChatScreen::new(
                &Measure(&font),
                &commands,
                "underlying",
                false,
                0,
                640,
                360,
                ChatAbilities::new(ChatVisibility::Full),
            ),
            calls: 0,
        };
        let handled = try_field_input(&mut target, "x");
        assert!(
            handled.is_none(),
            "the overlay retains ownership of the fallback"
        );
        assert_eq!(target.calls, 0);
        assert_eq!(target.chat.value(), "underlying");
        let mut dialog_input = EditBox::new(0.0, 0.0, 100.0, 20.0);
        if handled.is_none() {
            dialog_input.insert_text("x");
        }
        assert_eq!(dialog_input.value, "x");
        assert_eq!(target.chat.value(), "underlying");
    }

    #[test]
    fn measured_text_field_replacements_use_its_real_filter_and_bounds() {
        use horizon_client::{font::Font, hud::Rect};
        let font = Font::load(
            std::path::Path::new("/nonexistent-native-field-font"),
            &Default::default(),
            &[],
            (1, 1),
            (0, 0),
            Rect {
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
                w: 1,
                h: 1,
            },
        );
        let measure = Measure(&font);
        let mut edit = TextField::new(0, 0, 100, 20);
        edit.max_length = 3;
        edit.focused = true;
        let mut field = NativeTextField::from_text(&edit);
        field.value = "§a\n\u{1f600}bc".into();
        field.cursor = 1;
        field.highlight = usize::MAX;
        field.apply_text(&mut edit, &measure);
        // maxLength 3 is three UTF-16 code units, so the emoji fills the
        // last two and the b past them is cut.
        assert_eq!(
            (edit.value.as_str(), edit.cursor, edit.highlight),
            ("a\u{1f600}", 1, 2)
        );
    }
}
