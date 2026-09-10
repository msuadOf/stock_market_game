//! 销售与回款（K3 工商）：赊销（履约确认收入，CAS 14 §4/§13 已核验）、
//! 回款（只清应收，**不重复计收入**）、坏账准备（CAS 22 §63 整个存续期 ECL
//! 简化法）与核销。
//!
//! 赊销分录（单张，含成本结转）：Dr 应收（收入+销项税）、Dr 主营业务成本 /
//! Cr 主营业务收入、Cr 222101 销项税额、Cr 库存商品（移动加权成本）。
//! **赊销不动现金**；回款 Dr 现金 / Cr 应收（经营活动），收入不变。
//! 标签映射：核销/准备 = ReceivableCollection(NonCash)/Depreciation(NonCash)。

use crate::accounting::{
    ecl_allowance_target, output_vat_on, AccountingAmount, BusinessEventId, BusinessKind,
    CashFlowClass, InventoryItemCode, JournalEntry, OpenItemId, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::industrial::error::map_inventory_issue;
use crate::company::industrial::{chart, IndustrialBooks, IndustrialError};

/// 赊销结果。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CreditSaleOutcome {
    pub event: BusinessEventId,
    /// 应收开项 id（`AR-{event}`）。
    pub receivable: OpenItemId,
    pub revenue: AccountingAmount,
    pub output_vat: AccountingAmount,
    pub cost_of_goods_sold: AccountingAmount,
    /// 应收挂账全额 = 收入 + 销项税。
    pub receivable_amount: AccountingAmount,
}

impl IndustrialBooks {
    /// 赊销（履约时点确认收入）：校验（客户已登记、库存充足、到期日合法）→
    /// 原子过账 → 子账（应收开项 + 存货发出）。
    pub fn sell_credit(
        &mut self,
        customer: &CounterpartyId,
        item: InventoryItemCode,
        quantity: i128,
        unit_price_excl_vat: AccountingAmount,
        due_on: CivilDate,
        date: CivilDate,
    ) -> Result<CreditSaleOutcome, IndustrialError> {
        if self.counterparties().get(customer).is_none() {
            return Err(IndustrialError::Company(
                crate::company::CompanyError::UnknownCounterparty {
                    counterparty: customer.clone(),
                },
            ));
        }
        if quantity <= 0 {
            return Err(IndustrialError::NonPositiveAmount {
                what: "sale quantity",
                amount: AccountingAmount::from_cents(quantity),
            });
        }
        if !unit_price_excl_vat.is_positive() {
            return Err(IndustrialError::NonPositiveAmount {
                what: "unit price (excl. VAT)",
                amount: unit_price_excl_vat,
            });
        }
        if due_on < date {
            return Err(IndustrialError::InvalidDueDate { due_on, date });
        }
        let cogs = self
            .inventory()
            .preview_issue(&item, quantity)
            .map_err(|error| map_inventory_issue(quantity, error))?;
        let item_account = self
            .inventory()
            .get(&item)
            .map(|state| state.account().clone())
            .expect("preview_issue validated existence");
        let revenue = unit_price_excl_vat.mul_i128(quantity)?;
        let output_vat = output_vat_on(revenue, &self.tax_policy().vat)?;
        let receivable_amount = revenue.add(output_vat)?;

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::CreditSale,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::AR, PostingSide::Debit, receivable_amount),
                    super::line(chart::acct::COGS, PostingSide::Debit, cogs),
                    super::line(chart::acct::REVENUE, PostingSide::Credit, revenue),
                    super::line(chart::acct::VAT_OUT, PostingSide::Credit, output_vat),
                    super::line(&item_account.0, PostingSide::Credit, cogs),
                ],
            }],
        )?;

        let receivable = OpenItemId(format!("AR-{}", event.value()));
        self.receivables_mut().open(
            receivable.clone(),
            &customer.0,
            date,
            due_on,
            receivable_amount,
        )?;
        self.inventory_mut()
            .apply_issue(&item, quantity)
            .map_err(|error| map_inventory_issue(quantity, error))?;
        Ok(CreditSaleOutcome {
            event,
            receivable,
            revenue,
            output_vat,
            cost_of_goods_sold: cogs,
            receivable_amount,
        })
    }

    /// 回款（可部分）：Dr 现金 / Cr 应收（经营活动）；**不确认收入**。
    pub fn collect(
        &mut self,
        receivable: &OpenItemId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        if !amount.is_positive() {
            return Err(IndustrialError::NonPositiveAmount {
                what: "collection amount",
                amount,
            });
        }
        self.receivables().check_apply(receivable, amount)?;
        let party = self
            .receivables()
            .get(receivable)
            .map(|item| CounterpartyId(item.party().to_string()))
            .expect("check_apply validated existence");
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::ReceivableCollection,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::BANK, PostingSide::Debit, amount),
                    super::line(chart::acct::AR, PostingSide::Credit, amount),
                ],
            }],
        )?;
        self.receivables_mut().apply(receivable, amount)?;
        self.counterparties_mut().record_flow(super::flow(
            date,
            &party,
            FlowDirection::Inbound,
            amount,
            "receivable collection",
        ))?;
        Ok(event)
    }

    /// 坏账准备目标化（整个存续期 ECL 简化法）：目标 = 未结应收 × 比率；
    /// 差额计提（Dr 资产减值损失 / Cr 坏账准备）或冲回，非现金。
    /// 差额为零 → `Ok(None)`（目标已达成，无分录）。
    pub fn update_bad_debt_allowance(
        &mut self,
        ecl_rate_bp: i32,
        date: CivilDate,
    ) -> Result<Option<BusinessEventId>, IndustrialError> {
        if !(0..=10_000).contains(&ecl_rate_bp) {
            return Err(IndustrialError::InvalidRateBp {
                rate_bp: ecl_rate_bp,
            });
        }
        let total_open = self.receivables().total_open()?;
        let target = ecl_allowance_target(total_open, ecl_rate_bp)?;
        let posted = self.net_of(chart::acct::BAD_DEBT_ALLOW)?.neg()?;
        let delta = target.sub(posted)?;
        if delta.is_zero() {
            return Ok(None);
        }
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let (debit_account, credit_account) = if delta.is_positive() {
            (chart::acct::IMPAIR_LOSS, chart::acct::BAD_DEBT_ALLOW)
        } else {
            (chart::acct::BAD_DEBT_ALLOW, chart::acct::IMPAIR_LOSS)
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
                kind: BusinessKind::Depreciation,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(debit_account, PostingSide::Debit, magnitude),
                    super::line(credit_account, PostingSide::Credit, magnitude),
                ],
            }],
        )?;
        Ok(Some(event))
    }

    /// 坏账核销：要求坏账准备 ≥ 开项余额（不足 → 类型化拒绝，先补提）。
    /// Dr 坏账准备 / Cr 应收（非现金，不碰现金、不冲收入）。
    pub fn write_off_receivable(
        &mut self,
        receivable: &OpenItemId,
        date: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        let open = self.receivables().open_amount(receivable)?;
        if open.is_zero() {
            return Err(IndustrialError::Trade(
                crate::accounting::TradeLedgerError::ItemCleared {
                    id: receivable.clone(),
                },
            ));
        }
        let allowance = self.net_of(chart::acct::BAD_DEBT_ALLOW)?.neg()?;
        if allowance < open {
            return Err(IndustrialError::InsufficientAllowance {
                id: receivable.clone(),
                allowance,
                open,
            });
        }
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::ReceivableCollection,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(chart::acct::BAD_DEBT_ALLOW, PostingSide::Debit, open),
                    super::line(chart::acct::AR, PostingSide::Credit, open),
                ],
            }],
        )?;
        self.receivables_mut().write_off(receivable)?;
        Ok(event)
    }
}
