use super::*;
use horizon_client::arrows::{ArrowEvent, deflect_reverse};
use horizon_core::random::LegacyRandom;

/// `AbstractArrow.onHitEntity` after `hurtOrSimulate` fails and the target
/// still takes a projectile's side effects: `deflect(REVERSE, …, 0.2)`,
/// and no hit sound. A closed shulker's refusal is that miss
/// (`shulker::tests`). An enderman's teleport is `Dodged`, not this.
#[test]
fn a_refused_arrow_reverses_without_a_hit_sound() {
    assert_eq!(arrow_miss(false), EntityHit::Refused);
    assert_eq!(arrow_miss(true), EntityHit::Dodged);

    let mut arrow = Projectile::new(
        ProjectileKind::Arrow {
            base_damage: 2.0,
            critical: false,
            effect: None,
        },
        Vec3::new(0.0, 1.0, 0.0),
        Some(Who::Player),
    )
    .with_arrow_random(LegacyRandom::new(7));
    arrow.velocity = Vec3::new(3.0, -1.0, 2.0);
    arrow.yaw = 15.0;
    let mut oracle = LegacyRandom::new(7);
    let expect_yaw = 15.0 + 170.0 + oracle.next_float() * 20.0;
    let mut state = arrow.arrow.take().expect("an arrow");
    deflect_reverse(&mut arrow.velocity, &mut arrow.yaw, &mut state);
    // power 0.2, and REVERSE multiplies by -power * 0.5.
    assert!((arrow.velocity.x - -0.3).abs() < 1e-6);
    assert!((arrow.velocity.y - 0.1).abs() < 1e-6);
    assert!((arrow.velocity.z - -0.2).abs() < 1e-6);
    assert!((arrow.yaw - expect_yaw).abs() < 1e-4);
    assert!(!state.removed, "the reversed arrow stays in the world");
    assert!(
        state
            .events
            .iter()
            .all(|e| !matches!(e, ArrowEvent::Sound { .. })),
        "the deflect branch plays no hit sound"
    );
}
