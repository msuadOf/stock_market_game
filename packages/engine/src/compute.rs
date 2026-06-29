//! 计算后端抽象（ADR-0008）：CPU(rayon) / GPU(wgpu) 统一接口。
//!
//! 设计原则：
//! - 只抽象「并行可独立」的工作（NPC decide、V 演化）。
//! - 顺序依赖部分（撮合、结算、路由）留在 session.rs，不经 ComputeBackend。
//! - CPU 实现 = 现有 rayon 逻辑提取（零行为变化）。
//! - GPU 实现 = wgpu compute shader（整数定点，跨厂商确定性）。
//! - 动态切换：SessionSetup.compute 控制。

use crate::market::{Market, VParams};
#[allow(unused_imports)]
use crate::strategy::Rng;
use crate::strategy::{Intent, MarketView, SelfView, StrategyData};
use crate::account::AccountKind;
use crate::session::SplitMix64;
use rayon::prelude::*;

/// 计算后端 trait：抽象 CPU/GPU 并行计算。
///
/// 实现者负责：
/// - `decide_all`：并行跑所有 NPC 的 decide，返回每人 Intent 列表（按输入顺序）。
/// - `evolve_v_all`：并行跑所有股票的 V 演化（原地修改 Market）。
///
/// 调用方（session.rs step）负责顺序部分（路由 Intent、撮合、结算）。
pub trait ComputeBackend: Send + Sync {
    /// 并行 NPC decide。
    ///
    /// - `strategies`: 每NPC的策略数据（数据化后，可序列化→GPU buffer）。
    /// - `market_with_v`: 含 V 的市场视图（机构用）。
    /// - `market_no_v`: 不含 V 的市场视图（散户/游资用）。
    /// - `selves`: 每NPC的自身视图。
    /// - `seeds`: 每NPC的确定性 RNG 种子（seed ^ tick ^ npc_id）。
    ///
    /// 返回 Vec<Vec<Intent>>，索引与 strategies 对齐。
    fn decide_all(
        &self,
        strategies: &[StrategyData],
        market_with_v: &MarketView,
        market_no_v: &MarketView,
        selves: &[SelfView],
        seeds: &[u64],
    ) -> Vec<Vec<Intent>>;

    /// 并行 V 演化（原地修改 markets）。
    ///
    /// - `markets`: 可变切片（每股票一个 Market）。
    /// - `params`: V 演化参数。
    /// - `seeds`: 每股票的确定性 RNG 种子。
    fn evolve_v_all(
        &self,
        markets: &mut [Market],
        params: &VParams,
        seeds: &[u64],
    );

    /// 后端名称（"cpu" / "gpu"），用于日志/调试。
    fn name(&self) -> &'static str;
}

/// CPU 后端：rayon 多核并行。
pub struct CpuBackend;

impl ComputeBackend for CpuBackend {
    fn decide_all(
        &self,
        strategies: &[StrategyData],
        market_with_v: &MarketView,
        market_no_v: &MarketView,
        selves: &[SelfView],
        seeds: &[u64],
    ) -> Vec<Vec<Intent>> {
        strategies
            .par_iter()
            .enumerate()
            .map(|(i, s)| {
                let mv = if s.kind == AccountKind::Inst {
                    market_with_v
                } else {
                    market_no_v
                };
                let sv = &selves[i];
                let mut rng = SplitMix64::new(seeds[i]);
                crate::strategy::decide_data(s, mv, sv, &mut rng)
            })
            .collect()
    }

    fn evolve_v_all(
        &self,
        markets: &mut [Market],
        params: &VParams,
        seeds: &[u64],
    ) {
        markets
            .par_iter_mut()
            .enumerate()
            .for_each(|(i, m)| {
                let mut rng = SplitMix64::new(seeds[i]);
                let _ = m.evolve_v(params, &mut rng);
            });
    }

    fn name(&self) -> &'static str {
        "cpu"
    }
}

/// 计算模式（配置项）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum ComputeMode {
    /// CPU rayon 并行（默认）。
    #[default]
    Cpu,
    /// GPU wgpu compute shader（需要 engine-gpu crate + SharedArrayBuffer）。
    Gpu,
    /// 自动：有 GPU 且负载大时用 GPU，否则 CPU。
    Auto,
}

/// 创建指定模式的计算后端。
/// CPU 模式直接返回 CpuBackend。
/// GPU 模式返回 GpuBackend（需要 engine-gpu feature + wgpu 初始化）。
pub fn create_backend(mode: &ComputeMode) -> Box<dyn ComputeBackend> {
    match mode {
        ComputeMode::Cpu => Box::new(CpuBackend),
        ComputeMode::Gpu | ComputeMode::Auto => {
            #[cfg(feature = "gpu")]
            {
                if let Some(gpu) = crate::gpu_backend::try_create_gpu() {
                    return gpu;
                }
            }
            // GPU 不可用或未编译 → 回退 CPU
            Box::new(CpuBackend)
        }
    }
}
