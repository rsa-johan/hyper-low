# hyper_low

Compiler for `.cp` definitions of low-level protocols, aiming to be one common definition format across them. CAN is currently supported. Generates Rust modules and C99 headers with typed payload structs, encode/decode functions, acceptance filters and RPC-style request/response helpers.

**Documentation:** <https://rsa-johan.github.io/hyper-low/> (language reference, generated-code reference, CLI and library API). Build it locally with `cargo doc --no-deps --open`.

## Usage

```sh
cargo run -- protos/examples -o generated          # package -> generated/{rust,c}/examples/
cargo run -- protos/examples/dummy.cp -o generated  # one self-contained file (includes inlined)
cargo run -- --check protos/examples                # validate only
cargo run -- --lang c --c-out firmware/inc protos/examples
```

A directory is compiled as a **package**. `x.cp` becomes module/header `x` and `x.inc.cp` becomes `x_inc`. Rust also gets a `mod.rs` holding the shared prelude.

- **Rust:** `mod examples;`
- **C:** `#include "examples/dummy.h"`. Headers use only `static inline` functions, so no `.c` file is needed.

The compiler is also a library for use from `build.rs` or a proc-macro. `hyper_low::compile(path)` takes a directory or a file and returns `Output::Package` or `Output::File`. `compile_package`, `compile_file` and `compile_str` are narrower variants.

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

See [`protos/examples`](protos/examples) for a complete package and the [language reference](https://rsa-johan.github.io/hyper-low/hyper_low/guide/language/index.html) for the full syntax.

## License

Licensed under of [Apache License, Version 2.0](LICENSE-APACHE).
