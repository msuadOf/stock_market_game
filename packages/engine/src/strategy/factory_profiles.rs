//! 工厂侧分析档案派生（K5）：主导风格默认分布、个体一次采样与最大余数归一。
//!
//! 账户身份与主导风格仍由既有 ordinal 映射/抽样决定（本模块不重抽风格、不改
//! 身份）；只在其上派生「混合分析能力」：每个非零权重按声明顺序恰好采样一次
//! 0.6–1.4 倍率（独立 profile RNG，零权重不消耗随机数且保持零），再用最大余数
//! 法归一回 10000bp。基本面方法仅在工厂选择一次（K5a 链接规则）。

use super::analysis_profile::{
    AnalysisProfile, AnalysisProfileError, AnalysisWeights, FundamentalMethod,
};
use super::profile::{HotStyle, InstitutionStyle, RetailStyle, StrategyProfile};
use super::Rng;
use crate::orderbook::AccountId;

/// K5 默认分析权重分布（顺序：基本面/趋势/量价/技术/成本经历，单位 bp）。
///
/// 数表是计划文本 K5 节的逐字副本；13 个风格各自总和恒为 10000（由
/// `analysis_profiles` 测试逐字锁定，expect 因此不可达）。
pub fn default_analysis_weights(profile: &StrategyProfile) -> AnalysisWeights {
    let (fundamental, trend, price_volume, technical, experience_cost) = match profile {
        StrategyProfile::Retail(RetailStyle::LongTerm) => (5000, 1000, 500, 1000, 2500),
        StrategyProfile::Retail(RetailStyle::DipBuyer) => (1500, 1000, 1000, 1500, 5000),
        StrategyProfile::Retail(RetailStyle::Momentum) => (500, 4500, 2000, 2000, 1000),
        StrategyProfile::Retail(RetailStyle::Noise) => (0, 1000, 1000, 1000, 7000),
        StrategyProfile::Retail(RetailStyle::Panic) => (0, 2000, 1000, 1000, 6000),
        StrategyProfile::Retail(RetailStyle::Dormant) => (1000, 1000, 500, 500, 7000),
        StrategyProfile::Institution(InstitutionStyle::DeepValue) => (7000, 500, 500, 500, 1500),
        StrategyProfile::Institution(InstitutionStyle::Growth) => (7000, 1000, 500, 500, 1000),
        StrategyProfile::Institution(InstitutionStyle::Balanced) => (6000, 1000, 1000, 1000, 1000),
        StrategyProfile::Institution(InstitutionStyle::Defensive) => (6500, 500, 500, 500, 2000),
        StrategyProfile::Institution(InstitutionStyle::ActiveTrader) => (0, 4500, 2500, 2000, 1000),
        StrategyProfile::Hot(HotStyle::Momentum) => (0, 5000, 2500, 2000, 500),
        StrategyProfile::Hot(HotStyle::Reversal) => (0, 1500, 2500, 3000, 3000),
    };
    AnalysisWeights::new(
        i64::from(fundamental),
        i64::from(trend),
        i64::from(price_volume),
        i64::from(technical),
        i64::from(experience_cost),
    )
    .expect("K5 default distributions sum to 10000 bp (locked by analysis_profiles tests)")
}

/// 为既有身份档案派生个体分析能力（新局确定性构造入口；会话层用独立种子的
/// profile RNG 调用，不与既有策略采样共用流）。
///
/// - 主导风格是输入，本函数绝不重抽；
/// - 每个非零权重按声明顺序恰好采样一次 0.6–1.4 倍率，零权重不消耗随机数；
/// - 最大余数法归一回 10000bp（字段声明顺序破同分，零保持零）；
/// - 基本面方法只在工厂选择一次：机构 DeepValue/Defensive→盈利倍数、Growth→
///   现金流；其余一切风格按稳定 AccountId 奇偶（偶→盈利倍数，奇→现金流）。
pub fn derive_analysis_profile(
    profile: &StrategyProfile,
    account_id: AccountId,
    rng: &mut dyn Rng,
) -> Result<AnalysisProfile, AnalysisProfileError> {
    let defaults = default_analysis_weights(profile);
    let sampled = [
        sample_nonzero_weight(defaults.fundamental_bp(), rng),
        sample_nonzero_weight(defaults.trend_bp(), rng),
        sample_nonzero_weight(defaults.price_volume_bp(), rng),
        sample_nonzero_weight(defaults.technical_bp(), rng),
        sample_nonzero_weight(defaults.experience_cost_bp(), rng),
    ];
    let normalized = largest_remainder_normalize(sampled, u64::from(AnalysisWeights::TOTAL_BP))?;
    let weights = AnalysisWeights::new(
        i64::from(normalized[0]),
        i64::from(normalized[1]),
        i64::from(normalized[2]),
        i64::from(normalized[3]),
        i64::from(normalized[4]),
    )?;
    let fundamental_method = if weights.fundamental_bp() == 0 {
        None
    } else {
        select_fundamental_method(profile, account_id)
    };
    AnalysisProfile::new(weights, fundamental_method)
}

/// 最大余数法整数归一（公开契约）：把非负 `raw` 比例缩放为总和恰为 `target`。
///
/// - 每字段先取 `floor(raw_i * target / total)`，不足额按余数从大到小逐字段 +1；
/// - 余数相同按数组下标（=五权重声明顺序：基本面→趋势→量价→技术→成本经历）破同分；
/// - `raw` 为 0 的字段恒归一为 0：deficit 严格小于正余数字段数，+1 到不了余 0 字段。
pub fn largest_remainder_normalize(
    raw: [i64; 5],
    target: u64,
) -> Result<[u32; 5], AnalysisProfileError> {
    if raw.iter().any(|&value| value < 0) {
        return Err(AnalysisProfileError::InvalidNormalizationInput {
            total: raw.iter().sum(),
        });
    }
    let raw_total: i64 = raw.iter().sum();
    if raw_total <= 0 {
        return Err(AnalysisProfileError::InvalidNormalizationInput { total: raw_total });
    }
    if target == 0 || target > u64::from(u32::MAX) {
        return Err(AnalysisProfileError::InvalidNormalizationInput { total: raw_total });
    }
    let total = i128::from(raw_total);
    let target = i128::from(target);
    let mut floors = [0u32; 5];
    let mut remainders = [0i128; 5];
    let mut allocated: i128 = 0;
    for (index, &value) in raw.iter().enumerate() {
        let scaled = i128::from(value) * target;
        let floor = scaled / total;
        // floor_i ≤ raw_i*target/total ≤ target ≤ u32::MAX（raw_i ≤ total，均非负）。
        floors[index] = u32::try_from(floor).expect("floor is bounded by validated target");
        remainders[index] = scaled % total;
        allocated += floor;
    }
    // deficit = Σ余数/total < 正余数字段数 ≤ 5，恒在 0..5 内。
    let deficit = usize::try_from(target - allocated).expect("deficit is mathematically < 5");
    let mut order = [0usize, 1, 2, 3, 4];
    order.sort_by(|&a, &b| remainders[b].cmp(&remainders[a]).then(a.cmp(&b)));
    for &index in &order[..deficit] {
        floors[index] += 1;
    }
    Ok(floors)
}

/// 单次 0.6–1.4 倍率采样（整数 bp，6000..=14000 含端点）；沿用 sampling.rs 的
/// `[lo, lo + width)` → 整数映射约定。
fn sample_multiplier_bp(rng: &mut dyn Rng) -> i64 {
    const LO: i64 = 6_000;
    const WIDTH: u64 = 8_001;
    LO + ((rng.next_f64() * WIDTH as f64) as u64).min(WIDTH - 1) as i64
}

fn sample_nonzero_weight(baseline: u32, rng: &mut dyn Rng) -> i64 {
    if baseline == 0 {
        return 0;
    }
    i64::from(baseline) * sample_multiplier_bp(rng)
}

/// K5a 方法链接：选择只在工厂发生，与观察/估值时点无关。
fn select_fundamental_method(
    profile: &StrategyProfile,
    account_id: AccountId,
) -> Option<FundamentalMethod> {
    match profile {
        StrategyProfile::Institution(InstitutionStyle::DeepValue | InstitutionStyle::Defensive) => {
            Some(FundamentalMethod::EarningsMultiple)
        }
        StrategyProfile::Institution(InstitutionStyle::Growth) => Some(FundamentalMethod::CashFlow),
        StrategyProfile::Institution(
            InstitutionStyle::Balanced | InstitutionStyle::ActiveTrader,
        )
        | StrategyProfile::Retail(_)
        | StrategyProfile::Hot(_) => Some(if account_id.0.is_multiple_of(2) {
            FundamentalMethod::EarningsMultiple
        } else {
            FundamentalMethod::CashFlow
        }),
    }
}
