//! 项目子账（K3 地产）：单一开发项目的成本轨迹 + 资本化窗口状态机。
//!
//! 成本计价：交付结转 = rhe(结存成本 × 交付套数 / 结存套数)——与任务 8
//! 存货移动加权平均同构的整数半偶舍入公式（`accounting::inventory` 的
//! `InventoryLedger` 数据面不支持「只加成本不加数量」的开发投入语义，
//! 故此处按 rhe_div 孪生先例维护同算法副本）。守恒不变量：
//! Σ结转成本 + 期末结存 == Σ投入成本（土地 + 开发 + 资本化利息），分毫不差。
//!
//! 资本化窗口（**版本化游戏假设** `game-assumption-borrowing-capitalization`：
//! CAS 17（2006 批）原文两轮取证受阻 + 任务 11 tfs 通道补证仍 404，条款号
//! 一律不引用；参数随存档版本化，不声称真实准则参数）：
//! - 窗口自**首次开发投入日**起（土地款单独不触发窗口）；
//! - 完工日（含）起终止——终止永久，此后利息必须费用化（不无限资本化）；
//! - 中断处理（阈值 = 政策 `suspension_min_days`，Fixture 默认 90 日）：
//!   闭合中断长度 ≥ 阈值 ⇒ 中断区间内费用化；< 阈值 ⇒ 照常资本化；
//!   开放中断在计提时已历时 ≥ 阈值 ⇒ 本次计提的中断日全部费用化。
//!   早前计提已资本化的天数**不追溯重述**（确定性声明，登记 issues）。

use crate::accounting::{AccountingAmount, AccountingError};
use crate::calendar::CivilDate;
use crate::company::real_estate::error::RealEstateError;

/// 项目稳定 id newtype。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct ProjectId(pub String);

/// 已闭合的开发中断区间 [start, end)。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Interruption {
    pub start: CivilDate,
    pub end: CivilDate,
}

/// 单个开发项目的子账状态。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProjectState {
    total_units: i128,
    remaining_units: i128,
    land_cost: AccountingAmount,
    development_cost: AccountingAmount,
    capitalized_interest: AccountingAmount,
    remaining_cost: AccountingAmount,
    carried_out_cost: AccountingAmount,
    dev_started_on: Option<CivilDate>,
    interrupted_on: Option<CivilDate>,
    interruptions: Vec<Interruption>,
    completed_on: Option<CivilDate>,
}

impl ProjectState {
    pub(super) fn new(land_cost: AccountingAmount, total_units: i128) -> Self {
        Self {
            total_units,
            remaining_units: total_units,
            land_cost,
            development_cost: AccountingAmount::ZERO,
            capitalized_interest: AccountingAmount::ZERO,
            remaining_cost: land_cost,
            carried_out_cost: AccountingAmount::ZERO,
            dev_started_on: None,
            interrupted_on: None,
            interruptions: Vec::new(),
            completed_on: None,
        }
    }

    pub fn total_units(&self) -> i128 {
        self.total_units
    }

    pub fn remaining_units(&self) -> i128 {
        self.remaining_units
    }

    pub fn land_cost(&self) -> AccountingAmount {
        self.land_cost
    }

    pub fn development_cost(&self) -> AccountingAmount {
        self.development_cost
    }

    pub fn capitalized_interest(&self) -> AccountingAmount {
        self.capitalized_interest
    }

    pub fn remaining_cost(&self) -> AccountingAmount {
        self.remaining_cost
    }

    pub fn carried_out_cost(&self) -> AccountingAmount {
        self.carried_out_cost
    }

    pub fn dev_started_on(&self) -> Option<CivilDate> {
        self.dev_started_on
    }

    pub fn interrupted_on(&self) -> Option<CivilDate> {
        self.interrupted_on
    }

    pub fn interruptions(&self) -> &[Interruption] {
        &self.interruptions
    }

    pub fn completed_on(&self) -> Option<CivilDate> {
        self.completed_on
    }

    /// 追加开发投入（过账成功后调用；首笔同时开启资本化窗口）。
    pub(super) fn add_development(&mut self, amount: AccountingAmount, date: CivilDate) {
        if self.dev_started_on.is_none() {
            self.dev_started_on = Some(date);
        }
        self.development_cost = self
            .development_cost
            .add(amount)
            .expect("checked at posting");
        self.remaining_cost = self.remaining_cost.add(amount).expect("checked at posting");
    }

    /// 资本化利息入成本（过账成功后调用）。
    pub(super) fn add_capitalized_interest(&mut self, amount: AccountingAmount) {
        self.capitalized_interest = self
            .capitalized_interest
            .add(amount)
            .expect("checked at posting");
        self.remaining_cost = self.remaining_cost.add(amount).expect("checked at posting");
    }

    /// 中断开始（处理器已校验前置状态）。
    pub(super) fn begin_interruption(&mut self, start: CivilDate) {
        self.interrupted_on = Some(start);
    }

    /// 中断闭合（处理器已校验方向）。
    pub(super) fn close_interruption(&mut self, gap: Interruption) {
        self.interruptions.push(gap);
        self.interrupted_on = None;
    }

    /// 完工（处理器已校验前置状态）。
    pub(super) fn mark_completed(&mut self, date: CivilDate) {
        self.completed_on = Some(date);
    }

    /// 预览交付结转成本（纯读；与 [`Self::apply_carry_out`] 同一公式）。
    pub(super) fn preview_carry_out(
        &self,
        project: &ProjectId,
        units: i128,
    ) -> Result<AccountingAmount, RealEstateError> {
        if units <= 0 {
            return Err(RealEstateError::NonPositiveUnits { units });
        }
        if units > self.remaining_units {
            return Err(RealEstateError::CarryBeyondRemainingUnits {
                project: project.clone(),
                requested: units,
                available: self.remaining_units,
            });
        }
        let scaled =
            self.remaining_cost
                .cents()
                .checked_mul(units)
                .ok_or(RealEstateError::Accounting(
                    AccountingError::AmountOverflow {
                        op: "project carry-out",
                        detail: format!("{} * {units}", self.remaining_cost.cents()),
                    },
                ))?;
        Ok(AccountingAmount::from_cents(rhe_div(
            scaled,
            self.remaining_units,
        )?))
    }

    /// 交付结转（过账成功后调用；守恒：结存按差额递减）。
    pub(super) fn apply_carry_out(&mut self, cost: AccountingAmount, units: i128) {
        self.remaining_units -= units;
        self.remaining_cost = self
            .remaining_cost
            .sub(cost)
            .expect("cost previewed against remaining");
        self.carried_out_cost = self.carried_out_cost.add(cost).expect("checked at posting");
    }

    /// 资本化窗口内的天数（[from, through) 逐日判定；分类时点 = 计提时，
    /// 使用当前已知的中断/完工状态——见模块文档的不追溯声明）。
    pub(super) fn capitalizable_days(
        &self,
        from: CivilDate,
        through: CivilDate,
        suspension_min_days: i64,
    ) -> Result<i64, RealEstateError> {
        let mut count = 0i64;
        let mut day = from;
        while day < through {
            if self.day_capitalizable(day, through, suspension_min_days) {
                count += 1;
            }
            day = day.next().map_err(RealEstateError::Calendar)?;
        }
        Ok(count)
    }

    /// 单日是否在资本化窗口内。
    fn day_capitalizable(
        &self,
        day: CivilDate,
        through: CivilDate,
        suspension_min_days: i64,
    ) -> bool {
        let started = self.dev_started_on.is_some_and(|start| day >= start);
        let ceased = self.completed_on.is_some_and(|completed| day >= completed);
        started && !ceased && !self.day_suspended(day, through, suspension_min_days)
    }

    /// 单日是否落在暂停资本化的中断内。
    fn day_suspended(&self, day: CivilDate, through: CivilDate, min_days: i64) -> bool {
        for gap in &self.interruptions {
            if day >= gap.start && day < gap.end && gap.end.days_since(gap.start) >= min_days {
                return true;
            }
        }
        match self.interrupted_on {
            // 开放中断：计提时已历时 ≥ 阈值 ⇒ 中断日起全部费用化。
            Some(start) => day >= start && through.days_since(start) >= min_days,
            None => false,
        }
    }
}

/// 整数半偶舍入除法 `n/d`（d > 0）。任务 8 `accounting::inventory::rhe_div`
/// 同算法孪生（原件 pub(in crate::accounting) 对 company 域不可见，
/// 与 industrial::loans / company::bank::loans 先例一致）。
pub(super) fn rhe_div(n: i128, d: i128) -> Result<i128, RealEstateError> {
    if d <= 0 {
        return Err(RealEstateError::Accounting(
            AccountingError::AmountOverflow {
                op: "rhe_div",
                detail: format!("divisor {d} must be positive"),
            },
        ));
    }
    let negative = n < 0;
    let numerator = n.unsigned_abs();
    let divisor = d.unsigned_abs();
    let quotient = numerator / divisor;
    let remainder = numerator % divisor;
    let doubled = remainder * 2;
    let round_up = doubled > divisor || (doubled == divisor && !quotient.is_multiple_of(2));
    let magnitude = i128::try_from(if round_up { quotient + 1 } else { quotient })
        .expect("quotient of |i128| by positive divisor fits i128");
    Ok(if negative { -magnitude } else { magnitude })
}
