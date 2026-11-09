//! Constructor-only entity categories and production gateway state; no Game/world/server.
use crate::game::{
    boats::Boat,
    entity::Entity,
    minecarts::{CartKind, Minecart},
};
use horizon_client::{
    end_gateway::Traveller,
    projectiles::{Projectile, ProjectileKind},
};
use horizon_core::{geometry::Vec3, position::BlockPos};

#[test]
#[ignore = "needs explicit HORIZON_PORTAL_REFERENCE"]
fn gateway_counters_match_actual_vanilla_methods() {
    let reference: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var_os("HORIZON_PORTAL_REFERENCE").unwrap()).unwrap(),
    )
    .unwrap();
    let boat = Boat::new(1, "oak".into(), Vec3::ZERO, 0.0);
    let cart = Minecart::new(2, CartKind::Rideable, Vec3::ZERO);
    let arrow = Projectile::new(
        ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        },
        Vec3::ZERO,
        None,
    );
    let delays = [
        boat.dimension_changing_delay(),
        cart.dimension_changing_delay(),
        arrow.dimension_changing_delay(),
        horizon_client::nether_portal::ENTITY_DELAY,
    ];
    assert_eq!(serde_json::json!(delays), reference["delays"]);
    let mut travellers: [Traveller; 4] = std::array::from_fn(|_| Traveller::default());
    let entry = BlockPos::new(0, 100, 0);
    for (traveller, delay) in travellers.iter_mut().zip(delays) {
        traveller.enter_with_delay(entry, delay);
        assert_eq!(traveller.travel_with_delay(true, delay), Some(entry));
        assert_eq!(traveller.cooldown(), delay);
        traveller.set_cooldown(5);
    }
    let mut rows = Vec::new();
    for (step, ticks) in [1, 3, 1, 1, 8].into_iter().enumerate() {
        for _ in 0..ticks {
            for (i, (traveller, delay)) in travellers.iter_mut().zip(delays).enumerate() {
                if step < 2 && i == 2 {
                    traveller.enter_with_delay(entry, delay);
                }
                traveller.travel_with_delay(true, delay);
                if step < 2 && i != 2 {
                    traveller.enter_with_delay(entry, delay);
                }
            }
        }
        rows.push(
            travellers
                .iter()
                .map(Traveller::cooldown)
                .collect::<Vec<_>>(),
        );
    }
    let report = serde_json::json!({"contact":rows});
    assert_eq!(report["contact"], reference["contact"]);
    if let Some(path) = std::env::var_os("HORIZON_PORTAL_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
