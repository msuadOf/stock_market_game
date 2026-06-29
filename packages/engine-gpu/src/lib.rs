//! GPU 计算后端（wgpu 22 compute shader，ADR-0008）。
//!
//! V 演化 GPU 实现（整数定点）。NPC decide 待后续。
//! 确定性：全程 i32 整数 + 银行家舍入，跨 GPU 厂商 bit 一致。

use engine::compute::ComputeBackend;
use engine::market::{Market, VParams};
use engine::money::Money;
use engine::strategy::{Intent, MarketView, SelfView, StrategyData};

/// WGSL V 输入（32 bytes，与 WGSL struct 对齐）。
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct VInputGpu {
    v_cents: i32,
    mean_cents: i32,
    mean_reversion_bp: i32,
    volatility_bp: i32,
    seed: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

/// WGSL V 输出（16 bytes）。
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Default)]
struct VOutputGpu {
    new_v_cents: i32,
    error: i32,
    _pad0: u32,
    _pad1: u32,
}

pub struct GpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    v_pipeline: wgpu::ComputePipeline,
    v_bind_layout: wgpu::BindGroupLayout,
}

impl GpuBackend {
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            },
        ))?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("engine-gpu"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        )).ok()?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("evolve_v.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/evolve_v.wgsl").into()),
        });

        let v_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("evolve_v_bind"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("evolve_v_layout"),
            bind_group_layouts: &[&v_bind_layout],
            push_constant_ranges: &[],
        });

        let v_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("evolve_v_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "evolve_v_main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        Some(GpuBackend { device, queue, v_pipeline, v_bind_layout })
    }
}

impl ComputeBackend for GpuBackend {
    fn name(&self) -> &'static str { "gpu" }

    fn evolve_v_all(&self, markets: &mut [Market], params: &VParams, seeds: &[u64]) {
        let n = markets.len();
        if n == 0 { return; }

        let inputs: Vec<VInputGpu> = markets.iter().enumerate().map(|(i, m)| VInputGpu {
            v_cents: m.fundamental_value().cents() as i32,
            mean_cents: params.long_run_mean.cents() as i32,
            mean_reversion_bp: (params.mean_reversion * 10000.0) as i32,
            volatility_bp: (params.volatility * 10000.0) as i32,
            seed: seeds.get(i).map(|s| *s as u32).unwrap_or(0),
            _pad0: 0, _pad1: 0, _pad2: 0,
        }).collect();

        let input_bytes = bytemuck::cast_slice(&inputs);
        let input_size = input_bytes.len() as wgpu::BufferAddress;
        let output_size = (n * std::mem::size_of::<VOutputGpu>()) as wgpu::BufferAddress;

        let input_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("v_input"), size: input_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&input_buf, 0, input_bytes);

        let output_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("v_output"), size: output_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("v_staging"), size: output_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("v_bind"), layout: &self.v_bind_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: input_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: output_buf.as_entire_binding() },
            ],
        });

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("evolve_v_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("evolve_v_pass"), timestamp_writes: None,
            });
            pass.set_pipeline(&self.v_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(n as u32, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output_buf, 0, &staging_buf, 0, output_size);
        self.queue.submit(Some(encoder.finish()));

        staging_buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);

        let data = staging_buf.slice(..).get_mapped_range();
        let results: &[VOutputGpu] = bytemuck::cast_slice(&data);
        for (i, m) in markets.iter_mut().enumerate() {
            if results[i].error == 0 {
                m.set_fundamental_value(Money::from_cents(results[i].new_v_cents as i64));
            }
        }
        drop(data);
        staging_buf.unmap();
    }

    fn decide_all(
        &self,
        _strategies: &[StrategyData],
        _market_with_v: &MarketView,
        _market_no_v: &MarketView,
        _selves: &[SelfView],
        _seeds: &[u64],
    ) -> Vec<Vec<Intent>> {
        // TODO: NPC decide GPU shader
        Vec::new()
    }
}

pub fn try_create_gpu() -> Option<Box<dyn ComputeBackend>> {
    GpuBackend::new().map(|g| Box::new(g) as Box<dyn ComputeBackend>)
}
