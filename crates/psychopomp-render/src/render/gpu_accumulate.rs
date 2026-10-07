//! Shutter samples of a CPU-composed root (anything but a Stage), accumulated
//! on the GPU as the Stage's are.
//!
//! Each sample uploads to a byte texture, or, for an editor card kept on the
//! GPU, is read in place from the card's output under a cached overlay
//! layer. Its linear light is added with its weight into an `Rgba32Float`
//! sum, which the resolve pass encodes to sRGB bytes as
//! `exposure::encode_linear` does, read back once per frame. Blending that
//! sum needs `FLOAT32_BLENDABLE`; without it `exposure::accumulate` sums on the
//! CPU instead. `Rgba16Float`, which the Stage uses, is not a fallback here: a
//! 16-sample sum's alpha rounds to 254.

use anyhow::{Result, bail};

use super::{
    HeadlessRenderer,
    gpu_card::{Readback, byte_texture, upload},
};

/// The sum's format; blending into it needs `FLOAT32_BLENDABLE`.
const SUM: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

/// Whether `samples` shutter samples accumulate on the GPU on a device with
/// `features`. One sample needs no accumulation, and a device that cannot
/// blend an `Rgba32Float` sum leaves it to `exposure::accumulate`.
pub(crate) fn accumulates_on_gpu(features: wgpu::Features, samples: usize) -> bool {
    samples > 1 && features.contains(wgpu::Features::FLOAT32_BLENDABLE)
}

/// One shutter sample handed to the GPU accumulator.
pub(crate) enum GpuSample {
    /// Finished sRGB bytes to upload.
    Bytes(Vec<u8>),
    /// The editor card's composite, kept on the GPU, under the overlay layer
    /// last passed to `set_gpu_overlay` when `overlay` is set.
    ResidentEditorCard { overlay: bool },
}

pub(super) struct GpuAccumulator {
    size: [u32; 2],
    layout: wgpu::BindGroupLayout,
    add: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    weight: wgpu::Buffer,
    upload: wgpu::Texture,
    upload_view: wgpu::TextureView,
    overlay: wgpu::Texture,
    overlay_view: wgpu::TextureView,
    overlay_bytes: Vec<u8>,
    sum_view: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    readback: Readback,
}

impl GpuAccumulator {
    fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("gpu_accumulate.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("accumulate layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("accumulate pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, format: wgpu::TextureFormat, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
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
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let add = pipeline(
            "add_main",
            SUM,
            Some(wgpu::BlendState {
                color: additive,
                alpha: additive,
            }),
        );
        let resolve = pipeline("resolve_main", wgpu::TextureFormat::Rgba8Unorm, None);
        let weight = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("accumulate weight"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let upload = byte_texture(
            device,
            "accumulate sample",
            size,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            &[],
        );
        let overlay = byte_texture(
            device,
            "accumulate overlay",
            size,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            &[],
        );
        let sum = byte_texture(
            device,
            "accumulate sum",
            size,
            SUM,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            &[],
        );
        let output = byte_texture(
            device,
            "accumulate output",
            size,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            &[],
        );
        let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
        Self {
            size,
            layout,
            add,
            resolve,
            weight,
            upload_view: view(&upload),
            upload,
            overlay_view: view(&overlay),
            overlay,
            overlay_bytes: Vec::new(),
            sum_view: view(&sum),
            output_view: view(&output),
            output,
            readback: Readback::new(device, size),
        }
    }

    fn bind(&self, device: &wgpu::Device, texture: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("accumulate bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.weight.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.overlay_view),
                },
            ],
        })
    }
}

fn pass(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    clear: bool,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("accumulate pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if clear {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

impl HeadlessRenderer {
    /// Whether a frame of `samples` shutter samples accumulates on the GPU.
    pub(crate) fn accumulates_on_gpu(&self, samples: usize) -> bool {
        accumulates_on_gpu(self.device.features(), samples)
    }

    /// The CPU overlay layer resident samples blend under: uploaded only when
    /// it differs from the last one. Returns whether anything inks it.
    pub(crate) fn set_gpu_overlay(&mut self, pixels: &[u8]) -> bool {
        if pixels.iter().all(|&byte| byte == 0) {
            return false;
        }
        let size = [self.spec.width, self.spec.height];
        let accumulator = self
            .gpu_accumulator
            .get_or_insert_with(|| GpuAccumulator::new(&self.device, size));
        if accumulator.overlay_bytes != pixels {
            upload(&self.queue, &accumulator.overlay, size, pixels);
            accumulator.overlay_bytes.clear();
            accumulator.overlay_bytes.extend_from_slice(pixels);
        }
        true
    }

    /// `exposure::accumulate` on the GPU: one upload (or none, for a
    /// resident sample) per sample, one readback per frame. Callers check
    /// `accumulates_on_gpu` first.
    pub(crate) fn gpu_accumulate(
        &mut self,
        exposure: &[(f64, f32)],
        mut render_sample: impl FnMut(&mut Self, f64) -> Result<GpuSample>,
    ) -> Result<Vec<u8>> {
        let size = [self.spec.width, self.spec.height];
        let bytes = size[0] as usize * size[1] as usize * 4;
        for (index, &(time, weight)) in exposure.iter().enumerate() {
            let sample = render_sample(self, time)?;
            let accumulator = self
                .gpu_accumulator
                .get_or_insert_with(|| GpuAccumulator::new(&self.device, size));
            debug_assert_eq!(accumulator.size, size);
            let mut overlay = false;
            let bind_group = match &sample {
                GpuSample::Bytes(pixels) => {
                    if pixels.len() != bytes {
                        bail!("sample has {} bytes, expected {bytes}", pixels.len());
                    }
                    upload(&self.queue, &accumulator.upload, size, pixels);
                    accumulator.bind(&self.device, &accumulator.upload_view)
                }
                GpuSample::ResidentEditorCard { overlay: drawn } => {
                    let Some(card) = &self.gpu_card else {
                        bail!("resident sample without a GPU editor card");
                    };
                    overlay = *drawn;
                    accumulator.bind(&self.device, &card.output_view)
                }
            };
            self.queue.write_buffer(
                &accumulator.weight,
                0,
                bytemuck::bytes_of(&[weight, f32::from(u8::from(overlay)), 0.0, 0.0]),
            );
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("accumulate sample"),
                });
            pass(
                &mut encoder,
                &accumulator.sum_view,
                index == 0,
                &accumulator.add,
                &bind_group,
            );
            self.queue.submit([encoder.finish()]);
        }
        let accumulator = self.gpu_accumulator.as_ref().expect("accumulator prepared");
        let bind_group = accumulator.bind(&self.device, &accumulator.sum_view);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("accumulate resolve"),
            });
        pass(
            &mut encoder,
            &accumulator.output_view,
            true,
            &accumulator.resolve,
            &bind_group,
        );
        accumulator
            .readback
            .read(&self.device, &self.queue, encoder, &accumulator.output)
    }
}

#[cfg(test)]
mod tests {
    use super::{super::HeadlessRenderer, GpuSample, accumulates_on_gpu};

    #[test]
    fn only_multi_sample_frames_on_a_float32_blending_device_accumulate_on_the_gpu() {
        let blendable = wgpu::Features::FLOAT32_BLENDABLE | wgpu::Features::TIMESTAMP_QUERY;
        assert!(accumulates_on_gpu(blendable, 2));
        assert!(accumulates_on_gpu(blendable, 16));
        // One sample needs no accumulation.
        assert!(!accumulates_on_gpu(blendable, 1));
        assert!(!accumulates_on_gpu(blendable, 0));
        // Without the feature, the CPU sums; there is no `Rgba16Float` path.
        assert!(!accumulates_on_gpu(wgpu::Features::empty(), 16));
        assert!(!accumulates_on_gpu(wgpu::Features::TIMESTAMP_QUERY, 16));
    }

    #[test]
    #[ignore = "requires a headless GPU; checks GPU accumulation against the CPU sum"]
    fn gpu_accumulation_matches_the_cpu_sum() {
        let mut renderer = pollster::block_on(HeadlessRenderer::new(super::super::RenderSpec {
            width: 1920,
            height: 1080,
            file_name: "gpu-accumulate-proof".into(),
        }))
        .unwrap();
        if !renderer.accumulates_on_gpu(16) {
            eprintln!("skipped: the adapter cannot blend an Rgba32Float sum");
            return;
        }
        let sample = |time: f64| -> Vec<u8> {
            let seed = (time * 1000.0) as u32;
            (0..1920 * 1080 * 4_u32)
                .map(|index| (index.wrapping_mul(2_654_435_761).wrapping_add(seed) >> 11) as u8)
                .collect()
        };
        let exposure: Vec<(f64, f32)> = (0..16)
            .map(|index| (f64::from(index) * 0.001, 1.0 / 16.0))
            .collect();
        let cpu = crate::exposure::accumulate(&mut renderer, &exposure, |_, time| Ok(sample(time)))
            .unwrap();
        let gpu = renderer
            .gpu_accumulate(&exposure, |_, time| Ok(GpuSample::Bytes(sample(time))))
            .unwrap();
        let worst = gpu
            .iter()
            .zip(&cpu)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 1, "max difference {worst}");
    }
}
