Reference for the generated Rust code.

The output has no dependencies and only uses `core`, so it works in `no_std` crates.

# Layout

- **Package** (directory input): `<pkg>/mod.rs` holds the shared prelude and one `pub mod` per file. Each module starts with `use super::*;` and `use super::<include>::*;` for its `#inc=` files, so included items are shared, not duplicated.
- **Single file**: one self-contained `.rs` file with the prelude and all included items inlined.

Using a package that was written into the source tree (for example with `hyper_low protos/examples -o src/proto --lang rust`):

```rust,ignore
#[path = "proto/rust/examples/mod.rs"]
mod examples;

use examples::dummy::*;
```

A single file can also be generated into `OUT_DIR` from `build.rs` and pulled in with `include!`:

```rust,ignore
// build.rs (needs `hyper_low` as a build-dependency)
fn main() {
    println!("cargo::rerun-if-changed=protos");
    let g = hyper_low::compile_file("protos/dummy.cp".as_ref()).unwrap();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("dummy.rs"), g.rust).unwrap();
}

// src/lib.rs
pub mod dummy {
    include!(concat!(env!("OUT_DIR"), "/dummy.rs"));
}
```

# Prelude

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// The buffer is shorter than the payload layout requires.
    BufferTooShort { expected: usize, actual: usize },
    /// A field holds a value that is not a variant of its enum type.
    InvalidEnum { field: &'static str, value: u64 },
}
```

`CodecError` implements `Display`. Every fallible `encode*`/`decode*` returns `Result<_, CodecError>`.

# Items per construct

Names: types and structs keep the `.cp` name, enum variants become `PascalCase`, struct fields become `snake_case`, and spec constants become `UPPER_SNAKE_CASE`.

| `.cp` | Rust |
|---|---|
| `spec:` | `pub mod spec { pub const NODE_ID: u8 = 2; }` |
| `type T:` | `#[repr(uN)] enum T` |
| `def {..} D:` | `struct D` with one field per `.cp` field |
| `def 'N D:` | `struct D`, placed at a byte offset where used |
| `frame F:` | `struct F` with one field per member |
| `fn X(..): R` | `struct XRequest` (parameters) and, with a return type and `.rsp`, `struct X` (response side) |

## `type T`

- `from_raw(u64) -> Option<T>`
- `Default` (the variant with value 0, otherwise the first one), `From<T> for uN` and `TryFrom<uN> for T` (the error is the raw value).

## `def {..} D` (fixed)

| Item | Meaning |
|---|---|
| `BYTES: &[usize]` | payload bytes the def occupies |
| `MIN_LEN: usize` | minimum payload length that contains it |
| `encode(&self, buf) -> Result<(), CodecError>` | writes its fields, leaving other bits untouched |
| `decode(buf) -> Result<D, CodecError>` | reads its fields; enums are range-checked |

## `def 'N D` (generic)

| Item | Meaning |
|---|---|
| `SIZE: usize` | `N` |
| `encode_at(&self, buf, off) -> Result<(), CodecError>` | writes at byte offset `off` |
| `decode_at(buf, off) -> Result<D, CodecError>` | reads from byte offset `off` |

## `frame F`

| Item | Meaning |
|---|---|
| `SIZE: usize`, `FD: bool` | payload length (`.size`) and whether it needs CAN FD |
| `to_bytes(&self) -> [u8; SIZE]` | complete payload, including the filter's `.data` bits |
| `encode(&self, buf) -> Result<usize, CodecError>` | zeroes `buf[..SIZE]`, writes the payload, returns `SIZE` |
| `decode(buf) -> Result<F, CodecError>` | decodes every member |

With a `.filter`:

| Item | Meaning |
|---|---|
| `FILTER_ID_MASK`, `FILTER_ID_MATCH: u32` | exact id bits: `(id & MASK) == MATCH` (id ranges excluded) |
| `FILTER_ID_TERMS: &[(u32, u32)]` | hardware acceptance filter, one `(mask, match)` pair per slot; together exact, ranges included |
| `FILTER_EXTENDED: Option<bool>` | required id type: `Some(true)` 29-bit, `Some(false)` 11-bit, `None` either |
| `matches(id, extended, data) -> bool` | true if a received frame passes the whole filter (id, ranges, id type, `.data`) |

## `fn X(..): R`

`XRequest` has one field per parameter, plus `SIZE`, `FD`, `to_bytes`, `encode` and `decode` like a frame. The caller sends it with an id of their choosing.

If the function returns a value and has `.rsp`, the unit struct `X` describes the reply:

| Item | Meaning |
|---|---|
| `RSP_SIZE: usize`, `FD: bool` | response payload length and CAN FD flag |
| `RSP_ID_MASK`, `RSP_ID_MATCH`, `RSP_ID_TERMS`, `RSP_EXTENDED` | as the `FILTER_*` constants of a frame |
| `is_response(id, extended, data) -> bool` | true if a received frame is this reply |
| `decode_response(data) -> Result<R, CodecError>` | decodes the return value |
| `encode_response(value, buf) -> Result<usize, CodecError>` | for the responder: zeroes `buf[..RSP_SIZE]`, writes `value` and the filter's `.data` bits |

# Example

```rust,ignore
let bytes = SetFanSpeedRequest { value: 1200 }.to_bytes();           // send with your id
if GetTemperature::is_response(rx.id, rx.extended, rx.data) {
    let t: TemperatureError = GetTemperature::decode_response(rx.data)?;
}
for &(mask, id) in GetTemperature::RSP_ID_TERMS { /* one controller filter slot each */ }
```
