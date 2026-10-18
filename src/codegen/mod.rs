pub mod c;
pub mod rust;

/// Where a generated file sits.
pub enum Layout<'a> {
    /// Self-contained output (includes inlined).
    Standalone,
    /// Member of a package: shared prelude in the package root, includes in sibling modules/headers.
    Unit {
        pkg: &'a str,
        /// Every include in the transitive closure (Rust needs explicit imports).
        rust_uses: &'a [String],
        /// Direct includes only (C includes are transitive).
        c_includes: &'a [String],
    },
}
