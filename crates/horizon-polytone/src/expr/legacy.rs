//! Legacy math expressions (wiki "Math Expressions" and "Legacy Math Expressions"), which older
//! packs still use: uppercase variables (`AGE`, `DISTANCE_SQUARED`), `pi` and `e`, and bare
//! `rand()`, `gaussian()` and `noise(a, b)`. Each variable becomes the scripting call the wiki's
//! table names (`DISTANCE_SQUARED` is `o.distSq(p)`), so both syntaxes run the same way.
use super::lex::Token;
use super::parse::{BinOp, Expr};
use super::{MathFn, Root, Value, colors};

/// Functions whose legacy form differs from, or doesn't exist in, the scripting one: `lerp`
/// takes the delta first, `smoothstep` the value first, `color` the alpha first.
pub(super) const FUNCTIONS: &[(&str, usize, usize, MathFn)] = &[
    ("lerp", 3, 3, |a| a[1] + (a[2] - a[1]) * a[0]),
    ("smoothstep", 3, 3, |a| {
        let t = ((a[0] - a[1]) / (a[2] - a[1])).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }),
    ("step", 2, 2, |a| f64::from(u8::from(a[0] >= a[1]))),
    ("color", 4, 4, |a| colors::color(&[a[1], a[2], a[3], a[0]])),
    ("signum", 1, 1, |a| {
        if a[0] == 0.0 || a[0].is_nan() {
            a[0]
        } else {
            a[0].signum()
        }
    }),
    ("acos", 1, 1, |a| a[0].acos()),
    ("asin", 1, 1, |a| a[0].asin()),
    ("atan", 1, 1, |a| a[0].atan()),
    ("cbrt", 1, 1, |a| a[0].cbrt()),
    ("cosh", 1, 1, |a| a[0].cosh()),
    ("sinh", 1, 1, |a| a[0].sinh()),
    ("tanh", 1, 1, |a| a[0].tanh()),
    ("log10", 1, 1, |a| a[0].log10()),
    ("log2", 1, 1, |a| a[0].log2()),
];

/// The legacy calls that are random draws: `r.rand()`, `r.gaussian()`, `r.noise(a, b)` now.
const RANDOM: [&str; 3] = ["rand", "gaussian", "noise"];

const VARIABLES: [&str; 36] = [
    "TIME",
    "DAY_TIME",
    "SUN_TIME",
    "RAIN",
    "SEASON",
    "RENDER_DISTANCE",
    "POS_X",
    "POS_Y",
    "POS_Z",
    "X",
    "Y",
    "Z",
    "PLAYER_X",
    "PLAYER_Y",
    "PLAYER_Z",
    "DX",
    "DY",
    "DZ",
    "TEMPERATURE",
    "DOWNFALL",
    "SKY_LIGHT",
    "BLOCK_LIGHT",
    "PLAYER_SPEED_SQUARED",
    "DISTANCE_SQUARED",
    "AGE",
    "LIFETIME",
    "SIZE",
    "SCALE",
    "ROLL",
    "CUSTOM",
    "COLOR",
    "SPEED",
    "RED",
    "GREEN",
    "BLUE",
    "ALPHA",
];

/// Whether `tokens` are a legacy expression, which picks the legacy argument order of `lerp`,
/// `smoothstep` and `color`: they name an uppercase legacy variable, or call a function only
/// the legacy syntax has, other than as a method (`r.rand()` is scripting). `pi` and `e` don't
/// decide it: a script may have a local named `e`.
pub(super) fn is_legacy(tokens: &[(Token, usize)]) -> bool {
    tokens.iter().enumerate().any(|(i, (t, _))| {
        let Token::Ident(name) = t else { return false };
        let method = i > 0 && tokens[i - 1].0 == Token::Punct(".");
        let call = tokens.get(i + 1).is_some_and(|n| n.0 == Token::Punct("("));
        let only_legacy_call = RANDOM.contains(&name.as_str())
            || FUNCTIONS
                .iter()
                .any(|f| f.0 == name && !super::MATH.iter().any(|m| m.0 == name));
        !method && (VARIABLES.contains(&name.as_str()) || (call && only_legacy_call))
    })
}

fn method(root: Root, name: &str, args: Vec<Expr>) -> Expr {
    Expr::Method(Box::new(Expr::Root(root)), name.into(), args)
}

/// A bare legacy call that is a random draw.
pub(super) fn random(name: &str, args: Vec<Expr>) -> Option<Expr> {
    RANDOM
        .contains(&name)
        .then(|| method(Root::Random, name, args))
}

/// A legacy variable as the scripting expression the wiki maps it to. Any expression may use
/// them; they come after its locals and context objects.
pub(super) fn variable(name: &str) -> Option<Result<Expr, String>> {
    use Root::{Global, Object, Player};
    let o = |m: &str| method(Object, m, Vec::new());
    Some(Ok(match name {
        "pi" => Expr::Const(Value::Num(std::f64::consts::PI)),
        "e" => Expr::Const(Value::Num(std::f64::consts::E)),
        "TIME" => method(Global, "time", Vec::new()),
        "DAY_TIME" => method(Global, "dayTime", Vec::new()),
        "RAIN" => method(Global, "rain", Vec::new()),
        "SEASON" => method(Global, "seasonNumber", Vec::new()),
        "RENDER_DISTANCE" => method(Global, "renderDistance", Vec::new()),
        "POS_X" | "X" => o("x"),
        "POS_Y" | "Y" => o("y"),
        "POS_Z" | "Z" => o("z"),
        "PLAYER_X" => method(Player, "x", Vec::new()),
        "PLAYER_Y" => method(Player, "y", Vec::new()),
        "PLAYER_Z" => method(Player, "z", Vec::new()),
        "PLAYER_SPEED_SQUARED" => method(Player, "speedSq", Vec::new()),
        "DISTANCE_SQUARED" => method(Object, "distSq", vec![Expr::Root(Player)]),
        "DX" => o("xd"),
        "DY" => o("yd"),
        "DZ" => o("zd"),
        "TEMPERATURE" => o("temperature"),
        "DOWNFALL" => o("downfall"),
        "SKY_LIGHT" => o("skyLight"),
        "BLOCK_LIGHT" => o("blockLight"),
        "AGE" => o("age"),
        "LIFETIME" => o("life"),
        "SIZE" | "SCALE" => o("size"),
        "ROLL" => o("roll"),
        "CUSTOM" => o("custom"),
        "COLOR" => o("color"),
        "RED" => o("red"),
        "GREEN" => o("green"),
        "BLUE" => o("blue"),
        "ALPHA" => o("alpha"),
        "SPEED" => {
            let square = |m: &str| Expr::Binary(BinOp::Mul, Box::new(o(m)), Box::new(o(m)));
            let sum = Expr::Binary(
                BinOp::Add,
                Box::new(Expr::Binary(
                    BinOp::Add,
                    Box::new(square("xd")),
                    Box::new(square("yd")),
                )),
                Box::new(square("zd")),
            );
            Expr::Call(|a| a[0].sqrt(), vec![sum])
        }
        "SUN_TIME" => return Some(Err("SUN_TIME has no scripting equivalent".into())),
        _ => return None,
    }))
}
