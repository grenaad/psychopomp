//! The editor card's projected composite on the GPU.
//!
//! The editor background uploads once per theme, the card's content and
//! overlay layers upload only when `render_editor_full` repaints them, and
//! one full-screen pass (`gpu_card.wgsl`) reproduces `composite_card_layers`
//! over the background, in 8-bit straight-alpha steps like `blend_pixel`. The
//! result reads back for the CPU overlays. Only the editor card uses this;
//! every other card still composites on the CPU.

use std::sync::mpsc;

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};

use super::{BYTES_PER_PIXEL, COPY_ROW_ALIGNMENT, HeadlessRenderer, ui};

/// The card output's storage format: bytes, blended exactly like `blend_pixel`.
const BYTES: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CardUniforms {
    inverse: [[f32; 4]; 3],
    center_size: [f32; 4],
    style: [f32; 4],
    depth: [f32; 4],
    shadow: [f32; 4],
    border_color: [f32; 4],
    shell_box: [f32; 4],
    overlay_box: [f32; 4],
}

struct Layers {
    size: [u32; 2],
    content: wgpu::Texture,
    overlay: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

pub(super) struct GpuCard {
    size: [u32; 2],
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    background: wgpu::Texture,
    pub(super) background_ready: bool,
    layers: Option<Layers>,
    output: wgpu::Texture,
    pub(super) output_view: wgpu::TextureView,
    readback: Readback,
}

pub(super) struct Readback {
    buffer: wgpu::Buffer,
    padded_bytes_per_row: u32,
    size: [u32; 2],
}

impl Readback {
    pub(super) fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let padded_bytes_per_row =
            (size[0] * BYTES_PER_PIXEL).div_ceil(COPY_ROW_ALIGNMENT) * COPY_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU readback"),
            size: u64::from(padded_bytes_per_row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            padded_bytes_per_row,
            size,
        }
    }

    /// Copy `texture` into the buffer in `encoder`, submit, and return its bytes.
    pub(super) fn read(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Result<Vec<u8>> {
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.size[1]),
                },
            },
            extent(self.size),
        );
        queue.submit([encoder.finish()]);
        let slice = self.buffer.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .context("wait for GPU frame")?;
        receiver
            .recv()
            .context("receive GPU readback result")?
            .context("map GPU readback buffer")?;
        let row = self.size[0] as usize * BYTES_PER_PIXEL as usize;
        let bytes = slice.get_mapped_range().context("read mapped GPU frame")?;
        let mut frame = Vec::with_capacity(row * self.size[1] as usize);
        for padded in bytes
            .chunks_exact(self.padded_bytes_per_row as usize)
            .take(self.size[1] as usize)
        {
            frame.extend_from_slice(&padded[..row]);
        }
        drop(bytes);
        self.buffer.unmap();
        Ok(frame)
    }
}

pub(super) fn extent(size: [u32; 2]) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    }
}

pub(super) fn byte_texture(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    view_formats: &[wgpu::TextureFormat],
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: extent(size),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats,
    })
}

pub(super) fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture, size: [u32; 2], bytes: &[u8]) {
    queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * BYTES_PER_PIXEL),
            rows_per_image: Some(size[1]),
        },
        extent(size),
    );
}

impl GpuCard {
    fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("gpu_card.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("editor card pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(BYTES.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("editor card uniforms"),
            size: std::mem::size_of::<CardUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampled = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let background = byte_texture(device, "editor card background", size, BYTES, sampled, &[]);
        let output = byte_texture(
            device,
            "editor card output",
            size,
            BYTES,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            &[],
        );
        let output_view = output.create_view(&Default::default());
        Self {
            size,
            pipeline,
            uniforms,
            background,
            background_ready: false,
            layers: None,
            output,
            output_view,
            readback: Readback::new(device, size),
        }
    }

    fn upload_layers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        content: &[u8],
        overlay: &[u8],
    ) {
        if self
            .layers
            .as_ref()
            .is_none_or(|layers| layers.size != size)
        {
            let sampled = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
            let content = byte_texture(device, "editor card content", size, BYTES, sampled, &[]);
            let overlay = byte_texture(device, "editor card overlay", size, BYTES, sampled, &[]);
            let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
            let background = view(&self.background);
            let (content_view, overlay_view) = (view(&content), view(&overlay));
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("editor card bind group"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&background),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&content_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&overlay_view),
                    },
                ],
            });
            self.layers = Some(Layers {
                size,
                content,
                overlay,
                bind_group,
            });
        }
        let layers = self.layers.as_ref().expect("layers prepared");
        upload(queue, &layers.content, size, content);
        upload(queue, &layers.overlay, size, overlay);
    }

    fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: ui::card::CardFrame,
    ) -> wgpu::CommandEncoder {
        let terms = ui::card::gpu_card_terms(frame, self.size);
        let center = frame.bounds.center();
        let row = |r: [f32; 3]| [r[0], r[1], r[2], 0.0];
        let to_box = |b: [i32; 4]| b.map(|v| v as f32);
        let uniforms = CardUniforms {
            inverse: terms.inverse.map(row),
            center_size: [
                center[0],
                center[1],
                frame.bounds.size[0],
                frame.bounds.size[1],
            ],
            style: [
                frame.style.corner_radius,
                frame.opacity,
                frame.projection.surface_blur,
                frame.projection.near_edge_blur,
            ],
            depth: [
                terms.depth[0],
                terms.depth[1],
                terms.max_near_depth,
                frame.style.border_width,
            ],
            shadow: [
                frame.style.shadow_offset[0],
                frame.style.shadow_offset[1],
                frame.style.shadow_blur,
                frame.style.shadow_opacity,
            ],
            border_color: terms.border_color.map(|v| f32::from(v) / 255.0),
            shell_box: to_box(terms.boxes[0]),
            overlay_box: to_box(terms.boxes[1]),
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("editor card composite"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("editor card pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(
                0,
                &self.layers.as_ref().expect("layers uploaded").bind_group,
                &[],
            );
            pass.draw(0..3, 0..1);
        }
        encoder
    }
}

impl HeadlessRenderer {
    /// `ui.card_painted` over the editor background, on the GPU. Uploads the
    /// card's layers when `repainted` (or when none are on the GPU yet) and
    /// returns the composited frame.
    pub(super) fn gpu_composite_editor_card(
        &mut self,
        frame: ui::card::CardFrame,
        content: &[u8],
        overlay: &[u8],
        repainted: bool,
    ) -> Result<Vec<u8>> {
        let size = [self.spec.width, self.spec.height];
        let card = self
            .gpu_card
            .get_or_insert_with(|| GpuCard::new(&self.device, size));
        if !card.background_ready {
            // The bind group references the texture, not its contents, so a
            // new background invalidates nothing else.
            upload(
                &self.queue,
                &card.background,
                size,
                &self.editor_background_pixels,
            );
            card.background_ready = true;
        }
        let local_size = ui::card::gpu_card_terms(frame, size).local_size;
        if repainted
            || card
                .layers
                .as_ref()
                .is_none_or(|layers| layers.size != local_size)
        {
            card.upload_layers(&self.device, &self.queue, local_size, content, overlay);
        }
        let encoder = card.encode(&self.device, &self.queue, frame);
        card.readback
            .read(&self.device, &self.queue, encoder, &card.output)
    }
}

// `Card` in `gpu_card.wgsl`; `wgsl_tests` checks the shader's side.
const _: () = assert!(std::mem::size_of::<CardUniforms>() == 160);

#[cfg(test)]
mod tests {
    use super::super::{HeadlessRenderer, RenderSpec, ui};

    /// Deterministic straight-alpha noise, with transparent and opaque runs.
    fn pattern(size: [u32; 2], seed: u32) -> Vec<u8> {
        (0..size[0] * size[1] * 4)
            .map(|index| {
                let value = index.wrapping_mul(2_654_435_761).wrapping_add(seed) >> 13;
                match (index / 4) % 97 {
                    0..=20 if index % 4 == 3 => 0,
                    21..=50 if index % 4 == 3 => 255,
                    _ => value as u8,
                }
            })
            .collect()
    }

    #[test]
    #[ignore = "requires a headless GPU; checks the GPU editor card against the CPU compositor"]
    fn gpu_editor_card_matches_the_cpu_compositor_within_one_level() {
        let size = [480, 270];
        let mut renderer = pollster::block_on(HeadlessRenderer::new(RenderSpec {
            width: size[0],
            height: size[1],
            file_name: "gpu-card-proof".into(),
        }))
        .unwrap();
        renderer.editor_background_pixels = pattern(size, 7)
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect();
        let mut style = ui::card::CardStyle::standard();
        style.border_width = 0.75;
        style.shadow_blur = 12.0;
        for (index, projection) in [
            ui::card::CardProjection::default(),
            ui::card::CardProjection {
                scale: 0.85,
                rotation_z: 0.04,
                tilt_x: 0.25,
                tilt_y: -0.35,
                surface_blur: 0.6,
                near_edge_blur: 1.5,
            },
        ]
        .into_iter()
        .enumerate()
        {
            let frame = ui::card::CardFrame {
                bounds: ui::Bounds::from_center([240.3, 135.2], [400.0, 225.0]),
                style,
                projection,
                opacity: 0.8,
            };
            let local = ui::card::gpu_card_terms(frame, size).local_size;
            let content = pattern(local, 11 + index as u32);
            let overlay = pattern(local, 23 + index as u32);
            let gpu = renderer
                .gpu_composite_editor_card(frame, &content, &overlay, true)
                .unwrap();
            let mut cpu = renderer.editor_background_pixels.clone();
            renderer
                .composite_ui(&mut cpu, |ui| ui.card_painted(frame, &content, &overlay))
                .unwrap();
            let worst = gpu
                .iter()
                .zip(&cpu)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            let off = gpu
                .iter()
                .zip(&cpu)
                .filter(|(a, b)| a.abs_diff(**b) > 1)
                .count();
            // Bilinear taps of this high-frequency noise round a few channels
            // two levels apart; the editor's real layers stay within one.
            assert!(
                worst <= 2 && off * 1000 < gpu.len(),
                "projection {index}: max difference {worst}, {off} channels over one level"
            );
        }
    }
}
