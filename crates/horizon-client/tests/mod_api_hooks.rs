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

#[test]
fn a_tank_hook_can_refuse_a_fluid() {
    use horizon_client::mod_api::{
        BlockEntityAcceptsFluidEvent, Flow, dispatch_block_entity_accepts_fluid,
    };
    struct TestHost;
    impl Host for TestHost {}
    struct WaterTank;
    impl Hooks for WaterTank {
        fn block_entity_accepts_fluid(
            &self,
            _host: &dyn Host,
            event: &mut BlockEntityAcceptsFluidEvent<'_>,
        ) -> Flow {
            if event.block == "mymod:water_tank" && event.fluid != "minecraft:water" {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }
    let mut cancel = false;
    dispatch_block_entity_accepts_fluid(
        &TestHost,
        &[],
        "mymod:water_tank",
        0,
        "create:chocolate",
        &mut cancel,
    );
    assert!(!cancel, "every tank takes every fluid without a mod");
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(WaterTank)];
    let mut cancel = false;
    dispatch_block_entity_accepts_fluid(
        &TestHost,
        &mods,
        "mymod:water_tank",
        0,
        "create:chocolate",
        &mut cancel,
    );
    assert!(cancel, "the water tank refuses the mod's fluid");
    let mut cancel = false;
    dispatch_block_entity_accepts_fluid(
        &TestHost,
        &mods,
        "mymod:water_tank",
        0,
        "minecraft:water",
        &mut cancel,
    );
    assert!(!cancel, "the water tank takes water");
}

#[test]
fn tooltip_render_pre_intercepts_rendering_and_draws_gui_items() {
    use horizon_client::hud::HudBatch;
    use horizon_client::items::ItemStack;
    use horizon_client::mod_api::{
        Flow, KeyCode, TooltipRenderPreEvent, dispatch_tooltip_render_pre,
    };
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    struct TestHost {
        draw_count: AtomicU32,
        with_text_seen: AtomicBool,
    }
    impl Host for TestHost {
        fn gui_draw_item(&self, _batch: &mut HudBatch, stack: &ItemStack, at: [f32; 2]) {
            if &*stack.item == "minecraft:shulker_box" && at == [16.0, 32.0] {
                self.draw_count.fetch_add(1, Ordering::SeqCst);
            }
        }
        fn gui_draw_item_with(
            &self,
            _batch: &mut HudBatch,
            stack: &ItemStack,
            at: [f32; 2],
            text: Option<&str>,
        ) {
            if &*stack.item == "minecraft:diamond" && at == [20.0, 40.0] && text == Some("5") {
                self.with_text_seen.store(true, Ordering::SeqCst);
            }
        }
        fn has_shift_down(&self) -> bool {
            true
        }
        fn has_control_down(&self) -> bool {
            false
        }
        fn has_alt_down(&self) -> bool {
            false
        }
        fn is_key_down(&self, key: KeyCode) -> bool {
            key == KeyCode::ShiftLeft
        }
    }

    struct ShulkerPreviewMod;
    impl Hooks for ShulkerPreviewMod {
        fn tooltip_render_pre(
            &self,
            host: &mut dyn Host,
            event: &mut TooltipRenderPreEvent<'_>,
        ) -> Flow {
            if &*event.stack.item == "minecraft:shulker_box"
                && host.has_shift_down()
                && host.is_key_down(KeyCode::ShiftLeft)
            {
                host.gui_draw_item(event.batch, event.stack, [event.x, event.y]);
                let inner = ItemStack::new("minecraft:diamond", 5);
                host.gui_draw_item_with(event.batch, &inner, [20.0, 40.0], Some("5"));
                return Flow::Cancel;
            }
            Flow::Continue
        }
    }

    let mut host = TestHost {
        draw_count: AtomicU32::new(0),
        with_text_seen: AtomicBool::new(false),
    };
    let mods: Vec<Box<dyn Hooks>> = vec![Box::new(ShulkerPreviewMod)];
    let shulker = ItemStack::new("minecraft:shulker_box", 1);
    assert_eq!(shulker.container().len(), 0);
    assert!(shulker.custom_data().is_none());

    let mut batch = HudBatch::new([800, 600]);
    let mut cancel = false;
    dispatch_tooltip_render_pre(
        &mut host,
        &mods,
        &shulker,
        &mut batch,
        16.0,
        32.0,
        &mut cancel,
    );

    assert!(cancel, "tooltip_render_pre cancelled vanilla tooltip");
    assert_eq!(
        host.draw_count.load(Ordering::SeqCst),
        1,
        "gui_draw_item was called"
    );
    assert!(
        host.with_text_seen.load(Ordering::SeqCst),
        "gui_draw_item_with was called"
    );

    let stone = ItemStack::new("minecraft:stone", 64);
    let mut cancel_stone = false;
    dispatch_tooltip_render_pre(
        &mut host,
        &mods,
        &stone,
        &mut batch,
        16.0,
        32.0,
        &mut cancel_stone,
    );
    assert!(
        !cancel_stone,
        "unhandled stack continues to vanilla tooltip"
    );
}
