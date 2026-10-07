//! 拆股／缩股（股份拆细与股份合并）机制状态机与分配算法。
//!
//! 制度背景（依据分级见 docs/trading-rules.md「拆股／缩股」节与
//! agents/company-system/share-split.md）：
//!
//! - **拆股（`Split`，1 拆 N）**：A 股现行制度下无常规通道与先例（公司法无专门
//!   拆细程序、沪深发行人业务指南无拆股条文）；经济实质为股份拆细——每股面值按
//!   比例缩小、注册资本不变、股数按比例放大。本机制按该标准语义建模，登记为
//!   游戏实现口径（ADR-0039 决策 3 要求先核验再实现；核验结论即「无常规通道」，
//!   不硬造官方依据）。
//! - **缩股（`Consolidate`，N 并 1）**：现实仅见于特殊场景（破产重整出资人权益
//!   调整、股权分置改革缩股对价、纯 B 股缩股保上市），实质为形式减资：每股面值
//!   按比例放大、股数按比例缩小，注册资本按消灭股份的面值核减；不向股东分配
//!   资产，不产生股息红利所得。
//! - **回购专户参与**：拆股／缩股是全体股份的重新计值，不是权益分派；证监会
//!   回购规则第 13 条的专户失权针对「利润分配与公积金转增股本」，不覆盖重新
//!   计值，因此专户（`IssuerTreasury`）股份一并按比例换算（与送转排除专户
//!   形成语义区分）。
//! - **碎股**：拆股 1 拆 N（整数 N）逐户数量乘 N 恒整除，不产生碎股；缩股 N 并 1
//!   逐户取整产生余数——官方对缩股碎股无统一明文，按送转碎股同一先例口径
//!   （碎股余数降序、同数 seed 洗牌、依序各多得 1 股）分配汇总后的整股，
//!   seed 由事件身份派生并记录在回执中。
//!
//! 状态机只记录事实；真实股份入账由 Session 以
//! [`crate::company::share_registry::MovementScope::ShareReDenomination`] 候选事务执行。

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::share_registry::{HolderId, RegistrationSnapshot, ShareRegistryError};
use super::CompanyId;
use crate::account::StockCode;
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ShareSplitDirection {
    /// 拆股（股份拆细）：每 1 股换 `ratio` 股，面值除以 `ratio`。
    Split,
    /// 缩股（股份合并）：每 `ratio` 股换 1 股，面值乘以 `ratio`。
    Consolidate,
}

/// 拆股／缩股事件全链路方案：方向、整数比例与法定日期。
///
/// `ex_rights_on` 与送转同双重身份：交易所交易规则的除权日（沪市 4.3.1／深市
/// 4.4.1：权益登记日次一交易日；公式代入负／正比例即覆盖缩股／拆股，见
/// ex_reference_price 模块）与重新计值后的股份入账日（R+1，按送转先例口径）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareSplitEventPlan {
    pub event_id: String,
    pub approval_reference: String,
    pub issuer: CompanyId,
    pub stock: StockCode,
    pub exchange: CalendarExchange,
    pub direction: ShareSplitDirection,
    /// 整数换算比例（≥ 2）：拆股为每 1 股换 `ratio` 股；缩股为每 `ratio` 股换 1 股。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub ratio: u64,
    pub approved_on: CivilDate,
    pub announced_on: CivilDate,
    pub registered_on: CivilDate,
    pub ex_rights_on: CivilDate,
}

impl ShareSplitEventPlan {
    pub fn validate(&self) -> Result<(), ShareSplitError> {
        if self.event_id.trim().is_empty()
            || self.approval_reference.trim().is_empty()
            || self.issuer.0.trim().is_empty()
            || self.stock.0.trim().is_empty()
            || self.ratio < 2
            || self.approved_on > self.announced_on
            || self.announced_on > self.registered_on
            || self.registered_on >= self.ex_rights_on
        {
            return Err(ShareSplitError::InvalidPlan {
                detail: "identities, integer ratio >= 2 and ordered dates are required".into(),
            });
        }
        Ok(())
    }

    /// 使用调用方提供的权威日历校验登记日与 R+1 日期；不默认补齐日历。
    pub fn validate_calendar(
        &self,
        calendar: &TradingCalendar,
    ) -> Result<(), ShareSplitError> {
        let is_registration_day = calendar
            .is_trading_day(self.exchange, self.registered_on)
            .map_err(|error| ShareSplitError::CalendarLookup {
                detail: error.to_string(),
            })?;
        if !is_registration_day {
            return Err(ShareSplitError::InvalidTradingDate {
                date: self.registered_on,
                exchange: self.exchange,
            });
        }
        let next_trading_day = calendar
            .next_trading_day(self.exchange, self.registered_on)
            .map_err(|error| ShareSplitError::CalendarLookup {
                detail: error.to_string(),
            })?;
        if next_trading_day != self.ex_rights_on {
            return Err(ShareSplitError::InvalidExRightsDate {
                expected: next_trading_day,
                actual: self.ex_rights_on,
                exchange: self.exchange,
            });
        }
        Ok(())
    }

    /// 每股旧股换得的新股数（有理数：拆股 ratio/1，缩股 1/ratio）。
    pub fn new_shares_per_old_share(&self) -> (u64, u64) {
        match self.direction {
            ShareSplitDirection::Split => (self.ratio, 1),
            ShareSplitDirection::Consolidate => (1, self.ratio),
        }
    }
}

/// 每个持有人（含回购专户）的换算结果。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolderSplitOutcome {
    pub holder: HolderId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub original_shares: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub new_shares: u64,
    /// 缩股碎股分子（original_shares % ratio；拆股恒为零）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub fractional_numerator: u64,
    /// 碎股整股奖励（缩股：余数降序获奖 +1；拆股恒为零）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub tie_break_award: u64,
    /// 冻结登记快照中的原 lots（限售继承的事实来源，与送转回执同构）。
    pub original_lots: Vec<super::share_registry::ShareLot>,
}

/// 由冻结换算结果推导正向新增 lot 的限售继承（与送转同一纪律：原 lots 全部
/// 为同一限售类时继承该限售属性与截止日；混合限售来源显式拒绝，不按比例摊；
/// 原限售截止日早于入账日的按 Unrestricted 登记）。
pub fn holder_inherited_restriction(
    outcome: &HolderSplitOutcome,
    acquired_on: CivilDate,
) -> Result<super::share_registry::ShareRestriction, ShareSplitError> {
    use super::share_registry::ShareRestriction;
    let mut common: Option<&ShareRestriction> = None;
    for lot in &outcome.original_lots {
        match common {
            None => common = Some(&lot.restriction),
            Some(existing) if existing == &lot.restriction => {}
            Some(_) => {
                return Err(ShareSplitError::SnapshotMismatch {
                    detail: format!(
                        "holder {:?} mixes restriction sources; per-lot attribution stays pending",
                        outcome.holder
                    ),
                });
            }
        }
    }
    match common {
        None => Err(ShareSplitError::InvalidPlan {
            detail: "registered holder has no source lots to inherit from".into(),
        }),
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

/// 冻结登记快照上的确定性换算回执。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareSplitReceipt {
    pub event_id: String,
    pub approval_reference: String,
    pub registration_event_id: String,
    pub stock: StockCode,
    pub issuer: CompanyId,
    pub registered_on: CivilDate,
    pub direction: ShareSplitDirection,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub ratio: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issued_shares_before: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issued_shares_after: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub tie_break_seed: u64,
    pub holders: Vec<HolderSplitOutcome>,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum ShareSplitError {
    #[error("share split: {detail}")]
    InvalidPlan { detail: String },
    #[error("share split: integer arithmetic overflow")]
    ArithmeticOverflow,
    #[error("share split: operation {operation} is invalid in status {status:?}")]
    WrongStage {
        status: ShareSplitStatus,
        operation: &'static str,
    },
    #[error("share split: registration snapshot does not match plan: {detail}")]
    SnapshotMismatch { detail: String },
    #[error("share split: trading calendar lookup failed: {detail}")]
    CalendarLookup { detail: String },
    #[error("share split: registration date {date} is not a {exchange:?} trading day")]
    InvalidTradingDate {
        date: CivilDate,
        exchange: CalendarExchange,
    },
    #[error(
        "share split: ex-rights date {actual} is not the next {exchange:?} trading day {expected}"
    )]
    InvalidExRightsDate {
        expected: CivilDate,
        actual: CivilDate,
        exchange: CalendarExchange,
    },
    #[error("share split: share registry rejected the fact: {0}")]
    ShareRegistry(#[from] ShareRegistryError),
}

/// 在冻结登记快照上执行确定性换算。
///
/// - 拆股：每户新股 = 原股 × ratio（无碎股）。
/// - 缩股：每户基准 = 原股 ÷ ratio、余数 = 原股 % ratio；汇总整股奖励数
///   `extra = floor(总股数 / ratio) − Σ基准`，按余数降序、同数 seed 洗牌后
///   依序各多得 1 股（与送转碎股同一先例口径；守恒由回执字段勾稽）。
/// - 回购专户与其他持有人同规则换算（拆股／缩股不是权益分派，专户不失权）。
pub fn allocate_share_split(
    snapshot: &RegistrationSnapshot,
    plan: &ShareSplitEventPlan,
    tie_break_seed: u64,
) -> Result<ShareSplitReceipt, ShareSplitError> {
    snapshot
        .validate()
        .map_err(|error| ShareSplitError::InvalidPlan {
            detail: format!("invalid registration snapshot: {error}"),
        })?;
    plan.validate()?;
    if snapshot.event_id() != plan.event_id
        || snapshot.stock() != &plan.stock
        || snapshot.issuer() != &plan.issuer
        || snapshot.registered_on() != plan.registered_on
    {
        return Err(ShareSplitError::SnapshotMismatch {
            detail: "snapshot plan identity, issuer, security or registration date differs".into(),
        });
    }
    let mut holders = Vec::new();
    let mut total_before = 0_u64;
    for holding in snapshot.holdings() {
        let original = holding
            .lots
            .iter()
            .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
            .ok_or(ShareSplitError::ArithmeticOverflow)?;
        total_before = total_before
            .checked_add(original)
            .ok_or(ShareSplitError::ArithmeticOverflow)?;
        let (base, remainder) = match plan.direction {
            ShareSplitDirection::Split => (
                original
                    .checked_mul(plan.ratio)
                    .ok_or(ShareSplitError::ArithmeticOverflow)?,
                0,
            ),
            ShareSplitDirection::Consolidate => (original / plan.ratio, original % plan.ratio),
        };
        holders.push(HolderSplitOutcome {
            holder: holding.holder.clone(),
            original_shares: original,
            new_shares: base,
            fractional_numerator: remainder,
            tie_break_award: 0,
            original_lots: holding.lots.clone(),
        });
    }
    if total_before != snapshot.issued_shares() {
        return Err(ShareSplitError::SnapshotMismatch {
            detail: "snapshot holdings do not conserve issued shares".into(),
        });
    }
    let extra = match plan.direction {
        ShareSplitDirection::Split => 0,
        ShareSplitDirection::Consolidate => {
            let target_total = total_before / plan.ratio;
            let base_total = holders
                .iter()
                .try_fold(0_u64, |sum, holder| sum.checked_add(holder.new_shares))
                .ok_or(ShareSplitError::ArithmeticOverflow)?;
            target_total
                .checked_sub(base_total)
                .ok_or(ShareSplitError::SnapshotMismatch {
                    detail: "consolidation base total exceeds the aggregate target".into(),
                })?
        }
    };
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
        .take(usize::try_from(extra).map_err(|_| ShareSplitError::ArithmeticOverflow)?)
    {
        holders[index].new_shares = holders[index]
            .new_shares
            .checked_add(1)
            .ok_or(ShareSplitError::ArithmeticOverflow)?;
        holders[index].tie_break_award = 1;
    }
    let issued_shares_after = holders
        .iter()
        .try_fold(0_u64, |sum, holder| sum.checked_add(holder.new_shares))
        .ok_or(ShareSplitError::ArithmeticOverflow)?;
    match plan.direction {
        ShareSplitDirection::Split => {
            let expected = total_before
                .checked_mul(plan.ratio)
                .ok_or(ShareSplitError::ArithmeticOverflow)?;
            if issued_shares_after != expected {
                return Err(ShareSplitError::SnapshotMismatch {
                    detail: "split allocation does not conserve issued shares".into(),
                });
            }
        }
        ShareSplitDirection::Consolidate => {
            if issued_shares_after > total_before {
                return Err(ShareSplitError::SnapshotMismatch {
                    detail: "consolidation increased issued shares".into(),
                });
            }
        }
    }
    Ok(ShareSplitReceipt {
        event_id: plan.event_id.clone(),
        approval_reference: plan.approval_reference.clone(),
        registration_event_id: snapshot.event_id().to_owned(),
        stock: snapshot.stock().clone(),
        issuer: snapshot.issuer().clone(),
        registered_on: snapshot.registered_on(),
        direction: plan.direction.clone(),
        ratio: plan.ratio,
        issued_shares_before: total_before,
        issued_shares_after,
        tie_break_seed,
        holders,
    })
}

fn shuffle_equal_remainders(candidates: &mut [usize], holders: &[HolderSplitOutcome], seed: u64) {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ShareSplitStatus {
    Approved,
    Announced,
    Registered,
    Settled,
}

/// 拆股／缩股登记执行状态机：显式计划 → 公告 → R 日冻结快照换算 → R+1 重新计值入账。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ShareSplitBookState")]
#[serde(deny_unknown_fields)]
pub struct ShareSplitBook {
    plan: ShareSplitEventPlan,
    status: ShareSplitStatus,
    registration: Option<RegistrationSnapshot>,
    receipt: Option<ShareSplitReceipt>,
    settled_on: Option<CivilDate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShareSplitBookState {
    plan: ShareSplitEventPlan,
    status: ShareSplitStatus,
    registration: Option<RegistrationSnapshot>,
    receipt: Option<ShareSplitReceipt>,
    settled_on: Option<CivilDate>,
}

impl TryFrom<ShareSplitBookState> for ShareSplitBook {
    type Error = ShareSplitError;

    fn try_from(state: ShareSplitBookState) -> Result<Self, Self::Error> {
        let book = Self {
            plan: state.plan,
            status: state.status,
            registration: state.registration,
            receipt: state.receipt,
            settled_on: state.settled_on,
        };
        book.validate()?;
        Ok(book)
    }
}

impl ShareSplitBook {
    pub fn new(plan: ShareSplitEventPlan) -> Result<Self, ShareSplitError> {
        plan.validate()?;
        Ok(Self {
            plan,
            status: ShareSplitStatus::Approved,
            registration: None,
            receipt: None,
            settled_on: None,
        })
    }

    pub fn plan(&self) -> &ShareSplitEventPlan {
        &self.plan
    }
    pub fn status(&self) -> &ShareSplitStatus {
        &self.status
    }
    pub fn registration(&self) -> Option<&RegistrationSnapshot> {
        self.registration.as_ref()
    }
    pub fn receipt(&self) -> Option<&ShareSplitReceipt> {
        self.receipt.as_ref()
    }
    pub fn settled_on(&self) -> Option<CivilDate> {
        self.settled_on
    }

    pub fn announce(&mut self, on: CivilDate) -> Result<(), ShareSplitError> {
        if self.status != ShareSplitStatus::Approved {
            return if on == self.plan.announced_on {
                Ok(())
            } else {
                Err(ShareSplitError::WrongStage {
                    status: self.status.clone(),
                    operation: "announce on a conflicting date",
                })
            };
        }
        if on != self.plan.announced_on {
            return Err(ShareSplitError::WrongStage {
                status: self.status.clone(),
                operation: "announce",
            });
        }
        self.status = ShareSplitStatus::Announced;
        Ok(())
    }

    /// R 日冻结登记快照并确定每户换算结果；重复提交同一快照幂等。
    pub fn register(
        &mut self,
        snapshot: RegistrationSnapshot,
        calendar: &TradingCalendar,
    ) -> Result<&ShareSplitReceipt, ShareSplitError> {
        self.plan.validate_calendar(calendar)?;
        if let Some(existing) = &self.registration {
            if existing == &snapshot {
                return self
                    .receipt
                    .as_ref()
                    .ok_or_else(|| ShareSplitError::SnapshotMismatch {
                        detail: "registered book is missing its allocation receipt".into(),
                    });
            }
            return Err(ShareSplitError::SnapshotMismatch {
                detail: "registration was already frozen for this plan".into(),
            });
        }
        if self.status != ShareSplitStatus::Announced {
            return Err(ShareSplitError::WrongStage {
                status: self.status.clone(),
                operation: "register",
            });
        }
        let receipt =
            allocate_share_split(&snapshot, &self.plan, tie_break_seed(&self.plan.event_id))?;
        self.registration = Some(snapshot);
        self.receipt = Some(receipt);
        self.status = ShareSplitStatus::Registered;
        self.receipt
            .as_ref()
            .ok_or_else(|| ShareSplitError::SnapshotMismatch {
                detail: "allocation receipt insertion failed".into(),
            })
    }

    /// R+1 重新计值入账完成后由 Session 记录；状态机不自行修改股东名册或账户。
    pub fn mark_settled(&mut self, on: CivilDate) -> Result<(), ShareSplitError> {
        match self.status.clone() {
            ShareSplitStatus::Settled => {
                if self.settled_on == Some(on) {
                    Ok(())
                } else {
                    Err(ShareSplitError::WrongStage {
                        status: self.status.clone(),
                        operation: "mark settled on a conflicting date",
                    })
                }
            }
            ShareSplitStatus::Registered => {
                if on != self.plan.ex_rights_on {
                    return Err(ShareSplitError::WrongStage {
                        status: self.status.clone(),
                        operation: "mark settled before the ex-rights date",
                    });
                }
                self.settled_on = Some(on);
                self.status = ShareSplitStatus::Settled;
                Ok(())
            }
            status => Err(ShareSplitError::WrongStage {
                status,
                operation: "mark settled",
            }),
        }
    }

    pub fn validate(&self) -> Result<(), ShareSplitError> {
        self.plan.validate()?;
        let registered = self.registration.is_some();
        if registered
            != matches!(
                self.status,
                ShareSplitStatus::Registered | ShareSplitStatus::Settled
            )
        {
            return Err(ShareSplitError::SnapshotMismatch {
                detail: "status and frozen registration do not agree".into(),
            });
        }
        if registered != self.receipt.is_some() {
            return Err(ShareSplitError::SnapshotMismatch {
                detail: "registration and allocation receipt do not agree".into(),
            });
        }
        if let Some(snapshot) = &self.registration {
            let expected =
                allocate_share_split(snapshot, &self.plan, tie_break_seed(&self.plan.event_id))?;
            if self.receipt.as_ref() != Some(&expected) {
                return Err(ShareSplitError::SnapshotMismatch {
                    detail: "restored allocation receipt does not replay the frozen registration"
                        .into(),
                });
            }
        }
        match (self.status.clone(), self.settled_on) {
            (ShareSplitStatus::Settled, Some(settled_on)) => {
                if !registered || settled_on != self.plan.ex_rights_on {
                    return Err(ShareSplitError::SnapshotMismatch {
                        detail: "settled facts do not match the ex-rights date".into(),
                    });
                }
            }
            (_, Some(_)) => {
                return Err(ShareSplitError::SnapshotMismatch {
                    detail: "settle facts require the Settled status".into(),
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
    ) -> Result<(), ShareSplitError> {
        self.validate()?;
        self.plan.validate_calendar(calendar)
    }
}

/// 官方材料只要求同余碎股由结算系统"随机"排列（送转先例）；本实现以事件身份
/// 派生可复现 seed（FNV-1a 后接 SplitMix64 雪崩），作为显式游戏输入记录在回执中。
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

/// Simple 账面拆股／缩股展示登记的声明输入。
///
/// 面值口径：拆股面值按比例缩小（须整除为整数分）、注册资本不变；缩股面值按
/// 比例放大、注册资本按「旧股口径消灭面值：par_before×(S_before−ratio×S_after)」
/// 核减（真实入账后回填）。Simple 账面只冻结面值口径展示事实，不做借贷过账，
/// 不产生公司或投资者现金。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareSplitDeclaration {
    pub event_id: String,
    pub approval_reference: String,
    pub direction: ShareSplitDirection,
    pub approved_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub ratio: u64,
    pub par_value_before: crate::money::Money,
    pub par_value_after: crate::money::Money,
    pub registered_capital_at_approval: crate::accounting::AccountingAmount,
}

/// Simple 账面拆股／缩股事实投影：声明即冻结，`settled_on` 在真实入账后回填、
/// `destroyed_shares`（缩股消灭股数）与 `registered_capital_reduction` 同步回填。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareSplitFinanceFact {
    pub event_id: String,
    pub approval_reference: String,
    pub direction: ShareSplitDirection,
    pub approved_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub ratio: u64,
    pub par_value_before: crate::money::Money,
    pub par_value_after: crate::money::Money,
    pub registered_capital_at_approval: crate::accounting::AccountingAmount,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issued_shares_before: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issued_shares_after: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub destroyed_shares: u64,
    pub registered_capital_reduction: crate::accounting::AccountingAmount,
    pub settled_on: Option<CivilDate>,
}

#[cfg(test)]
#[path = "share_split_tests.rs"]
mod tests;
