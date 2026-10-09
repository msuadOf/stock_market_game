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
//! - 处置策略（H 批 2026-10-08 用户决策「目前暂时回购默认注销，其他方案先
//!   不做，可以留一个空的接口」）：完成判定后的首个日终自动注销全部已回购
//!   股份；库存股持有、转让等路径显式不做，`RepurchaseCompletionPolicy`
//!   枚举留扩展位。
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
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

/// 回购完成后的股份处置策略（H 批 2026-10-08 用户决策：「目前暂时回购默认
/// 注销，其他方案先不做，可以留一个空的接口」）。
///
/// 当前**唯一变体** = 完成判定后的首个日终自动注销全部已回购股份（复用既有
/// 手动注销路径）；库存股持有、转让等其他处置路径按用户决策显式不做——枚举
/// 只留类型与序列化扩展位，新增变体时必须同步 Session 日终自动处置逻辑与
/// Web 存档 parser 域（`corporate-actions.ts`），不得静默落空。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum RepurchaseCompletionPolicy {
    /// 完成判定（预算／数量用尽或窗口截止）后的首个日终自动注销。
    CancelOnCompletion,
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
    /// 完成后的股份处置策略（H 批默认注销；扩展位，当前唯一变体）。
    pub completion_policy: RepurchaseCompletionPolicy,
    /// 批准股份单位锚（必填 nullable，无 serde 默认）：`max_shares` 与
    /// `price_cap_per_share` 所处交易单位对应的拆股／缩股事件 id；`None` 表示
    /// 开局基准单位（该证券尚无已生效重新计值）。受理时必须与审批当刻真实
    /// 已应用的交易单位一致（Session 按 split 链核验），方案一经批准不可变。
    #[serde(deserialize_with = "required_unit_anchor")]
    pub approval_unit_event_id: Option<String>,
}

/// 股份单位调整事实（严格持久化，必填字段、无 serde 默认）：某次拆股／缩股
/// 重新计值对本 plan 当时库存的汇总换算结果。当前库存不由任意可改的计数器
/// 承载，而是由 fills（不可改写）+ 本调整事实 + 注销事实按 ex 日序重放推导。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareUnitAdjustment {
    pub split_event_id: String,
    /// 换算前库存（换算事件 ex 日之前累积的当前单位股数）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub shares_before: u64,
    /// 换算后库存（缩股碎股奖励含在内，与 Treasury 分桶分配结果一致）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub shares_after: u64,
}

/// 单位换算上下文：某证券已登记的拆股／缩股事件链（由 Session 从 share_splits
/// 账簿投影；不含未登记事件）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitUnitContext {
    pub event_id: String,
    pub ex_on: CivilDate,
    pub direction: crate::company::share_split::ShareSplitDirection,
    pub ratio: u64,
}

/// 批准锚单位 → `through` 日交易单位的精确换算比例 (分子 p, 分母 q)：
/// 新股数 = 旧股数 × p / q。锚 `None` 表示开局基准单位（链上全部 ex ≤ through
/// 的重新计值都计入）；锚为具体事件时从该事件之后（ex 严格大于锚 ex）计入。
pub fn unit_conversion_factor(
    anchor: Option<&str>,
    through: CivilDate,
    context: &[SplitUnitContext],
) -> Result<(u64, u64), IssuerRepurchaseError> {
    let anchor_ex = match anchor {
        None => None,
        Some(event_id) => {
            if event_id.trim().is_empty() {
                return Err(invalid("approval unit anchor event id must not be blank"));
            }
            Some(
                context
                    .iter()
                    .find(|entry| entry.event_id == event_id)
                    .ok_or_else(|| {
                        invalid(&format!(
                            "approval unit anchor {event_id} is not a registered split event of this stock"
                        ))
                    })?
                    .ex_on,
            )
        }
    };
    let mut numerator = 1_u64;
    let mut denominator = 1_u64;
    for entry in context {
        if entry.ratio < 2 {
            return Err(invalid("share unit ratio must be at least two"));
        }
        if entry.ex_on > through {
            continue;
        }
        // 锚事件自身的单位即批准单位，不重复计入。
        if anchor_ex.is_some_and(|anchor_ex| entry.ex_on <= anchor_ex) {
            continue;
        }
        match entry.direction {
            crate::company::share_split::ShareSplitDirection::Split => {
                numerator = numerator.checked_mul(entry.ratio).ok_or(
                    IssuerRepurchaseError::InvalidPlan {
                        detail: "share unit conversion factor overflow".into(),
                    },
                )?;
            }
            crate::company::share_split::ShareSplitDirection::Consolidate => {
                denominator = denominator.checked_mul(entry.ratio).ok_or(
                    IssuerRepurchaseError::InvalidPlan {
                        detail: "share unit conversion factor overflow".into(),
                    },
                )?;
            }
        }
    }
    Ok((numerator, denominator))
}

/// 精确比例向下取整换算：`value × p / q`（u128 中间量，不静默饱和）。
pub fn convert_shares_floor(
    value: u64,
    numerator: u64,
    denominator: u64,
) -> Result<u64, IssuerRepurchaseError> {
    if denominator == 0 {
        return Err(invalid("share unit denominator cannot be zero"));
    }
    u64::try_from(u128::from(value) * u128::from(numerator) / u128::from(denominator))
        .map_err(|_| invalid("converted share quantity exceeds u64"))
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
    #[error(
        "issuer repurchase: cancellation shares {requested} exceed current inventory {available}"
    )]
    CancellationExceedsInventory { requested: u64, available: u64 },
    #[error("issuer repurchase: fill {fill} would exceed the execution cap {cap} in current share units (inventory {inventory})")]
    FillExceedsExecutionCap { cap: u64, inventory: u64, fill: u64 },
    #[error("issuer repurchase: share unit replay failed: {detail}")]
    UnitReplay { detail: String },
}

fn invalid(detail: &str) -> IssuerRepurchaseError {
    IssuerRepurchaseError::InvalidPlan {
        detail: detail.to_owned(),
    }
}

/// 必填 nullable 的 serde 辅助：字段缺键显式拒绝、null 合法（与 DayNetChange
/// .acquisition 的 required_acquisition 同一仓库既有惯例，不引入兼容默认）。
fn required_unit_anchor<'de, Decoder: serde::Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<String>, Decoder::Error> {
    Option::<String>::deserialize(decoder)
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
        if self
            .approval_unit_event_id
            .as_ref()
            .is_some_and(|event_id| event_id.trim().is_empty())
        {
            return Err(invalid(
                "approval unit anchor must be null or a non-blank split event id",
            ));
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

    /// `through` 日交易单位下的数量执行上限：批准锚单位 → 该单位精确比例向下取整。
    pub fn execution_max_shares(
        &self,
        through: CivilDate,
        split_context: &[SplitUnitContext],
    ) -> Result<u64, IssuerRepurchaseError> {
        let (numerator, denominator) = unit_conversion_factor(
            self.approval_unit_event_id.as_deref(),
            through,
            split_context,
        )?;
        convert_shares_floor(self.max_shares, numerator, denominator)
    }

    /// `through` 日交易单位下的申报价上限（分）：批准价上限按精确比例反向换算
    /// （价格单位与股数单位互为倒数）并向下取最小价位；不足一个价位时返回 0
    ///（执行器跳过当日委托，不伪造价格）。
    pub fn execution_price_cap_cents(
        &self,
        through: CivilDate,
        tick_cents: i64,
        split_context: &[SplitUnitContext],
    ) -> Result<i64, IssuerRepurchaseError> {
        let (numerator, denominator) = unit_conversion_factor(
            self.approval_unit_event_id.as_deref(),
            through,
            split_context,
        )?;
        if tick_cents <= 0 {
            return Err(invalid("minimum price tick must be positive"));
        }
        let cap_cents = self.price_cap_per_share.cents().unsigned_abs() as u128;
        let tick = tick_cents.unsigned_abs() as u128;
        let scaled = cap_cents * u128::from(denominator) / u128::from(numerator);
        let floored = scaled / tick * tick;
        i64::try_from(floored).map_err(|_| IssuerRepurchaseError::InvalidPlan {
            detail: "converted price cap exceeds the money domain".into(),
        })
    }

    /// 基准锚已包含的事件不能因回填审批日期再调整一次。
    pub fn unit_event_follows_approval_anchor(&self, event_id: &str, context: &[SplitUnitContext]) -> Result<bool, IssuerRepurchaseError> {
        let event = context.iter().find(|entry| entry.event_id == event_id).ok_or_else(|| invalid("unknown unit event"))?;
        match self.approval_unit_event_id.as_deref() {
            None => Ok(true),
            Some(anchor_id) => {
                let anchor = context.iter().find(|entry| entry.event_id == anchor_id).ok_or_else(|| invalid("unknown approval unit anchor"))?;
                Ok(event.ex_on > anchor.ex_on)
            }
        }
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
    /// 窗口结束或额度／数量用尽：未用合成资金已回收。H 批起为**短暂过渡态**：
    /// 默认注销策略下，完成判定后的首个日终自动注销（`Cancelled`）；零成交
    /// 方案无股份可核减时保持本终态。
    Completed,
    /// Completed 且注销执行完成（默认注销策略自动或显式 API 手动执行）。
    Cancelled,
}

/// 回购执行状态机：显式方案 → 公告 → 窗口内真实委托成交 → 计划完成（回收未用
/// 合成资金）→ 注销（H 批默认策略：完成后的首个日终自动执行）。真实现金、
/// 账户与订单由 Session 侧执行。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "IssuerRepurchaseBookState")]
#[serde(deny_unknown_fields)]
pub struct IssuerRepurchaseBook {
    plan: IssuerRepurchasePlan,
    status: IssuerRepurchaseStatus,
    fills: Vec<RepurchaseFillRecord>,
    /// 股份单位调整事实链（严格持久化；当前库存按 fills + 本链 + 注销事实重放推导）。
    share_unit_adjustments: Vec<ShareUnitAdjustment>,
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
    share_unit_adjustments: Vec<ShareUnitAdjustment>,
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
            share_unit_adjustments: state.share_unit_adjustments,
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
            share_unit_adjustments: Vec::new(),
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
    pub fn share_unit_adjustments(&self) -> &[ShareUnitAdjustment] {
        &self.share_unit_adjustments
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

    /// 记录一个交易日的真实成交汇总（窗口内、额度内、数量上限内）。数量上限按
    /// 「批准锚单位 → 成交日交易单位」的精确比例向下取整执行；缩股合法碎股奖励
    /// 允许库存高于 floor(cap)（award 计数抵扣），不为奖励拒绝真实成交。
    pub fn record_fill(
        &mut self,
        fill: RepurchaseFillRecord,
        split_context: &[SplitUnitContext],
    ) -> Result<(), IssuerRepurchaseError> {
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
        let cap = self.plan.execution_max_shares(fill.day, split_context)?;
        let (inventory, _) = self.replay_inventory(
            split_context,
            Some((fill.day, Self::REPLAY_RANK_FILL)),
            true,
        )?;
        // 奖励只解释既有库存，不增加新的成交额度。
        if fill.shares > cap.saturating_sub(inventory) {
            return Err(IssuerRepurchaseError::FillExceedsExecutionCap {
                cap,
                inventory,
                fill: fill.shares,
            });
        }
        self.fills.push(fill);
        self.fills.sort_by(|left, right| left.day.cmp(&right.day));
        self.status = IssuerRepurchaseStatus::Executing;
        Ok(())
    }

    /// 记录一次拆股／缩股对本 plan 库存的汇总换算（Session 在重新计值入账时调用）。
    /// `shares_before` 必须与当前重放库存一致，换算结果须与事件方向/比例一致，
    /// 不接受凭空改写（重放校验显式拒绝数量篡改）。
    pub fn record_share_unit_adjustment(
        &mut self,
        adjustment: ShareUnitAdjustment,
        split_context: &[SplitUnitContext],
    ) -> Result<(), IssuerRepurchaseError> {
        if adjustment.split_event_id.trim().is_empty() {
            return Err(IssuerRepurchaseError::UnitReplay {
                detail: "share unit adjustment requires the split event identity".into(),
            });
        }
        if self
            .share_unit_adjustments
            .iter()
            .any(|existing| existing.split_event_id == adjustment.split_event_id)
        {
            return Err(IssuerRepurchaseError::UnitReplay {
                detail: format!(
                    "split event {} already adjusted this plan's inventory",
                    adjustment.split_event_id
                ),
            });
        }
        let entry = split_context
            .iter()
            .find(|entry| entry.event_id == adjustment.split_event_id)
            .ok_or_else(|| IssuerRepurchaseError::UnitReplay {
                detail: format!(
                    "split event {} is not registered for this stock's unit chain",
                    adjustment.split_event_id
                ),
            })?
            .clone();
        // 零库存同样经历单位事件，但不得借碎股奖励凭空取得股份。
        if adjustment.shares_before == 0 && adjustment.shares_after != 0 {
            return Err(IssuerRepurchaseError::UnitReplay {
                detail: "zero inventory must remain zero through a share unit adjustment".into(),
            });
        }
        let (inventory, _) = self.replay_inventory(
            split_context,
            Some((entry.ex_on, Self::REPLAY_RANK_ADJUSTMENT)),
            true,
        )?;
        if inventory != adjustment.shares_before {
            return Err(IssuerRepurchaseError::UnitReplay {
                detail: format!(
                    "adjustment {} claims {} shares before, but replayed inventory is {}",
                    adjustment.split_event_id, adjustment.shares_before, inventory
                ),
            });
        }
        match entry.direction {
            crate::company::share_split::ShareSplitDirection::Split => {
                let expected = adjustment.shares_before.checked_mul(entry.ratio).ok_or(
                    IssuerRepurchaseError::UnitReplay {
                        detail: "split adjustment overflow".into(),
                    },
                )?;
                if adjustment.shares_after != expected {
                    return Err(IssuerRepurchaseError::UnitReplay {
                        detail: format!(
                            "split adjustment {} must multiply inventory exactly (expected {expected})"
                            , adjustment.split_event_id
                        ),
                    });
                }
            }
            crate::company::share_split::ShareSplitDirection::Consolidate => {
                let base = adjustment.shares_before / entry.ratio;
                if adjustment.shares_after != base && adjustment.shares_after != base + 1 {
                    return Err(IssuerRepurchaseError::UnitReplay {
                        detail: format!(
                            "consolidation adjustment {} must equal the base share or the base plus one legal fractional award",
                            adjustment.split_event_id
                        ),
                    });
                }
            }
        }
        self.share_unit_adjustments.push(adjustment);
        self.share_unit_adjustments
            .sort_by(|left, right| left.split_event_id.cmp(&right.split_event_id));
        Ok(())
    }

    /// 当前库存（按 fills + 单位调整链 + 注销事实重放推导；不读历史成交合计）。
    pub fn current_inventory_shares(
        &self,
        split_context: &[SplitUnitContext],
    ) -> Result<u64, IssuerRepurchaseError> {
        Ok(self.replay_inventory(split_context, None, true)?.0)
    }

    /// 某次拆股／缩股 ex 日**之前**的库存（恢复复核分桶 uses_before 时使用）。
    pub fn inventory_before_split(
        &self,
        split_event_id: &str,
        split_context: &[SplitUnitContext],
    ) -> Result<u64, IssuerRepurchaseError> {
        let entry = split_context
            .iter()
            .find(|entry| entry.event_id == split_event_id)
            .ok_or_else(|| IssuerRepurchaseError::UnitReplay {
                detail: format!(
                    "split event {split_event_id} is not registered for this stock's unit chain"
                ),
            })?;
        Ok(self
            .replay_inventory(
                split_context,
                Some((entry.ex_on, Self::REPLAY_RANK_ADJUSTMENT)),
                true,
            )?
            .0)
    }

    /// 单位上下文校验（Session validate/restore 调用；结构性 validate 不含链信息）：
    /// 全量重放校验调整一致性与注销不超当前份额，并核对当前库存不超过
    /// 「批准锚 → as_of 交易单位」的执行上限 + 合法碎股奖励数。
    pub fn validate_share_units(
        &self,
        split_context: &[SplitUnitContext],
        as_of: CivilDate,
    ) -> Result<(), IssuerRepurchaseError> {
        let (inventory, awards) = self.replay_inventory(split_context, None, true)?;
        // 存档时钟可能已指向下一个 ex 日；库存单位只由实际成交/已记录单位事实决定。
        let mut unit_date = self
            .fills
            .iter()
            .map(|fill| fill.day)
            .max()
            .unwrap_or(self.plan.approved_on);
        for adjustment in &self.share_unit_adjustments {
            let entry = split_context
                .iter()
                .find(|entry| entry.event_id == adjustment.split_event_id)
                .ok_or_else(|| invalid("unknown share unit adjustment"))?;
            unit_date = unit_date.max(entry.ex_on);
        }
        if unit_date > as_of {
            return Err(invalid("inventory fact is later than validation date"));
        }
        let cap = self.plan.execution_max_shares(unit_date, split_context)?;
        let allowed = cap
            .checked_add(awards)
            .ok_or_else(|| invalid("inventory allowance overflow"))?;
        if inventory > allowed {
            return Err(IssuerRepurchaseError::UnitReplay {
                detail: format!(
                    "current inventory {inventory} exceeds the execution cap {cap} beyond legal fractional awards ({awards})"
                ),
            });
        }
        Ok(())
    }

    /// 重放事件秩：同一天内「单位调整（ex 日日终入账）先于当日新成交、注销最后」。
    pub(crate) const REPLAY_RANK_ADJUSTMENT: u8 = 0;
    pub(crate) const REPLAY_RANK_FILL: u8 = 1;
    const REPLAY_RANK_CANCELLATION: u8 = 2;

    /// 按 (日期, 秩) 重放 fills + 单位调整 + 注销，返回 (库存, 合法碎股奖励数)。
    /// `cutoff = Some((day, rank))` 时只重放严格早于该秩点的事件；注销事件由
    /// `include_cancellation` 控制（注销前的库存核验需要排除注销本身）。
    fn replay_inventory(
        &self,
        split_context: &[SplitUnitContext],
        cutoff: Option<(CivilDate, u8)>,
        include_cancellation: bool,
    ) -> Result<(u64, u64), IssuerRepurchaseError> {
        let mut events: Vec<(CivilDate, u8, u64)> = Vec::new();
        for adjustment in &self.share_unit_adjustments {
            let entry = split_context
                .iter()
                .find(|entry| entry.event_id == adjustment.split_event_id)
                .ok_or_else(|| IssuerRepurchaseError::UnitReplay {
                    detail: format!(
                        "share unit adjustment references unknown split event {}",
                        adjustment.split_event_id
                    ),
                })?;
            events.push((entry.ex_on, Self::REPLAY_RANK_ADJUSTMENT, 0));
        }
        for fill in &self.fills {
            events.push((fill.day, Self::REPLAY_RANK_FILL, fill.shares));
        }
        if include_cancellation {
            if let Some(cancelled_on) = self.cancelled_on {
                events.push((
                    cancelled_on,
                    Self::REPLAY_RANK_CANCELLATION,
                    self.cancelled_shares,
                ));
            }
        }
        events.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
        let mut inventory: u64 = 0;
        let mut awards: u64 = 0;
        let mut unit_cap = self.plan.max_shares;
        for (day, rank, _) in &events {
            if let Some((cutoff_day, cutoff_rank)) = cutoff {
                if (*day, *rank) >= (cutoff_day, cutoff_rank) {
                    break;
                }
            }
            match rank {
                rank if *rank == Self::REPLAY_RANK_ADJUSTMENT => {
                    let adjustment = self
                        .share_unit_adjustments
                        .iter()
                        .find(|adjustment| {
                            split_context.iter().any(|entry| {
                                entry.event_id == adjustment.split_event_id && entry.ex_on == *day
                            })
                        })
                        .expect("adjustment events were built from this book");
                    let entry = split_context
                        .iter()
                        .find(|entry| entry.event_id == adjustment.split_event_id)
                        .expect("context entry existence was checked above");
                    if inventory != adjustment.shares_before {
                        return Err(IssuerRepurchaseError::UnitReplay {
                            detail: format!(
                                "adjustment {} claims {} shares before, but replayed inventory is {}",
                                adjustment.split_event_id, adjustment.shares_before, inventory
                            ),
                        });
                    }
                    if adjustment.shares_before == 0 && adjustment.shares_after != 0 {
                        return Err(invalid("zero inventory cannot receive a fractional award"));
                    }
                    match entry.direction {
                        crate::company::share_split::ShareSplitDirection::Split => {
                            let expected = adjustment.shares_before.checked_mul(entry.ratio);
                            if expected != Some(adjustment.shares_after) {
                                return Err(IssuerRepurchaseError::UnitReplay {
                                    detail: format!(
                                        "split adjustment {} does not multiply inventory exactly",
                                        adjustment.split_event_id
                                    ),
                                });
                            }
                        }
                        crate::company::share_split::ShareSplitDirection::Consolidate => {
                            let base = adjustment.shares_before / entry.ratio;
                            if adjustment.shares_after != base
                                && adjustment.shares_after != base + 1
                            {
                                return Err(IssuerRepurchaseError::UnitReplay {
                                    detail: format!(
                                        "consolidation adjustment {} is neither the base share nor the base plus one legal award",
                                        adjustment.split_event_id
                                    ),
                                });
                            }
                        }
                    }
                    // 合并上界先换算，避免分别 floor(cap) 和 floor(award) 丢进位。
                    let bound = unit_cap
                        .checked_add(awards)
                        .ok_or_else(|| invalid("inventory allowance overflow"))?;
                    let (numerator, denominator) = match entry.direction {
                        crate::company::share_split::ShareSplitDirection::Split => (entry.ratio, 1),
                        crate::company::share_split::ShareSplitDirection::Consolidate => {
                            (1, entry.ratio)
                        }
                    };
                    let converted = u64::try_from(
                        u128::from(bound) * u128::from(numerator) / u128::from(denominator),
                    )
                    .map_err(|_| invalid("converted inventory allowance overflow"))?;
                    let cap_after = self.plan.execution_max_shares(entry.ex_on, split_context)?;
                    let actual_award = u64::from(
                        matches!(
                            entry.direction,
                            crate::company::share_split::ShareSplitDirection::Consolidate
                        ) && adjustment.shares_after > adjustment.shares_before / entry.ratio,
                    );
                    awards = converted
                        .saturating_sub(cap_after)
                        .checked_add(actual_award)
                        .ok_or_else(|| invalid("inventory allowance overflow"))?;
                    unit_cap = cap_after;
                    inventory = adjustment.shares_after;
                }
                rank if *rank == Self::REPLAY_RANK_FILL => {
                    let fill = self
                        .fills
                        .iter()
                        .find(|fill| fill.day == *day)
                        .expect("fill events were built from this book");
                    let cap = self.plan.execution_max_shares(fill.day, split_context)?;
                    if fill.shares > cap.saturating_sub(inventory) {
                        return Err(invalid(
                            "historical fill exceeds its current-unit approved room",
                        ));
                    }
                    inventory = inventory.checked_add(fill.shares).ok_or(
                        IssuerRepurchaseError::UnitReplay {
                            detail: "replayed inventory overflow".into(),
                        },
                    )?;
                }
                _ => {
                    if inventory < self.cancelled_shares {
                        return Err(IssuerRepurchaseError::UnitReplay {
                            detail: format!(
                                "cancellation of {} shares exceeds replayed inventory {inventory}",
                                self.cancelled_shares
                            ),
                        });
                    }
                    inventory -= self.cancelled_shares;
                }
            }
        }
        Ok((inventory, awards))
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

    /// 注销执行（减资用途）：注销股数不得超过**当前重放库存**（含单位换算与
    /// 历史部分注销残量），不得用历史成交合计限制当前单位股数；同一日重复幂等。
    pub fn record_cancellation(
        &mut self,
        on: CivilDate,
        shares: u64,
        split_context: &[SplitUnitContext],
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
        let available = self.replay_inventory(split_context, None, false)?.0;
        if shares > available {
            return Err(IssuerRepurchaseError::CancellationExceedsInventory {
                requested: shares,
                available,
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

    /// 结构一致性校验（恢复路径也走这里；无单位链信息的检查，含单位换算的
    /// 完整复核由 `validate_share_units` 在 Session validate/restore 执行）。
    pub fn validate(&self) -> Result<(), IssuerRepurchaseError> {
        self.plan.validate()?;
        if self.total_spent().is_err() || self.remaining_budget().is_err() {
            return Err(IssuerRepurchaseError::FillExceedsBudget {
                day: self.plan.window_deadline_on,
                gross: self.plan.total_budget,
            });
        }
        // 数量上限按批准锚单位表达；跨单位执行上限的重放校验在
        // validate_share_units（需要 split 链上下文）。
        let mut adjustment_ids = std::collections::BTreeSet::new();
        for adjustment in &self.share_unit_adjustments {
            if adjustment.split_event_id.trim().is_empty()
                || (adjustment.shares_before == 0 && adjustment.shares_after != 0)
                || !adjustment_ids.insert(adjustment.split_event_id.clone())
            {
                return Err(invalid(
                    "share unit adjustments must reference distinct split events; zero inventory must stay zero",
                ));
            }
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
            && (self.status != IssuerRepurchaseStatus::Cancelled || self.cancelled_shares == 0)
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
