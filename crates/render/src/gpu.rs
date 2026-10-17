use crate::instance::Instance;
use crate::options::{GhostStyle, RenderOptions, ViewParams};
use crate::plan::{
    BACKGROUND_SLOT, FramePlan, HudRect, MINIMAP_BORDER_SLOT, MINIMAP_SLOT, RenderError,
    UNIFORM_SLOTS, UNIFORM_STRIDE, ViewUniform, plan,
};
use crate::scene::Scene;
use replay::Session;
use std::num::NonZeroU64;

const INDEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Uint;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
// Palette index + 1 of a translucent ghost, 0 where there is none
const GHOST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;
// Where a frame ends up, after the palette lookup
pub struct Output<'a> {
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shader: wgpu::ShaderModule,
    uniforms: wgpu::Buffer,
    scene_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    present_layout: wgpu::BindGroupLayout,
    with_present: wgpu::PipelineLayout,
    stencil: wgpu::RenderPipeline,
    fill: wgpu::RenderPipeline,
    sprite_clipped: wgpu::RenderPipeline,
    sprite_unclipped: wgpu::RenderPipeline,
    front_grass: wgpu::RenderPipeline,
    dynamic: wgpu::RenderPipeline,
    ghost: Option<wgpu::RenderPipeline>,
    timer: wgpu::RenderPipeline,
    presents: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    dynamic_buffer: wgpu::Buffer,
    timer_buffer: wgpu::Buffer,
    frame: FramePlan,
    timing: Option<PassTiming>,
}

// At most this many passes a frame
const MAX_PASSES: u32 = 8;

// GPU timestamps at the start and end of each pass, for profiling
struct PassTiming {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    // The last frame's passes, in order
    passes: Vec<&'static str>,
}

pub struct GpuScene {
    pub scene: Scene,
    bind_group: wgpu::BindGroup,
    instances: Option<wgpu::Buffer>,
    polygons: Option<wgpu::Buffer>,
}

// Intermediate render targets, sized like the output
pub struct Targets {
    pub width: u32,
    pub height: u32,
    index: wgpu::Texture,
    index_view: wgpu::TextureView,
    layer_view: wgpu::TextureView,
    ghost_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    copy: wgpu::Texture,
    layers_group: wgpu::BindGroup,
    timer_group: wgpu::BindGroup,
    present_group: wgpu::BindGroup,
}

fn texture_entry(
    binding: u32,
    sample_type: wgpu::TextureSampleType,
    dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: dimension,
            multisampled: false,
        },
        count: None,
    }
}

fn texture_binding(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn depth_stencil(
    depth_compare: wgpu::CompareFunction,
    depth_write: bool,
    stencil_compare: wgpu::CompareFunction,
    pass_op: wgpu::StencilOperation,
) -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare: stencil_compare,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op,
    };
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(depth_write),
        depth_compare: Some(depth_compare),
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 1,
            write_mask: 1,
        },
        bias: wgpu::DepthBiasState::default(),
    }
}

fn target(format: wgpu::TextureFormat, write: bool) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: if write {
            wgpu::ColorWrites::ALL
        } else {
            wgpu::ColorWrites::empty()
        },
    })
}

struct PipelineDesc<'a> {
    label: &'a str,
    layout: &'a wgpu::PipelineLayout,
    vs: &'a str,
    buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    fs: &'a str,
    targets: &'a [Option<wgpu::ColorTargetState>],
    depth: Option<wgpu::DepthStencilState>,
    constants: &'a [(&'a str, f64)],
}

fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    d: PipelineDesc,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(d.label),
        layout: Some(d.layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(d.vs),
            compilation_options: Default::default(),
            buffers: d.buffers,
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: d.depth,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(d.fs),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: d.constants,
                ..Default::default()
            },
            targets: d.targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn depth_attachment(
    view: &wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPassDepthStencilAttachment<'_> {
    let ops = |clear_value| wgpu::Operations {
        load: if clear {
            wgpu::LoadOp::Clear(clear_value)
        } else {
            wgpu::LoadOp::Load
        },
        store: wgpu::StoreOp::Store,
    };
    wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(ops(1.0)),
        stencil_ops: Some(wgpu::Operations {
            load: if clear {
                wgpu::LoadOp::Clear(0)
            } else {
                wgpu::LoadOp::Load
            },
            store: wgpu::StoreOp::Store,
        }),
    }
}

fn color(
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    color_stored(view, load, true)
}

// A color attachment that is thrown away after the pass unless `store`
fn color_stored(
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    store: bool,
) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load,
            store: if store {
                wgpu::StoreOp::Store
            } else {
                wgpu::StoreOp::Discard
            },
        },
    })
}

fn vertex_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

// Writes `bytes` to `buffer`, growing it first when needed
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &mut wgpu::Buffer,
    label: &str,
    bytes: &[u8],
) {
    if bytes.len() as u64 > buffer.size() {
        *buffer = vertex_buffer(device, label, (bytes.len() as u64).next_power_of_two());
    }
    if !bytes.is_empty() {
        queue.write_buffer(buffer, 0, bytes);
    }
}

// The non-sRGB format if the surface has one, so the palette is written as is. None when
// the adapter can't draw to the surface.
pub fn surface_format(caps: &wgpu::SurfaceCapabilities) -> Option<wgpu::TextureFormat> {
    let first = *caps.formats.first()?;
    Some(
        caps.formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(first),
    )
}

// The surface's next frame, reconfiguring it when it went out of date
pub fn acquire(
    surface: &wgpu::Surface,
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> Option<wgpu::SurfaceTexture> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            surface.configure(device, config);
            None
        }
        _ => None,
    }
}

// Why the GPU couldn't be opened
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("no GPU adapter found: {0}")]
    NoAdapter(#[from] wgpu::RequestAdapterError),
    #[error("failed to open the GPU: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
}

impl Renderer {
    // Opens the GPU, one that can draw to `surface` when given
    pub async fn request(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        label: &str,
    ) -> Result<(Renderer, wgpu::Adapter), GpuError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some(label),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                // For pass timing, when asked for
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                ..Default::default()
            })
            .await?;
        // WebGL2 can't write one color target and leave the other alone in one draw
        let translucent_ghosts = adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::INDEPENDENT_BLEND);
        Ok((Renderer::new(device, queue, translucent_ghosts), adapter))
    }

    // Without `translucent_ghosts` support, translucent ghosts are drawn like any bike
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, translucent_ghosts: bool) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rectools"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("views"),
            size: UNIFORM_STRIDE * UNIFORM_SLOTS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        use wgpu::TextureSampleType as T;
        let d2 = wgpu::TextureViewDimension::D2;
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        // The view, the atlas, the timer's color map and the palette
        let scene_layout = layout(
            "scene",
            &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(std::mem::size_of::<ViewUniform>() as u64),
                    },
                    count: None,
                },
                texture_entry(1, T::Uint, wgpu::TextureViewDimension::D2Array),
                texture_entry(2, T::Uint, d2),
                texture_entry(3, T::Float { filterable: false }, d2),
            ],
        );
        // One render target: the layers for front grass, or the scene copy for the timer
        let texture_layout = layout("target", &[texture_entry(0, T::Uint, d2)]);
        let present_layout = layout(
            "present",
            &[texture_entry(0, T::Uint, d2), texture_entry(1, T::Uint, d2)],
        );

        let pipeline_layout = |layouts: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: layouts,
                immediate_size: 0,
            })
        };
        let scene_only = pipeline_layout(&[Some(&scene_layout)]);
        let with_texture = pipeline_layout(&[Some(&scene_layout), Some(&texture_layout)]);
        let with_present = pipeline_layout(&[Some(&scene_layout), Some(&present_layout)]);

        let instances = &[Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32, 4 => Uint32,
                5 => Uint32x4, 6 => Uint32x4, 7 => Float32x2, 8 => Uint32, 9 => Uint32,
                10 => Uint32, 11 => Uint32, 12 => Uint32
            ],
        })];
        let polygons = &[Some(wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        })];
        let rects = &[Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<HudRect>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Uint32],
        })];

        use wgpu::CompareFunction as C;
        use wgpu::StencilOperation as S;
        let level_write = [target(INDEX_FORMAT, true), target(INDEX_FORMAT, true)];
        let level_none = [target(INDEX_FORMAT, false), target(INDEX_FORMAT, false)];
        let bikes_write = [target(INDEX_FORMAT, true), target(GHOST_FORMAT, true)];
        let ghosts_only = [target(INDEX_FORMAT, false), target(GHOST_FORMAT, true)];
        let level = |label, fs, write: bool, depth| PipelineDesc {
            label,
            layout: &scene_only,
            vs: "vs_instance",
            buffers: instances,
            fs,
            targets: if write { &level_write } else { &level_none },
            depth: Some(depth),
            constants: &[],
        };
        let make = |d| pipeline(&device, &shader, d);
        let stencil = make(PipelineDesc {
            vs: "vs_polygon",
            buffers: polygons,
            ..level(
                "polygons",
                "fs_polygon",
                false,
                depth_stencil(C::Always, false, C::Always, S::Invert),
            )
        });
        // The stencil reference picks ground (0) or sky (1)
        let fill = make(level(
            "fill",
            "fs_level",
            true,
            depth_stencil(C::Always, false, C::Equal, S::Keep),
        ));
        let sprite_clipped = make(level(
            "clipped pictures",
            "fs_level",
            true,
            depth_stencil(C::Less, true, C::Equal, S::Keep),
        ));
        let sprite_unclipped = make(level(
            "unclipped pictures",
            "fs_level",
            true,
            depth_stencil(C::Less, true, C::Always, S::Keep),
        ));
        let front_grass = make(PipelineDesc {
            label: "front grass",
            layout: &with_texture,
            vs: "vs_fullscreen",
            buffers: &[],
            fs: "fs_front_grass",
            targets: &[],
            depth: Some(depth_stencil(C::Always, true, C::Always, S::Keep)),
            constants: &[],
        });
        // Bikes and objects write the scene and clear translucent ghosts under them;
        // translucent ghosts only go into their own layer
        let bikes = |label, fs, write_index: bool, depth_write| PipelineDesc {
            label,
            layout: &scene_only,
            vs: "vs_instance",
            buffers: instances,
            fs,
            targets: if write_index {
                &bikes_write
            } else {
                &ghosts_only
            },
            depth: Some(depth_stencil(C::LessEqual, depth_write, C::Always, S::Keep)),
            constants: &[],
        };
        let dynamic = make(bikes("bikes", "fs_dynamic", true, true));
        let ghost =
            translucent_ghosts.then(|| make(bikes("translucent ghosts", "fs_ghost", false, false)));
        let timer = make(PipelineDesc {
            label: "timer",
            layout: &with_texture,
            vs: "vs_timer",
            buffers: rects,
            fs: "fs_timer",
            targets: &[target(INDEX_FORMAT, true)],
            depth: None,
            constants: &[],
        });

        Renderer {
            dynamic_buffer: vertex_buffer(&device, "dynamic instances", 64 * 1024),
            timer_buffer: vertex_buffer(&device, "timer rects", 16 * 1024),
            device,
            queue,
            shader,
            uniforms,
            scene_layout,
            texture_layout,
            present_layout,
            with_present,
            stencil,
            fill,
            sprite_clipped,
            sprite_unclipped,
            front_grass,
            dynamic,
            ghost,
            timer,
            presents: Vec::new(),
            frame: FramePlan::default(),
            timing: None,
        }
    }

    // Times each pass of the frames that follow on the GPU, for pass_times. False when the
    // device can't.
    pub fn enable_pass_timing(&mut self) -> bool {
        if !self
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            return false;
        }
        let size = MAX_PASSES as u64 * 2 * 8;
        let buffer = |label, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        use wgpu::BufferUsages as B;
        self.timing = Some(PassTiming {
            queries: self.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("pass times"),
                ty: wgpu::QueryType::Timestamp,
                count: MAX_PASSES * 2,
            }),
            resolve: buffer("pass times", B::QUERY_RESOLVE | B::COPY_SRC),
            readback: buffer("pass times readback", B::MAP_READ | B::COPY_DST),
            passes: Vec::new(),
        });
        true
    }

    // How long each pass of the last frame took on the GPU, in milliseconds, once enabled.
    // Waits for the GPU to finish the frame.
    pub fn pass_times(&self) -> Option<Vec<(&'static str, f64)>> {
        let timing = self.timing.as_ref()?;
        let slice = timing.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let period = self.queue.get_timestamp_period() as f64;
        let times = {
            let data = slice.get_mapped_range().ok()?;
            let ticks: &[u64] = bytemuck::cast_slice(&data);
            timing
                .passes
                .iter()
                .enumerate()
                .map(|(i, &name)| {
                    let ticks = ticks[2 * i + 1].saturating_sub(ticks[2 * i]);
                    (name, ticks as f64 * period / 1e6)
                })
                .collect()
        };
        timing.readback.unmap();
        Some(times)
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    // Whether translucent ghosts can be drawn; WebGL2 draws them like any bike
    pub fn translucent_ghosts(&self) -> bool {
        self.ghost.is_some()
    }

    // Largest atlas layer the device supports, capped to keep memory use sane
    pub fn atlas_limit(&self) -> usize {
        (self.device.limits().max_texture_dimension_2d as usize).min(8192)
    }

    fn texture(
        &self,
        label: &str,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    // A texture filled with `data`, `bytes_per_row` per row and `layers` layers. An empty
    // layer is left blank.
    fn upload_texture(
        &self,
        label: &str,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        layers: &[&[u8]],
        bytes_per_row: u32,
    ) -> wgpu::Texture {
        let texture = self.texture(
            label,
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: layers.len() as u32,
            },
            format,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        for (z, data) in layers.iter().enumerate().filter(|(_, d)| !d.is_empty()) {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: z as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        texture
    }

    // The scene's atlas pixels go to the GPU and aren't kept: they can take 64 MB
    pub fn upload_scene(&self, mut scene: Scene) -> GpuScene {
        let size = scene.atlas_size as u32;
        let mut layers: Vec<&[u8]> = scene.atlas_layers.iter().map(|l| l.as_slice()).collect();
        // WebGL2 makes a texture of one layer a plain one, which the shader can't read as an
        // array, so there are at least two
        if layers.len() < 2 {
            layers.resize(2, &[]);
        }
        let atlas = self.upload_texture("atlas", INDEX_FORMAT, size, size, &layers, size);
        scene.atlas_layers = Vec::new();
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let timer_map =
            self.upload_texture("timer map", INDEX_FORMAT, 256, 1, &[&scene.timer_map], 256);
        let rgba: Vec<u8> = scene
            .palette
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect();
        let palette = self.upload_texture(
            "palette",
            wgpu::TextureFormat::Rgba8Unorm,
            256,
            1,
            &[&rgba],
            1024,
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &self.scene_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.uniforms,
                        offset: 0,
                        size: NonZeroU64::new(std::mem::size_of::<ViewUniform>() as u64),
                    }),
                },
                texture_binding(1, &atlas_view),
                texture_binding(2, &timer_map.create_view(&Default::default())),
                texture_binding(3, &palette.create_view(&Default::default())),
            ],
        });

        let buffer = |label, contents: &[u8]| {
            (!contents.is_empty()).then(|| {
                let buffer = vertex_buffer(&self.device, label, contents.len() as u64);
                self.queue.write_buffer(&buffer, 0, contents);
                buffer
            })
        };
        GpuScene {
            instances: scene
                .pictures()
                .and_then(|p| buffer("level instances", bytemuck::cast_slice(&p.instances))),
            polygons: buffer("polygons", bytemuck::cast_slice(&scene.polygon_vertices)),
            scene,
            bind_group,
        }
    }

    pub fn create_targets(&self, width: u32, height: u32) -> Targets {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        use wgpu::TextureUsages as U;
        let index = self.texture(
            "index",
            size,
            INDEX_FORMAT,
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC,
        );
        let layer = self.texture(
            "layer",
            size,
            INDEX_FORMAT,
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
        );
        let ghost = self.texture(
            "ghosts",
            size,
            GHOST_FORMAT,
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
        );
        let depth = self.texture("depth", size, DEPTH_FORMAT, U::RENDER_ATTACHMENT);
        let copy = self.texture(
            "index copy",
            size,
            INDEX_FORMAT,
            U::TEXTURE_BINDING | U::COPY_DST,
        );
        let view = |t: &wgpu::Texture| t.create_view(&Default::default());
        let (index_view, layer_view, ghost_view) = (view(&index), view(&layer), view(&ghost));
        let group = |label, layout, entries: &[wgpu::BindGroupEntry]| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries,
            })
        };
        Targets {
            width,
            height,
            layers_group: group(
                "layers",
                &self.texture_layout,
                &[texture_binding(0, &layer_view)],
            ),
            timer_group: group(
                "timer",
                &self.texture_layout,
                &[texture_binding(0, &view(&copy))],
            ),
            present_group: group(
                "present",
                &self.present_layout,
                &[
                    texture_binding(0, &index_view),
                    texture_binding(1, &ghost_view),
                ],
            ),
            index,
            index_view,
            layer_view,
            ghost_view,
            depth_view: view(&depth),
            copy,
        }
    }

    fn present_pipeline(&mut self, format: wgpu::TextureFormat) -> usize {
        if let Some(i) = self.presents.iter().position(|(f, _)| *f == format) {
            return i;
        }
        let pipeline = pipeline(
            &self.device,
            &self.shader,
            PipelineDesc {
                label: "present",
                layout: &self.with_present,
                vs: "vs_fullscreen",
                buffers: &[],
                fs: "fs_present",
                targets: &[target(format, true)],
                depth: None,
                constants: &[("linearize", format.is_srgb() as u32 as f64)],
            },
        );
        self.presents.push((format, pipeline));
        self.presents.len() - 1
    }

    // Fills the output with black, for when there is nothing to show
    pub fn clear(&self, output: Output) {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[color(output.view, wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
            ..Default::default()
        });
        self.queue.submit([encoder.finish()]);
    }

    pub fn render(
        &mut self,
        scene: &GpuScene,
        targets: &Targets,
        session: &Session,
        views: &[ViewParams],
        options: &RenderOptions,
        output: Output,
    ) -> Result<(), RenderError> {
        let s = &scene.scene;
        let size = (targets.width, targets.height);
        plan(&mut self.frame, s, session, views, options, size)?;
        self.queue
            .write_buffer(&self.uniforms, 0, &self.frame.uniforms);
        upload(
            &self.device,
            &self.queue,
            &mut self.dynamic_buffer,
            "dynamic instances",
            bytemuck::cast_slice(&self.frame.instances),
        );
        upload(
            &self.device,
            &self.queue,
            &mut self.timer_buffer,
            "timer rects",
            bytemuck::cast_slice(&self.frame.timer),
        );
        let present = self.present_pipeline(output.format);
        let draws = &self.frame.views;
        let background = self.frame.background;

        let mut encoder = self.device.create_command_encoder(&Default::default());
        let set_view = |pass: &mut wgpu::RenderPass, slot: usize, [x, y, w, h]: [u32; 4]| {
            pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
            pass.set_scissor_rect(x, y, w, h);
            pass.set_bind_group(
                0,
                &scene.bind_group,
                &[(slot as u64 * UNIFORM_STRIDE) as u32],
            );
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let mut passes = Vec::new();
        let timing = self.timing.as_ref();
        let mut timestamps = |name| {
            let t = timing?;
            let i = passes.len() as u32;
            passes.push(name);
            Some(wgpu::RenderPassTimestampWrites {
                query_set: &t.queries,
                beginning_of_pass_write_index: Some(2 * i),
                end_of_pass_write_index: Some(2 * i + 1),
            })
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("level"),
                timestamp_writes: timestamps("level"),
                color_attachments: &[
                    color(&targets.index_view, clear),
                    color(&targets.layer_view, clear),
                ],
                depth_stencil_attachment: Some(depth_attachment(&targets.depth_view, true)),
                ..Default::default()
            });
            set_view(
                &mut pass,
                BACKGROUND_SLOT,
                [0, 0, targets.width, targets.height],
            );
            pass.set_vertex_buffer(0, self.dynamic_buffer.slice(..));
            pass.set_pipeline(&self.fill);
            pass.set_stencil_reference(0);
            pass.draw(0..6, background..background + 1);
            for (i, (params, draw)) in views.iter().zip(draws).enumerate() {
                set_view(&mut pass, i, params.viewport);
                if let Some(polygons) = &scene.polygons {
                    pass.set_pipeline(&self.stencil);
                    pass.set_vertex_buffer(0, polygons.slice(..));
                    pass.draw(0..s.polygon_vertices.len() as u32, 0..1);
                }
                pass.set_vertex_buffer(0, self.dynamic_buffer.slice(..));
                pass.set_pipeline(&self.fill);
                pass.set_stencil_reference(0);
                pass.draw(0..6, draw.ground_fill.clone());
                pass.set_stencil_reference(1);
                pass.draw(0..6, draw.sky_fill.clone());
                if let (Some(instances), Some(p)) = (&scene.instances, s.pictures()) {
                    pass.set_vertex_buffer(0, instances.slice(..));
                    pass.set_pipeline(&self.sprite_clipped);
                    pass.set_stencil_reference(0);
                    pass.draw(0..6, p.ground.clone());
                    pass.set_stencil_reference(1);
                    pass.draw(0..6, p.sky.clone());
                    pass.set_pipeline(&self.sprite_unclipped);
                    pass.draw(0..6, p.unclipped.clone());
                }
            }
        }

        // With pictures in the background, the bikes go over everything
        let on_top = options.dynamic.pictures_in_background;
        if !on_top {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("front grass"),
                timestamp_writes: timestamps("front grass"),
                depth_stencil_attachment: Some(depth_attachment(&targets.depth_view, false)),
                ..Default::default()
            });
            pass.set_pipeline(&self.front_grass);
            pass.set_bind_group(1, &targets.layers_group, &[]);
            for (i, params) in views.iter().enumerate() {
                set_view(&mut pass, i, params.viewport);
                pass.draw(0..3, 0..1);
            }
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bikes"),
                timestamp_writes: timestamps("bikes"),
                color_attachments: &[
                    color(&targets.index_view, wgpu::LoadOp::Load),
                    // Only read by the present pass when there are translucent ghosts
                    color_stored(&targets.ghost_view, clear, self.frame.translucent_ghosts),
                ],
                depth_stencil_attachment: Some(depth_attachment(&targets.depth_view, on_top)),
                ..Default::default()
            });
            let ghost = match (options.ghosts, &self.ghost) {
                (GhostStyle::Translucent, Some(ghost)) => ghost,
                _ => &self.dynamic,
            };
            pass.set_vertex_buffer(0, self.dynamic_buffer.slice(..));
            for (i, (params, draw)) in views.iter().zip(draws).enumerate() {
                set_view(&mut pass, i, params.viewport);
                // Objects, then ghosts, then the replay's own bikes
                pass.set_pipeline(&self.dynamic);
                pass.draw(0..6, draw.dynamic.start..draw.ghosts.start);
                pass.set_pipeline(ghost);
                pass.draw(0..6, draw.ghosts.clone());
                pass.set_pipeline(&self.dynamic);
                pass.draw(0..6, draw.ghosts.end..draw.dynamic.end);
            }
        }

        if draws.iter().any(|d| d.minimap.is_some()) {
            // Its own stencil and depth: the level and bikes are done with theirs
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("minimap"),
                timestamp_writes: timestamps("minimap"),
                color_attachments: &[
                    color(&targets.index_view, wgpu::LoadOp::Load),
                    color(&targets.layer_view, wgpu::LoadOp::Load),
                ],
                depth_stencil_attachment: Some(depth_attachment(&targets.depth_view, true)),
                ..Default::default()
            });
            for (i, draw) in draws.iter().enumerate() {
                let Some(m) = &draw.minimap else {
                    continue;
                };
                pass.set_vertex_buffer(0, self.dynamic_buffer.slice(..));
                set_view(&mut pass, MINIMAP_BORDER_SLOT + i, m.border);
                pass.set_pipeline(&self.fill);
                pass.set_stencil_reference(0);
                pass.draw(0..6, m.fills..m.fills + 1);
                set_view(&mut pass, MINIMAP_SLOT + i, m.inner);
                if let Some(polygons) = &scene.polygons {
                    pass.set_pipeline(&self.stencil);
                    pass.set_vertex_buffer(0, polygons.slice(..));
                    pass.draw(0..s.polygon_vertices.len() as u32, 0..1);
                    pass.set_vertex_buffer(0, self.dynamic_buffer.slice(..));
                }
                pass.set_pipeline(&self.fill);
                pass.set_stencil_reference(0);
                pass.draw(0..6, m.fills + 1..m.fills + 2);
                pass.set_stencil_reference(1);
                pass.draw(0..6, m.fills + 2..m.fills + 3);
                pass.set_pipeline(&self.sprite_unclipped);
                pass.draw(0..6, m.icons.clone());
            }
        }

        if draws.iter().any(|d| !d.timer.is_empty()) {
            if let Some([x, y, width, height]) = self.frame.timer_copy {
                let origin = wgpu::Origin3d { x, y, z: 0 };
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        origin,
                        ..targets.index.as_image_copy()
                    },
                    wgpu::TexelCopyTextureInfo {
                        origin,
                        ..targets.copy.as_image_copy()
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("timer"),
                timestamp_writes: timestamps("timer"),
                color_attachments: &[color(&targets.index_view, wgpu::LoadOp::Load)],
                ..Default::default()
            });
            pass.set_pipeline(&self.timer);
            pass.set_bind_group(1, &targets.timer_group, &[]);
            pass.set_vertex_buffer(0, self.timer_buffer.slice(..));
            for (i, (params, draw)) in views.iter().zip(draws).enumerate() {
                set_view(&mut pass, i, params.viewport);
                pass.draw(0..6, draw.timer.clone());
            }
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                timestamp_writes: timestamps("present"),
                color_attachments: &[color(output.view, wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
                ..Default::default()
            });
            pass.set_pipeline(&self.presents[present].1);
            // The background slot, which says whether there are translucent ghosts
            pass.set_bind_group(
                0,
                &scene.bind_group,
                &[(BACKGROUND_SLOT as u64 * UNIFORM_STRIDE) as u32],
            );
            pass.set_bind_group(1, &targets.present_group, &[]);
            pass.draw(0..3, 0..1);
        }

        if let Some(t) = &mut self.timing {
            let count = passes.len() as u32 * 2;
            encoder.resolve_query_set(&t.queries, 0..count, &t.resolve, 0);
            encoder.copy_buffer_to_buffer(&t.resolve, 0, &t.readback, 0, count as u64 * 8);
            t.passes = passes;
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }
}
