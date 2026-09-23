//! 手续费及佣金收入处理器（K3 银行，任务 9；CAS 14 §4 服务履约收现）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::bank::{chart, BankBooks, BankError};
use crate::company::counterparty::{CounterpartyId, FlowDirection};

impl BankBooks {
    /// 服务手续费收现：Dr 1003 / Cr 6021（经营）。
    pub fn earn_fee(
        &mut self,
        customer: &CounterpartyId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, BankError> {
        self.ensure_counterparty(customer)?;
        if !amount.is_positive() {
            return Err(BankError::NonPositiveAmount {
                what: "fee income",
                amount,
            });
        }
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::FeeAndCommissionEarned,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CASH, PostingSide::Debit, amount),
                    super::line(chart::acct::FEE_INCOME, PostingSide::Credit, amount),
                ],
            }],
        )?;
        self.record_flow(date, customer, FlowDirection::Inbound, amount, "fee income")?;
        Ok(event)
    }
}
