Command-line usage of the `proto_c` binary.

```text
Usage: proto_c [OPTIONS] <INPUT>...

Arguments:
  <INPUT>...           a directory is compiled as a package: <name>/mod.rs plus one
                       .rs/.h per file (x.cp -> x, x.inc.cp -> x_inc); a single
                       file is compiled self-contained with its #inc= files inlined

Options:
  -o, --out <DIR>      output directory [default: generated]
      --rust-out <DIR> directory for .rs files [default: <out>/rust]
      --c-out <DIR>    directory for .h files [default: <out>/c]
  -l, --lang <LANG>    rust | c | all [default: all]
      --check          only parse and validate, write nothing
  -h, --help           print this help
```

# Examples

```sh
cargo install --git https://github.com/rsa-johan/can-proto   # installs `proto_c`

proto_c protos/examples -o generated          # package -> generated/{rust,c}/examples/
proto_c protos/examples/dummy.cp -o generated # one self-contained file (includes inlined)
proto_c --check protos/examples               # validate only
proto_c --lang c --c-out firmware/inc protos/examples
```

Several inputs can be given at once; each is compiled independently. Inputs whose header is `#ignore=` are skipped. The exit code is non-zero if any input fails, and every error is printed with its file and line.

The same functionality is available as a library, see [`compile`](crate::compile).
