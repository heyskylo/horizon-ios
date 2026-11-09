//! Custom uniforms and variables of `shaders.properties` (spec 3.4): `uniform.<type>.<name>` and
//! `variable.<type>.<name>` expressions, parsed once and evaluated on the CPU every frame in
//! file order. Uniforms reach every program; variables only later expressions.
//!
//! Decisions (impl-plan.md):
//! - OQ-25: numbers are doubles; booleans are 1 and 0; `%` is C's remainder (`fmod` floors);
//!   comparisons and logic read the first component; a value takes its declared type at the
//!   end (`int` truncates toward zero, `bool` is "not zero"). Unknown names read 0 and are
//!   reported once.
//! - OQ-26: `smooth` runs per frame with the frame time; its fade times (1 by default) are
//!   half-lives counted in tenths of a second (`FADE_UNIT`), `fadeIn` while rising, `fadeOut`
//!   while falling. Each call keeps its own state, so the optional id is not needed. As in Iris
//!   1.11.7, the first argument is that id when it is written as a number and more follow.
//!   A `smooth` in an `if` branch not taken keeps its value until the branch is taken again:
//!   `if` evaluates only the conditions up to the first true one and that branch (#9371).
//! - Unknown names, as in Iris 1.11.7 (#8995): an expression naming something the custom
//!   uniforms can't see (an unknown name, or one of [`HIDDEN`]) reads 0 as a whole, even when
//!   the name is in an `if` branch not taken, and so does every expression using its uniform or
//!   variable. Only the names of [`NOT_GIVEN_YET`] read 0 alone.
//! - Types, as in Iris 1.11.7 (#9086, #9126): an expression without its declared type (an
//!   `int` given a float, a `float` given a vector or a bool...), joining a vector and a scalar,
//!   or calling a function Iris's custom uniforms don't have, reads 0 as a whole, and so does
//!   everything using it. The rules ([`Ty`]) are a fit to our probe packs.

use std::collections::{HashMap, HashSet};

use crate::Properties;

/// The seconds one unit of a `smooth` fade time lasts. Fitted to Iris 1.11.7 with a probe pack of
/// our own (`smoothprobe`: `smooth(rainStrength, f, f)` for several fades, read across the scene
/// suite's weather changes): a half-life of a tenth of the fade time's seconds gives every
/// measured value within 0.016. A second probe pack of ours (#9371: a square wave from `if`,
/// `eyeBrightness`, `worldTime`; fades 0.5 to 100, the default 1, rising and falling) fits the
/// same unit: half-life / fade between 0.085 and 0.100 over 63 readings. These are fits to our
/// probes' Iris values, not Iris's rule. Horizon's built-in smoothed uniforms use the same 0.1 s
/// unit (`pack_uniforms.rs` `SECONDS_A_TICK`).
const FADE_UNIT: f64 = 0.1;

/// A value: up to 16 components (matrices only as names, for `m.column.row`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Val {
    pub v: [f64; 16],
    /// Components: 1 for a scalar.
    pub n: usize,
}

impl Val {
    pub fn scalar(x: f64) -> Self {
        let mut v = [0.0; 16];
        v[0] = x;
        Self { v, n: 1 }
    }
    pub fn vector(c: &[f64]) -> Self {
        let mut v = [0.0; 16];
        v[..c.len()].copy_from_slice(c);
        Self { v, n: c.len() }
    }
    fn x(&self) -> f64 {
        self.v[0]
    }
    fn truth(&self) -> bool {
        self.v[0] != 0.0
    }
    /// `f` per component; a scalar operand stretches to the other's size.
    fn zip(self, o: Val, f: impl Fn(f64, f64) -> f64) -> Val {
        let n = self.n.max(o.n).min(4);
        let at = |a: &Val, i: usize| if a.n == 1 { a.v[0] } else { a.v[i] };
        let mut out = Val::scalar(0.0);
        out.n = n;
        for i in 0..n {
            out.v[i] = f(at(&self, i), at(&o, i));
        }
        out
    }
    fn map(self, f: impl Fn(f64) -> f64) -> Val {
        let mut out = self;
        for i in 0..self.n.min(4) {
            out.v[i] = f(self.v[i]);
        }
        out
    }
}

/// Built-in names Iris 1.11.7's custom uniforms can't see, though the game may give them (most
/// are frame uniforms here). Our probe packs (#8995) read 0 from `(<name>) * 0.0 + 2.0` for
/// these, and 2 for 29 others (`skyColor.x`, `entityColor.x`, `blockEntityId`, `frameTime`...);
/// for `centerDepthSmooth`, Iris warns `Unknown variable` (#9126's probe).
const HIDDEN: [&str; 13] = [
    "alphaTestRef",
    "atlasSize",
    "blendFunc",
    "centerDepthSmooth",
    "dimension",
    "entityId",
    "fogColor",
    "fogDensity",
    "fogEnd",
    "fogMode",
    "fogShape",
    "fogStart",
    "renderStage",
];

/// Names Iris 1.11.7's custom uniforms see (our probe of #8995) that the game doesn't give yet:
/// such a name alone reads 0, and the expression keeps the rest.
const NOT_GIVEN_YET: [&str; 2] = ["currentRenderedItemId", "lightningBoltPosition"];

/// The `int` built-ins of Iris 1.11.7's custom uniforms (an int vector for `eyeBrightness`,
/// `eyeBrightnessSmooth` and the `...PositionInt`s). Our probe packs of #9126 wrote each name
/// alone into an `int`, a `float` and a `bool` uniform: Iris kept these in `int` and `float`.
const INT_NAMES: [&str; 21] = [
    "bedrockLevel",
    "biome",
    "biome_category",
    "biome_precipitation",
    "blockEntityId",
    "cameraPositionInt",
    "currentRenderedItemId",
    "eyeBrightness",
    "eyeBrightnessSmooth",
    "frameCounter",
    "heightLimit",
    "heldBlockLightValue",
    "heldBlockLightValue2",
    "heldItemId",
    "heldItemId2",
    "isEyeInWater",
    "logicalHeightLimit",
    "moonPhase",
    "previousCameraPositionInt",
    "worldDay",
    "worldTime",
];

/// The `bool` built-ins (the same probes: Iris kept these only in `bool`). Every other built-in
/// is a float there, kept only in `float`.
const BOOL_NAMES: [&str; 16] = [
    "feetInWater",
    "firstPersonCamera",
    "hasCeiling",
    "hasSkylight",
    "hideGUI",
    "inSwimmingAnimation",
    "isElytraFlying",
    "isRiding",
    "isRightHanded",
    "isSpectator",
    "is_burning",
    "is_hurt",
    "is_invisible",
    "is_on_ground",
    "is_sneaking",
    "is_sprinting",
];

/// The functions of Iris 1.11.7's custom uniforms that give a float whatever numbers they take
/// (our probe: into an `int` uniform, Iris refused each with numbers alone).
const FLOAT_FUNCTIONS: [&str; 25] = [
    "acos", "asin", "atan", "atan2", "cos", "degrees", "exp", "exp10", "exp2", "frac", "log",
    "log10", "log2", "mix", "pow", "radians", "random", "sign", "signum", "sin", "smooth", "sqrt",
    "tan", "todeg", "torad",
];

#[derive(Clone, Debug)]
enum Expr {
    /// A number, and whether it is written as a float (`2.0`, not `2`).
    Num(f64, bool),
    /// A name and its component (`x`, `2`, `2.1` as column 2, row 1).
    Name(String, Option<(usize, Option<usize>)>),
    Call(String, Vec<Expr>, usize),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug)]
struct Item {
    name: String,
    ty: String,
    uniform: bool,
    expr: Expr,
}

/// The pack's custom uniforms and variables, with the state `smooth` and `random` keep.
#[derive(Clone, Debug, Default)]
pub struct Custom {
    items: Vec<Item>,
    /// `smooth` state per call site.
    smooth: HashMap<usize, f64>,
    seed: u64,
    unknown: HashSet<String>,
    /// Per item, whether it reads 0 because it names something unknown (directly or through
    /// another item); worked out on the first `eval`.
    broken: Option<Vec<bool>>,
    /// `BIOME_*`, `CAT_*` and `PPT_*` (crate::biomes).
    constants: HashMap<String, f64>,
    /// Expressions that did not parse, and unknown names, as `key: message`.
    pub notes: Vec<String>,
    calls: usize,
    /// Whether `smooth` has started on a frame of the world (see [`Custom::world_frame`]).
    world_seen: bool,
}

impl Custom {
    /// Reads every `uniform.*` and `variable.*` key of `properties`, in file order.
    pub fn parse(properties: &Properties) -> Self {
        let mut out = Custom {
            seed: 0x9e37_79b9_7f4a_7c15,
            constants: crate::biomes::constants(&[]),
            ..Default::default()
        };
        for e in &properties.entries {
            let (uniform, rest) = match e.key.split_once('.') {
                Some(("uniform", r)) => (true, r),
                Some(("variable", r)) => (false, r),
                _ => continue,
            };
            let Some((ty, name)) = rest.split_once('.') else {
                continue;
            };
            if !matches!(ty, "float" | "int" | "bool" | "vec2" | "vec3" | "vec4") {
                out.notes
                    .push(format!("{}: type `{ty}` is not supported", e.key));
                continue;
            }
            // Tolerate one final terminator, while leaving extra statements to the parser.
            let value = e.value.trim();
            let value = value.strip_suffix(';').unwrap_or(value).trim_end();
            match Parser::new(value, &mut out.calls).expr_all() {
                Ok(expr) => out.items.push(Item {
                    name: name.to_owned(),
                    ty: ty.to_owned(),
                    uniform,
                    expr,
                }),
                Err(m) => out.notes.push(format!("{}: {m}", e.key)),
            }
        }
        out
    }

    /// Forgets what `smooth` keeps: each starts again from its next value, as on the first frame
    /// (for runs whose values are not the world's: the load-time dry run, frames before the
    /// camera's biome is known).
    pub fn restart_smoothing(&mut self) {
        self.smooth.clear();
    }

    /// Called before each frame's [`Custom::eval`], with whether the camera's biome is known (its
    /// chunk loaded). Until the first frame that knows it, `smooth` restarts every frame, so it
    /// starts from that frame's values rather than from the frames before it (the biome reads
    /// -1 there). From then on it keeps going for as long as the pack runs, through teleports
    /// and the frames whose chunk isn't loaded yet: in Iris 1.11.7, our probe pack's
    /// `smooth(cameraPosition.y + 200.0, 1000, 1000)` kept smoothing across teleports within a
    /// dimension, and started again when the player entered the Nether.
    pub fn world_frame(&mut self, biome_known: bool) {
        if !self.world_seen {
            self.restart_smoothing();
            self.world_seen = biome_known;
        }
    }

    /// Forgets the world's frames, as a pack just loaded has seen none: `smooth` restarts every
    /// frame again until one knows the camera's biome (`Renderer::reset_shader_pack_history`).
    pub fn forget_world(&mut self) {
        self.restart_smoothing();
        self.world_seen = false;
    }

    /// Restarts `smooth` and `random` for a frame of the world's tick `tick`, so a frame drawn
    /// again at that tick evaluates the same (`HORIZON_STILL`).
    pub fn restart_at(&mut self, tick: u64) {
        self.restart_smoothing();
        self.seed = (0x9e37_79b9_7f4a_7c15 ^ tick.wrapping_mul(0xbf58_476d_1ce4_e5b9)).max(1);
    }

    /// Names the game's biomes (registry identifiers, in id order) for `BIOME_*`.
    pub fn set_biomes(&mut self, biomes: &[&str]) {
        self.constants = crate::biomes::constants(biomes);
    }

    /// The uniforms' names and declared types, in file order.
    pub fn uniforms(&self) -> impl Iterator<Item = (&str, &str)> {
        self.items
            .iter()
            .filter(|i| i.uniform)
            .map(|i| (i.name.as_str(), i.ty.as_str()))
    }

    /// Evaluates everything for a frame `dt` seconds after the last; `builtin` gives the
    /// values of the game's uniforms and constants (`BIOME_*`...). Returns the uniforms'
    /// values, in the order of `uniforms`.
    pub fn eval(&mut self, dt: f64, builtin: &dyn Fn(&str) -> Option<Val>) -> Vec<Val> {
        if self.broken.is_none() {
            self.broken = Some(self.resolve(builtin));
        }
        let broken = self.broken.take().unwrap_or_default();
        let mut vars: HashMap<String, Val> = HashMap::new();
        let mut out = Vec::new();
        let items = std::mem::take(&mut self.items);
        for (item, &broken) in items.iter().zip(&broken) {
            let v = if broken {
                Val::scalar(0.0)
            } else {
                self.value(&item.expr, dt, &vars, builtin)
            };
            let v = match item.ty.as_str() {
                "int" => Val::scalar(v.x().trunc()),
                "bool" => Val::scalar(f64::from(u8::from(v.truth()))),
                "float" => Val::scalar(v.x()),
                t => {
                    let n = (t.as_bytes()[3] - b'0') as usize;
                    let mut v = v;
                    if v.n == 1 {
                        let x = v.v[0];
                        v.v[1..n].fill(x);
                    }
                    v.n = n;
                    v
                }
            };
            vars.insert(item.name.clone(), v);
            if item.uniform {
                out.push(v);
            }
        }
        self.items = items;
        self.broken = Some(broken);
        out
    }

    /// Which items read 0: those naming something neither an item, the game (outside
    /// [`HIDDEN`]) nor a constant gives, and then those naming such an item. Notes each once.
    fn resolve(&mut self, builtin: &dyn Fn(&str) -> Option<Val>) -> Vec<bool> {
        let items: HashSet<&str> = self.items.iter().map(|i| i.name.as_str()).collect();
        let known = |n: &str| {
            n == "pi"
                || n.starts_with("BIOME_")
                || self.constants.contains_key(n)
                || NOT_GIVEN_YET.contains(&n)
                || (!HIDDEN.contains(&n) && builtin(n).is_some())
        };
        let mut broken = vec![false; self.items.len()];
        let mut uses: Vec<Vec<&str>> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            let mut names = Vec::new();
            names_in(&item.expr, &mut names);
            if let Some(n) = names.iter().find(|n| !items.contains(*n) && !known(n)) {
                self.notes.push(format!(
                    "`{}` reads 0: `{n}` is unknown to custom uniforms",
                    item.name
                ));
                broken[i] = true;
            } else if let Err(why) = self.typed(item, builtin) {
                self.notes.push(format!("`{}` reads 0: {why}", item.name));
                broken[i] = true;
            }
            uses.push(names.into_iter().filter(|n| items.contains(n)).collect());
        }
        loop {
            let dead: HashSet<&str> = (self.items.iter().zip(&broken))
                .filter(|(_, b)| **b)
                .map(|(i, _)| i.name.as_str())
                .collect();
            let mut changed = false;
            for (i, item) in self.items.iter().enumerate() {
                if let Some(n) = uses[i].iter().find(|n| !broken[i] && dead.contains(*n)) {
                    self.notes.push(format!(
                        "`{}` reads 0: it uses `{n}`, which does",
                        item.name
                    ));
                    broken[i] = true;
                    changed = true;
                }
            }
            if !changed {
                return broken;
            }
        }
    }

    /// Whether `item`'s expression has its declared type, by the rules of [`Ty`].
    fn typed(&self, item: &Item, builtin: &dyn Fn(&str) -> Option<Val>) -> Result<(), String> {
        let name_ty = |n: &str| {
            if let Some(other) = self.items.iter().find(|i| i.name == n) {
                return Ty::declared(&other.ty);
            }
            let int = INT_NAMES.contains(&n);
            match builtin(n) {
                _ if n == "pi" => Ty::Float,
                _ if self.constants.contains_key(n) || n.starts_with("BIOME_") => Ty::Int,
                // The game doesn't give `lightningBoltPosition` yet: its size is unknown.
                None if n == "lightningBoltPosition" => Ty::Any,
                None | Some(Val { n: 1, .. }) if int => Ty::Int,
                None | Some(Val { n: 1, .. }) if BOOL_NAMES.contains(&n) => Ty::Bool,
                None | Some(Val { n: 1, .. }) => Ty::Float,
                Some(Val { n: c @ 2..=4, .. }) => Ty::Vector(c, int),
                Some(_) => Ty::Matrix,
            }
        };
        let got = ty(&item.expr, &name_ty)?;
        let want = Ty::declared(&item.ty);
        if got.fits(want) {
            Ok(())
        } else {
            Err(format!("it gives {got} where `{}` is declared", item.ty))
        }
    }

    fn random(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 11) as f64 / (1u64 << 53) as f64
    }

    fn value(
        &mut self,
        e: &Expr,
        dt: f64,
        vars: &HashMap<String, Val>,
        builtin: &dyn Fn(&str) -> Option<Val>,
    ) -> Val {
        let ev = |e: &Expr, s: &mut Self| s.value(e, dt, vars, builtin);
        match e {
            Expr::Num(x, _) => Val::scalar(*x),
            Expr::Name(name, component) => {
                let v = match name.as_str() {
                    "pi" => Some(Val::scalar(std::f64::consts::PI)),
                    n => vars
                        .get(n)
                        .copied()
                        .or_else(|| builtin(n))
                        .or_else(|| self.constants.get(n).map(|&x| Val::scalar(x))),
                };
                if v.is_none() && name.starts_with("BIOME_") {
                    if self.unknown.insert(name.clone()) {
                        self.notes
                            .push(format!("`{name}` names no biome; it reads -1"));
                    }
                    return Val::scalar(-1.0);
                }
                let Some(v) = v else {
                    if self.unknown.insert(name.clone()) {
                        self.notes.push(format!("`{name}` is unknown; it reads 0"));
                    }
                    return Val::scalar(0.0);
                };
                match component {
                    None => v,
                    // `m.c.r` is GLSL's m[c][r], stored column after column: packs transform a
                    // view vector to the world as `inverse.0.0 * x + inverse.1.0 * y + ...`.
                    Some((column, Some(row))) => Val::scalar(v.v[column * 4 + row]),
                    Some((i, None)) => Val::scalar(v.v[*i]),
                }
            }
            Expr::Neg(a) => ev(a, self).map(|x| -x),
            Expr::Not(a) => Val::scalar(f64::from(u8::from(!ev(a, self).truth()))),
            Expr::Bin(op, a, b) => {
                let (a, b) = (ev(a, self), ev(b, self));
                let bool_ = |c: bool| Val::scalar(f64::from(u8::from(c)));
                match *op {
                    "+" => a.zip(b, |x, y| x + y),
                    "-" => a.zip(b, |x, y| x - y),
                    "*" => a.zip(b, |x, y| x * y),
                    "/" => a.zip(b, |x, y| x / y),
                    "%" => a.zip(b, |x, y| x % y),
                    "<" => bool_(a.x() < b.x()),
                    "<=" => bool_(a.x() <= b.x()),
                    ">" => bool_(a.x() > b.x()),
                    ">=" => bool_(a.x() >= b.x()),
                    "==" => bool_(a.x() == b.x()),
                    "!=" => bool_(a.x() != b.x()),
                    "&&" => bool_(a.truth() && b.truth()),
                    _ => bool_(a.truth() || b.truth()),
                }
            }
            Expr::Call(f, args, _) if f == "if" => {
                // Only the conditions up to the first true one and its branch are evaluated: a
                // `smooth` in a branch not taken keeps its value and doesn't advance.
                let mut i = 0;
                while i + 1 < args.len() {
                    if ev(&args[i], self).truth() {
                        return ev(&args[i + 1], self);
                    }
                    i += 2;
                }
                args.last().map_or(Val::scalar(0.0), |e| ev(e, self))
            }
            Expr::Call(f, args, site) => {
                let a: Vec<Val> = args.iter().map(|e| ev(e, self)).collect();
                let arg = |i: usize| a.get(i).copied().unwrap_or(Val::scalar(0.0));
                let x = arg(0).x();
                let one = |g: fn(f64) -> f64| arg(0).map(g);
                match f.as_str() {
                    "sin" => one(f64::sin),
                    "cos" => one(f64::cos),
                    "tan" => one(f64::tan),
                    "asin" => one(f64::asin),
                    "acos" => one(f64::acos),
                    "atan" if a.len() == 2 => Val::scalar(x.atan2(arg(1).x())),
                    "atan" => one(f64::atan),
                    "atan2" => Val::scalar(x.atan2(arg(1).x())),
                    "torad" | "radians" => one(f64::to_radians),
                    "todeg" | "degrees" => one(f64::to_degrees),
                    "min" => a
                        .iter()
                        .copied()
                        .reduce(|p, q| p.zip(q, f64::min))
                        .unwrap_or(arg(0)),
                    "max" => a
                        .iter()
                        .copied()
                        .reduce(|p, q| p.zip(q, f64::max))
                        .unwrap_or(arg(0)),
                    // The upper bound first, then the lower: with min > max the lower wins, as
                    // our probes read in Iris 1.11.7 (`clamp(5.0, 3.0, 1.0)` gives 3, #9307).
                    "clamp" => arg(0).zip(arg(2), f64::min).zip(arg(1), f64::max),
                    "abs" => one(f64::abs),
                    "floor" => one(f64::floor),
                    "ceil" => one(f64::ceil),
                    "exp" => one(f64::exp),
                    "exp2" => one(f64::exp2),
                    "exp10" => one(|v| 10f64.powf(v)),
                    "frac" => one(|v| v - v.floor()),
                    "log" if a.len() == 2 => Val::scalar(arg(1).x().log(x)),
                    "log" => one(f64::ln),
                    "log2" => one(f64::log2),
                    "log10" => one(f64::log10),
                    "sqrt" => one(f64::sqrt),
                    "signum" | "sign" => one(|v| if v == 0.0 { 0.0 } else { v.signum() }),
                    "pow" => arg(0).zip(arg(1), f64::powf),
                    "fmod" => arg(0).zip(arg(1), |p, q| p - q * (p / q).floor()),
                    "mix" => arg(0).zip(arg(1), |p, q| p + (q - p) * arg(2).x()),
                    "edge" => Val::scalar(if arg(1).x() < x { 0.0 } else { 1.0 }),
                    "random" if a.len() == 2 => Val::scalar(x + (arg(1).x() - x) * self.random()),
                    "random" => Val::scalar(self.random()),
                    "randomInt" if a.len() == 2 => {
                        Val::scalar((x + (arg(1).x() - x) * self.random()).floor())
                    }
                    "randomInt" => Val::scalar((self.random() * f64::from(i32::MAX)).floor()),
                    "smooth" => {
                        // A first argument written as a number names the call when more
                        // follow (`smooth(1, x, 2)`); any other is the value (`smooth(x, 2)`).
                        let named = a.len() > 1 && matches!(args[0], Expr::Num(..));
                        let rest = &a[usize::from(named)..];
                        let at = |i: usize| rest.get(i).map(Val::x);
                        let val = at(0).unwrap_or(0.0);
                        let fade_in = at(1).unwrap_or(1.0);
                        let fade_out = at(2).unwrap_or(fade_in);
                        let state = self.smooth.entry(*site).or_insert(val);
                        let fade = if val > *state { fade_in } else { fade_out };
                        // A value it already holds stays as it is: an infinite one too, which
                        // the step below would turn into NaN (inf - inf). Our probe read
                        // `smooth(1.0 / 0.0, 5, 5)` as +inf in Iris 1.11.7.
                        if fade > 0.0 && *state != val {
                            *state = val + (*state - val) * 0.5f64.powf(dt / (fade * FADE_UNIT));
                        } else {
                            *state = val;
                        }
                        Val::scalar(*state)
                    }
                    "between" => {
                        Val::scalar(f64::from(u8::from(x >= arg(1).x() && x <= arg(2).x())))
                    }
                    "equals" => {
                        Val::scalar(f64::from(u8::from((x - arg(1).x()).abs() <= arg(2).x())))
                    }
                    "in" => Val::scalar(f64::from(u8::from(a[1..].iter().any(|v| v.x() == x)))),
                    "vec2" | "vec3" | "vec4" => {
                        let n = (f.as_bytes()[3] - b'0') as usize;
                        let mut c: Vec<f64> = a.iter().flat_map(|v| v.v[..v.n].to_vec()).collect();
                        if c.len() == 1 {
                            c = vec![c[0]; n];
                        }
                        c.resize(n, 0.0);
                        Val::vector(&c)
                    }
                    _ => {
                        if self.unknown.insert(f.clone()) {
                            self.notes.push(format!("`{f}()` is unknown; it gives 0"));
                        }
                        Val::scalar(0.0)
                    }
                }
            }
        }
    }
}

struct Parser<'a> {
    tokens: Vec<String>,
    at: usize,
    calls: &'a mut usize,
}

const OPERATORS: [&str; 18] = [
    "||", "&&", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "!", "(", ")", ",", ".",
];

impl<'a> Parser<'a> {
    fn new(text: &str, calls: &'a mut usize) -> Self {
        let mut tokens = Vec::new();
        let b = text.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i] as char;
            if c.is_whitespace() {
                i += 1;
            } else if c.is_ascii_digit()
                || (c == '.'
                    && b.get(i + 1).is_some_and(u8::is_ascii_digit)
                    && !tokens
                        .last()
                        .is_some_and(|t: &String| t == "." || is_name(t)))
            {
                let start = i;
                while i < b.len()
                    && (b[i].is_ascii_alphanumeric()
                        || b[i] == b'.'
                        || ((b[i] == b'-' || b[i] == b'+') && matches!(b[i - 1], b'e' | b'E')))
                {
                    i += 1;
                }
                tokens.push(text[start..i].to_owned());
            } else if c.is_ascii_alphabetic() || c == '_' {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                tokens.push(text[start..i].to_owned());
            } else {
                let op = OPERATORS
                    .iter()
                    .find(|o| text[i..].starts_with(**o))
                    .map_or(&text[i..i + c.len_utf8()], |o| o);
                i += op.len();
                tokens.push(op.to_owned());
            }
        }
        Parser {
            tokens,
            at: 0,
            calls,
        }
    }
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.at).map(String::as_str)
    }
    fn eat(&mut self, t: &str) -> bool {
        let hit = self.peek() == Some(t);
        self.at += usize::from(hit);
        hit
    }
    fn expr_all(&mut self) -> Result<Expr, String> {
        let e = self.binary(0)?;
        match self.peek() {
            None => Ok(e),
            Some(t) => Err(format!("unexpected `{t}`")),
        }
    }
    /// Precedence levels from the loosest: `||`, `&&`, equality, relations, sums, products.
    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        const LEVELS: [&[&str]; 6] = [
            &["||"],
            &["&&"],
            &["==", "!="],
            &["<", "<=", ">", ">="],
            &["+", "-"],
            &["*", "/", "%"],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(op) = self
            .peek()
            .and_then(|t| LEVELS[level].iter().find(|o| **o == t))
        {
            self.at += 1;
            let right = self.binary(level + 1)?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        self.eat("+");
        self.primary()
    }
    fn primary(&mut self) -> Result<Expr, String> {
        let Some(t) = self.peek().map(str::to_owned) else {
            return Err("the expression ends early".into());
        };
        self.at += 1;
        if t == "(" {
            let e = self.binary(0)?;
            return if self.eat(")") {
                Ok(e)
            } else {
                Err("missing `)`".into())
            };
        }
        if t.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            return t
                .trim_end_matches(['f', 'F'])
                .parse()
                .map(|x| Expr::Num(x, t.contains(['.', 'e', 'E', 'f', 'F'])))
                .map_err(|_| format!("bad number `{t}`"));
        }
        if !is_name(&t) {
            return Err(format!("unexpected `{t}`"));
        }
        if self.eat("(") {
            let mut args = Vec::new();
            if !self.eat(")") {
                loop {
                    args.push(self.binary(0)?);
                    if self.eat(")") {
                        break;
                    }
                    if !self.eat(",") {
                        return Err(format!("missing `)` after the arguments of `{t}`"));
                    }
                }
            }
            if t == "smooth" {
                smooth_arguments(&args)?;
            }
            *self.calls += 1;
            return Ok(Expr::Call(t, args, *self.calls));
        }
        let component = if self.eat(".") {
            let c = self.peek().map(str::to_owned).ok_or("nothing after `.`")?;
            self.at += 1;
            let index = |s: &str| match s {
                "x" | "r" | "s" | "0" => Some(0),
                "y" | "g" | "t" | "1" => Some(1),
                "z" | "b" | "p" | "2" => Some(2),
                "w" | "a" | "q" | "3" => Some(3),
                _ => None,
            };
            Some(match c.split_once('.') {
                Some((r, col)) => (
                    index(r).ok_or(format!("bad component `{c}`"))?,
                    Some(index(col).ok_or(format!("bad component `{c}`"))?),
                ),
                None => (index(&c).ok_or(format!("bad component `{c}`"))?, None),
            })
        } else {
            None
        };
        Ok(Expr::Name(t, component))
    }
}

/// The `smooth` calls Iris 1.11.7 resolves: `smooth(val)`, `smooth(val, fade)`,
/// `smooth(val, fadeIn, fadeOut)`, and the same with a number written first as the call's id
/// (`smooth(1, val, fadeIn, fadeOut)`). It drops a uniform with any other, which then reads 0.
fn smooth_arguments(args: &[Expr]) -> Result<(), String> {
    let named = args.len() > 1 && matches!(args[0], Expr::Num(..));
    if (1..=3).contains(&(args.len() - usize::from(named))) {
        Ok(())
    } else {
        Err(format!(
            "`smooth` with {} arguments{} (Iris reads it as 0)",
            args.len(),
            if named {
                ""
            } else {
                ", the first not a number"
            }
        ))
    }
}

/// The type of an expression, for the check Iris 1.11.7 makes before it keeps a custom uniform
/// or variable (#9086, #9126). The rules are a fit to our probe packs' values and warnings in
/// Iris, not a rule Iris documents:
/// - A number is an `int` (`2`) or a `float` (`2.0`). `pi` is a float, `BIOME_*`, `CAT_*` and
///   `PPT_*` are ints, and the built-ins have the types of [`INT_NAMES`] and [`BOOL_NAMES`].
///   `true` and `false` aren't names there at all.
/// - The only conversion is an `int` taken as a `float`: `float = 2` and `sin(0)` work, while
///   `int = 2.0`, `float = 1 > 0`, `bool = 1` and `vec2 = eyeBrightness` (an int vector) read 0.
/// - `+ - * %` of two ints give an int, with a float a float; `/` always gives a float
///   (`int = 7 / 2` reads 0). They take no bool. A vector meets only the same vector type:
///   `vec3 * 2.0`, `vec3 + vec2` and `eyeBrightness + vec2(...)` read 0. Unary minus keeps the
///   type and takes no bool.
/// - `< <= > >=` take numbers, `== !=` two numbers or two bools, `&& || !` bools; all give a
///   bool.
/// - `vecN(...)` takes exactly N numbers. `floor` and `ceil` give an int, and so does
///   `randomInt`, which takes ints only; `abs`, `min`, `max`, `clamp`, `fmod` and `edge` the
///   type of their numbers; [`FLOAT_FUNCTIONS`] a
///   float; `between`, `equals` and `in` a bool; `if` takes bool conditions and values of one
///   type (an int and a float give a float). `abs`, `floor`, `ceil`, `min`/`max` of two,
///   `clamp` and three-argument `if` also take vectors of one type; every other function only
///   scalars. `round`, `ifb`, `lerp` and `print` are no functions there (Iris warns "No such
///   function"), nor is any other name.
/// - A matrix counts only through a component (`m.0.1`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ty {
    /// A name whose type we don't know (`lightningBoltPosition`): it fits anything.
    Any,
    Int,
    Float,
    Bool,
    /// A vector of N components, of ints when `true` (`eyeBrightness`).
    Vector(usize, bool),
    Matrix,
}

impl Ty {
    fn declared(ty: &str) -> Self {
        match ty {
            "int" => Ty::Int,
            "bool" => Ty::Bool,
            "vec2" | "vec3" | "vec4" => Ty::Vector((ty.as_bytes()[3] - b'0') as usize, false),
            _ => Ty::Float,
        }
    }
    /// Whether a value of this type may stand where `want` is expected.
    fn fits(self, want: Ty) -> bool {
        self == want || self == Ty::Any || want == Ty::Any || (self, want) == (Ty::Int, Ty::Float)
    }
    fn vector(self) -> bool {
        matches!(self, Ty::Vector(..))
    }
    /// The type `types` share, `Any` aside, for vectors and bools.
    fn same(types: &[Ty]) -> Option<Ty> {
        let mut out = Ty::Any;
        for &t in types {
            if t != Ty::Any && out != Ty::Any && t != out {
                return None;
            }
            if t != Ty::Any {
                out = t;
            }
        }
        Some(out)
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Any => write!(f, "unknown"),
            Ty::Int => write!(f, "an int"),
            Ty::Float => write!(f, "a float"),
            Ty::Bool => write!(f, "a bool"),
            Ty::Vector(n, false) => write!(f, "a vec{n}"),
            Ty::Vector(n, true) => write!(f, "an ivec{n}"),
            Ty::Matrix => write!(f, "a matrix"),
        }
    }
}

/// Numbers only: a float when one of `types` is, else an int.
fn numbers(types: &[Ty], what: &str) -> Result<Ty, String> {
    let mut out = Ty::Any;
    for &t in types {
        match t {
            Ty::Any => {}
            Ty::Int if out != Ty::Float => out = Ty::Int,
            Ty::Int => {}
            Ty::Float => out = Ty::Float,
            _ => return Err(format!("{what} is given {t}")),
        }
    }
    Ok(out)
}

fn boolean(t: Ty, what: &str) -> Result<Ty, String> {
    if t.fits(Ty::Bool) {
        Ok(Ty::Bool)
    } else {
        Err(format!("{what} is given {t}"))
    }
}

/// The type of `e` (rules on [`Ty`]), or why it has none.
fn ty(e: &Expr, name: &dyn Fn(&str) -> Ty) -> Result<Ty, String> {
    match e {
        Expr::Num(_, float) => Ok(if *float { Ty::Float } else { Ty::Int }),
        Expr::Name(n, Some(_)) => Ok(match name(n) {
            Ty::Vector(_, true) => Ty::Int,
            Ty::Vector(_, false) | Ty::Matrix => Ty::Float,
            t => t,
        }),
        Expr::Name(n, None) => match name(n) {
            Ty::Matrix => Err(format!("the matrix `{n}` is used without a component")),
            t => Ok(t),
        },
        Expr::Neg(a) => match ty(a, name)? {
            Ty::Bool => Err("`-` is given a bool".into()),
            t => Ok(t),
        },
        Expr::Not(a) => boolean(ty(a, name)?, "`!`"),
        Expr::Bin(op, a, b) => {
            let (a, b) = (ty(a, name)?, ty(b, name)?);
            let what = format!("`{op}`");
            match *op {
                "+" | "-" | "*" | "/" | "%" if a.vector() || b.vector() => {
                    Ty::same(&[a, b]).ok_or_else(|| format!("`{op}` joins {a} and {b}"))
                }
                "+" | "-" | "*" | "%" => numbers(&[a, b], &what),
                "/" => numbers(&[a, b], &what).map(|_| Ty::Float),
                "&&" | "||" => boolean(a, &what).and(boolean(b, &what)),
                "==" | "!=" if a.fits(Ty::Bool) && b.fits(Ty::Bool) => Ok(Ty::Bool),
                _ => numbers(&[a, b], &what).map(|_| Ty::Bool),
            }
        }
        Expr::Call(f, args, _) => {
            let a = args
                .iter()
                .map(|e| ty(e, name))
                .collect::<Result<Vec<_>, _>>()?;
            let what = format!("`{f}`");
            let same = || Ty::same(&a).ok_or_else(|| format!("`{f}` mixes types"));
            let scalars = !a.iter().any(|t| t.vector());
            const OTHERS: [&str; 15] = [
                "vec2",
                "vec3",
                "vec4",
                "abs",
                "floor",
                "ceil",
                "min",
                "max",
                "clamp",
                "fmod",
                "edge",
                "randomInt",
                "between",
                "equals",
                "in",
            ];
            if !(OTHERS.contains(&f.as_str()) || f == "if" || FLOAT_FUNCTIONS.contains(&f.as_str()))
            {
                return Err(format!("`{f}()` is no function of custom uniforms"));
            }
            match f.as_str() {
                "vec2" | "vec3" | "vec4" => {
                    let n = (f.as_bytes()[3] - b'0') as usize;
                    match numbers(&a, &what) {
                        Ok(_) if a.len() == n => Ok(Ty::Vector(n, false)),
                        _ => Err(format!("`{f}` takes {n} numbers")),
                    }
                }
                "abs" | "floor" | "ceil" if a.len() == 1 && !scalars => Ok(a[0]),
                "min" | "max" if a.len() == 2 && !scalars => same(),
                "clamp" if a.len() == 3 && !scalars => same(),
                "if" if a.len() == 3 && !scalars => {
                    boolean(a[0], "the condition of `if`")?;
                    Ty::same(&a[1..]).ok_or_else(|| format!("`{f}` mixes types"))
                }
                _ if !scalars => Err(format!("`{f}` takes scalars only")),
                "floor" | "ceil" => numbers(&a, &what).map(|_| Ty::Int),
                "abs" | "min" | "max" | "clamp" | "fmod" | "edge" => numbers(&a, &what),
                "randomInt" if a.iter().all(|t| t.fits(Ty::Int)) => Ok(Ty::Int),
                "randomInt" => Err("`randomInt` takes ints only".into()),
                "between" | "equals" | "in" => numbers(&a, &what).map(|_| Ty::Bool),
                "if" => {
                    let mut values = Vec::new();
                    for (i, &t) in a.iter().enumerate() {
                        if i % 2 == 0 && i + 1 < a.len() {
                            boolean(t, "a condition of `if`")?;
                        } else {
                            values.push(t);
                        }
                    }
                    match Ty::same(&values) {
                        Some(Ty::Bool) => Ok(Ty::Bool),
                        _ => numbers(&values, &what),
                    }
                }
                _ => numbers(&a, &what).map(|_| Ty::Float),
            }
        }
    }
}

/// The names `e` reads, calls left out, in order.
fn names_in<'e>(e: &'e Expr, out: &mut Vec<&'e str>) {
    match e {
        Expr::Num(..) => {}
        Expr::Name(n, _) => out.push(n),
        Expr::Call(_, args, _) => args.iter().for_each(|a| names_in(a, out)),
        Expr::Neg(a) | Expr::Not(a) => names_in(a, out),
        Expr::Bin(_, a, b) => {
            names_in(a, out);
            names_in(b, out);
        }
    }
}

fn is_name(t: &str) -> bool {
    t.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Macros;

    fn custom(text: &str) -> Custom {
        Custom::parse(&Properties::parse(text, &Macros::standard()))
    }

    #[test]
    fn one_trailing_terminator_preserves_variable_and_uniform_values() {
        for text in [
            "variable.float.base = 0.25;\nuniform.float.answer = base * 2;\n",
            "variable.float.base = 0.25;  \nuniform.float.answer = base * 2;  \n",
            "variable.float.base = 0.25\nuniform.float.answer = base * 2\n",
        ] {
            let mut parsed = custom(text);
            assert!(parsed.notes.is_empty(), "{:?}", parsed.notes);
            assert_eq!(parsed.eval(0.016, &|_| None), [Val::scalar(0.5)]);
            assert!(parsed.notes.is_empty(), "{:?}", parsed.notes);
        }
    }

    #[test]
    fn terminator_does_not_allow_extra_statements_or_empty_expressions() {
        for expression in ["0.5; 0.1", "0.5;;", ";"] {
            let parsed = custom(&format!("uniform.float.answer = {expression}\n"));
            assert!(!parsed.notes.is_empty(), "{expression}");
            assert_eq!(parsed.uniforms().count(), 0);
        }
    }

    #[test]
    fn expressions_follow_c_precedence_and_file_order() {
        let mut c = custom(
            "variable.float.a = 1 + 2 * 3 - 4 % 3\n\
             uniform.float.b = a / 2\n\
             uniform.bool.c = a > 5 && !(b < 1) || false\n\
             uniform.int.d = -7 / 2\n\
             uniform.vec3.e = vec3(a, cameraPosition.y, 2) * 2\n\
             uniform.float.f = if(a < 0, 1, a < 7, 2, 3) + clamp(fmod(-1, 4), 0, 2.5) + min(4, 2, 3)\n\
             uniform.float.g = gbufferModelView.0.1 + frac(sunAngle - 0.25) + pi * 0\n\
             uniform.float.h = unknownThing + in(biome, 3, BIOME_DESERT)\n\
             uniform.bool.i = biome == BIOME_PALE_GARDEN || biome == BIOME_OLD\n\
             uniform.int.j = CAT_OCEAN + PPT_RAIN\n",
        );
        c.set_biomes(&["minecraft:beach", "minecraft:desert"]);
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        let names: Vec<_> = c.uniforms().map(|u| u.0).collect();
        assert_eq!(names, ["b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let builtin = |n: &str| match n {
            "cameraPosition" => Some(Val::vector(&[1.0, 64.0, 2.0])),
            "sunAngle" => Some(Val::scalar(0.125)),
            // Desert, as Iris numbers it.
            "biome" => Some(Val::scalar(5.0)),
            "gbufferModelView" => {
                let mut m = Val::scalar(0.0);
                m.v[1] = 9.0; // column 0, row 1
                m.n = 16;
                Some(m)
            }
            _ => None,
        };
        let v = c.eval(0.016, &builtin);
        let x: Vec<f64> = v.iter().map(|v| v.v[0]).collect();
        assert_eq!(
            x,
            [
                3.0,
                0.0,
                0.0,
                0.0,
                2.0 + 2.5 + 2.0,
                9.0 + 0.875,
                0.0,
                0.0,
                12.0
            ]
        );
        // Iris 1.11.7 doesn't take `false` (#9126), `int = -7 / 2` (`/` gives a float, #9126) or
        // `vec3 * 2` (a vector and a scalar, #9086).
        assert_eq!(&v[3].v[..3], &[0.0, 0.0, 0.0]);
        assert_eq!(
            c.notes,
            [
                "`c` reads 0: `false` is unknown to custom uniforms",
                "`d` reads 0: it gives a float where `int` is declared",
                "`e` reads 0: `*` joins a vec3 and an int",
                "`h` reads 0: `unknownThing` is unknown to custom uniforms",
                "`BIOME_PALE_GARDEN` names no biome; it reads -1",
                "`BIOME_OLD` names no biome; it reads -1"
            ]
        );
    }

    /// Iris 1.11.7, read back with probe packs of our own in three scenes: with the lower bound
    /// above the upper one, `clamp(5.0, 3.0, 1.0)`, `clamp(0.0, 3.0, 1.0)` and
    /// `clamp(2.0, 3.0, 1.0)` read 3, and `clamp(vec2(5.0, 0.5), vec2(3.0, 0.0), vec2(1.0, 1.0))`
    /// reads (3, 0.5).
    #[test]
    fn clamp_with_its_bounds_crossed_gives_the_lower_one() {
        let mut c = custom(
            "uniform.float.a = clamp(5.0, 3.0, 1.0)\n\
             uniform.float.b = clamp(0.0, 3.0, 1.0)\n\
             uniform.float.c = clamp(2.0, 3.0, 1.0)\n\
             uniform.float.d = clamp(2.0, 1.0, 3.0)\n\
             uniform.float.e = clamp(-2.0, 1.0, 3.0)\n\
             uniform.vec2.f = clamp(vec2(5.0, 0.5), vec2(3.0, 0.0), vec2(1.0, 1.0))\n",
        );
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        let v = c.eval(0.016, &|_| None);
        let x: Vec<f64> = v[..5].iter().map(|v| v.v[0]).collect();
        assert_eq!(x, [3.0, 3.0, 3.0, 2.0, 1.0]);
        assert_eq!(&v[5].v[..2], &[3.0, 0.5]);
    }

    /// Iris 1.11.7, read back with probe packs of our own after 3 s to 30 s in a scene:
    /// `smooth(1.0 / 0.0, 5, 5)` reads +inf, `smooth(-1.0 / 0.0, 5, 5)` -inf and
    /// `smooth(0.0 / 0.0, 5, 5)` NaN.
    #[test]
    fn smooth_keeps_an_infinite_value() {
        let mut c = custom(
            "uniform.float.a = smooth(1.0 / 0.0, 5, 5)\n\
             uniform.float.b = smooth(-1.0 / 0.0, 5, 5)\n\
             uniform.float.c = smooth(0.0 / 0.0, 5, 5)\n",
        );
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        for _ in 0..100 {
            let v = c.eval(0.016, &|_| None);
            assert_eq!(v[0].v[0], f64::INFINITY);
            assert_eq!(v[1].v[0], f64::NEG_INFINITY);
            assert!(v[2].v[0].is_nan());
        }
    }

    #[test]
    fn smooth_halves_the_distance_per_tenth_of_its_fade_time() {
        let mut c = custom("uniform.float.s = smooth(1, target, 20, 5)\n");
        let mut target = 0.0;
        let mut run = |t: f64, dt: f64| {
            target = t;
            c.eval(dt, &|n: &str| (n == "target").then(|| Val::scalar(target)))[0].v[0]
        };
        assert_eq!(run(0.0, 0.0), 0.0);
        assert!((run(1.0, 2.0) - 0.5).abs() < 1e-9, "rising: fadeIn");
        assert!((run(0.0, 0.5) - 0.25).abs() < 1e-9, "falling: fadeOut");
    }

    /// Iris 1.11.7 with a probe pack of our own, over the scene suite's weather: `weather rain`
    /// (the rain level climbs 0.01 a tick, vanilla's `ServerLevel.advanceWeatherCycle`) and a
    /// picture 9.1 s later, then `weather clear` and a picture 9.2 s after that, then one more
    /// 4.3 s on. Read there: `smooth(rainStrength, 20, 20)` 0.871, 0.118, 0.027;
    /// `smooth(rainStrength, 2, 10)` 1.0, 0.020, 0.0; `smooth(rainStrength, 5, 5)` 1.0, 0.0, 0.0.
    #[test]
    fn smooth_follows_the_rain_as_iris_does() {
        let mut c = custom(
            "uniform.float.a = smooth(rainStrength, 20, 20)\n\
             uniform.float.b = smooth(rainStrength, 2, 10)\n\
             uniform.float.c = smooth(rainStrength, 5, 5)\n",
        );
        let dt = 1.0 / 60.0;
        let (mut rain, mut read) = (0.0f64, Vec::new());
        for (seconds, target) in [(9.1, 1.0), (9.2, 0.0), (4.3, 0.0)] {
            for _ in 0..(seconds / dt) as usize {
                rain = if target > rain {
                    (rain + dt / 5.0).min(1.0)
                } else {
                    (rain - dt / 5.0).max(0.0)
                };
                let r = rain;
                read = c.eval(dt, &|n: &str| (n == "rainStrength").then(|| Val::scalar(r)));
            }
            let got: Vec<f64> = read.iter().map(|v| v.v[0]).collect();
            let iris = match target {
                1.0 => [0.871, 1.0, 1.0],
                _ if seconds > 9.0 => [0.118, 0.020, 0.0],
                _ => [0.027, 0.0, 0.0],
            };
            for (g, i) in got.iter().zip(iris) {
                assert!((g - i).abs() < 0.03, "{got:?} against Iris's {iris:?}");
            }
        }
    }

    /// Iris 1.11.7 with a probe pack of our own (#9371): a step in `worldTime / 24000` from 0.25
    /// to 0.75, read 1.355 s later, and a square wave's fall read 0.575 s later. Read there:
    /// fades 3, 10 and 30 give 0.7282, 0.5545 and 0.3844; the default fade 0.0186. Our probe's
    /// values fit a half-life of 0.1 s × the fade for every input tried (a fit, not Iris's code).
    #[test]
    fn smooth_steps_as_iris_does_for_other_inputs_and_the_default_fade() {
        let mut c = custom(
            "uniform.float.a = smooth(worldTime / 24000.0, 3.0)\n\
             uniform.float.b = smooth(worldTime / 24000.0, 10.0)\n\
             uniform.float.c = smooth(worldTime / 24000.0, 30.0)\n\
             uniform.float.d = smooth(if(worldTime > 15000, 0.0, 1.0))\n",
        );
        let mut hold = |time: f64, seconds: f64| {
            let mut read = Vec::new();
            for _ in 0..(seconds * 120.0).round() as usize {
                read = c.eval(1.0 / 120.0, &|n: &str| {
                    (n == "worldTime").then(|| Val::scalar(time))
                });
            }
            read.iter().map(|v| v.v[0]).collect::<Vec<f64>>()
        };
        hold(6000.0, 60.0);
        let got = hold(18000.0, 1.355);
        for (g, iris) in got.iter().zip([0.7282, 0.5545, 0.3844]) {
            assert!((g - iris).abs() < 0.01, "{got:?}: Iris read {iris}");
        }
        hold(6000.0, 60.0);
        let got = hold(18000.0, 0.575);
        assert!(
            (got[3] - 0.0186).abs() < 0.005,
            "default fade: {got:?}, Iris read 0.0186"
        );
    }

    /// Iris 1.11.7 with a probe pack of our own (#9371): our probe's Iris readings fit a `smooth`
    /// inside an `if` branch keeping its value while the branch isn't taken (within 0.008;
    /// updated every frame misses by up to 0.14). A fit, not Iris's code. The probe's
    /// `if(fmod(frameTimeCounter, 4) < 2, smooth(sq, 10), 0.5)`, with `sq` a square wave of 16 s
    /// (1 for 8 s, then 0), read in Iris at these points of the wave: 1.214 s 0.5962, 5.557 s
    /// 0.9205, 12.254 s 0.1965, 13.264 s 0.1041, 13.963 s 0.0603. Updated every frame, the
    /// smooth would read 0.571, 0.979, 0.052, 0.026 and 0.016 there.
    #[test]
    fn smooth_in_a_branch_not_taken_keeps_its_value() {
        let mut c = custom(
            "variable.float.sq = if(fmod(frameTimeCounter, 16.0) < 8.0, 1.0, 0.0)\n\
             uniform.float.br = if(fmod(frameTimeCounter, 4.0) < 2.0, smooth(sq, 10.0), 0.5)\n",
        );
        let dt = 1.0 / 120.0;
        let mut iris = vec![
            (1.214, 0.5962),
            (5.557, 0.9205),
            (12.254, 0.1965),
            (13.264, 0.1041),
            (13.963, 0.0603),
        ];
        for frame in 0..16 * 120 * 20 {
            let t = frame as f64 * dt;
            let got = c.eval(dt, &|n: &str| {
                (n == "frameTimeCounter").then(|| Val::scalar(t))
            })[0]
                .v[0];
            // The last of 20 periods: the wave has long forgotten where it started.
            let phase = t - 16.0 * 19.0;
            if let Some(i) = iris.iter().position(|p| (phase - p.0).abs() < dt / 2.0) {
                let (at, want) = iris.remove(i);
                assert!(
                    (got - want).abs() < 0.02,
                    "{got} at {at} s, Iris read {want}"
                );
            }
        }
        assert!(iris.is_empty(), "not reached: {iris:?}");
    }

    /// After a restart `smooth` starts from its next value, as on the first frame, instead of
    /// fading from the old one (a 1000-second fade from a dry run's zero).
    #[test]
    fn smooth_starts_again_from_its_next_value_after_a_restart() {
        let mut c = custom("uniform.float.s = smooth(target, 1000, 1000)\n");
        let eval = |c: &mut Custom, t: f64| {
            c.eval(0.05, &|n: &str| (n == "target").then(|| Val::scalar(t)))[0].v[0]
        };
        assert_eq!(eval(&mut c, 0.0), 0.0);
        assert!(eval(&mut c, 0.6) < 0.01, "the fade holds the old value");
        c.restart_smoothing();
        assert_eq!(eval(&mut c, 0.6), 0.6);
    }

    /// Iris 1.11.7, with a probe pack of our own (`scenes.json`'s caves after loading at the
    /// spawn): `smooth(cameraPosition.y + 200.0, 1000, 1000)` read 294.1, 291.1 and 288.1 in three
    /// scenes after teleports to y -5.4, -5.4 and 23.6, still fading from the spawn's 300, and
    /// `smooth(biome_category + 0.0, 1000, 1000)` read the biome's 10.0 in every scene. Horizon
    /// started both again after each teleport, from the frames before the chunk was loaded:
    /// 194.6 and 0.4 (the biome reads 0 there).
    #[test]
    fn smooth_starts_once_the_biome_is_known_and_keeps_going_through_teleports() {
        let mut c = custom(
            "uniform.float.cat = smooth(biome_category + 0.0, 1000, 1000)\n\
             uniform.float.y = smooth(cameraPosition.y + 200.0, 1000, 1000)\n",
        );
        let frame = |c: &mut Custom, known: bool, cat: f64, y: f64| {
            c.world_frame(known);
            let out = c.eval(0.05, &|n: &str| match n {
                "biome_category" => Some(Val::scalar(cat)),
                "cameraPosition" => Some(Val::vector(&[0.5, y, 0.5])),
                _ => None,
            });
            (out[0].v[0], out[1].v[0])
        };
        // Loading: the camera's chunk isn't there yet, so its biome reads 0.
        frame(&mut c, false, 0.0, 100.0);
        frame(&mut c, false, 0.0, 100.0);
        assert_eq!(frame(&mut c, true, 10.0, 100.0), (10.0, 300.0));
        // A teleport: a few frames before the chunk is there, then the scene.
        for _ in 0..3 {
            frame(&mut c, false, 0.0, -5.38);
        }
        let (cat, y) = (0..60).fold((0.0, 0.0), |_, _| frame(&mut c, true, 10.0, -5.38));
        assert!(cat > 9.9, "the biome's category from the start: {cat}");
        assert!(y > 290.0, "still fading from the spawn's 300: {y}");
        // Forgotten, as by a pack history reset: it starts again from the first known frame.
        c.forget_world();
        frame(&mut c, false, 0.0, 100.0);
        assert_eq!(frame(&mut c, true, 10.0, 100.0), (10.0, 300.0));
    }

    #[test]
    fn a_restart_at_a_tick_evaluates_that_tick_the_same_again() {
        let mut c = custom("uniform.float.r = random()\nuniform.float.s = smooth(target, 1000)\n");
        let eval = |c: &mut Custom, tick: u64, t: f64| {
            c.restart_at(tick);
            c.eval(0.05, &|n: &str| (n == "target").then(|| Val::scalar(t)))
        };
        let first = eval(&mut c, 7, 0.0);
        let other = eval(&mut c, 8, 0.6);
        assert_eq!(eval(&mut c, 7, 0.0), first);
        assert_ne!(other[0], first[0], "another tick draws another random");
        assert_eq!(other[1].v[0], 0.6, "smooth starts at its target");
    }

    /// Iris 1.11.7, with a probe pack of our own: a first argument written as a number is the
    /// call's id when more follow (`smooth(1, 5, f)` reads 5, `smooth(5, f)` reads f); any
    /// other first argument is the value (`smooth(1.5 * 2, 6.5)` reads 3).
    #[test]
    fn smooth_reads_a_leading_number_as_its_id() {
        let mut c = custom(
            "uniform.float.a = smooth(1, 5, 0.0001)\n\
             uniform.float.b = smooth(5, 0.0001)\n\
             uniform.float.c = smooth(2.5, 6, 0.0001)\n\
             uniform.float.d = smooth((2), 6, 0.0001)\n\
             uniform.float.e = smooth(y, 5, 0.0001)\n\
             uniform.float.f = smooth(1.5 * 2, 6.5)\n\
             uniform.float.g = smooth(-3, 6, 0.0001)\n\
             uniform.float.h = smooth(7, y)\n\
             uniform.float.i = smooth(4)\n",
        );
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        let v = c.eval(0.016, &|n: &str| (n == "y").then(|| Val::scalar(6.96)));
        let x: Vec<f64> = v.iter().map(|v| v.v[0]).collect();
        assert_eq!(x, [5.0, 0.0001, 6.0, 6.0, 6.96, 3.0, -3.0, 6.96, 4.0]);
    }

    /// Iris 1.11.7 drops a uniform whose `smooth` has four arguments and no number first, or
    /// five: it reads 0 there, as a uniform Horizon leaves out does.
    #[test]
    fn smooth_calls_iris_does_not_resolve_are_left_out() {
        let c = custom(
            "uniform.float.a = smooth(y, 5, 0.0001, 0.0001)\n\
             uniform.float.b = smooth(3, 6.5, 0.0001, 0.0001, 9)\n\
             uniform.float.c = smooth(3, 6.5, 0.0001, 0.0001)\n",
        );
        assert_eq!(c.uniforms().map(|u| u.0).collect::<Vec<_>>(), ["c"]);
        assert_eq!(c.notes.len(), 2, "{:?}", c.notes);
    }

    #[test]
    fn bad_expressions_are_reported() {
        let c = custom("uniform.float.a = (1 + \nuniform.mat4.m = 1\nuniform.float.ok = 2\n");
        assert_eq!(c.uniforms().count(), 1);
        assert_eq!(c.notes.len(), 2, "{:?}", c.notes);
    }

    /// Iris 1.11.7, with our probe pack of #8995: an expression naming something the custom
    /// uniforms can't see reads 0 as a whole, even in an `if` branch not taken, and so does
    /// anything using its uniform or variable.
    #[test]
    fn a_name_custom_uniforms_cannot_see_fails_the_whole_expression() {
        let mut c = custom(
            "variable.float.v0 = fogStart * 0.0 + 2.0\n\
             variable.float.v1 = 5.0\n\
             uniform.float.u0 = 2.0\n\
             uniform.float.u1 = dimension * 0.0 + 2.0\n\
             uniform.float.u2 = fogColor.r * 0.0 + 2.0\n\
             uniform.float.u3 = notAName * 0.0 + 2.0\n\
             uniform.float.u4 = if(1 > 0, 2.0, fogStart)\n\
             uniform.float.u5 = u0 + 1.0\n\
             uniform.float.u6 = u2 + 1.0\n\
             uniform.float.u7 = v0 + 1.0\n\
             uniform.float.u8 = v1 + 1.0\n\
             uniform.float.u9 = frameTime * 0.0 + 2.0\n\
             uniform.vec3.w = vec3(skyColor.r, entityColor.a, blockEntityId) * 0.0 + 2.0\n\
             uniform.float.r = currentRenderedItemId * 0.0 + lightningBoltPosition.x + 2.0\n",
        );
        // The game gives every name here but `notAName`, `currentRenderedItemId` and
        // `lightningBoltPosition`, as Horizon's frame uniforms do.
        let builtin = |n: &str| match n {
            "notAName" | "currentRenderedItemId" | "lightningBoltPosition" => None,
            "fogColor" | "skyColor" => Some(Val::vector(&[0.5, 0.6, 0.7])),
            "entityColor" => Some(Val::vector(&[0.0; 4])),
            _ => Some(Val::scalar(1.0)),
        };
        let x: Vec<f64> = c.eval(0.016, &builtin).iter().map(|v| v.v[0]).collect();
        // `w` reads 0 as well: `vec3 * 0.0` joins a vector and a scalar (#9086).
        assert_eq!(
            x,
            [2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 6.0, 2.0, 0.0, 2.0]
        );
        assert_eq!(c.notes.len(), 10, "{:?}", c.notes);
        // The same on later frames, with no new notes.
        let again: Vec<f64> = c.eval(0.016, &builtin).iter().map(|v| v.v[0]).collect();
        assert_eq!(again, x);
        assert_eq!(c.notes.len(), 10, "{:?}", c.notes);
    }

    /// Iris 1.11.7, with our probe packs of #9086 (`final` drew each uniform, a vector's `.y`):
    /// an expression without its declared shape, or joining a vector and a scalar, reads 0, and
    /// so does a uniform using such a variable. Each row: the probe's line, then what Iris read
    /// (`k5` reads our `skyColor.y` here; Iris read its own sky, 0.64). A variable isn't drawn:
    /// its number follows Iris's warnings (`vbad` failed) and the uniforms reading it.
    #[test]
    fn an_expression_without_its_declared_shape_reads_zero() {
        const ROWS: &str = "\
            uniform.float.k0 = 2.0 | 2
            uniform.float.k1 = skyColor * 0.0 + 2.0 | 0
            uniform.vec3.k2 = skyColor * 0.0 + 2.0 | 0
            uniform.float.k3 = skyColor.y * 0.0 + 2.0 | 2
            uniform.vec3.k4 = vec3(skyColor.x * 0.0 + 2.0, skyColor.y * 0.0 + 2.0, skyColor.z * 0.0 + 2.0) | 2
            uniform.vec3.k5 = skyColor | 0.5
            uniform.float.k6 = skyColor | 0
            uniform.vec3.k7 = vec3(2.0, 2.0, 2.0) * 1.0 | 0
            uniform.vec3.k8 = vec3(2.0, 2.0, 2.0) + 0.0 | 0
            uniform.vec3.k9 = skyColor * vec3(0.0, 0.0, 0.0) + vec3(2.0, 2.0, 2.0) | 2
            uniform.float.k10 = vec3(2.0, 2.0, 2.0) | 0
            uniform.vec2.k11 = vec3(2.0, 2.0, 2.0) | 0
            uniform.vec3.w0 = 2.0 * vec3(1.0, 1.0, 1.0) | 0
            uniform.vec3.w1 = vec3(4.0, 4.0, 4.0) / 2.0 | 0
            uniform.vec3.w19 = vec3(2.0, 2.0, 2.0) % 3.0 | 0
            uniform.vec3.w2 = -vec3(-2.0, -2.0, -2.0) | 2
            uniform.vec3.z11 = vec3(4.0, 4.0, 4.0) / vec3(2.0, 2.0, 2.0) | 2
            uniform.vec3.x0 = vec3(2.0, 2.0, 2.0) + vec2(0.0, 0.0) | 0
            uniform.vec3.x17 = -skyColor + skyColor + vec3(2.0, 2.0, 2.0) | 2
            uniform.bool.z0 = !vec3(0.0, 0.0, 0.0) | 0
            uniform.bool.z2 = vec3(1.0, 1.0, 1.0) < vec3(2.0, 2.0, 2.0) | 0
            uniform.vec3.w5 = 2.0 | 0
            uniform.vec3.w6 = vec3(2.0) | 0
            uniform.vec3.z10 = vec3(2.0, 2.0) | 0
            uniform.vec3.w7 = vec3(vec2(2.0, 2.0), 2.0) | 0
            uniform.vec3.x6 = vec3(vec3(2.0, 2.0, 2.0)) | 0
            uniform.vec2.x8 = vec2(frameCounter * 0 + 2, 2) | 2
            uniform.vec4.w4 = vec4(2.0, 2.0, 2.0, 2.0) | 2
            uniform.vec3.w11 = abs(vec3(-2.0, -2.0, -2.0)) | 2
            uniform.vec2.z5 = floor(vec3(2.5, 2.5, 2.5)) | 0
            uniform.float.z6 = abs(skyColor) | 0
            uniform.vec3.w10 = max(vec3(2.0, 2.0, 2.0), vec3(0.0, 0.0, 0.0)) | 2
            uniform.vec3.w9 = max(vec3(2.0, 2.0, 2.0), 0.0) | 0
            uniform.vec3.x12 = min(vec3(2.0, 2.0, 2.0), vec3(3.0, 3.0, 3.0), vec3(4.0, 4.0, 4.0)) | 0
            uniform.vec3.y3 = clamp(vec3(2.0, 2.0, 2.0), vec3(0.0, 0.0, 0.0), vec3(3.0, 3.0, 3.0)) | 2
            uniform.vec3.w12 = clamp(vec3(2.0, 2.0, 2.0), 0.0, 3.0) | 0
            uniform.vec3.y9 = sqrt(vec3(4.0, 4.0, 4.0)) | 0
            uniform.vec3.x3 = pow(vec3(2.0, 2.0, 2.0), 1.0) | 0
            uniform.vec3.x11 = smooth(vec3(2.0, 2.0, 2.0), 0.0001, 0.0001) | 0
            uniform.vec3.w8 = if(1.0 > 0.0, vec3(2.0, 2.0, 2.0), vec3(0.0, 0.0, 0.0)) | 2
            uniform.vec3.x19 = if(1.0 > 0.0, vec3(2.0, 2.0, 2.0), 0.0) | 0
            uniform.float.x9 = if(vec3(1.0, 1.0, 1.0), 2.0, 0.0) | 0
            uniform.vec3.z12 = if(1.0 < 0.0, vec3(0.0, 0.0, 0.0), 1.0 > 0.0, vec3(2.0, 2.0, 2.0), vec3(0.0, 0.0, 0.0)) | 0
            uniform.vec3.z9 = ifb(1.0 > 0.0, vec3(2.0, 2.0, 2.0), vec3(0.0, 0.0, 0.0)) | 0
            uniform.float.x15 = gbufferModelView * 0.0 + 2.0 | 0
            uniform.float.x16 = gbufferModelView.0.0 * 0.0 + 2.0 | 2
            variable.vec3.vbad = vec3(2.0, 2.0, 2.0) * 1.0 | 0
            uniform.float.w13 = vbad.y | 0
            variable.vec3.vgood = vec3(2.0, 2.0, 2.0) | 2
            uniform.float.w14 = vgood.y | 2
            uniform.vec3.w16 = vgood * vgood * vec3(0.5, 0.5, 0.5) | 2";
        let rows: Vec<(&str, f64)> = (ROWS.lines())
            .map(|l| l.trim().split_once(" | ").unwrap())
            .map(|(line, x)| (line, x.parse().unwrap()))
            .collect();
        let mut c = custom(
            &rows
                .iter()
                .map(|r| format!("{}\n", r.0))
                .collect::<String>(),
        );
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        let builtin = |n: &str| match n {
            "skyColor" => Some(Val::vector(&[0.25, 0.5, 0.75])),
            "gbufferModelView" => Some(Val::vector(&[0.0; 16])),
            "frameCounter" => Some(Val::scalar(7.0)),
            _ => None,
        };
        let got = c.eval(0.016, &builtin);
        let uniforms = rows.iter().filter(|r| r.0.starts_with("uniform."));
        assert_eq!(got.len(), uniforms.clone().count());
        for (v, (line, x)) in got.iter().zip(uniforms) {
            // A vector shows its `.y`, as the probe drew it; one reading 0 is 0 throughout.
            let shown = v.v[usize::from(v.n > 1)];
            assert!((shown - x).abs() < 1e-9, "{line}: {shown}, Iris {x}");
            assert!(*x != 0.0 || v.v[..v.n].iter().all(|&c| c == 0.0), "{line}");
        }
        let zeros = rows.iter().filter(|r| r.1 == 0.0).count();
        assert_eq!(c.notes.len(), zeros, "{:?}", c.notes);
    }

    /// Iris 1.11.7, with our probe packs of #9126 (`final` drew each uniform, a vector's `.y`):
    /// an expression whose type isn't the declared one (an `int` given a float, a `float` given a
    /// bool...), or that names `true`, `false` or a function Iris doesn't have, reads 0. Each row:
    /// the probe's line, what Iris read (`-` where it depends on the world: the built-ins), and
    /// `w` where Iris warned `Failed to resolve uniform`. Values are read from stripes, to 0.02.
    #[test]
    fn an_expression_without_its_declared_scalar_type_reads_zero() {
        const ROWS: &str = "\
            uniform.int.s0 = 2 | 2
            uniform.int.s1 = 2.0 | 0 | w
            uniform.float.s2 = 2 | 2
            uniform.int.s3 = 7 / 2 | 0 | w
            uniform.bool.s4 = 1 > 0 | 1
            uniform.float.s5 = 1 > 0 | 0 | w
            uniform.int.s6 = 1 > 0 | 0 | w
            uniform.bool.s7 = 2.0 | 0 | w
            uniform.float.s10 = frameCounter * 0 + 2 | 2
            uniform.int.t2 = 2 * 1.0 | 0 | w
            uniform.float.t4 = 7 / 2 | 3.5
            uniform.bool.t6 = 1 | 0 | w
            uniform.bool.t7 = true | 0 | w
            uniform.int.t8 = true | 0 | w
            uniform.float.t9 = true | 0 | w
            uniform.bool.t12 = !1 | 0 | w
            uniform.int.t17 = if(1 > 0, 2, 0) | 2
            uniform.int.t18 = round(2.4) | 0 | w
            uniform.bool.t21 = between(2.0, 1.0, 3.0) | 1
            uniform.float.t22 = between(2.0, 1.0, 3.0) | 0 | w
            uniform.int.t25 = max(2, 1.0) | 0 | w
            uniform.float.t26 = if(1 > 0, 2, 0) | 2
            uniform.float.q0 = if(1 < 0, 1.0, 1 > 0, 2.0, 0.0) | 2
            uniform.int.q1 = if(1 < 0, 1, 1 > 0, 2, 0) | 2
            uniform.float.q2 = randomInt(2.0, 2.0) | 0 | w
            uniform.bool.q4 = 1 > 0 || 1 | 0 | w
            uniform.bool.q11 = in(2.0, 1, 2) | 1
            uniform.vec2.v0 = eyeBrightness + vec2(0.0, 0.0) | 0 | w
            uniform.vec2.v1 = vec2(0.0, 0.0) + eyeBrightness | 0 | w
            uniform.vec2.v4 = eyeBrightness - eyeBrightness + vec2(2.0, 2.0) | 0 | w
            uniform.float.v5 = eyeBrightness.y / 100 | 2.4
            uniform.vec2.v6 = max(eyeBrightness, vec2(0.0, 0.0)) | 0 | w
            uniform.vec2.v7 = if(1 > 0, eyeBrightness, vec2(0.0, 0.0)) | 0 | w
            uniform.bool.v8 = (1 > 0) == 1 | 0 | w
            uniform.bool.v9 = 1 > (1 > 0) | 0 | w
            uniform.vec2.v14 = abs(eyeBrightness) | 0 | w
            uniform.int.v19 = fmod(5.0, 3) | 0 | w
            uniform.int.v20 = edge(1.5, 2.5) | 0 | w
            uniform.int.v21 = randomInt(2.0, 3.0) | 0 | w
            uniform.float.v25 = sin(1 > 0) | 0 | w
            uniform.float.v26 = between(1 > 0, 0, 2) * 0 + 2 | 0 | w
            uniform.int.o0 = (1 > 0) + 1 | 0 | w
            uniform.bool.o1 = (1 > 0) && 1 | 0 | w
            uniform.int.o5 = -(1 > 0) | 0 | w
            uniform.int.o6 = 5 % 3.0 | 0 | w
            uniform.int.o8 = if(1, 2, 0) | 0 | w
            uniform.float.o9 = if(1.0, 2.0, 0.0) | 0 | w
            uniform.int.o10 = if(1 > 0, 2, 1.0) | 0 | w
            uniform.float.o11 = if(1 > 0, 2, 1.0) | 2
            uniform.int.o14 = smooth(2, 0.0001) | 0 | w
            uniform.int.o16 = false | 0 | w
            uniform.bool.o17 = false | 0 | w
            uniform.vec2.o19 = eyeBrightness | 0 | w
            uniform.vec3.o20 = cameraPositionInt | 0 | w
            uniform.vec2.o21 = vec2(eyeBrightness.x, 2) | 2
            uniform.vec3.o22 = vec3(1 > 0, 2, 2) | 0 | w
            uniform.float.o24 = sin(0) + 2 | 2
            uniform.int.o25 = abs(-2.0) | 0 | w
            uniform.int.o27 = clamp(2, 0, 3.0) | 0 | w
            uniform.int.o30 = 2 + 0 * frameTimeCounter | 0 | w
            uniform.int.f0i = sin(1.5707963) | 0 | w
            uniform.int.f8i = torad(114.59156) | 0 | w
            uniform.float.f8f = torad(114.59156) | 2
            uniform.int.f16i = floor(2.5) | 2
            uniform.int.f17i = ceil(1.5) | 2
            uniform.int.f29i = pow(2, 1) | 0 | w
            uniform.int.f30i = fmod(5, 3) | 2
            uniform.int.f31i = lerp(0.5, 0, 4) | 0 | w
            uniform.float.f31f = lerp(0.5, 0, 4) | 0 | w
            uniform.int.f32i = mix(0, 4, 0.5) | 0 | w
            uniform.int.f33i = edge(1, 2) | 1
            uniform.bool.g0b = ifb(1 > 0, 1 > 0, 2 > 3) | 0 | w
            uniform.bool.g1b = ifb(1 > 0, 2, 3) | 0 | w
            uniform.bool.g2b = edge(1, 2) | 0 | w
            uniform.bool.g3b = between(2, 1, 3) | 1
            uniform.int.p0i = pi | 0 | w
            uniform.float.p0f = pi | 3.14
            uniform.bool.p0b = pi | 0 | w
            uniform.bool.p1b = BIOME_PLAINS | 0 | w
            uniform.int.p4i = notAFunction(2.0) | 0 | w
            uniform.int.b0i = frameCounter | -
            uniform.bool.b0b = frameCounter | 0 | w
            uniform.int.b4i = hideGUI | 0 | w
            uniform.bool.b4b = hideGUI | -
            uniform.int.b7i = frameTimeCounter | 0 | w
            uniform.bool.b7b = frameTimeCounter | 0 | w
            uniform.int.b19i = eyeBrightness.x | -
            uniform.bool.b19b = eyeBrightness.x | 0 | w
            uniform.int.b3_22i = centerDepthSmooth | 0 | w
            uniform.float.b3_22f = centerDepthSmooth | 0 | w
            uniform.bool.b3_22b = centerDepthSmooth | 0 | w
            uniform.int.b3_31i = currentRenderedItemId | -
            uniform.bool.b3_31b = currentRenderedItemId | 0 | w
            uniform.bool.b3_33b = worldTime | 0 | w";
        let rows: Vec<(&str, Option<f64>, bool)> = (ROWS.lines())
            .map(|l| {
                let mut p = l.trim().split(" | ");
                let line = p.next().unwrap();
                let x = p.next().unwrap().parse().ok();
                (line, x, p.next() == Some("w"))
            })
            .collect();
        let mut c = custom(
            &rows
                .iter()
                .map(|r| format!("{}\n", r.0))
                .collect::<String>(),
        );
        assert!(c.notes.is_empty(), "{:?}", c.notes);
        let builtin = |n: &str| match n {
            "frameCounter" | "worldTime" | "frameTimeCounter" => Some(Val::scalar(2.0)),
            "hideGUI" => Some(Val::scalar(1.0)),
            "sunAngle" => Some(Val::scalar(0.25)),
            "centerDepthSmooth" => Some(Val::scalar(0.5)),
            "eyeBrightness" => Some(Val::vector(&[0.0, 240.0])),
            "cameraPositionInt" => Some(Val::vector(&[1.0, 2.0, 3.0])),
            "cameraPosition" => Some(Val::vector(&[1.0, 64.0, 2.0])),
            _ => None,
        };
        let got = c.eval(0.016, &builtin);
        assert_eq!(got.len(), rows.len());
        for (v, (line, x, _)) in got.iter().zip(&rows) {
            let shown = v.v[usize::from(v.n > 1)];
            if let Some(x) = x {
                assert!((shown - x).abs() <= 0.02, "{line}: {shown}, Iris {x}");
            }
        }
        // Exactly the uniforms Iris warned about read 0, each with a note.
        let warned: Vec<&str> = (rows.iter().filter(|r| r.2))
            .map(|r| r.0.split(" = ").next().unwrap().rsplit('.').next().unwrap())
            .collect();
        let noted: Vec<&str> = (c.notes.iter())
            .filter(|n| n.contains("` reads 0: "))
            .map(|n| n[1..].split('`').next().unwrap())
            .collect();
        assert_eq!(noted, warned);
    }
}
