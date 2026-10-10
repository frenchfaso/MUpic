//! Optional native GPU hint planner. The same WGSL runs in the converter page.
//! CPU f64 refinement remains authoritative; unavailable GPUs return an error
//! which the CLI/portable quantizer handles by retaining its CPU path.
use crate::author_dither::AuthorDither;
use std::num::NonZeroU64;
use std::sync::{mpsc, Mutex};
use wgpu::util::DeviceExt;

pub const SHADER: &str = include_str!("../shaders/author-seed.wgsl");
const BATCH_KEYS: usize = 65_536;

pub struct GpuPlanner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    tree: wgpu::Buffer,
    slots: Mutex<[Buffers; 2]>,
    pub adapter_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(rgb: [u8; 3]) -> u32 {
        u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2])
    }

    #[test]
    fn native_hints_preserve_exact_plans_and_reuse_pipeline_buffers() {
        let mut dither = AuthorDither::new(&crate::PALETTE, true).unwrap();
        dither.set_threads(1);
        let gpu = match GpuPlanner::new(&dither) {
            Ok(gpu) => gpu,
            Err(error)
                if error.starts_with("GPU adapter unavailable:")
                    || error.starts_with("GPU device unavailable:") =>
            {
                eprintln!("Skipping GPU runtime test: {error}");
                return;
            }
            Err(error) => panic!("GPU shader must compile: {error}"),
        };
        let palette: Vec<_> = crate::PALETTE.iter().copied().map(key).collect();
        let mut keys = palette.clone();
        keys.extend((0..=255).step_by(17).map(|v| key([v, v, v])));
        let mut random = 0x6d757069u32;
        for _ in 0..64 {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            keys.push(random & 0xffffff);
        }
        let expected = dither.plan_keys(&keys, None).unwrap();
        let hints = gpu.seeds(&keys).unwrap();
        assert_eq!(hints.len(), keys.len());
        let tree = dither.gpu_search_data();
        for (&rgb, &hint) in keys.iter().zip(&hints) {
            let candidate = hint & 0x1ffff;
            let leaf = hint >> 17;
            assert!(candidate < tree[1]);
            assert!(leaf < tree[0]);
            let base = tree[6] as usize + leaf as usize * 12;
            assert_eq!(tree[base + 8], u32::MAX);
            let indices =
                &tree[(tree[4] + tree[base + 3]) as usize..(tree[4] + tree[base + 7]) as usize];
            assert!(indices.contains(&candidate));
            let bytes = [(rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255];
            if bytes.iter().max().unwrap() - bytes.iter().min().unwrap() <= 8 {
                assert_eq!(tree[tree[5] as usize + candidate as usize * 4 + 3], 1);
            }
        }
        assert_eq!(dither.plan_keys(&keys, Some(&hints)).unwrap(), expected);
        assert_eq!(
            dither
                .plan_keys(&keys, Some(&vec![u32::MAX; keys.len()]))
                .unwrap(),
            expected
        );

        let reused = gpu.seeds(&keys[..1]).unwrap();
        assert_eq!(reused.len(), 1);
        assert_eq!(
            dither.plan_keys(&keys[..1], Some(&reused)).unwrap(),
            expected[..1]
        );
        // Palette keys make a multi-slot runtime check cheap on the CPU while
        // still exercising both GPU batches, maps and subsequent buffer reuse.
        let pipeline_keys: Vec<_> = palette
            .iter()
            .copied()
            .cycle()
            .take(BATCH_KEYS + 17)
            .collect();
        gpu.preplan_keys(&mut dither, &pipeline_keys).unwrap();
        assert!(dither.missing_rgb_keys(&crate::PALETTE.concat()).is_empty());
        assert_eq!(
            dither
                .plan_keys(&keys, Some(&gpu.seeds(&keys).unwrap()))
                .unwrap(),
            expected
        );
        assert!(gpu.preplan_keys(&mut dither, &[0x1000000]).is_err());
    }
}

struct Buffers {
    input: wgpu::Buffer,
    output: wgpu::Buffer,
    readback: wgpu::Buffer,
}

struct Submitted {
    index: wgpu::SubmissionIndex,
    ready: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    size: u64,
}

impl Buffers {
    fn new(device: &wgpu::Device) -> Self {
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (BATCH_KEYS * 4) as u64,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            input: buffer(
                "MUpic unique RGB keys",
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
            output: buffer(
                "MUpic candidate hints",
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            ),
            readback: buffer(
                "MUpic hint readback",
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
        }
    }
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
        let slots = Mutex::new(std::array::from_fn(|_| Buffers::new(&device)));
        Ok(Self {
            device,
            queue,
            pipeline,
            tree,
            slots,
            adapter_name,
        })
    }

    pub fn preplan_keys(&self, dither: &mut AuthorDither, keys: &[u32]) -> Result<(), String> {
        if keys.iter().any(|&key| key > 0xffffff) {
            return Err("Invalid GPU RGB key".into());
        }
        if keys.is_empty() {
            return Ok(());
        }
        let slots = self.slots.lock().map_err(|e| e.to_string())?;
        let mut batches = keys.chunks(BATCH_KEYS).enumerate();
        let (_, mut batch) = batches.next().unwrap();
        let mut slot = 0;
        let mut submitted = self.submit(&slots[slot], batch);
        loop {
            let seeds = self.collect(&slots[slot], submitted)?;
            // Prepare the next GPU batch before exact CPU refinement. The
            // independent slots keep its input/readback alive during CPU work.
            let next = batches.next().map(|(index, next_batch)| {
                let next_slot = index % slots.len();
                (
                    next_slot,
                    next_batch,
                    self.submit(&slots[next_slot], next_batch),
                )
            });
            dither.preplan_keys_seeded(batch, &seeds)?;
            let Some((next_slot, next_batch, next_submission)) = next else {
                break;
            };
            slot = next_slot;
            batch = next_batch;
            submitted = next_submission;
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
        let slots = self.slots.lock().map_err(|e| e.to_string())?;
        self.collect(&slots[0], self.submit(&slots[0], keys))
    }

    fn submit(&self, buffers: &Buffers, keys: &[u32]) -> Submitted {
        let size = (keys.len() * 4) as u64;
        self.queue
            .write_buffer(&buffers.input, 0, bytemuck::cast_slice(keys));
        let binding = |buffer| {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: NonZeroU64::new(size),
            })
        };
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
                    resource: binding(&buffers.input),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: binding(&buffers.output),
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
        encoder.copy_buffer_to_buffer(&buffers.output, 0, &buffers.readback, 0, size);
        let index = self.queue.submit([encoder.finish()]);
        let (sender, ready) = mpsc::channel();
        buffers
            .readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        Submitted { index, ready, size }
    }

    fn collect(&self, buffers: &Buffers, submitted: Submitted) -> Result<Vec<u32>, String> {
        let result = (|| {
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submitted.index),
                    timeout: Some(std::time::Duration::from_secs(30)),
                })
                .map_err(|e| e.to_string())?;
            submitted
                .ready
                .recv()
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            Ok(bytemuck::cast_slice::<u8, u32>(
                &buffers
                    .readback
                    .slice(..submitted.size)
                    .get_mapped_range()
                    .map_err(|e| e.to_string())?,
            )
            .to_vec())
        })();
        buffers.readback.unmap();
        result
    }
}
