//! Line-level lexical helpers shared by the parser.

use super::ast::{BitSpec, Literal, Match, Value};

/// Removes a trailing `//` or `--` comment and surrounding whitespace.
pub fn strip_comment(line: &str) -> &str {
    let cut = [line.find("//"), line.find("--")].into_iter().flatten().min();
    match cut {
        Some(i) => line[..i].trim(),
        None => line.trim(),
    }
}

pub fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Parses decimal, `0x` hex or `0b` binary integers (`_` separators allowed).
pub fn parse_int(s: &str) -> Option<u64> {
    let s = s.replace('_', "");
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else if let Some(b) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        u64::from_str_radix(b, 2).ok()
    } else {
        s.parse().ok()
    }
}

/// `true`/`false`, integers (optionally negative) or decimal floats.
pub fn parse_literal(s: &str) -> Option<Literal> {
    match s {
        "true" => return Some(Literal::Bool(true)),
        "false" => return Some(Literal::Bool(false)),
        _ => {}
    }
    let (neg, body) = s.strip_prefix('-').map_or((false, s), |b| (true, b));
    if !body.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if let Some(v) = parse_int(body) {
        let v = i128::from(v);
        return Some(Literal::Int(if neg { -v } else { v }));
    }
    let f: f64 = body.parse().ok()?;
    Some(Literal::Float(if neg { -f } else { f }))
}

/// A filter condition value: exact (`v`) or range (`lo..hi`, `lo..=hi`, `lo..`, `..=hi`).
pub fn parse_match(s: &str) -> Option<Match> {
    let Some((a, b)) = s.split_once("..") else {
        return parse_value(s).map(Match::Eq);
    };
    let (inclusive, b) = match b.strip_prefix('=') {
        Some(b) => (true, b),
        None => (false, b),
    };
    let bound = |t: &str| if t.is_empty() { Some(None) } else { parse_value(t).map(Some) };
    let (lo, hi) = (bound(a)?, bound(b)?);
    if hi.is_none() && (lo.is_none() || inclusive) {
        return None;
    }
    Some(Match::Range { lo, hi, inclusive })
}

pub fn parse_value(s: &str) -> Option<Value> {
    if let Some(l) = parse_literal(s) {
        return Some(Value::Lit(l));
    }
    if let Some((a, b)) = s.split_once('.') {
        if !is_ident(a) || !is_ident(b) {
            return None;
        }
        return Some(if a == "spec" { Value::Spec(b.into()) } else { Value::Variant(a.into(), b.into()) });
    }
    is_ident(s).then(|| Value::Name(s.into()))
}

pub fn parse_bits(s: &str) -> Option<BitSpec> {
    let bit = |t: &str| parse_int(t).and_then(|v| u32::try_from(v).ok());
    if s == ".." {
        return Some(BitSpec::Full);
    }
    if let Some((a, b)) = s.split_once("..=") {
        let (a, b) = (bit(a)?, bit(b)?);
        return (a <= b).then_some(BitSpec::Range(a, b + 1));
    }
    if let Some((a, b)) = s.split_once("..") {
        let a = bit(a)?;
        if b.is_empty() {
            return Some(BitSpec::From(a));
        }
        let b = bit(b)?;
        return (a < b).then_some(BitSpec::Range(a, b));
    }
    bit(s).map(BitSpec::Bit)
}

/// Parses the `{0}` / `{2,3}` byte list of a `def`.
pub fn parse_byte_list(s: &str) -> Option<Vec<usize>> {
    let inner = s.strip_prefix('{')?.strip_suffix('}')?;
    inner
        .split(',')
        .map(|t| parse_int(t.trim()).and_then(|v| usize::try_from(v).ok()))
        .collect()
}

/// `snake_case` from `PascalCase`/`camelCase`, keeping acronyms together (`CANStatus` -> `can_status`).
pub fn to_snake(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            if prev.is_ascii_lowercase() || prev.is_ascii_digit() || (prev.is_ascii_uppercase() && next_lower) {
                out.push('_');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

pub fn to_pascal(s: &str) -> String {
    s.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect()
}

pub fn to_upper_snake(s: &str) -> String {
    to_snake(s).to_ascii_uppercase()
}

/// Identifiers that would break the generated Rust or C code.
pub fn is_reserved(s: &str) -> bool {
    const RESERVED: &[&str] = &[
        // Rust
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
        "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
        "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
        "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
        "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
        // C
        "auto", "case", "char", "default", "double", "float", "goto", "inline", "int", "long",
        "register", "restrict", "short", "signed", "sizeof", "switch", "typedef", "union", "unsigned",
        "void", "volatile", "bool",
        // generated helpers
        "buf", "len", "m", "v", "rc", "spec",
    ];
    RESERVED.contains(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits() {
        assert_eq!(parse_bits(".."), Some(BitSpec::Full));
        assert_eq!(parse_bits("3"), Some(BitSpec::Bit(3)));
        assert_eq!(parse_bits("4.."), Some(BitSpec::From(4)));
        assert_eq!(parse_bits("0..4"), Some(BitSpec::Range(0, 4)));
        assert_eq!(parse_bits("0..=3"), Some(BitSpec::Range(0, 4)));
        assert_eq!(parse_bits("4..4"), None);
    }

    #[test]
    fn values() {
        assert_eq!(parse_value("-3"), Some(Value::Lit(Literal::Int(-3))));
        assert_eq!(parse_value("0x10"), Some(Value::Lit(Literal::Int(16))));
        assert_eq!(parse_value("1.5"), Some(Value::Lit(Literal::Float(1.5))));
        assert_eq!(parse_value("spec.node_id"), Some(Value::Spec("node_id".into())));
        assert_eq!(parse_value("NodeId.Coproc"), Some(Value::Variant("NodeId".into(), "Coproc".into())));
        assert_eq!(parse_value("node"), Some(Value::Name("node".into())));
        assert_eq!(parse_value("a.b.c"), None);
    }

    #[test]
    fn matches() {
        let n = |v: i128| Some(Value::Lit(Literal::Int(v)));
        assert_eq!(parse_match("2..=5"), Some(Match::Range { lo: n(2), hi: n(5), inclusive: true }));
        assert_eq!(parse_match("4.."), Some(Match::Range { lo: n(4), hi: None, inclusive: false }));
        assert_eq!(parse_match("..=3"), Some(Match::Range { lo: None, hi: n(3), inclusive: true }));
        assert_eq!(parse_match("lo..hi").map(|m| matches!(m, Match::Range { .. })), Some(true));
        assert_eq!(parse_match("NodeId.MC"), Some(Match::Eq(Value::Variant("NodeId".into(), "MC".into()))));
        assert_eq!(parse_match(".."), None);
        assert_eq!(parse_match("2..="), None);
    }

    #[test]
    fn case() {
        assert_eq!(to_snake("ConnectionStatus"), "connection_status");
        assert_eq!(to_snake("CANStatus"), "can_status");
        assert_eq!(to_pascal("status_id"), "StatusId");
        assert_eq!(to_upper_snake("CoprocStatus"), "COPROC_STATUS");
    }
}
