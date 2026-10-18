---
title: Hyper_Low
---

# hyper_low

One definition format for low-level protocols. Describe payload layouts, acceptance filters and request/response functions once in a `.cp` file, and generate dependency-free Rust modules and C99 headers from it. CAN is currently supported.

## Features

- Typed payload structs with bit-exact encode/decode, little- or big-endian fields, signed values and range-checked enums
- Reusable layouts (generic defs) placed at any byte offset
- Acceptance filters on id and payload bits, with ready-made (mask, match) pairs for hardware filter slots
- Request/response helpers: build requests, recognize and decode replies
- Compile-time checks: overlapping fields, conflicting filters, filters that could match the same frame
- Rust output is `no_std` friendly; C output is header-only (`static inline`), clean under `-std=c99 -Wall -Wextra -Wpedantic -Wconversion`

## Example

```text
#protocol=can
#inc=common

def {1} ConnectionStatus:
.connected 0 Connection

frame CoprocStatus:
.size '4
.filter Node(spec.node_id) | Kind(Status) | Method(0)
.ConnectionStatus

fn GetTemperature(): TemperatureError
.rsp Node(spec.node_id) | Kind(GetResponse) | Method(1)
```

```sh
hyper_low protos/examples -o generated   # -> generated/rust/examples/, generated/c/examples/
```

## Get started

```sh
cargo install --git https://github.com/rsa-johan/hyper-low
```

## Documentation

- [Language reference](language.md)
- [Generated Rust code](generated-rust.md)
- [Generated C code](generated-c.md)
- [Command line](cli.md)
- [Architecture](architecture.md)

Source, issues and examples: [github.com/rsa-johan/hyper-low](https://github.com/rsa-johan/hyper-low)

Licensed under MIT or Apache-2.0.
