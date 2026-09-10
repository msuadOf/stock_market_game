//! 固定资产子账（共享，K3）：资产登记 + 直线折旧 + 减值面。
//!
//! 折旧（每资产独立守恒）：月折旧 = rhe(剩余应折旧基础 / 剩余月数)，应折旧基础
//! = 成本 − 残值 − 累计折旧 − 累计减值；按剩余基础/剩余月数推进 ⇒ Σ各月折旧 +
//! 累计减值 == 成本 − 残值，分毫不差（末月自然带走剩余）。减值后不改变剩余
//! 寿命，以新基础继续直线摊销（游戏简化；CAS 8 全文取证受阻 docs §2.1 ⛔，
//! 不声称其原文依据）。折旧/减值均为**非现金**事项——过账由调用方完成。
//!
//! 行业中立：无工商概念；银行/保险/地产（任务 9–11）复用同一登记/折旧面。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::inventory::rhe_div;
use thiserror::Error;

/// 固定资产代码 newtype。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct FixedAssetCode(pub String);

/// 单个固定资产的登记与计量状态。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct FixedAssetEntry {
    cost: AccountingAmount,
    salvage_value: AccountingAmount,
    life_months: i64,
    depreciated_months: i64,
    accumulated_depreciation: AccountingAmount,
    accumulated_impairment: AccountingAmount,
}

impl FixedAssetEntry {
    /// 账面价值 = 成本 − 累计折旧 − 累计减值（checked）。
    pub fn carrying_amount(&self) -> Result<AccountingAmount, AccountingError> {
        self.cost
            .sub(self.accumulated_depreciation)?
            .sub(self.accumulated_impairment)
    }

    /// 剩余应折旧基础 = 成本 − 残值 − 累计折旧 − 累计减值（checked，≥ 0 不变量）。
    fn depreciable_base_remaining(&self) -> Result<AccountingAmount, AccountingError> {
        self.cost
            .sub(self.salvage_value)?
            .sub(self.accumulated_depreciation)?
            .sub(self.accumulated_impairment)
    }

    pub fn cost(&self) -> AccountingAmount {
        self.cost
    }

    pub fn salvage_value(&self) -> AccountingAmount {
        self.salvage_value
    }

    pub fn life_months(&self) -> i64 {
        self.life_months
    }

    pub fn depreciated_months(&self) -> i64 {
        self.depreciated_months
    }

    pub fn remaining_months(&self) -> i64 {
        self.life_months - self.depreciated_months
    }

    pub fn accumulated_depreciation(&self) -> AccountingAmount {
        self.accumulated_depreciation
    }

    pub fn accumulated_impairment(&self) -> AccountingAmount {
        self.accumulated_impairment
    }
}

/// 固定资产登记簿（与总账同存同档）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FixedAssetRegister {
    assets: BTreeMap<FixedAssetCode, FixedAssetEntry>,
}

impl FixedAssetRegister {
    /// 登记前纯校验（计量政策）：正成本、0 ≤ 残值 ≤ 成本、寿命 ≥ 1 月。
    pub fn validate_policy(
        cost: AccountingAmount,
        salvage_value: AccountingAmount,
        life_months: i64,
    ) -> Result<(), FixedAssetError> {
        if !cost.is_positive() {
            return Err(policy_invalid("cost must be positive"));
        }
        if salvage_value.is_negative() || salvage_value > cost {
            return Err(policy_invalid("salvage must be within [0, cost]"));
        }
        if life_months < 1 {
            return Err(policy_invalid("life_months must be >= 1"));
        }
        Ok(())
    }

    /// 登记（策略校验 + 查重）。
    pub fn register(
        &mut self,
        code: FixedAssetCode,
        cost: AccountingAmount,
        salvage_value: AccountingAmount,
        life_months: i64,
    ) -> Result<(), FixedAssetError> {
        Self::validate_policy(cost, salvage_value, life_months)?;
        if self.assets.contains_key(&code) {
            return Err(FixedAssetError::DuplicateAsset { code });
        }
        self.assets.insert(
            code,
            FixedAssetEntry {
                cost,
                salvage_value,
                life_months,
                depreciated_months: 0,
                accumulated_depreciation: AccountingAmount::ZERO,
                accumulated_impairment: AccountingAmount::ZERO,
            },
        );
        Ok(())
    }

    /// 下月折旧预览（纯读；寿命已满 → `FullyDepreciated`）。
    pub fn preview_depreciation(
        &self,
        code: &FixedAssetCode,
    ) -> Result<AccountingAmount, FixedAssetError> {
        let entry = self.entry(code)?;
        if entry.remaining_months() <= 0 {
            return Err(FixedAssetError::FullyDepreciated { code: code.clone() });
        }
        let base = entry
            .depreciable_base_remaining()
            .map_err(FixedAssetError::Accounting)?;
        let cents = rhe_div(base.cents(), i128::from(entry.remaining_months()))
            .map_err(FixedAssetError::Accounting)?;
        Ok(AccountingAmount::from_cents(cents))
    }

    /// 计提一月折旧（预览同一公式，推进状态；守恒由结构保证）。
    pub fn apply_depreciation(
        &mut self,
        code: &FixedAssetCode,
    ) -> Result<AccountingAmount, FixedAssetError> {
        let depreciation = self.preview_depreciation(code)?;
        let entry = self
            .assets
            .get_mut(code)
            .expect("preview_depreciation validated existence");
        entry.accumulated_depreciation = entry
            .accumulated_depreciation
            .add(depreciation)
            .map_err(FixedAssetError::Accounting)?;
        entry.depreciated_months += 1;
        Ok(depreciation)
    }

    /// 减值前纯校验：正金额且不超过（账面 − 残值）——残值是折旧/减值的共同下限。
    pub fn validate_impairment(
        &self,
        code: &FixedAssetCode,
        amount: AccountingAmount,
    ) -> Result<(), FixedAssetError> {
        if !amount.is_positive() {
            return Err(policy_invalid("impairment amount must be positive"));
        }
        let entry = self.entry(code)?;
        let floor = entry
            .carrying_amount()
            .map_err(FixedAssetError::Accounting)?
            .sub(entry.salvage_value)
            .map_err(FixedAssetError::Accounting)?;
        if amount > floor {
            return Err(FixedAssetError::ImpairmentBeyondFloor {
                code: code.clone(),
                requested: amount,
                floor,
            });
        }
        Ok(())
    }

    /// 计提减值（验证后入账；剩余寿命内以新基础继续摊销）。
    pub fn apply_impairment(
        &mut self,
        code: &FixedAssetCode,
        amount: AccountingAmount,
    ) -> Result<(), FixedAssetError> {
        self.validate_impairment(code, amount)?;
        let entry = self
            .assets
            .get_mut(code)
            .expect("validate_impairment validated existence");
        entry.accumulated_impairment = entry
            .accumulated_impairment
            .add(amount)
            .map_err(FixedAssetError::Accounting)?;
        Ok(())
    }

    pub fn get(&self, code: &FixedAssetCode) -> Option<&FixedAssetEntry> {
        self.assets.get(code)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&FixedAssetCode, &FixedAssetEntry)> {
        self.assets.iter()
    }

    fn entry(&self, code: &FixedAssetCode) -> Result<&FixedAssetEntry, FixedAssetError> {
        self.assets
            .get(code)
            .ok_or(FixedAssetError::UnknownAsset { code: code.clone() })
    }
}

fn policy_invalid(reason: &str) -> FixedAssetError {
    FixedAssetError::AssetPolicyInvalid {
        reason: reason.to_string(),
    }
}

/// 固定资产子账错误（类型化）。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum FixedAssetError {
    #[error("unknown fixed asset {code:?}")]
    UnknownAsset { code: FixedAssetCode },
    #[error("duplicate fixed asset {code:?}")]
    DuplicateAsset { code: FixedAssetCode },
    #[error("invalid asset measurement policy: {reason}")]
    AssetPolicyInvalid { reason: String },
    #[error("asset {code:?} is fully depreciated")]
    FullyDepreciated { code: FixedAssetCode },
    #[error("impairment {requested:?} of {code:?} exceeds floor (carrying − salvage) {floor:?}")]
    ImpairmentBeyondFloor {
        code: FixedAssetCode,
        requested: AccountingAmount,
        floor: AccountingAmount,
    },
    #[error(transparent)]
    Accounting(#[from] AccountingError),
}
