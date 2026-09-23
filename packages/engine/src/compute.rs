//! 计算后端抽象（ADR-0008）：CPU(rayon) / GPU(wgpu) 统一接口。
//!
//! 设计原则：
//! - 只抽象「并行可独立」的工作（NPC decide）。
//! - 顺序依赖部分（撮合、结算、路由）留在 session.rs，不经 ComputeBackend。
//! - CPU 实现 = rayon 并行（零行为变化）。
//! - GPU 实现 = wgpu compute shader（整数定点，跨厂商确定性）。
//! - 动态切换：SessionSetup.compute 控制。
//!
//! 共同 V（隐藏公允价）已删除：后端只接收**纯批量输入**（同一份公共市场
//! 视图 + 每人自身视图 + 每人种子），按输入顺序返回稳定顺序输出，不按
//! 账户身份偷授予任何信息；个体信念/计划的会话侧编排不经本接缝。

use crate::session::SplitMix64;
use crate::strategy::{Intent, MarketView, SelfView, StrategyData};
use rayon::prelude::*;

/// 计算后端 trait：抽象 CPU/GPU 并行计算。
///
/// 实现者负责：
/// - `decide_all`：并行跑所有 NPC 的 decide，返回每人 Intent 列表（按输入顺序）。
///
/// 调用方（session.rs step）负责顺序部分（路由 Intent、撮合、结算）。
pub trait ComputeBackend: Send + Sync {
    /// 并行 NPC decide。
    ///
    /// - `strategies`: 每NPC的策略数据（数据化后，可序列化→GPU buffer）。
    /// - `market`: 每个决策者看到的同一份公共市场视图（无隐藏信息）。
    /// - `selves`: 每NPC的自身视图。
    /// - `seeds`: 每NPC的确定性 RNG 种子（seed ^ tick ^ npc_id）。
    ///
    /// 返回 Vec<Vec<Intent>>，索引与 strategies 对齐。
    fn decide_all(
        &self,
        strategies: &[StrategyData],
        market: &MarketView,
        selves: &[SelfView],
        seeds: &[u64],
    ) -> Result<Vec<Vec<Intent>>, ComputeError>;

    /// 后端名称（"cpu" / "gpu"），用于日志/调试。
    fn name(&self) -> &'static str;
}

/// CPU 后端：rayon 多核并行。
pub struct CpuBackend;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ComputeError {
    #[error("invalid compute input: {0}")]
    InvalidInput(String),
    #[error("requested compute backend is unavailable: {0}")]
    BackendUnavailable(&'static str),
}

impl ComputeBackend for CpuBackend {
    fn decide_all(
        &self,
        strategies: &[StrategyData],
        market: &MarketView,
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
                let sv = &selves[i];
                let mut rng = SplitMix64::new(seeds[i]);
                crate::strategy::decide_data(s, market, sv, &mut rng)
            })
            .collect())
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
