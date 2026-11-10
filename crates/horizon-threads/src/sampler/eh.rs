//! Unwinds a copied stack with the loaded objects' `.eh_frame` tables (framehop); runs on the draining thread only.

use std::collections::HashSet;
use std::ffi::{CStr, c_int, c_void};
use std::fs::File;
use std::ops::Range;
use std::sync::Arc;

use framehop::{CacheNative, MayAllocateDuringUnwind, Module, ModuleSectionInfo};
use framehop::{Unwinder, UnwinderNative};
use object::{Object, ObjectSection, ReadCache};

use super::Regs;

type Data = Arc<[u8]>;

/// A loaded ELF object: its file, load bias and address range.
pub(crate) struct Loaded {
    pub path: String,
    pub base: u64,
    pub range: Range<u64>,
}

/// An ELF file's sections as framehop wants them; only the unwind tables are read, not the code.
struct Sections<'a>(object::File<'a, &'a ReadCache<File>>);

impl ModuleSectionInfo<Data> for Sections<'_> {
    fn base_svma(&self) -> u64 {
        0
    }

    fn section_svma_range(&mut self, name: &[u8]) -> Option<Range<u64>> {
        let s = self.0.section_by_name_bytes(name)?;
        Some(s.address()..s.address() + s.size())
    }

    fn section_data(&mut self, name: &[u8]) -> Option<Data> {
        Some(Data::from(self.0.section_by_name_bytes(name)?.data().ok()?))
    }
}

unsafe extern "C" fn collect(
    info: *mut libc::dl_phdr_info,
    _: libc::size_t,
    out: *mut c_void,
) -> c_int {
    // SAFETY: dl_iterate_phdr passes a valid info for the call, and `out` is the Vec `refresh` handed it.
    let (info, out) = unsafe { (&*info, &mut *(out as *mut Vec<Loaded>)) };
    // SAFETY: a non-null dlpi_name is a NUL-terminated string and dlpi_phdr points at dlpi_phnum headers.
    let (name, phdrs) = unsafe {
        let name =
            (!info.dlpi_name.is_null()).then(|| CStr::from_ptr(info.dlpi_name).to_string_lossy());
        (
            name,
            std::slice::from_raw_parts(info.dlpi_phdr, info.dlpi_phnum.into()),
        )
    };
    let path = match name.as_deref() {
        Some("") => "/proc/self/exe".to_string(),
        Some(name) => name.to_string(),
        None => return 0,
    };
    let loads = phdrs.iter().filter(|p| p.p_type == libc::PT_LOAD);
    let lo = loads.clone().map(|p| p.p_vaddr).min();
    let hi = loads.map(|p| p.p_vaddr + p.p_memsz).max();
    let base = info.dlpi_addr;
    if let (Some(lo), Some(hi)) = (lo, hi) {
        out.push(Loaded {
            path,
            base,
            range: base + lo..base + hi,
        });
    }
    0
}

/// The objects mapped into the process now.
pub(crate) fn loaded_modules() -> Vec<Loaded> {
    let mut modules: Vec<Loaded> = Vec::new();
    // SAFETY: the callback runs before this returns and only touches `modules`.
    unsafe {
        libc::dl_iterate_phdr(
            Some(collect),
            &mut modules as *mut Vec<Loaded> as *mut c_void,
        )
    };
    modules
}

fn load(m: &Loaded) -> Option<Module<Data>> {
    let cache = ReadCache::new(File::open(&m.path).ok()?);
    let file = object::File::parse(&cache).ok()?;
    Some(Module::new(
        m.path.clone(),
        m.range.clone(),
        m.base,
        Sections(file),
    ))
}

#[cfg(target_arch = "x86_64")]
fn native(r: &Regs) -> framehop::UnwindRegsNative {
    framehop::UnwindRegsNative::new(r.pc as u64, r.sp as u64, r.fp as u64)
}

#[cfg(target_arch = "aarch64")]
fn native(r: &Regs) -> framehop::UnwindRegsNative {
    framehop::UnwindRegsNative::new(r.lr as u64, r.sp as u64, r.fp as u64)
}

pub struct StackUnwinder {
    unwinder: UnwinderNative<Data, MayAllocateDuringUnwind>,
    cache: CacheNative<MayAllocateDuringUnwind>,
    seen: HashSet<u64>,
}

impl StackUnwinder {
    pub fn new() -> Self {
        Self {
            unwinder: UnwinderNative::new(),
            cache: CacheNative::new(),
            seen: HashSet::new(),
        }
    }

    /// Adds the objects loaded since the last call (a `dlopen`ed driver's frames resolve from the next drain on).
    pub fn refresh(&mut self) {
        let modules = loaded_modules();
        for m in modules.iter().filter(|m| self.seen.insert(m.base)) {
            if let Some(module) = load(m) {
                self.unwinder.add_module(module);
            }
        }
    }

    /// Unwinds `regs` over `stack`, a copy of the thread's memory from `regs.sp` up, into `out`; returns the frames written.
    pub fn unwind(&mut self, regs: &Regs, stack: &[u8], out: &mut [usize]) -> usize {
        let sp = regs.sp as u64;
        let read = |addr: u64| -> Result<u64, ()> {
            let at = usize::try_from(addr.checked_sub(sp).ok_or(())?).map_err(|_| ())?;
            let word = stack.get(at..at.checked_add(8).ok_or(())?).ok_or(())?;
            Ok(u64::from_ne_bytes(word.try_into().map_err(|_| ())?))
        };
        self.unwind_with(regs, read, out)
    }

    /// Unwinds `regs` reading stack words through `read`; returns the frames written to `out`.
    pub fn unwind_with(
        &mut self,
        regs: &Regs,
        mut read: impl FnMut(u64) -> Result<u64, ()>,
        out: &mut [usize],
    ) -> usize {
        let mut frames =
            self.unwinder
                .iter_frames(regs.pc as u64, native(regs), &mut self.cache, &mut read);
        let mut n = 0;
        while n < out.len() {
            let Ok(Some(frame)) = frames.next() else {
                break;
            };
            out[n] = frame.address() as usize;
            n += 1;
        }
        n
    }
}
