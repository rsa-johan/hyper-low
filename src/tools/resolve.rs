//! Name resolution, validation and layout. Produces the IR consumed by code generators.

use std::collections::{HashMap, HashSet};

use crate::{error::ErrorSignals, protocols::can};

use super::{
    ast::{BitSpec, Cond, Def, FilterDef, FilterExpr, FnDef, Frame, Item, Literal, Match, Param, Proto, Target, TypeDef, Value},
    syntax::{is_reserved, to_pascal, to_snake, to_upper_snake},
};

type Result<T> = std::result::Result<T, ErrorSignals>;

const MAX_ENUM_VALUE: u64 = i32::MAX as u64;
const ID_BITS: u32 = 29;
const STD_ID_BITS: u32 = 11;
const MAX_PAYLOAD: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntTy {
    U8,
    U16,
    U32,
    U64,
}

impl IntTy {
    pub fn for_bits(bits: u32) -> Self {
        match bits {
            0..=8 => Self::U8,
            9..=16 => Self::U16,
            17..=32 => Self::U32,
            _ => Self::U64,
        }
    }

    pub fn rust(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
        }
    }

    pub fn c(self) -> &'static str {
        match self {
            Self::U8 => "uint8_t",
            Self::U16 => "uint16_t",
            Self::U32 => "uint32_t",
            Self::U64 => "uint64_t",
        }
    }

    pub fn rust_signed(self) -> &'static str {
        match self {
            Self::U8 => "i8",
            Self::U16 => "i16",
            Self::U32 => "i32",
            Self::U64 => "i64",
        }
    }

    pub fn c_signed(self) -> &'static str {
        match self {
            Self::U8 => "int8_t",
            Self::U16 => "int16_t",
            Self::U32 => "int32_t",
            Self::U64 => "int64_t",
        }
    }
}

/// Fixed-size standard types used by `spec`, filter parameters and fn parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Prim {
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl Prim {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bool" => Self::Bool,
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "i64" => Self::I64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            _ => return None,
        })
    }

    pub fn size(self) -> usize {
        match self {
            Self::Bool | Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }

    pub fn rust(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }

    pub fn c(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::U8 => "uint8_t",
            Self::U16 => "uint16_t",
            Self::U32 => "uint32_t",
            Self::U64 => "uint64_t",
            Self::I8 => "int8_t",
            Self::I16 => "int16_t",
            Self::I32 => "int32_t",
            Self::I64 => "int64_t",
            Self::F32 => "float",
            Self::F64 => "double",
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    pub fn is_signed(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    fn int_range(self) -> Option<(i128, i128)> {
        let bits = self.size() as u32 * 8;
        match self {
            Self::U8 | Self::U16 | Self::U32 | Self::U64 => Some((0, (1i128 << bits) - 1)),
            Self::I8 | Self::I16 | Self::I32 | Self::I64 => Some((-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)),
            _ => None,
        }
    }
}

/// Resolved scope of one file. Items marked `external` come from includes and are generated there.
#[derive(Debug, Default)]
pub struct Model {
    pub enums: Vec<EnumIr>,
    pub defs: Vec<DefIr>,
    pub specs: Vec<SpecIr>,
    pub frames: Vec<FrameIr>,
    pub fns: Vec<FnIr>,
}

impl Model {
    /// Every filter this file applies, labelled for diagnostics.
    pub fn filter_uses(&self) -> Vec<(String, &FilterIr)> {
        let frames = self.frames.iter().filter(|f| !f.external).filter_map(|f| {
            f.filter.as_ref().map(|flt| (format!("frame `{}` .filter", f.name), flt))
        });
        let fns = self.fns.iter().filter(|f| !f.external).filter_map(|f| {
            f.rsp.as_ref().map(|flt| (format!("fn `{}` .rsp", f.name), flt))
        });
        frames.chain(fns).collect()
    }
}

#[derive(Debug)]
pub struct EnumIr {
    pub name: String,
    pub repr: IntTy,
    /// Bits needed for the largest variant value.
    pub width: u32,
    pub variants: Vec<(String, u64)>,
    /// Index of the variant used for `Default` (the one with value 0, else the first).
    pub default: usize,
    pub external: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum FieldTy {
    Bool,
    Uint(IntTy),
    /// Two's complement, sign-extended from the field width into this storage size.
    Sint(IntTy),
    Enum(usize),
}

#[derive(Debug)]
pub struct FieldIr {
    pub name: String,
    /// Section byte the bit spec is relative to.
    pub byte: usize,
    /// Start bit (LSB of the value) counted from the section byte's bit 0.
    pub start: u32,
    pub width: u32,
    /// Big-endian: the section byte is the most significant byte of the field.
    pub big: bool,
    pub ty: FieldTy,
}

/// Payload bit (`byte * 8 + bit`) holding value bit `i` of a field.
///
/// Bits are numbered LSB-first across the bytes spanned from the section byte, which are
/// read as one little- or big-endian number.
pub fn phys_bit(byte: usize, start: u32, width: u32, big: bool, i: u32) -> usize {
    let l = start + i;
    let span = (start + width).div_ceil(8) as usize;
    let b = if big { byte + span - 1 - (l / 8) as usize } else { byte + (l / 8) as usize };
    b * 8 + (l % 8) as usize
}

/// A slice of a field that lives in a single payload byte.
#[derive(Debug, PartialEq)]
pub struct Chunk {
    pub byte: usize,
    /// Bit offset inside the byte.
    pub shift: u32,
    /// Unshifted mask of the chunk's bits.
    pub mask: u8,
    /// Offset of this chunk inside the field value.
    pub vshift: u32,
}

impl FieldIr {
    pub fn bit_pos(&self, i: u32) -> usize {
        phys_bit(self.byte, self.start, self.width, self.big, i)
    }

    /// Splits the field into per-byte chunks of consecutive value bits.
    pub fn chunks(&self) -> Vec<Chunk> {
        let mut out: Vec<Chunk> = Vec::new();
        for i in 0..self.width {
            let p = self.bit_pos(i);
            match out.last_mut() {
                Some(c) if c.byte == p / 8 => c.mask = (c.mask << 1) | 1,
                _ => out.push(Chunk { byte: p / 8, shift: (p % 8) as u32, mask: 1, vshift: i }),
            }
        }
        out
    }
}

#[derive(Debug)]
pub struct DefIr {
    pub name: String,
    /// Absolute bytes, or `0..N` relative to the placement offset for generic defs.
    pub bytes: Vec<usize>,
    /// Minimum payload length needed to hold this def (its size for generic defs).
    pub min_len: usize,
    pub fields: Vec<FieldIr>,
    pub generic: bool,
    pub external: bool,
}

#[derive(Debug)]
pub struct SpecIr {
    pub name: String,
    pub ty: Prim,
    pub value: Literal,
    pub external: bool,
}

/// Mask/match over the CAN id and payload. Data vectors are trimmed to the last checked byte.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilterIr {
    /// Exact id bits (ranges are kept separately in `id_ranges`).
    pub id_mask: u32,
    pub id_match: u32,
    /// Inclusive value ranges on id bit spans that are not a single mask/match.
    pub id_ranges: Vec<IdRange>,
    /// Required id type: `Some(true)` extended, `Some(false)` standard, `None` either.
    pub ext: Option<bool>,
    pub data_mask: Vec<u8>,
    pub data_match: Vec<u8>,
}

/// `lo <= (id >> start) & mask <= hi`, unsigned.
#[derive(Debug, Clone, PartialEq)]
pub struct IdRange {
    pub start: u32,
    pub width: u32,
    pub lo: u64,
    pub hi: u64,
}

impl IdRange {
    /// Unshifted mask of the span.
    pub fn mask(&self) -> u64 {
        (1u64 << self.width) - 1
    }

    /// Aligned blocks covering `lo..=hi`, as shifted id (mask, match) pairs.
    fn terms(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut lo = self.lo;
        while lo <= self.hi {
            let mut k = 0;
            while k < self.width && lo % (1 << (k + 1)) == 0 && lo + (1 << (k + 1)) - 1 <= self.hi {
                k += 1;
            }
            let mask = (self.mask() & !((1u64 << k) - 1)) << self.start;
            out.push((mask as u32, (lo << self.start) as u32));
            lo += 1 << k;
        }
        out
    }
}

impl FilterIr {
    pub fn data_len(&self) -> usize {
        self.data_mask.len()
    }

    /// Mask/match pairs whose union accepts exactly the filter's ids (one per hardware filter slot).
    pub fn id_terms(&self) -> Vec<(u32, u32)> {
        let mut terms = vec![(self.id_mask, self.id_match)];
        for r in &self.id_ranges {
            let rt = r.terms();
            terms = terms.iter().flat_map(|&(m, v)| rt.iter().map(move |&(rm, rv)| (m | rm, v | rv))).collect();
        }
        terms
    }

    /// True if some frame could pass both filters.
    pub fn overlaps(&self, o: &Self) -> bool {
        if matches!((self.ext, o.ext), (Some(a), Some(b)) if a != b) {
            return false;
        }
        let (ta, tb) = (self.id_terms(), o.id_terms());
        if !ta.iter().any(|&(ma, va)| tb.iter().any(|&(mb, vb)| (ma & mb) & (va ^ vb) == 0)) {
            return false;
        }
        (0..self.data_len().min(o.data_len()))
            .all(|i| (self.data_mask[i] & o.data_mask[i]) & (self.data_match[i] ^ o.data_match[i]) == 0)
    }

    fn data_bit(&self, pos: usize) -> bool {
        self.data_mask.get(pos / 8).is_some_and(|m| m >> (pos % 8) & 1 != 0)
    }
}

#[derive(Debug)]
pub struct FrameIr {
    pub name: String,
    pub size: usize,
    pub fd: bool,
    pub members: Vec<MemberIr>,
    pub filter: Option<FilterIr>,
    pub external: bool,
}

#[derive(Debug)]
pub struct MemberIr {
    /// Struct field name.
    pub field: String,
    /// Index into `Model::defs`.
    pub def: usize,
    /// Placement offset of a generic def (0 for fixed defs).
    pub at: usize,
}

/// Where a fn parameter or return value lives in the payload.
#[derive(Debug, Clone, Copy)]
pub enum Slot {
    /// Standard type at a byte offset (byte order from the fn's file).
    Prim(Prim, usize),
    /// Def and its placement offset (0 for fixed defs).
    Def(usize, usize),
    Frame(usize),
}

#[derive(Debug)]
pub struct ParamIr {
    pub name: String,
    pub slot: Slot,
}

#[derive(Debug)]
pub struct FnIr {
    pub name: String,
    pub params: Vec<ParamIr>,
    pub req_size: usize,
    pub ret: Option<Slot>,
    pub rsp: Option<FilterIr>,
    pub rsp_size: usize,
    pub fd: bool,
    /// Byte order of standard-type parameters and return values.
    pub big: bool,
    pub external: bool,
}

pub fn check_overlaps(uses: &[(String, &FilterIr)]) -> Result<()> {
    for (i, (a, fa)) in uses.iter().enumerate() {
        for (b, fb) in &uses[i + 1..] {
            if fa.overlaps(fb) {
                return Err(ErrorSignals::Generic(format!("{a} and {b} can match the same frame")));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Sym<'a> {
    Enum(usize),
    Def(usize),
    Frame(usize),
    Filter(&'a FilterDef),
    Other,
}

#[derive(Debug, Clone, Copy)]
enum Val {
    Int(i128),
    Float(f64),
    Bool(bool),
}

impl From<Literal> for Val {
    fn from(l: Literal) -> Self {
        match l {
            Literal::Int(v) => Val::Int(v),
            Literal::Float(v) => Val::Float(v),
            Literal::Bool(v) => Val::Bool(v),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ParamTy {
    Prim(Prim),
    Enum(usize),
}

struct Ctx<'a> {
    syms: HashMap<&'a str, Sym<'a>>,
    specs: HashMap<&'a str, Val>,
    model: Model,
}

fn check_name(name: &str, ln: usize) -> Result<()> {
    if is_reserved(name) {
        return Err(ErrorSignals::at(ln, format!("`{name}` is a reserved word in the generated code")));
    }
    Ok(())
}

/// Resolves a file together with its includes; the bool marks files whose items are external.
pub fn resolve(scope: &[(&Proto, bool)]) -> Result<Model> {
    let items: Vec<(&Item, bool)> =
        scope.iter().flat_map(|(p, ext)| p.items.iter().map(move |i| (i, *ext))).collect();

    let mut ctx = Ctx { syms: HashMap::new(), specs: HashMap::new(), model: Model::default() };
    let mut c_names: HashMap<String, usize> = HashMap::new();
    let (mut n_enum, mut n_def, mut n_frame) = (0, 0, 0);
    for (item, _) in &items {
        let Some(name) = item.name() else { continue };
        let ln = item.line();
        check_name(name, ln)?;
        let sym = match item {
            Item::Type(_) => Sym::Enum(post_inc(&mut n_enum)),
            Item::Def(_) => Sym::Def(post_inc(&mut n_def)),
            Item::Frame(_) => Sym::Frame(post_inc(&mut n_frame)),
            Item::Filter(f) => Sym::Filter(f),
            _ => Sym::Other,
        };
        let mut names = vec![name.to_string()];
        if let Item::Fn(_) = item {
            names.push(format!("{name}Request"));
        }
        for n in names {
            if let Some(prev) = c_names.insert(to_upper_snake(&n), ln) {
                return Err(ErrorSignals::at(ln, format!("`{n}` clashes with the name defined on line {prev}")));
            }
        }
        if ctx.syms.insert(name, sym).is_some() {
            return Err(ErrorSignals::at(ln, format!("`{name}` is defined more than once")));
        }
    }

    for (item, ext) in &items {
        if let Item::Type(t) = item {
            ctx.model.enums.push(resolve_enum(t, *ext)?);
        }
    }
    for (item, ext) in &items {
        let Item::Spec(s) = item else { continue };
        for e in &s.entries {
            check_name(&e.name, e.line)?;
            let ty = Prim::parse(&e.ty).ok_or_else(|| {
                ErrorSignals::at(e.line, format!("spec values must use a standard type, found `{}`", e.ty))
            })?;
            let val = coerce(e.value.into(), ParamTy::Prim(ty), &ctx.model).map_err(|m| ErrorSignals::at(e.line, m))?;
            if ctx.specs.insert(&e.name, val).is_some() {
                return Err(ErrorSignals::at(e.line, format!("`spec.{}` is defined more than once", e.name)));
            }
            let value = match val {
                Val::Int(v) => Literal::Int(v),
                Val::Float(v) => Literal::Float(v),
                Val::Bool(v) => Literal::Bool(v),
            };
            ctx.model.specs.push(SpecIr { name: e.name.clone(), ty, value, external: *ext });
        }
    }
    for (item, ext) in &items {
        if let Item::Def(d) = item {
            let def = resolve_def(d, *ext, &ctx)?;
            ctx.model.defs.push(def);
        }
    }
    for (item, ext) in &items {
        if let (Item::Filter(f), false) = (item, ext) {
            validate_filter(f, &ctx)?;
        }
    }
    for (item, ext) in &items {
        if let Item::Frame(fr) = item {
            let frame = resolve_frame(fr, *ext, &ctx)?;
            ctx.model.frames.push(frame);
        }
    }
    for (item, ext) in &items {
        if let Item::Fn(f) = item {
            let func = resolve_fn(f, *ext, &ctx)?;
            ctx.model.fns.push(func);
        }
    }
    Ok(ctx.model)
}

fn resolve_enum(t: &TypeDef, external: bool) -> Result<EnumIr> {
    if t.variants.is_empty() {
        return Err(ErrorSignals::at(t.line, format!("type `{}` has no variants", t.name)));
    }
    let (mut names, mut values) = (HashSet::new(), HashMap::new());
    for v in &t.variants {
        check_name(&v.name, v.line)?;
        if !names.insert(to_pascal(&v.name)) {
            return Err(ErrorSignals::at(v.line, format!("duplicate variant `{}`", v.name)));
        }
        if let Some(other) = values.insert(v.value, &v.name) {
            return Err(ErrorSignals::at(v.line, format!("`{}` has the same value as `{other}`", v.name)));
        }
        if v.value > MAX_ENUM_VALUE {
            return Err(ErrorSignals::at(v.line, format!("enum value must be <= {MAX_ENUM_VALUE}")));
        }
    }
    let width = bit_width(t.variants.iter().map(|v| v.value).max().unwrap_or(0));
    Ok(EnumIr {
        name: t.name.clone(),
        repr: IntTy::for_bits(width),
        width,
        variants: t.variants.iter().map(|v| (v.name.clone(), v.value)).collect(),
        default: t.variants.iter().position(|v| v.value == 0).unwrap_or(0),
        external,
    })
}

fn lookup_enum(ctx: &Ctx, ty: &str, ln: usize) -> Result<usize> {
    match ctx.syms.get(ty) {
        Some(Sym::Enum(i)) => Ok(*i),
        Some(_) => Err(ErrorSignals::at(ln, format!("`{ty}` is not a `type`"))),
        None => Err(ErrorSignals::at(ln, format!("unknown type `{ty}`"))),
    }
}

#[derive(Clone, Copy)]
enum FieldAnn {
    Enum(usize),
    Prim(Prim),
}

fn resolve_def(d: &Def, external: bool, ctx: &Ctx) -> Result<DefIr> {
    let mut seen_bytes = HashSet::new();
    if let Some(b) = d.bytes.iter().find(|b| !seen_bytes.insert(**b)) {
        return Err(ErrorSignals::at(d.line, format!("byte {b} listed twice")));
    }
    if let Some(b) = d.bytes.iter().find(|b| **b >= MAX_PAYLOAD) {
        return Err(ErrorSignals::at(d.line, format!("byte {b} is beyond the {MAX_PAYLOAD}-byte CAN FD payload")));
    }
    if d.fields.is_empty() {
        return Err(ErrorSignals::at(d.line, format!("def `{}` has no fields", d.name)));
    }
    let mut used_bits: HashMap<usize, &str> = HashMap::new();
    let mut names = HashSet::new();
    let mut fields = Vec::new();
    for f in &d.fields {
        check_name(&f.name, f.line)?;
        if !names.insert(f.name.as_str()) {
            return Err(ErrorSignals::at(f.line, format!("duplicate field `{}`", f.name)));
        }
        let ann = match Prim::parse(&f.ty) {
            Some(p) if p.is_float() => {
                return Err(ErrorSignals::at(f.line, "float fields are not supported in a `def`; use a fn parameter"));
            }
            Some(p) => FieldAnn::Prim(p),
            None => FieldAnn::Enum(lookup_enum(ctx, &f.ty, f.line)?),
        };
        // Bits the enum needs, or the standard type holds.
        let type_width = match ann {
            FieldAnn::Enum(i) => ctx.model.enums[i].width,
            FieldAnn::Prim(Prim::Bool) => 1,
            FieldAnn::Prim(p) => p.size() as u32 * 8,
        };
        let (start, width) = match f.bits {
            BitSpec::Full => (0, 8),
            BitSpec::Bit(n) => (n, type_width),
            BitSpec::From(n) => (n, 8u32.saturating_sub(n)),
            BitSpec::Range(a, b) => (a, b - a),
        };
        if start >= 8 {
            return Err(ErrorSignals::at(f.line, format!("start bit {start} is outside the byte (0..8)")));
        }
        if width > 64 {
            return Err(ErrorSignals::at(f.line, format!("field is {width} bits wide, max is 64")));
        }
        match ann {
            FieldAnn::Enum(_) if type_width > width => {
                return Err(ErrorSignals::at(f.line, format!("`{}` needs {type_width} bits but the field has {width}", f.ty)));
            }
            FieldAnn::Prim(_) if width > type_width => {
                return Err(ErrorSignals::at(f.line, format!("a {width}-bit field does not fit `{}`", f.ty)));
            }
            _ => {}
        }
        for i in 0..width {
            let bit = phys_bit(f.byte, start, width, f.big, i);
            if !d.bytes.contains(&(bit / 8)) {
                return Err(ErrorSignals::at(
                    f.line,
                    format!("field `{}` reaches byte {} which is not declared in `{}`", f.name, bit / 8, d.name),
                ));
            }
            if let Some(other) = used_bits.insert(bit, &f.name) {
                return Err(ErrorSignals::at(
                    f.line,
                    format!("field `{}` overlaps `{other}` at byte {} bit {}", f.name, bit / 8, bit % 8),
                ));
            }
        }
        let ty = match ann {
            FieldAnn::Enum(i) => FieldTy::Enum(i),
            FieldAnn::Prim(Prim::Bool) => FieldTy::Bool,
            FieldAnn::Prim(p) if p.is_signed() => FieldTy::Sint(IntTy::for_bits(p.size() as u32 * 8)),
            FieldAnn::Prim(p) => FieldTy::Uint(IntTy::for_bits(p.size() as u32 * 8)),
        };
        fields.push(FieldIr { name: f.name.clone(), byte: f.byte, start, width, big: f.big, ty });
    }
    Ok(DefIr {
        name: d.name.clone(),
        bytes: d.bytes.clone(),
        min_len: d.bytes.iter().max().map_or(0, |m| m + 1),
        fields,
        generic: d.generic,
        external,
    })
}

// ---- filters ----

/// Mask/match accumulator for one filter expression.
struct Acc {
    id_mask: u32,
    id_match: u32,
    id_ranges: Vec<IdRange>,
    ext: Option<bool>,
    data_mask: [u8; MAX_PAYLOAD],
    data_match: [u8; MAX_PAYLOAD],
    /// Validating a filter with placeholder arguments: skip value-dependent range errors.
    lenient: bool,
}

impl Acc {
    fn new() -> Self {
        Self {
            id_mask: 0,
            id_match: 0,
            id_ranges: Vec::new(),
            ext: None,
            data_mask: [0; MAX_PAYLOAD],
            data_match: [0; MAX_PAYLOAD],
            lenient: false,
        }
    }

    fn range_bits(&self) -> u32 {
        self.id_ranges.iter().fold(0, |m, r| m | ((r.mask() as u32) << r.start))
    }

    /// `lenient`: bounds depend on placeholder arguments, so value-dependent errors are skipped.
    fn set_range(&mut self, start: u32, width: u32, lo: i128, hi: i128, lenient: bool) -> std::result::Result<(), String> {
        let max = (1i128 << width) - 1;
        let span = format!("id bits {start}..{}", start + width);
        if lo > hi || lo == 0 && hi == max {
            if lenient {
                return Ok(());
            }
            return Err(if lo > hi {
                format!("empty range on {span}")
            } else {
                format!("range on {span} accepts every value; drop the condition")
            });
        }
        let bits = (max as u32) << start;
        if self.id_mask & bits != 0 {
            return Err(format!("range on {span} overlaps an exact id condition"));
        }
        if let Some(r) = self.id_ranges.iter_mut().find(|r| r.start == start && r.width == width) {
            r.lo = r.lo.max(lo as u64);
            r.hi = r.hi.min(hi as u64);
            if r.lo > r.hi && !lenient {
                return Err(format!("ranges on {span} have no value in common"));
            }
            return Ok(());
        }
        if self.range_bits() & bits != 0 {
            return Err(format!("range on {span} partly overlaps another id range"));
        }
        self.id_ranges.push(IdRange { start, width, lo: lo as u64, hi: hi as u64 });
        Ok(())
    }

    fn set_ext(&mut self, ext: bool) -> std::result::Result<(), String> {
        if self.ext.replace(ext).is_some_and(|cur| cur != ext) {
            return Err("conflicting values for `.ext`".into());
        }
        Ok(())
    }

    /// Standard ids only have bits 0..11.
    fn check_std(&self) -> std::result::Result<(), String> {
        let high = (self.id_mask | self.range_bits()) >> STD_ID_BITS;
        if self.ext == Some(false) && high != 0 {
            let bit = STD_ID_BITS + 31 - high.leading_zeros();
            return Err(format!("standard (11-bit) ids have no bit {bit}; use `.ext` or `#id=extended`"));
        }
        Ok(())
    }

    fn set(&mut self, data: bool, bits: &[usize], pattern: u64) -> std::result::Result<(), String> {
        for (i, &pos) in bits.iter().enumerate() {
            let want = (pattern >> i) & 1 != 0;
            let (set, cur, what) = if data {
                (
                    self.data_mask[pos / 8] >> (pos % 8) & 1 != 0,
                    self.data_match[pos / 8] >> (pos % 8) & 1 != 0,
                    format!("data byte {} bit {}", pos / 8, pos % 8),
                )
            } else {
                (self.id_mask >> pos & 1 != 0, self.id_match >> pos & 1 != 0, format!("id bit {pos}"))
            };
            if set && cur != want {
                return Err(format!("conflicting values for {what}"));
            }
            if !data && self.range_bits() >> pos & 1 != 0 {
                return Err(format!("exact id bit {pos} overlaps an id range condition"));
            }
            if data {
                self.data_mask[pos / 8] |= 1 << (pos % 8);
                self.data_match[pos / 8] |= u8::from(want) << (pos % 8);
            } else {
                self.id_mask |= 1 << pos;
                self.id_match |= u32::from(want) << pos;
            }
        }
        Ok(())
    }

    fn finish(self) -> FilterIr {
        let len = self.data_mask.iter().rposition(|m| *m != 0).map_or(0, |i| i + 1);
        let (mut id_mask, mut id_match, mut id_ranges) = (self.id_mask, self.id_match, Vec::new());
        // Ranges that are one aligned block are plain mask/match.
        for r in self.id_ranges {
            match r.terms()[..] {
                [(m, v)] => {
                    id_mask |= m;
                    id_match |= v;
                }
                _ => id_ranges.push(r),
            }
        }
        FilterIr {
            id_mask,
            id_match,
            id_ranges,
            ext: self.ext,
            data_mask: self.data_mask[..len].to_vec(),
            data_match: self.data_match[..len].to_vec(),
        }
    }
}

/// Id bit or payload bit for each value bit of a condition, LSB first.
fn cond_bits(c: &Cond) -> std::result::Result<Vec<usize>, String> {
    match c.target {
        Target::Ext => Ok(Vec::new()),
        Target::Id => {
            let (start, width) = match c.bits {
                BitSpec::Full => (0, ID_BITS),
                BitSpec::Bit(n) => (n, 1),
                BitSpec::From(n) => (n, ID_BITS.saturating_sub(n)),
                BitSpec::Range(a, b) => (a, b - a),
            };
            if width == 0 || start + width > ID_BITS {
                return Err(format!("id bits must be within 0..{ID_BITS}"));
            }
            Ok((start as usize..(start + width) as usize).collect())
        }
        Target::Data(byte) => {
            let (start, width) = match c.bits {
                BitSpec::Full => (0, 8),
                BitSpec::Bit(n) => (n, 1),
                BitSpec::From(n) => (n, 8u32.saturating_sub(n)),
                BitSpec::Range(a, b) => (a, b - a),
            };
            if start >= 8 || width == 0 {
                return Err(format!("start bit {start} is outside the byte (0..8)"));
            }
            if width > 64 {
                return Err(format!("condition is {width} bits wide, max is 64"));
            }
            if byte + (start + width).div_ceil(8) as usize > MAX_PAYLOAD {
                return Err(format!("data bits reach beyond the {MAX_PAYLOAD}-byte payload"));
            }
            Ok((0..width).map(|i| phys_bit(byte, start, width, c.big, i)).collect())
        }
    }
}

fn param_ty(p: &Param, ctx: &Ctx, ln: usize) -> Result<ParamTy> {
    if let Some(prim) = Prim::parse(&p.ty) {
        return Ok(ParamTy::Prim(prim));
    }
    lookup_enum(ctx, &p.ty, ln).map(ParamTy::Enum)
}

fn eval(v: &Value, env: &HashMap<&str, Val>, expect: Option<ParamTy>, ctx: &Ctx) -> std::result::Result<Val, String> {
    match v {
        Value::Lit(l) => Ok((*l).into()),
        Value::Spec(n) => ctx.specs.get(n.as_str()).copied().ok_or_else(|| format!("unknown `spec.{n}`")),
        Value::Variant(t, n) => {
            let Some(Sym::Enum(i)) = ctx.syms.get(t.as_str()) else {
                return Err(format!("unknown type `{t}`"));
            };
            variant_value(&ctx.model.enums[*i], n).ok_or_else(|| format!("`{t}` has no variant `{n}`"))
        }
        Value::Name(n) => {
            if let Some(v) = env.get(n.as_str()) {
                return Ok(*v);
            }
            if let Some(ParamTy::Enum(i)) = expect {
                let e = &ctx.model.enums[i];
                return variant_value(e, n).ok_or_else(|| format!("`{}` has no variant `{n}`", e.name));
            }
            Err(format!("unknown name `{n}`"))
        }
    }
}

fn variant_value(e: &EnumIr, name: &str) -> Option<Val> {
    e.variants.iter().find(|(v, _)| v == name).map(|(_, val)| Val::Int(i128::from(*val)))
}

fn coerce(v: Val, ty: ParamTy, model: &Model) -> std::result::Result<Val, String> {
    match (ty, v) {
        (ParamTy::Enum(i), Val::Int(n)) => {
            let e = &model.enums[i];
            if e.variants.iter().any(|(_, val)| i128::from(*val) == n) {
                Ok(v)
            } else {
                Err(format!("{n} is not a value of `{}`", e.name))
            }
        }
        (ParamTy::Prim(Prim::Bool), Val::Bool(_)) => Ok(v),
        (ParamTy::Prim(Prim::F32), Val::Float(f)) => Ok(Val::Float(f64::from(f as f32))),
        (ParamTy::Prim(Prim::F32), Val::Int(n)) => Ok(Val::Float(f64::from(n as f32))),
        (ParamTy::Prim(Prim::F64), Val::Float(_)) => Ok(v),
        (ParamTy::Prim(Prim::F64), Val::Int(n)) => Ok(Val::Float(n as f64)),
        (ParamTy::Prim(p), Val::Int(n)) if !p.is_float() && p != Prim::Bool => {
            let (lo, hi) = p.int_range().unwrap_or((0, 0));
            if (lo..=hi).contains(&n) {
                Ok(v)
            } else {
                Err(format!("{n} does not fit `{}`", p.rust()))
            }
        }
        (ParamTy::Enum(i), _) => Err(format!("expected a `{}` value", model.enums[i].name)),
        (ParamTy::Prim(p), _) => Err(format!("expected a `{}` value", p.rust())),
    }
}

/// Bit pattern of `v` in a `width`-bit field.
fn pattern(v: Val, width: u32) -> std::result::Result<u64, String> {
    let max = if width >= 64 { u64::MAX } else { (1u64 << width) - 1 };
    match v {
        Val::Bool(b) => Ok(u64::from(b)),
        Val::Int(n) if n >= 0 => {
            if n > i128::from(max) {
                return Err(format!("{n} does not fit in {width} bits"));
            }
            Ok(n as u64)
        }
        Val::Int(n) => {
            if width < 128 && n < -(1i128 << (width - 1)) {
                return Err(format!("{n} does not fit in {width} signed bits"));
            }
            Ok((n as i64 as u64) & max)
        }
        Val::Float(f) => match width {
            32 => Ok(u64::from((f as f32).to_bits())),
            64 => Ok(f.to_bits()),
            _ => Err(format!("float values need a 32 or 64 bit field, not {width}")),
        },
    }
}

/// Applies a filter's conditions; errors carry the condition's line.
fn apply(f: &FilterDef, args: &[Val], acc: &mut Acc, ctx: &Ctx) -> std::result::Result<(), (usize, String)> {
    let env: HashMap<&str, Val> = f.params.iter().map(|p| p.name.as_str()).zip(args.iter().copied()).collect();
    for c in &f.conds {
        let value = match &c.value {
            Match::Eq(v) => v,
            Match::Range { lo, hi, inclusive } => {
                let bits = cond_bits(c).map_err(|e| (c.line, e))?;
                let (start, width) = (bits[0] as u32, bits.len() as u32);
                let max = (1i128 << width) - 1;
                // An exclusive upper bound may be one past the largest value (`0..8` on 3 bits).
                let bound = |v: &Value, end: bool| match eval(v, &env, None, ctx) {
                    Ok(Val::Int(n)) if (0..=max + i128::from(end)).contains(&n) => Ok(n),
                    Ok(Val::Int(n)) => Err(format!("range bound {n} does not fit in {width} unsigned bits")),
                    Ok(_) => Err("range bounds must be integers".to_string()),
                    Err(e) => Err(e),
                };
                let lo_v = lo.as_ref().map(|v| bound(v, false)).transpose().map_err(|e| (c.line, e))?.unwrap_or(0);
                let hi_v = match hi {
                    Some(v) => bound(v, !inclusive).map_err(|e| (c.line, e))? - i128::from(!inclusive),
                    None => max,
                };
                let depends = [lo, hi].iter().any(|b| matches!(b, Some(Value::Name(n)) if env.contains_key(n.as_str())));
                acc.set_range(start, width, lo_v, hi_v, acc.lenient && depends).map_err(|e| (c.line, e))?;
                continue;
            }
        };
        let val = eval(value, &env, None, ctx).map_err(|e| (c.line, e))?;
        if c.target == Target::Ext {
            let ext = match val {
                Val::Bool(b) => b,
                Val::Int(n @ (0 | 1)) => n == 1,
                _ => return Err((c.line, "`.ext` needs a boolean".into())),
            };
            acc.set_ext(ext).map_err(|e| (c.line, e))?;
            continue;
        }
        let bits = cond_bits(c).map_err(|e| (c.line, e))?;
        let pat = pattern(val, bits.len() as u32).map_err(|e| (c.line, e))?;
        acc.set(matches!(c.target, Target::Data(_)), &bits, pat).map_err(|e| (c.line, e))?;
    }
    acc.check_std().map_err(|e| (f.line, e))
}

fn validate_filter(f: &FilterDef, ctx: &Ctx) -> Result<()> {
    if f.conds.is_empty() {
        return Err(ErrorSignals::at(f.line, format!("filter `{}` has no conditions", f.name)));
    }
    let mut names = HashSet::new();
    let mut placeholders = Vec::new();
    for p in &f.params {
        if !names.insert(p.name.as_str()) {
            return Err(ErrorSignals::at(f.line, format!("duplicate parameter `{}`", p.name)));
        }
        placeholders.push(match param_ty(p, ctx, f.line)? {
            ParamTy::Prim(Prim::Bool) => Val::Bool(false),
            ParamTy::Prim(p) if p.is_float() => Val::Float(0.0),
            _ => Val::Int(0),
        });
    }
    let mut acc = Acc::new();
    acc.lenient = true;
    apply(f, &placeholders, &mut acc, ctx).map_err(|(ln, e)| ErrorSignals::at(ln, e))
}

fn eval_expr(expr: &FilterExpr, ctx: &Ctx) -> Result<FilterIr> {
    let ln = expr.line;
    let mut acc = Acc::new();
    for call in &expr.calls {
        let f = match ctx.syms.get(call.name.as_str()) {
            Some(Sym::Filter(f)) => *f,
            Some(_) => return Err(ErrorSignals::at(ln, format!("`{}` is not a filter", call.name))),
            None => return Err(ErrorSignals::at(ln, format!("unknown filter `{}`", call.name))),
        };
        if call.args.len() != f.params.len() {
            return Err(ErrorSignals::at(
                ln,
                format!("filter `{}` takes {} argument(s), got {}", f.name, f.params.len(), call.args.len()),
            ));
        }
        let mut args = Vec::new();
        for (a, p) in call.args.iter().zip(&f.params) {
            let ty = param_ty(p, ctx, ln)?;
            let v = eval(a, &HashMap::new(), Some(ty), ctx)
                .and_then(|v| coerce(v, ty, &ctx.model))
                .map_err(|e| ErrorSignals::at(ln, format!("argument `{}` of `{}`: {e}", p.name, f.name)))?;
            args.push(v);
        }
        apply(f, &args, &mut acc, ctx)
            .map_err(|(_, e)| ErrorSignals::at(ln, format!("in filter `{}`: {e}", f.name)))?;
    }
    if acc.ext.is_none() {
        acc.ext = expr.ext_default;
    }
    acc.check_std().map_err(|e| ErrorSignals::at(ln, e))?;
    Ok(acc.finish())
}

/// Payload bits written by a def placed at byte `at`.
fn def_bits(model: &Model, d: usize, at: usize) -> impl Iterator<Item = usize> + '_ {
    model.defs[d].fields.iter().flat_map(move |f| (0..f.width).map(move |i| f.bit_pos(i) + at * 8))
}

/// Bits written by a def or frame (fields and, for frames, their own filter).
fn slot_bits(slot: Slot, model: &Model) -> Vec<usize> {
    match slot {
        Slot::Def(d, at) => def_bits(model, d, at).collect(),
        Slot::Frame(fr) => model.frames[fr].members.iter().flat_map(|m| def_bits(model, m.def, m.at)).collect(),
        Slot::Prim(p, off) => (off * 8..(off + p.size()) * 8).collect(),
    }
}

fn check_reserved(filter: &FilterIr, slot: Slot, model: &Model, ln: usize, what: &str) -> Result<()> {
    if let Some(bit) = slot_bits(slot, model).into_iter().find(|b| filter.data_bit(*b)) {
        return Err(ErrorSignals::at(
            ln,
            format!("{what} uses data byte {} bit {} which the filter reserves", bit / 8, bit % 8),
        ));
    }
    if let Slot::Frame(fr) = slot {
        if let Some(own) = &model.frames[fr].filter {
            for i in 0..filter.data_len().min(own.data_len()) {
                if (filter.data_mask[i] & own.data_mask[i]) & (filter.data_match[i] ^ own.data_match[i]) != 0 {
                    return Err(ErrorSignals::at(ln, format!("filter contradicts the data bits of `{}`", model.frames[fr].name)));
                }
            }
        }
    }
    Ok(())
}

fn resolve_frame(fr: &Frame, external: bool, ctx: &Ctx) -> Result<FrameIr> {
    if fr.members.is_empty() {
        return Err(ErrorSignals::at(fr.line, format!("frame `{}` has no members", fr.name)));
    }
    let mut owner: HashMap<usize, String> = HashMap::new();
    let mut members = Vec::new();
    let mut field_names = HashSet::new();
    for m in &fr.members {
        let ln = m.line;
        let d = match ctx.syms.get(m.def.as_str()) {
            Some(Sym::Def(i)) => *i,
            Some(_) => return Err(ErrorSignals::at(ln, format!("`{}` is not a `def`", m.def))),
            None => return Err(ErrorSignals::at(ln, format!("unknown def `{}`", m.def))),
        };
        let def = &ctx.model.defs[d];
        let at = match (def.generic, m.at) {
            (true, Some(at)) => at,
            (true, None) => {
                return Err(ErrorSignals::at(ln, format!("generic def `{}` needs a start byte: `.{} {{n}}`", m.def, m.def)));
            }
            (false, Some(_)) => {
                return Err(ErrorSignals::at(
                    ln,
                    format!("`{}` is fixed to bytes {:?}; only generic defs (`def 'N`) take a start byte", m.def, def.bytes),
                ));
            }
            (false, None) => 0,
        };
        let field = match &m.field {
            Some(f) => {
                check_name(f, ln)?;
                f.clone()
            }
            None => to_snake(&m.def),
        };
        if !field_names.insert(field.clone()) {
            return Err(ErrorSignals::at(ln, format!("field `{field}` is defined more than once; name it with `.name: {}`", m.def)));
        }
        for b in def.bytes.iter().map(|b| b + at) {
            if b >= MAX_PAYLOAD {
                return Err(ErrorSignals::at(ln, format!("`{field}` reaches byte {b}, beyond the {MAX_PAYLOAD}-byte payload")));
            }
            if let Some(other) = owner.insert(b, field.clone()) {
                return Err(ErrorSignals::at(ln, format!("`{field}` and `{other}` both use byte {b}")));
            }
        }
        members.push(MemberIr { field, def: d, at });
    }
    let filter = fr.filter.as_ref().map(|e| eval_expr(e, ctx)).transpose()?;
    if let Some(flt) = &filter {
        for m in &members {
            check_reserved(flt, Slot::Def(m.def, m.at), &ctx.model, fr.line, &format!("`{}`", m.field))?;
        }
    }
    let used = owner.keys().max().map_or(0, |m| m + 1).max(filter.as_ref().map_or(0, FilterIr::data_len));
    let size = fr.size.unwrap_or(used);
    if used > size {
        return Err(ErrorSignals::at(fr.line, format!("members and filter use {used} bytes but `.size` is {size}")));
    }
    can::check_size(size, fr.fd).map_err(|e| ErrorSignals::at(fr.line, e))?;
    Ok(FrameIr { name: fr.name.clone(), size, fd: fr.fd, members, filter, external })
}

/// Lowest offset >= `from` with `size` free bytes.
fn place(used: &[bool; MAX_PAYLOAD], from: usize, size: usize) -> Option<usize> {
    (from..=MAX_PAYLOAD.saturating_sub(size)).find(|o| used[*o..o + size].iter().all(|u| !u))
}

/// How a fn parameter / return type gets its bytes.
enum Need {
    /// Fixed def or frame: bytes known up front.
    Fixed(Slot),
    /// Standard type or generic def: packed into free bytes.
    Packed(Packed),
}

#[derive(Clone, Copy)]
enum Packed {
    Prim(Prim),
    Def(usize),
}

impl Packed {
    fn size(self, model: &Model) -> usize {
        match self {
            Packed::Prim(p) => p.size(),
            Packed::Def(d) => model.defs[d].min_len,
        }
    }

    fn at(self, off: usize) -> Slot {
        match self {
            Packed::Prim(p) => Slot::Prim(p, off),
            Packed::Def(d) => Slot::Def(d, off),
        }
    }
}

fn slot_need(ty: &str, ctx: &Ctx, ln: usize) -> Result<Need> {
    Ok(match ctx.syms.get(ty) {
        Some(Sym::Def(d)) if ctx.model.defs[*d].generic => Need::Packed(Packed::Def(*d)),
        Some(Sym::Def(d)) => Need::Fixed(Slot::Def(*d, 0)),
        Some(Sym::Frame(f)) => Need::Fixed(Slot::Frame(*f)),
        Some(Sym::Enum(_)) => return Err(ErrorSignals::at(ln, format!("enum `{ty}` cannot be sent directly; wrap it in a `def`"))),
        Some(_) => return Err(ErrorSignals::at(ln, format!("`{ty}` is not a type"))),
        None => match Prim::parse(ty) {
            Some(p) => Need::Packed(Packed::Prim(p)),
            None => {
                let hint = match ty {
                    "int" => " (use i8/i16/i32/i64 or u8..u64)",
                    "float" => " (use f32 or f64)",
                    "string" => " (strings are not supported in CAN payloads)",
                    _ => "",
                };
                return Err(ErrorSignals::at(ln, format!("unknown type `{ty}`{hint}")));
            }
        },
    })
}

fn slot_bytes(slot: Slot, model: &Model) -> Vec<usize> {
    match slot {
        Slot::Def(d, at) => model.defs[d].bytes.iter().map(|b| b + at).collect(),
        Slot::Frame(f) => (0..model.frames[f].size).collect(),
        Slot::Prim(p, off) => (off..off + p.size()).collect(),
    }
}

fn finish_size(size: usize, fd: bool, ln: usize, what: &str) -> Result<usize> {
    if fd {
        can::fd_len(size).ok_or_else(|| ErrorSignals::at(ln, format!("{what} needs {size} bytes, more than CAN FD allows")))
    } else if size > can::MAX_CLASSIC_LEN {
        Err(ErrorSignals::at(ln, format!("{what} needs {size} bytes; add `.fd` for CAN FD")))
    } else {
        Ok(size)
    }
}

fn resolve_fn(f: &FnDef, external: bool, ctx: &Ctx) -> Result<FnIr> {
    let ln = f.line;
    let mut names = HashSet::new();
    let mut used = [false; MAX_PAYLOAD];
    let mut needs: Vec<Need> = Vec::new();
    for p in &f.params {
        check_name(&p.name, ln)?;
        if !names.insert(p.name.as_str()) {
            return Err(ErrorSignals::at(ln, format!("duplicate parameter `{}`", p.name)));
        }
        let need = slot_need(&p.ty, ctx, ln)?;
        if let Need::Fixed(slot) = need {
            for b in slot_bytes(slot, &ctx.model) {
                if std::mem::replace(&mut used[b], true) {
                    return Err(ErrorSignals::at(ln, format!("parameter `{}` overlaps another at byte {b}", p.name)));
                }
            }
        }
        needs.push(need);
    }
    let mut cursor = 0;
    let mut params = Vec::new();
    for (p, need) in f.params.iter().zip(needs) {
        let slot = match need {
            Need::Fixed(s) => s,
            Need::Packed(k) => {
                let size = k.size(&ctx.model);
                let off = place(&used, cursor, size)
                    .ok_or_else(|| ErrorSignals::at(ln, format!("no room for parameter `{}`", p.name)))?;
                used[off..off + size].iter_mut().for_each(|u| *u = true);
                cursor = off + size;
                k.at(off)
            }
        };
        params.push(ParamIr { name: p.name.clone(), slot });
    }
    let req_end = used.iter().rposition(|u| *u).map_or(0, |i| i + 1);
    let req_size = finish_size(req_end, f.fd, ln, "request")?;

    let (ret, rsp, rsp_size) = match (&f.ret, &f.rsp) {
        (None, None) => (None, None, 0),
        (None, Some(e)) => return Err(ErrorSignals::at(e.line, "`Void` functions have no response to filter")),
        (Some(_), None) => return Err(ErrorSignals::at(ln, format!("fn `{}` returns a value and needs `.rsp`", f.name))),
        (Some(ty), Some(e)) => {
            let filter = eval_expr(e, ctx)?;
            let slot = match slot_need(ty, ctx, ln)? {
                Need::Fixed(s) => s,
                Need::Packed(k) => {
                    let mut reserved = [false; MAX_PAYLOAD];
                    filter.data_mask.iter().enumerate().for_each(|(i, m)| reserved[i] = *m != 0);
                    let off = place(&reserved, 0, k.size(&ctx.model))
                        .ok_or_else(|| ErrorSignals::at(ln, "no room for the return value"))?;
                    k.at(off)
                }
            };
            check_reserved(&filter, slot, &ctx.model, e.line, "return value")?;
            let end = slot_bytes(slot, &ctx.model).into_iter().max().map_or(0, |b| b + 1).max(filter.data_len());
            (Some(slot), Some(filter), finish_size(end, f.fd, ln, "response")?)
        }
    };
    Ok(FnIr { name: f.name.clone(), params, req_size, ret, rsp, rsp_size, fd: f.fd, big: f.big, external })
}

fn post_inc(n: &mut usize) -> usize {
    *n += 1;
    *n - 1
}

fn bit_width(v: u64) -> u32 {
    (64 - v.leading_zeros()).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_span_bytes() {
        let mut f = FieldIr { name: "x".into(), byte: 2, start: 4, width: 10, big: false, ty: FieldTy::Uint(IntTy::U16) };
        assert_eq!(
            f.chunks(),
            vec![
                Chunk { byte: 2, shift: 4, mask: 0x0F, vshift: 0 },
                Chunk { byte: 3, shift: 0, mask: 0x3F, vshift: 4 },
            ]
        );
        f.big = true;
        assert_eq!(
            f.chunks(),
            vec![
                Chunk { byte: 3, shift: 4, mask: 0x0F, vshift: 0 },
                Chunk { byte: 2, shift: 0, mask: 0x3F, vshift: 4 },
            ]
        );
    }

    #[test]
    fn big_endian_right_aligned() {
        // 12-bit value in bytes 0..2, MSB nibble in byte 0 (Motorola layout).
        let bits: Vec<usize> = (0..12).map(|i| phys_bit(0, 0, 12, true, i)).collect();
        assert_eq!(bits[..8], [8, 9, 10, 11, 12, 13, 14, 15]);
        assert_eq!(bits[8..], [0, 1, 2, 3]);
    }

    #[test]
    fn overlap() {
        let a = FilterIr { id_mask: 0x700, id_match: 0x200, ..Default::default() };
        let b = FilterIr { id_mask: 0x707, id_match: 0x201, ..Default::default() };
        let c = FilterIr { id_mask: 0x700, id_match: 0x300, ..Default::default() };
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn range_terms() {
        let r = IdRange { start: 0, width: 4, lo: 3, hi: 12 };
        assert_eq!(r.terms(), vec![(0xF, 0x3), (0xC, 0x4), (0xC, 0x8), (0xF, 0xC)]);
        let shifted = IdRange { start: 8, width: 3, lo: 4, hi: 7 };
        assert_eq!(shifted.terms(), vec![(0x400, 0x400)]);
        // Every 4-bit value is accepted by exactly the range.
        for v in 0u32..16 {
            assert_eq!(r.terms().iter().any(|&(m, x)| v & m == x), (3..=12).contains(&v));
        }
    }

    #[test]
    fn range_overlap() {
        let a = FilterIr { id_ranges: vec![IdRange { start: 0, width: 4, lo: 1, hi: 3 }], ..Default::default() };
        let b = FilterIr { id_ranges: vec![IdRange { start: 0, width: 4, lo: 4, hi: 9 }], ..Default::default() };
        let c = FilterIr { id_mask: 0xF, id_match: 3, ..Default::default() };
        assert!(!a.overlaps(&b));
        assert!(a.overlaps(&c));
        assert!(!b.overlaps(&c));
    }

    #[test]
    fn patterns() {
        assert_eq!(pattern(Val::Int(-1), 3), Ok(0b111));
        assert!(pattern(Val::Int(8), 3).is_err());
        assert_eq!(pattern(Val::Float(1.0), 32), Ok(u64::from(1.0f32.to_bits())));
    }
}
