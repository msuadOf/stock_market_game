//! 计算后端抽象（ADR-0008）：CPU(rayon) / GPU(wgpu) 统一接口。
//!
//! 设计原则：
//! - 只抽象「并行可独立」的工作（NPC decide、V 演化）。
//! - 顺序依赖部分（撮合、结算、路由）留在 session.rs，不经 ComputeBackend。
//! - CPU 实现 = 现有 rayon 逻辑提取（零行为变化）。
//! - GPU 实现 = wgpu compute shader（整数定点，跨厂商确定性）。
//! - 动态切换：SessionSetup.compute 控制。

use crate::account::AccountKind;
use crate::market::{Market, VParams};
use crate::session::SplitMix64;
use crate::strategy::{Intent, MarketView, SelfView, StrategyData};
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
    ) -> Result<Vec<Vec<Intent>>, ComputeError>;

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
    ) -> Result<(), ComputeError>;

    /// 后端名称（"cpu" / "gpu"），用于日志/调试。
    fn name(&self) -> &'static str;
}

/// CPU 后端：rayon 多核并行。
pub struct CpuBackend;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ComputeError {
    #[error("invalid compute input: {0}")]
    InvalidInput(String),
    #[error("market {index} value evolution failed: {reason}")]
    MarketEvolution { index: usize, reason: String },
    #[error("requested compute backend is unavailable: {0}")]
    BackendUnavailable(&'static str),
}

impl ComputeBackend for CpuBackend {
    fn decide_all(
        &self,
        strategies: &[StrategyData],
        market_with_v: &MarketView,
        market_no_v: &MarketView,
        selves: &[SelfView],
        seeds: &[u64],
    ) -> Result<Vec<Vec<Intent>>, ComputeError> {
        if strategies.len() != selves.len() || strategies.len() != seeds.len() {
            return Err(ComputeError::InvalidInput(format!(
                "decide_all requires equal lengths, got strategies={}, selves={}, seeds={}",
                strategies.len(),
                selves.len(),
                seeds.len()
            )));
        }
        Ok(strategies
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
            .collect())
    }

    fn evolve_v_all(
        &self,
        markets: &mut [Market],
        params: &VParams,
        seeds: &[u64],
    ) -> Result<(), ComputeError> {
        if markets.len() != seeds.len() {
            return Err(ComputeError::InvalidInput(format!(
                "evolve_v_all requires equal lengths, got markets={} and seeds={}",
                markets.len(),
                seeds.len()
            )));
        }
        let results: Vec<Result<Market, String>> = markets
            .par_iter()
            .enumerate()
            .map(|(i, market)| {
                let mut evolved = market.clone();
                let mut rng = SplitMix64::new(seeds[i]);
                evolved
                    .evolve_v(params, &mut rng)
                    .map(|()| evolved)
                    .map_err(|error| error.to_string())
            })
            .collect();
        let mut evolved = Vec::with_capacity(results.len());
        for (index, result) in results.into_iter().enumerate() {
            evolved.push(result.map_err(|reason| ComputeError::MarketEvolution { index, reason })?);
        }
        markets.clone_from_slice(&evolved);
        Ok(())
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
pub fn create_backend(mode: &ComputeMode) -> Result<Box<dyn ComputeBackend>, ComputeError> {
    match mode {
        ComputeMode::Cpu | ComputeMode::Auto => Ok(Box::new(CpuBackend)),
        ComputeMode::Gpu => Err(ComputeError::BackendUnavailable(
            "the authoritative session currently supports CPU only; use engine-gpu explicitly after parity validation",
        )),
    }
}
