How the compiler is structured, for contributors.

```text
init::load ──► tools::parser ──► tools::resolve ──► codegen::{rust, c}
 (files,        (lines → AST)     (names, checks,     (IR → source text)
  #inc= graph)                     layout → IR)
```

| Module | Role |
|---|---|
| `src/init.rs` | Loads a package or file, parses each file once, links `#inc=` closures (missing files, cycles, ignored includes) |
| `src/tools/parser.rs` | Line parser that builds the AST (`tools/ast.rs`) |
| `src/tools/resolve.rs` | Name resolution, validation, filter evaluation and payload layout; produces the IR (`Model`) |
| `src/protocols/` | Header detection (`#protocol=`) and CAN payload limits |
| `src/codegen/rust.rs`, `src/codegen/c.rs` | Code generators |
| `src/lib.rs` | Public API; runs the stages and checks filter overlaps across a package |
| `src/main.rs` | CLI |

In a package, every unit is resolved with its includes marked as external, so each item is generated only once, in its own module. A single file is resolved with its includes inlined.

# Testing

`cargo test` runs the unit tests, `tests/compile.rs` and the doctests. Changes to the generators should also be checked by compiling their output: Rust with `#![deny(warnings)]`, C with `gcc -std=c99 -Wall -Wextra -Wpedantic -Wconversion -Werror`.
