//! The `animations` of a custom entity model: compiled with the model when the packs load,
//! run per entity per frame after the vanilla animation has posed the parts, so `part.rx`
//! reads the vanilla pose and `head.rx = ...` overrides it.
//!
//! One slot array holds everything an expression reads: the entity's parameters
//! (`limb_swing`, `is_child`...), then 11 slots per model part an animation names (`tx ty tz rx
//! ry rz sx sy sz visible visible_boxes`, as `ModelPart` keeps them: pixels, radians, on
//! vanilla's axes), then the entity's `var.*` / `varb.*`, which persist from frame to frame,
//! then `render.*`, which nothing reads back yet.

use super::{Mob, Part, anim::Poses, anim::State};
use horizon_optifine::cem::expr::{Expr, compile};
use std::{cell::RefCell, collections::HashMap, sync::Mutex};

/// The model variables of a part, in slot order.
const PART_VARS: [&str; 11] = [
    "tx",
    "ty",
    "tz",
    "rx",
    "ry",
    "rz",
    "sx",
    "sy",
    "sz",
    "visible",
    "visible_boxes",
];

macro_rules! params {
    ($($name:literal => $p:ident),* $(,)?) => {
        /// The entity and render parameters, by slot.
        #[derive(Clone, Copy)]
        #[repr(u8)]
        enum Param { $($p),* }
        const PARAMS: &[(&str, Param)] = &[$(($name, Param::$p)),*];
    };
}
params! {
    "limb_swing" => LimbSwing, "limb_speed" => LimbSpeed, "age" => Age,
    "head_yaw" => HeadYaw, "head_pitch" => HeadPitch, "swing_progress" => SwingProgress,
    "health" => Health, "max_health" => MaxHealth, "hurt_time" => HurtTime,
    "death_time" => DeathTime, "pos_x" => PosX, "pos_y" => PosY, "pos_z" => PosZ,
    "rot_x" => RotX, "rot_y" => RotY, "id" => Id, "time" => Time, "day_time" => DayTime,
    "day_count" => DayCount, "frame_time" => FrameTime, "frame_counter" => FrameCounter,
    "dimension" => Dimension, "player_pos_x" => PlayerX, "player_pos_y" => PlayerY,
    "player_pos_z" => PlayerZ, "player_rot_x" => PlayerRotX, "player_rot_y" => PlayerRotY,
    "rule_index" => RuleIndex, "anger_time" => AngerTime, "anger_time_start" => AngerTimeStart,
    "distance" => Distance, "move_forward" => MoveForward, "move_strafing" => MoveStrafing,
    "height_above_ground" => HeightAboveGround, "fluid_depth" => FluidDepth,
    "fluid_depth_down" => FluidDepthDown, "fluid_depth_up" => FluidDepthUp,
    "is_aggressive" => Aggressive, "is_alive" => Alive, "is_burning" => Burning,
    "is_child" => Child, "is_glowing" => Glowing, "is_hurt" => Hurt, "is_in_hand" => InHand,
    "is_in_item_frame" => InItemFrame, "is_in_ground" => InGround, "is_in_gui" => InGui,
    "is_in_lava" => InLava, "is_in_water" => InWater, "is_invisible" => Invisible,
    "is_on_ground" => OnGround, "is_on_head" => OnHead, "is_on_shoulder" => OnShoulder,
    "is_ridden" => Ridden, "is_riding" => Riding, "is_sitting" => Sitting,
    "is_sneaking" => Sneaking, "is_sprinting" => Sprinting, "is_tamed" => Tamed,
    "is_wet" => Wet, "is_climbing" => Climbing, "is_blocking" => Blocking,
    "is_crawling" => Crawling, "is_jumping" => Jumping, "is_swimming" => Swimming,
    "is_right_handed" => RightHanded, "is_swinging_right_arm" => SwingingRight,
    "is_swinging_left_arm" => SwingingLeft, "is_first_person_hand" => FirstPersonHand,
    "is_using_item" => UsingItem, "is_holding_item_right" => HoldingRight,
    "is_holding_item_left" => HoldingLeft, "is_paused" => Paused, "is_hovered" => Hovered,
    "is_player_first_person" => FirstPerson, "is_player_third_person" => ThirdPerson,
    "is_player_third_person_reversed" => ThirdPersonReversed,
}

/// What every entity of a frame shares, set once per frame by the game.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// The world's game time and day time in ticks, with the partial tick.
    pub time: f64,
    pub day_time: f64,
    pub frame_time: f32,
    pub frame_counter: u32,
    /// Overworld 0, nether -1, end 1.
    pub dimension: f32,
    pub player: [f64; 3],
    pub player_rot: [f32; 2],
    pub paused: bool,
    pub first_person: bool,
    pub third_person_reversed: bool,
}

static FRAME: Mutex<Frame> = Mutex::new(Frame {
    time: 0.0,
    day_time: 0.0,
    frame_time: 0.0,
    frame_counter: 0,
    dimension: 0.0,
    player: [0.0; 3],
    player_rot: [0.0; 2],
    paused: false,
    first_person: true,
    third_person_reversed: false,
});

/// The frame the next animations see. Its `frame_time` and `frame_counter` come from the
/// calls themselves: the time since the previous one, and how many there were.
pub fn set_frame(mut frame: Frame) {
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let now = std::time::Instant::now();
    let mut last = LAST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    frame.frame_time = last.map_or(0.0, |t| (now - t).as_secs_f32());
    *last = Some(now);
    let mut current = FRAME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    frame.frame_counter = current.frame_counter.wrapping_add(1);
    *current = frame;
}

/// A part an animation names: its `Poses` name and its first slot.
struct Bound {
    name: String,
    base: u32,
}

/// The compiled `animations` of one model layer.
pub struct Animation {
    steps: Vec<(u32, Expr)>,
    parts: Vec<Bound>,
    /// `var.*` and `varb.*` slots, kept per entity.
    vars: std::ops::Range<u32>,
    slots: usize,
    /// Each entity's variables, by entity id.
    saved: Mutex<HashMap<u32, Box<[f32]>>>,
}

/// How an animation finds the parts it names.
pub(super) struct Names<'a> {
    pub model: &'a Part,
    /// The OptiFine part name to the vanilla part's name.
    pub vanilla: &'a dyn Fn(&str) -> String,
    /// Custom bones' ids by their part names (`cem#3`).
    pub ids: &'a HashMap<String, String>,
}

impl Names<'_> {
    /// The part a model reference names: a part name or bone id, then bone ids below it.
    fn find(&self, reference: &str, this: &str, part: &str) -> Option<String> {
        let mut path = reference.split(':');
        let first = path.next()?;
        let mut at = match first {
            "this" => find(self.model, &|p| p.name == this)?,
            "part" => find(self.model, &|p| p.name == part)?,
            _ => {
                let vanilla = (self.vanilla)(first);
                find(self.model, &|p| p.name == vanilla).or_else(|| {
                    find(self.model, &|p| {
                        self.ids.get(&p.name).is_some_and(|i| i == first)
                    })
                })?
            }
        };
        for id in path {
            at = at
                .children
                .iter()
                .find_map(|c| find(c, &|p| self.ids.get(&p.name).is_some_and(|i| i == id)))?;
        }
        Some(at.name.clone())
    }
}

fn find<'a>(part: &'a Part, test: &dyn Fn(&Part) -> bool) -> Option<&'a Part> {
    if test(part) {
        return Some(part);
    }
    part.children.iter().find_map(|c| find(c, test))
}

/// Where a name an animation uses lives.
#[derive(Clone, Copy)]
enum Key {
    Param(u32),
    Part(u32, u32),
    Var(u32),
    Render(u32),
}

/// The parts and variables a model's animations name, in the order they first appear.
#[derive(Default)]
struct Layout {
    parts: Vec<String>,
    vars: Vec<String>,
    render: Vec<String>,
}

fn index(list: &mut Vec<String>, name: &str) -> u32 {
    let i = list.iter().position(|v| v == name).unwrap_or_else(|| {
        list.push(name.to_owned());
        list.len() - 1
    });
    i as u32
}

impl Layout {
    fn key(&mut self, name: &str, names: &Names, this: &str, part: &str) -> Option<Key> {
        if let Some(i) = PARAMS.iter().position(|(n, _)| *n == name) {
            return Some(Key::Param(i as u32));
        }
        let (model, var) = name.rsplit_once('.')?;
        Some(match model {
            "var" | "varb" | "global_var" | "global_varb" => Key::Var(index(&mut self.vars, name)),
            "render" => Key::Render(index(&mut self.render, name)),
            _ => {
                let var = PART_VARS.iter().position(|v| *v == var)? as u32;
                let found = names.find(model, this, part)?;
                Key::Part(index(&mut self.parts, &found), var)
            }
        })
    }

    fn slot(&self, key: Key) -> u32 {
        let parts = PARAMS.len() as u32;
        let vars = parts + (self.parts.len() * PART_VARS.len()) as u32;
        let render = vars + self.vars.len() as u32;
        match key {
            Key::Param(i) => i,
            Key::Part(i, var) => parts + i * PART_VARS.len() as u32 + var,
            Key::Var(i) => vars + i,
            Key::Render(i) => render + i,
        }
    }
}

/// Builds an animation from `entries`: each model entry's `this` and `part` names and its
/// (target, expression) pairs. An expression that doesn't compile is left out, with a warning.
pub(super) fn compile_animation(
    file: &str,
    names: &Names,
    entries: &[(String, String, &[(String, String)])],
) -> Option<Animation> {
    // First the names every entry uses, which fixes the slots; then the expressions.
    let mut layout = Layout::default();
    for (this, part, pairs) in entries {
        for (target, src) in pairs.iter() {
            let mut note = |name: &str| layout.key(name, names, this, part).map(|_| 0);
            note(target);
            let _ = compile(src, &mut note);
        }
    }
    let mut steps = Vec::new();
    for (this, part, pairs) in entries {
        for (target, src) in pairs.iter() {
            // Every name is in the layout already: the slots don't move.
            let mut resolve = |name: &str| {
                let key = layout.key(name, names, this, part)?;
                Some(layout.slot(key))
            };
            let Some(slot) = resolve(target) else {
                log::warn!("Horizon: custom entity model {file}: unknown target {target}");
                continue;
            };
            match compile(src, &mut resolve) {
                Ok(expr) => steps.push((slot, expr)),
                Err(e) => log::warn!("Horizon: custom entity model {file}: {target}: {e}"),
            }
        }
    }
    if steps.is_empty() {
        return None;
    }
    let parts = layout
        .parts
        .iter()
        .enumerate()
        .map(|(i, name)| Bound {
            name: name.clone(),
            base: layout.slot(Key::Part(i as u32, 0)),
        })
        .collect();
    let vars = layout.slot(Key::Var(0))..layout.slot(Key::Render(0));
    Some(Animation {
        steps,
        parts,
        vars,
        slots: layout.slot(Key::Render(layout.render.len() as u32)) as usize,
        saved: Mutex::default(),
    })
}

thread_local!(static SLOTS: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) });

/// The most entities whose variables an animation keeps; past it they start over.
const SAVED: usize = 4096;

impl Animation {
    /// Runs the animation for `m` over `poses`, which the vanilla animation has set.
    pub fn run(&self, m: &Mob, s: &State, poses: &mut Poses) {
        self.run_with(m.id, |slots| params(m, s, slots), poses);
    }

    /// `run` for the entity `id` whose parameters `fill` writes.
    fn run_with(&self, id: u32, fill: impl FnOnce(&mut [f32]), poses: &mut Poses) {
        SLOTS.with_borrow_mut(|slots| {
            slots.clear();
            slots.resize(self.slots, 0.0);
            fill(slots);
            for b in &self.parts {
                let base = b.base as usize;
                let pose = *poses.get(&b.name);
                slots[base..base + 9].copy_from_slice(&pose);
                slots[base + 9] = 1.0;
                slots[base + 10] = 1.0;
            }
            let vars = self.vars.start as usize..self.vars.end as usize;
            let mut saved = self
                .saved
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(v) = saved.get(&id) {
                slots[vars.clone()].copy_from_slice(v);
            }
            for (slot, expr) in &self.steps {
                let v = expr.eval(slots);
                slots[*slot as usize] = v;
            }
            if !vars.is_empty() {
                if saved.len() >= SAVED && !saved.contains_key(&id) {
                    saved.clear();
                }
                saved.insert(id, slots[vars].into());
            }
            drop(saved);
            for b in &self.parts {
                let base = b.base as usize;
                poses.get(&b.name).copy_from_slice(&slots[base..base + 9]);
                poses.visible(&b.name, slots[base + 9] != 0.0);
                if slots[base + 10] == 0.0 {
                    poses.skip_draw(&b.name);
                }
            }
        });
    }
}

fn params(m: &Mob, s: &State, slots: &mut [f32]) {
    let frame = *FRAME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let t = |b: bool| if b { 1.0 } else { 0.0 };
    let max_health = super::finalize::max_health(m);
    let (dx, dy, dz) = (
        m.position.x - frame.player[0],
        m.position.y - frame.player[1],
        m.position.z - frame.player[2],
    );
    // The movement's direction against the head's: EMF's move_forward and move_strafing.
    let (forward, strafing) = {
        let (vx, vz) = (m.velocity.x as f32, m.velocity.z as f32);
        let len = (vx * vx + vz * vz).sqrt();
        if len < 1.0e-4 {
            (0.0, 0.0)
        } else {
            let yaw = m.head_yaw.to_radians();
            let (fx, fz) = (-yaw.sin(), yaw.cos());
            ((vx * fx + vz * fz) / len, (vx * -fz + vz * fx) / len)
        }
    };
    for (i, (_, p)) in PARAMS.iter().enumerate() {
        use Param::*;
        slots[i] = match p {
            LimbSwing => s.pos,
            LimbSpeed => s.speed,
            Age => s.age,
            HeadYaw => s.y_rot,
            HeadPitch => s.x_rot,
            SwingProgress => s.swing,
            Health => m.health,
            MaxHealth => max_health,
            HurtTime => m.hurt_time.max(0) as f32 - if m.hurt_time > 0 { s.partial } else { 0.0 },
            DeathTime => m.death_time as f32 + if m.death_time > 0 { s.partial } else { 0.0 },
            PosX => m.position.x as f32,
            PosY => m.position.y as f32,
            PosZ => m.position.z as f32,
            RotX => m.head_pitch,
            RotY => m.body_yaw,
            Id => m.id as f32,
            // OptiFine's `time` loops at 27720 ticks to keep float precision.
            Time => (frame.time % 27720.0) as f32,
            DayTime => (frame.day_time % 24000.0) as f32,
            DayCount => (frame.day_time / 24000.0).floor() as f32,
            FrameTime => frame.frame_time,
            FrameCounter => (frame.frame_counter % 27720) as f32,
            Dimension => frame.dimension,
            PlayerX => frame.player[0] as f32,
            PlayerY => frame.player[1] as f32,
            PlayerZ => frame.player[2] as f32,
            PlayerRotX => frame.player_rot[0],
            PlayerRotY => frame.player_rot[1],
            // Random models' rule (a later change); the model's own rule is 0 until then.
            RuleIndex => 0.0,
            AngerTime | AngerTimeStart => 0.0,
            Distance => (dx * dx + dy * dy + dz * dz).sqrt() as f32,
            MoveForward => forward,
            MoveStrafing => strafing,
            // How deep the entity is in its fluid, which is what `fluid_depth_down` measures
            // once it touches the bottom.
            FluidDepth | FluidDepthDown => m.fluids.water.max(m.fluids.lava) as f32,
            HeightAboveGround | FluidDepthUp => 0.0,
            Aggressive => t(m.ai.target.is_some()),
            Alive => t(m.alive()),
            Burning => t(m.fire_ticks > 0),
            Child => t(s.baby),
            Glowing => t(m.glowing),
            Hurt => t(m.hurt_time > 0),
            InWater => t(m.fluids.in_water),
            InLava => t(m.fluids.in_lava),
            Wet => t(m.fluids.in_water),
            Invisible => t(m.invisible),
            OnGround => t(m.on_ground),
            Riding => t(m.extra.passenger),
            Climbing => t(m.climbing),
            Jumping => t(!m.on_ground && m.velocity.y > 0.0),
            Paused => t(frame.paused),
            FirstPerson => t(frame.first_person),
            ThirdPerson => t(!frame.first_person && !frame.third_person_reversed),
            ThirdPersonReversed => t(frame.third_person_reversed),
            RightHanded => 1.0,
            InHand | InItemFrame | InGround | InGui | OnHead | OnShoulder | Ridden | Sitting
            | Sneaking | Sprinting | Tamed | Blocking | Crawling | Swimming | SwingingRight
            | SwingingLeft | FirstPersonHand | UsingItem | HoldingRight | HoldingLeft | Hovered => {
                0.0
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(name: &str, children: Vec<Part>) -> Part {
        Part {
            name: name.into(),
            pose: [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            polygons: Vec::new(),
            children,
        }
    }
    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }
    const LIMB_SWING: usize = Param::LimbSwing as usize;

    #[test]
    fn animations_override_poses_and_keep_variables_per_entity() {
        // A head (OptiFine's name for it maps to `real_head`) holding a custom bone `brim`.
        let model = bone(
            "root",
            vec![bone(
                "real_head",
                vec![bone("cem#0", vec![bone("cem#1", vec![])])],
            )],
        );
        let ids: HashMap<String, String> = [
            ("cem#0".into(), "hat".into()),
            ("cem#1".into(), "brim".into()),
        ]
        .into();
        let vanilla = |p: &str| {
            if p == "head" {
                "real_head".into()
            } else {
                p.to_owned()
            }
        };
        let names = Names {
            model: &model,
            vanilla: &vanilla,
            ids: &ids,
        };
        let head = pairs(&[
            ("var.steps", "var.steps + 1"),
            // Reads the vanilla pose (ty 2) before it changes.
            ("head.ty", "part.ty * 10"),
            ("head.rx", "sin(limb_swing) * 0.5"),
            ("hat:brim.tz", "var.steps"),
            ("brim.visible", "var.steps < 2"),
            ("this.sx", "nope + 1"),
        ]);
        let hat = pairs(&[("this.ry", "torad(90)"), ("render.shadow_size", "0.5")]);
        let entries = [
            (
                "real_head".to_owned(),
                "real_head".to_owned(),
                head.as_slice(),
            ),
            ("cem#0".to_owned(), "real_head".to_owned(), hat.as_slice()),
        ];
        let animation = compile_animation("test.jem", &names, &entries).unwrap();
        let frame = |id: u32, poses: &mut Poses| {
            animation.run_with(id, |s| s[LIMB_SWING] = std::f32::consts::FRAC_PI_2, poses);
        };
        let mut poses = Poses::new(&model);
        frame(7, &mut poses);
        assert_eq!(poses.get("real_head")[..4], [1.0, 20.0, 3.0, 0.5]);
        assert_eq!(poses.get("cem#1")[2], 1.0);
        assert!((poses.get("cem#0")[4] - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // The expression with an unknown name is left out: the scale stays.
        assert_eq!(poses.get("real_head")[6], 1.0);
        assert!(!poses.is_hidden(&bone("cem#1", vec![])));
        // The next frame starts from the vanilla pose again, with this entity's counter.
        let mut poses = Poses::new(&model);
        frame(7, &mut poses);
        assert_eq!(poses.get("cem#1")[2], 2.0);
        assert!(poses.is_hidden(&bone("cem#1", vec![])));
        // Another entity counts on its own.
        let mut poses = Poses::new(&model);
        frame(8, &mut poses);
        assert_eq!(poses.get("cem#1")[2], 1.0);
    }

    #[test]
    fn nothing_to_run_without_valid_expressions() {
        let model = bone("root", vec![]);
        let ids = HashMap::new();
        let names = Names {
            model: &model,
            vanilla: &|p| p.to_owned(),
            ids: &ids,
        };
        let bad = pairs(&[("missing.rx", "1"), ("root.rx", "1 +")]);
        let entries = [("root".to_owned(), "root".to_owned(), bad.as_slice())];
        assert!(compile_animation("test.jem", &names, &entries).is_none());
    }
}
