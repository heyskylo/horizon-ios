//! Let the GLSL compiler evaluate authored constants, then lay them out as the actual UBO.

use std::collections::BTreeMap;

use naga::{Expression, Handle, Literal, Module, TypeInner};

use super::{Error, scan::Stage};

pub(super) fn merge<'a>(
    stages: impl IntoIterator<Item = (&'a Stage<'a>, &'a Module)>,
) -> Result<Vec<u8>, Vec<Error>> {
    let mut result = Vec::new();
    let mut seen = BTreeMap::<String, Vec<u8>>::new();
    for (stage, module) in stages {
        let (span, _) = super::block_layout(module);
        result.resize(span as usize, 0);
        let members = module.global_variables.iter().find_map(|(_, var)| {
            if var.space == naga::AddressSpace::Uniform
                && var
                    .binding
                    .as_ref()
                    .is_some_and(|b| b.group == 0 && b.binding == 0)
                && let TypeInner::Struct { members, .. } = &module.types[var.ty].inner
            {
                Some(members)
            } else {
                None
            }
        });
        let Some(members) = members else { continue };
        for (name, at) in &stage.defaults {
            let Some(member) = members.iter().find(|m| {
                m.name.as_deref() == Some(name.as_str())
                    || m.name.as_deref() == Some(format!("hz_bool_{name}").as_str())
            }) else {
                // An engine-owned per-draw member was moved to its own block.
                continue;
            };
            let constant_name = format!("hz_default_{name}");
            let constant = module.constants.iter().find_map(|(_, c)| {
                (c.name.as_deref() == Some(constant_name.as_str())).then_some(c)
            });
            let failure = || {
                vec![stage.error(
                    *at,
                    &format!("`{name}` needs a supported constant initializer"),
                )]
            };
            let constant = constant.ok_or_else(failure)?;
            let mut words = Vec::new();
            flatten(module, constant.init, &mut words).ok_or_else(failure)?;
            let mut bytes = vec![0; span as usize];
            let mut words = words.into_iter();
            write(
                module,
                &module.types[member.ty].inner,
                &mut words,
                &mut bytes,
                member.offset as usize,
            )
            .ok_or_else(failure)?;
            if words.next().is_some() {
                return Err(failure());
            }
            if let Some(previous) = seen.get(name) {
                if previous != &bytes {
                    return Err(vec![stage.error(
                        *at,
                        &format!("`{name}` has conflicting initial values across stages"),
                    )]);
                }
            } else {
                for (dst, value) in result.iter_mut().zip(&bytes) {
                    *dst |= value;
                }
                seen.insert(name.clone(), bytes);
            }
        }
    }
    Ok(result)
}

fn flatten(module: &Module, value: Handle<Expression>, out: &mut Vec<[u8; 4]>) -> Option<()> {
    match &module.global_expressions[value] {
        Expression::Literal(literal) => out.push(match *literal {
            Literal::F32(v) => v.to_le_bytes(),
            Literal::I32(v) => v.to_le_bytes(),
            Literal::U32(v) => v.to_le_bytes(),
            Literal::Bool(v) => u32::from(v).to_le_bytes(),
            _ => return None,
        }),
        Expression::Constant(c) => flatten(module, module.constants[*c].init, out)?,
        Expression::Compose { components, .. } => {
            for &component in components {
                flatten(module, component, out)?;
            }
        }
        Expression::Splat { size, value } => {
            for _ in 0..*size as usize {
                flatten(module, *value, out)?;
            }
        }
        Expression::ZeroValue(ty) => zeros(module, &module.types[*ty].inner, out)?,
        _ => return None,
    }
    Some(())
}

fn zeros(module: &Module, ty: &TypeInner, out: &mut Vec<[u8; 4]>) -> Option<()> {
    match ty {
        TypeInner::Scalar(_) => out.push([0; 4]),
        TypeInner::Vector { size, .. } => out.extend(vec![[0; 4]; *size as usize]),
        TypeInner::Matrix { columns, rows, .. } => {
            out.extend(vec![[0; 4]; *columns as usize * *rows as usize])
        }
        TypeInner::Array {
            base,
            size: naga::ArraySize::Constant(n),
            ..
        } => {
            for _ in 0..n.get() {
                zeros(module, &module.types[*base].inner, out)?;
            }
        }
        _ => return None,
    }
    Some(())
}

fn write(
    module: &Module,
    ty: &TypeInner,
    words: &mut impl Iterator<Item = [u8; 4]>,
    bytes: &mut [u8],
    offset: usize,
) -> Option<()> {
    match ty {
        TypeInner::Scalar(naga::Scalar { width: 4, .. }) => bytes
            .get_mut(offset..offset + 4)?
            .copy_from_slice(&words.next()?),
        TypeInner::Vector { size, scalar } => {
            for i in 0..*size as usize {
                write(
                    module,
                    &TypeInner::Scalar(*scalar),
                    words,
                    bytes,
                    offset + i * 4,
                )?;
            }
        }
        TypeInner::Matrix {
            columns,
            rows,
            scalar,
        } => {
            // The frontend rejects two-row std140 matrices; supported columns use 16 bytes.
            let stride = 16;
            for column in 0..*columns as usize {
                write(
                    module,
                    &TypeInner::Vector {
                        size: *rows,
                        scalar: *scalar,
                    },
                    words,
                    bytes,
                    offset + column * stride,
                )?;
            }
        }
        TypeInner::Array {
            base,
            size: naga::ArraySize::Constant(n),
            stride,
        } => {
            for i in 0..n.get() as usize {
                write(
                    module,
                    &module.types[*base].inner,
                    words,
                    bytes,
                    offset + i * *stride as usize,
                )?;
            }
        }
        _ => return None,
    }
    Some(())
}
