//! The numeric fast path: a program of one expression whose every part is a number (constants,
//! arithmetic, comparisons of numbers, math functions, ternaries and `root.method(...)` calls) is
//! compiled again into a tree of `f64`s, which runs without the interpreter's `Value`s and
//! `Result`s. It answers exactly what the interpreter would: anything that could tell them apart
//! (strings, objects, `==` on a method's value, locals, game functions) keeps the interpreter.

use super::parse::{BinOp, Expr, Stmt};
use super::{Host, MathFn, Root, Value};
use std::sync::Arc;

/// What a node gives: always a number, always a boolean, or whatever a host method answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Num,
    Bool,
    Unknown,
}

/// A method argument, handed to the host as the interpreter would: a constant of any kind
/// (`o.custom('glow')`), another method's own value, or a number or boolean.
#[derive(Debug)]
pub(super) enum Arg {
    Value(Value),
    Method(Num),
    Num(Num, bool),
}

#[derive(Debug)]
pub(super) enum Num {
    Const(f64),
    Neg(Box<Num>),
    Not(Box<Num>),
    Bin(BinOp, Box<Num>, Box<Num>),
    And(Box<Num>, Box<Num>),
    Or(Box<Num>, Box<Num>),
    Cond(Box<Num>, Box<Num>, Box<Num>),
    Call(MathFn, Vec<Num>),
    Method(Root, Arc<str>, Vec<Arg>),
}

/// The fast form of a whole program, with the kind of value it gives.
#[derive(Debug)]
pub(super) struct Fast {
    root: Num,
    kind: Kind,
}

impl Fast {
    /// The fast form of a program, when it is one expression the fast path covers.
    pub(super) fn of(body: &[Stmt], locals: usize) -> Option<Self> {
        let [Stmt::Expr(e)] = body else {
            return None;
        };
        if locals != 0 {
            return None;
        }
        let root = Num::of(e)?;
        let kind = root.kind();
        // A method at the top hands back its own value; elsewhere the kind must be known.
        (kind != Kind::Unknown || matches!(root, Num::Method(..))).then_some(Self { root, kind })
    }

    pub(super) fn eval(&self, host: &mut dyn Host) -> Result<Value, String> {
        if let Num::Method(root, name, args) = &self.root {
            return call(host, *root, name, args);
        }
        let n = self.root.eval(host)?;
        Ok(match self.kind {
            Kind::Bool => Value::Bool(n != 0.0),
            _ => Value::Num(n),
        })
    }
}

fn call(host: &mut dyn Host, root: Root, name: &str, args: &[Arg]) -> Result<Value, String> {
    const FEW: usize = 4;
    let target = host.root(root);
    if args.is_empty() {
        return host.method(&target, name, &[]);
    }
    let mut values: [Value; FEW] = std::array::from_fn(|_| Value::Null);
    if args.len() > FEW {
        let all = args
            .iter()
            .map(|a| a.value(host))
            .collect::<Result<Vec<_>, _>>()?;
        return host.method(&target, name, &all);
    }
    for (slot, arg) in values.iter_mut().zip(args) {
        *slot = arg.value(host)?;
    }
    host.method(&target, name, &values[..args.len()])
}

impl Arg {
    fn value(&self, host: &mut dyn Host) -> Result<Value, String> {
        Ok(match self {
            Self::Value(v) => v.clone(),
            Self::Method(Num::Method(root, name, args)) => call(host, *root, name, args)?,
            Self::Method(n) | Self::Num(n, false) => Value::Num(n.eval(host)?),
            Self::Num(n, true) => Value::Bool(n.eval(host)? != 0.0),
        })
    }
}

/// Whether a subtree calls the host, whose answer may be no number.
fn has_method(e: &Expr) -> bool {
    match e {
        Expr::Method(..) => true,
        Expr::Neg(a) | Expr::Not(a) => has_method(a),
        Expr::Binary(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => has_method(a) || has_method(b),
        Expr::Cond(c, a, b) => has_method(c) || has_method(a) || has_method(b),
        Expr::Call(_, args) | Expr::Host(_, args) => args.iter().any(has_method),
        _ => false,
    }
}

impl Num {
    fn of(e: &Expr) -> Option<Self> {
        let boxed = |e: &Expr| Self::of(e).map(Box::new);
        Some(match e {
            Expr::Const(Value::Num(n)) => Self::Const(*n),
            Expr::Const(_) | Expr::Local(_) | Expr::Root(_) | Expr::Host(..) => return None,
            Expr::Neg(a) => Self::Neg(boxed(a)?),
            Expr::Not(a) => Self::Not(boxed(a)?),
            Expr::Binary(op, a, b) => {
                // `==` and `!=` also compare strings and objects: only between plain numbers.
                if matches!(op, BinOp::Eq | BinOp::Ne) && (has_method(a) || has_method(b)) {
                    return None;
                }
                Self::Bin(*op, boxed(a)?, boxed(b)?)
            }
            Expr::And(a, b) => Self::And(boxed(a)?, boxed(b)?),
            Expr::Or(a, b) => Self::Or(boxed(a)?, boxed(b)?),
            Expr::Cond(c, a, b) => Self::Cond(boxed(c)?, boxed(a)?, boxed(b)?),
            Expr::Call(f, args) => {
                Self::Call(*f, args.iter().map(Self::of).collect::<Option<_>>()?)
            }
            Expr::Method(target, name, args) => {
                let Expr::Root(root) = **target else {
                    return None;
                };
                let args = args
                    .iter()
                    .map(|a| match a {
                        Expr::Const(v) => Some(Arg::Value(v.clone())),
                        other => {
                            let n = Self::of(other)?;
                            match n.kind() {
                                Kind::Unknown if matches!(n, Self::Method(..)) => {
                                    Some(Arg::Method(n))
                                }
                                // A ternary between method values: the interpreter's own value.
                                Kind::Unknown => None,
                                kind => Some(Arg::Num(n, kind == Kind::Bool)),
                            }
                        }
                    })
                    .collect::<Option<_>>()?;
                Self::Method(root, name.clone(), args)
            }
        })
    }

    fn kind(&self) -> Kind {
        match self {
            Self::Const(_) | Self::Neg(_) | Self::Call(..) => Kind::Num,
            Self::Bin(op, ..) => match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => Kind::Bool,
                _ => Kind::Num,
            },
            Self::Not(_) | Self::And(..) | Self::Or(..) => Kind::Bool,
            Self::Cond(_, a, b) => match (a.kind(), b.kind()) {
                (x, y) if x == y => x,
                _ => Kind::Unknown,
            },
            Self::Method(..) => Kind::Unknown,
        }
    }

    /// The value as a number, as the interpreter's `Value::num` reads it.
    fn eval(&self, host: &mut dyn Host) -> Result<f64, String> {
        Ok(match self {
            Self::Const(n) => *n,
            Self::Neg(a) => -a.eval(host)?,
            Self::Not(a) => f64::from(u8::from(!a.truthy(host)?)),
            Self::Bin(op, a, b) => {
                let (x, y) = (a.eval(host)?, b.eval(host)?);
                let t = |b: bool| f64::from(u8::from(b));
                match op {
                    BinOp::Add => x + y,
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => x / y,
                    BinOp::Rem => x % y,
                    BinOp::Pow => x.powf(y),
                    BinOp::Eq => t(x == y),
                    BinOp::Ne => t(x != y),
                    BinOp::Lt => t(x < y),
                    BinOp::Le => t(x <= y),
                    BinOp::Gt => t(x > y),
                    BinOp::Ge => t(x >= y),
                }
            }
            Self::And(a, b) => f64::from(u8::from(a.truthy(host)? && b.truthy(host)?)),
            Self::Or(a, b) => f64::from(u8::from(a.truthy(host)? || b.truthy(host)?)),
            Self::Cond(c, a, b) => {
                if c.truthy(host)? {
                    a.eval(host)?
                } else {
                    b.eval(host)?
                }
            }
            Self::Call(f, args) if args.len() <= 4 => {
                let mut nums = [0.0; 4];
                for (slot, arg) in nums.iter_mut().zip(args) {
                    *slot = arg.eval(host)?;
                }
                f(&nums[..args.len()])
            }
            Self::Call(f, args) => {
                let nums = args
                    .iter()
                    .map(|a| a.eval(host))
                    .collect::<Result<Vec<_>, _>>()?;
                f(&nums)
            }
            Self::Method(root, name, args) => call(host, *root, name, args)?.num(),
        })
    }

    /// The value's truth, as the interpreter's `Value::truthy` reads it: a method's own answer
    /// (a string or an object is true), else whether the number isn't 0.
    fn truthy(&self, host: &mut dyn Host) -> Result<bool, String> {
        match self {
            Self::Method(root, name, args) => Ok(call(host, *root, name, args)?.truthy()),
            Self::Cond(c, a, b) => {
                if c.truthy(host)? {
                    a.truthy(host)
                } else {
                    b.truthy(host)
                }
            }
            other => Ok(other.eval(host)? != 0.0),
        }
    }
}
