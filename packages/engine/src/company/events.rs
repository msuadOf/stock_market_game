//! 默认经济事件目录与冲击参数（K4，任务 14）。
//!
//! 最低目录：市场需求扩张/收缩、行业成本变化、单公司合同取得/取消、客户
//! 延付/信用恶化、生产中断/恢复（= 中断到期）、资产减值迹象。**效果是经济
//! 参数变化**（需求/成本/履约/信用风险乘数的整数基点增量）——绝不是价格
//! 百分比，更绝不直接改财务余额；余额只能经行业处理器的业务事件过账变动。
//!
//! 冲击参数是**版本化待校准游戏假设**（K4：市场/行业每天独立 1% 候选概率、
//! 公司 2%；持续 5–30 自然日；幅度带 ±500/±1000/±2000bp）——不声称现实
//! 频率。压力场景是另一套显式参数（`stress_v1`），不按倍数派生。
//!
//! 跨行业适用面（事件只作用于适用经济字段；行业标签来自 `CompanySpec`）：
//! - 需求字段：工商（销量）、保险（新单量）、地产（预售节奏）适用；
//!   **银行存贷流不读需求字段**（K4 明文）。
//! - 成本字段：工商（采购单价）、地产（开发投入）适用；银行/保险无商品
//!   成本字段。
//! - 信用恶化：工商 → 应收 ECL 率上浮；银行 → 贷款 ECL 重估；保险/地产
//!   记录为风险事件（无自动重估面，登记 issues）。
//! - 生产中断：工商停产、地产暂停开发；银行/保险不适用。
//! - 减值迹象：工商固定资产减值计提；其余行业无驱动面。

use crate::calendar::{CivilDate, CivilDateError};
use crate::company::rng::OperatingRng;
use crate::company::spec::IndustryId;

/// 冲击参数（版本化游戏假设；serde 随存档冻结）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShockParams {
    pub version: u32,
    /// 市场冲击每天候选概率（bp；K4 默认 100 = 1%）。
    pub market_candidate_bp: i32,
    /// 行业冲击每天候选概率（每行业独立流）。
    pub industry_candidate_bp: i32,
    /// 单公司事件每天候选概率（K4 默认 200 = 2%）。
    pub company_candidate_bp: i32,
    /// 事件持续期范围（自然日，闭区间）。
    pub duration_min_days: i64,
    pub duration_max_days: i64,
    /// 市场需求幅度带（±bp）。
    pub market_demand_band_bp: i32,
    /// 行业成本幅度带（±bp）。
    pub industry_cost_band_bp: i32,
    /// 单公司需求幅度带（±bp）。
    pub company_demand_band_bp: i32,
    /// 信用恶化事件的 ECL 率上浮（bp，正值）。
    pub credit_deterioration_add_bp: i32,
}

impl ShockParams {
    /// K4 默认参数（待校准游戏假设，不声称现实频率）。
    pub fn default_v1() -> Self {
        Self {
            version: 1,
            market_candidate_bp: 100,
            industry_candidate_bp: 100,
            company_candidate_bp: 200,
            duration_min_days: 5,
            duration_max_days: 30,
            market_demand_band_bp: 500,
            industry_cost_band_bp: 1_000,
            company_demand_band_bp: 2_000,
            credit_deterioration_add_bp: 500,
        }
    }

    /// 显式压力场景（另一套独立数值，非倍数派生；游戏假设）。
    pub fn stress_v1() -> Self {
        Self {
            version: 1,
            market_candidate_bp: 10_000,
            industry_candidate_bp: 10_000,
            company_candidate_bp: 10_000,
            duration_min_days: 5,
            duration_max_days: 30,
            market_demand_band_bp: 500,
            industry_cost_band_bp: 1_000,
            company_demand_band_bp: 2_000,
            credit_deterioration_add_bp: 500,
        }
    }

    pub fn validate(&self) -> Result<(), crate::company::operations::OperationsError> {
        use crate::company::operations::OperationsError;
        let bad = |detail: String| OperationsError::ShockParamsInvalid { detail };
        for (label, bp) in [
            ("market_candidate_bp", self.market_candidate_bp),
            ("industry_candidate_bp", self.industry_candidate_bp),
            ("company_candidate_bp", self.company_candidate_bp),
        ] {
            if !(0..=10_000).contains(&bp) {
                return Err(bad(format!("{label} {bp}bp out of [0, 10000]")));
            }
        }
        for (label, band) in [
            ("market_demand_band_bp", self.market_demand_band_bp),
            ("industry_cost_band_bp", self.industry_cost_band_bp),
            ("company_demand_band_bp", self.company_demand_band_bp),
        ] {
            if band <= 0 {
                return Err(bad(format!("{label} {band}bp must be positive")));
            }
        }
        if self.credit_deterioration_add_bp < 0 {
            return Err(bad(format!(
                "credit_deterioration_add_bp {} must be >= 0",
                self.credit_deterioration_add_bp
            )));
        }
        if self.duration_min_days < 1 || self.duration_max_days < self.duration_min_days {
            return Err(bad(format!(
                "duration window [{}, {}] invalid",
                self.duration_min_days, self.duration_max_days
            )));
        }
        Ok(())
    }
}

/// 事件种类（目录值；效果字段见模块头注的适用面矩阵）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ShockKind {
    /// 市场需求扩张/收缩（全市场；幅度符号区分扩张/收缩）。
    MarketDemandShift,
    /// 行业成本变化（只作用于被标签行业的成本字段）。
    IndustryCostShift { industry: IndustryId },
    /// 单公司需求扩张/收缩。
    CompanyDemandShift,
    /// 单公司合同取得（订单流上行）。
    ContractWon,
    /// 单公司合同取消（订单流下行）。
    ContractCancelled,
    /// 客户延付/信用恶化。
    CreditDeterioration,
    /// 生产中断（到期自动恢复）。
    ProductionInterruption,
    /// 资产减值迹象。
    AssetImpairmentSignal,
}

/// 一个已激活（或待激活）的冲击：幅度（带符号 bp）+ 有效窗口（闭区间）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActiveShock {
    pub kind: ShockKind,
    pub amplitude_bp: i32,
    pub starts_on: CivilDate,
    /// 含当日；`< starts_on` 非法（注入时校验）。
    pub expires_on: CivilDate,
}

/// 采样一个带符号幅度：符号位独立抽取，幅度取 [1, band]（零幅度事件无意义）。
fn sample_amplitude(rng: &mut OperatingRng, band: i32) -> i32 {
    let sign = if rng.below(2) == 0 { 1 } else { -1 };
    let magnitude = i32::try_from(rng.below(u64::try_from(band).expect("band > 0") + 1))
        .expect("within band")
        .max(1);
    sign * magnitude.min(band)
}

fn sample_window(
    rng: &mut OperatingRng,
    date: CivilDate,
    params: &ShockParams,
) -> Result<CivilDate, CivilDateError> {
    let days = rng.range_i64(params.duration_min_days, params.duration_max_days);
    let mut cursor = date;
    for _ in 0..(days - 1).max(0) {
        cursor = cursor.next()?;
    }
    Ok(cursor)
}

/// 市场冲击候选（每天至多一次；全局市场流）。
pub(super) fn sample_market_shock(
    rng: &mut OperatingRng,
    date: CivilDate,
    params: &ShockParams,
) -> Result<Option<ActiveShock>, CivilDateError> {
    if !rng.chance_bp(params.market_candidate_bp) {
        return Ok(None);
    }
    Ok(Some(ActiveShock {
        kind: ShockKind::MarketDemandShift,
        amplitude_bp: sample_amplitude(rng, params.market_demand_band_bp),
        starts_on: date,
        expires_on: sample_window(rng, date, params)?,
    }))
}

/// 行业冲击候选（每行业独立流，每天至多一次）。
pub(super) fn sample_industry_shock(
    rng: &mut OperatingRng,
    industry: &IndustryId,
    date: CivilDate,
    params: &ShockParams,
) -> Result<Option<ActiveShock>, CivilDateError> {
    if !rng.chance_bp(params.industry_candidate_bp) {
        return Ok(None);
    }
    Ok(Some(ActiveShock {
        kind: ShockKind::IndustryCostShift {
            industry: industry.clone(),
        },
        amplitude_bp: sample_amplitude(rng, params.industry_cost_band_bp),
        starts_on: date,
        expires_on: sample_window(rng, date, params)?,
    }))
}

/// 单公司事件候选（公司经营流，每天至多一次；种类均匀抽取）。
pub(super) fn sample_company_shock(
    rng: &mut OperatingRng,
    date: CivilDate,
    params: &ShockParams,
) -> Result<Option<ActiveShock>, CivilDateError> {
    if !rng.chance_bp(params.company_candidate_bp) {
        return Ok(None);
    }
    let kind = match rng.below(6) {
        0 => ShockKind::CompanyDemandShift,
        1 => ShockKind::ContractWon,
        2 => ShockKind::ContractCancelled,
        3 => ShockKind::CreditDeterioration,
        4 => ShockKind::ProductionInterruption,
        _ => ShockKind::AssetImpairmentSignal,
    };
    let amplitude_bp = match kind {
        ShockKind::CreditDeterioration => params.credit_deterioration_add_bp,
        ShockKind::ProductionInterruption => 0,
        _ => sample_amplitude(rng, params.company_demand_band_bp),
    };
    Ok(Some(ActiveShock {
        kind,
        amplitude_bp,
        starts_on: date,
        expires_on: sample_window(rng, date, params)?,
    }))
}
