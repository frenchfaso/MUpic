//! Optional native GPU hint planner. The same WGSL runs in the converter page.
//! CPU f64 refinement remains authoritative; unavailable GPUs return an error
//! which the CLI/portable quantizer handles by retaining its CPU path.
use crate::author_dither::AuthorDither;
use std::sync::mpsc;
use wgpu::util::DeviceExt;

pub const SHADER: &str = include_str!("../shaders/author-seed.wgsl");
const BATCH_KEYS: usize = 65_536;

pub struct GpuPlanner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    tree: wgpu::Buffer,
    pub adapter_name: String,
}

impl GpuPlanner {
    pub fn new(dither: &AuthorDither) -> Result<Self, String> {
        pollster::block_on(Self::create(dither))
    }

    async fn create(dither: &AuthorDither) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("GPU adapter unavailable: {e}"))?;
        let adapter_name = adapter.get_info().name;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("MUpic author planner"),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("GPU device unavailable: {e}"))?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("MUpic author seed shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("MUpic author seed search"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(error) = scope.pop().await {
            return Err(format!("GPU shader unavailable: {error}"));
        }
        let tree_data = dither.gpu_search_data();
        if tree_data.is_empty() {
            return Err("Palette candidate tree exceeds the compact GPU hint format".into());
        }
        let tree = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("MUpic candidate tree"),
            contents: bytemuck::cast_slice(&tree_data),
            usage: wgpu::BufferUsages::STORAGE,
        });
        Ok(Self {
            device,
            queue,
            pipeline,
            tree,
            adapter_name,
        })
    }

    pub fn preplan(&self, dither: &mut AuthorDither, pixels: &[u8]) -> Result<(), String> {
        let keys = dither.missing_rgb_keys(pixels);
        // De-duplication precedes GPU submission: a long clip's common RGB
        // values are searched once, not once for every displayed pixel.
        for batch in keys.chunks(BATCH_KEYS) {
            let seeds = self.seeds(batch)?;
            dither.preplan_keys_seeded(batch, &seeds)?;
        }
        Ok(())
    }

    pub fn seeds(&self, keys: &[u32]) -> Result<Vec<u32>, String> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        if keys.len() > BATCH_KEYS {
            return Err("GPU seed batch exceeds 65536 keys".into());
        }
        let size = (keys.len() * 4) as u64;
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("MUpic unique RGB keys"),
                contents: bytemuck::cast_slice(keys),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("MUpic candidate hints"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("MUpic hint readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("MUpic hint inputs"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.tree.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(keys.len().div_ceil(64) as u32, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
        let submission = self.queue.submit([encoder.finish()]);
        let (sender, receiver) = mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(|e| e.to_string())?;
        receiver
            .recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let result = bytemuck::cast_slice::<u8, u32>(
            &readback
                .slice(..)
                .get_mapped_range()
                .map_err(|e| e.to_string())?,
        )
        .to_vec();
        readback.unmap();
        Ok(result)
    }
}
