//! Headless proof of the generated Host bridge and the ordinary particle queue.
use super::{Game, execution_role::ExecutionRole};
use horizon_client::{
    Result,
    hud::Rect,
    mod_api::Host,
    motion::PlayerMotion,
    particles::{ParticleEngine, ParticleLevel, ParticleOptions, ParticleType},
};
use horizon_core::geometry::Vec3;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

fn add(game: &mut Game<'_>, options: &ParticleOptions, at: [f64; 3], velocity: [f64; 3]) -> bool {
    let host: &mut dyn Host = game;
    host.add_particle(options, at, velocity)
}

impl Game<'_> {
    fn particle_fixture_tick(&mut self) {
        let level = ParticleLevel {
            world: &self.world,
            facts: self.res.facts(),
            collision: &self.collision,
            player: None,
            cached_chunk: std::cell::Cell::new(None),
        };
        self.particles.tick(&level);
    }

    pub(super) fn run_native_client_particles_fixture(&mut self, directory: &Path) -> Result<()> {
        let at = [0.0, 100.0, 0.0];
        let smoke = ParticleOptions::simple(ParticleType::Smoke);
        self.particles.requested = Some(Vec::new());
        self.player = None;
        assert!(!add(self, &smoke, at, [0.0; 3]), "menu has no client level");
        self.player = Some(PlayerMotion::new(&self.motion, Vec3::new(0.0, 100.0, 0.0))?);
        for role in [
            ExecutionRole::IntegratedServer,
            ExecutionRole::DedicatedServer,
        ] {
            self.options.execution_role = role;
            assert!(
                !add(self, &smoke, at, [0.0; 3]),
                "servers cannot spawn client particles"
            );
        }
        self.options.execution_role = ExecutionRole::Client;
        let encoded = [f64::from(0xff12ab), 7.0, -2.0];
        let mut custom = ParticleOptions::named("fixture:encoded");
        custom.color = [0.1, 0.2, 0.3, 0.4];
        custom.scale = 2.5;
        assert!(add(self, &custom, at, encoded));
        let requests = self.particles.requested.take().unwrap();
        assert_eq!(requests.len(), 1, "menu and server guards queued nothing");
        assert_eq!(requests[0].0.color, custom.color);
        assert_eq!(requests[0].0.scale, custom.scale);
        assert_eq!(
            requests[0].0.mod_particle.as_deref(),
            Some("fixture:encoded")
        );
        assert_eq!(
            (requests[0].1, requests[0].2),
            (at, encoded),
            "provider slots must remain unchanged"
        );
        self.particles.requested = Some(Vec::new());
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(!add(self, &smoke, [invalid, 100.0, 0.0], [0.0; 3]));
            assert!(!add(self, &smoke, at, [0.0, invalid, 0.0]));
        }
        let mut invalid_options = smoke.clone();
        invalid_options.color[1] = f32::NAN;
        assert!(!add(self, &invalid_options, at, [0.0; 3]));
        invalid_options = smoke.clone();
        invalid_options.scale = f32::INFINITY;
        assert!(!add(self, &invalid_options, at, [0.0; 3]));

        assert!(
            self.particles.requested.take().unwrap().is_empty(),
            "invalid requests must not touch the queue"
        );

        // Own sprite metadata and atlas rectangles exercise real constructors and CPU quads,
        // without a GPU, downloaded art, a resource reload or a change to imported resources.
        let pack = directory.join("particle-pack");
        std::fs::create_dir_all(pack.join("assets/minecraft/particles"))?;
        std::fs::create_dir_all(pack.join("assets/fixture/particles"))?;
        std::fs::write(
            pack.join("assets/minecraft/particles/smoke.json"),
            br#"{"textures":["fixture_smoke"]}"#,
        )?;
        std::fs::write(pack.join("assets/fixture/particles/spark.json"), br#"{"textures":["fixture:spark"],"horizon:behaviour":{"velocity":[0.25,0,0],"gravity":0,"friction":1,"lifetime":3,"size":[0.2,0.2],"animation":"first"}}"#)?;
        let rect = Rect {
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
            w: 16,
            h: 16,
        };
        self.particles = ParticleEngine::new(
            &pack,
            &HashMap::from([
                ("particle/fixture_smoke".into(), rect),
                ("fixture:particle/spark".into(), rect),
            ]),
            &HashSet::new(),
        );
        self.particles.camera = at;
        assert!(add(self, &smoke, at, [0.0; 3]));
        assert_eq!(
            self.particles.len(),
            0,
            "engine defers pending spawns until its tick"
        );
        self.particle_fixture_tick();
        assert_eq!(
            self.particles.len(),
            1,
            "vanilla provider reached the live queue"
        );
        let mut vertices = Vec::new();
        self.particles.render(
            glam::DVec3::from_array(at),
            0.0,
            0.0,
            1.0,
            &|_| 15,
            &mut vertices,
        );
        assert_eq!(
            vertices.len(),
            4,
            "vanilla particle reaches ordinary render data"
        );
        self.particles.clear();
        assert!(add(self, &smoke, [64.0, 100.0, 0.0], [0.0; 3]));
        self.particle_fixture_tick();
        assert!(
            self.particles.is_empty(),
            "ordinary camera-distance limiter is retained"
        );
        self.particles.particle_level = 2;
        assert!(add(self, &smoke, at, [0.0; 3]));
        self.particle_fixture_tick();
        assert!(
            self.particles.is_empty(),
            "ordinary minimal setting is retained"
        );
        self.particles.particle_level = 0;
        assert!(add(
            self,
            &ParticleOptions::named("fixture:spark"),
            at,
            [0.0; 3]
        ));
        self.particle_fixture_tick();
        assert_eq!(
            self.particles.len(),
            1,
            "existing named provider reached the live queue"
        );
        self.particle_fixture_tick();
        vertices.clear();
        self.particles.render(
            glam::DVec3::from_array(at),
            0.0,
            0.0,
            1.0,
            &|_| 15,
            &mut vertices,
        );
        assert_eq!(vertices.len(), 4);
        assert!(vertices.iter().flat_map(|v| v.pos).all(f32::is_finite));
        let center_x = vertices.iter().map(|v| v.pos[0]).sum::<f32>() / 4.0;
        assert!(
            (center_x - 0.25).abs() < 1e-6,
            "named behavior moved the real particle: {center_x}"
        );
        for _ in 0..10 {
            self.particle_fixture_tick();
        }
        assert!(
            self.particles.is_empty(),
            "named provider lifetime still expires normally"
        );
        std::fs::write(
            directory.join("report.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "client_spawn": true, "encoded_slots_unchanged": true,
                "menu_and_servers_refused": true, "nonfinite_refused": true,
                "vanilla_live_particles": 1, "vanilla_vertices": 4,
                "named_live_particles": 1, "named_vertices": 4, "named_tick_x": center_x,
                "named_expired": true, "normal_limiters_retained": true,
                "scope": "Generated Host bridge into existing particle queue, providers, tick and CPU render data; no GPU/window or new providers/triggers."
            }))?,
        )?;
        Ok(())
    }
}
