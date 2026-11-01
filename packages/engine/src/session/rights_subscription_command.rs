//! 玩家显式配股认购受理；成功仅表示申请进入当日队列，不表示已缴款或取得股份。

use super::{corporate_actions::QueuedRightsSubscription, GameSession};
use crate::{AccountId, CivilDate};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RightsSubscriptionReceipt {
    pub event_id: String,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    #[ts(type = "string")]
    pub requested_shares: u64,
    pub submitted_on: CivilDate,
}

#[derive(Debug, thiserror::Error)]
pub enum RightsSubscriptionError {
    #[error("本局未启用配股／增发机制（rights_offering_enabled=false）")]
    Disabled,
    #[error("当前 runtime authority 不支持配股认购；需要真实账户结算路径")]
    UnsupportedRuntime,
    #[error("认购股数必须为正数")]
    InvalidQuantity,
    #[error("认购账户 {account:?} 不存在")]
    UnknownAccount { account: AccountId },
    #[error("当前会话没有可写入的公司系统或公司行为状态")]
    MissingCompanyState,
    #[error("未找到未结配股方案 {event_id}")]
    UnknownEvent { event_id: String },
    #[error("配股方案 {event_id} 不支持玩家认购：仅支持面向全体股东的受限路径")]
    UnsupportedPlan { event_id: String },
    #[error("配股方案 {event_id} 当前不在缴款窗口或权证尚未派发")]
    OutsidePaymentWindow { event_id: String },
    #[error("账户 {account:?} 在配股方案 {event_id} 中没有可认购权利")]
    NoEntitlement {
        event_id: String,
        account: AccountId,
    },
    #[error("请求股数 {requested} 超过账户 {account:?} 的剩余配股权 {available}")]
    ExceedsEntitlement {
        requested: u64,
        available: u64,
        account: AccountId,
    },
    #[error("账户 {account:?} 已为配股方案 {event_id} 提交认购")]
    Duplicate {
        event_id: String,
        account: AccountId,
    },
    #[error("会话健康门失败：{0}")]
    Step(#[from] super::StepFatal),
    #[error("配股方案能力校验失败：{source}")]
    Capability {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("当前税务模式不能完成该配股结算")]
    UnsupportedTaxMode,
    #[error("发行人回购专户不能作为配股认购账户")]
    UnsupportedAccountKind,
    #[error("配股方案 {event_id} 的股份流通或除权组合不满足当前支持范围")]
    UnsupportedSettlementScope { event_id: String },
    #[error("外部修订序号溢出")]
    RevisionExhausted,
}

impl GameSession {
    /// 受理玩家认购意向并加入当前自然日队列；不预扣现金。
    pub fn subscribe_rights_offering(
        &mut self,
        owner: AccountId,
        event_id: &str,
        requested_shares: u64,
    ) -> Result<RightsSubscriptionReceipt, RightsSubscriptionError> {
        self.require_healthy()?;
        if !self.state.setup.rights_offering_enabled {
            return Err(RightsSubscriptionError::Disabled);
        }
        if self.has_current_external_runtime_authority()
            || (cfg!(target_arch = "wasm32") && self.uses_runtime_civil_authority())
        {
            return Err(RightsSubscriptionError::UnsupportedRuntime);
        }
        if requested_shares == 0 {
            return Err(RightsSubscriptionError::InvalidQuantity);
        }
        let account = self
            .state
            .accounts
            .get(&owner)
            .ok_or(RightsSubscriptionError::UnknownAccount { account: owner })?;
        if account.kind() == crate::AccountKind::IssuerRepurchase {
            return Err(RightsSubscriptionError::UnsupportedAccountKind);
        }
        if !matches!(
            self.state.setup.dividend_tax_mode,
            Some(
                crate::company::CashDividendTaxMode::Exempt
                    | crate::company::CashDividendTaxMode::FlatWithholding
            )
        ) {
            return Err(RightsSubscriptionError::UnsupportedTaxMode);
        }
        let next_revision = self
            .external_revision()
            .checked_add(1)
            .ok_or(RightsSubscriptionError::RevisionExhausted)?;
        let day = self.civil_date();
        let actions = self
            .state
            .companies
            .corporate_actions_mut()
            .ok_or(RightsSubscriptionError::MissingCompanyState)?;
        let book_index = actions
            .rights_offerings
            .iter()
            .position(|book| {
                book.plan().event_id == event_id
                    && book.status()
                        != &crate::company::rights_offering::RightsOfferingStatus::Settled
            })
            .ok_or_else(|| RightsSubscriptionError::UnknownEvent {
                event_id: event_id.to_owned(),
            })?;
        let book = &actions.rights_offerings[book_index];
        if !actions.dividend_tax_books.is_empty() {
            return Err(RightsSubscriptionError::UnsupportedSettlementScope {
                event_id: event_id.to_owned(),
            });
        }
        if !matches!(
            &book.plan().mode,
            crate::company::rights_offering::RightsOfferingMode::RightsToAllShareholders { .. }
        ) {
            return Err(RightsSubscriptionError::UnsupportedPlan {
                event_id: event_id.to_owned(),
            });
        }
        super::corporate_actions::rights_offering_day_end::validate_rights_offering_capability(
            book.plan(),
        )
        .map_err(|source| RightsSubscriptionError::Capability {
            source: Box::new(source),
        })?;
        if !book.plan().payment_window_contains(day)
            || book.status() != &crate::company::rights_offering::RightsOfferingStatus::Entitled
        {
            return Err(RightsSubscriptionError::OutsidePaymentWindow {
                event_id: event_id.to_owned(),
            });
        }
        if actions
            .rights_subscription_queue
            .iter()
            .any(|row| row.event_id == event_id && row.account == owner)
            || book
                .subscriptions()
                .iter()
                .any(|row| row.holder == crate::company::share_registry::HolderId::Account(owner))
        {
            return Err(RightsSubscriptionError::Duplicate {
                event_id: event_id.to_owned(),
                account: owner,
            });
        }
        let snapshot =
            book.registration()
                .ok_or_else(|| RightsSubscriptionError::OutsidePaymentWindow {
                    event_id: event_id.to_owned(),
                })?;
        if !super::civil_rights_ex::all_shares_circulating(snapshot)
            || actions
                .rights_offerings
                .iter()
                .enumerate()
                .any(|(index, other)| {
                    index != book_index
                        && other.plan().stock == book.plan().stock
                        && other.plan().ex_rights_on == book.plan().ex_rights_on
                })
            || actions.dividends.iter().any(|other| {
                other.plan().stock == book.plan().stock
                    && other.plan().ex_dividend_on == book.plan().ex_rights_on
            })
            || actions.stock_distributions.iter().any(|other| {
                other.plan().stock == book.plan().stock
                    && other.plan().ex_rights_on == book.plan().ex_rights_on
            })
            || actions.share_splits.iter().any(|other| {
                other.plan().stock == book.plan().stock
                    && other.plan().ex_rights_on == book.plan().ex_rights_on
            })
            || actions.applied_ex_reference_groups.iter().any(|other| {
                other.stock == book.plan().stock && other.date == book.plan().ex_rights_on
            })
        {
            return Err(RightsSubscriptionError::UnsupportedSettlementScope {
                event_id: event_id.to_owned(),
            });
        }
        let available = book
            .entitlement()
            .and_then(|receipt| {
                receipt.entitlements.iter().find(|row| {
                    row.holder == crate::company::share_registry::HolderId::Account(owner)
                })
            })
            .map(|row| row.rights_shares)
            .ok_or_else(|| RightsSubscriptionError::NoEntitlement {
                event_id: event_id.to_owned(),
                account: owner,
            })?;
        if requested_shares > available {
            return Err(RightsSubscriptionError::ExceedsEntitlement {
                requested: requested_shares,
                available,
                account: owner,
            });
        }
        actions
            .rights_subscription_queue
            .push(QueuedRightsSubscription {
                event_id: event_id.to_owned(),
                requested_shares,
                submitted_on: day,
                account: owner,
            });
        self.revision
            .store(next_revision, std::sync::atomic::Ordering::Relaxed);
        Ok(RightsSubscriptionReceipt {
            event_id: event_id.to_owned(),
            requested_shares,
            submitted_on: day,
        })
    }
}
