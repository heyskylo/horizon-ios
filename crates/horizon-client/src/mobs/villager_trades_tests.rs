use super::*;
use crate::items::ItemStack;
use crate::merchant::{ItemCost, MerchantOffer, Trader};
use std::sync::Arc;

#[test]
fn displayed_trade_items_have_no_drop_chance_and_clear_restores_it() {
    let ran = super::super::world::with_test_world(&[], |world, types| {
        let mut w = world.clone();
        let mut player = super::super::world::test_player(Vec3::ZERO);
        player.main_hand = Some("minecraft:emerald".into());
        w.player = Some(player);
        let mut m = Mob::new(
            1,
            Arc::new(types.types["minecraft:villager"].clone()),
            Vec3::ZERO,
            0.0,
        );
        let stack = |item: &str| ItemStack {
            item: Arc::from(item),
            count: 1,
            damage: 0,
            extra: None,
        };
        let offer = |item| MerchantOffer {
            base_cost_a: ItemCost {
                stack: stack("minecraft:emerald"),
            },
            cost_b: None,
            result: stack(item),
            uses: 0,
            max_uses: 10,
            reward_exp: true,
            special_price_diff: 0,
            demand: 0,
            price_multiplier: 0.0,
            xp: 0,
        };
        m.extra.trade = Some(Box::new(Trader {
            offers: vec![offer("minecraft:bread"), offer("minecraft:arrow")],
            ..Trader::default()
        }));
        mem(&mut m).interaction_target.set(Who::Player);
        let mut lr = LegacyRandom::new(1);
        let mut events = Vec::new();
        let mut cx = Bx {
            w: &w,
            lr: &mut lr,
            events: &mut events,
            now: 0,
            refresh: false,
        };
        let mut behavior = show_trades();
        assert!(behavior.try_start(&mut m, &mut cx));
        behavior.tick_or_stop(&mut m, &mut cx);
        assert_eq!(m.extra.main_hand.as_deref(), Some("minecraft:bread"));
        assert_eq!(super::super::drops::drop_chance(&m, "mainhand"), 0.0);
        for _ in 1..40 {
            behavior.tick_or_stop(&mut m, &mut cx);
        }
        assert_eq!(m.extra.main_hand.as_deref(), Some("minecraft:arrow"));
        assert_eq!(super::super::drops::drop_chance(&m, "mainhand"), 0.0);
        let mut no_match = w.clone();
        no_match.player.as_mut().unwrap().main_hand = Some("minecraft:stick".into());
        cx.w = &no_match;
        behavior.tick_or_stop(&mut m, &mut cx);
        assert!(m.extra.main_hand.is_none());
        assert_eq!(super::super::drops::drop_chance(&m, "mainhand"), 0.085);
        cx.w = &w;
        behavior.tick_or_stop(&mut m, &mut cx);
        behavior.do_stop(&mut m, &mut cx);
        assert!(m.extra.main_hand.is_none());
        assert_eq!(super::super::drops::drop_chance(&m, "mainhand"), 0.085);
    });
    // Without the vanilla data under .local/horizon-client the world
    // cannot be built: the test has nothing to check.
    let _ = ran;
}
