use render::wgpu;
use render::{GpuScene, Output, RenderError, RenderOptions, Renderer, Targets, ViewParams};
use replay::Session;

const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

// Renders frames offscreen and reads them back as packed RGB
pub struct Headless {
    pub renderer: Renderer,
    targets: Targets,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row: u32,
    rgb: Vec<u8>,
}

impl Headless {
    pub fn new(width: u32, height: u32) -> Result<Headless, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let (renderer, _) = pollster::block_on(Renderer::request(&instance, None, "rec2vid"))
            .map_err(|e| e.to_string())?;
        let device = renderer.device();
        let targets = renderer.create_targets(width, height);
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output.create_view(&Default::default());
        let padded_row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Headless {
            renderer,
            targets,
            output,
            output_view,
            readback,
            padded_row,
            rgb: vec![0; width as usize * height as usize * 3],
        })
    }

    pub fn render(
        &mut self,
        scene: &GpuScene,
        session: &Session,
        views: &[ViewParams],
        options: &RenderOptions,
    ) -> Result<&[u8], RenderError> {
        self.renderer.render(
            scene,
            &self.targets,
            session,
            views,
            options,
            Output {
                view: &self.output_view,
                format: OUTPUT_FORMAT,
            },
        )?;
        let (width, height) = (self.targets.width, self.targets.height);
        let device = self.renderer.device();
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.renderer.queue().submit([encoder.finish()]);
        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| {
            result.expect("failed to read the frame")
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU stopped responding");
        {
            let data = slice.get_mapped_range().expect("failed to map the frame");
            for y in 0..height as usize {
                let row = &data[y * self.padded_row as usize..][..width as usize * 4];
                let out = &mut self.rgb[y * width as usize * 3..][..width as usize * 3];
                for (o, p) in out
                    .as_chunks_mut::<3>()
                    .0
                    .iter_mut()
                    .zip(row.as_chunks::<4>().0)
                {
                    o.copy_from_slice(&p[..3]);
                }
            }
        }
        self.readback.unmap();
        Ok(&self.rgb)
    }

    pub fn last_frame(&self) -> &[u8] {
        &self.rgb
    }
}
