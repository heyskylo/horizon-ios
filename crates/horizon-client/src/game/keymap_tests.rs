use super::{follow_keymap, follow_named_keymap};
use horizon_client::{
    debug_keys::{DebugAction, DebugHost, DebugKeys, KeyAction, OpenScreen},
    keybinds::{self, KeyBinds},
};
use std::collections::HashSet;
use winit::{
    event::WindowEvent,
    keyboard::{Key, KeyCode, NamedKey, NativeKeyCode, PhysicalKey},
};

// KeyEvent has private platform fields. Exercise the production mapping with its actual public
// winit key fields, without creating a window or sending operating-system keyboard events.
#[test]
fn xkb_f3_reaches_debug_press_repeat_release_and_binding_capture() {
    struct Host(u32);
    impl DebugHost for Host {
        fn screen(&self) -> OpenScreen {
            OpenScreen::None
        }
        fn toggle_overlay(&mut self) {
            self.0 += 1;
        }
        fn pause_game(&mut self, _: bool) {
            panic!("F3 alone must not pause");
        }
        fn debug_action(&mut self, _: DebugAction, _: bool) -> bool {
            false
        }
    }
    let mut host = Host(0);
    let mut debug = DebugKeys::default();
    let binds = KeyBinds::default();
    let mut held = HashSet::new();
    for (action, overlays, modifier) in [
        (KeyAction::Press, 0, true),
        (KeyAction::Repeat, 0, true),
        (KeyAction::Release, 1, false),
    ] {
        let mut physical = PhysicalKey::Code(KeyCode::BrowserRefresh);
        follow_named_keymap(&Key::Named(NamedKey::F3), &mut physical);
        assert_eq!(
            physical,
            PhysicalKey::Code(KeyCode::F3),
            "XKB's F3 must override the media-key position"
        );
        let PhysicalKey::Code(code) = physical else {
            panic!("resolved F3 is a known key")
        };
        let name = keybinds::key_name(code).unwrap();
        assert_eq!(
            name, "key.keyboard.f3",
            "the binding editor must receive F3 too"
        );
        if action == KeyAction::Release {
            held.remove(&code);
        } else {
            held.insert(code);
        }
        debug.key_press(&mut host, &binds, name, action, false, false, 0);
        assert_eq!(host.0, overlays, "repeats must not toggle the overlay");
        assert_eq!(debug.modifier_down, modifier);
    }
    assert!(
        held.is_empty(),
        "the remapped release must clear the held key"
    );
}

#[test]
fn mapped_function_keys_keep_their_binding_names() {
    let keys = [
        NamedKey::F1,
        NamedKey::F2,
        NamedKey::F3,
        NamedKey::F4,
        NamedKey::F5,
        NamedKey::F6,
        NamedKey::F7,
        NamedKey::F8,
        NamedKey::F9,
        NamedKey::F10,
        NamedKey::F11,
        NamedKey::F12,
        NamedKey::F13,
        NamedKey::F14,
        NamedKey::F15,
        NamedKey::F16,
        NamedKey::F17,
        NamedKey::F18,
        NamedKey::F19,
        NamedKey::F20,
        NamedKey::F21,
        NamedKey::F22,
        NamedKey::F23,
        NamedKey::F24,
    ];
    for (index, named) in keys.into_iter().enumerate() {
        let mut physical = PhysicalKey::Code(KeyCode::BrowserBack);
        follow_named_keymap(&Key::Named(named), &mut physical);
        let PhysicalKey::Code(code) = physical else {
            panic!("a function key has a code")
        };
        assert_eq!(
            keybinds::key_name(code),
            Some(format!("key.keyboard.f{}", index + 1).as_str())
        );
    }
}

#[test]
fn ordinary_layout_keys_and_existing_escape_caps_remaps_are_preserved() {
    for (logical, before, expected) in [
        (Key::Character("z".into()), KeyCode::KeyW, KeyCode::KeyW),
        (Key::Character("F3".into()), KeyCode::KeyA, KeyCode::KeyA),
        (Key::Dead(Some('´')), KeyCode::Quote, KeyCode::Quote),
        (
            Key::Named(NamedKey::ArrowLeft),
            KeyCode::ArrowLeft,
            KeyCode::ArrowLeft,
        ),
        (
            Key::Named(NamedKey::Escape),
            KeyCode::CapsLock,
            KeyCode::Escape,
        ),
        (
            Key::Named(NamedKey::CapsLock),
            KeyCode::Escape,
            KeyCode::CapsLock,
        ),
    ] {
        let mut physical = PhysicalKey::Code(before);
        follow_named_keymap(&logical, &mut physical);
        assert_eq!(physical, PhysicalKey::Code(expected));
    }
    let mut unidentified = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);
    follow_named_keymap(&Key::Character("é".into()), &mut unidentified);
    assert_eq!(
        unidentified,
        PhysicalKey::Unidentified(NativeKeyCode::Unidentified)
    );
    let mut focused = WindowEvent::Focused(false);
    follow_keymap(&mut focused);
    assert!(matches!(focused, WindowEvent::Focused(false)));
}
