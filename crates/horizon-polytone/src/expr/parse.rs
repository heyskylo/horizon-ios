//! Parses tokens into a tree whose names are already resolved: locals to slots, functions to
//! the math table or the host, roots (`o`, `player`...) to [`Root`]s. An unknown name is an
//! error here, when the pack loads, not when the expression runs.
use super::lex::{Token, tokens};
use super::{MATH, MathFn, Root, Value, legacy};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug)]
pub(super) enum Expr {
    Const(Value),
    Local(usize),
    Root(Root),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// A math or color function.
    Call(MathFn, Vec<Expr>),
    /// A global function the game answers (`config`, `colormap`...).
    Host(Arc<str>, Vec<Expr>),
    /// `target.name(args)`; `p.age`, `p.age()` and `p.getAge()` all name `age`.
    Method(Box<Expr>, Arc<str>, Vec<Expr>),
}

#[derive(Debug)]
pub(super) enum Stmt {
    Expr(Expr),
    /// `slot = value`, or `slot op= value`.
    Assign(usize, Option<BinOp>, Expr),
    If(Vec<(Expr, Vec<Stmt>)>, Vec<Stmt>),
}

/// Global functions the game provides rather than this crate.
const HOST_FUNCTIONS: [&str; 11] = [
    "colormap",
    "config",
    "modOn",
    "modLoader",
    "dateYear",
    "dateMonth",
    "dateDay",
    "dateHour",
    "dateMinute",
    "dateTime",
    "dateDayOfYear",
];

struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    locals: Vec<String>,
    /// A legacy math expression: `lerp`, `smoothstep` and `color` take their old argument order.
    legacy: bool,
}

/// The statements and how many local slots they use.
pub(super) fn parse(src: &str) -> Result<(Vec<Stmt>, usize), String> {
    let tokens = tokens(src)?;
    let mut p = Parser {
        legacy: legacy::is_legacy(&tokens),
        tokens,
        pos: 0,
        locals: Vec::new(),
    };
    let body = p.statements(false)?;
    Ok((body, p.locals.len()))
}

fn binary(op: &str) -> Option<(BinOp, u8)> {
    Some(match op {
        "==" => (BinOp::Eq, 4),
        "!=" => (BinOp::Ne, 4),
        "<" => (BinOp::Lt, 5),
        "<=" => (BinOp::Le, 5),
        ">" => (BinOp::Gt, 5),
        ">=" => (BinOp::Ge, 5),
        "+" => (BinOp::Add, 6),
        "-" => (BinOp::Sub, 6),
        "*" => (BinOp::Mul, 7),
        "/" => (BinOp::Div, 7),
        "%" => (BinOp::Rem, 7),
        _ => return None,
    })
}

/// `getAge` and `age` are the same method.
fn method_name(name: &str) -> String {
    match name.strip_prefix("get") {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_uppercase()) => {
            let mut chars = rest.chars();
            let first = chars.next().unwrap_or_default().to_ascii_lowercase();
            std::iter::once(first).chain(chars).collect()
        }
        _ => name.to_owned(),
    }
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.0)
    }
    fn at(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Token::Punct(q)) if *q == p)
    }
    fn eat(&mut self, p: &str) -> bool {
        let found = self.at(p);
        self.pos += usize::from(found);
        found
    }
    fn error(&self, what: &str) -> String {
        match self.tokens.get(self.pos) {
            Some((t, at)) => format!("{what}, found {t:?} at {at}"),
            None => format!("{what} at the end"),
        }
    }
    fn expect(&mut self, p: &str) -> Result<(), String> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {p:?}")))
        }
    }

    fn statements(&mut self, in_block: bool) -> Result<Vec<Stmt>, String> {
        let mut out = Vec::new();
        loop {
            while self.eat(";") {}
            match self.peek() {
                None if !in_block => return Ok(out),
                Some(Token::Punct("}")) if in_block => return Ok(out),
                None => return Err(self.error("expected \"}\"")),
                _ => out.push(self.statement()?),
            }
        }
    }

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect("{")?;
        let body = self.statements(true)?;
        self.expect("}")?;
        Ok(body)
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        if matches!(self.peek(), Some(Token::Ident(s)) if s == "if") {
            let mut branches = Vec::new();
            let mut otherwise = Vec::new();
            loop {
                self.pos += 1;
                self.expect("(")?;
                let condition = self.expr(0)?;
                self.expect(")")?;
                branches.push((condition, self.block()?));
                if !matches!(self.peek(), Some(Token::Ident(s)) if s == "else") {
                    break;
                }
                self.pos += 1;
                if !matches!(self.peek(), Some(Token::Ident(s)) if s == "if") {
                    otherwise = self.block()?;
                    break;
                }
            }
            return Ok(Stmt::If(branches, otherwise));
        }
        if let (Some(Token::Ident(name)), Some((Token::Punct(op), _))) =
            (self.peek(), self.tokens.get(self.pos + 1))
        {
            let op = match *op {
                "=" => Some(None),
                "+=" => Some(Some(BinOp::Add)),
                "-=" => Some(Some(BinOp::Sub)),
                "*=" => Some(Some(BinOp::Mul)),
                "/=" => Some(Some(BinOp::Div)),
                _ => None,
            };
            if let Some(op) = op {
                let name = name.clone();
                self.pos += 2;
                let value = self.expr(0)?;
                let slot = match self.locals.iter().position(|l| *l == name) {
                    Some(slot) => slot,
                    None if op.is_none() => {
                        self.locals.push(name);
                        self.locals.len() - 1
                    }
                    None => return Err(format!("{name} is updated before it is set")),
                };
                return Ok(Stmt::Assign(slot, op, value));
            }
        }
        Ok(Stmt::Expr(self.expr(0)?))
    }

    /// Precedence climbing: ternary 1, `||` 2, `&&` 3, equality 4, comparison 5, `+ -` 6,
    /// `* / %` 7, unary 8, `^` 9 (right to left, so `-2^2` is -4).
    fn expr(&mut self, min: u8) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            let Some(Token::Punct(op)) = self.peek().cloned() else {
                return Ok(left);
            };
            if op == "?" && min <= 1 {
                self.pos += 1;
                let then = self.expr(1)?;
                self.expect(":")?;
                let otherwise = self.expr(1)?;
                left = Expr::Cond(Box::new(left), Box::new(then), Box::new(otherwise));
            } else if op == "||" && min <= 2 {
                self.pos += 1;
                left = Expr::Or(Box::new(left), Box::new(self.expr(3)?));
            } else if op == "&&" && min <= 3 {
                self.pos += 1;
                left = Expr::And(Box::new(left), Box::new(self.expr(4)?));
            } else if let Some((bin, prec)) = binary(op).filter(|b| b.1 >= min) {
                self.pos += 1;
                left = Expr::Binary(bin, Box::new(left), Box::new(self.expr(prec + 1)?));
            } else {
                return Ok(left);
            }
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        self.eat("+");
        let base = self.postfix()?;
        if self.eat("^") {
            let exponent = self.unary()?;
            return Ok(Expr::Binary(BinOp::Pow, Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if self.eat(")") {
            return Ok(args);
        }
        loop {
            args.push(self.expr(0)?);
            if self.eat(")") {
                return Ok(args);
            }
            self.expect(",")?;
        }
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        while self.eat(".") {
            let Some(Token::Ident(name)) = self.peek().cloned() else {
                return Err(self.error("expected a method name"));
            };
            self.pos += 1;
            let args = if self.eat("(") {
                self.arguments()?
            } else {
                Vec::new()
            };
            e = Expr::Method(Box::new(e), method_name(&name).into(), args);
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let token = self
            .peek()
            .cloned()
            .ok_or_else(|| self.error("expected a value"))?;
        self.pos += 1;
        Ok(match token {
            Token::Num(n) => Expr::Const(Value::Num(n)),
            Token::Str(s) => Expr::Const(Value::Str(s.into())),
            Token::Punct("(") => {
                let e = self.expr(0)?;
                self.expect(")")?;
                e
            }
            Token::Ident(name) if self.eat("(") => {
                let args = self.arguments()?;
                let legacy = legacy::FUNCTIONS.iter().filter(|_| self.legacy);
                if let Some(&(_, fewest, most, f)) =
                    legacy.chain(MATH).find(|entry| entry.0 == name)
                {
                    if !(fewest..=most).contains(&args.len()) {
                        let takes = if fewest == most {
                            fewest.to_string()
                        } else {
                            format!("{fewest} to {most}")
                        };
                        return Err(format!(
                            "{name} takes {takes} arguments, not {}",
                            args.len()
                        ));
                    }
                    Expr::Call(f, args)
                } else if HOST_FUNCTIONS.contains(&name.as_str()) {
                    Expr::Host(name.into(), args)
                } else if let Some(draw) = legacy::random(&name, args) {
                    draw
                } else {
                    return Err(format!("unknown function {name}"));
                }
            }
            Token::Ident(name) => match name.as_str() {
                "true" => Expr::Const(Value::Bool(true)),
                "false" => Expr::Const(Value::Bool(false)),
                "PI" => Expr::Const(Value::Num(std::f64::consts::PI)),
                "TAU" => Expr::Const(Value::Num(std::f64::consts::TAU)),
                "E" => Expr::Const(Value::Num(std::f64::consts::E)),
                "PSI" => Expr::Const(Value::Num(std::f64::consts::GOLDEN_RATIO)),
                _ => match (
                    Root::parse(&name),
                    self.locals.iter().position(|l| *l == name),
                ) {
                    (_, Some(slot)) => Expr::Local(slot),
                    (Some(root), None) => Expr::Root(root),
                    (None, None) => match legacy::variable(&name) {
                        Some(e) => e?,
                        None => return Err(format!("unknown name {name}")),
                    },
                },
            },
            Token::Punct(_) => {
                self.pos -= 1;
                return Err(self.error("expected a value"));
            }
        })
    }
}
