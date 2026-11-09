//! The small packed colortex formats `RGB5_A1`, `RGB565` and `R3_G3_B2`, read back as in Iris
//! 1.11.7. wgpu has no such formats, so these buffers are 8-bit (`texture_format`); what a pass
//! writes into one and its clear colour go through [`quantise`] first.
//!
//! Probe pack p8106c of our own wrote 600 values to each format and read them back with
//! `texelFetch` in Iris 1.11.7 (NVIDIA 615.78.08). Our probe's Iris values fit this, with no
//! exception: each channel becomes the byte an `RGBA8` buffer stores for it, keeps that byte's
//! top bits (5, 5, 5 and 1 for `RGB5_A1`; 5, 6 and 5 for `RGB565` and also for `R3_G3_B2`), and
//! reads back those bits repeated to fill 8 (`0.3337` → byte 85 → 10 of 31 → 82 / 255). This is
//! a fit to one driver's values, not a rule Iris states. `RGBA4` and `RGBA2` read back the `RGBA8`
//! bytes in the same probe, as Horizon's 8-bit buffers already do.

use horizon_shaders::{Format, Kind};
use naga::{BinaryOperator, Expression, Literal, MathFunction, Statement, VectorSize};

/// The bits each channel keeps in a buffer of format `f`, for the formats that keep fewer than
/// 8; a three-channel format's alpha keeps 8 (it reads 1 anyway).
pub(super) fn bits(f: Format) -> Option<[u8; 4]> {
    match (f.kind, f.channels, f.bits) {
        (Kind::Unorm, 4, 5) => Some([5, 5, 5, 1]),
        // `RGB565`, and `R3_G3_B2` (3 × 3 bits in `config.rs`), which our probe's Iris values
        // put in 5, 6 and 5 bits too.
        (Kind::Unorm, 3, 6 | 3) => Some([5, 6, 5, 8]),
        _ => None,
    }
}

/// The value a buffer keeping `bits` per channel reads back after `v` is written or cleared.
pub(super) fn quantise(v: [f32; 4], bits: [u8; 4]) -> [f32; 4] {
    std::array::from_fn(|i| {
        let byte = (v[i].clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u32;
        let n = u32::from(bits[i]);
        let kept = byte >> (8 - n);
        // The kept bits, repeated until there are 8.
        let mut wide = 0;
        let mut have = 0;
        while have < 8 {
            wide = wide << n | kept;
            have += n;
        }
        (wide >> (have - 8)) as f32 / 255.0
    })
}

/// The factors of the shader's [`quantise`] for a channel keeping `n` bits: the kept value is
/// `floor(byte / step)`, and it reads back `kept × step + floor(kept × repeat)`.
fn factors(n: u8) -> (f32, f32) {
    let step = f32::from(1u16 << (8 - n));
    // `kept × repeat` is the kept bits shifted down to fill what `kept × step` leaves: one bit
    // fills the other 7 (× 127); 5 or 6 bits fill 3 or 2 with their top bits.
    let repeat = match n {
        1 => 127.0,
        _ => 2f32.powi(8 - 2 * i32::from(n)),
    };
    (step, repeat)
}

/// Makes the fragment entry point of `module` write each output location of `outputs` as
/// [`quantise`] gives it for its bits. The translator's entry point ends in one `return` of its
/// outputs; a module without one is left as it is.
pub(super) fn quantise_outputs(module: &mut naga::Module, outputs: &[(u32, [u8; 4])]) {
    let Some(ep) =
        (module.entry_points.iter_mut()).find(|e| e.stage == naga::ShaderStage::Fragment)
    else {
        return;
    };
    let Some(result) = &ep.function.result else {
        return;
    };
    let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        return;
    };
    let vec4 = |ty: naga::Handle<naga::Type>| {
        matches!(
            module.types[ty].inner,
            naga::TypeInner::Vector {
                size: VectorSize::Quad,
                scalar: naga::Scalar::F32
            }
        )
    };
    // (member, bits, the member's type) of each output to change.
    let changed: Vec<(usize, [u8; 4], naga::Handle<naga::Type>)> = (members.iter().enumerate())
        .filter_map(|(i, m)| {
            let Some(naga::Binding::Location { location, .. }) = m.binding else {
                return None;
            };
            let bits = outputs.iter().find(|o| o.0 == location)?.1;
            vec4(m.ty).then_some((i, bits, m.ty))
        })
        .collect();
    if changed.is_empty() {
        return;
    }
    let struct_ty = result.ty;
    let f = &mut ep.function;
    let body = std::mem::take(&mut f.body);
    let mut out = naga::Block::with_capacity(body.len() + 1);
    for (statement, span) in body.span_into_iter() {
        let Statement::Return { value: Some(value) } = statement else {
            out.push(statement, span);
            continue;
        };
        let Expression::Compose { components, .. } = &f.expressions[value] else {
            out.push(statement, span);
            continue;
        };
        let mut components = components.clone();
        let e = &mut f.expressions;
        let lit = |e: &mut naga::Arena<Expression>, v: f32| {
            e.append(Expression::Literal(Literal::F32(v)), span)
        };
        // Literals need no emitting: they come before the range that is emitted.
        let scalars: Vec<_> = (changed.iter())
            .map(|&(_, bits, _)| {
                let per = bits.map(factors);
                (
                    per.map(|(step, _)| lit(e, step)),
                    per.map(|(step, _)| lit(e, 1.0 / step)),
                    per.map(|(_, repeat)| lit(e, repeat)),
                )
            })
            .collect();
        let (zero, one, max, half) = (lit(e, 0.0), lit(e, 1.0), lit(e, 255.0), lit(e, 0.5));
        let mut add = |x: Expression| e.append(x, span);
        let splat = |add: &mut dyn FnMut(Expression) -> _, value| {
            add(Expression::Splat {
                size: VectorSize::Quad,
                value,
            })
        };
        let (zero, one, max, half) = (
            splat(&mut add, zero),
            splat(&mut add, one),
            splat(&mut add, max),
            splat(&mut add, half),
        );
        let binary = |add: &mut dyn FnMut(Expression) -> _, op, left, right| {
            add(Expression::Binary { op, left, right })
        };
        let math = |add: &mut dyn FnMut(Expression) -> _, fun, arg, arg1, arg2| {
            add(Expression::Math {
                fun,
                arg,
                arg1,
                arg2,
                arg3: None,
            })
        };
        for (&(member, _, ty), (steps, inverse, repeats)) in changed.iter().zip(scalars) {
            let mut vector = |components: [_; 4]| {
                add(Expression::Compose {
                    ty,
                    components: components.to_vec(),
                })
            };
            let (step, inverse, repeat) = (vector(steps), vector(inverse), vector(repeats));
            let x = components[member];
            let clamped = math(&mut add, MathFunction::Clamp, x, Some(zero), Some(one));
            let scaled = binary(&mut add, BinaryOperator::Multiply, clamped, max);
            let rounded = binary(&mut add, BinaryOperator::Add, scaled, half);
            let byte = math(&mut add, MathFunction::Floor, rounded, None, None);
            let shifted = binary(&mut add, BinaryOperator::Multiply, byte, inverse);
            let kept = math(&mut add, MathFunction::Floor, shifted, None, None);
            let high = binary(&mut add, BinaryOperator::Multiply, kept, step);
            let low = binary(&mut add, BinaryOperator::Multiply, kept, repeat);
            let low = math(&mut add, MathFunction::Floor, low, None, None);
            let wide = binary(&mut add, BinaryOperator::Add, high, low);
            components[member] = binary(&mut add, BinaryOperator::Divide, wide, max);
        }
        let value = add(Expression::Compose {
            ty: struct_ty,
            components,
        });
        // Everything from the first splat on is new and not a literal.
        let range = naga::Range::new_from_bounds(zero, value);
        out.push(Statement::Emit(range), span);
        out.push(Statement::Return { value: Some(value) }, span);
    }
    f.body = out;
}

/// A program's `vec4` float output into an integer buffer (`RGBA32UI`...) writes the float's
/// bits there, and the program draws its other buffers. Horizon refused such a program, since
/// wgpu has no float output into an integer target. Iris 1.11.7 drew it, with a probe pack of
/// ours (#10490): its other buffers kept every bit written, NaN payloads included, and the
/// integer buffer read the floats' bits back (1.5 as 0x3FC00000). GL leaves the integer
/// buffer's value undefined, so those bits are what this driver (NVIDIA 615.78.08) wrote.
pub(super) fn bitcast_outputs(module: &mut naga::Module, outputs: &[(u32, naga::ScalarKind)]) {
    let Some(ep) =
        (module.entry_points.iter_mut()).find(|e| e.stage == naga::ShaderStage::Fragment)
    else {
        return;
    };
    let Some(result) = &ep.function.result else {
        return;
    };
    let naga::TypeInner::Struct { members, span } = module.types[result.ty].inner.clone() else {
        return;
    };
    let mut new_members = members.clone();
    // (member, integer kind) of each output that changes.
    let mut changed = Vec::new();
    for (i, m) in members.iter().enumerate() {
        let Some(naga::Binding::Location { location, .. }) = m.binding else {
            continue;
        };
        let Some(&(_, kind)) = outputs.iter().find(|o| o.0 == location) else {
            continue;
        };
        if !matches!(
            module.types[m.ty].inner,
            naga::TypeInner::Vector {
                size: VectorSize::Quad,
                scalar: naga::Scalar::F32
            }
        ) {
            continue;
        }
        let ty = module.types.insert(
            naga::Type {
                name: None,
                inner: naga::TypeInner::Vector {
                    size: VectorSize::Quad,
                    scalar: naga::Scalar { kind, width: 4 },
                },
            },
            naga::Span::UNDEFINED,
        );
        new_members[i].ty = ty;
        if let Some(naga::Binding::Location {
            interpolation,
            sampling,
            ..
        }) = &mut new_members[i].binding
        {
            *interpolation = Some(naga::Interpolation::Flat);
            *sampling = None;
        }
        changed.push((i, kind));
    }
    if changed.is_empty() {
        return;
    }
    let old_name = module.types[result.ty].name.clone();
    let struct_ty = module.types.insert(
        naga::Type {
            name: old_name.map(|n| n + "_bits"),
            inner: naga::TypeInner::Struct {
                members: new_members,
                span,
            },
        },
        naga::Span::UNDEFINED,
    );
    let f = &mut ep.function;
    if let Some(r) = &mut f.result {
        r.ty = struct_ty;
    }
    let body = std::mem::take(&mut f.body);
    let mut out = naga::Block::with_capacity(body.len() + 1);
    for (statement, sp) in body.span_into_iter() {
        let Statement::Return { value: Some(value) } = statement else {
            out.push(statement, sp);
            continue;
        };
        let Expression::Compose { components, .. } = &f.expressions[value] else {
            out.push(statement, sp);
            continue;
        };
        let mut components = components.clone();
        let mut first = None;
        for &(member, kind) in &changed {
            let e = f.expressions.append(
                Expression::As {
                    expr: components[member],
                    kind,
                    convert: None,
                },
                sp,
            );
            first.get_or_insert(e);
            components[member] = e;
        }
        let value = f.expressions.append(
            Expression::Compose {
                ty: struct_ty,
                components,
            },
            sp,
        );
        out.push(
            Statement::Emit(naga::Range::new_from_bounds(first.unwrap_or(value), value)),
            sp,
        );
        out.push(Statement::Return { value: Some(value) }, sp);
    }
    f.body = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_colours_keep_the_same_bits() {
        // The CPU side, for clear colours. Iris 1.11.7 read p8106d's clear colour back as below in
        // RGB5_A1 and RGB565, and p8106c's 0.3337 as 82/255 in the 5-bit channels and 85/255 in
        // the 6-bit one.
        let to_255 = |v: [f32; 4]| v.map(|c| (c * 255.0 * 1000.0).round() / 1000.0);
        let rgb5_a1 = bits(Format::parse("RGB5_A1").unwrap()).unwrap();
        let rgb565 = bits(Format::parse("RGB565").unwrap()).unwrap();
        let r3_g3_b2 = bits(Format::parse("R3_G3_B2").unwrap()).unwrap();
        let clear = [0.3337, 0.1, 0.6, 0.3];
        assert_eq!(to_255(quantise(clear, rgb5_a1)), [82.0, 24.0, 156.0, 0.0]);
        assert_eq!(to_255(quantise(clear, rgb565))[..3], [82.0, 24.0, 156.0]);
        assert_eq!(
            to_255(quantise([0.3337; 4], r3_g3_b2))[..3],
            [82.0, 85.0, 82.0]
        );
        assert!(bits(Format::parse("RGBA4").unwrap()).is_none());
    }
}
