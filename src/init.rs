//! Entry stage: turns an input path into parsed units with their include closures.
//!
//! A directory is a package (every `.cp` / `.inc.cp` file is a unit); a file is compiled on its
//! own with its `#inc=` files loaded from the same directory. Each file is read and parsed once.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    common::PROTO_EXT,
    error::ErrorSignals,
    tools::{
        ast::{Item, Proto},
        parser::parse,
        syntax::{is_ident, is_reserved},
    },
};

type Result<T> = std::result::Result<T, ErrorSignals>;

pub const INC_EXT: &str = ".inc.cp";

/// One parsed `.cp` / `.inc.cp` file.
pub struct Unit {
    pub path: PathBuf,
    /// Rust module / C header name: `x.cp` -> `x`, `x.inc.cp` -> `x_inc`.
    pub module: String,
    pub proto: Proto,
    /// Transitive includes as indices into `Inputs::units`, dependencies first.
    pub deps: Vec<usize>,
    /// Includes loaded and linked; units with load errors are reported but not compiled.
    pub linked: bool,
}

impl Unit {
    pub fn label(&self) -> String {
        self.path.display().to_string()
    }

    pub fn file_name(&self) -> &str {
        file_name(&self.path)
    }

    /// Direct includes as module names.
    pub fn include_modules(&self) -> Vec<String> {
        self.proto.includes.iter().map(|(n, _)| format!("{n}_inc")).collect()
    }
}

pub enum Kind {
    /// Every file of a directory; includes are generated as sibling modules.
    Package { name: String, skipped: Vec<String>, errors: Vec<String> },
    /// One file (index into `units`); its includes are inlined.
    File(usize),
}

pub struct Inputs {
    pub kind: Kind,
    pub units: Vec<Unit>,
}

/// Loads a directory as a package or a single file.
pub fn load(path: &Path) -> Result<Inputs> {
    if path.is_dir() {
        load_package(path)
    } else if path.is_file() {
        load_file(path)
    } else {
        Err(ErrorSignals::Generic(format!("{}: no such file or directory", path.display())))
    }
}

fn load_package(dir: &Path) -> Result<Inputs> {
    let label = dir.display().to_string();
    let name = dir
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().and_then(|s| s.to_str()).map(str::to_string))
        .ok_or_else(|| ErrorSignals::Generic(format!("{label}: cannot determine package name")))?;
    let name = module_name(name, "package")?;

    let mut loader = Loader { dir, units: Vec::new(), by_module: HashMap::new() };
    let (mut skipped, mut errors) = (Vec::new(), Vec::new());
    for path in collect_dir(dir).map_err(|e| e.in_file(&label))? {
        let file = file_name(&path).to_string();
        match loader.add(path) {
            Ok(Some(_)) => {}
            Ok(None) => skipped.push(file),
            Err(e) => push_unique(&mut errors, e),
        }
    }
    for idx in 0..loader.units.len() {
        if let Err(e) = loader.link(idx) {
            push_unique(&mut errors, e);
        }
    }
    Ok(Inputs { kind: Kind::Package { name, skipped, errors }, units: loader.units })
}

fn load_file(path: &Path) -> Result<Inputs> {
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut loader = Loader { dir, units: Vec::new(), by_module: HashMap::new() };
    let idx = loader.add(path.to_path_buf())?.ok_or(ErrorSignals::NoError)?;
    loader.link(idx)?;
    // Includes are validated on their own scope before being inlined.
    for dep in loader.units[idx].deps.clone() {
        loader.link(dep)?;
    }
    Ok(Inputs { kind: Kind::File(idx), units: loader.units })
}

struct Loader<'a> {
    dir: &'a Path,
    units: Vec<Unit>,
    by_module: HashMap<String, usize>,
}

impl Loader<'_> {
    /// Reads and parses one file; `Ok(None)` if it is marked `#ignore=`.
    fn add(&mut self, path: PathBuf) -> Result<Option<usize>> {
        let label = path.display().to_string();
        let module = module_of(&path).map_err(|e| e.in_file(&label))?;
        if self.by_module.contains_key(&module) {
            return Err(ErrorSignals::Generic(format!("{label}: module `{module}` already exists")));
        }
        let src = fs::read_to_string(&path).map_err(|e| ErrorSignals::Generic(format!("{label}: {e}")))?;
        let proto = match parse(&src) {
            Err(ErrorSignals::NoError) => return Ok(None),
            r => r.map_err(|e| e.in_file(&label))?,
        };
        if label.ends_with(INC_EXT) {
            if let Some(item) = proto.items.iter().find(|i| matches!(i, Item::Frame(_) | Item::Fn(_))) {
                let e = ErrorSignals::at(item.line(), "include files may only contain `type`, `def`, `spec` and `filter`");
                return Err(e.in_file(&label));
            }
        }
        let idx = self.units.len();
        self.by_module.insert(module.clone(), idx);
        self.units.push(Unit { path, module, proto, deps: Vec::new(), linked: false });
        Ok(Some(idx))
    }

    /// Finds or loads `<name>.inc.cp`; errors point at the including line.
    fn include(&mut self, name: &str, ln: usize, from: &str) -> Result<usize> {
        if let Some(&idx) = self.by_module.get(&format!("{name}_inc")) {
            return Ok(idx);
        }
        let path = self.dir.join(format!("{name}{INC_EXT}"));
        if !path.is_file() {
            let e = ErrorSignals::at(ln, format!("include `{name}` not found ({})", path.display()));
            return Err(e.in_file(from));
        }
        self.add(path)?
            .ok_or_else(|| ErrorSignals::at(ln, format!("`{name}` is marked `#ignore=`")).in_file(from))
    }

    /// Fills `deps` of unit `idx`, loading includes on demand and rejecting cycles.
    fn link(&mut self, idx: usize) -> Result<()> {
        let mut deps = Vec::new();
        self.visit(idx, &mut vec![idx], &mut deps)?;
        self.units[idx].deps = deps;
        self.units[idx].linked = true;
        Ok(())
    }

    fn visit(&mut self, idx: usize, stack: &mut Vec<usize>, out: &mut Vec<usize>) -> Result<()> {
        let from = self.units[idx].label();
        for (name, ln) in self.units[idx].proto.includes.clone() {
            let dep = self.include(&name, ln, &from)?;
            if stack.contains(&dep) {
                let chain: Vec<&str> = stack.iter().chain([&dep]).map(|&i| self.units[i].file_name()).collect();
                return Err(ErrorSignals::at(ln, format!("include cycle: {}", chain.join(" -> "))).in_file(&from));
            }
            if out.contains(&dep) {
                continue;
            }
            stack.push(dep);
            self.visit(dep, stack, out)?;
            stack.pop();
            out.push(dep);
        }
        Ok(())
    }
}

fn push_unique(errors: &mut Vec<String>, e: ErrorSignals) {
    let e = e.to_string();
    if !errors.contains(&e) {
        errors.push(e);
    }
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|s| s.to_str()).unwrap_or("proto.cp")
}

fn module_name(name: String, what: &str) -> Result<String> {
    if !is_ident(&name) || is_reserved(&name) || name == "mod" {
        return Err(ErrorSignals::Generic(format!("{what} name `{name}` is not usable as a Rust module / C header name")));
    }
    Ok(name)
}

fn module_of(path: &Path) -> Result<String> {
    let name = file_name(path);
    let module = match name.strip_suffix(INC_EXT) {
        Some(base) => format!("{base}_inc"),
        None => name.strip_suffix(&format!(".{PROTO_EXT}")).unwrap_or(name).to_string(),
    };
    module_name(module, "file")
}

/// Proto files (`*.cp`, including `*.inc.cp`) of a directory, sorted; dotfiles are skipped.
fn collect_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Err(ErrorSignals::NotDir);
    }
    let mut files = Vec::new();
    for item in fs::read_dir(dir).map_err(|e| ErrorSignals::Generic(e.to_string()))? {
        let Ok(item) = item else { continue };
        let path = item.path();
        let name = file_name(&path);
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if path.is_file() && ext == PROTO_EXT && !name.starts_with('.') {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}
