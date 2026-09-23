//! 税务政策与纯计算（共享，K3）：增值税销项/进项拆分 + 当期/递延所得税。
//!
//! **无生产默认值**：增值税/企业所得税现行税率法源（`vat-law-current`/
//! `cit-law-current`）取证受阻（docs/company-accounting.md §7）——本模块只提供
//! 显式版本化政策 + 校验；测试/游戏配置必须显式注入并自行标注 Fixture 合成，
//! 不得宣称真实税率。增值税为价外税（财会〔2016〕22号已核验：销项/进项分开、
//! 不可抵扣进项按政策归集）。
//!
//! 所得税（游戏简化，CAS 18 全文受阻 ⛔）：应税 = max(0, 期间税前 − 可抵扣亏损
//! FIFO 弥补)；当期税 = 应税 × 税率（半偶舍入）。递延所得税资产 = 期末未用亏损
//! × 税率**全额确认**（不确认门槛/折现，不声称完整 CAS 18 合规）。亏损结转
//! 年限为政策参数（到期出池，先到期先弥补）。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use thiserror::Error;

/// 增值税政策（价外，基点）。`deductible_share_bp` = 进项可抵扣比例
/// （10000 = 全抵；不足部分按政策归集进成本/费用，财会〔2016〕22号）。
#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct VatPolicy {
    pub output_rate_bp: i32,
    pub input_rate_bp: i32,
    pub deductible_share_bp: i32,
}

/// 企业所得税政策（基点 + 亏损结转年限）。
#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct IncomeTaxPolicy {
    pub rate_bp: i32,
    pub loss_carryforward_years: u16,
}

/// 版本化税务政策（显式配置；无默认构造器——生产默认值待税法取证解除阻塞）。
#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct TaxPolicy {
    pub version: u32,
    pub vat: VatPolicy,
    pub income_tax: IncomeTaxPolicy,
}

impl TaxPolicy {
    /// 结构校验：税率/比例 ∈ [0, 10000]bp、结转年限 ≥ 1、版本 ≥ 1。
    pub fn validate(&self) -> Result<(), TaxPolicyError> {
        let bad = |detail: String| Err(TaxPolicyError::Invalid { detail });
        if self.version == 0 {
            return bad("version must be >= 1".to_string());
        }
        let rates = [
            ("vat.output_rate_bp", self.vat.output_rate_bp),
            ("vat.input_rate_bp", self.vat.input_rate_bp),
            ("vat.deductible_share_bp", self.vat.deductible_share_bp),
            ("income_tax.rate_bp", self.income_tax.rate_bp),
        ];
        for (name, bp) in rates {
            if !(0..=10_000).contains(&bp) {
                return bad(format!("{name} = {bp}bp out of [0, 10000]"));
            }
        }
        if self.income_tax.loss_carryforward_years == 0 {
            return bad("loss_carryforward_years must be >= 1".to_string());
        }
        Ok(())
    }
}

/// 税务政策非法（越界税率/比例或版本）。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum TaxPolicyError {
    #[error("invalid tax policy: {detail}")]
    Invalid { detail: String },
}

/// 销项税额 = 不含税基数 × 销项税率（半偶舍入落分）。
pub fn output_vat_on(
    base_excl_vat: AccountingAmount,
    vat: &VatPolicy,
) -> Result<AccountingAmount, AccountingError> {
    base_excl_vat.apply_basis_points(vat.output_rate_bp)
}

/// 进项税额拆分：全额进项 = 基数 × 进项税率；可抵扣 = 全额 × 抵扣比例；
/// 不可抵扣 = 全额 − 可抵扣（调用方按政策归集进存货成本等）。
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct InputVatSplit {
    pub deductible: AccountingAmount,
    pub non_deductible: AccountingAmount,
}

pub fn split_input_vat(
    base_excl_vat: AccountingAmount,
    vat: &VatPolicy,
) -> Result<InputVatSplit, AccountingError> {
    let full = base_excl_vat.apply_basis_points(vat.input_rate_bp)?;
    let deductible = full.apply_basis_points(vat.deductible_share_bp)?;
    Ok(InputVatSplit {
        deductible,
        non_deductible: full.sub(deductible)?,
    })
}

/// 可抵扣亏损池条目（起源年 + 剩余金额；FIFO 弥补，到期出池）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct LossEntry {
    pub origin_year: i32,
    pub remaining: AccountingAmount,
}

/// 所得税计算结果（纯函数输出；亏损池更新由调用方在过账成功后落地）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct IncomeTaxComputation {
    pub pretax: AccountingAmount,
    pub current_tax: AccountingAmount,
    pub loss_offset_used: AccountingAmount,
    pub loss_added: AccountingAmount,
    pub losses_expired: AccountingAmount,
    pub ending_pool: Vec<LossEntry>,
    /// 期末未用亏损 × 税率（全额确认简化）。
    pub deferred_tax_asset: AccountingAmount,
}

/// 当期 + 递延所得税（纯函数）：到期亏损出池 → 亏损 FIFO 弥补正税前 →
/// 当期税 = 应税 × 税率；负税前新增亏损条目；DTA = 期末池 × 税率。
pub fn compute_income_tax(
    pretax: AccountingAmount,
    year: i32,
    pool: &[LossEntry],
    policy: &IncomeTaxPolicy,
) -> Result<IncomeTaxComputation, AccountingError> {
    // 到期出池：year − origin_year > 结转年限 ⇔ 不可再用。
    let mut surviving: Vec<LossEntry> = Vec::new();
    let mut losses_expired = AccountingAmount::ZERO;
    for entry in pool {
        if year.saturating_sub(entry.origin_year) > i32::from(policy.loss_carryforward_years) {
            losses_expired = losses_expired.add(entry.remaining)?;
        } else if entry.remaining.is_positive() {
            surviving.push(entry.clone());
        }
    }
    // FIFO 弥补（池已按起源年有序传入；此处再按起源年稳定排序）。
    surviving.sort_by_key(|entry| entry.origin_year);
    let mut loss_offset_used = AccountingAmount::ZERO;
    if pretax.is_positive() {
        for entry in &mut surviving {
            if loss_offset_used >= pretax {
                break;
            }
            let room = pretax.sub(loss_offset_used)?;
            let take = if entry.remaining > room {
                room
            } else {
                entry.remaining
            };
            entry.remaining = entry.remaining.sub(take)?;
            loss_offset_used = loss_offset_used.add(take)?;
        }
    }
    let taxable = if pretax.is_positive() {
        pretax.sub(loss_offset_used)?
    } else {
        AccountingAmount::ZERO
    };
    let current_tax = taxable.apply_basis_points(policy.rate_bp)?;
    let mut ending_pool: Vec<LossEntry> = surviving
        .into_iter()
        .filter(|e| e.remaining.is_positive())
        .collect();
    let loss_added = if pretax.is_negative() {
        pretax.neg()?
    } else {
        AccountingAmount::ZERO
    };
    if loss_added.is_positive() {
        ending_pool.push(LossEntry {
            origin_year: year,
            remaining: loss_added,
        });
    }
    let mut pool_total = AccountingAmount::ZERO;
    for entry in &ending_pool {
        pool_total = pool_total.add(entry.remaining)?;
    }
    let deferred_tax_asset = pool_total.apply_basis_points(policy.rate_bp)?;
    Ok(IncomeTaxComputation {
        pretax,
        current_tax,
        loss_offset_used,
        loss_added,
        losses_expired,
        ending_pool,
        deferred_tax_asset,
    })
}
