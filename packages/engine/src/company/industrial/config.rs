//! 工商账套装配输入与开局种子对账（工商会计约束）：配置值类型 + 构造期守卫
//! （子账种子与总账逐科目精确对账、开局借款隐式合同与 2001 余额对账）。
//! 全部为显式配置（无生产默认值——税务默认税率待税法取证解除阻塞，
//! docs/company-accounting.md §7）。

use std::collections::BTreeMap;

use crate::accounting::{
    AccountingAmount, FixedAssetCode, FixedAssetRegister, InventoryItemCode, InventoryLedger,
    JournalLine, LedgerAccountId, TaxPolicy,
};
use crate::calendar::CivilDate;
use crate::company::contracts::{
    ContractBook, ContractId, DayCountBasis, OperatingBudget, OperatingContract,
};
use crate::company::counterparty::CounterpartyId;
use crate::company::industrial::loans::LoanPortfolio;
use crate::company::industrial::{chart, IndustrialError, OPENING_DEBT_CONTRACT_ID};

/// 开局借款条款：构造时与开局 2001 贷方余额精确对账（不匹配 → 类型化拒绝），
/// 并作为隐式合同（`OPENING_DEBT_CONTRACT_ID`）进入同一套计息/付息/授信机制
/// ——授信占用自动包含开局债务（历史复核来源 task-7 review O2）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OpeningDebtTerms {
    pub lender: CounterpartyId,
    pub principal: AccountingAmount,
    pub annual_rate_bp: i32,
    pub maturity_date: CivilDate,
}

/// 开局存货种子：数量/成本子账必须与对应总账科目余额精确一致。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OpeningInventoryItem {
    pub account: LedgerAccountId,
    pub item: InventoryItemCode,
    pub quantity: i128,
    pub cost: AccountingAmount,
}

/// 开局固定资产种子（全新资产；开局累计折旧暂不支持——前史由自然日经营演化生成）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OpeningAssetItem {
    pub code: FixedAssetCode,
    pub cost: AccountingAmount,
    pub salvage_value: AccountingAmount,
    pub life_months: i64,
}

/// 工商账套装配输入。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct IndustrialConfig {
    pub chart: crate::accounting::AccountChart,
    pub as_of: CivilDate,
    /// 显式平衡的开局行（经 `post_batch` 验证路径，公司规格与开局账套语义）。
    pub opening_lines: Vec<JournalLine>,
    pub opening_inventory: Vec<OpeningInventoryItem>,
    pub opening_assets: Vec<OpeningAssetItem>,
    pub opening_debt: Option<OpeningDebtTerms>,
    pub counterparties: Vec<crate::company::ExternalCounterparty>,
    pub budget: OperatingBudget,
    /// 显式版本化税务政策（无默认构造——生产默认税率待税法取证解除阻塞）。
    pub tax_policy: TaxPolicy,
}

/// 已过账总账与 config 开局种子的窄借用视图；分阶段核对不复制开局事实。
pub(super) struct IndustrialOpeningReconciliation<'a> {
    pub(super) ledger: &'a crate::accounting::Ledger,
    pub(super) as_of: CivilDate,
    pub(super) inventory_seeds: &'a [OpeningInventoryItem],
    pub(super) asset_seeds: &'a [OpeningAssetItem],
    pub(super) debt_terms: Option<&'a OpeningDebtTerms>,
}

impl IndustrialOpeningReconciliation<'_> {
    /// inventory/assets 必须先于 counterparty 登记与 debt 核对。
    pub(super) fn seed_inventory_and_assets(
        &self,
    ) -> Result<(InventoryLedger, FixedAssetRegister), IndustrialError> {
        let inventory = self.seed_inventory()?;
        let assets = self.seed_assets()?;
        Ok((inventory, assets))
    }

    /// 已完成 counterparty 登记后才核对 debt；未配置时保留原空合同/贷款接受集。
    pub(super) fn seed_debt_after_counterparties(
        &self,
        counterparties: &crate::company::counterparty::CounterpartyLedger,
    ) -> Result<(ContractBook, LoanPortfolio), IndustrialError> {
        match self.debt_terms {
            Some(terms) => {
                self.seed_opening_debt(terms, counterparties.get(&terms.lender).is_some())
            }
            None => Ok((ContractBook::new(), LoanPortfolio::default())),
        }
    }

    /// 开局存货种子对账：逐项入账子账 + 按科目与总账余额精确比对。
    fn seed_inventory(&self) -> Result<InventoryLedger, IndustrialError> {
        let mut inventory = InventoryLedger::default();
        let mut seeded: BTreeMap<LedgerAccountId, AccountingAmount> = BTreeMap::new();
        for seed in self.inventory_seeds {
            inventory.receipt(
                seed.item.clone(),
                seed.account.clone(),
                seed.quantity,
                seed.cost,
            )?;
            let total = seeded
                .entry(seed.account.clone())
                .or_insert(AccountingAmount::ZERO);
            *total = total.add(seed.cost)?;
        }
        for (account, seeded_total) in &seeded {
            let ledger_balance = self.ledger.account_net_debit(account)?;
            if &ledger_balance != seeded_total {
                return Err(IndustrialError::OpeningSeedMismatch {
                    account: account.clone(),
                    ledger: ledger_balance,
                    seeded: *seeded_total,
                });
            }
        }
        Ok(inventory)
    }

    /// 开局固定资产种子对账：登记 + 1601 余额精确比对；1602 非零 → 诚实拒绝。
    fn seed_assets(&self) -> Result<FixedAssetRegister, IndustrialError> {
        let mut assets = FixedAssetRegister::default();
        let mut asset_cost = AccountingAmount::ZERO;
        for seed in self.asset_seeds {
            assets.register(
                seed.code.clone(),
                seed.cost,
                seed.salvage_value,
                seed.life_months,
            )?;
            asset_cost = asset_cost.add(seed.cost)?;
        }
        let fixed_asset_balance = self
            .ledger
            .account_net_debit(&LedgerAccountId(chart::acct::FIXED_ASSET.to_string()))?;
        if fixed_asset_balance != asset_cost {
            return Err(IndustrialError::OpeningSeedMismatch {
                account: LedgerAccountId(chart::acct::FIXED_ASSET.to_string()),
                ledger: fixed_asset_balance,
                seeded: asset_cost,
            });
        }
        let acc_dep = self
            .ledger
            .account_net_debit(&LedgerAccountId(chart::acct::ACC_DEP.to_string()))?;
        if acc_dep != AccountingAmount::ZERO {
            return Err(IndustrialError::OpeningAccumulatedDepreciation {
                balance: acc_dep.neg()?,
            });
        }
        Ok(assets)
    }

    /// 开局借款隐式合同：与 2001 贷方余额精确对账后登记合同与借款状态。
    fn seed_opening_debt(
        &self,
        terms: &OpeningDebtTerms,
        lender_registered: bool,
    ) -> Result<(ContractBook, LoanPortfolio), IndustrialError> {
        if !lender_registered {
            return Err(IndustrialError::Company(
                crate::company::CompanyError::UnknownCounterparty {
                    counterparty: terms.lender.clone(),
                },
            ));
        }
        let st_debt_balance = self
            .ledger
            .account_net_debit(&LedgerAccountId(chart::acct::ST_DEBT.to_string()))?;
        let st_debt_credit = st_debt_balance.neg()?;
        if st_debt_credit != terms.principal {
            return Err(IndustrialError::OpeningDebtMismatch {
                ledger: st_debt_credit,
                configured: terms.principal,
            });
        }
        let contract = OperatingContract {
            id: ContractId(OPENING_DEBT_CONTRACT_ID.to_string()),
            role: crate::company::ContractRole::Borrowing,
            counterparty: terms.lender.clone(),
            principal: terms.principal,
            annual_rate_bp: terms.annual_rate_bp,
            start_date: self.as_of,
            maturity_date: terms.maturity_date,
            basis: DayCountBasis::Act365F,
        };
        contract.validate()?;
        let mut contracts = ContractBook::new();
        contracts.register(contract.clone())?;
        let mut loans = LoanPortfolio::default();
        loans.insert_registered_loan(contract.id, terms.principal, self.as_of);
        Ok((contracts, loans))
    }
}
