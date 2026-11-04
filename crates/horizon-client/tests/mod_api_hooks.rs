//! A mod crate's view of the mod API: `Hooks` is implemented with the names `mod_api` exports.
//! The test is that this file builds; #7044's mod could not name `key`'s event.
use horizon_client::mod_api::{
    Hooks, Host, InputKeyEvent, KeyEvent, MovementInputEvent, dispatch_movement_input, winit,
};

struct Keys;

impl Hooks for Keys {
    fn key(&self, _host: &mut dyn Host, event: &mut KeyEvent<'_>) {
        let _ = (event.key, event.pressed);
    }

    fn input_key(&self, _host: &mut dyn Host, event: &mut InputKeyEvent<'_>) {
        let _: &winit::event::KeyEvent = event.event;
    }
}

#[test]
fn a_mod_implements_the_key_hooks() {
    let _: Box<dyn Hooks> = Box::new(Keys);
}

#[test]
fn movement_input_writes_back_all_fields() {
    struct TestHost;
    impl Host for TestHost {}
    struct Movement;
    impl Hooks for Movement {
        fn movement_input(&self, _host: &dyn Host, event: &mut MovementInputEvent) {
            event.forward = -0.5;
            event.strafe = 0.25;
            event.jump = true;
            event.sneak = true;
        }
    }
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Movement)];
    let (mut forward, mut strafe, mut jump, mut sneak) = (1.0, 0.0, false, false);
    dispatch_movement_input(
        &TestHost,
        &mods,
        &mut forward,
        &mut strafe,
        &mut jump,
        &mut sneak,
    );
    assert_eq!((forward, strafe, jump, sneak), (-0.5, 0.25, true, true));
}

#[test]
fn recipe_book_categories_accept_mod_declarations() {
    use horizon_client::mod_api::{
        RecipeBookCategoriesEvent, RecipeBookCategory, dispatch_recipe_book_categories,
    };
    struct TestHost;
    impl Host for TestHost {}
    struct Categories;
    impl Hooks for Categories {
        fn recipe_book_categories(
            &self,
            _host: &dyn Host,
            event: &mut RecipeBookCategoriesEvent<'_>,
        ) {
            event.categories.push(RecipeBookCategory {
                id: "mymod:crushing".into(),
                recipe_type: "mymod:crusher".into(),
                icon: "minecraft:stone".into(),
            });
        }
    }
    let mut declarations = Vec::new();
    dispatch_recipe_book_categories(&TestHost, &[], &mut declarations);
    assert!(declarations.is_empty());
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Categories)];
    dispatch_recipe_book_categories(&TestHost, &mods, &mut declarations);
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].id, "mymod:crushing");
    assert_eq!(declarations[0].recipe_type, "mymod:crusher");
    assert_eq!(declarations[0].icon, "minecraft:stone");
}

#[test]
fn received_chat_is_replaced_in_order_and_cancel_stops_later_mods() {
    use horizon_client::mod_api::{ClientChatReceivedEvent, Flow, dispatch_client_chat_received};
    use horizon_client::text::Text;
    struct TestHost;
    impl Host for TestHost {}
    struct Tag;
    impl Hooks for Tag {
        fn client_chat_received(
            &self,
            _host: &mut dyn Host,
            event: &mut ClientChatReceivedEvent<'_>,
        ) -> Flow {
            if event.message.plain().contains("drop") {
                return Flow::Cancel;
            }
            let who = if event.system { "server" } else { "player" };
            *event.message = Text::literal(format!("[{who}] {}", event.message.plain()));
            Flow::Continue
        }
    }
    struct Shout;
    impl Hooks for Shout {
        fn client_chat_received(
            &self,
            _host: &mut dyn Host,
            event: &mut ClientChatReceivedEvent<'_>,
        ) -> Flow {
            *event.message = Text::literal(event.message.plain().to_uppercase());
            Flow::Continue
        }
    }
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Tag), Box::new(Shout)];
    let receive = |system: bool, text: &str| {
        let (mut message, mut cancel) = (Text::literal(text), false);
        let chat_type = (!system).then_some("minecraft:chat");
        dispatch_client_chat_received(
            &mut TestHost,
            &mods,
            system,
            false,
            7,
            chat_type,
            &mut message,
            &mut cancel,
        );
        (message.plain(), cancel)
    };
    assert_eq!(receive(true, "hi"), ("[SERVER] HI".to_owned(), false));
    assert_eq!(receive(false, "hi"), ("[PLAYER] HI".to_owned(), false));
    // A cancelled line keeps its text: the second mod never ran.
    assert_eq!(receive(false, "drop me"), ("drop me".to_owned(), true));
}

#[test]
fn client_packet_hands_a_mod_the_decoded_packet() {
    use horizon_client::mod_api::{
        ClientPacketEvent, ClientboundPlayPacket, dispatch_client_packet,
    };
    use horizon_protocol::packets::configuration::{CustomPayload, Ping};
    use std::sync::Mutex;
    static HEARD: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());
    struct TestHost;
    impl Host for TestHost {}
    // A plugin-channel listener, matching the packet by the name `mod_api` exports.
    struct Channels;
    impl Hooks for Channels {
        fn client_packet(&self, _host: &mut dyn Host, event: &mut ClientPacketEvent<'_>) {
            if let ClientboundPlayPacket::CustomPayload(p) = event.packet {
                HEARD
                    .lock()
                    .unwrap()
                    .push((p.channel.clone(), p.data.clone()));
            }
        }
    }
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Channels)];
    dispatch_client_packet(
        &mut TestHost,
        &mods,
        &ClientboundPlayPacket::Ping(Ping { id: 7 }),
    );
    let payload = CustomPayload {
        channel: "voicechat:secret".into(),
        data: vec![1, 2, 3],
    };
    dispatch_client_packet(
        &mut TestHost,
        &mods,
        &ClientboundPlayPacket::CustomPayload(payload),
    );
    assert_eq!(
        *HEARD.lock().unwrap(),
        [("voicechat:secret".to_string(), vec![1, 2, 3])]
    );
}
