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
//! - Unknown names, as in Iris 1.11.7 (#8995): an expression naming something the custom
//!   uniforms can't see (an unknown name, or one of [`HIDDEN`]) reads 0 as a whole, even when
//!   the name is in an `if` branch not taken, and so does every expression using its uniform or
//!   variable. Only the names of [`NOT_GIVEN_YET`] read 0 alone.

use std::collections::{HashMap, HashSet};

use crate::Properties;

/// The seconds one unit of a `smooth` fade time lasts. Fitted to Iris 1.11.7 with a probe pack of
/// our own (`smoothprobe`: `smooth(rainStrength, f, f)` for several fades, read across the scene
/// suite's weather changes): a half-life of a tenth of the fade time's seconds gives every
/// measured value within 0.016. Horizon's built-in smoothed uniforms use the same 0.1 s unit
/// (`pack_uniforms.rs` `SECONDS_A_TICK`).
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
/// these, and 2 for 29 others (`skyColor.x`, `entityColor.x`, `blockEntityId`, `frameTime`...).
const HIDDEN: [&str; 12] = [
    "alphaTestRef",
    "atlasSize",
    "blendFunc",
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

#[derive(Clone, Debug)]
enum Expr {
    Num(f64),
    /// A name and its component (`x`, `2`, `2.1` as column 2, row 1).
    Name(String, Option<(usize, Option<usize>)>),
    Call(String, Vec<Expr>, usize),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
}

#[derive(Debug)]
struct Item {
    name: String,
    ty: String,
    uniform: bool,
    expr: Expr,
}

/// The pack's custom uniforms and variables, with the state `smooth` and `random` keep.
#[derive(Debug, Default)]
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
            matches!(n, "pi" | "true" | "false")
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
            Expr::Num(x) => Val::scalar(*x),
            Expr::Name(name, component) => {
                let v = match name.as_str() {
                    "pi" => Some(Val::scalar(std::f64::consts::PI)),
                    "true" => Some(Val::scalar(1.0)),
                    "false" => Some(Val::scalar(0.0)),
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
                    "clamp" => arg(0).zip(arg(1), f64::max).zip(arg(2), f64::min),
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
                    "round" => one(f64::round),
                    "signum" | "sign" => one(|v| if v == 0.0 { 0.0 } else { v.signum() }),
                    "pow" => arg(0).zip(arg(1), f64::powf),
                    "fmod" => arg(0).zip(arg(1), |p, q| p - q * (p / q).floor()),
                    "lerp" => arg(1).zip(arg(2), |p, q| p + (q - p) * x),
                    "mix" => arg(0).zip(arg(1), |p, q| p + (q - p) * arg(2).x()),
                    "edge" => Val::scalar(if arg(1).x() < x { 0.0 } else { 1.0 }),
                    "random" if a.len() == 2 => Val::scalar(x + (arg(1).x() - x) * self.random()),
                    "random" => Val::scalar(self.random()),
                    "randomInt" if a.len() == 2 => {
                        Val::scalar((x + (arg(1).x() - x) * self.random()).floor())
                    }
                    "randomInt" => Val::scalar((self.random() * f64::from(i32::MAX)).floor()),
                    "if" | "ifb" => {
                        let mut i = 0;
                        while i + 1 < a.len() {
                            if a[i].truth() {
                                return a[i + 1];
                            }
                            i += 2;
                        }
                        a.last().copied().unwrap_or(Val::scalar(0.0))
                    }
                    "smooth" => {
                        // A first argument written as a number names the call when more
                        // follow (`smooth(1, x, 2)`); any other is the value (`smooth(x, 2)`).
                        let named = a.len() > 1 && matches!(args[0], Expr::Num(_));
                        let rest = &a[usize::from(named)..];
                        let at = |i: usize| rest.get(i).map(Val::x);
                        let val = at(0).unwrap_or(0.0);
                        let fade_in = at(1).unwrap_or(1.0);
                        let fade_out = at(2).unwrap_or(fade_in);
                        let state = self.smooth.entry(*site).or_insert(val);
                        let fade = if val > *state { fade_in } else { fade_out };
                        if fade > 0.0 {
                            *state = val + (*state - val) * 0.5f64.powf(dt / (fade * FADE_UNIT));
                        } else {
                            *state = val;
                        }
                        Val::scalar(*state)
                    }
                    "print" => arg(2),
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
                .map(Expr::Num)
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
    let named = args.len() > 1 && matches!(args[0], Expr::Num(_));
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

/// The names `e` reads, calls left out, in order.
fn names_in<'e>(e: &'e Expr, out: &mut Vec<&'e str>) {
    match e {
        Expr::Num(_) => {}
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
             uniform.int.i = biome == BIOME_PALE_GARDEN || biome == BIOME_OLD\n\
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
                1.0,
                -3.0,
                12.0,
                2.0 + 2.5 + 2.0,
                9.0 + 0.875,
                0.0,
                0.0,
                12.0
            ]
        );
        assert_eq!(&v[3].v[..3], &[12.0, 128.0, 4.0]);
        assert_eq!(
            c.notes,
            [
                "`h` reads 0: `unknownThing` is unknown to custom uniforms",
                "`BIOME_PALE_GARDEN` names no biome; it reads -1",
                "`BIOME_OLD` names no biome; it reads -1"
            ]
        );
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
        assert_eq!(
            x,
            [2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 6.0, 2.0, 2.0, 2.0]
        );
        assert_eq!(c.notes.len(), 9, "{:?}", c.notes);
        // The same on later frames, with no new notes.
        let again: Vec<f64> = c.eval(0.016, &builtin).iter().map(|v| v.v[0]).collect();
        assert_eq!(again, x);
        assert_eq!(c.notes.len(), 9, "{:?}", c.notes);
    }
}
