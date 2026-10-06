use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::share_registry::{
    HolderId, RegistrationSnapshot, ShareLot, ShareRegistryError, ShareRestriction,
};
use super::CompanyId;
use crate::accounting::AccountingAmount;
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::money::Money;
use crate::account::StockCode;

const RATIO_DENOMINATOR: u64 = 1_000_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StockDistributionKind {
    BonusShares,
    CapitalReserveConversion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDistributionPlan {
    pub event_id: String,
    pub approval_reference: String,
    pub kind: StockDistributionKind,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub shares_per_existing_share_micros: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub approved_total_new_shares: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolderDistribution {
    pub holder: HolderId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub original_shares: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub whole_shares: u64,
    pub fractional_numerator: u64,
    pub original_lots: Vec<ShareLot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SourceLotAttribution {
    SourceLotAttributionPending,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDistributionReceipt {
    pub event_id: String,
    pub approval_reference: String,
    pub registration_event_id: String,
    pub stock: StockCode,
    pub issuer: CompanyId,
    pub registered_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issued_shares_before: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issuer_treasury_shares_excluded: u64,
    pub kind: StockDistributionKind,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub shares_per_existing_share_micros: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub approved_total_new_shares: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub tie_break_seed: u64,
    pub source_lot_attribution: SourceLotAttribution,
    pub holders: Vec<HolderDistribution>,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum StockDistributionError {
    #[error("stock distribution: {detail}")]
    InvalidPlan { detail: String },
    #[error("stock distribution: approved total {approved} is outside feasible interval {minimum}..={maximum}")]
    ApprovedTotalInfeasible {
        approved: u64,
        minimum: u64,
        maximum: u64,
    },
    #[error("stock distribution: integer arithmetic overflow")]
    ArithmeticOverflow,
    #[error("stock distribution: operation {operation} is invalid in status {status:?}")]
    WrongStage {
        status: StockDistributionStatus,
        operation: &'static str,
    },
    #[error("stock distribution: registration snapshot does not match plan: {detail}")]
    SnapshotMismatch { detail: String },
    #[error(
        "stock distribution: holder {holder:?} mixes restriction sources; per-lot attribution stays pending"
    )]
    SourceLotAttributionConflict { holder: HolderId },
    #[error("stock distribution: trading calendar lookup failed: {detail}")]
    CalendarLookup { detail: String },
    #[error("stock distribution: registration date {date} is not a {exchange:?} trading day")]
    InvalidTradingDate {
        date: CivilDate,
        exchange: CalendarExchange,
    },
    #[error(
        "stock distribution: ex-rights/credit date {actual} is not the next {exchange:?} trading day {expected}"
    )]
    InvalidExRightsDate {
        expected: CivilDate,
        actual: CivilDate,
        exchange: CalendarExchange,
    },
    #[error("stock distribution: share registry rejected the fact: {0}")]
    ShareRegistry(#[from] ShareRegistryError),
}

pub fn allocate_stock_distribution(
    snapshot: &RegistrationSnapshot,
    plan: &StockDistributionPlan,
    tie_break_seed: u64,
) -> Result<StockDistributionReceipt, StockDistributionError> {
    snapshot
        .validate()
        .map_err(|error| StockDistributionError::InvalidPlan {
            detail: format!("invalid registration snapshot: {error}"),
        })?;
    if plan.event_id.trim().is_empty() || plan.approval_reference.trim().is_empty() {
        return Err(invalid(
            "distribution event identity and approval reference are required",
        ));
    }
    if plan.shares_per_existing_share_micros == 0 {
        return Err(invalid("share distribution ratio must be positive"));
    }

    let mut grouped = BTreeMap::<HolderId, (u64, Vec<ShareLot>)>::new();
    for holding in snapshot.holdings() {
        let entry = grouped.entry(holding.holder.clone()).or_default();
        for lot in &holding.lots {
            entry.0 = entry
                .0
                .checked_add(lot.qty)
                .ok_or(StockDistributionError::ArithmeticOverflow)?;
            entry.1.push(lot.clone());
        }
    }

    let treasury_shares = grouped
        .get(&HolderId::IssuerTreasury)
        .map(|(shares, _)| *shares)
        .unwrap_or(0);
    if matches!(&plan.kind, StockDistributionKind::CapitalReserveConversion)
        && treasury_shares > 0
        && snapshot.issuer_repurchase_account().is_none()
    {
        return Err(invalid(
            "capital reserve conversion requires registered issuer repurchase account facts to exclude issuer treasury shares",
        ));
    }
    let issuer_treasury_shares_excluded = grouped
        .remove(&HolderId::IssuerTreasury)
        .map(|(shares, _)| shares)
        .unwrap_or(0);
    let mut holders = Vec::with_capacity(grouped.len());
    let mut base_total = 0_u64;
    let mut maximum_total = 0_u64;
    let mut eligible_shares = 0_u64;
    for (holder, (original_shares, original_lots)) in grouped {
        eligible_shares = eligible_shares
            .checked_add(original_shares)
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
        let product = u128::from(original_shares)
            .checked_mul(u128::from(plan.shares_per_existing_share_micros))
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
        let whole = u64::try_from(product / u128::from(RATIO_DENOMINATOR))
            .map_err(|_| StockDistributionError::ArithmeticOverflow)?;
        let remainder = u64::try_from(product % u128::from(RATIO_DENOMINATOR))
            .map_err(|_| StockDistributionError::ArithmeticOverflow)?;
        base_total = base_total
            .checked_add(whole)
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
        maximum_total = maximum_total
            .checked_add(
                whole
                    .checked_add(u64::from(remainder > 0))
                    .ok_or(StockDistributionError::ArithmeticOverflow)?,
            )
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
        holders.push(HolderDistribution {
            holder,
            original_shares,
            whole_shares: whole,
            fractional_numerator: remainder,
            original_lots,
        });
    }

    let aggregate_product = u128::from(eligible_shares)
        .checked_mul(u128::from(plan.shares_per_existing_share_micros))
        .ok_or(StockDistributionError::ArithmeticOverflow)?;
    let aggregate_floor = u64::try_from(aggregate_product / u128::from(RATIO_DENOMINATOR))
        .map_err(|_| StockDistributionError::ArithmeticOverflow)?;
    let aggregate_ceil = aggregate_floor
        .checked_add(u64::from(
            aggregate_product % u128::from(RATIO_DENOMINATOR) > 0,
        ))
        .ok_or(StockDistributionError::ArithmeticOverflow)?;
    let minimum = base_total.max(aggregate_floor);
    let maximum = maximum_total.min(aggregate_ceil);
    if minimum > maximum
        || plan.approved_total_new_shares < minimum
        || plan.approved_total_new_shares > maximum
    {
        return Err(StockDistributionError::ApprovedTotalInfeasible {
            approved: plan.approved_total_new_shares,
            minimum,
            maximum,
        });
    }
    let extra = plan.approved_total_new_shares - base_total;
    let mut candidates: Vec<usize> = holders
        .iter()
        .enumerate()
        .filter_map(|(index, holder)| (holder.fractional_numerator > 0).then_some(index))
        .collect();
    candidates.sort_by(|left, right| {
        holders[*right]
            .fractional_numerator
            .cmp(&holders[*left].fractional_numerator)
    });
    shuffle_equal_remainders(&mut candidates, &holders, tie_break_seed);
    for index in candidates
        .into_iter()
        .take(usize::try_from(extra).map_err(|_| StockDistributionError::ArithmeticOverflow)?)
    {
        holders[index].whole_shares = holders[index]
            .whole_shares
            .checked_add(1)
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
    }

    Ok(StockDistributionReceipt {
        event_id: plan.event_id.clone(),
        approval_reference: plan.approval_reference.clone(),
        registration_event_id: snapshot.event_id().to_owned(),
        stock: snapshot.stock().clone(),
        issuer: snapshot.issuer().clone(),
        registered_on: snapshot.registered_on(),
        issued_shares_before: snapshot.issued_shares(),
        issuer_treasury_shares_excluded,
        kind: plan.kind.clone(),
        shares_per_existing_share_micros: plan.shares_per_existing_share_micros,
        approved_total_new_shares: plan.approved_total_new_shares,
        tie_break_seed,
        source_lot_attribution: SourceLotAttribution::SourceLotAttributionPending,
        holders,
    })
}

fn shuffle_equal_remainders(candidates: &mut [usize], holders: &[HolderDistribution], seed: u64) {
    let mut random = SeededRandom(seed);
    let mut start = 0;
    while start < candidates.len() {
        let remainder = holders[candidates[start]].fractional_numerator;
        let mut end = start + 1;
        while end < candidates.len() && holders[candidates[end]].fractional_numerator == remainder {
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

fn invalid(detail: &str) -> StockDistributionError {
    StockDistributionError::InvalidPlan {
        detail: detail.to_owned(),
    }
}

/// 送转事件全链路方案：在纯分配输入之上补充发行人、市场与法定日期。
///
/// `ex_rights_on` 同时承担两重身份并按各自法源校验为同一日期：
/// 一是交易所交易规则的除权日（沪市 4.3.1／深市 4.4.1：权益登记日次一交易日），
/// 二是中国结算发行人业务指南的新增股份入账（上市）日（R+1，两市指南一致，依据摘录
/// 见 `agents/company-system/stock-distribution.md`）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDistributionEventPlan {
    pub event_id: String,
    pub approval_reference: String,
    pub issuer: CompanyId,
    pub stock: StockCode,
    pub exchange: CalendarExchange,
    pub kind: StockDistributionKind,
    pub approved_on: CivilDate,
    pub announced_on: CivilDate,
    pub registered_on: CivilDate,
    pub ex_rights_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub shares_per_existing_share_micros: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub approved_total_new_shares: u64,
}

impl StockDistributionEventPlan {
    pub fn validate(&self) -> Result<(), StockDistributionError> {
        if self.event_id.trim().is_empty()
            || self.approval_reference.trim().is_empty()
            || self.issuer.0.trim().is_empty()
            || self.stock.0.trim().is_empty()
            || self.shares_per_existing_share_micros == 0
            || self.approved_on > self.announced_on
            || self.announced_on > self.registered_on
            || self.registered_on >= self.ex_rights_on
        {
            return Err(invalid(
                "identities, positive ratio and ordered dates are required",
            ));
        }
        Ok(())
    }

    /// 使用调用方提供的权威日历校验登记日与 R+1 日期；不默认补齐日历。
    pub fn validate_calendar(
        &self,
        calendar: &TradingCalendar,
    ) -> Result<(), StockDistributionError> {
        let is_registration_day = calendar
            .is_trading_day(self.exchange, self.registered_on)
            .map_err(|error| StockDistributionError::CalendarLookup {
                detail: error.to_string(),
            })?;
        if !is_registration_day {
            return Err(StockDistributionError::InvalidTradingDate {
                date: self.registered_on,
                exchange: self.exchange,
            });
        }
        let next_trading_day = calendar
            .next_trading_day(self.exchange, self.registered_on)
            .map_err(|error| StockDistributionError::CalendarLookup {
                detail: error.to_string(),
            })?;
        if next_trading_day != self.ex_rights_on {
            return Err(StockDistributionError::InvalidExRightsDate {
                expected: next_trading_day,
                actual: self.ex_rights_on,
                exchange: self.exchange,
            });
        }
        Ok(())
    }

    fn allocation_plan(&self) -> StockDistributionPlan {
        StockDistributionPlan {
            event_id: self.event_id.clone(),
            approval_reference: self.approval_reference.clone(),
            kind: self.kind.clone(),
            shares_per_existing_share_micros: self.shares_per_existing_share_micros,
            approved_total_new_shares: self.approved_total_new_shares,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StockDistributionStatus {
    Approved,
    Announced,
    Registered,
    Credited,
}

/// 送转登记执行状态机：显式计划 → 公告 → R 日按登记快照分配 → R+1 新股入账。
///
/// 状态机本身只记录事实；真实股份入账由 Session 以
/// [`crate::company::share_registry::MovementScope::NonTradingTransfer`] 候选事务执行。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StockDistributionBookState")]
#[serde(deny_unknown_fields)]
pub struct StockDistributionBook {
    plan: StockDistributionEventPlan,
    status: StockDistributionStatus,
    #[serde(deserialize_with = "required_nullable_registration")]
    registration: Option<RegistrationSnapshot>,
    receipt: Option<StockDistributionReceipt>,
    credited_on: Option<CivilDate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StockDistributionBookState {
    plan: StockDistributionEventPlan,
    status: StockDistributionStatus,
    #[serde(deserialize_with = "required_nullable_registration")]
    registration: Option<RegistrationSnapshot>,
    receipt: Option<StockDistributionReceipt>,
    credited_on: Option<CivilDate>,
}

impl TryFrom<StockDistributionBookState> for StockDistributionBook {
    type Error = StockDistributionError;

    fn try_from(state: StockDistributionBookState) -> Result<Self, Self::Error> {
        let book = Self {
            plan: state.plan,
            status: state.status,
            registration: state.registration,
            receipt: state.receipt,
            credited_on: state.credited_on,
        };
        book.validate()?;
        Ok(book)
    }
}

impl StockDistributionBook {
    pub fn new(plan: StockDistributionEventPlan) -> Result<Self, StockDistributionError> {
        plan.validate()?;
        Ok(Self {
            plan,
            status: StockDistributionStatus::Approved,
            registration: None,
            receipt: None,
            credited_on: None,
        })
    }

    pub fn plan(&self) -> &StockDistributionEventPlan {
        &self.plan
    }
    pub fn status(&self) -> &StockDistributionStatus {
        &self.status
    }
    pub fn registration(&self) -> Option<&RegistrationSnapshot> {
        self.registration.as_ref()
    }
    pub fn receipt(&self) -> Option<&StockDistributionReceipt> {
        self.receipt.as_ref()
    }
    pub fn credited_on(&self) -> Option<CivilDate> {
        self.credited_on
    }

    pub fn announce(&mut self, on: CivilDate) -> Result<(), StockDistributionError> {
        if self.status != StockDistributionStatus::Approved {
            return if on == self.plan.announced_on {
                Ok(())
            } else {
                Err(StockDistributionError::WrongStage {
                    status: self.status.clone(),
                    operation: "announce on a conflicting date",
                })
            };
        }
        if on != self.plan.announced_on {
            return Err(StockDistributionError::WrongStage {
                status: self.status.clone(),
                operation: "announce",
            });
        }
        self.status = StockDistributionStatus::Announced;
        Ok(())
    }

    /// R 日冻结登记快照并按账户级碎股算法确定每户新股数；重复提交同一快照幂等。
    pub fn register(
        &mut self,
        snapshot: RegistrationSnapshot,
        calendar: &TradingCalendar,
    ) -> Result<&StockDistributionReceipt, StockDistributionError> {
        self.plan.validate_calendar(calendar)?;
        if let Some(existing) = &self.registration {
            if existing == &snapshot {
                return self
                    .receipt
                    .as_ref()
                    .ok_or_else(|| StockDistributionError::SnapshotMismatch {
                        detail: "registered book is missing its allocation receipt".into(),
                    });
            }
            return Err(StockDistributionError::SnapshotMismatch {
                detail: "registration was already frozen for this plan".into(),
            });
        }
        if self.status != StockDistributionStatus::Announced {
            return Err(StockDistributionError::WrongStage {
                status: self.status.clone(),
                operation: "register",
            });
        }
        snapshot.validate()?;
        if snapshot.event_id() != self.plan.event_id
            || snapshot.stock() != &self.plan.stock
            || snapshot.issuer() != &self.plan.issuer
            || snapshot.registered_on() != self.plan.registered_on
        {
            return Err(StockDistributionError::SnapshotMismatch {
                detail: "snapshot plan identity, issuer, security or registration date differs"
                    .into(),
            });
        }
        let receipt =
            allocate_stock_distribution(&snapshot, &self.plan.allocation_plan(), {
                tie_break_seed(&self.plan.event_id)
            })?;
        self.registration = Some(snapshot);
        self.receipt = Some(receipt);
        self.status = StockDistributionStatus::Registered;
        self.receipt
            .as_ref()
            .ok_or_else(|| StockDistributionError::SnapshotMismatch {
                detail: "allocation receipt insertion failed".into(),
            })
    }

    /// R+1 新股入账完成后由 Session 记录；状态机不自行修改股东名册或账户。
    pub fn mark_credited(&mut self, on: CivilDate) -> Result<(), StockDistributionError> {
        match self.status.clone() {
            StockDistributionStatus::Credited => {
                if self.credited_on == Some(on) {
                    Ok(())
                } else {
                    Err(StockDistributionError::WrongStage {
                        status: self.status.clone(),
                        operation: "mark credited on a conflicting date",
                    })
                }
            }
            StockDistributionStatus::Registered => {
                if on != self.plan.ex_rights_on {
                    return Err(StockDistributionError::WrongStage {
                        status: self.status.clone(),
                        operation: "mark credited before the ex-rights/credit date",
                    });
                }
                self.credited_on = Some(on);
                self.status = StockDistributionStatus::Credited;
                Ok(())
            }
            status => Err(StockDistributionError::WrongStage {
                status,
                operation: "mark credited",
            }),
        }
    }

    pub fn validate(&self) -> Result<(), StockDistributionError> {
        self.plan.validate()?;
        let registered = self.registration.is_some();
        if registered
            != matches!(
                self.status,
                StockDistributionStatus::Registered | StockDistributionStatus::Credited
            )
        {
            return Err(StockDistributionError::SnapshotMismatch {
                detail: "status and frozen registration do not agree".into(),
            });
        }
        if registered != self.receipt.is_some() {
            return Err(StockDistributionError::SnapshotMismatch {
                detail: "registration and allocation receipt do not agree".into(),
            });
        }
        if let Some(snapshot) = &self.registration {
            snapshot.validate()?;
            if snapshot.event_id() != self.plan.event_id
                || snapshot.stock() != &self.plan.stock
                || snapshot.issuer() != &self.plan.issuer
                || snapshot.registered_on() != self.plan.registered_on
            {
                return Err(StockDistributionError::SnapshotMismatch {
                    detail: "restored registration does not match the plan".into(),
                });
            }
            let expected =
                allocate_stock_distribution(snapshot, &self.plan.allocation_plan(), {
                    tie_break_seed(&self.plan.event_id)
                })?;
            if self.receipt.as_ref() != Some(&expected) {
                return Err(StockDistributionError::SnapshotMismatch {
                    detail: "restored allocation receipt does not replay the frozen registration"
                        .into(),
                });
            }
        }
        match (self.status.clone(), self.credited_on) {
            (StockDistributionStatus::Credited, Some(credited_on)) => {
                if !registered || credited_on != self.plan.ex_rights_on {
                    return Err(StockDistributionError::SnapshotMismatch {
                        detail: "credited facts do not match the ex-rights/credit date".into(),
                    });
                }
            }
            (_, Some(_)) => {
                return Err(StockDistributionError::SnapshotMismatch {
                    detail: "credit facts require the Credited status".into(),
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// 使用调用方选择的权威日历校验恢复状态；不默认补齐日历。
    pub fn validate_with_calendar(
        &self,
        calendar: &TradingCalendar,
    ) -> Result<(), StockDistributionError> {
        self.validate()?;
        self.plan.validate_calendar(calendar)
    }
}

/// 每个持有人在 R+1 入账的新增股份与限售继承事实。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolderCreditLot {
    pub holder: HolderId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub qty: u64,
    pub restriction: ShareRestriction,
}

/// 由冻结分配回执推导 R+1 每户新 lot 的数量与限售属性。
///
/// 限售继承依据深市指南明文（限售股份孳生的送转股仍为限售股、截止日与原股份一致）；
/// 沪市对应条文未定位，两市均不做跨市场推断。持有者原 lots 全部为同一限售类时继承
/// 该限售属性与 `release_on`；混合限售来源显式拒绝（保留 SourceLotAttributionPending
/// 语义，不按比例摊）。原限售截止日在入账日前已经到期的，处置路径本就视同可流通，
/// 新 lot 登记为 Unrestricted 而不是伪造一个早于取得日的截止日。
pub fn holder_credit_lots(
    receipt: &StockDistributionReceipt,
    acquired_on: CivilDate,
) -> Result<Vec<HolderCreditLot>, StockDistributionError> {
    if acquired_on <= receipt.registered_on {
        return Err(invalid(
            "credit lots can only be derived for a date after the registration date",
        ));
    }
    let mut lots = Vec::with_capacity(receipt.holders.len());
    for holder in &receipt.holders {
        if holder.whole_shares == 0 {
            continue;
        }
        let restriction = inherited_restriction(holder, acquired_on)?;
        lots.push(HolderCreditLot {
            holder: holder.holder.clone(),
            qty: holder.whole_shares,
            restriction,
        });
    }
    Ok(lots)
}

fn inherited_restriction(
    holder: &HolderDistribution,
    acquired_on: CivilDate,
) -> Result<ShareRestriction, StockDistributionError> {
    let mut common: Option<&ShareRestriction> = None;
    for lot in &holder.original_lots {
        match common {
            None => common = Some(&lot.restriction),
            Some(existing) if existing == &lot.restriction => {}
            Some(_) => {
                return Err(StockDistributionError::SourceLotAttributionConflict {
                    holder: holder.holder.clone(),
                });
            }
        }
    }
    match common {
        None => Err(invalid("registered holder has no source lots to inherit from")),
        Some(ShareRestriction::Unrestricted) => Ok(ShareRestriction::Unrestricted),
        Some(ShareRestriction::Restricted { reason, release_on }) => {
            if *release_on >= acquired_on {
                Ok(ShareRestriction::Restricted {
                    reason: reason.clone(),
                    release_on: *release_on,
                })
            } else {
                Ok(ShareRestriction::Unrestricted)
            }
        }
    }
}

/// 官方材料只要求同余碎股由结算系统"随机"排列；本实现以事件身份派生可复现 seed
/// （FNV-1a 后接 SplitMix64 雪崩），作为显式游戏输入记录在分配回执中。
fn tie_break_seed(event_id: &str) -> u64 {
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

/// Simple 账面送转展示登记的声明输入。
///
/// 送股（`BonusShares`）与转增（`CapitalReserveConversion`）都按面值增加股本；
/// 本批 Simple 账面未建模资本公积科目与来源类别，也不变更注册资本法定事实，
/// 因此只做面值口径的展示登记，不进行借贷过账，不产生投资者现金或公司现金。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDistributionDeclaration {
    pub event_id: String,
    pub approval_reference: String,
    pub kind: StockDistributionKind,
    pub approved_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub new_shares: u64,
    pub par_value_per_share: Money,
    pub capital_increase: AccountingAmount,
    pub registered_capital_at_approval: AccountingAmount,
}

/// Simple 账面送转事实投影：声明即冻结，`credited_on` 在真实入账后回填。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDistributionFinanceFact {
    pub event_id: String,
    pub approval_reference: String,
    pub kind: StockDistributionKind,
    pub approved_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub new_shares: u64,
    pub par_value_per_share: Money,
    pub capital_increase: AccountingAmount,
    pub registered_capital_at_approval: AccountingAmount,
    pub credited_on: Option<CivilDate>,
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
#[path = "stock_distribution_tests.rs"]
mod tests;
