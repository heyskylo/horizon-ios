//! Executes a synthetic, clean-room shader pack through the real `PackPost::run` frame flow.
use super::super::Frame;
use super::*;
use horizon_shaders::ComputeGroup;

const WINDOW: [u32; 2] = [4, 4];

struct ScratchPack(std::path::PathBuf);
impl ScratchPack {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "horizon-frame-{}-{unique}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("shaders")).unwrap();
        let files = [
            (
                "shaders.properties",
                "image.volume=volumeSampler rgba rgba8 unsigned_byte false false 2 2 2\n\
                 image.history=historySampler rgba rgba8 unsigned_byte false true 0.5 0.5\n",
            ),
            (
                // Once per allocation or render-size change: seeds red with 0.125.
                "setup.csh",
                "#version 430\n\
                 layout(local_size_x=1, local_size_y=1, local_size_z=1) in;\n\
                 const ivec3 workGroups = ivec3(2, 2, 2);\n\
                 layout(rgba8) uniform writeonly image3D volume;\n\
                 void main() {\n\
                   imageStore(volume, ivec3(gl_GlobalInvocationID), vec4(0.125, 0.0, 0.0, 1.0));\n\
                 }\n",
            ),
            (
                // Every frame: reads the previous contents through the sampler alias (which the
                // renderer serves from a snapshot, since this stage also stores) and adds 0.125.
                "begin.csh",
                "#version 430\n\
                 layout(local_size_x=1, local_size_y=1, local_size_z=1) in;\n\
                 const ivec3 workGroups = ivec3(2, 2, 2);\n\
                 layout(rgba8) uniform writeonly image3D volume;\n\
                 uniform sampler3D volumeSampler;\n\
                 void main() {\n\
                   ivec3 at = ivec3(gl_GlobalInvocationID);\n\
                   imageStore(volume, at, texelFetch(volumeSampler, at, 0) + vec4(0.125, 0.0, 0.0, 0.0));\n\
                 }\n",
            ),
            (
                "final.vsh",
                "#version 330 compatibility\nvoid main() { gl_Position = vec4(gl_Vertex.xy * 2.0 - 1.0, 0.0, 1.0); }\n",
            ),
            (
                "final.fsh",
                "#version 330 compatibility\n\
                 uniform sampler3D volumeSampler;\n\
                 uniform float viewWidth;\n\
                 uniform float viewHeight;\n\
                 void main() {\n\
                   gl_FragColor = vec4(texelFetch(volumeSampler, ivec3(0), 0).r, gl_FragCoord.x / viewWidth, gl_FragCoord.y / viewHeight, 1.0);\n\
                 }\n",
            ),
        ];
        for (name, source) in files {
            std::fs::write(root.join("shaders").join(name), source).unwrap();
        }
        Self(root)
    }
}
impl Drop for ScratchPack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The world as the renderer hands it over: colour and depth at the render size.
struct WorldTargets {
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
}
impl WorldTargets {
    fn new(device: &wgpu::Device, side: u32) -> Self {
        let make = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: side,
                        height: side,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Self {
            color: make("frame regression world", wgpu::TextureFormat::Rgba8Unorm),
            depth: make(
                "frame regression world depth",
                wgpu::TextureFormat::Depth32Float,
            ),
        }
    }
    /// Gives both textures defined contents (the pack draws no geometry over them).
    fn clear(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frame regression world clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.5,
                        g: 0.25,
                        b: 0.75,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
    }
}

/// An empty frame: no geometry, every list empty. `Frame` has no `Default`.
pub(super) fn empty_frame(hud: &crate::hud::HudBatch) -> Frame<'_> {
    Frame {
        weather: &[],
        weather_time: crate::weather::Time::new(0, 0.0),
        camera: glam::DVec3::new(0.5, 100.0, 0.5),
        yaw: 0.0,
        pitch: 10.0,
        fov_degrees: 85.0,
        bob: glam::Mat4::IDENTITY,
        spin: glam::Mat4::IDENTITY,
        sky: crate::environment::Sky::default(),
        distance: 12.0,
        detail_distance: 12.0,
        depth_far: 1000.0,
        underwater: false,
        cave_culling: false,
        title: false,
        sky_blocked: false,
        skybox: false,
        end_sky: false,
        end_sky_sprite: None,
        environment_fog: None,
        water_fog: [0.0; 3],
        fog: None,
        dark_disc: false,
        outline: Vec::new(),
        outline_after_terrain: false,
        outline_high_contrast: false,
        debug_lines: &[],
        debug_lines_on_top: &[],
        debug_crosshair: &[],
        hud,
        glint_alpha: 0.75,
        glint_ticks: 0.0,
        nether_lighting: false,
        entities: &[],
        entity_spans: &[],
        dynamic_lights: &[],
        clouds: None,
        lightning: Default::default(),
        additive: &[],
        see_through: &[],
        glowing: &[],
        celestial: &[],
        stars: None,
        hand: &[],
        hand_spans: &[],
        hand_fov_degrees: 70.0,
        screen: &[],
        pack: Default::default(),
        border: None,
        game_time: 0.0,
        shader_game_time: 0.0,
        end_portal_sprites: [[0.0; 4]; 2],
        crumbling_sprite: [0.0; 4],
        ticks: 0,
        darkness: (0.0, 0.0),
        night_vision: 0.0,
        block_flicker: 0.0,
        boss_darken: 0.0,
        menu_blur: None,
    }
}

/// The pack draws no terrain, so the game's terrain callback must never be reached.
fn no_terrain(
    _: &mut wgpu::RenderPass<'_>,
    list: TerrainList,
    _: &dyn Fn(&mut wgpu::RenderPass<'_>, u32),
) {
    panic!("the pack has no terrain program, but {list:?} was requested");
}

type Pixels = [[[u8; 4]; 4]; 4];

struct Harness<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    screen: wgpu::Texture,
    screen_view: wgpu::TextureView,
    readback: wgpu::Buffer,
}
impl<'a> Harness<'a> {
    fn new(device: &'a wgpu::Device, queue: &'a wgpu::Queue) -> Self {
        let screen = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame regression screen"),
            size: wgpu::Extent3d {
                width: WINDOW[0],
                height: WINDOW[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let screen_view = screen.create_view(&Default::default());
        // Four rows of the 256-byte aligned row pitch.
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame regression readback"),
            size: 256 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            device,
            queue,
            screen,
            screen_view,
            readback,
        }
    }

    /// One `PackPost::run` at `render` pixels per side onto the 4x4 window, read back row 0 first.
    fn frame(&self, post: &mut PackPost, render: u32) -> Pixels {
        let world = WorldTargets::new(self.device, render);
        let hud = crate::hud::HudBatch::new(WINDOW);
        let frame = empty_frame(&hud);
        let inputs = PackInputs::default();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        world.clear(&mut encoder);
        post.run(
            self.device,
            self.queue,
            &mut encoder,
            &world.color,
            &world.depth,
            &self.screen_view,
            &frame,
            &inputs,
            [render, render],
            WINDOW,
            &[],
            &mut no_terrain,
        );
        encoder.copy_texture_to_buffer(
            self.screen.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: None,
                },
            },
            self.screen.size(),
        );
        self.queue.submit([encoder.finish()]);
        let (send, done) = std::sync::mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        done.recv().unwrap().unwrap();
        let mut pixels = [[[0; 4]; 4]; 4];
        {
            let data = self.readback.slice(..).get_mapped_range().unwrap();
            for (y, row) in pixels.iter_mut().enumerate() {
                for (x, pixel) in row.iter_mut().enumerate() {
                    pixel.copy_from_slice(&data[y * 256 + x * 4..][..4]);
                }
            }
        }
        self.readback.unmap();
        pixels
    }
}

/// An 8-bit unorm store of `value`, as `imageStore` to rgba8 and a render target both do.
fn unorm(value: f32) -> u8 {
    (value * 255.0).round() as u8
}
/// What `begin.csh` stores after reading `previous` back through the sampler alias.
fn begin(previous: u8) -> u8 {
    unorm(f32::from(previous) / 255.0 + 0.125)
}
/// `gl_FragCoord.{x,y} / view{Width,Height}` for the pixel `index` counted from GL's origin
/// (the bottom-left corner, texel centres at .5), stored as 8-bit unorm.
fn coordinate(side: u32, index: u32) -> u8 {
    unorm((index as f32 + 0.5) / side as f32)
}

/// The final program's target is the window convention (`Target::Window`): clip y is not
/// negated, and the translator sets `gl_FragCoord.y = hz_target.y - y` from wgpu's top-left
/// rows (translate/mod.rs). So blue counts up from the BOTTOM row, green from the left column.
fn check_native(pixels: &Pixels, red: u8, label: &str) {
    for (y, row) in pixels.iter().enumerate() {
        for (x, pixel) in row.iter().enumerate() {
            let expected = [
                red,
                coordinate(4, x as u32),
                coordinate(4, 3 - y as u32),
                255,
            ];
            assert_eq!(*pixel, expected, "{label}, pixel ({x},{y})");
        }
    }
}

/// A 2x2 final upscaled to 4x4 by the present pass, which does not flip. Bilinear filtering
/// only makes the edge pixels exact (they clamp to a texel centre); the rest must stay between
/// the two texels and move in the gradient's direction.
fn check_upscaled(pixels: &Pixels, red: u8, label: &str) {
    let (low, high) = (coordinate(2, 0), coordinate(2, 1));
    for (y, row) in pixels.iter().enumerate() {
        for (x, pixel) in row.iter().enumerate() {
            assert_eq!(pixel[0], red, "{label}, red at ({x},{y})");
            assert_eq!(pixel[3], 255, "{label}, alpha at ({x},{y})");
            assert!(
                (low..=high).contains(&pixel[1]) && (low..=high).contains(&pixel[2]),
                "{label}, ({x},{y}) leaves the gradient range: {pixel:?}"
            );
        }
    }
    for (y, row) in pixels.iter().enumerate() {
        assert_eq!(row[0][1], low, "{label}, left green at row {y}");
        assert_eq!(row[3][1], high, "{label}, right green at row {y}");
        assert!(
            row.windows(2).all(|p| p[0][1] <= p[1][1]),
            "{label}, green must not decrease to the right in row {y}"
        );
    }
    for (x, (top, bottom)) in pixels[0].iter().zip(&pixels[3]).enumerate() {
        assert_eq!(top[2], high, "{label}, top blue at column {x}");
        assert_eq!(bottom[2], low, "{label}, bottom blue at column {x}");
        assert!(
            pixels.windows(2).all(|p| p[0][x][2] >= p[1][x][2]),
            "{label}, blue must not increase downward in column {x}"
        );
    }
}

fn extent(post: &PackPost, name: &str) -> [u32; 3] {
    let image = &post.images.0[post.images.image(name).unwrap()];
    [
        image.size.width,
        image.size.height,
        image.size.depth_or_array_layers,
    ]
}
fn texture_of(post: &PackPost, name: &str) -> wgpu::Texture {
    post.images.0[post.images.image(name).unwrap()]
        .texture
        .clone()
}
fn ran(post: &PackPost, group: ComputeGroup) -> bool {
    let mut passes = post.compute.iter().filter(|p| p.group == group);
    let pass = passes.next().expect("the compute stage was built");
    assert!(passes.next().is_none(), "exactly one {group:?} stage");
    pass.ran
}

/// `PackPost::run` end to end: `setup` once per render-size change, `begin` every frame reading
/// its own previous output through the sampler alias, and the real final path (render
/// resolution, then the non-flipping upscale, or straight to the screen at 1:1). Calling
/// `run_compute` by hand cannot prove that ordering, the lifecycle, or the final orientation.
#[test]
#[ignore = "requires a Metal adapter; run explicitly on macOS"]
fn run_executes_setup_begin_and_final_across_resize_on_metal() {
    use horizon_shaders::{GameInputs, Macros, Pack, Properties, load_post};
    // Documented arithmetic the expectations below rely on, checked once against its own values.
    assert_eq!([coordinate(2, 0), coordinate(2, 1)], [64, 191]);
    let red = [
        begin(unorm(0.125)),
        begin(begin(unorm(0.125))),
        begin(unorm(0.125)),
    ];
    assert_eq!(red, [64, 96, 64]);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("this execution regression requires a Metal adapter");
    // The features the renderer asks of the adapter for packs (gfx.rs), so the pipelines and
    // texture formats match production.
    let features = adapter.features()
        & (wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::RG11B10UFLOAT_RENDERABLE
            | wgpu::Features::VERTEX_WRITABLE_STORAGE
            | wgpu::Features::CLEAR_TEXTURE);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("frame regression device"),
        required_features: features,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();

    let fixture = ScratchPack::new();
    let pack = Pack::open(&fixture.0).unwrap();
    let macros = Macros::standard();
    let properties = Properties::from_pack(&pack, "", &macros);
    let parsed = load_post(&pack, "", &properties, &macros, &GameInputs::default());
    assert!(parsed.complete, "parse notes: {:?}", parsed.notes);
    let world = World {
        adapter: &adapter,
        atlas: None,
        lightmap: texture(&device, &queue, "frame regression lightmap", 1, &[255; 4]),
        terrain: None,
        block_ids: &[],
        celestial: None,
        clouds: None,
        weather: false,
        hand: false,
    };
    let mut post = PackPost::new(
        &device,
        &queue,
        parsed,
        wgpu::TextureFormat::Rgba8Unorm,
        world,
    );
    assert!(post.complete, "pipeline notes: {:?}", post.notes);
    assert_eq!(post.compute.len(), 2, "setup and begin must both be built");
    assert_eq!(post.images.0.len(), 2);
    assert_eq!(
        post.images.0[post.images.image("volume").unwrap()].dimension,
        wgpu::TextureViewDimension::D3
    );
    // Allocated for a 1x1 screen, before any frame: 0.5 x 0.5 of it, at least one texel.
    assert_eq!(extent(&post, "volume"), [2, 2, 2]);
    assert_eq!(extent(&post, "history"), [1, 1, 1]);
    let volume = texture_of(&post, "volume");
    let history = texture_of(&post, "history");

    let harness = Harness::new(&device, &queue);

    // Frame 1, 2x2 render resolution on a 4x4 window: the first run sizes the targets, so
    // setup seeds 0.125 and begin adds 0.125.
    let pixels = harness.frame(&mut post, 2);
    assert!(post.complete, "frame 1 notes: {:?}", post.notes);
    assert!(ran(&post, ComputeGroup::Setup), "frame 1 must run setup");
    assert!(ran(&post, ComputeGroup::Begin), "frame 1 must run begin");
    check_upscaled(&pixels, red[0], "frame 1");
    assert_eq!(extent(&post, "history"), [1, 1, 1], "0.5 x 2 pixels");
    assert!(
        texture_of(&post, "history") == history,
        "an unchanged extent keeps the allocation"
    );
    assert!(texture_of(&post, "volume") == volume);

    // Frame 2, same size: setup must not run again, so begin reads frame 1's 64 (through the
    // snapshot, not the image it is storing to) and the volume keeps its history.
    let pixels = harness.frame(&mut post, 2);
    assert!(post.complete, "frame 2 notes: {:?}", post.notes);
    assert!(
        !ran(&post, ComputeGroup::Setup),
        "frame 2 must not rerun setup"
    );
    assert!(ran(&post, ComputeGroup::Begin), "frame 2 must run begin");
    check_upscaled(&pixels, red[1], "frame 2");
    assert!(
        texture_of(&post, "history") == history,
        "a same-size frame keeps the allocation"
    );
    assert!(texture_of(&post, "volume") == volume);

    // Frame 3, render size 4x4: the relative image is reallocated at 0.5 x 4 and setup runs
    // again, overwriting the retained volume (fixed size, so the same allocation) before begin.
    // Render size equals the window, so the final pass draws straight onto the screen.
    let pixels = harness.frame(&mut post, 4);
    assert!(post.complete, "frame 3 notes: {:?}", post.notes);
    assert!(ran(&post, ComputeGroup::Setup), "a resize must rerun setup");
    assert!(ran(&post, ComputeGroup::Begin), "frame 3 must run begin");
    check_native(&pixels, red[2], "frame 3");
    assert_eq!(extent(&post, "history"), [2, 2, 1], "0.5 x 4 pixels");
    assert!(
        texture_of(&post, "history") != history,
        "a new extent needs a new allocation"
    );
    assert_eq!(extent(&post, "volume"), [2, 2, 2]);
    assert!(
        texture_of(&post, "volume") == volume,
        "a fixed-size image survives a resize"
    );
}

/// A compute-only shadow consumer needs integer color and copied depth allocations too.
#[test]
#[ignore = "requires a Metal adapter; run explicitly on macOS"]
fn compute_only_shadow_inputs_use_declared_formats_and_allocations_on_metal() {
    use horizon_shaders::{GameInputs, Macros, Pack, Properties, load_post};
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let features = adapter.features()
        & (wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::CLEAR_TEXTURE);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let fixture = ScratchPack::new();
    for file in ["setup.csh", "begin.csh"] {
        std::fs::remove_file(fixture.0.join("shaders").join(file)).unwrap();
    }
    std::fs::write(
        fixture.0.join("shaders/shadowcomp.csh"),
        r#"#version 430
layout(local_size_x=1,local_size_y=1,local_size_z=1) in;
const ivec3 workGroups=ivec3(2,2,2);
/* const int shadowMapResolution=4;
const int shadowcolor0Format=RGBA32UI;
const vec4 shadowcolor0ClearColor=vec4(4.0,0.0,0.0,0.0); */
uniform usampler2D shadowcolor0;
uniform sampler2D shadowtex1;
layout(rgba8) uniform writeonly image3D volume;
void main(){
    float r=float(texelFetch(shadowcolor0,ivec2(0),0).r)/16.0+0.25;
    float g=texelFetch(shadowtex1,ivec2(0),0).r*0.5;
    imageStore(volume,ivec3(gl_GlobalInvocationID),vec4(r,g,0.0,1.0));
}
"#,
    )
    .unwrap();
    std::fs::write(fixture.0.join("shaders/final.fsh"),"#version 330 compatibility\nuniform sampler3D volumeSampler;\nvoid main(){gl_FragColor=texelFetch(volumeSampler,ivec3(0),0);}").unwrap();
    let pack = Pack::open(&fixture.0).unwrap();
    let macros = Macros::standard();
    let properties = Properties::from_pack(&pack, "", &macros);
    let parsed = load_post(&pack, "", &properties, &macros, &GameInputs::default());
    assert!(parsed.complete, "{:?}", parsed.notes);
    assert!(parsed.shadow_pass);
    let world = World {
        adapter: &adapter,
        atlas: None,
        lightmap: texture(&device, &queue, "shadow compute lightmap", 1, &[255; 4]),
        terrain: None,
        block_ids: &[],
        celestial: None,
        clouds: None,
        weather: false,
        hand: false,
    };
    let mut post = PackPost::new(
        &device,
        &queue,
        parsed,
        wgpu::TextureFormat::Rgba8Unorm,
        world,
    );
    assert!(post.complete, "{:?}", post.notes);
    let shadow = post
        .shadow_targets
        .as_ref()
        .expect("compute sampler enables shadows");
    assert_eq!(shadow.size, 4);
    assert!(shadow.color[0].is_some());
    assert!(shadow.depth1.is_some());
    let pixels = Harness::new(&device, &queue).frame(&mut post, 4);
    for row in pixels {
        for pixel in row {
            assert_eq!(pixel, [128, 128, 0, 255]);
        }
    }
    assert!(ran(&post, ComputeGroup::ShadowComp));
}

/// A compute-only depth consumer must receive converted GL depth after the world blit.
#[test]
#[ignore = "requires a Metal adapter; run explicitly on macOS"]
fn compute_only_depth_reads_the_current_world_after_blit_on_metal() {
    use horizon_shaders::{GameInputs, Macros, Pack, Properties, load_post};
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let features = adapter.features()
        & (wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::CLEAR_TEXTURE);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let fixture = ScratchPack::new();
    for file in ["setup.csh", "begin.csh"] {
        std::fs::remove_file(fixture.0.join("shaders").join(file)).unwrap();
    }
    std::fs::write(fixture.0.join("shaders/composite.csh"),r#"#version 430
layout(local_size_x=1,local_size_y=1,local_size_z=1) in;
const ivec3 workGroups=ivec3(2,2,2);
uniform sampler2D depthtex0;
layout(rgba8) uniform writeonly image3D volume;
void main(){float d=texelFetch(depthtex0,ivec2(0),0).r;imageStore(volume,ivec3(gl_GlobalInvocationID),vec4(d,d,d,1.0));}
"#).unwrap();
    std::fs::write(fixture.0.join("shaders/final.fsh"),"#version 330 compatibility\nuniform sampler3D volumeSampler;\nvoid main(){gl_FragColor=texelFetch(volumeSampler,ivec3(0),0);}").unwrap();
    let pack = Pack::open(&fixture.0).unwrap();
    let macros = Macros::standard();
    let properties = Properties::from_pack(&pack, "", &macros);
    let parsed = load_post(&pack, "", &properties, &macros, &GameInputs::default());
    assert!(parsed.complete, "{:?}", parsed.notes);
    let world = World {
        adapter: &adapter,
        atlas: None,
        lightmap: texture(&device, &queue, "compute depth lightmap", 1, &[255; 4]),
        terrain: None,
        block_ids: &[],
        celestial: None,
        clouds: None,
        weather: false,
        hand: false,
    };
    let mut post = PackPost::new(
        &device,
        &queue,
        parsed,
        wgpu::TextureFormat::Rgba8Unorm,
        world,
    );
    assert!(post.complete, "{:?}", post.notes);
    assert!(post.blit_depth);
    // WorldTargets clears reverse-Z to0 (far). GL window depth is therefore1, not0.
    for row in Harness::new(&device, &queue).frame(&mut post, 4) {
        for pixel in row {
            assert_eq!(pixel, [255; 4]);
        }
    }
    assert!(ran(&post, ComputeGroup::Composite));
}
