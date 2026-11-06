//! Actual in-memory Game potion paths. No run loop, world load or server startup.
use super::*;
use crate::game::{Audio, Options, Progress, Settings, execution_role, load_resources};
use horizon_client::{chunk::Chunk, mobs::Mob, motion::PlayerMotion};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn stack(row: &Value) -> ItemStack {
    let item = match row["kind"].as_str().unwrap() {
        "arrow" => "tipped_arrow",
        "lingering" => "lingering_potion",
        _ => "splash_potion",
    };
    let mut stack = ItemStack::new(&format!("minecraft:{item}"), 1);
    if let Some(potion) = row["potion"].as_str() {
        stack = stack.with_potion(&format!("minecraft:{potion}"));
    }
    let effects = row["custom"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            Tag::compound([
                (
                    "id",
                    Tag::String(format!("minecraft:{}", e["id"].as_str().unwrap()).into()),
                ),
                ("duration", Tag::Int(e["duration"].as_i64().unwrap() as i32)),
                ("amplifier", Tag::Int(e["amp"].as_i64().unwrap() as i32)),
                (
                    "ambient",
                    Tag::Byte(i8::from(e["ambient"].as_bool().unwrap())),
                ),
                (
                    "show_particles",
                    Tag::Byte(i8::from(e["visible"].as_bool().unwrap())),
                ),
            ])
        })
        .collect();
    stack
        .data_mut()
        .potion_extra
        .push(("custom_effects".into(), Tag::List(effects)));
    if let Some(color) = row["color"].as_i64() {
        stack
            .data_mut()
            .potion_extra
            .push(("custom_color".into(), Tag::Int(color as i32)));
    }
    if let Some(name) = row["name"].as_str() {
        stack
            .data_mut()
            .potion_extra
            .push(("custom_name".into(), Tag::String(name.into())));
    }
    stack.data_mut().set_other(
        "minecraft:potion_duration_scale",
        Some(Tag::Float(
            (row["scale"].as_f64().unwrap() as f32).to_bits(),
        )),
    );
    stack
}
fn effect(id: &str, duration: i32, amp: i32, ambient: bool, visible: bool) -> Value {
    json!({"id":id.trim_start_matches("minecraft:"),"duration":duration,"amp":amp,"ambient":ambient,"visible":visible})
}

#[test]
#[ignore = "explicit HORIZON_TEST_RESOURCES and isolated potion oracle input/output"]
fn actual_thrown_contents_match_pinned_methods() {
    let root =
        PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").expect("explicit resources"));
    let input = PathBuf::from(std::env::var_os("HORIZON_POTION_INPUT").expect("explicit input"));
    let output = PathBuf::from(std::env::var_os("HORIZON_POTION_OUTPUT").expect("explicit output"));
    let expected: Vec<Value> = serde_json::from_slice(
        &std::fs::read(std::env::var_os("HORIZON_POTION_EXPECTED").expect("pinned method output"))
            .unwrap(),
    )
    .unwrap();
    let rows: Vec<Value> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let settings = Settings::default();
    let loaded = load_resources(
        root.clone(),
        None,
        &root,
        &settings,
        Instant::now(),
        false,
        true,
        &Progress::new(),
        &mut |_, _| unreachable!(),
    )
    .unwrap();
    let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
    options.resources = root.clone();
    options.game_dir = Some(std::env::current_dir().unwrap());
    options.execution_role = execution_role::ExecutionRole::IntegratedServer;
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
    game.world
        .chunks
        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
    let stone = game
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();
    game.world.set(0, 64, 0, stone).unwrap();
    let kind = game
        .res
        .mob_types()
        .get("minecraft:villager")
        .unwrap()
        .clone();
    let mut results = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        let stack = stack(&row);
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64., 0.5)).unwrap());
        game.set_mode_id(0);
        let healing = row["custom"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["id"] == "instant_health");
        let health = if healing { 5.0 } else { 20.0 };
        game.player
            .as_mut()
            .unwrap()
            .health
            .set_health(health, false)
            .unwrap();
        game.potions = PotionState::default();
        game.mobs.clear();
        game.breath_clouds.clear();
        let mut mob = Mob::new(10, kind.clone(), Vec3::new(0.5, 64., 0.5), 0.);
        mob.health = health;
        game.mobs.push(mob);
        let id = row["id"].clone();
        if row["kind"] == "arrow" {
            let mut arrow = Projectile::new(
                ProjectileKind::Arrow {
                    base_damage: 2.,
                    critical: false,
                    effect: None,
                },
                Vec3::new(0.5, 64.5, 0.5),
                None,
            );
            let st = arrow.arrow.as_mut().unwrap();
            st.pickup_item = stack;
            st.in_ground = true;
            st.in_ground_time = 598;
            st.last_state = Some(stone);
            arrow.stuck = true;
            let rain = |_| false;
            let scene = game.arrow_scene(&rain);
            horizon_client::arrows::tick(&mut arrow, &scene, &[]);
            let st = arrow.arrow.as_ref().unwrap();
            let item599 = st
                .pickup_item
                .item
                .trim_start_matches("minecraft:")
                .to_owned();
            let color599 = st.potion_color().map_or(-1, |v| v as i32);
            horizon_client::arrows::tick(&mut arrow, &scene, &[]);
            let st = arrow.arrow.as_ref().unwrap();
            results.push(json!({"id":id,"item599":item599,"color599":color599,"item600":st.pickup_item.item.trim_start_matches("minecraft:"),"color600":st.potion_color().map_or(-1,|v|v as i32)}));
            assert_eq!(
                results.last().unwrap(),
                &expected[index],
                "actual arrow tick: {}",
                id
            );
            continue;
        }
        let color = {
            let mut arrow = Projectile::new(
                ProjectileKind::Arrow {
                    base_damage: 2.,
                    critical: false,
                    effect: None,
                },
                Vec3::ZERO,
                None,
            );
            arrow.arrow.as_mut().unwrap().pickup_item = stack.clone();
            arrow.arrow.as_ref().unwrap().potion_color()
        };
        let water = game.potion_in_tag(&stack, "minecraft:douses_fire");
        let x = if row["half"].as_bool().unwrap_or(false) {
            2.925
        } else {
            0.5
        };
        game.splash(
            &stack,
            Vec3::new(x, 64.5, 0.5),
            None,
            None,
            1,
            &PotionOwner::default(),
        );
        if row["kind"] == "lingering" {
            // Existence is checked before new raw metadata assertions, so stock
            // current main fails actual custom-only cloud creation first.
            let cloud = game
                .breath_clouds
                .last()
                .expect("actual lingering cloud exists");
            let potion = cloud.potion.as_ref().unwrap();
            let effects: Vec<_> = potion
                .effects
                .iter()
                .map(|(id, d, a)| {
                    effect(
                        id,
                        *d,
                        *a,
                        potion.ambient.contains(id),
                        !potion.hidden.contains(id),
                    )
                })
                .collect();
            results.push(json!({"id":id,"scale":potion.settings.duration_scale,"color":potion.color as i32,"effects":effects}));
        } else {
            let player: Vec<_> = game
                .potions
                .effects
                .list
                .iter()
                .map(|e| effect(e.id, e.duration, e.amplifier, e.ambient, e.visible))
                .collect();
            let mob = &game.mobs[0];
            let effects: Vec<_> = mob
                .extra
                .effects
                .iter()
                .map(|(id, d, a)| {
                    effect(
                        id,
                        *d,
                        *a,
                        mob.extra.ambient_effects.contains(id),
                        !mob.extra.hidden_effects.contains(id),
                    )
                })
                .collect();
            results.push(json!({"id":id,"player":{"delta":game.player.as_ref().unwrap().health.health()-health,"effects":player},"mob":{"delta":mob.health-health,"effects":effects},"color":color.map_or(0xFF38_5DC6u32 as i32,|v|v as i32),"empty":color.is_none(),"water":water}));
        }
        let observed = results.last().unwrap();
        if row["kind"] != "lingering" {
            // Stock current main compiles this test, then fails these actual
            // application checks before any new colour/metadata assertion.
            assert_eq!(
                observed["player"]["effects"], expected[index]["player"]["effects"],
                "actual player potion effects: {}",
                id
            );
            assert_eq!(
                observed["mob"]["effects"], expected[index]["mob"]["effects"],
                "actual mob potion effects: {}",
                id
            );
        }
        assert_eq!(observed, &expected[index], "actual potion method: {}", id);
    }
    std::fs::write(output, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
}
