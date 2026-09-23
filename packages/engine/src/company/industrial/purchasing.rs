//! 采购与应付结算（K3 工商）：现付/赊购、进项税拆分、应付开项与逾期面。
//!
//! 增值税为价外税（财会〔2016〕22号，已核验）：存货按不含税成本入账，进项
//! 可抵扣部分记 222102 借方，不可抵扣部分按政策归集进存货成本。赊购不动现金
//! （非现金分录），到期现付结清；资金不足 → `PaymentFailed`，应付保持开项并
//! 进入 `payables().overdue()` 逾期面（不补钱、不透支）。
//!
//! BusinessKind 标签映射（journal.rs 属任务 6 语义冻结区，行业枚举扩充前以最
//! 接近的通用标签记录，见 issues.md 登记）：现付采购 = CashExpense/Operating，
//! 赊购 = CreditSale/NonCash（非现金信用交易），应付结清 = CashExpense/Operating。

use crate::accounting::{
    split_input_vat, AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, InputVatSplit,
    InventoryItemCode, JournalEntry, LedgerAccountId, OpenItemId, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::industrial::{chart, IndustrialBooks, IndustrialError};

/// 采购结算方式。
#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Settlement {
    /// 现付（含税全额现金流出，经营活动；`due_on` 不使用）。
    Cash,
    /// 赊购（应付开项，非现金；`due_on` 为到期日）。
    Credit,
}

/// 采购结果。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct PurchaseOutcome {
    pub event: BusinessEventId,
    /// 不含税货款。
    pub goods: AccountingAmount,
    /// 进项税额拆分（可抵扣 / 不可抵扣）。
    pub input_vat: InputVatSplit,
    /// 赊购开立的应付开项 id（现付 = None）。
    pub payable: Option<OpenItemId>,
}

impl IndustrialBooks {
    /// 采购入库：Dr 存货（货款+不可抵扣进项）、Dr 222102（可抵扣进项）/
    /// Cr 现金或应付（货款+全部进项）。先验证后原子过账，再入子账。
    #[allow(clippy::too_many_arguments)] // 结算方式/到期日与货款参数是正交经营输入
    pub fn purchase(
        &mut self,
        supplier: &CounterpartyId,
        item: InventoryItemCode,
        account: LedgerAccountId,
        quantity: i128,
        unit_price_excl_vat: AccountingAmount,
        settlement: Settlement,
        due_on: CivilDate,
        date: CivilDate,
    ) -> Result<PurchaseOutcome, IndustrialError> {
        if self.counterparties().get(supplier).is_none() {
            return Err(IndustrialError::Company(
                crate::company::CompanyError::UnknownCounterparty {
                    counterparty: supplier.clone(),
                },
            ));
        }
        if quantity <= 0 {
            return Err(IndustrialError::NonPositiveAmount {
                what: "purchase quantity",
                amount: AccountingAmount::from_cents(quantity),
            });
        }
        if !unit_price_excl_vat.is_positive() {
            return Err(IndustrialError::NonPositiveAmount {
                what: "unit price (excl. VAT)",
                amount: unit_price_excl_vat,
            });
        }
        let goods = unit_price_excl_vat.mul_i128(quantity)?;
        let split = split_input_vat(goods, &self.tax_policy().vat)?;
        let vat_full = split.deductible.add(split.non_deductible)?;
        let total_payment = goods.add(vat_full)?;
        let inventory_cost = goods.add(split.non_deductible)?;

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let (kind, cash_flow, credit_account) = match settlement {
            Settlement::Cash => (
                BusinessKind::CashExpense,
                CashFlowClass::Operating,
                chart::acct::BANK,
            ),
            Settlement::Credit => (
                BusinessKind::CreditSale,
                CashFlowClass::NonCash,
                chart::acct::PAYABLE,
            ),
        };
        let mut lines = vec![
            super::line(&account.0, PostingSide::Debit, inventory_cost),
            super::line(chart::acct::VAT_IN, PostingSide::Debit, split.deductible),
            super::line(credit_account, PostingSide::Credit, total_payment),
        ];
        if split.deductible.is_zero() {
            lines.remove(1); // 行金额恒正：零进项不设行
        }
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind,
                cash_flow,
                lines,
            }],
        )?;

        self.inventory_mut()
            .receipt(item, account, quantity, inventory_cost)?;
        let payable = match settlement {
            Settlement::Cash => {
                self.counterparties_mut().record_flow(super::flow(
                    date,
                    supplier,
                    FlowDirection::Outbound,
                    total_payment,
                    "purchase cash payment",
                ))?;
                None
            }
            Settlement::Credit => {
                let id = OpenItemId(format!("AP-{}", event.value()));
                self.payables_mut()
                    .open(id.clone(), &supplier.0, date, due_on, total_payment)?;
                Some(id)
            }
        };
        Ok(PurchaseOutcome {
            event,
            goods,
            input_vat: split,
            payable,
        })
    }

    /// 结清应付开项（全额）：Dr 应付 / Cr 现金（经营活动）。资金不足 →
    /// `PaymentFailed`，开项保持（逾期面由 `payables().overdue()` 呈现）。
    pub fn settle_payable(
        &mut self,
        payable: &OpenItemId,
        date: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        let open = self.payables().open_amount(payable)?;
        if open.is_zero() {
            return Err(IndustrialError::Trade(
                crate::accounting::TradeLedgerError::ItemCleared {
                    id: payable.clone(),
                },
            ));
        }
        let party = self
            .payables()
            .get(payable)
            .map(|item| CounterpartyId(item.party().to_string()))
            .expect("open_amount validated existence");
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::CashExpense,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::PAYABLE, PostingSide::Debit, open),
                    super::line(chart::acct::BANK, PostingSide::Credit, open),
                ],
            }],
        )?;
        self.payables_mut().apply(payable, open)?;
        self.counterparties_mut().record_flow(super::flow(
            date,
            &party,
            FlowDirection::Outbound,
            open,
            "payable settlement",
        ))?;
        Ok(event)
    }
}
