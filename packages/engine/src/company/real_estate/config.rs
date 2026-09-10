//! 地产账套装配输入与开局守卫（K3 地产，任务 11）：配置值类型 + 构造期
//! 守卫（资本化政策校验先行；开局行不得给地产子账科目种子——经营前史由
//! 任务 14 用同一处理器生成，不从存档倒推，诚实边界）。
//!
//! **资本化政策是版本化游戏假设**（`game-assumption-borrowing-capitalization`，
//! CAS 17 原文取证受阻——docs/company-accounting.md §7；参数不声称真实准则
//! 参数，随存档固化）。

use crate::accounting::reports::real_estate::codes;
use crate::accounting::{AccountChart, JournalLine, LedgerAccountId};
use crate::calendar::CivilDate;
use crate::company::contracts::OperatingBudget;
use crate::company::real_estate::RealEstateError;
use crate::company::ExternalCounterparty;

/// 借款费用资本化政策（游戏假设，版本化；阈值单位 = 自然日）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CapitalizationPolicy {
    pub version: u32,
    /// 开发中断达到该自然日数即暂停资本化（Fixture 默认 90 ≈ 3 个月）。
    pub suspension_min_days: i64,
}

impl CapitalizationPolicy {
    pub(super) fn validate(&self) -> Result<(), RealEstateError> {
        if self.suspension_min_days < 1 {
            return Err(RealEstateError::InvalidPolicy {
                detail: format!(
                    "suspension_min_days must be >= 1, got {}",
                    self.suspension_min_days
                ),
            });
        }
        Ok(())
    }
}

/// 地产账套装配输入。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct RealEstateConfig {
    pub chart: AccountChart,
    pub as_of: CivilDate,
    /// 显式平衡的开局行（经 `post_batch` 验证路径，任务 7 语义）。
    /// 只允许现金 + 权益侧科目；触碰地产子账科目 → 构造期拒绝。
    pub opening_lines: Vec<JournalLine>,
    pub counterparties: Vec<ExternalCounterparty>,
    /// 经营预算（含授信——项目借款必须经额度约束，K2）。
    pub budget: OperatingBudget,
    /// 版本化资本化政策（游戏假设，无生产默认构造）。
    pub capitalization_policy: CapitalizationPolicy,
    /// 单公司同时存续的项目数上限（K2 需求约束；≥1）。
    pub max_projects: usize,
}

/// 开局行子账种子守卫：触碰任何地产子账/损益科目（开发存货/准备/合同负债/
/// 应收/借款/应付利息/收入成本/费用/减值）→ `OpeningRealEstateSeeded`。
/// 现金（1002）与权益（4001/4103）是开局唯一合法落点。
pub(super) fn check_opening_lines(lines: &[JournalLine]) -> Result<(), RealEstateError> {
    const SEEDED_ACCOUNTS: [&str; 11] = [
        codes::AR,
        codes::DEV_INVENTORY,
        codes::DEV_IMPAIR_ALLOW,
        codes::ST_DEBT,
        codes::CONTRACT_LIAB,
        codes::INT_PAYABLE,
        codes::LT_DEBT,
        codes::REVENUE,
        codes::COGS,
        codes::FIN_EXP,
        codes::IMPAIR_LOSS,
    ];
    for line in lines {
        if SEEDED_ACCOUNTS.contains(&line.account.0.as_str()) {
            return Err(RealEstateError::OpeningRealEstateSeeded {
                account: LedgerAccountId(line.account.0.clone()),
            });
        }
    }
    Ok(())
}
