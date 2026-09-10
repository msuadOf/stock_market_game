//! 保险账套装配输入与开局守卫 + 版本化贴现假设（K3 保险，任务 10）。
//!
//! 贴现假设是显式游戏假设（无默认构造——不声称真实精算利率）：单一平坦
//! 年利率（bp），ACT/365F 简单贴现。风险调整为逐组显式金额输入（同样
//! 不折现——见 csm.rs 头注的登记简化）。

use crate::accounting::reports::insurance::codes;
use crate::accounting::{JournalLine, LedgerAccountId};
use crate::calendar::CivilDate;
use crate::company::insurance::InsuranceError;
use crate::company::ExternalCounterparty;

/// 版本化贴现假设（游戏假设，Fixture 标注）：单一平坦年利率。
/// 利率必须为正（0 拒绝——不虚构无贴现计量）且 ≤ 10000bp。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct DiscountAssumption {
    pub version: u32,
    pub rate_bp: i32,
}

impl DiscountAssumption {
    pub fn validate(&self) -> Result<(), InsuranceError> {
        if !(1..=10_000).contains(&self.rate_bp) {
            return Err(InsuranceError::InvalidDiscountRate {
                rate_bp: self.rate_bp,
            });
        }
        Ok(())
    }
}

/// 保险账套装配输入。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct InsuranceConfig {
    pub chart: crate::accounting::AccountChart,
    pub as_of: CivilDate,
    /// 显式平衡的开局行（经 `post_batch` 验证路径，任务 7 语义）。
    /// 只允许现金 + 权益侧科目；触碰保险子账科目 → 构造期拒绝。
    pub opening_lines: Vec<JournalLine>,
    pub counterparties: Vec<ExternalCounterparty>,
    /// 版本化贴现假设（无默认——游戏假设显式配置）。
    pub discount: DiscountAssumption,
}

/// 开局行子账种子守卫：触碰任何保险子账/损益科目（应收保费/未到期责任
/// 负债/已发生赔款负债/保险服务收入/费用/财务损益）→
/// `OpeningInsuranceBooksSeeded`。现金（1002）与权益（4001/4103）是开局
/// 唯一合法落点。
pub(super) fn check_opening_lines(lines: &[JournalLine]) -> Result<(), InsuranceError> {
    const SEEDED_ACCOUNTS: [&str; 6] = [
        codes::PREMIUM_RECEIVABLE,
        codes::LRC,
        codes::LIC,
        codes::INSURANCE_REVENUE,
        codes::INSURANCE_EXPENSE,
        codes::INSURANCE_FINANCE,
    ];
    for line in lines {
        if SEEDED_ACCOUNTS.contains(&line.account.0.as_str()) {
            return Err(InsuranceError::OpeningInsuranceBooksSeeded {
                account: LedgerAccountId(line.account.0.clone()),
            });
        }
    }
    Ok(())
}
