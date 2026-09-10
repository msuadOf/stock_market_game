//! 任务 17 QA：账户身份、主导风格与分析权重解耦。
//!
//! 契约来源：计划 K5（默认分布/一次采样/最大余数归一）与 K5a（基本面方法链接）。
//! 布局：gold（逐字金样+工厂重放）、invariants（跨身份/零权重/归一/serde）、
//! failures（负权重/全0/非法映射/恢复冲突的类型化拒绝）。

mod failures;
mod gold;
mod invariants;

use engine::strategy::{HotParams, InstParams, RetailParams, StrategyParams};

/// 严格序列 RNG：只允许预先给出的 draw 序列，多抽一次即 panic。
/// 用于证明「每个非零权重恰好采样一次、零权重不消耗随机数」的采样次数契约。
pub(crate) struct StrictSeqRng {
    values: Vec<f64>,
    next: usize,
}

impl StrictSeqRng {
    pub(crate) fn new(values: &[f64]) -> Self {
        Self {
            values: values.to_vec(),
            next: 0,
        }
    }

    pub(crate) fn draws_used(&self) -> usize {
        self.next
    }
}

impl engine::strategy::Rng for StrictSeqRng {
    fn next_f64(&mut self) -> f64 {
        assert!(
            self.next < self.values.len(),
            "analysis profile derivation drew more random numbers than non-zero weights"
        );
        let value = self.values[self.next];
        self.next += 1;
        value
    }

    fn next_range_u32(&mut self, lo: u32, _hi: u32) -> u32 {
        lo
    }
}

/// 合法群体基准参数样本（与 tests/strategy.rs 相同构造，供工厂重放测试复用）。
pub(crate) fn sample_params() -> StrategyParams {
    StrategyParams {
        retail: RetailParams {
            arrival_rate: 0.5,
            order_size_mean: 100,
            chase_prob: 0.2,
            tick_cents: 1,
        },
        inst: InstParams {
            margin: 0.05,
            order_size: 200,
        },
        hot: HotParams {
            lookback: 3,
            trend_threshold: 0.02,
            order_size: 150,
        },
    }
}
