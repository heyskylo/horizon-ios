//! The frame-pointer stack walker the sampler and the allocation profiler share.

/// Frames kept per stack: the interrupted pc, then return addresses innermost first.
pub const MAX_FRAMES: usize = 64;

/// The address range of one thread's stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StackBounds {
    pub lo: usize,
    pub hi: usize,
}

impl StackBounds {
    /// The calling thread's stack. Allocates (glibc), so never from a signal handler: call it once per thread.
    #[cfg(target_os = "linux")]
    pub fn current() -> Option<Self> {
        // SAFETY: attr is initialised by pthread_getattr_np before use and destroyed after; the outputs are plain integers.
        unsafe {
            let mut attr: libc::pthread_attr_t = std::mem::zeroed();
            if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
                return None;
            }
            let mut base = std::ptr::null_mut();
            let mut size = 0;
            let ok = libc::pthread_attr_getstack(&attr, &mut base, &mut size) == 0;
            libc::pthread_attr_destroy(&mut attr);
            (ok && size > 0).then(|| Self {
                lo: base as usize,
                hi: base as usize + size,
            })
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn current() -> Option<Self> {
        None
    }
}

/// Walks the frame-pointer chain from `pc`/`fp`/`sp` into `out` (`out[0]` is `pc`, then return addresses); `bounds` must be the stack they belong to, mapped from `sp` up.
#[allow(clippy::missing_safety_doc)]
pub unsafe fn walk(
    pc: usize,
    fp: usize,
    sp: usize,
    bounds: StackBounds,
    out: &mut [usize],
) -> usize {
    let Some(first) = out.first_mut() else {
        return 0;
    };
    *first = pc;
    let mut n = 1;
    if sp < bounds.lo || sp >= bounds.hi {
        return n;
    }
    let (mut fp, mut floor) = (fp, sp);
    while n < out.len() && fp % 16 == 0 && fp >= floor && fp >= bounds.lo && fp <= bounds.hi - 16 {
        // SAFETY: fp and fp+8 lie in [sp, hi) of this thread's stack, which is mapped (the caller's contract).
        let (next, ret) = unsafe { (*(fp as *const usize), *((fp + 8) as *const usize)) };
        if ret == 0 {
            break;
        }
        out[n] = ret;
        n += 1;
        floor = fp + 1;
        fp = next;
    }
    n
}

/// The calling thread's frame pointer register.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn frame_pointer() -> usize {
    let fp;
    // SAFETY: reads a register; no memory or flags touched.
    unsafe {
        std::arch::asm!("mov {}, rbp", out(reg) fp, options(nomem, nostack, preserves_flags))
    };
    fp
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub fn frame_pointer() -> usize {
    let fp;
    // SAFETY: reads a register; no memory or flags touched.
    unsafe {
        std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack, preserves_flags))
    };
    fp
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[inline(always)]
pub fn frame_pointer() -> usize {
    0
}

/// The calling thread's own stack into `out`, innermost first; 0 when its bounds are unknown.
#[inline(never)]
pub fn capture_current(out: &mut [usize]) -> usize {
    let Some(bounds) = StackBounds::current() else {
        return 0;
    };
    let here = 0u8;
    // SAFETY: the calling thread's own live stack; the address of a local is at or below every frame read.
    unsafe {
        walk(
            capture_current as *const () as usize,
            frame_pointer(),
            &here as *const u8 as usize,
            bounds,
            out,
        )
    }
}

/// Whether code built here keeps frame pointers (false under `-C force-frame-pointers=no` with optimisation).
pub fn frame_pointers_present() -> bool {
    #[inline(never)]
    fn inner(out: &mut [usize]) -> usize {
        std::hint::black_box(capture_current(out))
    }
    #[inline(never)]
    fn outer(out: &mut [usize]) -> usize {
        std::hint::black_box(inner(out))
    }
    let mut frames = [0usize; 8];
    let n = outer(&mut frames);
    let outer_at = outer as *const () as usize;
    frames[..n]
        .iter()
        .any(|&a| a > outer_at && a < outer_at + 256)
        && n >= 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C, align(16))]
    struct Fake([usize; 16]);

    fn fill(s: &mut Fake) -> StackBounds {
        let base = s.0.as_ptr() as usize;
        (s.0[0], s.0[1], s.0[4], s.0[5], s.0[8], s.0[9]) =
            (base + 32, 0xaaa, base + 64, 0xbbb, 0, 0xccc);
        StackBounds {
            lo: base,
            hi: base + 128,
        }
    }

    fn walked(s: &Fake, b: StackBounds, sp: usize, out: usize) -> Vec<usize> {
        let mut frames = vec![0; out];
        // SAFETY: the fake stack is a live array that bounds describes.
        let n = unsafe { walk(1, s.0.as_ptr() as usize, sp, b, &mut frames) };
        frames.truncate(n);
        frames
    }

    #[test]
    fn walk_follows_the_chain_and_stops_at_every_bad_link() {
        let mut s = Fake([0; 16]);
        let b = fill(&mut s);
        let base = b.lo;
        assert_eq!(walked(&s, b, base, 8), [1, 0xaaa, 0xbbb, 0xccc]);
        assert_eq!(walked(&s, b, base, 3), [1, 0xaaa, 0xbbb]);
        assert_eq!(walked(&s, b, base - 16, 8), [1], "sp outside the stack");
        s.0[4] = base;
        assert_eq!(
            walked(&s, b, base, 8),
            [1, 0xaaa, 0xbbb],
            "a link back down"
        );
        s.0[0] = base + 8;
        assert_eq!(walked(&s, b, base, 8), [1, 0xaaa], "an unaligned link");
        s.0[0] = base + 4096;
        assert_eq!(walked(&s, b, base, 8), [1, 0xaaa], "a link past the stack");
    }
}
