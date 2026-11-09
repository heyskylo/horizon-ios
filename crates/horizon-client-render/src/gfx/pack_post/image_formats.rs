//! Image formats: a storage image's format qualifier as wgpu's, and the stand-in a module's
//! storage images declare.

/// Makes each storage image of `module` in group 0 declare the format its layout entry binds:
/// a `layout(r11f_g11f_b10f)` image whose texture is the `RGBA16F` stand-in (`bindings`).
pub(super) fn storage_formats_of(
    module: &mut naga::Module,
    entries: &[wgpu::BindGroupLayoutEntry],
) {
    let globals: Vec<_> = module.global_variables.iter().map(|(h, _)| h).collect();
    for handle in globals {
        let var = &module.global_variables[handle];
        let Some(naga::ResourceBinding { group: 0, binding }) = var.binding else {
            continue;
        };
        let naga::TypeInner::Image {
            dim,
            arrayed,
            class: naga::ImageClass::Storage { format, access },
        } = module.types[var.ty].inner
        else {
            continue;
        };
        let bound = entries.iter().find_map(|e| match e.ty {
            wgpu::BindingType::StorageTexture { format, .. } if e.binding == binding => {
                Some(format)
            }
            _ => None,
        });
        if format == naga::StorageFormat::Rg11b10Ufloat
            && bound == Some(wgpu::TextureFormat::Rgba16Float)
        {
            let ty = module.types.insert(
                naga::Type {
                    name: module.types[var.ty].name.clone(),
                    inner: naga::TypeInner::Image {
                        dim,
                        arrayed,
                        class: naga::ImageClass::Storage {
                            format: naga::StorageFormat::Rgba16Float,
                            access,
                        },
                    },
                },
                naga::Span::UNDEFINED,
            );
            module.global_variables[handle].ty = ty;
        }
    }
}

/// The wgpu format of an image's format qualifier.
pub(super) fn image_format(qualifier: &str) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as T;
    Some(match qualifier {
        "rgba32f" => T::Rgba32Float,
        "rgba16f" => T::Rgba16Float,
        "rg32f" => T::Rg32Float,
        "rg16f" => T::Rg16Float,
        "r11f_g11f_b10f" => T::Rg11b10Ufloat,
        "r32f" => T::R32Float,
        "r16f" => T::R16Float,
        "rgba16" => T::Rgba16Unorm,
        "rgb10_a2" => T::Rgb10a2Unorm,
        "rgba8" => T::Rgba8Unorm,
        "rg16" => T::Rg16Unorm,
        "rg8" => T::Rg8Unorm,
        "r16" => T::R16Unorm,
        "r8" => T::R8Unorm,
        "rgba16_snorm" => T::Rgba16Snorm,
        "rgba8_snorm" => T::Rgba8Snorm,
        "rg16_snorm" => T::Rg16Snorm,
        "rg8_snorm" => T::Rg8Snorm,
        "r16_snorm" => T::R16Snorm,
        "r8_snorm" => T::R8Snorm,
        "rgba32i" => T::Rgba32Sint,
        "rgba16i" => T::Rgba16Sint,
        "rgba8i" => T::Rgba8Sint,
        "rg32i" => T::Rg32Sint,
        "rg16i" => T::Rg16Sint,
        "rg8i" => T::Rg8Sint,
        "r32i" => T::R32Sint,
        "r16i" => T::R16Sint,
        "r8i" => T::R8Sint,
        "rgba32ui" => T::Rgba32Uint,
        "rgba16ui" => T::Rgba16Uint,
        "rgb10_a2ui" => T::Rgb10a2Uint,
        "rgba8ui" => T::Rgba8Uint,
        "rg32ui" => T::Rg32Uint,
        "rg16ui" => T::Rg16Uint,
        "rg8ui" => T::Rg8Uint,
        "r32ui" => T::R32Uint,
        "r16ui" => T::R16Uint,
        "r8ui" => T::R8Uint,
        _ => return None,
    })
}
