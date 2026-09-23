//! 开发存货减值处理器（K3 地产）：目标化补提/转回（可变现净值显式输入，
//! Fixture 游戏假设——CAS 8 原文取证受阻，条款号不引用）。
//!
//! 分录：补提 Dr 6701 / Cr 1542；转回 Dr 1542 / Cr 6701（均非现金）。
//! 简化（登记 issues）：交付结转不自动转回准备——减值准备的释放经同一
//! 目标化入口以更高的可变现净值重估完成。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::real_estate::{chart, RealEstateBooks, RealEstateError};

impl RealEstateBooks {
    /// 目标化减值：目标准备 = max(0, 开发存货毛额 − 可变现净值)；与已入账
    /// 准备（1542 贷方余额）的差额补提（或转回）。差额为零 → `Ok(None)`。
    pub fn update_inventory_impairment(
        &mut self,
        nrv_total: AccountingAmount,
        date: CivilDate,
    ) -> Result<Option<BusinessEventId>, RealEstateError> {
        if nrv_total.is_negative() {
            return Err(RealEstateError::NonPositiveAmount {
                what: "net realizable value",
                amount: nrv_total,
            });
        }
        let gross = self.net_of(chart::acct::DEV_INVENTORY)?;
        let target = if gross > nrv_total {
            gross.sub(nrv_total)?
        } else {
            AccountingAmount::ZERO
        };
        let posted = self.net_of(chart::acct::DEV_IMPAIR_ALLOW)?.neg()?;
        let delta = target.sub(posted)?;
        if delta.is_zero() {
            return Ok(None);
        }
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let (debit_account, credit_account) = if delta.is_positive() {
            (chart::acct::IMPAIR_LOSS, chart::acct::DEV_IMPAIR_ALLOW)
        } else {
            (chart::acct::DEV_IMPAIR_ALLOW, chart::acct::IMPAIR_LOSS)
        };
        let magnitude = if delta.is_negative() {
            delta.neg()?
        } else {
            delta
        };
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::DevelopmentImpairment,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(debit_account, PostingSide::Debit, magnitude),
                    super::line(credit_account, PostingSide::Credit, magnitude),
                ],
            }],
        )?;
        Ok(Some(event))
    }
}
