//! Raw syntax tree produced by the parser, before name resolution and validation.

#[derive(Debug, Clone, Default)]
pub struct Proto {
    /// `#inc=<name>` directives: (include name, line).
    pub includes: Vec<(String, usize)>,
    /// `#endian=big` file default; applies to fields, data conditions and fn payloads.
    pub big_endian: bool,
    /// `#id=standard|extended` file default for filter expressions that leave `.ext` open.
    pub extended_id: Option<bool>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Type(TypeDef),
    Def(Def),
    Frame(Frame),
    Spec(Spec),
    Filter(FilterDef),
    Fn(FnDef),
}

impl Item {
    /// Name in the shared type namespace; `spec` blocks have none.
    pub fn name(&self) -> Option<&str> {
        match self {
            Item::Type(t) => Some(&t.name),
            Item::Def(d) => Some(&d.name),
            Item::Frame(f) => Some(&f.name),
            Item::Filter(f) => Some(&f.name),
            Item::Fn(f) => Some(&f.name),
            Item::Spec(_) => None,
        }
    }

    pub fn line(&self) -> usize {
        match self {
            Item::Type(t) => t.line,
            Item::Def(d) => d.line,
            Item::Frame(f) => f.line,
            Item::Spec(s) => s.line,
            Item::Filter(f) => f.line,
            Item::Fn(f) => f.line,
        }
    }
}

/// `type Name:` followed by `.variant <value>` lines.
#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub line: usize,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub name: String,
    pub value: u64,
    pub line: usize,
}

/// `def {b0,b1,..} Name:` (fixed bytes) or `def 'N Name:` (generic, bytes `0..N` placed at use)
/// followed by optional `[#b]` sections and `.field <bits> [Type]` lines.
#[derive(Debug, Clone)]
pub struct Def {
    pub name: String,
    pub line: usize,
    pub bytes: Vec<usize>,
    pub fields: Vec<Field>,
    pub generic: bool,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub line: usize,
    /// Absolute payload byte the bit spec is relative to.
    pub byte: usize,
    pub bits: BitSpec,
    /// Enum or standard type (required).
    pub ty: String,
    /// Big-endian: the section byte is the most significant byte.
    pub big: bool,
}

/// Bit positions; bit 0 is the LSB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BitSpec {
    /// `..` — the whole byte (or the whole id).
    Full,
    /// `n` — a single bit, or the start bit of a typed field.
    Bit(u32),
    /// `n..` — from bit n to the end of the byte (or id).
    From(u32),
    /// `a..b` (exclusive) / `a..=b` (inclusive), stored as half-open.
    Range(u32, u32),
}

/// `frame Name:` followed by `.size 'N`, `.fd [bool]`, `.filter <expr>` and `.[field:] Def [{n}]` lines.
#[derive(Debug, Clone)]
pub struct Frame {
    pub name: String,
    pub line: usize,
    pub size: Option<usize>,
    pub fd: bool,
    pub filter: Option<FilterExpr>,
    pub members: Vec<Member>,
}

/// `.[field:] Def [{n}]` inside a frame.
#[derive(Debug, Clone)]
pub struct Member {
    pub def: String,
    /// Struct field name; defaults to the def name in snake_case.
    pub field: Option<String>,
    /// Start byte; required for generic defs, rejected for fixed ones.
    pub at: Option<usize>,
    pub line: usize,
}

/// `spec:` followed by `.name <literal> <std type>` lines.
#[derive(Debug, Clone)]
pub struct Spec {
    pub line: usize,
    pub entries: Vec<SpecEntry>,
}

#[derive(Debug, Clone)]
pub struct SpecEntry {
    pub name: String,
    pub line: usize,
    pub value: Literal,
    pub ty: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Literal {
    Int(i128),
    Float(f64),
    Bool(bool),
}

/// A value in a filter condition or filter call argument.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Lit(Literal),
    /// `spec.name`
    Spec(String),
    /// `Type.Variant`
    Variant(String, String),
    /// A filter parameter, or a bare variant when the expected type is an enum.
    Name(String),
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: String,
}

/// `filter Name[(p: Type, ..)]:` followed by `.id <bits> <value>` / `.data {b} <bits> <value>` lines.
#[derive(Debug, Clone)]
pub struct FilterDef {
    pub name: String,
    pub line: usize,
    pub params: Vec<Param>,
    pub conds: Vec<Cond>,
}

#[derive(Debug, Clone)]
pub struct Cond {
    pub line: usize,
    pub target: Target,
    pub bits: BitSpec,
    pub value: Match,
    /// Byte order for data conditions spanning several bytes.
    pub big: bool,
}

/// Value side of a filter condition.
#[derive(Debug, Clone, PartialEq)]
pub enum Match {
    Eq(Value),
    /// `lo..hi`, `lo..=hi`, `lo..`, `..=hi` (unsigned compare; id conditions only).
    Range { lo: Option<Value>, hi: Option<Value>, inclusive: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    Id,
    /// Data byte the bit spec is relative to.
    Data(usize),
    /// The id type flag (IDE): `true` = 29-bit extended, `false` = 11-bit standard.
    Ext,
}

/// `A(args) | B | C(args)` at a use site.
#[derive(Debug, Clone)]
pub struct FilterExpr {
    pub line: usize,
    pub calls: Vec<FilterCall>,
    /// File's `#id=` default, applied when no call sets `.ext`.
    pub ext_default: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct FilterCall {
    pub name: String,
    pub args: Vec<Value>,
}

/// `fn Name(p: Type, ..): Ret` followed by `.rsp <expr>` / `.fd` lines.
#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub line: usize,
    pub params: Vec<Param>,
    /// `None` for `Void`.
    pub ret: Option<String>,
    pub rsp: Option<FilterExpr>,
    pub fd: bool,
    /// Byte order of standard-type parameters and return values.
    pub big: bool,
}
