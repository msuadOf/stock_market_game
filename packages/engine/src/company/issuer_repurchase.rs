//! 发行人回购的公司层基础契约（ADR-0038，2026-10-07 M 批）。
//!
//! 官方口径（摘录与证据边界见 `agents/company-system/remaining-corporate-rules-research.md`
//! 与 `docs/trading-rules.md`）：
//! - 现行依据为证监会《上市公司股份回购规则》（〔2023〕63 号公布，经〔2025〕5 号
//!   修改与新《公司法》衔接）与沪深交易所回购股份自律监管指引（沪 7 号 2025 年
//!   3 月修订／深 9 号 2025 年重新发布）。
//! - 集中竞价委托限制（63 号第 30 条，已核全文）：申报价格不得为当日涨幅限制的
//!   价格；不得在开盘集合竞价、收盘集合竞价及无涨跌幅限制交易日委托。窗口禁止
//!   （第 31 条）等细则在本游戏不建模，显式登记不支持。
//! - 期限（63 号第 11 条）：一般用途最长 12 个月、维护公司价值最长 3 个月——
//!   本模块参数化窗口日期，不硬编码期限。
//! - 专户失权（63 号第 13 条；沪 7 号 2025 修订第 22 条同文）：回购股份过户至
//!   专用账户即丧失表决、利润分配、公积金转增、认购新股（含配股）等权利，不得
//!   质押出借；计算指标时从总股本扣减——股东名册以 `HolderId::IssuerTreasury`
//!   表达，既有分红／送转排除规则直接复用。
//! - 注销（63 号第 17 条／《公司法》第 162 条）：减资回购应自收购之日起十日内
//!   注销；其他用途合计持有不超过已发行股份 10%、三年内转让、未转让则注销。
//!   回购注销不除权：交易所除权公式只覆盖股份增加情形，注销无除权条文，市场
//!   实践不除权（登记为待证口径，无官方明文）。
//! - 资金（ADR-0038）：获批计划额度内合成发行人结算资金（专款语义），经真实
//!   委托与成交买入；卖方投资者真实收到资金（投资者资金池注入）。计划额度与
//!   实际成交差额无官方规则——本游戏口径：未用差额在计划完成时显式回收，
//!   不落地为任何人余额、不记为公司资产（登记为游戏口径）。

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::CompanyId;
use crate::account::StockCode;
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::money::Money;

/// 回购法定用途（63 号第 2 条四类）。用途决定法定期限与注销义务；本模块不硬编码
/// 期限，由方案窗口参数承载。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RepurchasePurpose {
    /// 减少注册资本（注销式回购）。
    ReduceCapital,
    /// 员工持股计划或股权激励。
    EmployeeIncentive,
    /// 转换上市公司发行的可转换为股票的公司债券。
    ConvertibleConversion,
    /// 维护公司价值及股东权益所必需。
    ValueMaintenance,
}

/// 回购方案：额度、价格上限与执行窗口（期限参数化：一般 12 个月／维护价值
/// 3 个月由调用方按用途显式给出，不硬编码）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssuerRepurchasePlan {
    pub event_id: String,
    pub approval_reference: String,
    pub issuer: CompanyId,
    pub stock: StockCode,
    pub exchange: CalendarExchange,
    pub approved_on: CivilDate,
    pub announced_on: CivilDate,
    /// 执行窗口首个交易日（含）。
    pub window_start_on: CivilDate,
    /// 执行窗口截止交易日（含）；窗口内未用完的额度在窗口截止后的首个日终完成回收。
    pub window_deadline_on: CivilDate,
    /// 回购价格上限（申报仍受当日涨停价与价格笼子约束——执行层按撮合规则处理）。
    pub price_cap_per_share: Money,
    /// 获批计划额度（合成资金总额）。
    pub total_budget: Money,
    /// 数量上限（63 号第 17 条：非减资用途合计持有不超过已发行股份 10%；
    /// 方案显式给出，模块不校验总股本比例——由 Session 按名册核验）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub max_shares: u64,
    pub purpose: RepurchasePurpose,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum IssuerRepurchaseError {
    #[error("issuer repurchase: {detail}")]
    InvalidPlan { detail: String },
    #[error("issuer repurchase: trading calendar lookup failed: {0}")]
    CalendarLookup(String),
    #[error(
        "issuer repurchase: date {date} is not a {exchange:?} trading day"
    )]
    InvalidTradingDate {
        date: CivilDate,
        exchange: CalendarExchange,
    },
    #[error("issuer repurchase: operation {operation} is invalid in status {status:?}")]
    WrongStage {
        status: IssuerRepurchaseStatus,
        operation: &'static str,
    },
    #[error(
        "issuer repurchase: fill record {day} {gross:?} is inconsistent with remaining budget"
    )]
    FillExceedsBudget {
        day: CivilDate,
        gross: Money,
    },
    #[error("issuer repurchase: cancellation shares {requested} exceed completed fills {completed}")]
    CancellationExceedsFills { requested: u64, completed: u64 },
}

fn invalid(detail: &str) -> IssuerRepurchaseError {
    IssuerRepurchaseError::InvalidPlan {
        detail: detail.to_owned(),
    }
}

impl IssuerRepurchasePlan {
    pub fn validate(&self) -> Result<(), IssuerRepurchaseError> {
        if self.event_id.trim().is_empty()
            || self.approval_reference.trim().is_empty()
            || self.issuer.0.trim().is_empty()
            || self.stock.0.trim().is_empty()
        {
            return Err(invalid("identities and approval reference are required"));
        }
        if self.price_cap_per_share <= Money::ZERO || self.total_budget <= Money::ZERO {
            return Err(invalid("price cap and budget must be positive"));
        }
        if self.max_shares == 0 {
            return Err(invalid("share cap must be positive"));
        }
        if !(self.approved_on <= self.announced_on
            && self.announced_on <= self.window_start_on
            && self.window_start_on <= self.window_deadline_on)
        {
            return Err(invalid(
                "dates must satisfy approved <= announced <= window start <= window deadline",
            ));
        }
        Ok(())
    }

    /// 窗口日期均为该交易所交易日。
    pub fn validate_calendar(
        &self,
        calendar: &TradingCalendar,
    ) -> Result<(), IssuerRepurchaseError> {
        for date in [self.window_start_on, self.window_deadline_on] {
            let trading = calendar
                .is_trading_day(self.exchange, date)
                .map_err(|error| IssuerRepurchaseError::CalendarLookup(error.to_string()))?;
            if !trading {
                return Err(IssuerRepurchaseError::InvalidTradingDate {
                    date,
                    exchange: self.exchange,
                });
            }
        }
        Ok(())
    }

    pub fn window_contains(&self, day: CivilDate) -> bool {
        self.window_start_on <= day && day <= self.window_deadline_on
    }
}

/// 单个交易日回购成交汇总（真实委托经既有订单簿撮合的成交；不含伪造成交）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepurchaseFillRecord {
    pub stock: StockCode,
    pub day: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub shares: u64,
    /// 成交金额（不含费用）。
    pub gross: Money,
    /// 真实交易费用（佣金＋过户费；回购买入无印花税）。
    pub fees: Money,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum IssuerRepurchaseStatus {
    Approved,
    Announced,
    Executing,
    /// 窗口结束或额度／数量用尽：未用合成资金已回收。
    Completed,
    /// Completed 且注销执行完成（减资用途）。
    Cancelled,
}

/// 回购执行状态机：显式方案 → 公告 → 窗口内真实委托成交 → 计划完成（回收未用
/// 合成资金）→ 注销（减资用途）。真实现金、账户与订单由 Session 侧执行。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "IssuerRepurchaseBookState")]
#[serde(deny_unknown_fields)]
pub struct IssuerRepurchaseBook {
    plan: IssuerRepurchasePlan,
    status: IssuerRepurchaseStatus,
    fills: Vec<RepurchaseFillRecord>,
    completed_on: Option<CivilDate>,
    /// 计划完成时回收的未用合成资金（含费用余量）。
    withdrawn_remainder: Option<Money>,
    cancelled_on: Option<CivilDate>,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    cancelled_shares: u64,
    /// 执行器最近一次下委托的自然日（每日至多一单的幂等去重）。
    last_order_day: Option<CivilDate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IssuerRepurchaseBookState {
    plan: IssuerRepurchasePlan,
    status: IssuerRepurchaseStatus,
    fills: Vec<RepurchaseFillRecord>,
    completed_on: Option<CivilDate>,
    withdrawn_remainder: Option<Money>,
    cancelled_on: Option<CivilDate>,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    cancelled_shares: u64,
    last_order_day: Option<CivilDate>,
}

impl TryFrom<IssuerRepurchaseBookState> for IssuerRepurchaseBook {
    type Error = IssuerRepurchaseError;

    fn try_from(state: IssuerRepurchaseBookState) -> Result<Self, Self::Error> {
        let book = Self {
            plan: state.plan,
            status: state.status,
            fills: state.fills,
            completed_on: state.completed_on,
            withdrawn_remainder: state.withdrawn_remainder,
            cancelled_on: state.cancelled_on,
            cancelled_shares: state.cancelled_shares,
            last_order_day: state.last_order_day,
        };
        book.validate()?;
        Ok(book)
    }
}

impl IssuerRepurchaseBook {
    pub fn new(plan: IssuerRepurchasePlan) -> Result<Self, IssuerRepurchaseError> {
        plan.validate()?;
        Ok(Self {
            plan,
            status: IssuerRepurchaseStatus::Approved,
            fills: Vec::new(),
            completed_on: None,
            withdrawn_remainder: None,
            cancelled_on: None,
            cancelled_shares: 0,
            last_order_day: None,
        })
    }

    pub fn plan(&self) -> &IssuerRepurchasePlan {
        &self.plan
    }
    pub fn status(&self) -> &IssuerRepurchaseStatus {
        &self.status
    }
    pub fn fills(&self) -> &[RepurchaseFillRecord] {
        &self.fills
    }
    pub fn completed_on(&self) -> Option<CivilDate> {
        self.completed_on
    }
    pub fn withdrawn_remainder(&self) -> Option<Money> {
        self.withdrawn_remainder
    }
    pub fn cancelled_on(&self) -> Option<CivilDate> {
        self.cancelled_on
    }
    pub fn cancelled_shares(&self) -> u64 {
        self.cancelled_shares
    }
    pub fn last_order_day(&self) -> Option<CivilDate> {
        self.last_order_day
    }
    pub fn set_last_order_day(&mut self, day: CivilDate) {
        self.last_order_day = Some(day);
    }

    pub fn announce(&mut self, on: CivilDate) -> Result<(), IssuerRepurchaseError> {
        if self.status != IssuerRepurchaseStatus::Approved {
            return if on == self.plan.announced_on {
                Ok(())
            } else {
                Err(IssuerRepurchaseError::WrongStage {
                    status: self.status.clone(),
                    operation: "announce on a conflicting date",
                })
            };
        }
        if on != self.plan.announced_on {
            return Err(IssuerRepurchaseError::WrongStage {
                status: self.status.clone(),
                operation: "announce",
            });
        }
        self.status = IssuerRepurchaseStatus::Announced;
        Ok(())
    }

    pub fn total_filled_shares(&self) -> u64 {
        self.fills.iter().map(|fill| fill.shares).sum()
    }

    pub fn total_spent(&self) -> Result<Money, IssuerRepurchaseError> {
        self.fills.iter().try_fold(Money::ZERO, |sum, fill| {
            sum.add(fill.gross)
                .and_then(|sum| sum.add(fill.fees))
                .map_err(|_| IssuerRepurchaseError::FillExceedsBudget {
                    day: fill.day,
                    gross: fill.gross,
                })
        })
    }

    /// 剩余可用额度（含费用余量口径：直接按总额度减累计支出）。
    pub fn remaining_budget(&self) -> Result<Money, IssuerRepurchaseError> {
        self.plan
            .total_budget
            .sub(self.total_spent()?)
            .map_err(|_| {
                IssuerRepurchaseError::FillExceedsBudget {
                    day: self.plan.window_deadline_on,
                    gross: self.plan.total_budget,
                }
            })
    }

    /// 记录一个交易日的真实成交汇总（窗口内、额度内、数量上限内）。
    pub fn record_fill(&mut self, fill: RepurchaseFillRecord) -> Result<(), IssuerRepurchaseError> {
        if !matches!(
            self.status,
            IssuerRepurchaseStatus::Announced | IssuerRepurchaseStatus::Executing
        ) {
            return Err(IssuerRepurchaseError::WrongStage {
                status: self.status.clone(),
                operation: "record fill",
            });
        }
        if !self.plan.window_contains(fill.day) {
            return Err(invalid("fill day is outside the execution window"));
        }
        if fill.shares == 0 || fill.gross <= Money::ZERO || fill.fees < Money::ZERO {
            return Err(invalid("fill quantity, gross and fees must be positive"));
        }
        if self.fills.iter().any(|existing| existing.day == fill.day) {
            return Err(invalid("fill day already recorded; one net record per day"));
        }
        let spent_after = self
            .total_spent()?
            .add(fill.gross)
            .and_then(|sum| sum.add(fill.fees))
            .map_err(|_| IssuerRepurchaseError::FillExceedsBudget {
                day: fill.day,
                gross: fill.gross,
            })?;
        if spent_after > self.plan.total_budget {
            return Err(IssuerRepurchaseError::FillExceedsBudget {
                day: fill.day,
                gross: fill.gross,
            });
        }
        if self.total_filled_shares() + fill.shares > self.plan.max_shares {
            return Err(invalid("fill exceeds the approved share cap"));
        }
        self.fills.push(fill);
        self.fills.sort_by(|left, right| left.day.cmp(&right.day));
        self.status = IssuerRepurchaseStatus::Executing;
        Ok(())
    }

    /// 计划完成：窗口截止（或额度／数量用尽后调用方显式完成）时回收未用合成资金。
    pub fn complete(
        &mut self,
        on: CivilDate,
        withdrawn_remainder: Money,
    ) -> Result<(), IssuerRepurchaseError> {
        if self.status == IssuerRepurchaseStatus::Completed {
            return if self.completed_on == Some(on)
                && self.withdrawn_remainder == Some(withdrawn_remainder)
            {
                Ok(())
            } else {
                Err(IssuerRepurchaseError::WrongStage {
                    status: self.status.clone(),
                    operation: "complete on conflicting facts",
                })
            };
        }
        if !matches!(
            self.status,
            IssuerRepurchaseStatus::Announced | IssuerRepurchaseStatus::Executing
        ) {
            return Err(IssuerRepurchaseError::WrongStage {
                status: self.status.clone(),
                operation: "complete",
            });
        }
        if on < self.plan.window_start_on {
            return Err(invalid("completion cannot precede the execution window"));
        }
        let spent = self.total_spent()?;
        if withdrawn_remainder
            != self
                .plan
                .total_budget
                .sub(spent)
                .map_err(|_| IssuerRepurchaseError::FillExceedsBudget {
                    day: on,
                    gross: spent,
                })?
        {
            return Err(invalid(
                "withdrawn remainder must equal budget minus total spent",
            ));
        }
        self.completed_on = Some(on);
        self.withdrawn_remainder = Some(withdrawn_remainder);
        self.status = IssuerRepurchaseStatus::Completed;
        Ok(())
    }

    /// 注销执行（减资用途）：注销股数不得超过累计成交股数；同一日重复幂等。
    pub fn record_cancellation(
        &mut self,
        on: CivilDate,
        shares: u64,
    ) -> Result<(), IssuerRepurchaseError> {
        if shares == 0 {
            return Err(invalid("cancelled shares must be positive"));
        }
        if self.cancelled_on.is_some() {
            return if self.cancelled_on == Some(on) && self.cancelled_shares == shares {
                Ok(())
            } else {
                Err(IssuerRepurchaseError::WrongStage {
                    status: self.status.clone(),
                    operation: "record cancellation on conflicting facts",
                })
            };
        }
        if self.status != IssuerRepurchaseStatus::Completed {
            return Err(IssuerRepurchaseError::WrongStage {
                status: self.status.clone(),
                operation: "record cancellation",
            });
        }
        let filled = self.total_filled_shares();
        if shares > filled {
            return Err(IssuerRepurchaseError::CancellationExceedsFills {
                requested: shares,
                completed: filled,
            });
        }
        if on < self.completed_on.unwrap_or(on) {
            return Err(invalid("cancellation cannot precede plan completion"));
        }
        self.cancelled_on = Some(on);
        self.cancelled_shares = shares;
        self.status = IssuerRepurchaseStatus::Cancelled;
        Ok(())
    }

    /// 结构一致性校验（恢复路径也走这里）。
    pub fn validate(&self) -> Result<(), IssuerRepurchaseError> {
        self.plan.validate()?;
        if self.total_spent().is_err() || self.remaining_budget().is_err() {
            return Err(IssuerRepurchaseError::FillExceedsBudget {
                day: self.plan.window_deadline_on,
                gross: self.plan.total_budget,
            });
        }
        if self.total_filled_shares() > self.plan.max_shares {
            return Err(invalid("fills exceed the approved share cap"));
        }
        match (self.status.clone(), self.completed_on, self.withdrawn_remainder) {
            (IssuerRepurchaseStatus::Approved, None, None) => {}
            (IssuerRepurchaseStatus::Announced, None, None) => {}
            (IssuerRepurchaseStatus::Executing, None, None) => {}
            (IssuerRepurchaseStatus::Completed, Some(_), Some(_)) => {}
            (IssuerRepurchaseStatus::Cancelled, Some(_), Some(_)) => {}
            _ => {
                return Err(invalid(
                    "status does not agree with completion facts",
                ));
            }
        }
        if matches!(
            self.status,
            IssuerRepurchaseStatus::Executing | IssuerRepurchaseStatus::Announced
        ) && !self.fills.is_empty()
            && self.fills.iter().any(|fill| !self.plan.window_contains(fill.day))
        {
            return Err(invalid("fill day outside the execution window"));
        }
        if (self.cancelled_on.is_some() || self.cancelled_shares > 0)
            && (self.status != IssuerRepurchaseStatus::Cancelled
                || self.cancelled_shares > self.total_filled_shares())
        {
            return Err(invalid("cancellation facts are inconsistent"));
        }
        Ok(())
    }

    pub fn validate_with_calendar(
        &self,
        calendar: &TradingCalendar,
    ) -> Result<(), IssuerRepurchaseError> {
        self.validate()?;
        self.plan.validate_calendar(calendar)
    }
}

/// Simple 账面回购事实：合成资金来源与去向都显式登记（ADR-0038：发行人侧记账面
/// 合成来源事实，不追踪真实公司资金链；不伪造公司现金科目）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssuerRepurchaseFinanceFact {
    pub event_id: String,
    pub approval_reference: String,
    pub approved_on: CivilDate,
    /// 获批计划额度（凭空生成的合成资金总额）。
    pub synthetic_funding: crate::accounting::AccountingAmount,
    pub purpose: RepurchasePurpose,
    /// 实际成交支出（含费用；计划完成时回填）。
    pub spent: Option<crate::accounting::AccountingAmount>,
    /// 回收的未用合成资金（计划完成时回填）。
    pub withdrawn_remainder: Option<crate::accounting::AccountingAmount>,
    pub completed_on: Option<CivilDate>,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub cancelled_shares: u64,
    pub cancelled_on: Option<CivilDate>,
    /// 注销的注册资本减少额 = 面值 × 注销股数（注销时回填）。
    pub capital_reduction: Option<crate::accounting::AccountingAmount>,
}

#[cfg(test)]
#[path = "issuer_repurchase_tests.rs"]
mod tests;
