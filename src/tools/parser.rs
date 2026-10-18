//! Line-oriented parser for `.cp` / `.inc.cp` files.
//!
//! ```text
//! #protocol=can                  -- optional directives, before any block
//! #inc=common                    -- include common.inc.cp from the same directory
//! #endian=big                    -- default byte order of this file (little if omitted)
//! #id=standard|extended          -- id type for `.filter`/`.rsp` that leave `.ext` open
//! type Name:                     -- enum
//! .variant <int>
//! def {b0[,b1..]} Name:          -- bit-field group pinned to payload bytes
//! def 'N Name:                   -- generic group of N bytes, placed where it is used
//! [#b1]                          -- switch to byte b1 (multi-byte defs)
//! .field <bits> <Type> [big|little] -- Type required: enum, bool, u8..u64, i8..i64
//! spec:                          -- constants, standard types only
//! .name <literal> <type>         -- type required
//! filter Name[(p: Type, ..)]:    -- raw bit matches, independent of other filters
//! .id <bits> <value>             -- value: exact, or a range `lo..hi` `lo..=hi` `lo..` `..=hi`
//! .data {b} <bits> <value> [big|little]
//! .ext [value]                   -- id type: true = extended (29-bit), false = standard
//! frame Name:
//! .size 'N | .fd [bool] | .filter A | B(args) | .[field:] Def [{n}]
//! fn Name(p: Type, ..): Ret|Void
//! .rsp A | B(args) | .fd [bool]
//! ```

use crate::{
    error::ErrorSignals,
    protocols::common::{type_protocol, Protocols},
};

use super::{
    ast::{
        BitSpec, Cond, Def, Field, FilterCall, FilterDef, FilterExpr, FnDef, Frame, Item, Literal, Match, Member, Param,
        Proto, Spec, SpecEntry, Target, TypeDef, Value, Variant,
    },
    keywords::{FrameKeyWords, KeyWords},
    syntax::{is_ident, parse_bits, parse_byte_list, parse_int, parse_literal, parse_match, parse_value, strip_comment},
};

type Result<T> = std::result::Result<T, ErrorSignals>;

const INC_DIRECTIVE: &str = "#inc=";
const MAX_PAYLOAD: usize = 64;
const ENDIAN_DIRECTIVE: &str = "#endian=";
const ID_DIRECTIVE: &str = "#id=";
const BIG: &str = "big";
const LITTLE: &str = "little";

fn endian(tok: &str) -> Option<bool> {
    match tok {
        BIG => Some(true),
        LITTLE => Some(false),
        _ => None,
    }
}

pub fn parse(src: &str) -> Result<Proto> {
    let mut proto = Proto::default();
    let mut cur: Option<Item> = None;
    let mut section: Option<usize> = None;

    for (idx, raw) in src.lines().enumerate() {
        let ln = idx + 1;
        let line = strip_comment(raw);
        if line.is_empty() {
            continue;
        }

        if line.starts_with(<&str>::from(KeyWords::Hash)) {
            if cur.is_some() {
                return Err(ErrorSignals::at(ln, "`#` directives must come before any definition"));
            }
            if let Some(name) = line.strip_prefix(INC_DIRECTIVE) {
                let name = name.trim();
                if !is_ident(name) {
                    return Err(ErrorSignals::at(ln, format!("invalid include `{name}`, expected `#inc=<name>`")));
                }
                proto.includes.push((name.to_string(), ln));
                continue;
            }
            if let Some(order) = line.strip_prefix(ENDIAN_DIRECTIVE) {
                proto.big_endian = endian(order.trim())
                    .ok_or_else(|| ErrorSignals::at(ln, format!("expected `#endian={BIG}` or `#endian={LITTLE}`")))?;
                continue;
            }
            if let Some(kind) = line.strip_prefix(ID_DIRECTIVE) {
                proto.extended_id = Some(match kind.trim() {
                    "standard" => false,
                    "extended" => true,
                    _ => return Err(ErrorSignals::at(ln, "expected `#id=standard` or `#id=extended`")),
                });
                continue;
            }
            match type_protocol(line) {
                Ok(Protocols::Can) => continue,
                Ok(Protocols::Nil) => return Err(ErrorSignals::at(ln, "unsupported protocol (only `can`)")),
                Err(ErrorSignals::NoError) => return Err(ErrorSignals::NoError),
                Err(e) => return Err(ErrorSignals::at(ln, e.to_string())),
            }
        }

        if line.starts_with('[') {
            let Some(Item::Def(def)) = cur.as_ref() else {
                return Err(ErrorSignals::at(ln, "byte section `[#n]` outside of a `def`"));
            };
            let byte = line
                .strip_prefix("[#")
                .and_then(|s| s.strip_suffix(']'))
                .and_then(|s| parse_int(s.trim()))
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| ErrorSignals::at(ln, format!("malformed byte section `{line}`, expected `[#n]`")))?;
            if !def.bytes.contains(&byte) {
                return Err(ErrorSignals::at(
                    ln,
                    format!("byte {byte} is not declared in `def {:?} {}`", def.bytes, def.name),
                ));
            }
            section = Some(byte);
            continue;
        }

        if let Some(member) = line.strip_prefix(<&str>::from(KeyWords::Dot)) {
            let Some(item) = cur.as_mut() else {
                return Err(ErrorSignals::at(ln, "member line outside of a block"));
            };
            parse_member(item, section, member, &proto, ln)?;
            continue;
        }

        let (kw, _) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let item = if matches!(KeyWords::from(kw), KeyWords::Fn) {
            parse_fn_header(line[kw.len()..].trim(), proto.big_endian, ln)?
        } else {
            let header = line
                .strip_suffix(':')
                .ok_or_else(|| ErrorSignals::at(ln, format!("expected a block header ending in `:`, found `{line}`")))?
                .trim();
            let (kw, rest) = header.split_once(char::is_whitespace).unwrap_or((header, ""));
            let rest = rest.trim();
            match KeyWords::from(kw) {
                KeyWords::Type => Item::Type(TypeDef { name: ident(rest, ln)?, line: ln, variants: Vec::new() }),
                KeyWords::Frame => Item::Frame(Frame {
                    name: ident(rest, ln)?,
                    line: ln,
                    size: None,
                    fd: false,
                    filter: None,
                    members: Vec::new(),
                }),
                KeyWords::Spec => {
                    if !rest.is_empty() {
                        return Err(ErrorSignals::at(ln, "expected `spec:`"));
                    }
                    Item::Spec(Spec { line: ln, entries: Vec::new() })
                }
                KeyWords::Filter => {
                    let (name, params) = parse_signature(rest, ln)?;
                    Item::Filter(FilterDef { name, line: ln, params, conds: Vec::new() })
                }
                KeyWords::Def if rest.starts_with(<&str>::from(FrameKeyWords::SizeMark)) => {
                    let (size, name) = rest
                        .split_once(char::is_whitespace)
                        .ok_or_else(|| ErrorSignals::at(ln, "expected `def 'N Name:`"))?;
                    let size = size_mark(size)
                        .and_then(|v| usize::try_from(v).ok())
                        .filter(|n| (1..=MAX_PAYLOAD).contains(n))
                        .ok_or_else(|| ErrorSignals::at(ln, format!("generic def size must be 1..={MAX_PAYLOAD}, found `{size}`")))?;
                    Item::Def(Def {
                        name: ident(name.trim(), ln)?,
                        line: ln,
                        bytes: (0..size).collect(),
                        fields: Vec::new(),
                        generic: true,
                    })
                }
                KeyWords::Def => {
                    let close = rest
                        .find('}')
                        .ok_or_else(|| ErrorSignals::at(ln, "expected `def {bytes} Name:` or `def 'N Name:`"))?;
                    let bytes = parse_byte_list(&rest[..=close])
                        .filter(|b| !b.is_empty())
                        .ok_or_else(|| ErrorSignals::at(ln, format!("malformed byte list `{}`", &rest[..=close])))?;
                    Item::Def(Def {
                        name: ident(rest[close + 1..].trim(), ln)?,
                        line: ln,
                        bytes,
                        fields: Vec::new(),
                        generic: false,
                    })
                }
                _ => return Err(ErrorSignals::at(ln, format!("unknown keyword `{kw}`"))),
            }
        };

        proto.items.extend(cur.replace(item));
        section = None;
    }

    proto.items.extend(cur);
    Ok(proto)
}

fn ident(s: &str, ln: usize) -> Result<String> {
    if is_ident(s) {
        Ok(s.to_string())
    } else {
        Err(ErrorSignals::at(ln, format!("invalid identifier `{s}`")))
    }
}

/// Optional boolean argument; a bare flag means `true`.
fn flag(args: &[&str], ln: usize) -> Result<bool> {
    match args {
        [] | ["true"] => Ok(true),
        ["false"] => Ok(false),
        _ => Err(ErrorSignals::at(ln, "expected no argument, `true` or `false`")),
    }
}

fn size_mark(tok: &str) -> Option<u64> {
    parse_int(tok.strip_prefix(<&str>::from(FrameKeyWords::SizeMark)).unwrap_or(tok))
}

/// `Name` or `Name(p: Type, ..)`.
fn parse_signature(s: &str, ln: usize) -> Result<(String, Vec<Param>)> {
    let Some(open) = s.find('(') else {
        return Ok((ident(s, ln)?, Vec::new()));
    };
    let inner = s[open + 1..]
        .strip_suffix(')')
        .ok_or_else(|| ErrorSignals::at(ln, "expected `)` at the end of the parameter list"))?;
    Ok((ident(s[..open].trim(), ln)?, parse_params(inner, ln)?))
}

fn parse_params(s: &str, ln: usize) -> Result<Vec<Param>> {
    if s.trim().is_empty() {
        return Ok(Vec::new());
    }
    s.split(',')
        .map(|p| {
            let (name, ty) = p
                .split_once(':')
                .ok_or_else(|| ErrorSignals::at(ln, format!("expected `name: Type`, found `{}`", p.trim())))?;
            Ok(Param { name: ident(name.trim(), ln)?, ty: ident(ty.trim(), ln)? })
        })
        .collect()
}

/// `Name(params): Ret`
fn parse_fn_header(s: &str, big: bool, ln: usize) -> Result<Item> {
    let close = s.find(')').ok_or_else(|| ErrorSignals::at(ln, "expected `fn Name(params): Ret`"))?;
    let (name, params) = parse_signature(&s[..=close], ln)?;
    let ret = s[close + 1..]
        .trim()
        .strip_prefix(':')
        .ok_or_else(|| ErrorSignals::at(ln, "expected `: Ret` after the parameter list"))?
        .trim();
    let ret = ident(ret, ln)?;
    Ok(Item::Fn(FnDef {
        name,
        line: ln,
        params,
        ret: (ret != "Void").then_some(ret),
        rsp: None,
        fd: false,
        big,
    }))
}

/// `A | B(x, y) | C`
fn parse_filter_expr(s: &str, ext_default: Option<bool>, ln: usize) -> Result<FilterExpr> {
    let calls = s
        .split('|')
        .map(|call| {
            let call = call.trim();
            let (name, args) = match call.find('(') {
                None => (call, ""),
                Some(open) => (
                    &call[..open],
                    call[open + 1..]
                        .strip_suffix(')')
                        .ok_or_else(|| ErrorSignals::at(ln, format!("missing `)` in `{call}`")))?,
                ),
            };
            let args = if args.trim().is_empty() {
                Vec::new()
            } else {
                args.split(',')
                    .map(|a| parse_value(a.trim()).ok_or_else(|| ErrorSignals::at(ln, format!("invalid value `{}`", a.trim()))))
                    .collect::<Result<_>>()?
            };
            Ok(FilterCall { name: ident(name.trim(), ln)?, args })
        })
        .collect::<Result<_>>()?;
    Ok(FilterExpr { line: ln, calls, ext_default })
}

/// `.[field:] Def [{n}]`; `None` if the line is a frame attribute (`.size`, `.fd`, `.filter`, `.id`).
fn parse_frame_member(member: &str, ln: usize) -> Result<Option<Member>> {
    let (field, rest) = match member.split_once(':') {
        Some((field, rest)) => (Some(ident(field.trim(), ln)?), rest.trim()),
        None => {
            let first = member.split_whitespace().next().unwrap_or("");
            if matches!(
                FrameKeyWords::from(first),
                FrameKeyWords::Size | FrameKeyWords::Fd | FrameKeyWords::Filter | FrameKeyWords::Id
            ) {
                return Ok(None);
            }
            (None, member.trim())
        }
    };
    let (def, at) = match rest.split_whitespace().collect::<Vec<_>>()[..] {
        [def] => (def, None),
        [def, at] => match parse_byte_list(at).as_deref() {
            Some([b]) => (def, Some(*b)),
            _ => return Err(ErrorSignals::at(ln, format!("expected a start byte `{{n}}`, found `{at}`"))),
        },
        _ => return Err(ErrorSignals::at(ln, "expected `.[field:] Def [{n}]`")),
    };
    Ok(Some(Member { def: ident(def, ln)?, field, at, line: ln }))
}

/// File-level defaults applied while parsing members.
fn parse_member(item: &mut Item, section: Option<usize>, member: &str, file: &Proto, ln: usize) -> Result<()> {
    if let Item::Frame(f) = item {
        if let Some(m) = parse_frame_member(member, ln)? {
            f.members.push(m);
            return Ok(());
        }
    }
    let big_default = file.big_endian;
    let toks: Vec<&str> = member.split_whitespace().collect();
    let Some((&name, args)) = toks.split_first() else {
        return Err(ErrorSignals::at(ln, "empty member line"));
    };
    let rest = member[member.find(name).unwrap_or(0) + name.len()..].trim();
    let name = ident(name, ln)?;

    match item {
        Item::Type(t) => {
            let [value] = args else {
                return Err(ErrorSignals::at(ln, "expected `.variant <value>`"));
            };
            let value = parse_int(value).ok_or_else(|| ErrorSignals::at(ln, format!("invalid value `{value}`")))?;
            t.variants.push(Variant { name, value, line: ln });
        }
        Item::Def(d) => {
            const USAGE: &str = "expected `.field <bits> <Type> [big|little]` (the type is required)";
            let Some((bits, extra)) = args.split_first() else {
                return Err(ErrorSignals::at(ln, USAGE));
            };
            let (mut ty, mut big) = (None, None);
            for tok in extra {
                match endian(tok) {
                    Some(b) if big.is_none() => big = Some(b),
                    None if ty.is_none() => ty = Some(ident(tok, ln)?),
                    _ => return Err(ErrorSignals::at(ln, USAGE)),
                }
            }
            let ty = ty.ok_or_else(|| ErrorSignals::at(ln, USAGE))?;
            let bits = parse_bits(bits).ok_or_else(|| ErrorSignals::at(ln, format!("invalid bit spec `{bits}`")))?;
            let byte = section.unwrap_or(d.bytes[0]);
            d.fields.push(Field { name, line: ln, byte, bits, ty, big: big.unwrap_or(big_default) });
        }
        Item::Spec(s) => {
            let [value, ty] = args else {
                return Err(ErrorSignals::at(ln, "expected `.name <value> <type>` (the type is required)"));
            };
            let value = parse_literal(value)
                .ok_or_else(|| ErrorSignals::at(ln, format!("spec values must be literals, found `{value}`")))?;
            s.entries.push(SpecEntry { name, line: ln, value, ty: ident(ty, ln)? });
        }
        Item::Filter(f) => {
            if let FrameKeyWords::Ext = FrameKeyWords::from(name.as_str()) {
                let value = match args {
                    [] => Value::Lit(Literal::Bool(true)),
                    [v] => parse_value(v).ok_or_else(|| ErrorSignals::at(ln, format!("invalid value `{v}`")))?,
                    _ => return Err(ErrorSignals::at(ln, "expected `.ext [value]`")),
                };
                f.conds.push(Cond { line: ln, target: Target::Ext, bits: BitSpec::Bit(0), value: Match::Eq(value), big: false });
                return Ok(());
            }
            let (args, big) = match args.split_last() {
                Some((last, rest)) if endian(last).is_some() => (rest, endian(last).unwrap_or(big_default)),
                _ => (args, big_default),
            };
            let (target, bits, value) = match (FrameKeyWords::from(name.as_str()), args) {
                (FrameKeyWords::Id, [bits, value]) => (Target::Id, bits, value),
                (FrameKeyWords::Data, [byte, bits, value]) => {
                    let byte = match parse_byte_list(byte).as_deref() {
                        Some([b]) => *b,
                        _ => return Err(ErrorSignals::at(ln, format!("expected a single data byte `{{n}}`, found `{byte}`"))),
                    };
                    (Target::Data(byte), bits, value)
                }
                _ => {
                    return Err(ErrorSignals::at(
                        ln,
                        "expected `.id <bits> <value>`, `.data {byte} <bits> <value> [big|little]` or `.ext [value]`",
                    ))
                }
            };
            let bits = parse_bits(bits).ok_or_else(|| ErrorSignals::at(ln, format!("invalid bit spec `{bits}`")))?;
            let value = parse_match(value).ok_or_else(|| ErrorSignals::at(ln, format!("invalid value `{value}`")))?;
            if matches!(value, Match::Range { .. }) && target != Target::Id {
                return Err(ErrorSignals::at(ln, "value ranges are only supported on `.id`"));
            }
            f.conds.push(Cond { line: ln, target, bits, value, big });
        }
        Item::Frame(f) => match FrameKeyWords::from(name.as_str()) {
            FrameKeyWords::Size => {
                let [value] = args else {
                    return Err(ErrorSignals::at(ln, "expected `.size 'N`"));
                };
                let size = size_mark(value)
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| ErrorSignals::at(ln, format!("invalid size `{value}`")))?;
                if f.size.replace(size).is_some() {
                    return Err(ErrorSignals::at(ln, "duplicate `.size`"));
                }
            }
            FrameKeyWords::Fd => f.fd = flag(args, ln)?,
            FrameKeyWords::Filter => {
                if f.filter.replace(parse_filter_expr(rest, file.extended_id, ln)?).is_some() {
                    return Err(ErrorSignals::at(ln, "duplicate `.filter`"));
                }
            }
            FrameKeyWords::Id => return Err(ErrorSignals::at(ln, "frames have no `.id`; match id bits with `.filter`")),
            _ => return Err(ErrorSignals::at(ln, format!("unknown frame attribute `.{name}`"))),
        },
        Item::Fn(f) => match FrameKeyWords::from(name.as_str()) {
            FrameKeyWords::Rsp => {
                if f.rsp.replace(parse_filter_expr(rest, file.extended_id, ln)?).is_some() {
                    return Err(ErrorSignals::at(ln, "duplicate `.rsp`"));
                }
            }
            FrameKeyWords::Fd => f.fd = flag(args, ln)?,
            _ => return Err(ErrorSignals::at(ln, format!("unknown fn attribute `.{name}` (expected `.rsp` or `.fd`)"))),
        },
    }
    Ok(())
}
