//! 资本开支、折旧与固定资产减值（K3 工商）。
//!
//! 资本开支 Dr 固定资产 / Cr 现金（**投资活动**现金）。折旧月扫：对每个尚有
//! 剩余寿命的资产计提一月直线折旧（Dr 管理费用 / Cr 累计折旧，**非现金**；
//! 全部折完的资产自然跳过——返回空扫是合法 no-op）。减值 Dr 资产减值损失 /
//! Cr 固定资产减值准备（非现金）。折旧费用暂全部归集管理费用（简化）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, FixedAssetCode, JournalEntry,
    PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::industrial::{chart, IndustrialBooks, IndustrialError};

/// 单资产折旧过账结果。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DepreciationPosting {
    pub code: FixedAssetCode,
    pub amount: AccountingAmount,
    pub event: BusinessEventId,
}

impl IndustrialBooks {
    /// 资本开支：计量政策校验（正成本、残值 ∈ [0, 成本]、寿命 ≥ 1 月、代码
    /// 唯一）先于过账；Dr 1601 / Cr 现金（投资活动）→ 登记资产子账。
    pub fn acquire_asset(
        &mut self,
        code: FixedAssetCode,
        cost: AccountingAmount,
        salvage_value: AccountingAmount,
        life_months: i64,
        date: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        crate::accounting::FixedAssetRegister::validate_policy(cost, salvage_value, life_months)?;
        if self.assets().get(&code).is_some() {
            return Err(IndustrialError::Asset(
                crate::accounting::FixedAssetError::DuplicateAsset { code },
            ));
        }
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::CashExpense,
                cash_flow: CashFlowClass::Investing,
                lines: vec![
                    super::line(chart::acct::FIXED_ASSET, PostingSide::Debit, cost),
                    super::line(chart::acct::BANK, PostingSide::Credit, cost),
                ],
            }],
        )?;
        self.assets_mut()
            .register(code, cost, salvage_value, life_months)?;
        Ok(event)
    }

    /// 折旧月扫（每个剩余寿命 > 0 的资产计提一月；无资产可提 = 合法空扫）。
    pub fn depreciate_month(
        &mut self,
        date: CivilDate,
    ) -> Result<Vec<DepreciationPosting>, IndustrialError> {
        let mut due: Vec<(FixedAssetCode, AccountingAmount)> = Vec::new();
        for (code, entry) in self.assets().iter() {
            if entry.remaining_months() > 0 {
                due.push((code.clone(), self.assets().preview_depreciation(code)?));
            }
        }
        let base = self.next_event_id;
        let mut postings = Vec::new();
        let mut entries = Vec::new();
        for (index, (code, amount)) in due.into_iter().enumerate() {
            let event = BusinessEventId::new(base + index as u64);
            entries.push(JournalEntry {
                source: event,
                date,
                kind: BusinessKind::Depreciation,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(chart::acct::ADMIN_EXP, PostingSide::Debit, amount),
                    super::line(chart::acct::ACC_DEP, PostingSide::Credit, amount),
                ],
            });
            postings.push(DepreciationPosting {
                code,
                amount,
                event,
            });
        }
        self.post_with_commit(base + postings.len() as u64, entries)?;
        for posting in &postings {
            self.assets_mut().apply_depreciation(&posting.code)?;
        }
        Ok(postings)
    }

    /// 固定资产减值：Dr 资产减值损失 / Cr 固定资产减值准备（非现金）。
    /// 金额必须 ≤ 账面 − 残值（类型化拒绝）；剩余寿命内以新基础继续摊销。
    pub fn impair_asset(
        &mut self,
        code: &FixedAssetCode,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        self.assets().validate_impairment(code, amount)?;
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::Depreciation,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(chart::acct::IMPAIR_LOSS, PostingSide::Debit, amount),
                    super::line(chart::acct::ACC_IMPAIR, PostingSide::Credit, amount),
                ],
            }],
        )?;
        self.assets_mut().apply_impairment(code, amount)?;
        Ok(event)
    }
}
