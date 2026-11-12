use crate::game::{Audio, Game, Options, Progress, Settings, load_resources};
use horizon_client::{
    chunk::Chunk,
    mobs::Who,
    motion::PlayerMotion,
    projectiles::{Projectile, ProjectileKind},
};
use horizon_core::{geometry::Vec3, position::ChunkPos, random::RandomSource};
use std::sync::Arc;
fn with_game(test: impl FnOnce(&mut Game<'_>)) {
    let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        std::time::Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.game_dir = Some(
        std::path::PathBuf::from(std::env::var_os("HORIZON_HOME").unwrap())
            .join(format!("h114-{:?}", std::thread::current().id())),
    );
    options.execution_role = crate::game::execution_role::ExecutionRole::IntegratedServer;
    let mut game = Game::new(
        &loaded.resources,
        loaded.pack,
        loaded.gui.unwrap(),
        options,
        settings,
        Audio::silent(&root),
        None,
    )
    .unwrap();
    game.build_registries();
    game.set_allow_commands(true);

    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    game.block_ticks
        .add_empty_chunk(ChunkPos::new(0, 0))
        .unwrap();
    assert!(game.run_command("difficulty normal"));
    assert!(game.run_command("forceload add 0 0"));
    for (x, z) in [(0, 1), (1, 0), (1, 1)] {
        game.world
            .chunks
            .insert((x, z), Arc::new(Chunk::empty(x, z)));
        game.block_ticks
            .add_empty_chunk(ChunkPos::new(x, z))
            .unwrap();
    }
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(2.5, 200.0, 2.5)).unwrap());
    game.creative = true;
    assert!(game.run_command("fill 0 199 0 31 199 31 stone"));
    assert!(game.run_command("summon pig 13.5 200 2.5 {UUID:[I;114,0,0,99],NoAI:1b,NoGravity:1b}"));
    game.world.random.set_seed(114);
    test(&mut game);
}
fn start_events() {
    crate::game::sculk::TEST_POSTED_EVENTS.with(|v| *v.borrow_mut() = Some(Vec::new()));
}
fn take_shots() -> Vec<(&'static str, [f64; 3], Option<Who>)> {
    crate::game::sculk::TEST_POSTED_EVENTS.with(|v| {
        std::mem::take(v.borrow_mut().as_mut().unwrap())
            .into_iter()
            .filter(|e| e.0 == "minecraft:projectile_shoot")
            .collect()
    })
}
fn stop_events() {
    crate::game::sculk::TEST_POSTED_EVENTS.with(|v| *v.borrow_mut() = None);
}
fn owner(game: &Game<'_>) -> u32 {
    game.mobs
        .iter()
        .find(|m| game.persist.uuid_of(m.id) == Some([114, 0, 0, 99]))
        .unwrap()
        .id
}
/// `Projectile.tick`: a rocket loaded without `HasBeenShot` posts
/// `projectile_shoot` on its first tick, once; one loaded shot posts none.
/// `FireworkRocketEntity.tick`'s launch sound skips a silent one.
#[test]
#[ignore = "prepared resources; actual registered load, Game tick and posted events"]
fn a_rocket_posts_its_shot_once_and_a_silent_one_launches_quietly() {
    with_game(|game| {
        let launch = "minecraft:entity.firework_rocket.launch";
        assert!(game.run_command(
            "summon firework_rocket 6.5 201.5 10.5 {LifeTime:100,Silent:1b,NoGravity:1b}"
        ));
        assert!(
            game.run_command("summon firework_rocket 9.5 201.5 10.5 {LifeTime:100,HasBeenShot:1b}")
        );
        assert!(
            game.rockets
                .iter()
                .all(|r| r.has_been_shot == (r.position.x == 9.5))
        );
        game.audio.heard.clear();
        start_events();
        game.tick_rockets();
        let first = take_shots();
        game.tick_rockets();
        let second = take_shots();
        stop_events();
        assert_eq!(
            first,
            vec![("minecraft:projectile_shoot", [6.5, 201.5, 10.5], None)]
        );
        assert!(second.is_empty());
        assert!(game.rockets.iter().all(|r| r.has_been_shot));
        // Only the rocket that isn't silent launches with a sound.
        assert_eq!(game.audio.heard.get(launch), Some(&1));
    });
}
/// #14440: an orb's move adds its drop to the fall outside water; landing
/// after a fall posts HIT_GROUND and forgets the fall (`Entity.checkFallDamage`);
/// in water the drop adds nothing, so a landing posts nothing.
#[test]
#[ignore = "prepared resources; actual registered load, Game tick and posted events"]
fn a_landing_orb_posts_hit_ground_after_a_fall() {
    use horizon_client::entities::OrbMove;
    with_game(|game| {
        assert!(game.run_command("summon experience_orb 6.5 200 10.5 {Value:1,fall_distance:2d}"));
        let i = game.orbs.len() - 1;
        let from = Vec3::new(6.5, 200.5, 10.5);
        let down = Vec3::new(0.0, -0.5, 0.0);
        start_events();
        // In the air: the drop adds to the fall.
        game.orbs[i].on_ground = false;
        game.orb_moved(
            i,
            OrbMove {
                from,
                moved: down,
                in_water: false,
            },
        );
        assert_eq!(game.orbs[i].fall_distance, 2.5);
        // On the ground: HIT_GROUND, then the fall is gone.
        game.orbs[i].on_ground = true;
        game.orb_moved(
            i,
            OrbMove {
                from,
                moved: down,
                in_water: false,
            },
        );
        assert_eq!(game.orbs[i].fall_distance, 0.0);
        // In water with no fall: no drop, so no landing event.
        game.orb_moved(
            i,
            OrbMove {
                from,
                moved: down,
                in_water: true,
            },
        );
        let events: Vec<_> = crate::game::sculk::TEST_POSTED_EVENTS.with(|v| {
            std::mem::take(v.borrow_mut().as_mut().unwrap())
                .into_iter()
                .filter(|e| e.0 == "minecraft:hit_ground")
                .collect()
        });
        stop_events();
        assert_eq!(events.len(), 1);
        assert_eq!(game.orbs[i].fall_distance, 0.0);
    });
}
/// #14318: an angled rocket falling onto stone builds up its fall and posts
/// HIT_GROUND when it lands (`Entity.checkFallDamage`); one falling into water
/// splashes once (`Entity.doWaterSplashEffect`: the sound and SPLASH) and lands
/// with no fall.
#[test]
#[ignore = "prepared resources; actual registered load, Game tick and posted events"]
fn a_falling_rocket_hits_the_ground_and_one_in_water_splashes() {
    with_game(|game| {
        assert!(game.run_command("setblock 20 200 10 water"));
        for x in [6, 20] {
            assert!(game.run_command(&format!(
                "summon firework_rocket {x}.5 202 10.5 {{LifeTime:100,HasBeenShot:1b,ShotAtAngle:1b,Motion:[0d,-0.5d,0d]}}"
            )));
        }
        game.audio.heard.clear();
        start_events();
        let mut falls = Vec::new();
        for _ in 0..6 {
            game.tick_rockets();
            falls.push(game.rockets[0].fall_distance);
        }
        let events: Vec<_> = crate::game::sculk::TEST_POSTED_EVENTS.with(|v| {
            std::mem::take(v.borrow_mut().as_mut().unwrap())
                .into_iter()
                .map(|e| (e.0, e.1[0].floor() as i32))
                .filter(|e| matches!(e.0, "minecraft:hit_ground" | "minecraft:splash"))
                .collect()
        });
        stop_events();
        // The fourth move ends on the stone without a collision; the fifth lands.
        assert_eq!(falls, [0.5, 1.0, 1.5, 2.0, 0.0, 0.0]);
        assert_eq!(game.rockets[1].fall_distance, 0.0);
        assert_eq!(
            events,
            [("minecraft:splash", 20), ("minecraft:hit_ground", 6)]
        );
        assert_eq!(
            game.audio.heard.get("minecraft:entity.generic.splash"),
            Some(&1)
        );
    });
}
#[test]
#[ignore = "prepared resources; actual registered load, Game tick and posted events"]
fn loaded_arrow_first_tick_dispatches_once_with_its_real_owner() {
    with_game(|game| {
        let owner = owner(game);
        // Already-shot is an independent silent control before the old failure.
        for (n, flag) in [(1, Some(1)), (2, None), (3, Some(0))] {
            let extra = flag
                .map(|f| format!(",HasBeenShot:{f}b"))
                .unwrap_or_default();
            assert!(game.run_command(&format!("summon arrow 6.5 201.5 10.5 {{UUID:[I;114,0,0,{n}],NoGravity:1b,Motion:[0d,0d,0d],Owner:[I;114,0,0,99]{extra}}}")));
            let id = game
                .creatures
                .projectiles
                .iter()
                .find(|p| game.persist.uuid_of(p.id) == Some([114, 0, 0, n]))
                .unwrap()
                .id;
            let p = game
                .creatures
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .unwrap();
            assert_eq!(p.owner_uuid, Some([114, 0, 0, 99]));
            assert_eq!(p.arrow.as_ref().unwrap().has_been_shot, flag == Some(1));
            assert!(p.no_gravity);
            assert_eq!(p.velocity, Vec3::ZERO);
            start_events();
            game.tick_projectiles().unwrap();
            let p = game
                .creatures
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .unwrap();
            assert!(p.arrow.as_ref().unwrap().has_been_shot);
            assert_eq!(p.position, Vec3::new(6.5, 201.5, 10.5));
            assert_eq!(p.velocity, Vec3::ZERO);
            let first = take_shots();
            game.tick_projectiles().unwrap();
            let second = take_shots();
            stop_events();
            assert!(second.is_empty(), "no repeated shot on tick2");
            let expected = if flag == Some(1) {
                Vec::new()
            } else {
                vec![(
                    "minecraft:projectile_shoot",
                    [6.5, 201.5, 10.5],
                    Some(Who::Mob(owner)),
                )]
            };
            assert_eq!(first, expected, "actual first tick, input={flag:?}");
            assert!(game.run_command(&format!(
                "kill {}",
                horizon_protocol::crypto::uuid_string(horizon_client::lan::playerdata::ints_uuid(
                    [114, 0, 0, n]
                ))
            )));
        }
    });
}
fn natural(kind: &str) {
    with_game(|game| {
        let shooter = owner(game);
        match kind {
                "bow"=>{
                    assert!(game.run_command("item replace entity @s weapon.mainhand with bow"));
                    assert!(game.run_command("item replace entity @s hotbar.1 with arrow 4"));
                },
                "crossbow"=>assert!(game.run_command("item replace entity @s weapon.mainhand with crossbow[charged_projectiles=[{id:\"minecraft:arrow\",count:1}]]")),
                _=>{}
            }
        let clock = game.world.game_time;
        start_events();
        match kind {
            "bow" => {
                let bow = game.held().unwrap().clone();
                game.release_bow(&bow, 20);
            }
            "crossbow" => {
                let bow = game.held().unwrap().clone();
                assert!(game.use_crossbow(&bow));
            }
            "mob" => {
                let mut p = Projectile::new(
                    ProjectileKind::Arrow {
                        base_damage: 2.0,
                        critical: false,
                        effect: None,
                    },
                    Vec3::new(6.5, 201.5, 6.5),
                    Some(Who::Mob(shooter)),
                );
                p.velocity = Vec3::new(0.0, 0.0, 1.0);
                game.launch_mob_projectile(p);
            }
            _ => unreachable!(),
        }
        assert_eq!(game.creatures.projectiles.len(), 1, "real launch {kind}");
        let p = &game.creatures.projectiles[0];
        let id = p.id;
        assert!(id > 0);
        assert!(
            !p.arrow.as_ref().unwrap().has_been_shot,
            "saved pre-tick state"
        );
        let launch = take_shots();
        game.tick_projectiles().unwrap();
        let p = game
            .creatures
            .projectiles
            .iter()
            .find(|p| p.id == id)
            .unwrap();
        assert!(p.arrow.as_ref().unwrap().has_been_shot);
        let at = [p.position.x, p.position.y, p.position.z];
        let first = take_shots();
        game.tick_projectiles().unwrap();
        let second = take_shots();
        stop_events();
        assert_eq!(
            game.world.game_time, clock,
            "fixture directly invokes actual projectile ticks; runtime clocks independently admitted"
        );
        println!("H114_RNG {kind} {}", game.world.random.next_long());
        assert_eq!(
            [launch.len(), first.len(), second.len()],
            [0, 1, 0],
            "actual launch/tick1/tick2 counts {kind}"
        );
        assert_eq!(
            first[0],
            (
                "minecraft:projectile_shoot",
                at,
                Some(if kind == "mob" {
                    Who::Mob(shooter)
                } else {
                    Who::Player
                })
            )
        );
    });
}
#[test]
#[ignore = "prepared resources; actual bow release and Game ticks"]
fn bow_launch_uses_only_the_first_tick_event() {
    natural("bow")
}
#[test]
#[ignore = "prepared resources; actual charged crossbow use and Game ticks"]
fn crossbow_launch_uses_only_the_first_tick_event() {
    natural("crossbow")
}
#[test]
#[ignore = "prepared resources; actual Mob launch adapter and Game ticks"]
fn mob_launch_uses_only_the_first_tick_event() {
    natural("mob")
}
#[test]
#[ignore = "prepared resources; non-arrow launch posts remain unchanged"]
fn a_crossbow_rocket_posts_on_its_first_tick_and_a_mob_trident_at_once() {
    with_game(|game| {
        assert!(game.run_command("item replace entity @s weapon.mainhand with crossbow[charged_projectiles=[{id:\"minecraft:firework_rocket\",count:1}]]"));
        start_events();
        let bow = game.held().unwrap().clone();
        assert!(game.use_crossbow(&bow));
        assert_eq!(game.rockets.len(), 1);
        assert!(take_shots().is_empty());
        // Projectile.tick: where the rocket is, with the player as owner.
        let at = game.rockets[0].position.components();
        game.tick_rockets();
        assert_eq!(
            take_shots(),
            vec![("minecraft:projectile_shoot", at, Some(Who::Player))]
        );
        game.tick_rockets();
        assert!(take_shots().is_empty());
        stop_events();
        let mut p = Projectile::new(
            ProjectileKind::Trident,
            Vec3::new(6.5, 201.5, 6.5),
            Some(Who::Mob(owner(game))),
        );
        p.velocity = Vec3::new(0.0, 0.0, 1.0);
        start_events();
        game.launch_mob_projectile(p);
        assert_eq!(take_shots().len(), 1);
        game.tick_projectiles().unwrap();
        assert!(take_shots().is_empty());
        stop_events();
    });
}
#[test]
#[ignore = "prepared resources; real loaded owner and finish-hit source"]
fn finish_hit_dispatches_once_and_the_next_tick_does_not_repeat_it() {
    with_game(|game| {
        assert!(game.run_command("summon arrow 6.5 201.5 10.5 {UUID:[I;114,0,0,20],NoGravity:1b,Motion:[0d,0d,0d],Owner:[I;114,0,0,99]}"));
        let mut arrow = game.creatures.projectiles.pop().unwrap();
        let owner = owner(game);
        assert_eq!(arrow.owner_uuid, Some([114, 0, 0, 99]));
        // Resolve through the actual registered owner, as tick_projectiles does.
        arrow.owner = game.who_by_uuid(arrow.owner_uuid.unwrap());
        assert_eq!(arrow.owner, Some(Who::Mob(owner)));
        assert!(!arrow.arrow.as_ref().unwrap().has_been_shot);
        let id = arrow.id;
        start_events();
        game.finish_arrow_hit(arrow, super::EntityHit::Deflected);
        let first = take_shots();
        assert!(
            game.creatures
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .arrow
                .as_ref()
                .unwrap()
                .has_been_shot
        );
        game.tick_projectiles().unwrap();
        assert!(take_shots().is_empty());
        stop_events();
        assert_eq!(
            first,
            vec![(
                "minecraft:projectile_shoot",
                [6.5, 201.5, 10.5],
                Some(Who::Mob(owner))
            )]
        );
    });
}
#[test]
#[ignore = "prepared resources; registered Guest context and finish-hit source"]
fn finish_hit_uses_registered_guest_context_and_restores_the_host() {
    use horizon_client::lan::{Joined, NetConfig, PlayerSender, guest::Guest, hub::HubConfig};
    use horizon_protocol::packets::configuration::ClientInformation;
    use std::sync::{atomic::AtomicI32, mpsc};
    with_game(|game| {
        game.lan_host(
            crate::game::lan::LanSettings {
                port: 0,
                guest_commands: true,
                force_game_mode: false,
            },
            NetConfig {
                announce: false,
                ..NetConfig::loopback(0, "arrow-test".into(), 2)
            },
            HubConfig {
                default_game_mode: 1,
                guest_commands: true,
                force_game_mode: false,
                hardcore: false,
                view_distance: 2,
                simulation_distance: 2,
                dimension: "minecraft:overworld".into(),
                allow_flight: false,
                seed: 0,
                max_players: 2,
                integrated: false,
                levels: Default::default(),
            },
        )
        .unwrap();
        let (tx, _rx) = mpsc::channel();
        let mut guest = Guest::new(
            Joined {
                conn: 2,
                uuid: 114,
                name: "ArrowGuest".into(),
                properties: Vec::new(),
                info: ClientInformation::new(2),
                brand: "test".into(),
                sender: PlayerSender::new(tx, Arc::default()),
                latency: Arc::new(AtomicI32::new(0)),
                address: "127.0.0.1:25565".parse().unwrap(),
                expected: false,
                configured: Default::default(),
            },
            2,
            [6.5, 200.0, 6.5],
            2,
        );
        guest.dimension = "minecraft:overworld".into();
        guest.placed = true;
        guest.loaded = true;
        guest.permission = 4;
        let mut host = game.lan.take().unwrap();
        host.hub.guests.push(guest);
        let host_uuid = game.acting_player_uuid();
        game.as_guest(&mut host,2,|g| {
                assert_eq!(g.lan_acting,Some(2));
                assert_eq!(g.acting_player_uuid(),[0,0,0,114]);
                assert!(g.run_command("item replace entity @s weapon.mainhand with crossbow[charged_projectiles=[{id:\"minecraft:arrow\",count:1}]]"));
                let bow=g.held().unwrap().clone();assert!(g.use_crossbow(&bow));
            }).unwrap();
        game.lan = Some(host);
        assert_eq!(game.acting_player_uuid(), host_uuid);
        assert_eq!(game.lan_acting, None);
        let arrow = game.creatures.projectiles.pop().unwrap();
        assert_eq!(arrow.guest, Some(2));
        assert_eq!(arrow.owner, Some(Who::Player));
        assert_eq!(arrow.owner_uuid, Some([0, 0, 0, 114]));
        assert!(!arrow.arrow.as_ref().unwrap().has_been_shot);
        let at = [arrow.position.x, arrow.position.y, arrow.position.z];
        let id = arrow.id;
        start_events();
        game.finish_arrow_hit(arrow, super::EntityHit::Deflected);
        let first = take_shots();
        assert_eq!(game.acting_player_uuid(), host_uuid);
        assert_eq!(game.lan_acting, None);
        assert_eq!(game.lan.as_ref().unwrap().hub.guests[0].uuid, 114);
        assert!(
            game.creatures
                .projectiles
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .arrow
                .as_ref()
                .unwrap()
                .has_been_shot
        );
        game.tick_projectiles().unwrap();
        assert!(take_shots().is_empty());
        stop_events();
        assert_eq!(
            first,
            vec![("minecraft:projectile_shoot", at, Some(Who::Player))]
        );
    });
}
