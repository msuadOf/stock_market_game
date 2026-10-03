//! 债务偿付处理器（地产会计约束）：付息（全部已提未付）与还本（可部分）。
//! 两条分录均为筹资活动现金流出（与工商经营会计采用同一 CAS 31 口径）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::FlowDirection;
use crate::company::real_estate::{chart, RealEstateBooks, RealEstateError};

impl RealEstateBooks {
    /// 付息（全部已提未付）：Dr 2231 / Cr 1002（筹资）。
    pub fn pay_interest(
        &mut self,
        contract: &ContractId,
        date: CivilDate,
    ) -> Result<BusinessEventId, RealEstateError> {
        let state = self
            .loan(contract)
            .ok_or_else(|| RealEstateError::UnknownLoan {
                contract: contract.clone(),
            })?;
        let accrued = state.preview_interest_payment(contract)?;
        let lender = state.lender().clone();

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::InterestPayment,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    super::line(chart::acct::INT_PAYABLE, PostingSide::Debit, accrued),
                    super::line(chart::acct::CASH, PostingSide::Credit, accrued),
                ],
            }],
        )?;
        self.loans_map_mut()
            .get_mut(contract)
            .expect("validated above")
            .settle_accrued();
        self.record_flow(
            date,
            &lender,
            FlowDirection::Outbound,
            accrued,
            "interest payment",
        )?;
        Ok(event)
    }

    /// 还本（可部分）：Dr 2001|2501 / Cr 1002（筹资）。
    pub fn repay_principal(
        &mut self,
        contract: &ContractId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, RealEstateError> {
        let state = self
            .loan(contract)
            .ok_or_else(|| RealEstateError::UnknownLoan {
                contract: contract.clone(),
            })?;
        state.validate_repayment(contract, amount)?;
        let account = state.debt_account();
        let lender = state.lender().clone();

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::LoanRepayment,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    super::line(account, PostingSide::Debit, amount),
                    super::line(chart::acct::CASH, PostingSide::Credit, amount),
                ],
            }],
        )?;
        self.loans_map_mut()
            .get_mut(contract)
            .expect("validated above")
            .repay(amount)?;
        self.record_flow(
            date,
            &lender,
            FlowDirection::Outbound,
            amount,
            "principal repayment",
        )?;
        Ok(event)
    }
}
