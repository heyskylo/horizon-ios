//! Persistent custom shader images. Storage writes and sampler aliases share one allocation.
use horizon_shaders::ImageDefinition;

#[derive(Clone)]
pub(super) struct Image {
    pub definition: ImageDefinition,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub dimension: wgpu::TextureViewDimension,
    pub size: wgpu::Extent3d,
    pub snapshot: Option<(wgpu::Texture, wgpu::TextureView)>,
    pub storage: bool,
    pub capabilities: wgpu::TextureFormatFeatures,
}
pub(super) struct Images(pub Vec<Image>);

pub(super) fn format(name: &str) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat::*;
    Some(match name {
        "r8" => R8Unorm,
        "r8_snorm" => R8Snorm,
        "r8ui" => R8Uint,
        "r8i" => R8Sint,
        "rg8" => Rg8Unorm,
        "rg8_snorm" => Rg8Snorm,
        "rg8ui" => Rg8Uint,
        "rg8i" => Rg8Sint,
        "rgba8" => Rgba8Unorm,
        "rgba8_snorm" => Rgba8Snorm,
        "rgba8ui" => Rgba8Uint,
        "rgba8i" => Rgba8Sint,
        "r16f" => R16Float,
        "r16ui" => R16Uint,
        "r16i" => R16Sint,
        "rg16f" => Rg16Float,
        "rg16ui" => Rg16Uint,
        "rg16i" => Rg16Sint,
        "rgba16f" => Rgba16Float,
        "rgba16ui" => Rgba16Uint,
        "rgba16i" => Rgba16Sint,
        "r32f" => R32Float,
        "r32ui" => R32Uint,
        "r32i" => R32Sint,
        "rg32f" => Rg32Float,
        "rg32ui" => Rg32Uint,
        "rg32i" => Rg32Sint,
        "rgba32f" => Rgba32Float,
        "rgba32ui" => Rgba32Uint,
        "rgba32i" => Rgba32Sint,
        "rgb10_a2" => Rgb10a2Unorm,
        "rgb10_a2ui" => Rgb10a2Uint,
        "r11f_g11f_b10f" => Rg11b10Ufloat,
        _ => return None,
    })
}
fn extent(
    definition: &ImageDefinition,
    screen: [u32; 2],
    limits: &wgpu::Limits,
) -> Result<
    (
        wgpu::Extent3d,
        wgpu::TextureDimension,
        wgpu::TextureViewDimension,
    ),
    String,
> {
    let (dimension, view, limit) = match definition.size.len() {
        1 => (
            wgpu::TextureDimension::D1,
            wgpu::TextureViewDimension::D1,
            limits.max_texture_dimension_1d,
        ),
        2 => (
            wgpu::TextureDimension::D2,
            wgpu::TextureViewDimension::D2,
            limits.max_texture_dimension_2d,
        ),
        3 => (
            wgpu::TextureDimension::D3,
            wgpu::TextureViewDimension::D3,
            limits.max_texture_dimension_3d,
        ),
        _ => {
            return Err(format!(
                "{}: image must have 1–3 dimensions",
                definition.name
            ));
        }
    };
    let mut axes = [1; 3];
    for (axis, &value) in definition.size.iter().enumerate() {
        let value = if definition.relative && axis < 2 {
            value * screen[axis] as f32
        } else {
            value
        };
        if !value.is_finite() || value <= 0.0 || value.ceil() > limit as f32 {
            return Err(format!(
                "{}: image dimension exceeds device limit {limit}",
                definition.name
            ));
        }
        axes[axis] = (value.floor() as u32).max(1);
    }
    Ok((
        wgpu::Extent3d {
            width: axes[0],
            height: axes[1],
            depth_or_array_layers: axes[2],
        },
        dimension,
        view,
    ))
}
/// Storage usage is required only for live imageLoad/imageStore bindings, not sampler-only aliases.
pub(super) fn storage_names(post: &horizon_shaders::Post) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    let geometry = [
        &post.terrain,
        &post.water,
        &post.sky,
        &post.sky_textured,
        &post.entities,
        &post.block,
        &post.entities_translucent,
        &post.particles,
        &post.clouds,
        &post.weather,
        &post.hand,
        &post.shadow,
        &post.shadow_water,
        &post.shadow_entities,
        &post.shadow_block,
    ];
    for program in post
        .programs
        .iter()
        .chain(&post.shadow_comp)
        .chain(geometry.into_iter().flatten())
    {
        names.extend(
            program
                .program
                .interface
                .images
                .iter()
                .map(|i| i.name.clone()),
        );
    }
    for program in &post.compute.programs {
        names.extend(
            program
                .program
                .interface
                .images
                .iter()
                .map(|i| i.name.clone()),
        );
    }
    names
}
fn validate_usage(
    name: &str,
    capabilities: wgpu::TextureFormatFeatures,
    storage: bool,
) -> Result<(), String> {
    let required = wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_DST
        | wgpu::TextureUsages::COPY_SRC
        | if storage {
            wgpu::TextureUsages::STORAGE_BINDING
        } else {
            wgpu::TextureUsages::empty()
        };
    if !capabilities.allowed_usages.contains(required) {
        return Err(format!(
            "{name}: device format does not support required image usages {required:?}"
        ));
    }
    Ok(())
}
impl Image {
    pub fn supports_access(&self, access: naga::StorageAccess) -> bool {
        use wgpu::TextureFormatFeatureFlags as F;
        let required = match (
            access.contains(naga::StorageAccess::LOAD),
            access.contains(naga::StorageAccess::STORE),
        ) {
            (true, true) => F::STORAGE_READ_WRITE,
            (true, false) => F::STORAGE_READ_ONLY,
            (_, true) => F::STORAGE_WRITE_ONLY,
            _ => F::empty(),
        };
        self.storage && self.capabilities.flags.contains(required)
    }
}
fn allocate(
    device: &wgpu::Device,
    definition: &ImageDefinition,
    screen: [u32; 2],
    storage: bool,
    capabilities: wgpu::TextureFormatFeatures,
) -> Result<Image, String> {
    let format = format(&definition.format).ok_or_else(|| {
        format!(
            "{}: unknown storage image format {}",
            definition.name, definition.format
        )
    })?;
    validate_usage(&definition.name, capabilities, storage)?;
    let (size, dimension, view_dimension) = extent(definition, screen, &device.limits())?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&definition.name),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | if storage {
                wgpu::TextureUsages::STORAGE_BINDING
            } else {
                wgpu::TextureUsages::empty()
            },
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(view_dimension),
        ..Default::default()
    });
    Ok(Image {
        definition: definition.clone(),
        texture,
        view,
        format,
        dimension: view_dimension,
        size,
        snapshot: None,
        storage,
        capabilities,
    })
}
impl Images {
    pub fn new(
        device: &wgpu::Device,
        adapter: &wgpu::Adapter,
        definitions: &[ImageDefinition],
        storage: &std::collections::BTreeSet<String>,
        screen: [u32; 2],
    ) -> Result<Self, String> {
        if definitions.iter().any(|d| d.clear)
            && !device.features().contains(wgpu::Features::CLEAR_TEXTURE)
        {
            return Err(
                "Shader images require GPU texture clearing, unavailable on this device".into(),
            );
        }
        definitions
            .iter()
            .map(|definition| {
                let format = format(&definition.format).ok_or_else(|| {
                    format!(
                        "{}: unknown image format {}",
                        definition.name, definition.format
                    )
                })?;
                let capabilities = if device
                    .features()
                    .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
                {
                    adapter.get_texture_format_features(format)
                } else {
                    format.guaranteed_format_features(device.features())
                };
                allocate(
                    device,
                    definition,
                    screen,
                    storage.contains(&definition.name),
                    capabilities,
                )
            })
            .collect::<Result<Vec<_>, String>>()
            .map(Self)
    }
    pub fn resize(&mut self, device: &wgpu::Device, screen: [u32; 2]) -> Result<bool, String> {
        let mut replacement = self.0.clone();
        let mut changed = false;
        for (i, image) in self.0.iter().enumerate() {
            let (size, _, _) = extent(&image.definition, screen, &device.limits())?;
            if size != image.size {
                let mut new = Self(vec![allocate(
                    device,
                    &image.definition,
                    screen,
                    image.storage,
                    image.capabilities,
                )?]);
                if image.snapshot.is_some() {
                    new.ensure_snapshot(device, 0);
                }
                replacement[i] = new.0.remove(0);
                changed = true;
            }
        }
        self.0 = replacement;
        Ok(changed)
    }
    pub fn ensure_snapshot(&mut self, device: &wgpu::Device, index: usize) {
        let image = &mut self.0[index];
        if image.snapshot.is_some() {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Shader image read snapshot"),
            size: image.size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: image.texture.dimension(),
            format: image.format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(image.dimension),
            ..Default::default()
        });
        image.snapshot = Some((texture, view));
    }
    pub fn snapshot(&self, encoder: &mut wgpu::CommandEncoder, indices: &[usize]) {
        for &i in indices {
            let image = &self.0[i];
            let target = &image.snapshot.as_ref().expect("prepared image snapshot").0;
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &image.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: target,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                image.size,
            );
        }
    }
    pub fn sampler(&self, name: &str) -> Option<usize> {
        self.0.iter().position(|i| i.definition.sampler == name)
    }
    pub fn image(&self, name: &str) -> Option<usize> {
        self.0.iter().position(|i| i.definition.name == name)
    }
    /// Only explicitly cleared images lose history. First allocation is WebGPU-zero initialized.
    pub fn clear(&self, encoder: &mut wgpu::CommandEncoder) {
        for image in &self.0 {
            if image.definition.clear {
                encoder.clear_texture(&image.texture, &wgpu::ImageSubresourceRange::default());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampled_only_images_do_not_require_storage_format_support() {
        let sampled = wgpu::TextureFormatFeatures {
            allowed_usages: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            flags: wgpu::TextureFormatFeatureFlags::empty(),
        };
        assert!(validate_usage("sampled", sampled, false).is_ok());
        assert!(
            validate_usage("stored", sampled, true)
                .unwrap_err()
                .contains("stored")
        );
    }
    #[test]
    fn volume_dimensions_and_relative_limits() {
        let mut image = ImageDefinition {
            name: "volume".into(),
            sampler: "voxels".into(),
            format: "r8ui".into(),
            clear: true,
            relative: false,
            size: vec![192.0, 96.0, 192.0],
        };
        let limits = wgpu::Limits::default();
        let (size, _, view) = extent(&image, [800, 600], &limits).unwrap();
        assert_eq!(
            [size.width, size.height, size.depth_or_array_layers],
            [192, 96, 192]
        );
        assert_eq!(view, wgpu::TextureViewDimension::D3);
        image.size = vec![0.5, 0.5];
        image.relative = true;
        let (size, _, _) = extent(&image, [800, 600], &limits).unwrap();
        assert_eq!([size.width, size.height], [400, 300]);
        image.size[0] = f32::INFINITY;
        assert!(extent(&image, [800, 600], &limits).is_err());
    }
}
