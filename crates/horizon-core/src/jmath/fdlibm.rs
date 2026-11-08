// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
//
// Rust port of fdlibm 5.3's e_log.c, e_log10.c, s_cbrt.c, e_hypot.c, s_sin.c, s_cos.c, k_sin.c, k_cos.c and e_rem_pio2.c.

//! fdlibm 5.3, Sun's freely distributable math library
//! (<https://www.netlib.org/fdlibm/>), in Rust.
//!
//! - [`log`], [`log10`], [`cbrt`] and [`hypot`] are the C functions with the
//!   same double operations in the same order, so they round exactly as
//!   fdlibm does. The Java SE API defines `StrictMath`'s versions as fdlibm
//!   5.3's results.
//! - [`sin_fma`] and [`cos_fma`] are fdlibm's `sin` and `cos` as HotSpot's
//!   AArch64 intrinsics compute `Math.sin` and `Math.cos`. They differ from
//!   the C in these ways, checked against JDK 25 on Apple silicon:
//!   - `|x| < 2^-27` returns `x` (sin) or 1 (cos) first; the kernels have no
//!     shortcut of their own for a tiny argument.
//!   - The kernels' polynomials and corrections use fused multiply-adds, and
//!     the cosine kernel's `z * r` is `(z * z) * P`.
//!   - The cosine kernel picks its branch from the caller's argument, before
//!     reduction, and its first branch includes the bound `0x3fd33333`.
//!   - The reduction fuses its quotient and its second and third tails, and
//!     for n = 32 takes the extra steps only when the high word is 16 pi's,
//!     as for n < 32.
//!   - Beyond |x| = 2^19 * pi/2 the reduction is not modelled: the `libm`
//!     crate's `sin` and `cos` answer there, and may differ in the last bit.

/// High 32 bits of `x`, as a signed int (fdlibm's `__HI(x)`).
fn high_word(x: f64) -> i32 {
    (x.to_bits() >> 32) as i32
}

/// Low 32 bits of `x` (fdlibm's `__LO(x)`).
fn low_word(x: f64) -> u32 {
    x.to_bits() as u32
}

/// `x` with its high 32 bits replaced by `hw` (fdlibm's `__HI(x) = hw`).
fn set_high_word(x: f64, hw: i32) -> f64 {
    f64::from_bits((u64::from(hw as u32) << 32) | u64::from(low_word(x)))
}

/// The double whose high 32 bits are `hw` and whose low 32 bits are 0.
fn from_high_word(hw: i32) -> f64 {
    f64::from_bits(u64::from(hw as u32) << 32)
}

/// 2^54, to bring subnormal arguments into the normal range.
const TWO54: f64 = f64::from_bits(0x4350_0000_0000_0000);

// e_log.c: ln 2 split so that k * LN2_HI is exact for |k| < 2000, and the
// polynomial for log(1 + f) in s = f / (2 + f).
const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000); // 6.93147180369123816490e-01
const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76); // 1.90821492927058770002e-10
const LG1: f64 = f64::from_bits(0x3fe5_5555_5555_5593); // 6.666666666666735130e-01
const LG2: f64 = f64::from_bits(0x3fd9_9999_9997_fa04); // 3.999999999940941908e-01
const LG3: f64 = f64::from_bits(0x3fd2_4924_9422_9359); // 2.857142874366239149e-01
const LG4: f64 = f64::from_bits(0x3fcc_71c5_1d8e_78af); // 2.222219843214978396e-01
const LG5: f64 = f64::from_bits(0x3fc7_4664_96cb_03de); // 1.818357216161805012e-01
const LG6: f64 = f64::from_bits(0x3fc3_9a09_d078_c69f); // 1.531383769920937332e-01
const LG7: f64 = f64::from_bits(0x3fc2_f112_df3e_5244); // 1.479819860511658591e-01
/// The C literal `0.33333333333333333`, rounded to a double.
const ONE_THIRD: f64 = f64::from_bits(0x3fd5_5555_5555_5555);

/// Natural logarithm, fdlibm's `__ieee754_log`.
///
/// Writes `x = 2^k * (1 + f)` with `sqrt(2)/2 < 1 + f < sqrt(2)`, then
/// returns `k * ln 2 + log(1 + f)`, with `log(1 + f)` from a degree-14
/// polynomial in `s = f / (2 + f)` (or a short series when |f| < 2^-20).
/// `log(±0)` is -inf, negative arguments give NaN, `log(+inf)` is +inf.
pub fn log(x: f64) -> f64 {
    let mut x = x;
    let mut hx = high_word(x);
    let mut k = 0i32;
    if hx < 0x0010_0000 {
        // Zero, negative, or subnormal.
        if ((hx & 0x7fff_ffff) as u32 | low_word(x)) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = high_word(x);
    }
    if hx >= 0x7ff0_0000 {
        return x + x; // +inf or NaN
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000f_ffff;
    // Give 1 + f the exponent of 1, or of 1/2 when the mantissa is above
    // sqrt(2), and move that factor of 2 into k.
    let i = (hx + 0x95f64) & 0x0010_0000;
    x = set_high_word(x, hx | (i ^ 0x3ff0_0000));
    k += i >> 20;
    let f = x - 1.0;
    let dk = f64::from(k);
    if (0x000f_ffff & (2 + hx)) < 3 {
        // |f| < 2^-20: log(1 + f) = f - f^2/2 + f^3/3.
        if f == 0.0 {
            return if k == 0 {
                0.0
            } else {
                dk * LN2_HI + dk * LN2_LO
            };
        }
        let r = f * f * (0.5 - ONE_THIRD * f);
        return if k == 0 {
            f - r
        } else {
            dk * LN2_HI - ((r - dk * LN2_LO) - f)
        };
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t2 + t1;
    // Mantissas between 0x6147a and 0x6b851 (in the top 20 bits) keep
    // f^2/2 apart for accuracy.
    if ((hx - 0x6147a) | (0x6b851 - hx)) > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            f - (hfsq - s * (hfsq + r))
        } else {
            dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
        }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
    }
}

// e_log10.c: 1/ln 10, and log10(2) split so that n * LOG10_2HI is exact.
const IVLN10: f64 = f64::from_bits(0x3fdb_cb7b_1526_e50e); // 4.34294481903251816668e-01
const LOG10_2HI: f64 = f64::from_bits(0x3fd3_4413_509f_6000); // 3.01029995663611771306e-01
const LOG10_2LO: f64 = f64::from_bits(0x3d59_fef3_11f1_2b36); // 3.69423907715893078616e-13

/// Base-10 logarithm, fdlibm's `__ieee754_log10`.
///
/// Writes `x = 2^n * m`, with `m` in [1, 2), or in [1/2, 1) and `n` one
/// higher when the exponent is negative, and returns
/// `n * log10_2hi + (n * log10_2lo + log(m) / ln 10)`, with [`log`] for
/// `log(m)`. Special cases as [`log`].
pub fn log10(x: f64) -> f64 {
    let mut x = x;
    let mut hx = high_word(x);
    let mut k = 0i32;
    if hx < 0x0010_0000 {
        // Zero, negative, or subnormal.
        if ((hx & 0x7fff_ffff) as u32 | low_word(x)) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = high_word(x);
    }
    if hx >= 0x7ff0_0000 {
        return x + x; // +inf or NaN
    }
    k += (hx >> 20) - 1023;
    let i = i32::from(k < 0);
    hx = (hx & 0x000f_ffff) | ((0x3ff - i) << 20);
    let y = f64::from(k + i);
    x = set_high_word(x, hx);
    let z = y * LOG10_2LO + IVLN10 * log(x);
    z + y * LOG10_2HI
}

// s_cbrt.c: exponent biases for the first estimate, (682 - 0.03306235651) *
// 2^20 and (664 - 0.03306235651) * 2^20, and the rational correction's
// coefficients.
const B1: i32 = 715_094_163;
const B2: i32 = 696_219_795;
const CBRT_C: f64 = f64::from_bits(0x3fe1_5f15_f15f_15f1); // 19/35
const CBRT_D: f64 = f64::from_bits(0xbfe6_91de_2532_c834); // -864/1225
const CBRT_E: f64 = f64::from_bits(0x3ff6_a0ea_0ea0_ea0f); // 99/70
const CBRT_F: f64 = f64::from_bits(0x3ff9_b6db_6db6_db6e); // 45/28
const CBRT_G: f64 = f64::from_bits(0x3fd6_db6d_b6db_6db7); // 5/14

/// Cube root, fdlibm's `cbrt`.
///
/// A first estimate from the high word divided by 3 (5 bits), a rational
/// correction (23 bits), the estimate chopped to 20 bits and nudged up, then
/// one Newton step. `cbrt(±0)`, `cbrt(±inf)` and `cbrt(NaN)` return the
/// argument.
pub fn cbrt(x: f64) -> f64 {
    let hx = high_word(x);
    let sign = hx & i32::MIN;
    let hx = hx ^ sign;
    if hx >= 0x7ff0_0000 {
        return x + x; // NaN or ±inf
    }
    if (hx as u32 | low_word(x)) == 0 {
        return x; // ±0
    }
    let x = set_high_word(x, hx); // |x|
    let mut t = if hx < 0x0010_0000 {
        // Subnormal: estimate from 2^54 * x, keeping its low word.
        let t = TWO54 * x;
        set_high_word(t, high_word(t) / 3 + B2)
    } else {
        from_high_word(hx / 3 + B1)
    };
    let r = t * t / x;
    let s = CBRT_C + r * t;
    t *= CBRT_G + CBRT_F / (s + CBRT_E + CBRT_D / s);
    // Keep the top 20 bits of the mantissa and add one in the last of them,
    // so that t is above cbrt(x) and t * t is exact.
    t = from_high_word(high_word(t) + 1);
    let s = t * t;
    let r = x / s;
    let w = t + t;
    let r = (r - t) / (w + r);
    t += t * r;
    set_high_word(t, high_word(t) | sign)
}

/// `sqrt(x^2 + y^2)` without undue overflow or underflow, fdlibm's
/// `__ieee754_hypot`.
///
/// With `a >= b` the magnitudes, compared by high word: `a + b` when `a` is
/// more than 2^60 times `b`; otherwise both scaled by a power of two when
/// large or tiny, `a^2 + b^2` summed from parts with the low 32 bits split
/// off, and the square root scaled back. An infinite argument gives +inf,
/// even with a NaN; otherwise a NaN gives NaN.
pub fn hypot(x: f64, y: f64) -> f64 {
    let mut ha = high_word(x) & 0x7fff_ffff;
    let mut hb = high_word(y) & 0x7fff_ffff;
    let (mut a, mut b) = if hb > ha {
        std::mem::swap(&mut ha, &mut hb);
        (y, x)
    } else {
        (x, y)
    };
    a = set_high_word(a, ha);
    b = set_high_word(b, hb);
    if ha - hb > 0x03c0_0000 {
        return a + b; // a / b > 2^60
    }
    let mut k = 0i32;
    if ha > 0x5f30_0000 {
        // a > 2^500
        if ha >= 0x7ff0_0000 {
            // inf or NaN: inf wins over NaN.
            let mut w = a + b;
            if ((ha & 0x000f_ffff) as u32 | low_word(a)) == 0 {
                w = a;
            }
            if ((hb ^ 0x7ff0_0000) as u32 | low_word(b)) == 0 {
                w = b;
            }
            return w;
        }
        // Scale both by 2^-600.
        ha -= 0x2580_0000;
        hb -= 0x2580_0000;
        k += 600;
        a = set_high_word(a, ha);
        b = set_high_word(b, hb);
    }
    if hb < 0x20b0_0000 {
        // b < 2^-500
        if hb <= 0x000f_ffff {
            // b subnormal or zero
            if (hb as u32 | low_word(b)) == 0 {
                return a;
            }
            // Scale both by 2^1022. As in the C, `ha` and `hb` keep the
            // high words from before this scaling.
            let t1 = from_high_word(0x7fd0_0000);
            b *= t1;
            a *= t1;
            k -= 1022;
        } else {
            // Scale both by 2^600.
            ha += 0x2580_0000;
            hb += 0x2580_0000;
            k -= 600;
            a = set_high_word(a, ha);
            b = set_high_word(b, hb);
        }
    }
    let mut w = a - b;
    if w > b {
        // a > 2b: a^2 + b^2 = t1^2 + (b^2 + t2 * (a + t1)), t1 = a with
        // its low word cleared.
        let t1 = from_high_word(ha);
        let t2 = a - t1;
        w = (t1 * t1 - (b * (-b) - t2 * (a + t1))).sqrt();
    } else {
        // a <= 2b: a^2 + b^2 = 2a * b + (a - b)^2, 2a and b split in two.
        a += a;
        let y1 = from_high_word(hb);
        let y2 = b - y1;
        let t1 = from_high_word(ha + 0x0010_0000);
        let t2 = a - t1;
        w = (t1 * y1 - (w * (-w) - (t1 * y2 + t2 * b))).sqrt();
    }
    if k == 0 {
        w
    } else {
        // 2^k * w
        from_high_word(0x3ff0_0000 + (k << 20)) * w
    }
}

// k_sin.c's coefficients: sin x ~ x + S1 x^3 + ... + S6 x^13 on [-pi/4, pi/4].
const S1: f64 = f64::from_bits(0xbfc5_5555_5555_5549); // -1.66666666666666324348e-01
const S2: f64 = f64::from_bits(0x3f81_1111_1110_f8a6); // 8.33333333332248946124e-03
const S3: f64 = f64::from_bits(0xbf2a_01a0_19c1_61d5); // -1.98412698298579493134e-04
const S4: f64 = f64::from_bits(0x3ec7_1de3_57b1_fe7d); // 2.75573137070700676789e-06
const S5: f64 = f64::from_bits(0xbe5a_e5e6_8a2b_9ceb); // -2.50507602534068634195e-08
const S6: f64 = f64::from_bits(0x3de5_d93a_5acf_d57c); // 1.58969099521155010221e-10

// k_cos.c's coefficients: cos x ~ 1 - x^2/2 + C1 x^4 + ... + C6 x^14.
const C1: f64 = f64::from_bits(0x3fa5_5555_5555_554c); // 4.16666666666666019037e-02
const C2: f64 = f64::from_bits(0xbf56_c16c_16c1_5177); // -1.38888888888741095749e-03
const C3: f64 = f64::from_bits(0x3efa_01a0_19cb_1590); // 2.48015872894767294178e-05
const C4: f64 = f64::from_bits(0xbe92_7e4f_809c_52ad); // -2.75573143513906633035e-07
const C5: f64 = f64::from_bits(0x3e21_ee9e_bdb4_b1c4); // 2.08757232129817482790e-09
const C6: f64 = f64::from_bits(0xbda8_fae9_be88_38d4); // -1.13596475577881948265e-11

// e_rem_pio2.c: 2/pi, and pi/2 cut into 33-bit pieces (so that n times a piece
// is exact for n < 2^20) and the remainders after each.
const INVPIO2: f64 = f64::from_bits(0x3fe4_5f30_6dc9_c883); // 6.36619772367581382433e-01
const PIO2_1: f64 = f64::from_bits(0x3ff9_21fb_5440_0000); // 1.57079632673412561417e+00
const PIO2_1T: f64 = f64::from_bits(0x3dd0_b461_1a62_6331); // 6.07710050650619224932e-11
const PIO2_2: f64 = f64::from_bits(0x3dd0_b461_1a60_0000); // 6.07710050630396597660e-11
const PIO2_2T: f64 = f64::from_bits(0x3ba3_198a_2e03_7073); // 2.02226624879595063154e-21
const PIO2_3: f64 = f64::from_bits(0x3ba3_198a_2e00_0000); // 2.02226624871116645580e-21
const PIO2_3T: f64 = f64::from_bits(0x397b_839a_2520_49c1); // 8.47842766036889956997e-32
/// High words of n * pi/2 for n = 1..=32.
const NPIO2_HW: [i32; 32] = [
    0x3ff9_21fb,
    0x4009_21fb,
    0x4012_d97c,
    0x4019_21fb,
    0x401f_6a7a,
    0x4022_d97c,
    0x4025_fdbb,
    0x4029_21fb,
    0x402c_463a,
    0x402f_6a7a,
    0x4031_475c,
    0x4032_d97c,
    0x4034_6b9c,
    0x4035_fdbb,
    0x4037_8fdb,
    0x4039_21fb,
    0x403a_b41b,
    0x403c_463a,
    0x403d_d85a,
    0x403f_6a7a,
    0x4040_7e4c,
    0x4041_475c,
    0x4042_106c,
    0x4042_d97c,
    0x4043_a28c,
    0x4044_6b9c,
    0x4045_34ac,
    0x4045_fdbb,
    0x4046_c6cb,
    0x4047_8fdb,
    0x4048_58eb,
    0x4049_21fb,
];

/// The biased exponent of `v`, 11 bits.
fn biased_exponent(v: f64) -> i32 {
    (high_word(v) >> 20) & 0x7ff
}

/// fdlibm's `__kernel_sin(x, y, iy)` with fused multiply-adds: sin(x + y)
/// for |x| <= pi/4 + a little, `y` the tail of a reduced argument
/// (`reduced == false`: y is 0). Unlike the C, a tiny `x` goes through the
/// polynomial too.
fn sin_kernel_fma(x: f64, y: f64, reduced: bool) -> f64 {
    let z = x * x;
    let v = z * x;
    // S2 + z (S3 + z (S4 + z (S5 + z S6)))
    let r = z.mul_add(z.mul_add(z.mul_add(z.mul_add(S6, S5), S4), S3), S2);
    if !reduced {
        // x + v (S1 + z r)
        return v.mul_add(z.mul_add(r, S1), x);
    }
    // x - ((z (y/2 - v r) - y) - v S1)
    let t = (-v).mul_add(r, 0.5 * y);
    let u = (-z).mul_add(t, y);
    x + v.mul_add(S1, u)
}

/// fdlibm's `__kernel_cos(x, y)` with fused multiply-adds: cos(x + y) for
/// |x| <= pi/4 + a little. `hw` is the high word (sign cleared) of the
/// caller's argument before any reduction, and it alone picks the branch:
/// so a reduced argument always takes the second one with `qx = 0.28125`.
/// The first branch includes `hw == 0x3fd33333`, and a tiny `x` goes
/// through the polynomial too.
fn cos_kernel_fma(x: f64, y: f64, hw: i32) -> f64 {
    let z = x * x;
    // C1 + z (C2 + z (C3 + z (C4 + z (C5 + z C6)))); the C's z * r is zz * p.
    let p = z.mul_add(
        z.mul_add(z.mul_add(z.mul_add(z.mul_add(C6, C5), C4), C3), C2),
        C1,
    );
    let xy = x * y;
    let zz = z * z;
    if hw <= 0x3fd3_3333 {
        // 1 - (z/2 - (z r - x y))
        return 1.0 - 0.5f64.mul_add(z, (-zz).mul_add(p, xy));
    }
    // (1 - qx) - ((z/2 - qx) - (z r - x y)), with qx = |x|/4 cut to its
    // high word, or 0.28125 above 0.78125.
    let qx = if hw > 0x3fe9_0000 {
        0.28125
    } else {
        from_high_word(hw - 0x0020_0000)
    };
    let hz = 0.5f64.mul_add(z, -qx);
    let a = 1.0 - qx;
    a - (hz - zz.mul_add(p, -xy))
}

/// fdlibm's `__ieee754_rem_pio2(x)` for 2^-27 <= |x| <= 2^19 * pi/2, with
/// fused multiply-adds: `(n, y0, y1)` with `x - n * pi/2 = y0 + y1`.
fn rem_pio2_fma(x: f64) -> (i32, f64, f64) {
    let hx = high_word(x);
    let ix = hx & 0x7fff_ffff;
    if ix < 0x4002_d97c {
        // |x| < 3pi/4: n = ±1, as in the C. Next to pi/2 itself (high word
        // 0x3ff921fb), subtract the second 33 bits of pi/2 too.
        let (z, tail) = match (hx > 0, ix == 0x3ff9_21fb) {
            (true, false) => (x - PIO2_1, -PIO2_1T),
            (true, true) => (x - PIO2_1 - PIO2_2, -PIO2_2T),
            (false, false) => (x + PIO2_1, PIO2_1T),
            (false, true) => (x + PIO2_1 + PIO2_2, PIO2_2T),
        };
        let y0 = z + tail;
        let n = if hx > 0 { 1 } else { -1 };
        return (n, y0, (z - y0) + tail);
    }
    let t = x.abs();
    let fnum = t.mul_add(INVPIO2, 0.5).trunc();
    let n = fnum as i32;
    // t - n * pi/2 with 33 + 53 bits of pi/2.
    let mut r = (-fnum).mul_add(PIO2_1, t);
    let mut w = fnum * PIO2_1T;
    let mut y0 = r - w;
    if n > 32 || ix == NPIO2_HW[(n - 1) as usize] {
        // Past n = 32, or with the high word of n * pi/2 itself, y0 may have
        // lost bits to cancellation: past 16 bits take the next 33 bits of
        // pi/2, and past 49 the next 33.
        let j = ix >> 20;
        if j - biased_exponent(y0) > 16 {
            let t = r;
            let w2 = fnum * PIO2_2;
            r = t - w2;
            w = fnum.mul_add(PIO2_2T, w2 - (t - r));
            y0 = r - w;
            if j - biased_exponent(y0) > 49 {
                let t = r;
                let w3 = fnum * PIO2_3;
                r = t - w3;
                w = fnum.mul_add(PIO2_3T, w3 - (t - r));
                y0 = r - w;
            }
        }
    }
    let y1 = (r - y0) - w;
    if hx < 0 { (-n, -y0, -y1) } else { (n, y0, y1) }
}

/// `sin(x)`: fdlibm's `sin` (s_sin.c) with fused multiply-adds; see the
/// module documentation for how it departs from the C.
pub fn sin_fma(x: f64) -> f64 {
    let ix = high_word(x) & 0x7fff_ffff;
    if ix < 0x3e40_0000 {
        return x;
    }
    if ix <= 0x3fe9_21fb {
        return sin_kernel_fma(x, 0.0, false);
    }
    if ix >= 0x7ff0_0000 {
        return f64::NAN;
    }
    if ix > 0x4139_21fb {
        return libm::sin(x);
    }
    let (n, y0, y1) = rem_pio2_fma(x);
    match n & 3 {
        0 => sin_kernel_fma(y0, y1, true),
        1 => cos_kernel_fma(y0, y1, ix),
        2 => -sin_kernel_fma(y0, y1, true),
        _ => -cos_kernel_fma(y0, y1, ix),
    }
}

/// `cos(x)`: fdlibm's `cos` (s_cos.c) with fused multiply-adds; see the
/// module documentation for how it departs from the C.
pub fn cos_fma(x: f64) -> f64 {
    let ix = high_word(x) & 0x7fff_ffff;
    if ix < 0x3e40_0000 {
        return 1.0;
    }
    if ix <= 0x3fe9_21fb {
        return cos_kernel_fma(x, 0.0, ix);
    }
    if ix >= 0x7ff0_0000 {
        return f64::NAN;
    }
    if ix > 0x4139_21fb {
        return libm::cos(x);
    }
    let (n, y0, y1) = rem_pio2_fma(x);
    match n & 3 {
        0 => cos_kernel_fma(y0, y1, ix),
        1 => -sin_kernel_fma(y0, y1, true),
        2 => -cos_kernel_fma(y0, y1, ix),
        _ => sin_kernel_fma(y0, y1, true),
    }
}

#[cfg(test)]
mod tests {
    use super::{cbrt, cos_fma, hypot, log, log10, sin_fma};

    fn check1(f: fn(f64) -> f64, cases: &[(u64, u64)]) {
        for &(x, want) in cases {
            assert_eq!(f(f64::from_bits(x)).to_bits(), want, "{x:016x}");
        }
    }

    /// `StrictMath` on JDK 25 (x86-64), on the paths the plain vectors in
    /// `jmath::tests` don't reach: subnormal arguments, |f| < 2^-20 in `log`,
    /// and each of `hypot`'s scalings.
    #[test]
    fn matches_strictmath_on_every_path() {
        check1(log, &[(0x0006204cc2b51c13, 0xc0862ad9fbcaf99b)]);
        check1(log, &[(0x3feffffffffffd15, 0xbd37580000000110)]);
        check1(log10, &[(0x0003397f14b3dd31, 0xc0734592a35d544b)]);
        check1(cbrt, &[(0x8007a2937cf342e8, 0xaa9f817dc09448aa)]);
        for (x, y, want) in [
            // a > 2^500: scaled by 2^-600.
            (0xe08d3dde0d47e8aa, 0xdf642854234b0418, 0x608d3dde0d4857d4),
            // b < 2^-500: scaled by 2^600.
            (0x9656cdb86ae83b21, 0x96303a08c1241dab, 0x1657295fd0495647),
            // b subnormal: scaled by 2^1022, the split still using the high
            // words from before the scaling (updating them changes these).
            (0x00d66c302c6465c5, 0x000ffffff731f247, 0x00d66c303219c78c),
            (0x007cf84ccb90d050, 0x000fffffd32f892e, 0x007cf8937ce3c936),
            // inf beats NaN.
            (0x7ff0000000000000, 0x7ff8000000000000, 0x7ff0000000000000),
        ] {
            let got = hypot(f64::from_bits(x), f64::from_bits(y)).to_bits();
            assert_eq!(got, want, "{x:016x} {y:016x}");
        }
    }

    /// `Math.sin` / `Math.cos` on JDK 25, Apple M4: one argument for each way
    /// the fused versions differ from fdlibm's C, where the C's way gives
    /// another result.
    #[test]
    fn matches_hotspot_aarch64_where_it_departs_from_fdlibm() {
        // n = 32 away from 16 pi's high word: one reduction step only.
        check1(sin_fma, &[(0x404921fcfcc2b2f1, 0x3f0a87e85d5af49d)]);
        // High word 0x3fd33333: the cosine kernel's first branch.
        check1(cos_fma, &[(0x3fd33333d22b3d20, 0x3fee921dbcb1c922)]);
        // The cosine kernel's branch follows the caller's argument.
        check1(sin_fma, &[(0xc1262ac2ad35bfa3, 0xbfefbacd8d55fb04)]);
        // A tiny reduced argument still goes through the sine polynomial.
        check1(cos_fma, &[(0x412136d51041526e, 0x3e3dc3d8bd377c4d)]);
        // Fused polynomial, tail correction and z * r.
        check1(sin_fma, &[(0xbfe1b3f7fec43873, 0xbfe0d04fc1a57b1d)]);
        check1(cos_fma, &[(0x401e6ba3a2f24963, 0x3fcf86e6398c69d5)]);
        check1(cos_fma, &[(0x400f271ce0981ec5, 0xbfe75bfc80508df7)]);
    }
}
