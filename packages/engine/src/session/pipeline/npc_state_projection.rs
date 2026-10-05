//! 将 NPC 决策结果投影到可丢弃 tick candidate。
//!
//! 工作单 reconciliation 决策先于 residual intents 路由，二者保持分离。
//! candidate composer 在投影后为每个账户分配连续局部序号，
//! 保留 reconciliation 在 residual 之前的顺序，不重用 raw strategy 身份。

use super::decision_snapshot_capture::CapturedDecisionSnapshot;
use super::npc_decisions::{NpcDecisionSourceOutput, NpcStrategyUpdate};
use super::IntentCandidateKey;
use crate::session::execution::reconcile_plan::WorkingOrderDecision;
use crate::session::{GameSession, ReconcileScope, RetailDecisionTrace, WorkingOrderSlices};
use crate::strategy::Intent;
use crate::{AccountId, AccountKind, LimitPrice, OrderId, StockCode};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub(in crate::session) struct ProjectedNpcIntent {
    account: AccountId,
    source_key: Option<IntentCandidateKey>,
    #[cfg(test)]
    source_intent: Option<Intent>,
    projected_intent: Intent,
}

impl ProjectedNpcIntent {
    pub(in crate::session) const fn account(&self) -> AccountId {
        self.account
    }

    pub(in crate::session) const fn source_key(&self) -> Option<&IntentCandidateKey> {
        self.source_key.as_ref()
    }

    #[cfg(test)]
    pub(in crate::session) const fn source_intent(&self) -> Option<&Intent> {
        self.source_intent.as_ref()
    }

    pub(in crate::session) const fn projected_intent(&self) -> &Intent {
        &self.projected_intent
    }
}

#[derive(Clone, Debug)]
pub(in crate::session) enum NpcReconciliationDecision {
    Keep {
        #[cfg(test)]
        account: AccountId,
        #[cfg(test)]
        order_id: OrderId,
    },
    Cancel {
        account: AccountId,
        order_id: OrderId,
        code: StockCode,
    },
    Replace {
        account: AccountId,
        old_order_id: OrderId,
        new_intent: Intent,
    },
}

impl NpcReconciliationDecision {
    #[cfg(test)]
    pub(in crate::session) const fn contract_parts(
        &self,
    ) -> (AccountId, OrderId, Option<&StockCode>, Option<&Intent>) {
        match self {
            Self::Keep { account, order_id } => (*account, *order_id, None, None),
            Self::Cancel {
                account,
                order_id,
                code,
            } => (*account, *order_id, Some(code), None),
            Self::Replace {
                account,
                old_order_id,
                new_intent,
            } => (*account, *old_order_id, None, Some(new_intent)),
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionProjectionOutput {
    #[cfg(test)]
    accepted_due_npc_ids: Vec<AccountId>,
    reconciliation_decisions: Vec<NpcReconciliationDecision>,
    residual_intents: Vec<ProjectedNpcIntent>,
}

impl NpcDecisionProjectionOutput {
    #[cfg(test)]
    pub(in crate::session) fn accepted_due_npc_ids(&self) -> &[AccountId] {
        &self.accepted_due_npc_ids
    }

    /// Must be consumed before [`Self::residual_intents`].
    pub(in crate::session) fn reconciliation_decisions(&self) -> &[NpcReconciliationDecision] {
        &self.reconciliation_decisions
    }

    pub(in crate::session) fn residual_intents(&self) -> &[ProjectedNpcIntent] {
        &self.residual_intents
    }
}

#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum NpcDecisionProjectionError {
    #[error(
        "DecisionShadow NPC 投影时钟不一致：shadow tick/phase {shadow_tick}/{shadow_phase:?}，snapshot {snapshot_tick}/{snapshot_phase:?}"
    )]
    ClockMismatch {
        shadow_tick: u64,
        shadow_phase: crate::TradingPhase,
        snapshot_tick: u64,
        snapshot_phase: crate::TradingPhase,
    },
    #[error("DecisionShadow NPC 投影来源账户或 intent 顺序与密封快照不一致")]
    AccountOrderMismatch,
    #[error("DecisionShadow NPC 投影缺少 shadow 账户 {0:?}")]
    MissingAccount(AccountId),
    #[error("DecisionShadow NPC 投影账户 {0:?} 的 kind 在捕获后发生变化")]
    AccountKindMismatch(AccountId),
    #[error("DecisionShadow NPC 投影账户 {0:?} 缺少 retail experience state")]
    MissingRetailExperience(AccountId),
    #[error("DecisionShadow NPC 投影账户 {account:?} 的 parent-order 状态无效：{reason}")]
    InvalidParentOrder { account: AccountId, reason: String },
    #[error("DecisionShadow NPC 投影账户 {0:?} 的 parent-order horizon 溢出")]
    ParentOrderHorizonOverflow(AccountId),
    #[error("DecisionShadow NPC 投影已移交账户 {0:?} 的 strategy state")]
    ConsumedStrategyState(AccountId),
}

/// 把纯 NPC 决策结果投影到调用方可丢弃的 tick shadow。
/// 失败时调用方必须丢弃整个 tick candidate，不能继续使用部分投影。
#[cfg(test)]
pub(in crate::session) fn project_npc_state(
    shadow: &mut GameSession,
    captured: &CapturedDecisionSnapshot,
    source: &mut NpcDecisionSourceOutput,
) -> Result<NpcDecisionProjectionOutput, NpcDecisionProjectionError> {
    let snapshot = &captured.snapshot;
    if shadow.state.tick != snapshot.tick() || shadow.phase() != snapshot.phase() {
        return Err(NpcDecisionProjectionError::ClockMismatch {
            shadow_tick: shadow.state.tick,
            shadow_phase: shadow.phase(),
            snapshot_tick: snapshot.tick(),
            snapshot_phase: snapshot.phase(),
        });
    }
    if snapshot.due_npc_ids() != source.accounts()
        || source
            .account_outputs()
            .iter()
            .map(|output| output.account())
            .ne(source.accounts().iter().copied())
    {
        return Err(NpcDecisionProjectionError::AccountOrderMismatch);
    }

    project_npc_state_in_place(shadow, captured, source, false)
}

/// 将一个已完成的 NPC 账户结果投影到可丢弃 tick shadow。
/// 调用方拥有账户完成 channel；任一账户投影失败都必须丢弃整个 candidate。
pub(in crate::session) fn project_npc_account(
    shadow: &mut GameSession,
    captured: &CapturedDecisionSnapshot,
    source: &mut NpcDecisionSourceOutput,
) -> Result<NpcDecisionProjectionOutput, NpcDecisionProjectionError> {
    let snapshot = &captured.snapshot;
    if shadow.state.tick != snapshot.tick() || shadow.phase() != snapshot.phase() {
        return Err(NpcDecisionProjectionError::ClockMismatch {
            shadow_tick: shadow.state.tick,
            shadow_phase: shadow.phase(),
            snapshot_tick: snapshot.tick(),
            snapshot_phase: snapshot.phase(),
        });
    }
    if source.accounts().len() != 1
        || source.account_outputs().len() != 1
        || source.accounts()[0] != source.account_outputs()[0].account()
        || snapshot
            .due_npc_ids()
            .binary_search(&source.accounts()[0])
            .is_err()
    {
        return Err(NpcDecisionProjectionError::AccountOrderMismatch);
    }
    project_npc_state_in_place(shadow, captured, source, true)
}

fn project_npc_state_in_place(
    shadow: &mut GameSession,
    captured: &CapturedDecisionSnapshot,
    source: &mut NpcDecisionSourceOutput,
    project_single_account: bool,
) -> Result<NpcDecisionProjectionOutput, NpcDecisionProjectionError> {
    let snapshot = &captured.snapshot;
    let phase = snapshot.phase();
    let market_minute = snapshot.market_minute();
    let working_continuous = &captured.working_continuous;
    let working_auction = &captured.working_auction;
    let mut decisions = Vec::new();
    let mut residual = Vec::new();
    let mut retail_reviews = Vec::new();
    let (account_outputs, raw_intents) = source.projection_parts();
    let mut source_intents = raw_intents.iter().peekable();
    for account_output in account_outputs {
        let account = account_output.account();
        let sealed = snapshot
            .account(account)
            .map_err(|_| NpcDecisionProjectionError::MissingAccount(account))?;
        let entry = shadow
            .state
            .accounts
            .get(&account)
            .ok_or(NpcDecisionProjectionError::MissingAccount(account))?;
        let account_kind = entry.kind();
        if account_kind != sealed.kind() {
            return Err(NpcDecisionProjectionError::AccountKindMismatch(account));
        }
        match account_output
            .take_strategy()
            .ok_or(NpcDecisionProjectionError::ConsumedStrategyState(account))?
        {
            NpcStrategyUpdate::Unchanged => {}
            NpcStrategyUpdate::Replace(next_strategy) => {
                shadow
                    .state
                    .accounts
                    .get_mut(&account)
                    .expect("the account was checked above")
                    .restore_strategy(Some(next_strategy));
            }
        }
        if account_kind == AccountKind::Retail {
            if let Some(position_decision) = account_output.position_decision() {
                shadow
                    .state
                    .last_retail_decisions
                    .push(RetailDecisionTrace {
                        account,
                        decision: position_decision.clone(),
                        execution_urgency: account_output.execution_urgency().clone(),
                    });
            }
            retail_reviews.push((account, account_output.reviewed_stocks().clone()));
        }
        let mut raw = Vec::new();
        while matches!(
            source_intents.peek().map(|intent| intent.key()),
            Some(IntentCandidateKey::Npc { account: owner, .. }) if *owner == account
        ) {
            let intent = source_intents
                .next()
                .expect("peeked source intent is present");
            raw.push((intent.key().clone(), intent.intent().clone()));
        }
        let mut desired: Vec<_> = raw.iter().map(|(_, intent)| intent.clone()).collect();
        let mut unused_raw = raw;
        let working = WorkingOrderSlices {
            continuous: working_continuous
                .get(&account)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            auction: working_auction
                .get(&account)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        };
        if account_output.uses_parent_order_execution() && account_kind == AccountKind::Inst {
            validate_parent_order_materialization(shadow, account, &desired, market_minute)?;
            desired =
                shadow.materialize_parent_order_intents(account, desired, market_minute, working);
        }
        if account_output.updates_working_quotes() {
            let scope = if account_kind == AccountKind::Retail {
                ReconcileScope::ReviewedStocks(account_output.reviewed_stocks().clone())
            } else {
                ReconcileScope::AllWorkingOrders
            };
            let plan = shadow.plan_npc_working_order_reconciliation(desired, phase, scope, working);
            for decision in &plan.decisions {
                let WorkingOrderDecision::Keep { order_id } = decision else {
                    continue;
                };
                if let Some(kept) = working_intent(*order_id, phase, working) {
                    remove_first_matching_raw(&mut unused_raw, &kept);
                }
            }
            decisions.extend(
                plan.decisions
                    .into_iter()
                    .map(|decision| project_reconciliation_decision(account, decision)),
            );
            desired = plan.residual_intents;
        }

        for projected_intent in desired {
            let source = take_exact_raw_source(&mut unused_raw, &projected_intent);
            residual.push(ProjectedNpcIntent {
                account,
                source_key: source.as_ref().map(|(key, _)| key.clone()),
                #[cfg(test)]
                source_intent: source.map(|(_, intent)| intent),
                projected_intent,
            });
        }
    }
    if source_intents.next().is_some() {
        return Err(NpcDecisionProjectionError::AccountOrderMismatch);
    }
    if project_single_account {
        for (account, reviewed) in retail_reviews {
            let held: BTreeSet<_> = shadow.state.accounts[&account]
                .positions()
                .keys()
                .cloned()
                .collect();
            let experience = shadow
                .state
                .retail_experience
                .get_mut(&account)
                .ok_or(NpcDecisionProjectionError::MissingRetailExperience(account))?;
            for code in &reviewed {
                experience.observe_stock(code, market_minute);
            }
            experience.prune_watchlist(&held);
        }
    } else {
        let review_ids = retail_reviews
            .iter()
            .map(|(account, _)| *account)
            .collect::<Vec<_>>();
        let reviews = retail_reviews
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        let accounts = &shadow.state.accounts;
        shadow.state.retail_experience.mutate_existing_parallel(
            &review_ids,
            NpcDecisionProjectionError::MissingRetailExperience,
            |account, experience| {
                for code in &reviews[&account] {
                    experience.observe_stock(code, market_minute);
                }
                let held: BTreeSet<_> = accounts[&account].positions().keys().cloned().collect();
                experience.prune_watchlist(&held);
                Ok(())
            },
        )?;
    }
    Ok(NpcDecisionProjectionOutput {
        #[cfg(test)]
        accepted_due_npc_ids: source.accounts().to_vec(),
        reconciliation_decisions: decisions,
        residual_intents: residual,
    })
}

fn working_intent(
    order_id: OrderId,
    phase: crate::TradingPhase,
    working: WorkingOrderSlices<'_>,
) -> Option<Intent> {
    match phase {
        crate::TradingPhase::Continuous => working.continuous.iter().find_map(|(code, order)| {
            (order.id == order_id).then(|| Intent::PlaceLimit {
                code: code.clone(),
                side: order.side,
                price: LimitPrice::Fixed(order.price),
                qty: order.qty,
            })
        }),
        crate::TradingPhase::CallAuction => working.auction.iter().find_map(|(code, order)| {
            (OrderId(order.order_id) == order_id).then(|| Intent::PlaceLimit {
                code: code.clone(),
                side: order.side,
                price: LimitPrice::Fixed(order.limit),
                qty: order.qty,
            })
        }),
        crate::TradingPhase::ClosingAuction | crate::TradingPhase::PreOpen => None,
    }
}

pub(super) fn remove_first_matching_raw(
    raw: &mut Vec<(IntentCandidateKey, Intent)>,
    consumed: &Intent,
) {
    if let Some(index) = raw
        .iter()
        .position(|(_, intent)| intents_equal(intent, consumed))
    {
        raw.remove(index);
    }
}

pub(super) fn take_exact_raw_source(
    raw: &mut Vec<(IntentCandidateKey, Intent)>,
    projected: &Intent,
) -> Option<(IntentCandidateKey, Intent)> {
    raw.iter()
        .position(|(_, intent)| intents_equal(intent, projected))
        .map(|index| raw.remove(index))
}

fn validate_parent_order_materialization(
    session: &GameSession,
    account: AccountId,
    desired: &[Intent],
    market_minute: u64,
) -> Result<(), NpcDecisionProjectionError> {
    if desired
        .iter()
        .any(|intent| matches!(intent, Intent::PlaceLimit { qty, .. } if *qty > 0 && qty.is_multiple_of(session.state.setup.config.lot_size)))
        && market_minute
            .checked_add(crate::session::PARENT_ORDER_HORIZON_MINUTES)
            .is_none()
    {
        return Err(NpcDecisionProjectionError::ParentOrderHorizonOverflow(account));
    }
    for (code, plan) in session
        .state
        .parent_orders
        .get(&account)
        .into_iter()
        .flatten()
    {
        if plan.code() != code {
            return Err(NpcDecisionProjectionError::InvalidParentOrder {
                account,
                reason: format!(
                    "map key {} does not match plan code {}",
                    code.0,
                    plan.code().0
                ),
            });
        }
        let remaining = plan
            .target_qty()
            .checked_sub(plan.filled_qty())
            .ok_or_else(|| NpcDecisionProjectionError::InvalidParentOrder {
                account,
                reason: format!(
                    "{} filled quantity {} exceeds target {}",
                    code.0,
                    plan.filled_qty(),
                    plan.target_qty()
                ),
            })?;
        match (
            plan.active_child_order_id(),
            plan.active_child_remaining_qty(),
        ) {
            (None, None) => {}
            (Some(_), Some(quantity)) if quantity > 0 && quantity <= remaining => {}
            _ => {
                return Err(NpcDecisionProjectionError::InvalidParentOrder {
                    account,
                    reason: format!("{} has inconsistent active-child identity/quantity", code.0),
                })
            }
        }
    }
    Ok(())
}

fn project_reconciliation_decision(
    account: AccountId,
    decision: WorkingOrderDecision,
) -> NpcReconciliationDecision {
    match decision {
        WorkingOrderDecision::Keep {
            #[cfg(test)]
            order_id,
            #[cfg(not(test))]
                order_id: _,
        } => NpcReconciliationDecision::Keep {
            #[cfg(test)]
            account,
            #[cfg(test)]
            order_id,
        },
        WorkingOrderDecision::Cancel { order_id, code } => NpcReconciliationDecision::Cancel {
            account,
            order_id,
            code,
        },
        WorkingOrderDecision::Replace {
            old_order_id,
            new_intent,
        } => NpcReconciliationDecision::Replace {
            account,
            old_order_id,
            new_intent,
        },
    }
}

fn intents_equal(left: &Intent, right: &Intent) -> bool {
    match (left, right) {
        (
            Intent::PlaceLimit {
                code: left_code,
                side: left_side,
                price: left_price,
                qty: left_qty,
            },
            Intent::PlaceLimit {
                code: right_code,
                side: right_side,
                price: right_price,
                qty: right_qty,
            },
        ) => {
            left_code == right_code
                && left_side == right_side
                && left_price == right_price
                && left_qty == right_qty
        }
        (
            Intent::PlaceMarket {
                code: left_code,
                side: left_side,
                qty: left_qty,
            },
            Intent::PlaceMarket {
                code: right_code,
                side: right_side,
                qty: right_qty,
            },
        ) => left_code == right_code && left_side == right_side && left_qty == right_qty,
        (
            Intent::Cancel {
                code: left_code,
                id: left_id,
            },
            Intent::Cancel {
                code: right_code,
                id: right_id,
            },
        ) => left_code == right_code && left_id == right_id,
        _ => false,
    }
}

#[cfg(test)]
mod auction_identity_tests {
    use super::*;
    use crate::session::AuctionOrderSnap;
    use crate::TradingPhase;
    use crate::{Money, Side};

    #[test]
    fn kept_auction_order_consumes_its_raw_quote_by_order_id() {
        let account = AccountId(1);
        let code = StockCode("600001".to_owned());
        let auction = [(
            code.clone(),
            AuctionOrderSnap {
                owner: account,
                side: Side::Buy,
                limit: Money::from_cents(1_000),
                qty: 100,
                order_id: 77,
            },
        )];
        let working = || WorkingOrderSlices {
            continuous: &[],
            auction: &auction,
        };
        let kept = working_intent(OrderId(77), TradingPhase::CallAuction, working())
            .expect("the queued order ID must find the matching quote");
        assert!(working_intent(OrderId(0), TradingPhase::CallAuction, working()).is_none());
        let mut raw = vec![(
            IntentCandidateKey::npc(account, 0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )];

        remove_first_matching_raw(&mut raw, &kept);

        assert!(raw.is_empty(), "a kept quote cannot be submitted again");
    }
}
