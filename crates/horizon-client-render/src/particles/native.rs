//! Opt-in, compiled particle behavior. The ordinary engine still owns the
//! particle, its sprite set, interpolation, rendering and total count limits.
use super::{Particle, ParticleLevel, ParticleOptions};
use std::fmt::Debug;

/// Requests made by one particle tick, submitted through normal spawn limits.
#[derive(Default)]
pub struct ParticleUpdate {
    /// An index in this particle's declared sprite set. Out-of-range is clamped.
    pub frame: Option<usize>,
    pub spawned: Vec<(ParticleOptions, [f64; 3], [f64; 3])>,
}

/// Owned behavior for a compiled native particle. Cloning a particle clones its
/// controller state. Do not retain world references; inspect `level` each tick.
pub trait ParticleController: Clone + Debug + Send + Sync + 'static {
    /// Call `particle.advance_age()` for ordinary lifetime/interpolation, or
    /// explicitly maintain those fields for an overridden source tick.
    fn tick(&mut self, particle: &mut Particle, level: &ParticleLevel<'_>) -> ParticleUpdate;
}

pub(super) trait Controller: Debug + Send + Sync {
    fn tick(&mut self, particle: &mut Particle, level: &ParticleLevel<'_>) -> ParticleUpdate;
    fn clone_box(&self) -> Box<dyn Controller>;
}
impl<T: ParticleController> Controller for T {
    fn tick(&mut self, particle: &mut Particle, level: &ParticleLevel<'_>) -> ParticleUpdate {
        ParticleController::tick(self, particle, level)
    }
    fn clone_box(&self) -> Box<dyn Controller> {
        Box::new(self.clone())
    }
}
impl Clone for Box<dyn Controller> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use horizon_world::{
        DATA_VERSION,
        collision::CachedCollisionRegistry,
        nbt::Tag,
        registry::{BlockRegistry, BlockStateId},
    };
    #[derive(Clone, Debug)]
    struct Moving(u32);
    impl ParticleController for Moving {
        fn tick(&mut self, p: &mut Particle, _: &ParticleLevel<'_>) -> ParticleUpdate {
            if p.advance_age() {
                self.0 += 1;
                p.position[0] += f64::from(self.0);
            }
            ParticleUpdate {
                frame: Some(self.0 as usize),
                ..Default::default()
            }
        }
    }
    fn engine() -> ParticleEngine {
        let mut e = ParticleEngine::new(Path::new(""), &HashMap::new(), &HashSet::new());
        e.mod_sets.insert("example:orbit".into(), (0, None));
        e.set_sprites.push(vec![0, 1]);
        e.rects = vec![
            Some(Rect {
                u0: 0.0,
                v0: 0.0,
                u1: 1.0,
                v1: 1.0,
                w: 16,
                h: 16
            });
            2
        ];
        e
    }
    #[test]
    fn controlled_particle_keeps_owned_motion_sprite_interpolation_and_expiry() {
        let registry = BlockRegistry::from_nbt(&Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            (
                "states",
                Tag::List(vec![Tag::String("minecraft:air".into())]),
            ),
            ("defaults", Tag::compound([("minecraft:air", Tag::Int(0))])),
        ]))
        .unwrap();
        let cached = CachedCollisionRegistry::from_nbt(
            &Tag::compound([
                ("DataVersion", Tag::Int(DATA_VERSION)),
                (
                    "states",
                    Tag::List(vec![Tag::String("minecraft:air".into())]),
                ),
                ("shapes", Tag::List(vec![])),
                ("cached_shapes", Tag::IntArray(vec![-1])),
            ]),
            &registry,
        )
        .unwrap();
        let collision = BlockCollisionRegistry::new(std::sync::Arc::new(cached)).unwrap();
        let facts = BlockFacts {
            states: vec![],
            air: BlockStateId(0),
            woods: HashMap::new(),
        };
        let world = World::new(BlockStateId(0), 0);
        let level = ParticleLevel {
            world: &world,
            facts: &facts,
            collision: &collision,
            player: None,
        };
        let mut e = engine();
        let mut p = e
            .controlled("example:orbit", [0.0; 3], true, Moving(0))
            .unwrap();
        p.lifetime = 2;
        p.rgba[3] = 0.5;
        p.velocity = [99.0; 3]; // no vanilla drift may leak into the custom tick
        let mut copy = p.clone();
        e.tick_particle(&mut p, &level);
        e.tick_particle(&mut p, &level);
        e.tick_particle(&mut copy, &level);
        assert_eq!((p.position, p.previous), ([3.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        assert_eq!(copy.position, [1.0, 0.0, 0.0]); // controller state cloned independently
        assert!(matches!(p.sprite, Sprite::Index(1))); // clamped frame
        p.size = 0.1;
        e.particles.push(p.clone());
        let mut vertices = Vec::new();
        e.render(DVec3::ZERO, 0.0, 0.0, 0.5, &|_| 0, &mut vertices);
        assert_eq!(vertices.len(), 4);
        assert!(vertices.iter().all(
            |v| v.color >> 24 == 127 && v.layer & crate::gfx::TRANSLUCENT_PARTICLE_OVERLAY != 0
        ));
        e.particles[0].set_visible(false);
        vertices.clear();
        e.render(DVec3::ZERO, 0.0, 0.0, 0.5, &|_| 0, &mut vertices);
        assert!(vertices.is_empty());
        assert_eq!(e.particles[0].age, 2);
        e.pending.push(copy);
        e.reload(Path::new(""), &HashMap::new(), &HashSet::new());
        assert!(e.pending.is_empty());
        vertices.clear();
        e.render(DVec3::ZERO, 0.0, 0.0, 0.5, &|_| 0, &mut vertices);
        assert!(vertices.is_empty());
        e.tick_particle(&mut p, &level);
        assert!(p.removed);
        assert_eq!(p.position, [3.0, 0.0, 0.0]);
    }
    #[test]
    fn controlled_spawn_respects_distance_settings_and_declared_sprite_sets() {
        let mut e = engine();
        assert!(
            e.controlled("example:missing", [0.0; 3], true, Moving(0))
                .is_none()
        );
        assert!(
            e.controlled("example:orbit", [33.0, 0.0, 0.0], true, Moving(0))
                .is_none()
        );
        assert!(
            e.controlled("example:orbit", [f64::NAN, 0.0, 0.0], true, Moving(0))
                .is_none()
        );
        assert!(
            e.controlled("example:orbit", [32.0, 0.0, 0.0], true, Moving(0))
                .is_some()
        );
        e.particle_level = 2;
        assert!(
            e.controlled("example:orbit", [0.0; 3], true, Moving(0))
                .is_none()
        );
    }
}
