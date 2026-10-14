use super::conservation::{FeeComponents, ReceiptDelta, ResVec};
use super::{EnvelopeKey, StepFatal};
use crate::{LimitPrice, Money};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum EnvelopeOrigin {
    TickStart,

    CreatedAtValidation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct EnvelopeAudit {
    pub limit: Money,
    pub remaining_qty: u32,
    pub filled_qty: u32,
    pub filled_value: Money,
    pub nominal: FeeComponents,
    pub charged: FeeComponents,
}

/// 精确费用输入；不是整个GameConfig的等同性，更不是page地址。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FeeInputs {
    commission_rate_bits: u64,
    commission_min: Money,
    stamp_tax_rate_bits: u64,
    transfer_rate_bits: u64,
}
impl FeeInputs {
    fn capture(config: &crate::GameConfig) -> Self {
        Self { commission_rate_bits: config.commission_rate.to_bits(), commission_min: config.commission_min,
            stamp_tax_rate_bits: config.stamp_tax_rate.to_bits(),
            transfer_rate_bits: crate::config::A_SHARE_TRANSFER_FEE_RATE.to_bits() }
    }
}

/// 只有完整财务校验成功才能建立；字段与构造器均为本模块私有，不可serde信任。
#[derive(Clone, Debug)]
struct FeeProof {
    inputs: FeeInputs,
    content_revision: u64,
    // 不进存档；checked迁移使用同一已验证费用上下文（test探针仍按实例Arc归属）。
    calculator: crate::GameConfig,
}

#[derive(Clone, Debug)]
enum FeeFailure {
    Fatal(StepFatal),
    LiveResources,
}
impl From<StepFatal> for FeeFailure {
    fn from(value: StepFatal) -> Self { Self::Fatal(value) }
}
#[derive(Clone, Copy)]
enum FeeCheckSite { Generic, BookCapture, StockRound }

/// 每个规范Envelope持有自己的证明。clone/COW共享同内容的不可变证明，
/// 财务内容改变时换新OnceLock；同page其他订单不受粗粒度失效影响。
#[derive(Clone, Default)]
struct FinancialRuntime {
    content_revision: u64,
    proof: std::sync::Arc<std::sync::OnceLock<Result<FeeProof, FeeFailure>>>,
}
impl std::fmt::Debug for FinancialRuntime {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("FinancialRuntime")
    }
}
impl PartialEq for FinancialRuntime {
    fn eq(&self, _other: &Self) -> bool { true }
}
impl Eq for FinancialRuntime {}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Envelope {
    key: EnvelopeKey,
    origin: EnvelopeOrigin,
    basis: ResVec,
    live: ResVec,
    spent: ResVec,
    released: ResVec,
    audit: EnvelopeAudit,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_price: Option<LimitPrice>,
    #[serde(skip)]
    financial_runtime: FinancialRuntime,
}

/// 只读 TickStart 投影借用股票代码；费用与资源仍由订单事实独立计算。
#[derive(Clone, Copy, Debug)]
pub(in crate::session) struct BorrowedTickStartEnvelope<'a> {
    pub account: crate::AccountId,
    pub stock: &'a crate::StockId,
    pub order: crate::OrderId,
    pub side: crate::Side,
    pub live: ResVec,
    pub audit: EnvelopeAudit,
}

impl BorrowedTickStartEnvelope<'_> {
    pub(in crate::session) fn identity(
        &self,
    ) -> (crate::AccountId, &crate::StockId, crate::OrderId, u8) {
        (
            self.account,
            self.stock,
            self.order,
            match self.side {
                crate::Side::Buy => 0,
                crate::Side::Sell => 1,
            },
        )
    }

    pub(in crate::session) fn copy_key_into(&self, key: &mut EnvelopeKey) {
        key.account = self.account;
        key.stock.0.clone_from(&self.stock.0);
        key.order = self.order;
        key.side = self.side;
    }

    pub(in crate::session) fn matches_key(&self, key: &EnvelopeKey) -> bool {
        key.account == self.account
            && key.stock == *self.stock
            && key.order == self.order
            && key.side == self.side
    }

    pub(in crate::session) fn matches(&self, envelope: &Envelope) -> bool {
        // 显式解构所有字段，Envelope 新增字段时必须同步此完整比较。
        let Envelope {
            key,
            origin,
            basis,
            live,
            spent,
            released,
            audit,
            pending_price,
            // 运行时证明不是业务投影字段，完整业务比较仍逐项覆盖上方全部事实。
            financial_runtime: _,
        } = envelope;
        self.matches_key(key)
            && *origin == EnvelopeOrigin::TickStart
            && *basis == self.live
            && *live == self.live
            && *spent == ResVec::ZERO
            && *released == ResVec::ZERO
            && *audit == self.audit
            && pending_price.is_none()
    }
}

impl Envelope {
    pub fn tick_start_existing(
        key: EnvelopeKey,
        cash: Money,
        shares: u32,
        audit: EnvelopeAudit,
    ) -> Self {
        let live = ResVec::new(cash, shares);
        Self {
            key,
            origin: EnvelopeOrigin::TickStart,
            basis: live,
            live,
            spent: ResVec::ZERO,
            released: ResVec::ZERO,
            audit,
            pending_price: None,
            financial_runtime: FinancialRuntime::default(),
        }
    }

    pub fn created_at_validation(
        key: EnvelopeKey,
        cash: Money,
        shares: u32,
        audit: EnvelopeAudit,
    ) -> Self {
        let live = ResVec::new(cash, shares);
        Self {
            key,
            origin: EnvelopeOrigin::CreatedAtValidation,
            basis: live,
            live,
            spent: ResVec::ZERO,
            released: ResVec::ZERO,
            audit,
            pending_price: None,
            financial_runtime: FinancialRuntime::default(),
        }
    }

    pub fn created_at_validation_with_pending_price(
        key: EnvelopeKey,
        cash: Money,
        shares: u32,
        audit: EnvelopeAudit,
        pending_price: Option<LimitPrice>,
    ) -> Self {
        let mut envelope = Self::created_at_validation(key, cash, shares, audit);
        envelope.pending_price = pending_price;
        envelope
    }

    pub const fn live(&self) -> ResVec {
        self.live
    }
    pub const fn basis(&self) -> ResVec {
        self.basis
    }
    pub const fn spent(&self) -> ResVec {
        self.spent
    }
    pub const fn released(&self) -> ResVec {
        self.released
    }

    pub const fn key(&self) -> &EnvelopeKey {
        &self.key
    }

    pub const fn origin(&self) -> EnvelopeOrigin {
        self.origin
    }
    pub const fn audit(&self) -> EnvelopeAudit {
        self.audit
    }

    pub const fn pending_price(&self) -> Option<LimitPrice> {
        self.pending_price
    }

    /// 获得同一规范订单的已验证资源事实；所有消费者复用此入口，不再重建费用。
    /// 一般basis/spent/released守恒每次照常检查，不在FeeProof的缓存覆盖范围内。
    pub(in crate::session) fn verified_live(&self, config: &crate::GameConfig) -> Result<ResVec, StepFatal> {
        self.verified_live_at(config, FeeCheckSite::Generic)
    }
    pub(super) fn verified_live_for_capture(&self, config: &crate::GameConfig) -> Result<ResVec, StepFatal> {
        self.verified_live_at(config, FeeCheckSite::BookCapture)
    }
    pub(super) fn verified_live_for_stock(&self, config: &crate::GameConfig) -> Result<ResVec, StepFatal> {
        self.verified_live_at(config, FeeCheckSite::StockRound)
    }
    /// 出生财务不变量（零成交额 ⇒ 零已计/已收费用）。不在通用`validate()`中：
    /// restore的存档审计不一致须按既有类型化fault（如2228）报。本检查只在
    /// 证明建立（verified_live）处执行——晚于全部typed fault层；ledger构造器
    /// 回归纯结构校验（green006曾放构造器导致2219压平2228，green007纠正）。
    pub(in crate::session) fn validate_financial_birth(&self) -> Result<(), StepFatal> {
        if self.audit.filled_value == Money::ZERO
            && (self.audit.nominal != FeeComponents::ZERO || self.audit.charged != FeeComponents::ZERO) {
            return Err(StepFatal::InvariantViolation {
                description: "live-order nominal fee audit disagrees with cumulative filled value".to_owned(),
                location: "GameSession::project_live_envelopes".to_owned(),
            });
        }
        Ok(())
    }

    fn verified_live_at(&self, config: &crate::GameConfig, site: FeeCheckSite) -> Result<ResVec, StepFatal> {
        self.validate()?;
        self.validate_financial_birth()?;
        let inputs = FeeInputs::capture(config);
        let result = self.financial_runtime.proof.get_or_init(|| {
            config.validate().map_err(|error| super::ledger_validation::invariant(&error.to_string()))?;
            let nominal = crate::session::envelope_projection::cumulative_nominal_fees(
                config, self.key.side, self.audit.filled_value)?;
            if self.audit.nominal != nominal {
                return Err(StepFatal::InvariantViolation {
                    description: "live-order nominal fee audit disagrees with cumulative filled value".to_owned(),
                    location: "GameSession::project_live_envelopes".to_owned(),
                }.into());
            }
            crate::session::envelope_projection::validate_hydrated_charged_audit(self.key.side, self.audit)?;
            // terminal Release可保留尚未成交qty，资源已经为零；活动行则必须精确匹配预留。
            if self.live != ResVec::ZERO {
                let expected = match self.key.side {
                    crate::Side::Buy => ResVec::new(crate::config::buy_order_reservation(
                        config, self.audit.limit, self.audit.remaining_qty, self.audit.filled_value)
                        .map_err(|error| super::ledger_validation::invariant(&error.to_string()))?, 0),
                    crate::Side::Sell => ResVec::new(Money::ZERO, self.audit.remaining_qty),
                };
                if self.live != expected {
                    return Err(FeeFailure::LiveResources);
                }
            }
            Ok(FeeProof { inputs, content_revision: self.financial_runtime.content_revision, calculator: config.clone() })
        });
        let proof = result.as_ref().map_err(|failure| match failure {
            FeeFailure::Fatal(error) => error.clone(),
            FeeFailure::LiveResources => {
                let (description, location) = match site {
                    FeeCheckSite::Generic => ("live envelope projection disagrees with ledger", "pipeline::EnvelopeLedger"),
                    FeeCheckSite::BookCapture => ("continuous order books disagree with live envelope resources", "pipeline::continuous_matching_adapter"),
                    FeeCheckSite::StockRound => (match self.key.side {
                        crate::Side::Buy => "resting buy envelope disagrees with its exact reservation",
                        crate::Side::Sell => "resting sell envelope disagrees with its quantity",
                    }, "pipeline::continuous_matching"),
                };
                StepFatal::InvariantViolation { description: description.to_owned(), location: location.to_owned() }
            }
        })?;
        if proof.inputs != inputs || proof.content_revision != self.financial_runtime.content_revision {
            return Err(super::ledger_validation::invariant("订单财务证明的费用输入或内容版本不一致"));
        }
        Ok(self.live)
    }

    /// 只重置已校验 private candidate 的 tick 局部资源，不复制键或累计费用。
    /// key/side/limit/remaining_qty/filled_qty/filled_value/nominal/charged/live/pending不变；
    /// basis/origin/spent/released不参与费用公式，并仍由validate逐次检查，不能洗白错audit。
    pub(super) fn rebase_for_next_tick(&mut self) -> Result<(), StepFatal> {
        if self.pending_price.is_some() {
            return Err(super::ledger_validation::invariant(
                "live envelope reached tick commit with an unresolved price",
            ));
        }
        self.origin = EnvelopeOrigin::TickStart;
        self.basis = self.live;
        self.spent = ResVec::ZERO;
        self.released = ResVec::ZERO;
        self.validate()
    }

    pub fn apply(
        &mut self,
        delta: ReceiptDelta,
        audit_after: EnvelopeAudit,
        resolved_price: bool,
    ) -> Result<(), StepFatal> {
        let mut candidate = self.clone();
        let consumed = delta.spent.checked_add(delta.released)?;
        let expected = candidate.live.checked_sub(consumed)?;
        if expected != delta.live_after {
            return Err(StepFatal::InvariantViolation {
                description: format!("envelope {:?} chain mismatch", candidate.key),
                location: "pipeline::Envelope::apply".to_owned(),
            });
        }
        candidate.spent = candidate.spent.checked_add(delta.spent)?;
        candidate.released = candidate.released.checked_add(delta.released)?;
        candidate.live = delta.live_after;
        candidate.audit = audit_after;
        if resolved_price {
            if candidate.pending_price.is_none() {
                return Err(StepFatal::InvariantViolation {
                    description: format!("envelope {:?} has no pending price", candidate.key),
                    location: "pipeline::Envelope::apply".to_owned(),
                });
            }
            candidate.pending_price = None;
        }
        candidate.validate()?;
        if candidate.audit != self.audit || candidate.live != self.live || candidate.pending_price != self.pending_price {
            candidate.financial_runtime.content_revision = self.financial_runtime.content_revision.checked_add(1)
                .ok_or_else(|| super::ledger_validation::invariant("订单财务内容版本耗尽"))?;
            candidate.financial_runtime.proof = std::sync::Arc::default();
            if let Some(Ok(proof)) = self.financial_runtime.proof.get() {
                // 旧行有真实proof时，checked receipt必须用同一上下文验证新财务事实，
                // 成功才安装；失败仍保留原行。未验证raw行不能据此继承旧凭证。
                candidate.verified_live(&proof.calculator)?;
            }
        }
        *self = candidate;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), StepFatal> {
        if self.pending_price.is_some() && self.origin != EnvelopeOrigin::CreatedAtValidation {
            return Err(StepFatal::InvariantViolation {
                description: format!(
                    "envelope {:?} 在 AccountValidation 之外仍含有 pending price",
                    self.key
                ),
                location: "pipeline::Envelope::validate".to_owned(),
            });
        }
        self.basis.validate_nonnegative()?;
        self.spent.validate_nonnegative()?;
        self.released.validate_nonnegative()?;
        self.live.validate_nonnegative()?;
        let used = self
            .spent
            .checked_add(self.released)?
            .checked_add(self.live)?;
        if used != self.basis {
            return Err(StepFatal::InvariantViolation {
                description: format!("envelope {:?} conservation mismatch", self.key),
                location: "pipeline::Envelope::validate".to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod borrowed_projection_tests {
    use super::*;
    use crate::{AccountId, OrderId, Side, StockId};

    #[test]
    fn borrowed_projection_matches_exact_owned_tick_start_payload_including_every_field() {
        // 数值身份后无字符串可断言：纯 fixture 单股按目录 rank 取 StockId(0)。
        let stock = StockId(0);
        let projection = BorrowedTickStartEnvelope {
            account: AccountId(1),
            stock: &stock,
            order: OrderId(9),
            side: Side::Buy,
            live: ResVec::new(Money::from_cents(100), 0),
            audit: EnvelopeAudit {
                limit: Money::from_cents(1),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        };
        let expected = Envelope::tick_start_existing(
            EnvelopeKey {
                account: projection.account,
                stock: stock.clone(),
                order: projection.order,
                side: projection.side,
            },
            projection.live.cash,
            projection.live.shares,
            projection.audit,
        );
        let mut cases = vec![expected.clone()];
        macro_rules! changed {
            ($field:ident $(.$nested:ident)*, $value:expr) => {{
                let mut candidate = expected.clone();
                candidate.$field$(.$nested)* = $value;
                cases.push(candidate);
            }};
        }
        changed!(key.account, AccountId(2));
        changed!(key.stock, StockId(1));
        changed!(key.order, OrderId(10));
        changed!(key.side, Side::Sell);
        changed!(origin, EnvelopeOrigin::CreatedAtValidation);
        changed!(basis.cash, Money::from_cents(101));
        changed!(basis.shares, 1);
        changed!(live.cash, Money::from_cents(99));
        changed!(live.shares, 1);
        changed!(spent.cash, Money::from_cents(1));
        changed!(spent.shares, 1);
        changed!(released.cash, Money::from_cents(1));
        changed!(released.shares, 1);
        changed!(audit.limit, Money::from_cents(2));
        changed!(audit.remaining_qty, 99);
        changed!(audit.filled_qty, 1);
        changed!(audit.filled_value, Money::from_cents(1));
        changed!(audit.nominal.commission, Money::from_cents(1));
        changed!(audit.nominal.stamp_tax, Money::from_cents(1));
        changed!(audit.nominal.transfer_fee, Money::from_cents(1));
        changed!(audit.charged.commission, Money::from_cents(1));
        changed!(audit.charged.stamp_tax, Money::from_cents(1));
        changed!(audit.charged.transfer_fee, Money::from_cents(1));
        changed!(pending_price, Some(crate::LimitPrice::Highest));
        for candidate in cases {
            assert_eq!(
                projection.matches(&candidate),
                candidate == expected,
                "{candidate:?}"
            );
        }
    }
}
