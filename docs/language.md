Reference for the `.cp` definition language.

A `.cp` file describes the payload layout of CAN frames, the acceptance filters that identify them, and request/response functions built on top of them. `--` or `//` starts a comment. Bit 0 is the LSB.

# Files and packages

A directory is compiled as a **package**: `x.cp` becomes module/header `x` and `x.inc.cp` becomes `x_inc`. A single file can also be compiled on its own; its includes are then inlined.

`.inc.cp` files may contain `type`, `def`, `spec` and `filter`, but not `frame` or `fn`. Each item is generated once, in the include's own module, and imported by the files that include it.

Names must be usable in both Rust and C: Rust and C keywords (including `gen`, `bool`, `default`) and the names used inside generated helpers (`buf`, `len`, `m`, `v`, `rc`, `spec`) are rejected.

# Directives and includes

```text
#protocol=can        -- optional; `#ignore=` skips the file
#inc=common          -- common.inc.cp from the same directory (nesting allowed, cycles rejected)
#endian=big          -- default byte order of this file (little if omitted)
#id=standard         -- id type for this file's `.filter`/`.rsp` that don't set `.ext` (or `extended`)
```

# Types and payload layout

```text
type Connection:            -- enum
.connected 1
.disconnected 0

def {1} ConnectionStatus:   -- bit fields pinned to payload byte(s)
.connected 0 Connection     -- typed field, width = bits the type needs

def {2,3} TemperatureError: -- `[#n]` selects the byte for the following fields
[#2]
.temp0 0 bool               -- one bit
.level 1..4 u8              -- bits 1..3 (also `1..=3`, `4..`, `..` = whole byte)
[#3]
.temp8 0 bool

def {4,5} Motor:
.rpm 0..12 i16 big          -- signed (sign-extended on decode), big-endian override
.dir 4 bool
```

Every field needs a type: an enum, `bool`, `u8`..`u64` or `i8`..`i64`. It is the type of the generated Rust/C field, so it must hold the field's bits (`bool` is exactly one bit). With only a start bit (`.x 4 u8`), the field takes the type's full width. Enums are range-checked on decode.

**Byte order.** Bits are numbered LSB-first across the bytes a field spans, starting at its section byte. Those bytes are read as one little-endian number (default) or big-endian number (`big`, or `#endian=big`). With `big`, the section byte is the most significant byte. `[#0] .x 0..12 big` is the usual Motorola layout: high nibble in byte 0, low byte in byte 1. The file's byte order also applies to `.data` filter conditions and to standard-type fn parameters and return values.

# Generic defs (reusable layouts)

```text
def '3 Motor:                -- 3 bytes; [#n] counts from the def's own start
.mode 0 Mode
.rpm 4..16 u16
[#2]
.temp .. i8

frame Drives:
.size '8
.left: Motor {0}             -- start byte required in frames
.Speed {3}                   -- field name defaults to the def name (`speed`)
.right: Motor {5}

fn Set(a: u8, motor: Motor): Void   -- packed into the next free bytes, like standard types
```

A generic def can be declared in any file; `.inc.cp` files are the natural place to share them. Placement is in whole bytes. All checks apply to the placed bytes, so if two members end up using the same byte (for example after a def grows), the compiler reports it. A def's byte order is fixed where it is declared. Fixed defs (`def {..}`) cannot be given a start byte, and generic defs in frames must have one.

# Spec (constants)

```text
spec:
.node_id 0x02 u8            -- `.name <value> <type>`; type required: bool u8..u64 i8..i64 f32 f64
.gain 1.5 f32
```

Use a spec value as `spec.node_id`.

# Filters

A filter matches raw bits of the CAN id and/or payload. Filters are independent of each other and can take parameters (standard types or enums):

```text
filter Node(node: NodeId):
.id 8..11 node              -- CAN id bits 8..10 (29-bit range)

filter Tag(tag: u8):
.data {0} 0..4 tag          -- payload byte 0, bits 0..3

filter Extended:
.ext                        -- id type: `.ext` / `.ext true` = 29-bit, `.ext false` = 11-bit

filter NodeGroup(lo: u8, hi: u8):
.id 8..11 lo..=hi           -- id range: `lo..=hi`, `lo..hi`, `lo..` (>=), `..=hi` (<=)
```

Values can be literals, parameters, `spec.x` or `Type.Variant`. A filter that doesn't set `.ext` (and isn't covered by `#id=`) accepts both id types. Filters for standard ids may only check id bits 0..10.

Ranges are only allowed on `.id` and compare the raw unsigned bits. Ranges on the same bits combined with `|` must have values in common; empty ranges, ranges that accept every value, and ranges overlapping other id conditions are errors. A range usually needs several hardware filter slots: `ID_TERMS` lists the (mask, match) pairs that together accept exactly the filter's ids (one pair when there is no range, or the range is an aligned block like `4..=7`).

Filters are combined with `|` at the point of use:

```text
frame CoprocStatus:
.size '4
.filter Node(spec.node_id) | Kind(Status) | Method(0)
.ConnectionStatus
.TemperatureError
```

The compiler rejects:
- combined filters that set a bit to two different values
- two filters in a package that could match the same frame
- payload fields that overlap bits a filter checks (the encoders set those bits automatically)

# Functions

```text
fn GetTemperature(): TemperatureError
.rsp Node(spec.node_id) | Kind(GetResponse) | Method(1)

fn Configure(motor: Motor, gain: f32, offset: i16): Void -- Void: no response
.fd                                                     -- allow payloads over 8 bytes
```

The caller builds and sends the request with a CAN id of their choosing; `.rsp` identifies the reply.

Parameters and return values can be standard types, `def`s or `frame`s:
- Defs and frames keep their fixed bytes.
- Standard types are packed in the file's byte order (little-endian by default) into the next free bytes, skipping bytes that a `.rsp` filter checks.

See [`generated_rust`](crate::guide::generated_rust) and [`generated_c`](crate::guide::generated_c) for what each construct produces.
