/// The game's `mod_api` types a hook names with a `crate::mod_api::` path.
#[cfg(test)]
mod mod_api {
    #[derive(Clone, Copy)]
    pub struct NativeInventoryKind;
}
#[cfg(test)]
mod tests {
    include!("block_clone_item_tests.rs");
    include!("state_block_entity_tests.rs");
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct ItemStack;
    struct StackView {
        count: i32,
    }
    struct BlockEntityDraw(Vec<u32>);
    struct NativeWorldDraw(Vec<u32>);
    #[allow(
        dead_code,
        reason = "stubs shaped like the game's, only some of them read"
    )]
    mod stubs {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub(super) struct BlockStateId;
        #[derive(Clone, Copy)]
        pub(super) struct FluidState;
        pub(super) struct PlacementContext;
        #[derive(Clone, Copy)]
        pub(super) struct BlockPos;
        #[derive(Clone, Copy)]
        pub(super) struct BlockId;
        #[derive(Clone, Copy)]
        pub(super) struct Direction;
        #[derive(Clone, Copy)]
        pub(super) struct LevelDimension;
        pub(super) struct Ticks;
        pub(super) trait LevelHost {}
        pub(super) trait LevelTicksHost: LevelHost {}
        pub(super) trait LevelRandomHost: LevelHost {}
        pub(super) trait LevelWriteHost: LevelTicksHost + LevelRandomHost {}
        pub(super) struct Compound;
        pub(super) type NativeDispensed = (Option<super::ItemStack>, i32);
        pub(super) struct NativeLootDrops<'a>(pub &'a ());
        pub(super) struct NativeNoteBlockChange<'a>(pub &'a ());
        pub(super) struct NativeScreenView<'a>(pub &'a ());
        pub(super) struct NativeTextField;
        pub(super) struct ClientboundPlayPacket;
        pub(super) struct EntityDataValue;
        pub(super) struct NativeGenerate;
        pub(super) struct NativeProtoChunk<'a>(pub &'a ());
        pub(super) struct NativeLodColumn<'a>(pub &'a ());
        pub(super) struct TickSpan;
        pub(super) struct Corners;
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub(super) struct ItemEnergyBuffer;
        pub(super) struct Suggestion;
        #[derive(Default)]
        pub(super) struct HudBatch(pub Vec<u32>);
        pub(super) struct BiomeColors;
        pub(super) struct SpawnEntry;
        pub(super) struct UseAnimation;
        #[derive(Clone, Copy)]
        pub(super) struct Hand;
        pub(super) struct NativeMobRender<'a>(pub &'a ());
        pub(super) struct LayerDefinition;
        pub(super) struct ModelPart;
        pub(super) struct NativeShake;
        pub(super) struct WorldPrimitives;
        pub(super) struct WorldHold;
        pub(super) struct Music;
        pub(super) struct MusicLayer;
        pub(super) struct NativeBlockRim;
        pub(super) struct NativeColoredLight;
        pub(super) struct NativeMinimap;
        pub(super) struct NativeMod;
        pub(super) struct NativePanelEvent;
        pub(super) struct NativeScreen;
        pub(super) struct NativeStack;
        pub(super) struct NativeRecipeLookup;
        pub(super) struct NativeRecipeCategory;
        pub(super) struct ModBlockEntity;
        pub(super) struct ModContainer;
        pub(super) struct MerchantOffer;
        pub(super) struct ModFluidTanks;
        #[derive(Clone, Copy)]
        pub(super) struct NativeFluidAction;
        pub(super) struct NativeFluidDrain;
        pub(super) struct NativeFluidStack;
        pub(super) struct NativeFluidTankView;
        pub(super) struct NativeFishingMedium;
        pub(super) struct ModEnergyBuffer;
        pub(super) struct Mob;
        pub(super) struct LootStack;
        pub(super) struct SoundPlayback<'a>(pub &'a ());
        pub(super) struct NativeLootContext<'a>(pub &'a ());
        pub(super) struct NativeDataMapType;
        pub(super) struct NativeConfig;
        pub(super) struct NativeConfigFile;
        pub(super) struct NativeReloadListener;
        pub(super) struct RecipeBookCategory;
        pub(super) struct NativeMapDecoration;
        pub(super) struct NativeMapInfo<'a>(pub &'a ());
        pub(super) struct NativePlayerRender<'a>(pub &'a ());
        pub(super) struct NativeCommand;
        pub(super) struct NativeLifecycleContext;
        pub(super) struct NativeServerPlayer;
        #[derive(Clone, Copy)]
        pub(super) struct ServerTickPhase;
        #[derive(Clone, Copy)]
        pub(super) struct WorldPhase;
    }
    use serde_json::{Map, Value};
    use stubs::*;
    struct Text;
    #[derive(Debug, PartialEq)]
    struct NativeZoom(u32);
    enum Flow {
        Continue,
        Cancel,
    }
    trait Host {}
    trait RenderHooks {}
    struct TestHost;
    impl Host for TestHost {}
    mod winit {
        pub mod event {
            pub struct KeyEvent;
        }
    }
    #[allow(
        dead_code,
        reason = "stubs shaped like the game's, only some of them read"
    )]
    mod generated {
        mod horizon_mods {
            pub mod content {
                pub struct Block;
                pub struct Item;
            }
        }
        mod horizon_client {
            pub mod camera {
                pub struct CameraEntity;
                pub struct FovPlayer;
            }
            pub mod menu {
                pub struct AnvilCraft;
            }
            pub mod fog {
                #[derive(Clone, Copy)]
                pub struct FogType;
                pub struct FogData;
            }
            pub mod first_person {
                #[derive(Clone, Copy)]
                pub struct Hand;
            }
            pub mod hud_layout {
                pub struct HealthInputs;
                pub struct GuiDraw;
            }
        }
        mod horizon_client_core {
            pub mod text {
                pub struct Text;
            }
        }
        mod horizon_protocol {
            pub mod packets {
                pub mod login {
                    pub struct Property;
                }
            }
        }
        include!(concat!(env!("OUT_DIR"), "/mod_api_generated.rs"));
    }
    use generated::*;
    thread_local! {
        /// The hooks each dispatch entered, a mod at a time.
        static ENTERED: std::cell::RefCell<Vec<(usize, &'static str)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    fn entered<'a>(
        mods: &'a [Box<dyn Hooks>],
        hook: &'static str,
    ) -> impl Iterator<Item = &'a Box<dyn Hooks>> {
        mods.iter().enumerate().map(move |(index, hooks)| {
            ENTERED.with(|entered| entered.borrow_mut().push((index, hook)));
            hooks
        })
    }
    struct Mod {
        id: u32,
        calls: Arc<Mutex<Vec<u32>>>,
    }
    impl Hooks for Mod {
        fn world_submit(&self, _: &dyn Host, event: &mut WorldSubmitEvent<'_>) {
            event.draw.0.push(self.id);
        }
        fn block_entity_submit(&self, _: &dyn Host, event: &mut BlockEntitySubmitEvent<'_>) {
            assert_eq!(event.pos, [1, 2, 3]);
            assert_eq!(event.block, "example:machine");
            event.draw.0.push(self.id);
        }
        fn living_entity_use_item_finish(
            &self,
            _: &mut dyn Host,
            event: &mut LivingEntityUseItemFinishEvent<'_>,
        ) {
            *event.result = None;
        }
        fn mouse_button_pre(&self, _: &mut dyn Host, _: &mut MouseButtonPreEvent) -> Flow {
            self.calls.lock().unwrap().push(self.id);
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn menu_slot_rules(&self, event: &mut MenuSlotRulesEvent<'_>) {
            event.may_place = false;
            event.max_stack = self.id;
        }
        fn living_entity_use_item_tick(
            &self,
            _: &mut dyn Host,
            event: &mut LivingEntityUseItemTickEvent<'_>,
        ) -> Flow {
            event.duration -= 1;
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn loot_table_load(&self, event: &mut LootTableLoadEvent<'_>) {
            event.table.as_array_mut().unwrap().push(self.id.into());
        }
        fn modify_default_components(&self, event: &mut ModifyDefaultComponentsEvent<'_>) {
            let value = event
                .components
                .entry("order")
                .or_insert_with(|| serde_json::json!([]));
            value.as_array_mut().unwrap().push(self.id.into());
        }
        fn level_tick_pre(&self, _: &mut dyn Host, _: &mut LevelTickPreEvent<'_>) {
            self.calls.lock().unwrap().push(self.id);
        }
        fn tooltip(&self, _: &dyn Host, event: &mut TooltipEvent<'_>) {
            event.lines.push((Text, self.id));
        }
        fn zoom(&self, _: &dyn Host, _: &mut ZoomEvent) -> Option<NativeZoom> {
            self.calls.lock().unwrap().push(self.id);
            Some(NativeZoom(self.id))
        }
        fn should_apply_effect_tick_this_tick(
            &self,
            _: &dyn Host,
            event: &mut ShouldApplyEffectTickThisTickEvent<'_>,
        ) {
            event.apply = !event.apply;
            self.calls.lock().unwrap().push(self.id);
        }
        fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
            event.amount = event.amount * 2.0 + self.id as f32;
            if self.id == 2 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
        fn use_item_on(&self, _: &mut dyn Host, _: &mut UseItemOnEvent<'_>) -> Flow {
            self.calls.lock().unwrap().push(self.id);
            if self.id == 1 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        }
    }
    #[test]
    fn world_submit_dispatches_in_order_with_shared_draw() {
        let mods: Vec<Box<dyn Hooks>> = vec![1, 2]
            .into_iter()
            .map(|id| {
                Box::new(Mod {
                    id,
                    calls: Arc::new(Mutex::new(Vec::new())),
                }) as Box<dyn Hooks>
            })
            .collect();
        let mut draw = NativeWorldDraw(Vec::new());
        dispatch_world_submit(&TestHost, &mods, &mut draw);
        assert_eq!(draw.0, [1, 2]);
    }

    #[test]
    fn block_entity_submit_dispatches_in_order_with_shared_draw() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<Box<dyn Hooks>> = vec![
            Box::new(Mod {
                id: 1,
                calls: calls.clone(),
            }),
            Box::new(Mod { id: 2, calls }),
        ];
        let mut draw = BlockEntityDraw(Vec::new());
        dispatch_block_entity_submit(
            &TestHost,
            &mods,
            [1, 2, 3],
            "example:machine",
            BlockStateId,
            &ModBlockEntity,
            &mut draw,
        );
        assert_eq!(draw.0, [1, 2]);
    }

    #[test]
    fn foreground_gui_preserves_draws_and_dispatches_with_visibility_flags() {
        struct Indicator(u32);
        impl Hooks for Indicator {
            fn gui_render_post(&self, _: &dyn Host, event: &mut GuiRenderPostEvent<'_>) {
                // A persistent indicator opts into F1 and hides on focus loss.
                if event.focused {
                    event
                        .batch
                        .0
                        .push(self.0 + u32::from(event.gui_hidden) * 10);
                }
            }
        }
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Indicator(1)), Box::new(Indicator(2))];
        for (hidden, focused, expected) in [
            (false, true, vec![99, 1, 2]),
            (true, true, vec![99, 11, 12]),
            (true, false, vec![99]),
        ] {
            let mut batch = HudBatch(vec![99]);
            dispatch_gui_render_post(&TestHost, &mods, &mut batch, hidden, focused);
            assert_eq!(batch.0, expected);
        }
        let mut batch = HudBatch(vec![99]);
        dispatch_gui_render_post(&TestHost, &[], &mut batch, true, true);
        assert_eq!(batch.0, [99]);
    }

    #[test]
    fn slot_rules_mutate_and_item_tick_can_cancel() {
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Mod {
            id: 1,
            calls: Arc::new(Mutex::new(Vec::new())),
        })];
        let (mut may_place, mut max_stack) = (true, 64);
        dispatch_menu_slot_rules(
            &mods,
            "test:menu",
            0,
            &ItemStack,
            &mut may_place,
            &mut max_stack,
        );
        assert_eq!((may_place, max_stack), (false, 1));
        let (mut duration, mut canceled) = (10, false);
        dispatch_living_entity_use_item_tick(
            &mut TestHost,
            &mods,
            &ItemStack,
            None,
            Hand,
            &mut duration,
            &mut canceled,
        );
        assert_eq!((duration, canceled), (9, true));
    }

    #[test]
    fn item_finish_can_clear_vanilla_result() {
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Mod {
            id: 1,
            calls: Arc::new(Mutex::new(Vec::new())),
        })];
        let mut result = Some(ItemStack);
        dispatch_living_entity_use_item_finish(
            &mut TestHost,
            &mods,
            &ItemStack,
            None,
            Hand,
            0,
            &mut result,
        );
        assert!(result.is_none());
    }

    #[test]
    fn fragment_cancel_stops_later_mods() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<Box<dyn Hooks>> = vec![
            Box::new(Mod {
                id: 1,
                calls: calls.clone(),
            }),
            Box::new(Mod {
                id: 2,
                calls: calls.clone(),
            }),
        ];
        let mut canceled = false;
        dispatch_mouse_button_pre(&mut TestHost, &mods, 0, true, 0, &mut canceled);
        assert!(canceled);
        assert_eq!(*calls.lock().unwrap(), [1]);
        // Each call is entered under the hook's name; the cancelled one's is the last.
        let entered = ENTERED.with(|entered| entered.take());
        assert_eq!(entered, [(0, "mouse_button_pre")]);
    }

    #[test]
    fn placement_replacement_can_refuse_and_stops_later_mods() {
        struct Level;
        impl LevelHost for Level {}
        struct Refuse(Arc<Mutex<Vec<u32>>>, u32);
        impl Hooks for Refuse {
            fn state_for_placement(
                &self,
                _: &dyn LevelHost,
                _: &mut StateForPlacementEvent<'_>,
            ) -> Option<Option<BlockStateId>> {
                self.0.lock().unwrap().push(self.1);
                Some(None)
            }
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<Box<dyn Hooks>> = vec![
            Box::new(Refuse(calls.clone(), 1)),
            Box::new(Refuse(calls.clone(), 2)),
        ];
        let mut answer = None;
        dispatch_state_for_placement(
            &Level,
            &mods,
            LevelDimension,
            BlockStateId,
            &PlacementContext,
            FluidState,
            &mut answer,
        );
        assert_eq!(answer, Some(None));
        assert_eq!(*calls.lock().unwrap(), vec![1]);
        dispatch_state_for_placement(
            &Level,
            &mods,
            LevelDimension,
            BlockStateId,
            &PlacementContext,
            FluidState,
            &mut answer,
        );
        assert_eq!(*calls.lock().unwrap(), vec![1]);
    }

    #[test]
    fn item_model_property_dispatches_borrowed_stack_and_preserves_unhandled_answers() {
        struct Property;
        impl Hooks for Property {
            fn item_model_property(&self, _: &dyn Host, event: &mut ItemModelPropertyEvent<'_>) {
                if event.property == "example:charged" {
                    event.value = event.stack.count > 1;
                }
            }
        }
        let mods: Vec<Box<dyn Hooks>> = vec![Box::new(Property)];
        let host = TestHost;
        let mut value = false;
        dispatch_item_model_property(
            &host,
            &mods,
            "example:charged",
            &StackView { count: 2 },
            &mut value,
        );
        assert!(value);
        dispatch_item_model_property(
            &host,
            &mods,
            "other:property",
            &StackView { count: 1 },
            &mut value,
        );
        assert!(value);
        dispatch_item_model_property(
            &host,
            &mods,
            "example:charged",
            &StackView { count: 1 },
            &mut value,
        );
        assert!(!value);
        dispatch_item_model_property(
            &host,
            &[],
            "example:charged",
            &StackView { count: 2 },
            &mut value,
        );
        assert!(!value);
    }

    #[test]
    fn dispatch_contracts() {
        struct Weapon;
        impl Hooks for Weapon {
            fn living_knockback_v2(&self, event: &mut LivingKnockbackV2Event<'_>) -> Flow {
                assert_eq!(
                    (event.entity, event.kind, event.attacker),
                    (12, "minecraft:armor_stand", 7)
                );
                assert!(event.attacker_main.is_none() && event.attacker_off.is_none());
                assert_eq!(event.strength, 0.4);
                event.strength *= 2.0;
                event.x = -event.x;
                Flow::Cancel
            }
        }
        let weapon_mods: Vec<Box<dyn Hooks>> = vec![Box::new(Weapon)];
        let (mut strength, mut x, mut z, mut canceled) = (0.4, 1.0, 0.0, false);
        dispatch_living_knockback_v2(
            &weapon_mods,
            12,
            "minecraft:armor_stand",
            7,
            None,
            None,
            &mut strength,
            &mut x,
            &mut z,
            &mut canceled,
        );
        assert_eq!((strength, x, z), (0.8, -1.0, 0.0));
        assert!(canceled);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<Box<dyn Hooks>> = (1..=2)
            .map(|id| {
                Box::new(Mod {
                    id,
                    calls: calls.clone(),
                }) as Box<dyn Hooks>
            })
            .collect();
        let mut table = serde_json::json!([]);
        dispatch_loot_table_load(&mods, "table", &mut table);
        assert_eq!(table, serde_json::json!([1, 2]));
        let mut components = Map::new();
        dispatch_modify_default_components(&mods, "item", &mut components);
        assert_eq!(components["order"], serde_json::json!([1, 2]));
        let mut host = TestHost;
        dispatch_level_tick_pre(&mut host, &mods, "overworld");
        assert_eq!(*calls.lock().unwrap(), [1, 2]);
        calls.lock().unwrap().clear();
        let mut zoom = None;
        dispatch_zoom(&host, &mods, &mut zoom);
        assert_eq!(zoom, Some(NativeZoom(1)));
        assert_eq!(*calls.lock().unwrap(), [1]);
        calls.lock().unwrap().clear();
        dispatch_zoom(&host, &mods, &mut zoom);
        assert!(calls.lock().unwrap().is_empty());
        let mut lines = Vec::new();
        dispatch_tooltip(&host, &mods, &ItemStack, &mut lines);
        assert_eq!(lines.iter().map(|line| line.1).collect::<Vec<_>>(), [1, 2]);
        let mut apply = false;
        calls.lock().unwrap().clear();
        dispatch_should_apply_effect_tick_this_tick(&host, &mods, "effect", 0, 0, &mut apply);
        assert!(!apply);
        assert_eq!(*calls.lock().unwrap(), [1, 2]);
        let mut amount = 1.0;
        let mut canceled = false;
        dispatch_living_incoming_damage(
            &mods,
            0,
            "kind",
            "damage",
            0,
            0,
            &mut amount,
            &mut canceled,
        );
        // Each mod sees the one before: (1 × 2 + 1) × 2 + 2, kept though mod 2 cancels.
        assert_eq!(amount, 8.0);
        assert!(canceled);
        let chained: Vec<Box<dyn Hooks>> = [1, 3]
            .map(|id| {
                Box::new(Mod {
                    id,
                    calls: calls.clone(),
                }) as Box<dyn Hooks>
            })
            .into();
        let mut amount = 1.0;
        let mut canceled = false;
        dispatch_living_incoming_damage(
            &chained,
            0,
            "kind",
            "damage",
            0,
            0,
            &mut amount,
            &mut canceled,
        );
        assert_eq!(amount, 9.0);
        assert!(!canceled);
        calls.lock().unwrap().clear();
        let mut claimed = false;
        dispatch_use_item_on(&mut host, &mods, &ItemStack, [0; 3], [0; 3], &mut claimed);
        assert!(claimed);
        assert_eq!(*calls.lock().unwrap(), [1]);
        calls.lock().unwrap().clear();
        dispatch_use_item_on(&mut host, &mods, &ItemStack, [0; 3], [0; 3], &mut claimed);
        assert!(calls.lock().unwrap().is_empty());
    }
}
