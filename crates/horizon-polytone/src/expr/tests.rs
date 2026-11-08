use super::*;

/// A player of age 5 standing in a jungle, a particle whose `custom` value can be set, and a
/// `config` that answers 50.
#[derive(Default)]
struct World {
    custom: f64,
    calls: Vec<String>,
}

impl Host for World {
    fn root(&mut self, root: Root) -> Value {
        match root {
            Root::Value => Value::Num(0.25),
            other => Value::Object(other as u32),
        }
    }
    fn method(&mut self, target: &Value, name: &str, args: &[Value]) -> Result<Value, String> {
        self.calls.push(name.to_owned());
        Ok(match (target, name) {
            (Value::Object(o), "age") if *o == Root::Player as u32 => Value::Num(5.0),
            (Value::Object(o), "biome") if *o == Root::Object as u32 => {
                Value::Str("minecraft:jungle".into())
            }
            (Value::Object(o), "custom") if *o == Root::Object as u32 => {
                if let Some(v) = args.first() {
                    self.custom = v.num();
                }
                Value::Num(self.custom)
            }
            _ => return Err(format!("no method {name}")),
        })
    }
    fn function(&mut self, name: &str, args: &[Value]) -> Result<Value, String> {
        match (name, args) {
            ("config", [Value::Str(id)]) if &**id == "pack:fog_opacity" => Ok(Value::Num(50.0)),
            _ => Err(format!("no function {name}")),
        }
    }
}

fn run(src: &str) -> Value {
    Program::compile(src)
        .unwrap()
        .eval(&mut World::default())
        .unwrap()
}
fn num(src: &str) -> f64 {
    run(src).num()
}

#[test]
fn operators_follow_the_usual_precedence() {
    assert_eq!(num("1 + 2 * 3"), 7.0);
    assert_eq!(num("(1 + 2) * 3"), 9.0);
    assert_eq!(num("10 - 4 - 3"), 3.0);
    assert_eq!(num("-2^2"), -4.0);
    assert_eq!(num("2^3^2"), 512.0);
    assert_eq!(num("0xFF8800"), 16_746_496.0);
}

#[test]
fn remainders_follow_java_and_mod_is_positive() {
    assert_eq!(num("-7 % 3"), -1.0);
    assert_eq!(num("mod(-7, 3)"), 2.0);
    assert_eq!(num("round(2.5) + round(-2.5)"), 1.0);
}

#[test]
fn comparisons_count_as_one_and_zero() {
    assert_eq!(num("(1 < 2) + (3 == 3) + (2 >= 5)"), 2.0);
    assert_eq!(num("0 ? 1 : 2"), 2.0);
    assert_eq!(num("1 > 0 && 0 || 1 ? 5 : 6"), 5.0);
    assert_eq!(run("!(1 == 1)"), Value::Bool(false));
}

#[test]
fn math_functions_and_constants() {
    assert!((num("lerp(1.0, 0.4, 0.5)") - 0.7).abs() < 1e-12);
    assert_eq!(num("smoothstep(0, 1, 0.5)"), 0.5);
    assert_eq!(num("clamp(5, -1, 2) + clamp(-5, -1, 2)"), 1.0);
    assert_eq!(num("inverseLerp(10, 20, 15)"), 0.5);
    assert!((num("sin(PI / 2) + cos(TAU)") - 2.0).abs() < 1e-12);
}

#[test]
fn locals_assignments_and_if_blocks() {
    assert_eq!(num("a = 2; a += 3; a * 2"), 10.0);
    assert_eq!(num("x = 4"), 4.0);
    let script = "n = 1; if (n == 2) { n = 10; } else if (n == 1) { n = 20; } else { n = 30; }; n";
    assert_eq!(num(script), 20.0);
    assert_eq!(num("s = 0; if (s > 0) { s = 1; } s"), 0.0);
}

#[test]
fn context_methods_take_any_of_their_spellings() {
    let mut world = World::default();
    let program = Program::compile("p.age + p.age() + player.getAge()").unwrap();
    assert_eq!(program.eval(&mut world).unwrap().num(), 15.0);
    assert_eq!(world.calls, ["age", "age", "age"]);
    assert_eq!(num("o.biome() == 'minecraft:jungle' ? 1 : 0"), 1.0);
    assert_eq!(num("object.biome == 'minecraft:desert'"), 0.0);
    assert_eq!(num("v * 4"), 1.0);
}

#[test]
fn methods_can_store_state_on_the_object() {
    assert_eq!(num("o.custom(3); o.custom() * 2"), 6.0);
}

#[test]
fn global_functions_go_to_the_host() {
    assert_eq!(num("config('pack:fog_opacity') / 100"), 0.5);
    let mut world = World::default();
    let missing = Program::compile("config('pack:other')").unwrap();
    assert!(missing.eval(&mut world).is_err());
}

#[test]
fn mistakes_fail_when_the_pack_loads() {
    for bad in [
        "foo + 1",
        "bar(1)",
        "sin(1, 2)",
        "x += 1",
        "(1 + 2",
        "if (1) { 2",
        "1 +",
        "o.",
        "'open",
        "a # b",
    ] {
        assert!(Program::compile(bad).is_err(), "{bad} should not compile");
    }
}

/// A particle of age 10 out of 40, at x 1, three blocks from a player at x 3, while it rains
/// half, with every random draw 0.25.
struct Particle;

impl Host for Particle {
    fn root(&mut self, root: Root) -> Value {
        Value::Object(root as u32)
    }
    fn method(&mut self, target: &Value, name: &str, args: &[Value]) -> Result<Value, String> {
        let on = |root: Root| *target == Value::Object(root as u32);
        Ok(Value::Num(match name {
            "age" if on(Root::Object) => 10.0,
            "life" if on(Root::Object) => 40.0,
            "x" if on(Root::Object) => 1.0,
            "x" if on(Root::Player) => 3.0,
            "distSq" if on(Root::Object) && args == [Value::Object(Root::Player as u32)] => 9.0,
            "rain" if on(Root::Global) => 0.5,
            "rand" if on(Root::Random) => 0.25,
            _ => return Err(format!("no method {name}")),
        }))
    }
    fn function(&mut self, name: &str, _: &[Value]) -> Result<Value, String> {
        Err(format!("no function {name}"))
    }
}

fn particle(src: &str) -> f64 {
    Program::compile(src)
        .unwrap()
        .eval(&mut Particle)
        .unwrap()
        .num()
}

#[test]
fn legacy_variables_run_as_their_scripting_calls() {
    assert_eq!(particle("AGE / LIFETIME"), 0.25);
    assert_eq!(particle("sqrt(DISTANCE_SQUARED) < 4"), 1.0);
    assert_eq!(particle("(RAIN == 0.5) + (PLAYER_X - POS_X)"), 3.0);
    assert_eq!(particle("rand() * 4"), 1.0);
    assert!((particle("sin(pi / 2) * e") - std::f64::consts::E).abs() < 1e-12);
    assert!(Program::compile("SUN_TIME * 2").is_err());
}

#[test]
fn legacy_expressions_keep_their_argument_order() {
    // Legacy `lerp(delta, start, end)` and `smoothstep(value, edge0, edge1)`...
    assert_eq!(particle("lerp(0.25, AGE, LIFETIME)"), 17.5);
    assert_eq!(particle("smoothstep(0.5, 0, 1) + AGE * 0"), 0.5);
    // ...and the scripting `lerp(start, end, t)`.
    assert_eq!(particle("lerp(10, 40, 0.25)"), 17.5);
    // A local named `e` is no legacy marker, and wins over the constant.
    assert_eq!(particle("e = 0.5; lerp(0, 4, e)"), 2.0);
}

#[test]
fn colors_pack_and_unpack_like_java_ints() {
    assert_eq!(num("red(0xFF336699)"), 0.2);
    assert_eq!(num("alpha(0x80000000)"), 128.0 / 255.0);
    // floor(0.5 * 255) = 127: 0xFFFF7F00, a negative Java int.
    assert_eq!(num("color(1, 0.5, 0, 1)"), -33_024.0);
    assert_eq!(num("color(1, 0, 0)"), -65_536.0);
    assert_eq!(num("red(color(1, 0.5, 0))"), 1.0);
    assert_eq!(num("withAlpha(0xFF0000, 0.5)"), 2_147_418_112.0);
    assert_eq!(num("lerpColor(0xFF000000, 0xFFFFFFFF, 0.5)"), -8_421_505.0);
}

#[test]
fn hue_saturation_and_brightness() {
    assert_eq!(num("hsb(0, 1, 1)"), -65_536.0);
    assert_eq!(num("hsb(1.0 / 3, 1, 1)"), -16_711_936.0);
    assert!((num("hue(0xFF00FF00)") - 1.0 / 3.0).abs() < 1e-12);
    assert_eq!(num("saturation(0xFF808080) + brightness(0xFFFF0000)"), 1.0);
    // Red to blue the short way passes through magenta, 0xFFFF00FF.
    assert_eq!(num("lerpHsb(0xFFFF0000, 0xFF0000FF, 0.5)"), -65_281.0);
}

#[test]
fn distances_between_two_points() {
    assert_eq!(num("dist(0, 0, 3, 4) + distSquare(0, 0, 3, 4)"), 30.0);
}
