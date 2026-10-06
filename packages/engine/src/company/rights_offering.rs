//! 配股／增发（面向全体股东的配股与定向增发）的公司层基础契约。
//!
//! 官方口径（摘录与证据边界见 `agents/company-system/remaining-corporate-rules-research.md`
//! 与 `docs/trading-rules.md`）：
//! - 深市《证券发行人业务指南》（深业〔2025〕68号）2.4.3（一）—（五）：R-3 前提交申请、
//!   R 日日终向股东派发配股权证、缴款期自起始日至 L 日经交易系统报盘认购、
//!   L+2 扣除登记费后将认购资金及利息划至主承销商结算备付金账户、再划发行人募集资金账户。
//! - 2.4.4（一）：发行失败时 L+2 将认购资金及利息退回认购股东委托的结算参与机构。
//! - 2.4.4（二）：不足 1 份的零碎配股权证按数量降序、同数量系统随机、依序各登记 1 份。
//! - 206号令第 53 条：配股数量不超过配售前股本 50%、采用代销；代销期满认购低于拟配售
//!   量 70% 为发行失败。
//! - 沪市《证券发行人业务指南》（沪业字〔2024〕6号）第 2.4 节只确认注册与登记要求，
//!   无权证派发／缴款期／L+2 划款退款条文——**沪市操作时点登记为证据缺口**，本模块
//!   按「深市有据口径实现 + 沪市登记简化/待证」处理，两市共用同一日程框架。
//! - 沪市指南第 2.4 节明文：上市公司回购专用证券账户股份不享有配售权——分配显式排除
//!   `HolderId::IssuerTreasury`。
//!
//! 本模块只做纯事实与算法：方案、权证分配、认购与结算的状态机；真实现金划扣、
//! 股份入账、税账与公告由 Session 侧接线完成（ADR-0035/0038/0039）。

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::share_registry::{HolderId, RegistrationSnapshot, ShareRegistryError};
use super::CompanyId;
use crate::account::StockCode;
use crate::calendar::{CalendarError, CalendarExchange, CivilDate, TradingCalendar};
use crate::money::Money;

/// 比例单位：百万分之一股，与 `company::stock_distribution` 一致。
const RATIO_DENOMINATOR: u64 = 1_000_000;

/// NPC 认购策略选择器（2026-10-07 产品决策）：默认策略为足额认购；
/// 其他策略显式「未实现」，受理时显式拒绝，不静默降级。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RightsSubscriptionStrategy {
    /// 默认策略：NPC 以本人真实现金足额认购；不足部分放弃并如实记录弃配。
    FullByDefault,
    /// 按策略决定认购（属 P 批偏好域）：engine 侧未实现，受理时显式拒绝。
    StrategyBased,
}

/// 定向增发的对象去向（ADR-0039 第 5 条：三类去向都要能表达）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DirectedPlacementTarget {
    /// 定向到具名持有人（持仓机构股东／玩家承购）。`lock_until` 为可选锁定期
    /// （206号令第 59 条定向锁定期由方案决定；None = 不设锁）。
    NamedHolder {
        holder: HolderId,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        shares: u64,
        lock_until: Option<CivilDate>,
    },
    /// 公开配售：缴款期内全市场账户可申购，仅保留额度（申购记录在认购窗口产生）。
    OpenPublicSubscription {
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        shares: u64,
    },
}

/// 配股／增发模式。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RightsOfferingMode {
    /// 配股：面向全体股东，按登记日快照每持有 1 股配 `shares_per_existing_share_micros`
    /// 百万股（每 10 股配 N 股 = `N × 100_000`）。206号令第 53 条：拟配售数量不得超过
    /// 配售前股本的 50%。
    RightsToAllShareholders {
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        shares_per_existing_share_micros: u64,
    },
    /// 定向增发：公司侧显式指定对象清单（意向的自动产生属 P 批偏好域）。
    DirectedPlacement { targets: Vec<DirectedPlacementTarget> },
}

/// 配股／增发全链路方案：发行人、市场、法定日期与价格。
///
/// 日程框架（深市指南有据口径，沪市登记简化/待证）：
/// R（登记日）→ 缴款期 [R+1, L] → 除权 L+1（实践口径，登记待证）→ L+2 划款/退款
/// 与新股登记入账（深市 2.4.3（六）：上市日前一交易日日终标识为上市股份；本游戏
/// 取 L+2 与划款同日）。缴款期天数由方案参数化（指南未固定默认天数）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RightsOfferingEventPlan {
    pub event_id: String,
    pub approval_reference: String,
    pub issuer: CompanyId,
    pub stock: StockCode,
    pub exchange: CalendarExchange,
    pub approved_on: CivilDate,
    pub announced_on: CivilDate,
    /// R：股权登记日（日终派发配股权证）。
    pub registered_on: CivilDate,
    /// 缴款起始日（= R 的次一交易日）。
    pub payment_start_on: CivilDate,
    /// L：缴款截止日。
    pub payment_deadline_on: CivilDate,
    /// 除权日（= L 的次一交易日）。
    pub ex_rights_on: CivilDate,
    /// 划款／退款日（= L+2）。
    pub settlement_on: CivilDate,
    /// 新股登记入账日（= 划款日；取得日即税账取得日）。
    pub listing_on: CivilDate,
    pub price_per_share: Money,
    pub mode: RightsOfferingMode,
    pub npc_subscription_strategy: RightsSubscriptionStrategy,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum RightsOfferingError {
    #[error("rights offering: {detail}")]
    InvalidPlan { detail: String },
    #[error("rights offering: integer arithmetic overflow")]
    ArithmeticOverflow,
    #[error(
        "rights offering: NPC subscription strategy {strategy:?} is not implemented; only FullByDefault is available"
    )]
    SubscriptionStrategyUnimplemented {
        strategy: RightsSubscriptionStrategy,
    },
    #[error("rights offering: registration date {date} is not a {exchange:?} trading day")]
    InvalidTradingDate {
        date: CivilDate,
        exchange: CalendarExchange,
    },
    #[error(
        "rights offering: ex-rights date {actual} is not the next {exchange:?} trading day {expected} after the payment deadline"
    )]
    InvalidExRightsDate {
        expected: CivilDate,
        actual: CivilDate,
        exchange: CalendarExchange,
    },
    #[error(
        "rights offering: settlement date {actual} is not the next {exchange:?} trading day {expected} after the ex-rights date"
    )]
    InvalidSettlementDate {
        expected: CivilDate,
        actual: CivilDate,
        exchange: CalendarExchange,
    },
    #[error("rights offering: trading calendar lookup failed: {0}")]
    CalendarLookup(String),
    #[error("rights offering: registration snapshot does not match plan: {detail}")]
    SnapshotMismatch { detail: String },
    #[error("rights offering: entitlement total {approved} is outside feasible interval {minimum}..={maximum}")]
    EntitlementTotalInfeasible {
        approved: u64,
        minimum: u64,
        maximum: u64,
    },
    #[error("rights offering: share registry rejected the fact: {0}")]
    ShareRegistry(#[from] ShareRegistryError),
}

fn invalid(detail: &str) -> RightsOfferingError {
    RightsOfferingError::InvalidPlan {
        detail: detail.to_owned(),
    }
}

impl RightsOfferingEventPlan {
    /// 结构校验：身份、正价格、有序日期与 50% 配股上限（206号令第 53 条）。
    pub fn validate(&self) -> Result<(), RightsOfferingError> {
        if self.event_id.trim().is_empty()
            || self.approval_reference.trim().is_empty()
            || self.issuer.0.trim().is_empty()
            || self.stock.0.trim().is_empty()
        {
            return Err(invalid("identities and approval reference are required"));
        }
        if self.price_per_share <= Money::ZERO {
            return Err(invalid("subscription price must be positive"));
        }
        if !(self.approved_on <= self.announced_on
            && self.announced_on <= self.registered_on
            && self.registered_on < self.payment_start_on
            && self.payment_start_on <= self.payment_deadline_on
            && self.payment_deadline_on < self.ex_rights_on
            && self.ex_rights_on < self.settlement_on
            && self.settlement_on == self.listing_on)
        {
            return Err(invalid(
                "dates must satisfy approved <= announced <= R < start <= L < ex < settlement == listing",
            ));
        }
        if let RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros,
        } = &self.mode
        {
            if *shares_per_existing_share_micros == 0 {
                return Err(invalid("rights ratio must be positive"));
            }
            if *shares_per_existing_share_micros > RATIO_DENOMINATOR / 2 {
                return Err(invalid(
                    "rights ratio must not exceed 50% of pre-offering shares (CSRC Order 206 Article 53)",
                ));
            }
        }
        if let RightsOfferingMode::DirectedPlacement { targets } = &self.mode {
            if targets.is_empty() {
                return Err(invalid("directed placement requires an explicit target list"));
            }
            let mut seen = std::collections::BTreeSet::new();
            let mut total = 0_u64;
            for target in targets {
                match target {
                    DirectedPlacementTarget::NamedHolder {
                        holder,
                        shares,
                        lock_until,
                    } => {
                        if !seen.insert(holder.clone()) {
                            return Err(invalid(
                                "directed placement target holder is duplicated",
                            ));
                        }
                        if *shares == 0 {
                            return Err(invalid("directed placement target shares must be positive"));
                        }
                        if let Some(lock_until) = lock_until {
                            if *lock_until <= self.listing_on {
                                return Err(invalid(
                                    "directed placement lock must end after the listing date",
                                ));
                            }
                        }
                        total = total
                            .checked_add(*shares)
                            .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                    }
                    DirectedPlacementTarget::OpenPublicSubscription { shares } => {
                        if *shares == 0 {
                            return Err(invalid("open public subscription headroom must be positive"));
                        }
                        total = total
                            .checked_add(*shares)
                            .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                    }
                }
            }
        }
        match &self.npc_subscription_strategy {
            RightsSubscriptionStrategy::FullByDefault => {}
            strategy @ RightsSubscriptionStrategy::StrategyBased => {
                return Err(RightsOfferingError::SubscriptionStrategyUnimplemented {
                    strategy: strategy.clone(),
                });
            }
        }
        Ok(())
    }

    /// 使用调用方提供的权威日历校验日程：R、缴款期、L 均为该交易所交易日，
    /// 缴款起始日为 R 次一交易日、除权日为 L 次一交易日、划款/入账日为除权日次一
    /// 交易日（L+2）。不默认补齐日历。
    pub fn validate_calendar(
        &self,
        calendar: &TradingCalendar,
    ) -> Result<(), RightsOfferingError> {
        let check_trading = |date: CivilDate| -> Result<(), RightsOfferingError> {
            let trading = calendar
                .is_trading_day(self.exchange, date)
                .map_err(|error| RightsOfferingError::CalendarLookup(error.to_string()))?;
            if trading {
                Ok(())
            } else {
                Err(RightsOfferingError::InvalidTradingDate {
                    date,
                    exchange: self.exchange,
                })
            }
        };
        check_trading(self.registered_on)?;
        check_trading(self.payment_start_on)?;
        check_trading(self.payment_deadline_on)?;
        check_trading(self.ex_rights_on)?;
        check_trading(self.settlement_on)?;
        let next_after_registration = calendar
            .next_trading_day(self.exchange, self.registered_on)
            .map_err(|error| RightsOfferingError::CalendarLookup(error.to_string()))?;
        if self.payment_start_on != next_after_registration {
            return Err(RightsOfferingError::InvalidExRightsDate {
                expected: next_after_registration,
                actual: self.payment_start_on,
                exchange: self.exchange,
            });
        }
        let next_after_deadline = calendar
            .next_trading_day(self.exchange, self.payment_deadline_on)
            .map_err(|error| RightsOfferingError::CalendarLookup(error.to_string()))?;
        if self.ex_rights_on != next_after_deadline {
            return Err(RightsOfferingError::InvalidExRightsDate {
                expected: next_after_deadline,
                actual: self.ex_rights_on,
                exchange: self.exchange,
            });
        }
        let next_after_ex = calendar
            .next_trading_day(self.exchange, self.ex_rights_on)
            .map_err(|error| RightsOfferingError::CalendarLookup(error.to_string()))?;
        if self.settlement_on != next_after_ex {
            return Err(RightsOfferingError::InvalidSettlementDate {
                expected: next_after_ex,
                actual: self.settlement_on,
                exchange: self.exchange,
            });
        }
        Ok(())
    }

    /// 按登记日与缴款期交易日数推导日程（缴款期天数参数化：指南未固定默认天数，
    /// 默认值由调用方显式给出）。缴费期第 1 日 = R 次一交易日，L 为第
    /// `payment_days` 个交易日，除权 L+1，划款/入账 L+2。
    pub fn derive_schedule(
        &mut self,
        calendar: &TradingCalendar,
        exchange: CalendarExchange,
        payment_days: u32,
    ) -> Result<(), RightsOfferingError> {
        if payment_days == 0 {
            return Err(invalid("payment window must cover at least one trading day"));
        }
        self.exchange = exchange;
        let mut cursor = calendar
            .next_trading_day(self.exchange, self.registered_on)
            .map_err(|error| RightsOfferingError::CalendarLookup(error.to_string()))?;
        self.payment_start_on = cursor;
        for _ in 1..payment_days {
            cursor = calendar
                .next_trading_day(self.exchange, cursor)
                .map_err(|error| RightsOfferingError::CalendarLookup(error.to_string()))?;
        }
        self.payment_deadline_on = cursor;
        self.ex_rights_on = calendar
            .next_trading_day(self.exchange, self.payment_deadline_on)
            .map_err(|error| RightsOfferingError::CalendarLookup(error.to_string()))?;
        self.settlement_on = calendar
            .next_trading_day(self.exchange, self.ex_rights_on)
            .map_err(|error| RightsOfferingError::CalendarLookup(error.to_string()))?;
        self.listing_on = self.settlement_on;
        Ok(())
    }

    /// 缴款期窗口（含两端）。
    pub fn payment_window_contains(&self, day: CivilDate) -> bool {
        self.payment_start_on <= day && day <= self.payment_deadline_on
    }

    /// 面向全体股东模式下的拟配售总数（由登记快照与比例推导前先给出比例）。
    pub fn rights_ratio_micros(&self) -> Option<u64> {
        match &self.mode {
            RightsOfferingMode::RightsToAllShareholders {
                shares_per_existing_share_micros,
            } => Some(*shares_per_existing_share_micros),
            RightsOfferingMode::DirectedPlacement { .. } => None,
        }
    }
}

/// 单个持有人的整数配股权证（对应可认购股数）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RightsEntitlement {
    pub holder: HolderId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub rights_shares: u64,
    /// 定向锁定期截止日（None = 无锁；配股面向全体股东模式恒为 None）。
    pub lock_until: Option<CivilDate>,
}

/// R 日权证派发回执：冻结登记快照上的整数权证分配。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RightsEntitlementReceipt {
    pub event_id: String,
    pub approval_reference: String,
    pub registration_event_id: String,
    pub stock: StockCode,
    pub issuer: CompanyId,
    pub registered_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issued_shares_before: u64,
    /// 回购专户（IssuerTreasury）持有的排除股数：不享有配售权（沪市指南 2.4）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issuer_treasury_shares_excluded: u64,
    pub price_per_share: Money,
    pub mode: RightsOfferingMode,
    /// 拟配售总股数（含公开配售额度；70% 失败判定基数，206号令第 53 条）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub planned_total_rights_shares: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub tie_break_seed: u64,
    pub entitlements: Vec<RightsEntitlement>,
    /// 公开配售剩余额度（只有定向模式可为正）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub open_subscription_shares: u64,
}

/// 按登记快照分配配股权证。
///
/// - 面向全体股东：每持有 1 股配 `ratio_micros` 百万股；整数部分为权证，
///   碎权证按数量降序、同额 seed 洗牌依序各登记 1 份（深市 2.4.4（二）；
///   官方只说"系统随机"，seed 由事件身份派生并显式记录为游戏输入）。
///   `IssuerTreasury`（回购专户）排除。
/// - 定向：具名对象直接成权（可带锁定期），公开配售只保留额度。
pub fn allocate_rights_entitlements(
    snapshot: &RegistrationSnapshot,
    plan: &RightsOfferingEventPlan,
    tie_break_seed: u64,
) -> Result<RightsEntitlementReceipt, RightsOfferingError> {
    snapshot
        .validate()
        .map_err(|error| invalid(&format!("invalid registration snapshot: {error}")))?;
    plan.validate()?;
    if snapshot.event_id() != plan.event_id
        || snapshot.stock() != &plan.stock
        || snapshot.issuer() != &plan.issuer
        || snapshot.registered_on() != plan.registered_on
    {
        return Err(RightsOfferingError::SnapshotMismatch {
            detail: "snapshot plan identity, issuer, security or registration date differs".into(),
        });
    }
    let treasury_shares = snapshot
        .holdings()
        .iter()
        .filter(|holding| holding.holder == HolderId::IssuerTreasury)
        .flat_map(|holding| holding.lots.iter())
        .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
        .ok_or(RightsOfferingError::ArithmeticOverflow)?;

    match &plan.mode {
        RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros,
        } => {
            let ratio = *shares_per_existing_share_micros;
            let mut holders: Vec<(HolderId, u64, u64)> = Vec::new();
            let mut eligible = 0_u64;
            let mut base_total = 0_u64;
            for holding in snapshot.holdings() {
                if holding.holder == HolderId::IssuerTreasury {
                    continue;
                }
                let original = holding
                    .lots
                    .iter()
                    .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
                    .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                if original == 0 {
                    continue;
                }
                eligible = eligible
                    .checked_add(original)
                    .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                let product = u128::from(original)
                    .checked_mul(u128::from(ratio))
                    .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                let whole = u64::try_from(product / u128::from(RATIO_DENOMINATOR))
                    .map_err(|_| RightsOfferingError::ArithmeticOverflow)?;
                base_total = base_total
                    .checked_add(whole)
                    .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                holders.push((
                    holding.holder.clone(),
                    whole,
                    u64::try_from(product % u128::from(RATIO_DENOMINATOR))
                        .map_err(|_| RightsOfferingError::ArithmeticOverflow)?,
                ));
            }
            let aggregate_product = u128::from(eligible)
                .checked_mul(u128::from(ratio))
                .ok_or(RightsOfferingError::ArithmeticOverflow)?;
            let aggregate_floor = u64::try_from(aggregate_product / u128::from(RATIO_DENOMINATOR))
                .map_err(|_| RightsOfferingError::ArithmeticOverflow)?;
            let aggregate_ceil = aggregate_floor
                .checked_add(u64::from(
                    aggregate_product % u128::from(RATIO_DENOMINATOR) > 0,
                ))
                .ok_or(RightsOfferingError::ArithmeticOverflow)?;
            // 拟配售总数取聚合上限（碎权证补整使每份权证都是整数份）；
            // 必须落在逐户可行区间 [max(整权和, 聚合下限), min(整权和+碎权证户数, 聚合上限)]。
            let planned_total = aggregate_ceil;
            let minimum = base_total.max(aggregate_floor);
            let maximum = base_total
                .checked_add(
                    u64::try_from(holders.iter().filter(|(_, _, rem)| *rem > 0).count())
                        .map_err(|_| RightsOfferingError::ArithmeticOverflow)?,
                )
                .ok_or(RightsOfferingError::ArithmeticOverflow)?
                .min(aggregate_ceil);
            if minimum > maximum || planned_total < minimum || planned_total > maximum {
                return Err(RightsOfferingError::EntitlementTotalInfeasible {
                    approved: planned_total,
                    minimum,
                    maximum,
                });
            }
            let extras = planned_total
                .checked_sub(base_total)
                .ok_or(RightsOfferingError::ArithmeticOverflow)?;
            let mut candidates: Vec<usize> = holders
                .iter()
                .enumerate()
                .filter_map(|(index, (_, _, remainder))| (*remainder > 0).then_some(index))
                .collect();
            candidates.sort_by(|left, right| holders[*right].2.cmp(&holders[*left].2));
            shuffle_equal_remainders(&mut candidates, &holders, tie_break_seed);
            for index in candidates
                .into_iter()
                .take(usize::try_from(extras).map_err(|_| RightsOfferingError::ArithmeticOverflow)?)
            {
                holders[index].1 = holders[index]
                    .1
                    .checked_add(1)
                    .ok_or(RightsOfferingError::ArithmeticOverflow)?;
            }
            let entitlements = holders
                .into_iter()
                .filter(|(_, whole, _)| *whole > 0)
                .map(|(holder, whole, _)| RightsEntitlement {
                    holder,
                    rights_shares: whole,
                    lock_until: None,
                })
                .collect();
            Ok(RightsEntitlementReceipt {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                registration_event_id: snapshot.event_id().to_owned(),
                stock: snapshot.stock().clone(),
                issuer: snapshot.issuer().clone(),
                registered_on: snapshot.registered_on(),
                issued_shares_before: snapshot.issued_shares(),
                issuer_treasury_shares_excluded: treasury_shares,
                price_per_share: plan.price_per_share,
                mode: plan.mode.clone(),
                planned_total_rights_shares: planned_total,
                tie_break_seed,
                entitlements,
                open_subscription_shares: 0,
            })
        }
        RightsOfferingMode::DirectedPlacement { targets } => {
            let mut entitlements = Vec::with_capacity(targets.len());
            let mut open = 0_u64;
            let mut planned_total = 0_u64;
            for target in targets {
                match target {
                    DirectedPlacementTarget::NamedHolder {
                        holder,
                        shares,
                        lock_until,
                    } => {
                        planned_total = planned_total
                            .checked_add(*shares)
                            .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                        entitlements.push(RightsEntitlement {
                            holder: holder.clone(),
                            rights_shares: *shares,
                            lock_until: *lock_until,
                        });
                    }
                    DirectedPlacementTarget::OpenPublicSubscription { shares } => {
                        planned_total = planned_total
                            .checked_add(*shares)
                            .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                        open = open
                            .checked_add(*shares)
                            .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                    }
                }
            }
            entitlements.sort_by(|left, right| left.holder.cmp(&right.holder));
            Ok(RightsEntitlementReceipt {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                registration_event_id: snapshot.event_id().to_owned(),
                stock: snapshot.stock().clone(),
                issuer: snapshot.issuer().clone(),
                registered_on: snapshot.registered_on(),
                issued_shares_before: snapshot.issued_shares(),
                issuer_treasury_shares_excluded: treasury_shares,
                price_per_share: plan.price_per_share,
                mode: plan.mode.clone(),
                planned_total_rights_shares: planned_total,
                tie_break_seed,
                entitlements,
                open_subscription_shares: open,
            })
        }
    }
}

fn shuffle_equal_remainders(
    candidates: &mut [usize],
    holders: &[(HolderId, u64, u64)],
    seed: u64,
) {
    let mut random = SeededRandom(seed);
    let mut start = 0;
    while start < candidates.len() {
        let remainder = holders[candidates[start]].2;
        let mut end = start + 1;
        while end < candidates.len() && holders[candidates[end]].2 == remainder {
            end += 1;
        }
        for index in (start + 1..end).rev() {
            let swap = start + random.index(index - start + 1);
            candidates.swap(index, swap);
        }
        start = end;
    }
}

struct SeededRandom(u64);

impl SeededRandom {
    fn index(&mut self, upper_exclusive: usize) -> usize {
        let upper = u64::try_from(upper_exclusive)
            .expect("candidate group length fits the u64 index domain");
        let rejection_floor = upper.wrapping_neg() % upper;
        loop {
            let value = self.next_u64();
            if value >= rejection_floor {
                return usize::try_from(value % upper)
                    .expect("bounded result fits the candidate group length");
            }
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

/// 由事件身份派生可复现的碎权证同额洗牌 seed（FNV-1a + SplitMix64 雪崩）。
/// 官方只要求"系统随机"，该 seed 是显式游戏输入，随回执记录。
pub fn rights_tie_break_seed(event_id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in event_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut value = hash;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// 配股认购申请来源。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SubscriptionOrigin {
    /// 玩家／宿主显式认购。
    Explicit,
    /// NPC 默认足额认购策略（FullByDefault）自动产生。
    NpcFullByDefault,
}

/// 缴款期内的一条认购申请（含实际划扣结果由 Session 侧回填）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RightsSubscriptionRecord {
    pub holder: HolderId,
    /// 申请认购股数（≤权利股数或公开额度余量）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub requested_shares: u64,
    pub price_per_share: Money,
    pub submitted_on: CivilDate,
    pub origin: SubscriptionOrigin,
    /// 缴款日终实际划扣股数（现金不足时 < 申请数；弃配部分显式记录）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub paid_shares: u64,
    /// 实际划扣金额（分）。
    pub paid_amount: Money,
    /// 现金不足放弃的股数（如实记录，不静默截断）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub waived_shares: u64,
}

/// 结算时单个持有人的最终事实（成功=认购股份；失败=全额退款）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolderRightsSettlement {
    pub holder: HolderId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub paid_shares: u64,
    pub paid_amount: Money,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub waived_shares: u64,
    /// 失败路径退回的认购款（成功恒为零；无利息——显式不支持并登记）。
    pub refunded_amount: Money,
}

/// L+2 结算回执：成功=划款给发行人（账面募集资金）＋新股入账；失败=退款。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RightsSettlementReceipt {
    pub event_id: String,
    pub settlement_on: CivilDate,
    /// 配股（面向全体股东）认购不足拟配售量 70% 时为发行失败（206号令第 53 条）；
    /// 定向增发不适用 70% 代销门槛，恒为 false。
    pub failed: bool,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub total_paid_shares: u64,
    pub total_paid_amount: Money,
    pub refunded_total: Money,
    pub holders: Vec<HolderRightsSettlement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RightsOfferingStatus {
    Approved,
    Announced,
    Entitled,
    Closed,
    Settled,
}

/// 配股／增发执行状态机：显式计划 → 公告 → R 日权证 → 缴款期认购 → L 关窗
/// → L+2 结算（成功划款／失败退款）＋新股入账。
///
/// 状态机只记录事实；真实现金划扣、股份入账、税账与公告由 Session 以候选事务执行。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RightsOfferingBookState")]
#[serde(deny_unknown_fields)]
pub struct RightsOfferingBook {
    plan: RightsOfferingEventPlan,
    status: RightsOfferingStatus,
    #[serde(deserialize_with = "required_nullable_registration")]
    registration: Option<RegistrationSnapshot>,
    entitlement: Option<RightsEntitlementReceipt>,
    subscriptions: Vec<RightsSubscriptionRecord>,
    closed_on: Option<CivilDate>,
    settlement: Option<RightsSettlementReceipt>,
    credited_on: Option<CivilDate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RightsOfferingBookState {
    plan: RightsOfferingEventPlan,
    status: RightsOfferingStatus,
    #[serde(deserialize_with = "required_nullable_registration")]
    registration: Option<RegistrationSnapshot>,
    entitlement: Option<RightsEntitlementReceipt>,
    subscriptions: Vec<RightsSubscriptionRecord>,
    closed_on: Option<CivilDate>,
    settlement: Option<RightsSettlementReceipt>,
    credited_on: Option<CivilDate>,
}

impl TryFrom<RightsOfferingBookState> for RightsOfferingBook {
    type Error = RightsOfferingError;

    fn try_from(state: RightsOfferingBookState) -> Result<Self, Self::Error> {
        let book = Self {
            plan: state.plan,
            status: state.status,
            registration: state.registration,
            entitlement: state.entitlement,
            subscriptions: state.subscriptions,
            closed_on: state.closed_on,
            settlement: state.settlement,
            credited_on: state.credited_on,
        };
        book.validate()?;
        Ok(book)
    }
}

impl RightsOfferingBook {
    pub fn new(plan: RightsOfferingEventPlan) -> Result<Self, RightsOfferingError> {
        plan.validate()?;
        Ok(Self {
            plan,
            status: RightsOfferingStatus::Approved,
            registration: None,
            entitlement: None,
            subscriptions: Vec::new(),
            closed_on: None,
            settlement: None,
            credited_on: None,
        })
    }

    pub fn plan(&self) -> &RightsOfferingEventPlan {
        &self.plan
    }
    pub fn status(&self) -> &RightsOfferingStatus {
        &self.status
    }
    pub fn registration(&self) -> Option<&RegistrationSnapshot> {
        self.registration.as_ref()
    }
    pub fn entitlement(&self) -> Option<&RightsEntitlementReceipt> {
        self.entitlement.as_ref()
    }
    pub fn subscriptions(&self) -> &[RightsSubscriptionRecord] {
        &self.subscriptions
    }
    pub fn settlement(&self) -> Option<&RightsSettlementReceipt> {
        self.settlement.as_ref()
    }
    pub fn credited_on(&self) -> Option<CivilDate> {
        self.credited_on
    }

    pub fn announce(&mut self, on: CivilDate) -> Result<(), RightsOfferingError> {
        if self.status != RightsOfferingStatus::Approved {
            return if on == self.plan.announced_on {
                Ok(())
            } else {
                Err(RightsOfferingError::SnapshotMismatch {
                    detail: "announce was already recorded on a different date".into(),
                })
            };
        }
        if on != self.plan.announced_on {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "announce date differs from the approved plan".into(),
            });
        }
        self.status = RightsOfferingStatus::Announced;
        Ok(())
    }

    /// R 日冻结登记快照并派发整数配股权证；重复提交同一快照幂等。
    pub fn entitle(
        &mut self,
        snapshot: RegistrationSnapshot,
        calendar: &TradingCalendar,
    ) -> Result<&RightsEntitlementReceipt, RightsOfferingError> {
        self.plan.validate_calendar(calendar)?;
        if let Some(existing) = &self.registration {
            if existing == &snapshot {
                return self.entitlement.as_ref().ok_or_else(|| {
                    RightsOfferingError::SnapshotMismatch {
                        detail: "entitled book is missing its entitlement receipt".into(),
                    }
                });
            }
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "registration was already frozen for this plan".into(),
            });
        }
        if self.status != RightsOfferingStatus::Announced {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "entitlement requires the Announced status".into(),
            });
        }
        let receipt = allocate_rights_entitlements(
            &snapshot,
            &self.plan,
            rights_tie_break_seed(&self.plan.event_id),
        )?;
        self.registration = Some(snapshot);
        self.entitlement = Some(receipt);
        self.status = RightsOfferingStatus::Entitled;
        self.entitlement
            .as_ref()
            .ok_or_else(|| RightsOfferingError::SnapshotMismatch {
                detail: "entitlement receipt insertion failed".into(),
            })
    }

    /// 记录一条认购（申请与缴款日终划扣结果由 Session 一并提交）。
    ///
    /// 校验：窗口日（含两端）、价格一致、paid + waived == requested、
    /// paid × price == paid_amount、不重复提交同一持有人、权利额度不超卖
    /// （配股按权利上限；公开配售按额度上限，先到先得——游戏简化，已登记）。
    pub fn record_subscription(
        &mut self,
        record: RightsSubscriptionRecord,
    ) -> Result<(), RightsOfferingError> {
        if self.status != RightsOfferingStatus::Entitled {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "subscriptions are only accepted inside the payment window after entitlement".into(),
            });
        }
        if !self.plan.payment_window_contains(record.submitted_on) {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "subscription was submitted outside the payment window".into(),
            });
        }
        if record.requested_shares == 0 || record.paid_shares > record.requested_shares {
            return Err(invalid("subscription quantity facts are inconsistent"));
        }
        if record
            .paid_shares
            .checked_add(record.waived_shares)
            != Some(record.requested_shares)
        {
            return Err(invalid("paid plus waived shares must equal the requested shares"));
        }
        let expected_amount = Money::from_cents(
            i64::try_from(
                u128::from(record.paid_shares)
                    .checked_mul(u128::from(self.plan.price_per_share.cents().unsigned_abs()))
                    .ok_or(RightsOfferingError::ArithmeticOverflow)?,
            )
            .map_err(|_| RightsOfferingError::ArithmeticOverflow)?,
        );
        if record.price_per_share != self.plan.price_per_share
            || (record.paid_shares > 0 && record.paid_amount != expected_amount)
            || (record.paid_shares == 0 && record.paid_amount != Money::ZERO)
        {
            return Err(invalid("subscription price or paid amount does not match the plan"));
        }
        if self
            .subscriptions
            .iter()
            .any(|existing| existing.holder == record.holder)
        {
            return Err(invalid("holder already subscribed; one net subscription per holder"));
        }
        let entitlement = self.entitlement.as_ref().ok_or_else(|| {
            RightsOfferingError::SnapshotMismatch {
                detail: "subscriptions require a frozen entitlement receipt".into(),
            }
        })?;
        if let Some(entitlement) = entitlement
            .entitlements
            .iter()
            .find(|entry| entry.holder == record.holder)
        {
            if record.requested_shares > entitlement.rights_shares {
                return Err(invalid("subscription exceeds the holder rights"));
            }
        } else if entitlement.open_subscription_shares > 0 {
            let open_used: u64 = self
                .subscriptions
                .iter()
                .filter(|existing| {
                    !entitlement
                        .entitlements
                        .iter()
                        .any(|entry| entry.holder == existing.holder)
                })
                .map(|existing| existing.requested_shares)
                .try_fold(0_u64, |sum, shares| sum.checked_add(shares))
                .ok_or(RightsOfferingError::ArithmeticOverflow)?;
            if open_used
                .checked_add(record.requested_shares)
                .ok_or(RightsOfferingError::ArithmeticOverflow)?
                > entitlement.open_subscription_shares
            {
                return Err(invalid("subscription exceeds the open public headroom"));
            }
        } else {
            return Err(invalid("holder has no rights and the plan has no open headroom"));
        }
        self.subscriptions.push(record);
        Ok(())
    }

    /// L 日终关窗：此后不再受理认购。重复关窗幂等。
    pub fn close_payment_window(&mut self, day: CivilDate) -> Result<(), RightsOfferingError> {
        match self.status.clone() {
            RightsOfferingStatus::Closed => {
                if self.closed_on == Some(day) {
                    Ok(())
                } else {
                    Err(RightsOfferingError::SnapshotMismatch {
                        detail: "payment window was closed on a different date".into(),
                    })
                }
            }
            RightsOfferingStatus::Entitled => {
                if day != self.plan.payment_deadline_on {
                    return Err(RightsOfferingError::SnapshotMismatch {
                        detail: "payment window must close on the deadline day".into(),
                    });
                }
                self.closed_on = Some(day);
                self.status = RightsOfferingStatus::Closed;
                Ok(())
            }
            status => Err(RightsOfferingError::SnapshotMismatch {
                detail: format!("close_payment_window is invalid in status {status:?}"),
            }),
        }
    }

    /// 配股（面向全体股东）失败判定：认购不足拟配售量 70%（206号令第 53 条）。
    /// 定向增发不适用代销 70% 门槛，恒为成功（未缴款部分不发行，不退款）。
    pub fn determine_failure(&self) -> Result<bool, RightsOfferingError> {
        let entitlement = self.entitlement.as_ref().ok_or_else(|| {
            RightsOfferingError::SnapshotMismatch {
                detail: "failure determination requires a frozen entitlement".into(),
            }
        })?;
        if !matches!(
            self.plan.mode,
            RightsOfferingMode::RightsToAllShareholders { .. }
        ) {
            return Ok(false);
        }
        let paid: u64 = self
            .subscriptions
            .iter()
            .map(|record| record.paid_shares)
            .try_fold(0_u64, |sum, shares| sum.checked_add(shares))
            .ok_or(RightsOfferingError::ArithmeticOverflow)?;
        let planned = entitlement.planned_total_rights_shares;
        // paid / planned < 7 / 10 ⇔ paid × 10 < planned × 7（精确整数比较）。
        Ok(paid
            .checked_mul(10)
            .ok_or(RightsOfferingError::ArithmeticOverflow)?
            < planned
                .checked_mul(7)
                .ok_or(RightsOfferingError::ArithmeticOverflow)?)
    }

    /// L+2 结算：记录成功划款或失败退款事实（执行由 Session 完成）。
    pub fn settle(
        &mut self,
        day: CivilDate,
        receipt: RightsSettlementReceipt,
    ) -> Result<(), RightsOfferingError> {
        if self.status != RightsOfferingStatus::Closed {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "settlement requires the Closed status".into(),
            });
        }
        if day != self.plan.settlement_on {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "settlement must happen on the settlement day".into(),
            });
        }
        if receipt.event_id != self.plan.event_id || receipt.settlement_on != day {
            return Err(invalid("settlement receipt identity does not match the plan"));
        }
        let failed = self.determine_failure()?;
        if receipt.failed != failed {
            return Err(invalid(
                "settlement failure flag disagrees with the 70% determination",
            ));
        }
        let mut paid_shares = 0_u64;
        let mut paid_amount = Money::ZERO;
        let mut refunded = Money::ZERO;
        let mut seen = std::collections::BTreeSet::new();
        for holder in &receipt.holders {
            if !seen.insert(holder.holder.clone()) {
                return Err(invalid("settlement contains a duplicated holder"));
            }
            let subscription = self
                .subscriptions
                .iter()
                .find(|record| record.holder == holder.holder)
                .ok_or_else(|| {
                    invalid("settlement holder has no subscription record")
                })?;
            if holder.paid_shares != subscription.paid_shares
                || holder.paid_amount != subscription.paid_amount
                || holder.waived_shares != subscription.waived_shares
            {
                return Err(invalid("settlement differs from the recorded subscription"));
            }
            if (failed && holder.refunded_amount != holder.paid_amount)
                || (!failed && holder.refunded_amount != Money::ZERO)
            {
                return Err(invalid("refund facts disagree with the failure determination"));
            }
            paid_shares = paid_shares
                .checked_add(holder.paid_shares)
                .ok_or(RightsOfferingError::ArithmeticOverflow)?;
            paid_amount = paid_amount
                .add(holder.paid_amount)
                .map_err(|_| RightsOfferingError::ArithmeticOverflow)?;
            refunded = refunded
                .add(holder.refunded_amount)
                .map_err(|_| RightsOfferingError::ArithmeticOverflow)?;
        }
        if paid_shares != receipt.total_paid_shares
            || paid_amount != receipt.total_paid_amount
            || refunded != receipt.refunded_total
            || receipt.holders.len() != self.subscriptions.len()
        {
            return Err(invalid("settlement aggregates do not match the holder rows"));
        }
        self.settlement = Some(receipt);
        self.status = RightsOfferingStatus::Settled;
        Ok(())
    }

    /// 新股入账完成（结算成功同日）后由 Session 记录；失败路径无新股。
    pub fn mark_credited(&mut self, on: CivilDate) -> Result<(), RightsOfferingError> {
        let Some(settlement) = &self.settlement else {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "credit requires a settlement receipt".into(),
            });
        };
        if settlement.failed {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "failed offerings issue no new shares".into(),
            });
        }
        if on != self.plan.listing_on {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "credit must happen on the listing day".into(),
            });
        }
        if self.credited_on.is_some() {
            return if self.credited_on == Some(on) {
                Ok(())
            } else {
                Err(RightsOfferingError::SnapshotMismatch {
                    detail: "credit was already recorded on a different date".into(),
                })
            };
        }
        self.credited_on = Some(on);
        Ok(())
    }

    /// 无新参数结构一致性校验（恢复路径也走这里）。
    pub fn validate(&self) -> Result<(), RightsOfferingError> {
        self.plan.validate()?;
        self.validate_subscriptions()?;
        let entitled = self.entitlement.is_some();
        if entitled
            != matches!(
                self.status,
                RightsOfferingStatus::Entitled
                    | RightsOfferingStatus::Closed
                    | RightsOfferingStatus::Settled
            )
        {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "status and frozen entitlement do not agree".into(),
            });
        }
        if entitled != self.registration.is_some() {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "entitlement and registration snapshot do not agree".into(),
            });
        }
        if let Some(snapshot) = &self.registration {
            snapshot.validate()?;
            let expected = allocate_rights_entitlements(
                snapshot,
                &self.plan,
                rights_tie_break_seed(&self.plan.event_id),
            )?;
            if self.entitlement.as_ref() != Some(&expected) {
                return Err(RightsOfferingError::SnapshotMismatch {
                    detail: "restored entitlement receipt does not replay the frozen registration"
                        .into(),
                });
            }
        }
        match (&self.status, self.closed_on) {
            (RightsOfferingStatus::Closed, Some(closed_on))
            | (RightsOfferingStatus::Settled, Some(closed_on)) => {
                if entitled && closed_on != self.plan.payment_deadline_on {
                    return Err(RightsOfferingError::SnapshotMismatch {
                        detail: "closed date does not match the payment deadline".into(),
                    });
                }
            }
            (_, Some(_)) => {
                return Err(RightsOfferingError::SnapshotMismatch {
                    detail: "closed facts require the Closed status".into(),
                });
            }
            _ => {}
        }
        if let Some(settlement) = &self.settlement {
            if self.status != RightsOfferingStatus::Settled
                || settlement.settlement_on != self.plan.settlement_on
            {
                return Err(RightsOfferingError::SnapshotMismatch {
                    detail: "settlement facts do not match the status or date".into(),
                });
            }
        } else if self.status == RightsOfferingStatus::Settled {
            return Err(RightsOfferingError::SnapshotMismatch {
                detail: "Settled status requires a settlement receipt".into(),
            });
        }
        if let Some(credited_on) = self.credited_on {
            let failed = self
                .settlement
                .as_ref()
                .map(|settlement| settlement.failed)
                .unwrap_or(true);
            if failed || credited_on != self.plan.listing_on {
                return Err(RightsOfferingError::SnapshotMismatch {
                    detail: "credit facts require a successful settlement on the listing day"
                        .into(),
                });
            }
        }
        Ok(())
    }

    /// 使用调用方选择的权威日历校验恢复状态；不默认补齐日历。
    pub fn validate_with_calendar(
        &self,
        calendar: &TradingCalendar,
    ) -> Result<(), RightsOfferingError> {
        self.validate()?;
        self.plan.validate_calendar(calendar)
    }

    /// 已记录认购的内部一致性与额度复核（恢复防篡改面）。
    fn validate_subscriptions(&self) -> Result<(), RightsOfferingError> {
        let Some(entitlement) = &self.entitlement else {
            if !self.subscriptions.is_empty() {
                return Err(RightsOfferingError::SnapshotMismatch {
                    detail: "subscriptions require a frozen entitlement receipt".into(),
                });
            }
            return Ok(());
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut open_used = 0_u64;
        for record in &self.subscriptions {
            if !seen.insert(record.holder.clone()) {
                return Err(invalid("holder already subscribed; one net subscription per holder"));
            }
            if record.requested_shares == 0 || record.paid_shares > record.requested_shares {
                return Err(invalid("subscription quantity facts are inconsistent"));
            }
            if record
                .paid_shares
                .checked_add(record.waived_shares)
                != Some(record.requested_shares)
            {
                return Err(invalid("paid plus waived shares must equal the requested shares"));
            }
            let expected_amount = Money::from_cents(
                i64::try_from(
                    u128::from(record.paid_shares)
                        .checked_mul(u128::from(
                            self.plan.price_per_share.cents().unsigned_abs(),
                        ))
                        .ok_or(RightsOfferingError::ArithmeticOverflow)?,
                )
                .map_err(|_| RightsOfferingError::ArithmeticOverflow)?,
            );
            if record.price_per_share != self.plan.price_per_share
                || (record.paid_shares > 0 && record.paid_amount != expected_amount)
                || (record.paid_shares == 0 && record.paid_amount != Money::ZERO)
            {
                return Err(invalid("subscription price or paid amount does not match the plan"));
            }
            if !self.plan.payment_window_contains(record.submitted_on) {
                return Err(invalid("subscription was submitted outside the payment window"));
            }
            match entitlement
                .entitlements
                .iter()
                .find(|entry| entry.holder == record.holder)
            {
                Some(holder_rights) => {
                    if record.requested_shares > holder_rights.rights_shares {
                        return Err(invalid("subscription exceeds the holder rights"));
                    }
                }
                None => {
                    if entitlement.open_subscription_shares == 0 {
                        return Err(invalid(
                            "holder has no rights and the plan has no open headroom",
                        ));
                    }
                    open_used = open_used
                        .checked_add(record.requested_shares)
                        .ok_or(RightsOfferingError::ArithmeticOverflow)?;
                }
            }
        }
        if open_used > entitlement.open_subscription_shares {
            return Err(invalid("open public subscriptions exceed the headroom"));
        }
        Ok(())
    }
}

fn required_nullable_registration<'de, D>(
    deserializer: D,
) -> Result<Option<RegistrationSnapshot>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<RegistrationSnapshot>::deserialize(deserializer)
}

#[cfg(test)]
#[path = "rights_offering_tests.rs"]
mod tests;
