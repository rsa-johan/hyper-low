//! Compiler for `.cp` CAN definitions.
//!
//! A `.cp` file describes CAN payload layouts, acceptance filters and request/response functions.
//! `can-proto` turns it into a dependency-free Rust module and a C99 header with typed payload structs,
//! encoders/decoders, filter constants for hardware acceptance filters, and response matchers.
//!
//! It can be used through the `proto_c` command-line tool ([`guide::cli`]) or as a library, for
//! example from `build.rs`.
//!
//! # Example
//!
//! ```
//! let src = "\
//! filter Node(n: u8):
//! .id 8..11 n
//!
//! def {0} Status:
//! .ready 0 bool
//! .level 1..4 u8
//!
//! frame Heartbeat:
//! .size '1
//! .filter Node(2)
//! .Status
//! ";
//! let generated = can_proto::compile_str(src, "heartbeat", "heartbeat.cp")?;
//! assert!(generated.rust.contains("pub struct Heartbeat"));
//! assert!(generated.c.contains("Heartbeat_encode"));
//! # Ok::<(), can_proto::ErrorSignals>(())
//! ```
//!
//! # Guide
//!
//! - [`guide::language`]: the `.cp` language
//! - [`guide::generated_rust`] and [`guide::generated_c`]: what the generated code looks like
//! - [`guide::cli`]: command-line usage
//! - [`guide::architecture`]: how the compiler is structured

#![warn(missing_docs)]

mod protocols;
mod common;
mod tools;
mod error;
mod init;
mod codegen;

use std::path::Path;

use codegen::{c, rust, Layout};
use init::{Kind, Unit};
use tools::{
    parser::parse,
    resolve::{check_overlaps, resolve, FilterIr, Model},
};

pub use crate::common::PROTO_EXT;
pub use crate::error::ErrorSignals;

/// User guide and references. These modules contain documentation only.
pub mod guide {
    #[doc = include_str!("../docs/language.md")]
    pub mod language {}
    #[doc = include_str!("../docs/generated-rust.md")]
    pub mod generated_rust {}
    #[doc = include_str!("../docs/generated-c.md")]
    pub mod generated_c {}
    #[doc = include_str!("../docs/cli.md")]
    pub mod cli {}
    #[doc = include_str!("../docs/architecture.md")]
    pub mod architecture {}
}

type Result<T> = std::result::Result<T, ErrorSignals>;

/// Generated sources for one `.cp` / `.inc.cp` file.
#[derive(Debug, Clone)]
pub struct Generated {
    /// Rust module source, see [`guide::generated_rust`].
    pub rust: String,
    /// C99 header source, see [`guide::generated_c`].
    pub c: String,
}

/// A compiled proto directory.
///
/// Rust: `<name>/mod.rs` + `<name>/<module>.rs`. C: `<name>/<module>.h`.
/// `x.cp` becomes module `x`, `x.inc.cp` becomes `x_inc`.
#[derive(Debug, Clone)]
pub struct Package {
    /// Package name, taken from the directory name.
    pub name: String,
    /// Shared prelude and module declarations.
    pub mod_rs: String,
    /// (module name, output) for every file.
    pub units: Vec<(String, Generated)>,
    /// Files skipped by an `#ignore=` header.
    pub skipped: Vec<String>,
}

/// Result of [`compile`].
#[derive(Debug, Clone)]
pub enum Output {
    /// A directory compiled as a package.
    Package(Package),
    /// A single self-contained file (includes inlined).
    File {
        /// Module name: `x` for `x.cp`, `x_inc` for `x.inc.cp`.
        module: String,
        /// Generated sources.
        generated: Generated,
    },
}

/// Entry point: compiles a directory as a package, or a single `.cp` / `.inc.cp` file.
///
/// # Errors
///
/// Returns [`ErrorSignals::NoError`] if a single file is skipped by its `#ignore=` header. Other
/// errors are [`ErrorSignals::Generic`] messages that start with the file name and, for parse and
/// validation errors, include the line. In a package the errors of all files are reported together.
///
/// # Example
///
/// ```no_run
/// use can_proto::{compile, Output};
///
/// match compile("protos/examples".as_ref())? {
///     Output::Package(pkg) => {
///         for (module, generated) in &pkg.units {
///             std::fs::write(format!("{module}.rs"), &generated.rust).unwrap();
///         }
///     }
///     Output::File { module, generated } => println!("{module}: {} bytes of C", generated.c.len()),
/// }
/// # Ok::<(), can_proto::ErrorSignals>(())
/// ```
pub fn compile(path: &Path) -> Result<Output> {
    let inputs = init::load(path)?;
    Ok(match inputs.kind {
        Kind::Package { name, skipped, errors } => Output::Package(build_package(name, skipped, errors, &inputs.units)?),
        Kind::File(idx) => Output::File {
            module: inputs.units[idx].module.clone(),
            generated: build_file(&inputs.units, idx)?,
        },
    })
}

/// [`compile`] restricted to directories.
///
/// # Errors
///
/// As [`compile`], and an error if `dir` is a file.
pub fn compile_package(dir: &Path) -> Result<Package> {
    match compile(dir)? {
        Output::Package(pkg) => Ok(pkg),
        Output::File { .. } => Err(ErrorSignals::NotDir.in_file(&dir.display().to_string())),
    }
}

/// [`compile`] restricted to files.
///
/// # Errors
///
/// As [`compile`], and an error if `path` is a directory.
pub fn compile_file(path: &Path) -> Result<Generated> {
    match compile(path)? {
        Output::File { generated, .. } => Ok(generated),
        Output::Package(_) => Err(ErrorSignals::Generic(format!("{}: is a directory", path.display()))),
    }
}

/// Compiles `.cp` source text without includes. `unit` names the C include guard and spec macros,
/// `source` is quoted in the banner.
///
/// # Errors
///
/// Parse and validation errors as [`ErrorSignals::At`] with the line, and an error if the source
/// uses `#inc=`.
///
/// # Example
///
/// ```
/// let err = can_proto::compile_str("def {0} A:\n.x 0\n", "t", "t.cp").unwrap_err();
/// assert!(matches!(err, can_proto::ErrorSignals::At { line: 2, .. }));
/// ```
pub fn compile_str(src: &str, unit: &str, source: &str) -> Result<Generated> {
    let proto = parse(src)?;
    if let Some((_, ln)) = proto.includes.first() {
        return Err(ErrorSignals::at(*ln, "`#inc=` needs a file on disk; use `compile`"));
    }
    let model = resolve(&[(&proto, false)])?;
    check_overlaps(&model.filter_uses())?;
    Ok(Generated {
        rust: rust::generate(&model, source, &Layout::Standalone),
        c: c::generate(&model, source, unit, &Layout::Standalone),
    })
}

/// Resolves a unit with its includes (inlined unless `deps_external`) and checks filter overlaps.
fn resolve_unit(units: &[Unit], idx: usize, deps_external: bool) -> Result<Model> {
    let unit = &units[idx];
    let mut scope: Vec<_> = unit.deps.iter().map(|&d| (&units[d].proto, deps_external)).collect();
    scope.push((&unit.proto, false));
    let label = unit.label();
    let model = resolve(&scope).map_err(|e| e.in_file(&label))?;
    check_overlaps(&model.filter_uses()).map_err(|e| e.in_file(&label))?;
    Ok(model)
}

fn build_file(units: &[Unit], idx: usize) -> Result<Generated> {
    for &dep in &units[idx].deps {
        resolve_unit(units, dep, false)?;
    }
    let model = resolve_unit(units, idx, false)?;
    let unit = &units[idx];
    Ok(Generated {
        rust: rust::generate(&model, unit.file_name(), &Layout::Standalone),
        c: c::generate(&model, unit.file_name(), &unit.module, &Layout::Standalone),
    })
}

/// Load errors and errors of all units are reported together; filters must not overlap across
/// the package.
fn build_package(name: String, skipped: Vec<String>, mut errors: Vec<String>, units: &[Unit]) -> Result<Package> {
    let results: Vec<Option<Result<Model>>> =
        (0..units.len()).map(|idx| units[idx].linked.then(|| resolve_unit(units, idx, true))).collect();
    let failed = |idx: usize| matches!(results[idx], Some(Err(_)));
    let mut out = Vec::new();
    let mut uses: Vec<(String, FilterIr)> = Vec::new();
    for (unit, result) in units.iter().zip(&results) {
        let model = match result {
            None => continue,
            Some(Ok(m)) => m,
            // An include's error is already reported against the include itself.
            Some(Err(_)) if unit.deps.iter().any(|&d| failed(d)) => continue,
            Some(Err(e)) => {
                errors.push(e.to_string());
                continue;
            }
        };
        let rust_uses: Vec<String> = unit.deps.iter().map(|&d| units[d].module.clone()).collect();
        let c_includes = unit.include_modules();
        let layout = Layout::Unit { pkg: &name, rust_uses: &rust_uses, c_includes: &c_includes };
        let file = unit.file_name();
        uses.extend(model.filter_uses().into_iter().map(|(l, f)| (format!("{file}: {l}"), f.clone())));
        out.push((
            unit.module.clone(),
            Generated { rust: rust::generate(&model, file, &layout), c: c::generate(&model, file, &unit.module, &layout) },
        ));
    }
    if !errors.is_empty() {
        return Err(ErrorSignals::Generic(errors.join("\n")));
    }
    let refs: Vec<(String, &FilterIr)> = uses.iter().map(|(l, f)| (l.clone(), f)).collect();
    check_overlaps(&refs)?;

    let modules: Vec<String> = out.iter().map(|(m, _)| m.clone()).collect();
    Ok(Package { mod_rs: rust::package_mod(&name, &modules), name, units: out, skipped })
}
