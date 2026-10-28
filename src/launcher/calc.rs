// Copyright (c) Danil Klimov (lyoraeth)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launcher line calculator: arithmetic, quoted strings, comparisons. No boolean logic.
//!
//! ```text
//! 2 + 2 * 3     → 8          "ха" * 3   → хахаха
//! (1 + 2) ^ 2   → 9          "a" + "b"  → ab
//! sqrt(16) √16  → 4          sin(30)    → 0.5 (degrees)
//! 2 * pi        → 6.28…      ln(e)      → 1
//! 10 / 4        → 2.5        0.1 + 0.2 == 0.3 → true
//! ```

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f64),
    Str(String),
    Bool(bool),
}

/// Strings are never built longer than this: `"a" * 1e9` must not eat the memory.
const MAX_STR: usize = 10_000;

impl Value {
    /// Result text, both for display and for the clipboard.
    pub fn text(&self) -> String {
        match self {
            Value::Num(n) => fmt_num(*n),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
        }
    }
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    if n.abs() >= 1e15 || n.abs() < 1e-9 {
        return format!("{n:e}");
    }
    // 10 decimal places strip tails like 0.30000000000000004.
    let s = format!("{n:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Op(&'static str),
    Open,
    Close,
    /// Function or constant name.
    Name(String),
}

/// Trigonometry in degrees, as a person expects: sin(30) = 0.5.
fn function(name: &str, x: f64) -> Option<f64> {
    let r = x.to_radians();
    let v = match name {
        "sqrt" | "√" => x.sqrt(),
        "sin" => r.sin(),
        "cos" => r.cos(),
        "tan" | "tg" => r.tan(),
        "asin" => x.asin().to_degrees(),
        "acos" => x.acos().to_degrees(),
        "atan" => x.atan().to_degrees(),
        "ln" => x.ln(),
        "log" | "lg" => x.log10(),
        "abs" => x.abs(),
        "round" => x.round(),
        "floor" => x.floor(),
        "ceil" => x.ceil(),
        "exp" => x.exp(),
        _ => return None,
    };
    // Degrees are inexact in binary fractions: sin(180) would give 1.2e-16 instead of zero.
    Some(if v.abs() < 1e-12 { 0.0 } else { v })
}

fn constant(name: &str) -> Option<f64> {
    match name {
        "pi" | "π" => Some(std::f64::consts::PI),
        "e" => Some(std::f64::consts::E),
        _ => None,
    }
}

fn tokens(src: &str) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '0'..='9' | '.' | ',' if c.is_ascii_digit() || chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()) => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',') {
                    i += 1;
                }
                // A comma is a decimal separator, as written in Russian.
                let s: String = chars[start..i].iter().collect::<String>().replace(',', ".");
                out.push(Tok::Num(s.parse().ok()?));
            }
            '"' | '\'' | '«' => {
                let close = match c {
                    '«' => '»',
                    q => q,
                };
                let end = chars[i + 1..].iter().position(|&d| d == close)? + i + 1;
                out.push(Tok::Str(chars[i + 1..end].iter().collect()));
                i = end + 1;
            }
            '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            c if c.is_alphabetic() || c == '√' || c == 'π' => {
                let start = i;
                i += 1;
                // √ and π are names on their own; letters are collected into a word.
                if c.is_ascii_alphabetic() {
                    while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                        i += 1;
                    }
                }
                let name: String = chars[start..i].iter().collect::<String>().to_lowercase();
                if function(&name, 0.0).is_none() && constant(&name).is_none() {
                    return None;
                }
                out.push(Tok::Name(name));
            }
            ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            _ => {
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                let op = match two.as_str() {
                    "<=" | "≤" => Some("<="),
                    ">=" | "≥" => Some(">="),
                    "==" => Some("=="),
                    "!=" | "≠" => Some("!="),
                    _ => None,
                };
                if let Some(op) = op {
                    out.push(Tok::Op(op));
                    i += 2;
                    continue;
                }
                let op = match c {
                    '+' => "+",
                    '-' | '−' => "-",
                    '*' | '×' => "*",
                    '/' | '÷' => "/",
                    '%' => "%",
                    '^' => "^",
                    '<' => "<",
                    '>' => ">",
                    '=' => "==",
                    '≤' => "<=",
                    '≥' => ">=",
                    '≠' => "!=",
                    _ => return None,
                };
                out.push(Tok::Op(op));
                i += 1;
            }
        }
    }
    Some(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek_op(&self) -> Option<&'static str> {
        match self.toks.get(self.pos) {
            Some(Tok::Op(o)) => Some(o),
            _ => None,
        }
    }

    fn compare(&mut self) -> Option<Value> {
        let a = self.sum()?;
        let Some(op) = self.peek_op().filter(|o| ["<", ">", "<=", ">=", "==", "!="].contains(o)) else { return Some(a) };
        self.pos += 1;
        let b = self.sum()?;
        let ord = match (&a, &b) {
            // With tolerance: 0.1 + 0.2 displays as 0.3 and must equal it.
            (Value::Num(x), Value::Num(y)) if (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0) => std::cmp::Ordering::Equal,
            (Value::Num(x), Value::Num(y)) => x.partial_cmp(y)?,
            (Value::Str(x), Value::Str(y)) => x.cmp(y),
            (Value::Bool(x), Value::Bool(y)) if matches!(op, "==" | "!=") => x.cmp(y),
            _ => return None,
        };
        use std::cmp::Ordering::*;
        Some(Value::Bool(match op {
            "<" => ord == Less,
            ">" => ord == Greater,
            "<=" => ord != Greater,
            ">=" => ord != Less,
            "==" => ord == Equal,
            _ => ord != Equal,
        }))
    }

    fn sum(&mut self) -> Option<Value> {
        let mut a = self.term()?;
        while let Some(op) = self.peek_op().filter(|o| matches!(*o, "+" | "-")) {
            self.pos += 1;
            let b = self.term()?;
            a = match (a, b, op) {
                (Value::Num(x), Value::Num(y), "+") => Value::Num(x + y),
                (Value::Num(x), Value::Num(y), _) => Value::Num(x - y),
                // A string plus anything concatenates as text.
                (Value::Str(x), y, "+") => Value::Str(x + &y.text()),
                (x, Value::Str(y), "+") => Value::Str(x.text() + &y),
                _ => return None,
            };
            if matches!(&a, Value::Str(s) if s.chars().count() > MAX_STR) {
                return None;
            }
        }
        Some(a)
    }

    fn term(&mut self) -> Option<Value> {
        let mut a = self.unary()?;
        while let Some(op) = self.peek_op().filter(|o| matches!(*o, "*" | "/" | "%")) {
            self.pos += 1;
            let b = self.unary()?;
            a = match (a, b, op) {
                (Value::Num(x), Value::Num(y), "*") => Value::Num(x * y),
                (Value::Num(_), Value::Num(0.0), "/" | "%") => return None,
                (Value::Num(x), Value::Num(y), "/") => Value::Num(x / y),
                (Value::Num(x), Value::Num(y), _) => Value::Num(x % y),
                (Value::Str(s), Value::Num(n), "*") | (Value::Num(n), Value::Str(s), "*") => repeat(&s, n)?,
                _ => return None,
            };
        }
        Some(a)
    }

    fn unary(&mut self) -> Option<Value> {
        if self.peek_op() == Some("-") {
            self.pos += 1;
            return match self.unary()? {
                Value::Num(n) => Some(Value::Num(-n)),
                _ => None,
            };
        }
        self.power()
    }

    fn power(&mut self) -> Option<Value> {
        let base = self.atom()?;
        if self.peek_op() != Some("^") {
            return Some(base);
        }
        self.pos += 1;
        // Power is right-associative: 2^3^2 = 2^9.
        match (base, self.unary()?) {
            (Value::Num(x), Value::Num(y)) => Some(Value::Num(x.powf(y))).filter(|v| matches!(v, Value::Num(n) if n.is_finite())),
            _ => None,
        }
    }

    fn atom(&mut self) -> Option<Value> {
        let t = self.toks.get(self.pos)?.clone();
        self.pos += 1;
        match t {
            Tok::Num(n) => Some(Value::Num(n)),
            Tok::Str(s) => Some(Value::Str(s)),
            Tok::Name(name) => {
                if let Some(c) = constant(&name) {
                    return Some(Value::Num(c));
                }
                // The argument is in parentheses or right after: sqrt(16), √16, sin 30.
                match self.unary()? {
                    Value::Num(x) => function(&name, x).filter(|v| v.is_finite()).map(Value::Num),
                    _ => None,
                }
            }
            Tok::Open => {
                let v = self.compare()?;
                (self.toks.get(self.pos) == Some(&Tok::Close)).then(|| {
                    self.pos += 1;
                    v
                })
            }
            _ => None,
        }
    }
}

fn repeat(s: &str, n: f64) -> Option<Value> {
    if n < 0.0 || n.fract() != 0.0 || s.chars().count() as f64 * n > MAX_STR as f64 {
        return None;
    }
    Some(Value::Str(s.repeat(n as usize)))
}

/// Expression value. `None`: not an expression, i.e. the line has no operation (a bare number
/// or word is not a calculation) or it does not parse.
pub fn eval(src: &str) -> Option<Value> {
    let toks = tokens(src.trim())?;
    // An expression needs an operation or a function; a bare number or pi is not one.
    let func = toks.iter().any(|t| matches!(t, Tok::Name(n) if constant(n).is_none()));
    if !func && !toks.iter().any(|t| matches!(t, Tok::Op(_))) {
        return None;
    }
    let mut p = Parser { toks, pos: 0 };
    let v = p.compare()?;
    (p.pos == p.toks.len()).then_some(v).filter(|v| !matches!(v, Value::Num(n) if !n.is_finite()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Option<String> {
        eval(s).map(|v| v.text())
    }

    #[test]
    fn arithmetic() {
        assert_eq!(t("2 + 2 * 3").as_deref(), Some("8"));
        assert_eq!(t("(1 + 2) ^ 2").as_deref(), Some("9"));
        assert_eq!(t("2^3^2").as_deref(), Some("512"));
        assert_eq!(t("10 / 4").as_deref(), Some("2.5"));
        assert_eq!(t("1,5 * 2").as_deref(), Some("3"));
        assert_eq!(t("-3 + 1").as_deref(), Some("-2"));
        assert_eq!(t("0.1 + 0.2").as_deref(), Some("0.3"));
        assert_eq!(t("7 % 3").as_deref(), Some("1"));
    }

    #[test]
    fn strings() {
        assert_eq!(t(r#""ha" * 3"#).as_deref(), Some("hahaha"));
        assert_eq!(t("«a» + 'b'").as_deref(), Some("ab"));
        assert_eq!(t(r#""a" * 100000"#), None);
    }

    #[test]
    fn comparisons() {
        assert_eq!(t("0.1 + 0.2 == 0.3").as_deref(), Some("true"));
        assert_eq!(t("2 * 2 >= 4").as_deref(), Some("true"));
        assert_eq!(t(r#""a" < "b""#).as_deref(), Some("true"));
    }

    #[test]
    fn functions() {
        assert_eq!(t("sqrt(16)").as_deref(), Some("4"));
        assert_eq!(t("√16 + 1").as_deref(), Some("5"));
        assert_eq!(t("sin(30)").as_deref(), Some("0.5"));
        assert_eq!(t("cos 180").as_deref(), Some("-1"));
        assert_eq!(t("sin(180)").as_deref(), Some("0"));
        assert_eq!(t("ln(e)").as_deref(), Some("1"));
        assert_eq!(t("2 * pi").as_deref(), Some("6.2831853072"));
        assert_eq!(t("sqrt(-1)"), None);
        assert_eq!(t("pi"), None);
    }

    #[test]
    fn not_expressions() {
        assert_eq!(t("42"), None);
        assert_eq!(t("telegram"), None);
        assert_eq!(t("1 / 0"), None);
        assert_eq!(t("2 +"), None);
        assert_eq!(t("c++"), None);
    }
}
