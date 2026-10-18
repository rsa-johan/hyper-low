Reference for the generated C99 headers.

Headers contain only types, macros and `static inline` functions, so no `.c` file needs to be compiled or linked. They compile cleanly with `gcc -std=c99 -Wall -Wextra -Wpedantic -Wconversion -Werror` and are wrapped in `extern "C"` for C++.

# Layout

- **Package** (directory input): one `<pkg>/<module>.h` per file. A header `#include`s the headers of its `#inc=` files by plain name (`#include "common_inc.h"`), so keep a package's headers together in one directory. Include guards are `PROTO_C_<PKG>_<MODULE>_H`.
- **Single file**: one self-contained header with all included items inlined.

```c
#include "examples/dummy.h"
```

# Status codes

Every `*_encode*`/`*_decode*` function returns `int`:

| Macro | Value | Meaning |
|---|---|---|
| `PROTO_C_OK` | `0` | success |
| `PROTO_C_ERR_SHORT_BUFFER` | `-1` | `len` is shorter than the layout requires |
| `PROTO_C_ERR_INVALID_ENUM` | `-2` | a field holds a value that is not a variant of its enum |

# Items per construct

Names: types, structs and functions keep the `.cp` name (`Name_encode`); macros and enum constants use `UPPER_SNAKE_CASE` (`NAME_SIZE`, `TYPE_VARIANT`). In the tables below `X` is the item name and `X_` its `UPPER_SNAKE_CASE` macro prefix.

## `spec:`

`#define <UNIT>_SPEC_<NAME> ((type)value)`, where `<UNIT>` is the module name (for example `DUMMY_SPEC_NODE_ID`).

## `type T`

- `typedef enum T { T_VARIANT = value, ... } T;`
- `bool T_is_valid(uint64_t v)`

## `def {..} D` (fixed)

| Item | Meaning |
|---|---|
| `D_MIN_LEN` | minimum payload length that contains the def |
| `void D_write(const D *m, uint8_t *buf)` | writes its fields without bounds checks, leaving other bits untouched |
| `int D_encode(const D *m, uint8_t *buf, size_t len)` | checked `D_write` |
| `int D_decode(D *m, const uint8_t *buf, size_t len)` | reads its fields; enums are range-checked |

## `def 'N D` (generic)

| Item | Meaning |
|---|---|
| `D_SIZE` | `N` |
| `void D_write_at(const D *m, uint8_t *buf, size_t off)` | unchecked write at byte offset `off` |
| `int D_encode_at(const D *m, uint8_t *buf, size_t len, size_t off)` | checked write at `off` |
| `int D_decode_at(D *m, const uint8_t *buf, size_t len, size_t off)` | read from `off` |

## `frame F`

| Item | Meaning |
|---|---|
| `F_SIZE`, `F_IS_FD` | payload length (`.size`) and whether it needs CAN FD |
| `void F_write(const F *m, uint8_t *buf)` | writes members and the filter's `.data` bits; no bounds check, no zeroing |
| `int F_encode(const F *m, uint8_t *buf, size_t len)` | zeroes `buf[0..F_SIZE)` and writes the payload |
| `int F_decode(F *m, const uint8_t *buf, size_t len)` | decodes every member |

With a `.filter`:

| Item | Meaning |
|---|---|
| `F_FILTER_ID_MASK`, `F_FILTER_ID_MATCH` | exact id bits: `(id & MASK) == MATCH` (id ranges excluded) |
| `F_FILTER_ID_TERMS`, `F_FILTER_ID_TERM_COUNT` | hardware acceptance filter as `{mask, match}` pairs, one per slot; together exact, ranges included |
| `F_FILTER_EXTENDED` | required id type: `1` 29-bit, `0` 11-bit; not defined if either is accepted |
| `bool F_matches(uint32_t id, bool extended, const uint8_t *data, size_t len)` | true if a received frame passes the whole filter |

```c
static const uint32_t terms[][2] = COPROC_STATUS_FILTER_ID_TERMS;
```

## `fn X(..): R`

| Item | Meaning |
|---|---|
| `X_REQUEST_SIZE`, `X_IS_FD` | request payload length and CAN FD flag |
| `XRequest`, `XRequest_encode`, `XRequest_decode` | request struct and codec; only when the function has parameters |

If the function returns a value and has `.rsp`:

| Item | Meaning |
|---|---|
| `X_RSP_SIZE` | response payload length |
| `X_RSP_ID_MASK`, `X_RSP_ID_MATCH`, `X_RSP_ID_TERMS`, `X_RSP_ID_TERM_COUNT`, `X_RSP_EXTENDED` | as the `F_FILTER_*` macros of a frame |
| `bool X_is_response(uint32_t id, bool extended, const uint8_t *data, size_t len)` | true if a received frame is this reply |
| `int X_decode_response(R *out, const uint8_t *data, size_t len)` | decodes the return value |
| `int X_encode_response(const R *value, uint8_t *buf, size_t len)` | for the responder: zeroes `buf[0..X_RSP_SIZE)`, writes `value` and the filter's `.data` bits; standard types are passed by value |

# Example

```c
uint8_t buf[8];
SetFanSpeedRequest req = { 1200 };
SetFanSpeedRequest_encode(&req, buf, sizeof buf);   /* send with your id */

TemperatureError temp;
if (GetTemperature_is_response(id, extended, data, len)
    && GetTemperature_decode_response(&temp, data, len) == PROTO_C_OK) {
    /* use temp */
}
```
