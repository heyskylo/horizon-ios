//! `java.lang.Math` as HotSpot computes it on the machine Horizon runs on.
//!
//! Rust's float functions call the platform's libm, and those round
//! differently: glibc, Apple's libm and fdlibm disagree in the last bit for
//! a few percent of arguments. Vanilla's own results differ between machines
//! too, because HotSpot replaces some `Math` methods with intrinsics:
//!
//! - `atan2`, `atan`, `asin`, `acos` and `hypot` are `StrictMath`'s (fdlibm)
//!   everywhere.
//! - On x86-64, `sin`, `cos`, `tan`, `exp`, `log`, `log10` and `pow` are
//!   Intel's libm stubs, which round like glibc for nearly every argument;
//!   there they use the platform libm, as before. Windows' libm (Microsoft's)
//!   rounds `sin` and `cos` differently for about 3% of arguments, so there
//!   they are correctly rounded instead ([`exact`]), as Intel's stubs are for
//!   all but about 0.1%.
//! - On AArch64 only `sin` and `cos` are intrinsics, which compute fdlibm's
//!   `sin` and `cos` with fused multiply-adds ([`aarch64`]). Everything else
//!   is `StrictMath`, which the Java SE API defines as fdlibm 5.3's results
//!   ([`fdlibm`]).
//!
//! So these functions return what vanilla returns on the same machine, on
//! Linux and on an Apple silicon Mac alike.

pub mod fdlibm;

/// `Math.sin` and `Math.cos` as HotSpot computes them on AArch64: fdlibm's
/// algorithm with fused multiply-adds, on every platform.
pub mod aarch64 {
    pub use super::fdlibm::{cos_fma as cos, sin_fma as sin};
}

/// `Math.round(float)`: the `int` closest to `a`, ties toward positive
/// infinity (so `-2.5` is `-2`); `NaN` is 0, and values beyond the `int`
/// range give `i32::MIN` or `i32::MAX`.
pub fn round_f32(a: f32) -> i32 {
    // floor(a + 1/2). The sum is exact in an f64 for 2^-30 <= |a| < 2^31.
    // Below that it rounds to within 2^-30 of 1/2, whose floor is 0 as the
    // exact sum's is; above it the cast saturates either way. The cast also
    // maps NaN to 0.
    (f64::from(a) + 0.5).floor() as i32
}

/// `Math.sin`.
pub fn sin(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        aarch64::sin(x)
    } else if cfg!(windows) {
        exact::sin(x)
    } else {
        x.sin()
    }
}

/// `Math.cos`.
pub fn cos(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        aarch64::cos(x)
    } else if cfg!(windows) {
        exact::cos(x)
    } else {
        x.cos()
    }
}

/// `Math.tan`.
pub fn tan(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        libm::tan(x)
    } else {
        x.tan()
    }
}

/// `Math.exp`.
pub fn exp(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        libm::exp(x)
    } else {
        x.exp()
    }
}

/// `Math.log`.
pub fn log(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        fdlibm::log(x)
    } else {
        x.ln()
    }
}

/// `Math.log10`.
pub fn log10(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        fdlibm::log10(x)
    } else {
        x.log10()
    }
}

/// `Math.pow`.
pub fn pow(x: f64, y: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        libm::pow(x, y)
    } else {
        x.powf(y)
    }
}

/// `Math.cbrt`.
pub fn cbrt(x: f64) -> f64 {
    if cfg!(target_arch = "aarch64") {
        fdlibm::cbrt(x)
    } else {
        x.cbrt()
    }
}

/// `Math.atan2`.
pub fn atan2(y: f64, x: f64) -> f64 {
    crate::geometry::trig::fdlibm_atan2(y, x)
}

/// `Math.atan`.
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

/// `Math.asin`.
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

/// `Math.acos`.
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

/// `Math.hypot`.
pub fn hypot(x: f64, y: f64) -> f64 {
    fdlibm::hypot(x, y)
}

/// `Math.round(double)`: the nearest `long`, ties toward positive infinity
/// (so `-2.5` is `-2`), saturating, `NaN` is 0.
pub fn round(x: f64) -> i64 {
    let f = x.floor();
    (if x - f >= 0.5 { f + 1.0 } else { f }) as i64
}

/// `sin` and `cos` correctly rounded, for Windows, whose libm rounds them
/// unlike Java's x86-64 intrinsics. Double-double arithmetic (two `f64`s per
/// value): `x` reduced by a four-part pi/2, a short series checked against an
/// error bound, and where the bound leaves the rounding open, the full series
/// with its terms summed exactly. Checked against 400-bit results on 1.3 million
/// arguments, small ones and ones next to multiples of pi/2 among them; not a
/// proof.
pub mod exact {
    /// A value `hi + lo` with |lo| <= ulp(hi) / 2.
    #[derive(Clone, Copy)]
    struct Dd(f64, f64);

    /// `a + b` exactly, as a sum and its rounding error (Knuth).
    fn two_sum(a: f64, b: f64) -> Dd {
        let s = a + b;
        let v = s - a;
        Dd(s, (a - (s - v)) + (b - v))
    }

    /// `a + b` exactly when |a| >= |b| (Dekker).
    fn fast_two_sum(a: f64, b: f64) -> Dd {
        let s = a + b;
        Dd(s, b - (s - a))
    }

    /// `a * b` exactly, as a product and its rounding error (Dekker's split, no fused multiply-add).
    fn two_prod(a: f64, b: f64) -> Dd {
        fn split(a: f64) -> (f64, f64) {
            let t = 134_217_729.0 * a; // 2^27 + 1
            let hi = t - (t - a);
            (hi, a - hi)
        }
        let p = a * b;
        let (ah, al) = split(a);
        let (bh, bl) = split(b);
        Dd(p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
    }

    impl Dd {
        fn add(self, b: Dd) -> Dd {
            let s = two_sum(self.0, b.0);
            let t = two_sum(self.1, b.1);
            let s = fast_two_sum(s.0, s.1 + t.0);
            fast_two_sum(s.0, s.1 + t.1)
        }
        fn mul(self, b: Dd) -> Dd {
            let p = two_prod(self.0, b.0);
            fast_two_sum(p.0, p.1 + (self.0 * b.1 + self.1 * b.0))
        }
        /// Divided by a small whole number (exact in an `f64`).
        fn div(self, d: f64) -> Dd {
            let q = self.0 / d;
            let p = two_prod(q, d);
            let r = ((self.0 - p.0) - p.1 + self.1) / d;
            fast_two_sum(q, r)
        }
        fn neg(self) -> Dd {
            Dd(-self.0, -self.1)
        }
    }

    /// pi/2 = P1 + P2 + P3 + P4 to 2^-160: the first three have 33 significant bits, so `k * Pn` is
    /// exact for |k| < 2^20.
    const P1: f64 = f64::from_bits(0x3FF9_21FB_5440_0000);
    const P2: f64 = f64::from_bits(0x3DD0_B461_1A60_0000);
    const P3: f64 = f64::from_bits(0x3BA3_198A_2E00_0000);
    const P4: f64 = f64::from_bits(0x397B_839A_2520_49C1);

    /// `x - k * pi/2` and the quadrant `k mod 4`, for |x| <= 2^20.
    fn reduce(x: f64) -> (Dd, u32) {
        let k = (x * std::f64::consts::FRAC_2_PI).round();
        let r = Dd(x - k * P1, 0.0)
            .add(Dd(-(k * P2), 0.0))
            .add(Dd(-(k * P3), 0.0))
            .add(two_prod(k, P4).neg());
        (r, (k as i64 & 3) as u32)
    }

    /// `sum(xs)` rounded once, exactly (Shewchuk's partials, with the half-way fix-up of a final
    /// rounding that a running sum can't see).
    fn exact_sum(xs: &[f64]) -> f64 {
        let mut partials = [0.0f64; 64];
        let mut len = 0;
        for &x0 in xs {
            let mut x = x0;
            let mut i = 0;
            for j in 0..len {
                let mut y = partials[j];
                if x.abs() < y.abs() {
                    std::mem::swap(&mut x, &mut y);
                }
                let hi = x + y;
                let lo = y - (hi - x);
                if lo != 0.0 {
                    partials[i] = lo;
                    i += 1;
                }
                x = hi;
            }
            partials[i] = x;
            len = i + 1;
        }
        if len == 0 {
            return 0.0;
        }
        let mut n = len - 1;
        let mut hi = partials[n];
        let mut lo = 0.0;
        while n > 0 {
            let x = hi;
            n -= 1;
            let y = partials[n];
            hi = x + y;
            lo = y - (hi - x);
            if lo != 0.0 {
                break;
            }
        }
        if n > 0 && ((lo < 0.0 && partials[n - 1] < 0.0) || (lo > 0.0 && partials[n - 1] > 0.0)) {
            let y = lo * 2.0;
            let x = hi + y;
            if y == x - hi {
                hi = x;
            }
        }
        hi
    }

    /// sin(r) or cos(r) for |r| <= pi/4 + a little: the Taylor series to below 2^-110, each term
    /// kept as a double-double and the terms summed exactly. The leading terms are exact for a small
    /// `x` (r = x), where cos x and sin x come closest to half-way between two doubles.
    fn series(r: Dd, cos: bool) -> f64 {
        let r2 = r.mul(r);
        let mut parts = [0.0f64; 32];
        let mut len = 0;
        let (mut term, first) = if cos { (Dd(1.0, 0.0), 1.0) } else { (r, 2.0) };
        parts[0] = term.0;
        parts[1] = term.1;
        len += 2;
        for n in 1..=14 {
            let k = 2.0 * f64::from(n) + first - 1.0;
            term = term.mul(r2).div(k * (k - 1.0)).neg();
            parts[len] = term.0;
            parts[len + 1] = term.1;
            len += 2;
        }
        exact_sum(&parts[..len])
    }

    /// Taylor coefficients 1/n! with alternating sign, for the f64 tails: sin from r^7, cos from r^8.
    const SIN_TAIL: [f64; 9] = [
        -1.0 / 5040.0,
        1.0 / 362880.0,
        -1.0 / 39916800.0,
        1.0 / 6227020800.0,
        -1.0 / 1307674368000.0,
        1.0 / 355687428096000.0,
        -1.0 / 121645100408832000.0,
        1.0 / 51090942171709440000.0,
        -1.0 / 25852016738884976640000.0,
    ];
    const COS_TAIL: [f64; 8] = [
        1.0 / 40320.0,
        -1.0 / 3628800.0,
        1.0 / 479001600.0,
        -1.0 / 87178291200.0,
        1.0 / 20922789888000.0,
        -1.0 / 6402373705728000.0,
        1.0 / 2432902008176640000.0,
        -1.0 / 1124000727777607680000.0,
    ];

    fn horner(c: &[f64], x: f64) -> f64 {
        c.iter().rev().fold(0.0, |acc, &k| acc * x + k)
    }

    /// sin(r) or cos(r) to about 2^-64: the first terms in double-double, the small rest in f64.
    fn fast(r: Dd, cos: bool) -> Dd {
        let r2 = r.mul(r);
        let x2 = r2.0;
        if cos {
            let t2 = r2.div(2.0).neg();
            let t4 = r2.mul(r2).div(24.0);
            let t6 = t4.mul(r2).div(30.0).neg();
            let tail = x2 * x2 * x2 * x2 * horner(&COS_TAIL, x2);
            Dd(1.0, 0.0).add(t2).add(t4).add(t6).add(Dd(tail, 0.0))
        } else {
            let t3 = r2.mul(r).div(6.0).neg();
            let t5 = t3.mul(r2).div(20.0).neg();
            let tail = r.0 * x2 * x2 * x2 * horner(&SIN_TAIL, x2);
            r.add(t3).add(t5).add(Dd(tail, 0.0))
        }
    }

    /// Rounds `v` when its error bound can't change the rounding, else `None`.
    fn rounded(v: Dd) -> Option<f64> {
        let e = v.0.abs() * f64::from_bits(0x3C30_0000_0000_0000); // 2^-60
        let (a, b) = (v.0 + (v.1 - e), v.0 + (v.1 + e));
        (a == b).then_some(a)
    }

    /// sin (`cos == false`) or cos of a reduced angle in quadrant `q`.
    fn eval(r: Dd, q: u32, cos: bool) -> f64 {
        // sin(x) = sin r, cos r, -sin r, -cos r in quadrants 0..3; cos(x) is a quadrant later.
        let q = (q + u32::from(cos)) & 3;
        let want_cos = q & 1 == 1;
        let v = rounded(fast(r, want_cos)).unwrap_or_else(|| series(r, want_cos));
        if q >= 2 { -v } else { v }
    }

    /// `sin(x)` rounded to nearest; beyond |x| = 2^20 the platform's.
    pub fn sin(x: f64) -> f64 {
        if x.abs() < f64::from_bits(0x3E40_0000_0000_0000) {
            // |x| < 2^-27: sin x rounds to x (0 included).
            return x;
        }
        if x.is_nan() || x.abs() > 1_048_576.0 {
            return x.sin();
        }
        let (r, q) = reduce(x);
        eval(r, q, false)
    }

    /// `cos(x)` rounded to nearest; beyond |x| = 2^20 the platform's.
    pub fn cos(x: f64) -> f64 {
        if x.abs() < f64::from_bits(0x3E40_0000_0000_0000) {
            // |x| < 2^-27: cos x rounds to 1.
            return 1.0;
        }
        if x.is_nan() || x.abs() > 1_048_576.0 {
            return x.cos();
        }
        let (r, q) = reduce(x);
        eval(r, q, true)
    }
}

#[cfg(test)]
mod tests {
    use super::{aarch64, exact, fdlibm, round_f32};

    #[test]
    fn round_f32_is_exact_like_java() {
        for (x, expected) in [
            (0.499_999_97, 0),
            (0.5, 1),
            (-0.5, 0),
            (2.5, 3),
            (-2.5, -2),
            (1e10, i32::MAX),
            (-1e10, i32::MIN),
            (f32::NAN, 0),
            (8_388_609.0, 8_388_609),
        ] {
            assert_eq!(round_f32(x), expected, "{x}");
        }
    }

    #[test]
    fn round_f32_at_the_edges_of_the_int_range() {
        for (x, expected) in [
            (2_147_483_520.0, 2_147_483_520), // the largest float below 2^31
            (2_147_483_648.0, i32::MAX),
            (f32::INFINITY, i32::MAX),
            (-2_147_483_648.0, i32::MIN),
            (-2_147_483_904.0, i32::MIN),
            (f32::NEG_INFINITY, i32::MIN),
            (8_388_607.5, 8_388_608), // the largest floats with a fraction
            (-8_388_607.5, -8_388_607),
            (-f32::from_bits(1), 0),
        ] {
            assert_eq!(round_f32(x), expected, "{x}");
        }
    }

    #[test]
    fn round_ties_toward_positive_infinity() {
        for (x, want) in [
            (-2.5, -2),
            (2.5, 3),
            (-0.5, 0),
            (-1.5, -1),
            (0.5, 1),
            (-2.6, -3),
            (0.49999999999999994, 0),
            (f64::NAN, 0),
            (f64::INFINITY, i64::MAX),
            (f64::NEG_INFINITY, i64::MIN),
        ] {
            assert_eq!(super::round(x), want, "round({x})");
        }
    }

    /// Java 24's `Math.sin` and `Math.cos` on x86-64 HotSpot, for arguments
    /// where Windows' libm rounds the other way.
    #[test]
    fn exact_sin_and_cos_round_like_javas_x86_intrinsics() {
        for case in [
            (0x400cfed4c52a225f, 0xbfddb6fb8dfc34d9),
            (0x401562e96dc95e79, 0xbfe9c702a472eb20),
            (0x3fe83defdda344b2, 0x3fe5fd24ae6866f7),
            (0x4015f2973d92bdce, 0xbfe6df4f6eb17f8b),
            (0x400ccab8c6bca404, 0xbfdc43578964fcd9),
            (0x4081275db9d375bb, 0x3fe835467b8bb2a8),
            (0x408545bc77918c1a, 0x3feb130e731423b6),
            (0xc08a86d7f86386d0, 0xbfe2bc064eff6f2b),
        ] {
            check1(exact::sin, case);
        }
        for case in [
            (0x3fff44a3b3c18058, 0xbfd7f1ca30696aa9),
            (0x401824074317a30f, 0x3fef055d8c6cc46e),
            (0x4001b86d21e7b163, 0xbfe33824a109fcc3),
            (0xc08b37219efa0d36, 0xbfe913c9a4e569fe),
            (0xc084ce175c9dc180, 0x3feef489ec429ba7),
            (0x400ed26ab1d4ada5, 0xbfe83e5b263a94b1),
            (0x40156db4b780dcab, 0x3fe33b5a52983c7c),
            (0x408e8b5accbb6a88, 0xbfedab57ae231703),
            // About 3e-28 below a half-way point: a double-double running sum loses that.
            (0x3e937b1311fb4936, 0x3feffffffffffe85),
        ] {
            check1(exact::cos, case);
        }
        // Small arguments, the quadrant edges and beyond the reduced range.
        assert_eq!(exact::sin(1e-300), 1e-300);
        assert_eq!(exact::cos(-1e-10), 1.0);
        assert_eq!(exact::sin(0.0).to_bits(), 0);
        assert_eq!(exact::sin(-0.0).to_bits(), (-0.0f64).to_bits());
        assert!(exact::sin(f64::NAN).is_nan() && exact::cos(f64::INFINITY).is_nan());
        assert_eq!(exact::sin(std::f64::consts::FRAC_PI_2), 1.0);
        assert_eq!(exact::cos(std::f64::consts::PI), -1.0);
        assert_eq!(exact::sin(3.0e6), 3.0e6f64.sin());
    }

    fn check1(f: fn(f64) -> f64, (x, expected): (u64, u64)) {
        assert_eq!(f(f64::from_bits(x)).to_bits(), expected, "{x:x}");
    }

    fn check2(f: fn(f64, f64) -> f64, (x, y, expected): (u64, u64, u64)) {
        assert_eq!(
            f(f64::from_bits(x), f64::from_bits(y)).to_bits(),
            expected,
            "{x:x} {y:x}"
        );
    }

    #[test]
    fn matches_hotspot_on_aarch64() {
        // (argument bits, Math result bits) from HotSpot 25 on an Apple M4, each
        // one where glibc rounds differently.
        // sin
        check1(aarch64::sin, (0x3fe63c5ab66b09f0, 0x3fe47d34215b54f7));
        check1(aarch64::sin, (0xc0569150fe105e6f, 0xbfe7bc19d65ab55c));
        check1(aarch64::sin, (0x4072c3f15b47b5d6, 0xbfef32be0f0a9124));
        check1(aarch64::sin, (0x3fee86c6ffce047e, 0x3fea1a47f46cbf28));
        // cos
        check1(aarch64::cos, (0x3fbb10690eeadb00, 0x3fefd24375dbae28));
        check1(aarch64::cos, (0xc0496cc08a65245d, 0x3feab1a44a8022c4));
        check1(aarch64::cos, (0x40099262f89e73f8, 0xbfeff3aa148795ce));
        check1(aarch64::cos, (0x4001ce4e022387bc, 0xbfe37dd512be4d00));
        // log
        check1(fdlibm::log, (0x40569150fe105e6f, 0x401202e102f27b78));
        check1(fdlibm::log, (0x3ff5dec70f518390, 0x3fd4009cda1f8856));
        check1(fdlibm::log, (0x3feb3d17325d493e, 0xbfc49ee8c4866f84));
        check1(fdlibm::log, (0x3fe735a40edf77ba, 0xbfd48e1816b18bf2));
        // log10
        check1(fdlibm::log10, (0x3ff5dec70f518390, 0x3fc15fb52aa5f051));
        check1(fdlibm::log10, (0x3feb3d17325d493e, 0xbfb1e933cae02f5f));
        check1(fdlibm::log10, (0x3fe735a40edf77ba, 0xbfc1da98cbb8974c));
        check1(fdlibm::log10, (0x3fe7febd9ca1bab0, 0xbfc000e8e3b0b56e));
        // cbrt
        check1(fdlibm::cbrt, (0x401bb21f8568bd0c, 0x3ffe7ee30c7a56f9));
        check1(fdlibm::cbrt, (0x3fc59f3a45c8fb80, 0x3fe1b0791e0f8a74));
        check1(fdlibm::cbrt, (0xbfdd73cdc693ddab, 0xbfe8b4a6f43f1b73));
        check1(fdlibm::cbrt, (0xbfeb0715a8425e10, 0xbfee3f99498da6f6));
        // hypot
        check2(
            fdlibm::hypot,
            (0xc0569150fe105e6f, 0xc0056b970bf3b41a, 0x405693db72f461a6),
        );
        check2(
            fdlibm::hypot,
            (0x3fdc5999df9270f4, 0xc006c3ad990f72c0, 0x400709de26a1e40a),
        );
        check2(
            fdlibm::hypot,
            (0x401bb21f8568bd0c, 0xbffeb93d1a2ef904, 0x401cbdbb623d20c3),
        );
        check2(
            fdlibm::hypot,
            (0x4026d31c32ddb50b, 0xc00e7b2771d6867c, 0x40281027bb313aae),
        );
        // pow
        check2(
            libm::pow,
            (0x3fee04c313607ea6, 0xc00fd156138baf96, 0x3ff4a199ac2c8e9c),
        );
        check2(
            libm::pow,
            (0x401bb21f8568bd0c, 0xbffeb93d1a2ef904, 0x3f98ecc16a60feee),
        );
        check2(
            libm::pow,
            (0x407383ff427dadbd, 0x3ffdc5d178d00264, 0x40e56695c058d704),
        );
        check2(
            libm::pow,
            (0x3fe0c95b66dd6030, 0xc00c9cfde259a3e6, 0x4024194449e9deca),
        );
        // exp
        check1(libm::exp, (0x3fdc5999df9270f4, 0x3ff8eacc53fdbf56));
        check1(libm::exp, (0x3fcffff502ecbd80, 0x3ff48b5c78bca894));
        check1(libm::exp, (0x3fc59f3a45c8fb80, 0x3ff2f1c6cb35728c));
        check1(libm::exp, (0x4056561bf10ba7f6, 0x47fdd21dc9013084));
        // tan
        check1(libm::tan, (0x40023034f56881f0, 0xbff2e3f762ac9810));
        check1(libm::tan, (0xbfebf862877c88ec, 0xbff31f61e3e8539e));
        check1(libm::tan, (0xc00e37e234490564, 0xbfe79ca6eee66726));
        check1(libm::tan, (0xc075c085c04800b5, 0x3fea18b4a92fcf22));
    }

    #[test]
    fn atan2_matches_strictmath() {
        // (y bits, x bits, StrictMath.atan2 bits) from HotSpot 25: x < 0 and
        // |y/x| > 2^60, where libm::atan2 rounds one ulp lower,
        check2(
            super::atan2,
            (0x440e8aff5d1e293c, 0xc007e9768e83b37e, 0x3ff921fb54442d19),
        );
        check2(
            super::atan2,
            (0x440c610b44a22b37, 0xbfb07327bcb4c280, 0x3ff921fb54442d19),
        );
        check2(
            super::atan2,
            (0x43eb71600e035874, 0xbfeaff77e5a9ea38, 0x3ff921fb54442d19),
        );
        check2(
            super::atan2,
            (0x440edb2112965d8f, 0xbffaec4345ac33b1, 0x3ff921fb54442d19),
        );
        check2(
            super::atan2,
            (0x3ff0000000000000, 0x8000000000000001, 0x3ff921fb54442d19),
        );
        check2(
            super::atan2,
            (0xbff0000000000000, 0x8000000000000001, 0xbff921fb54442d19),
        );
        check2(
            super::atan2,
            (0x7fefffffffffffff, 0xbff0000000000000, 0x3ff921fb54442d19),
        );
        check2(
            super::atan2,
            (0xffefffffffffffff, 0xbff0000000000000, 0xbff921fb54442d19),
        );
        check2(
            super::atan2,
            (0x38fd955bf292fbac, 0xb4f940da66879af2, 0x3ff921fb54442d19),
        );
        check2(
            super::atan2,
            (0xf8954a6a9b403072, 0xf49292c2e2d50436, 0xbff921fb54442d19),
        );
        check2(
            super::atan2,
            (0x07e155d47eb507f2, 0x83d22629ee8e8471, 0x3ff921fb54442d19),
        );
        check2(
            super::atan2,
            (0xd2471cd599853356, 0xce403e2e72061985, 0xbff921fb54442d19),
        );
        // and the other quadrants and special cases, which it already matched
        check2(
            super::atan2,
            (0x440e8aff5d1e293c, 0x4007e9768e83b37e, 0x3ff921fb54442d18),
        );
        check2(
            super::atan2,
            (0xc40e8aff5d1e293c, 0x4007e9768e83b37e, 0xbff921fb54442d18),
        );
        check2(
            super::atan2,
            (0x0000000000000000, 0xbff0000000000000, 0x400921fb54442d18),
        );
        check2(
            super::atan2,
            (0x8000000000000000, 0xbff0000000000000, 0xc00921fb54442d18),
        );
        check2(
            super::atan2,
            (0x3ff0000000000000, 0x3ff0000000000000, 0x3fe921fb54442d18),
        );
        check2(
            super::atan2,
            (0x3ff0000000000000, 0xc410000000000000, 0x400921fb54442d18),
        );
        check2(
            super::atan2,
            (0x3e7ad7f29abcaf48, 0xc0f86a0000000000, 0x400921fb5444244c),
        );
        check2(
            super::atan2,
            (0x7ff0000000000000, 0xfff0000000000000, 0x4002d97c7f3321d2),
        );
        // NaN payloads may differ, so only NaN-ness is checked.
        assert!(super::atan2(f64::NAN, 1.0).is_nan());
        assert!(super::atan2(1.0, f64::NAN).is_nan());
    }
}
