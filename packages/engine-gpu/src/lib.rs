//! GPU 计算后端（wgpu 22 compute shader，ADR-0008）。
//!
//! 当前只探测/初始化 GPU；权威计算委托 `CpuBackend`。共同 V（隐藏公允价）
//! 及其演化管线已删除；后端保留的唯一抽象面是无隐藏信息的批量 NPC decide。

use engine::compute::{ComputeBackend, ComputeError, CpuBackend};
use engine::strategy::{Intent, MarketView, SelfView, StrategyData};

/// GPU 可用性探针。
///
/// 真实 GPU 计算尚未达到与 CPU 权威实现相同的确定性与错误语义，因此这个后端只在
/// 初始化时验证适配器/设备可用性，所有计算仍交给 `CpuBackend`。
pub struct GpuBackend;

impl GpuBackend {
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;

        let _device = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("engine-gpu"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .ok()?;

        Some(GpuBackend)
    }
}

impl ComputeBackend for GpuBackend {
    fn name(&self) -> &'static str {
        "gpu"
    }

    fn decide_all(
        &self,
        _strategies: &[StrategyData],
        _market: &MarketView,
        _selves: &[SelfView],
        _seeds: &[u64],
    ) -> Result<Vec<Vec<Intent>>, ComputeError> {
        // ADR-0008 currently designates CPU as the only authoritative implementation.
        CpuBackend.decide_all(_strategies, _market, _selves, _seeds)
    }
}

pub fn try_create_gpu() -> Option<Box<dyn ComputeBackend>> {
    GpuBackend::new().map(|g| Box::new(g) as Box<dyn ComputeBackend>)
}
