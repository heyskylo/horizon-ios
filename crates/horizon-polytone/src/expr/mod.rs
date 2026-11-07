//! Scripting expressions (wiki "Scripting Expressions"): `o.age() * 0.1`, ternaries, `&&`, local
//! variables, `if` blocks and calls on context objects. An expression is parsed once when the pack
//! loads, with every name resolved, and then evaluated against a [`Host`], the game side that
//! answers context objects (`o`, `p`, `g`...) and their methods.
mod lex;
mod parse;

use parse::{BinOp, Expr, Stmt};
use std::rc::Rc;

/// A value while an expression runs. Booleans count as 1 and 0 in arithmetic.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Num(f64),
    Bool(bool),
    Str(Rc<str>),
    /// A context object the host hands out (a block, an entity...), by the host's own id.
    Object(u32),
    Null,
}

impl Value {
    /// The number a value stands for: booleans are 1 and 0, anything else not a number is 0.
    pub fn num(&self) -> f64 {
        match self {
            Self::Num(n) => *n,
            Self::Bool(b) => f64::from(u8::from(*b)),
            _ => 0.0,
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Self::Num(n) => *n != 0.0,
            Self::Bool(b) => *b,
            Self::Str(s) => !s.is_empty(),
            Self::Object(_) => true,
            Self::Null => false,
        }
    }
}

/// The context objects an expression starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    /// `object` / `o`: the block, particle or entity the expression is about.
    Object,
    Player,
    Camera,
    Global,
    Random,
    /// `value` / `v`: the number the feature hands in, such as the attribute value so far.
    Value,
}

impl Root {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "object" | "o" => Self::Object,
            "player" | "p" => Self::Player,
            "camera" | "c" => Self::Camera,
            "global" | "g" => Self::Global,
            "random" | "r" => Self::Random,
            "value" | "v" => Self::Value,
            _ => return None,
        })
    }
}

/// What the game answers while an expression runs.
pub trait Host {
    fn root(&mut self, root: Root) -> Value;
    /// `target.name(args)`, with `name` without its `get` prefix. An unknown method is an
    /// error the host reports.
    fn method(&mut self, target: &Value, name: &str, args: &[Value]) -> Result<Value, String>;
    /// A global function of the game's (`config`, `colormap`, `modOn`, `date*`).
    fn function(&mut self, name: &str, args: &[Value]) -> Result<Value, String>;
}

type MathFn = fn(&[f64]) -> f64;

/// The math functions (name, arity, function), the wiki's "Math Functions" in Java's `Math`
/// terms: `round` rounds half up, `%` keeps the dividend's sign and `mod` is always positive.
pub(crate) const MATH: [(&str, usize, MathFn); 25] = [
    ("sin", 1, |a| a[0].sin()),
    ("cos", 1, |a| a[0].cos()),
    ("tan", 1, |a| a[0].tan()),
    ("atan2", 2, |a| a[0].atan2(a[1])),
    ("sqrt", 1, |a| a[0].sqrt()),
    ("abs", 1, |a| a[0].abs()),
    ("log", 1, |a| a[0].ln()),
    ("exp", 1, |a| a[0].exp()),
    ("pow", 2, |a| a[0].powf(a[1])),
    ("floor", 1, |a| a[0].floor()),
    ("ceil", 1, |a| a[0].ceil()),
    ("round", 1, |a| (a[0] + 0.5).floor()),
    ("fract", 1, |a| a[0] - a[0].floor()),
    ("sign", 1, |a| {
        if a[0] == 0.0 || a[0].is_nan() {
            a[0]
        } else {
            a[0].signum()
        }
    }),
    ("radians", 1, |a| a[0].to_radians()),
    ("degrees", 1, |a| a[0].to_degrees()),
    ("mod", 2, |a| a[0].rem_euclid(a[1])),
    ("min", 2, |a| a[0].min(a[1])),
    ("max", 2, |a| a[0].max(a[1])),
    ("clamp", 3, |a| a[0].max(a[1]).min(a[2])),
    ("square", 1, |a| a[0] * a[0]),
    ("cube", 1, |a| a[0] * a[0] * a[0]),
    ("lerp", 3, |a| a[0] + (a[1] - a[0]) * a[2]),
    ("inverseLerp", 3, |a| (a[2] - a[0]) / (a[1] - a[0])),
    ("smoothstep", 3, |a| {
        let t = ((a[2] - a[0]) / (a[1] - a[0])).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }),
];

/// A parsed expression, ready to run many times.
#[derive(Debug)]
pub struct Program {
    body: Vec<Stmt>,
    locals: usize,
}

impl Program {
    /// Parses `src`; a syntax error or an unknown name fails here.
    pub fn compile(src: &str) -> Result<Self, String> {
        let (body, locals) = parse::parse(src)?;
        Ok(Self { body, locals })
    }

    /// Runs the statements; the result is the last statement's value (an assignment's is the
    /// value it stored).
    pub fn eval(&self, host: &mut dyn Host) -> Result<Value, String> {
        let mut run = Run {
            host,
            locals: vec![Value::Null; self.locals],
            last: Value::Null,
        };
        run.block(&self.body)?;
        Ok(run.last)
    }
}

struct Run<'h> {
    host: &'h mut dyn Host,
    locals: Vec<Value>,
    last: Value,
}

fn binary(op: BinOp, a: &Value, b: &Value) -> Value {
    let (x, y) = (a.num(), b.num());
    let equal = match (a, b) {
        (Value::Str(s), Value::Str(t)) => s == t,
        (Value::Object(i), Value::Object(j)) => i == j,
        (Value::Null, Value::Null) => true,
        (Value::Str(_) | Value::Object(_) | Value::Null, _)
        | (_, Value::Str(_) | Value::Object(_) | Value::Null) => false,
        _ => x == y,
    };
    match op {
        BinOp::Add => Value::Num(x + y),
        BinOp::Sub => Value::Num(x - y),
        BinOp::Mul => Value::Num(x * y),
        BinOp::Div => Value::Num(x / y),
        BinOp::Rem => Value::Num(x % y),
        BinOp::Pow => Value::Num(x.powf(y)),
        BinOp::Eq => Value::Bool(equal),
        BinOp::Ne => Value::Bool(!equal),
        BinOp::Lt => Value::Bool(x < y),
        BinOp::Le => Value::Bool(x <= y),
        BinOp::Gt => Value::Bool(x > y),
        BinOp::Ge => Value::Bool(x >= y),
    }
}

impl Run<'_> {
    fn block(&mut self, body: &[Stmt]) -> Result<(), String> {
        for stmt in body {
            match stmt {
                Stmt::Expr(e) => self.last = self.expr(e)?,
                Stmt::Assign(slot, op, e) => {
                    let v = self.expr(e)?;
                    let v = match op {
                        Some(op) => binary(*op, &self.locals[*slot], &v),
                        None => v,
                    };
                    self.locals[*slot] = v.clone();
                    self.last = v;
                }
                Stmt::If(branches, otherwise) => {
                    let mut taken = None;
                    for (condition, body) in branches {
                        if self.expr(condition)?.truthy() {
                            taken = Some(body);
                            break;
                        }
                    }
                    self.block(taken.unwrap_or(otherwise))?;
                }
            }
        }
        Ok(())
    }

    fn all(&mut self, args: &[Expr]) -> Result<Vec<Value>, String> {
        args.iter().map(|a| self.expr(a)).collect()
    }

    fn expr(&mut self, e: &Expr) -> Result<Value, String> {
        Ok(match e {
            Expr::Const(v) => v.clone(),
            Expr::Local(slot) => self.locals[*slot].clone(),
            Expr::Root(root) => self.host.root(*root),
            Expr::Neg(a) => Value::Num(-self.expr(a)?.num()),
            Expr::Not(a) => Value::Bool(!self.expr(a)?.truthy()),
            Expr::Binary(op, a, b) => {
                let a = self.expr(a)?;
                binary(*op, &a, &self.expr(b)?)
            }
            Expr::And(a, b) => Value::Bool(self.expr(a)?.truthy() && self.expr(b)?.truthy()),
            Expr::Or(a, b) => Value::Bool(self.expr(a)?.truthy() || self.expr(b)?.truthy()),
            Expr::Cond(c, a, b) => {
                let pick = if self.expr(c)?.truthy() { a } else { b };
                self.expr(pick)?
            }
            Expr::Math(i, args) => {
                let args: Vec<f64> = self.all(args)?.iter().map(Value::num).collect();
                Value::Num(MATH[*i].2(&args))
            }
            Expr::Host(name, args) => {
                let args = self.all(args)?;
                self.host.function(name, &args)?
            }
            Expr::Method(target, name, args) => {
                let target = self.expr(target)?;
                let args = self.all(args)?;
                self.host.method(&target, name, &args)?
            }
        })
    }
}

#[cfg(test)]
mod tests;
