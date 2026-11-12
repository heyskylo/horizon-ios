//! In-process symbolization: the running binary's own DWARF line tables (inlined frames included), then its symbol table.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;

use addr2line::gimli::{self, EndianSlice, RunTimeEndian};
use object::{Object, ObjectSection, ObjectSymbol, SymbolKind};

use crate::sampler::{Loaded, loaded_modules};

/// One function at an address; `file` is a base name only, so no home directory reaches a report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sym {
    pub name: String,
    pub file: Option<String>,
    pub line: u32,
}

/// Addresses to their frames, innermost inlined function first.
#[derive(Default)]
pub struct Symbols {
    frames: HashMap<u64, Vec<Sym>>,
    offsets: HashMap<u64, u64>,
}

impl Symbols {
    pub fn frames(&self, addr: u64) -> &[Sym] {
        self.frames.get(&addr).map_or(&[], Vec::as_slice)
    }

    /// `addr` relative to the start of its module, or 0 when it is in none.
    pub fn module_offset(&self, addr: u64) -> u64 {
        self.offsets.get(&addr).copied().unwrap_or(0)
    }
}

/// What the running binary can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolQuality {
    /// Line tables: file, line and inlined functions.
    Dwarf,
    /// Function names only.
    Symtab,
    None,
}

/// How well [`symbolize`] can name the functions of the running executable.
pub fn symbol_quality() -> SymbolQuality {
    let opened = std::env::current_exe().and_then(File::open).ok();
    // SAFETY: the mapping is read-only and dropped before this returns; see `resolve_module`.
    let map = opened.and_then(|f| unsafe { memmap2::Mmap::map(&f) }.ok());
    let Some(object) = map.as_deref().and_then(|m| object::File::parse(m).ok()) else {
        return SymbolQuality::None;
    };
    if object
        .section_by_name(".debug_line")
        .is_some_and(|s| s.size() > 0)
    {
        SymbolQuality::Dwarf
    } else if object.symbols().any(|s| s.kind() == SymbolKind::Text) {
        SymbolQuality::Symtab
    } else {
        SymbolQuality::None
    }
}

fn demangled(raw: &str) -> String {
    format!("{:#}", rustc_demangle::demangle(raw))
}

fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// A function symbol: start, size (0 when unknown) and name.
struct FnSym(u64, u64, String);

fn function_symbols(file: &object::File) -> Vec<FnSym> {
    let mut found: Vec<FnSym> = file
        .symbols()
        .chain(file.dynamic_symbols())
        .filter(|s| s.kind() == SymbolKind::Text && s.is_definition() && s.address() != 0)
        .filter_map(|s| Some(FnSym(s.address(), s.size(), demangled(s.name().ok()?))))
        .collect();
    found.sort_by_key(|s| s.0);
    found
}

fn from_symbol_table(symbols: &[FnSym], svma: u64) -> Option<&str> {
    let at = symbols.partition_point(|s| s.0 <= svma).checked_sub(1)?;
    let s = &symbols[at];
    (s.1 == 0 || svma < s.0 + s.1).then_some(s.2.as_str())
}

fn resolve_module(module: &Loaded, addrs: &[u64], out: &mut Symbols) {
    let Some(file) = File::open(&module.path).ok() else {
        return;
    };
    // SAFETY: the mapping is read-only and dropped before this returns; a file truncated meanwhile could fault, which a profiler tolerates.
    let Some(map) = (unsafe { memmap2::Mmap::map(&file) }).ok() else {
        return;
    };
    let Ok(object) = object::File::parse(&*map) else {
        return;
    };
    let endian = if object.is_little_endian() {
        RunTimeEndian::Little
    } else {
        RunTimeEndian::Big
    };
    let load = |id: gimli::SectionId| -> Result<EndianSlice<'_, RunTimeEndian>, gimli::Error> {
        let data = match object
            .section_by_name(id.name())
            .map(|s| s.uncompressed_data())
        {
            Some(Ok(Cow::Borrowed(data))) => data,
            _ => &[],
        };
        Ok(EndianSlice::new(data, endian))
    };
    let context = gimli::Dwarf::load(load)
        .ok()
        .and_then(|dwarf| addr2line::Context::from_dwarf(dwarf).ok());
    let symbols = function_symbols(&object);
    let unknown = || base_name(&module.path);
    for &addr in addrs {
        let svma = addr.wrapping_sub(module.base);
        let mut frames = Vec::new();
        let found = context
            .as_ref()
            .and_then(|c| c.find_frames(svma).skip_all_loads().ok());
        if let Some(mut iter) = found {
            while let Ok(Some(frame)) = iter.next() {
                let name = frame
                    .function
                    .as_ref()
                    .and_then(|f| f.raw_name().ok())
                    .map(|raw| demangled(&raw));
                let (file, line) = frame
                    .location
                    .map_or((None, 0), |l| (l.file.map(base_name), l.line.unwrap_or(0)));
                frames.push((name, file, line));
            }
        }
        if frames.is_empty() {
            frames.push((None, None, 0));
        }
        let table = from_symbol_table(&symbols, svma);
        // The symbol table has the full path; DWARF names of out-of-line functions are often the short name only.
        let outer = frames.len() - 1;
        let resolved = frames
            .into_iter()
            .enumerate()
            .map(|(i, (name, file, line))| Sym {
                name: (i == outer)
                    .then(|| table.map(str::to_string))
                    .flatten()
                    .or(name)
                    .unwrap_or_else(|| format!("[{} 0x{svma:x}]", unknown())),
                file,
                line,
            })
            .collect();
        out.frames.insert(addr, resolved);
        out.offsets.insert(addr, svma);
    }
}

/// Resolves `addrs` (lookup addresses inside the calling instruction) against the objects loaded now.
pub fn symbolize(addrs: impl IntoIterator<Item = u64>) -> Symbols {
    let modules = loaded_modules();
    let mut by_module: Vec<Vec<u64>> = vec![Vec::new(); modules.len()];
    let mut unmapped = Vec::new();
    for addr in addrs {
        match modules.iter().position(|m| m.range.contains(&addr)) {
            Some(i) => by_module[i].push(addr),
            None => unmapped.push(addr),
        }
    }
    let mut symbols = Symbols::default();
    for (module, addrs) in modules.iter().zip(&by_module) {
        if !addrs.is_empty() {
            resolve_module(module, addrs, &mut symbols);
        }
    }
    for addr in unmapped {
        let name = format!("[unmapped 0x{addr:x}]");
        symbols.frames.insert(
            addr,
            vec![Sym {
                name,
                file: None,
                line: 0,
            }],
        );
    }
    symbols
}
