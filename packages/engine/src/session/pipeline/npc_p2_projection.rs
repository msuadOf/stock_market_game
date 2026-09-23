//! Atomic shadow projection for the pure NPC P2 source.
//!
//! Reconciliation decisions deliberately remain separate from residual intents:
//! legacy executes working-order decisions first, then routes residual intents.
//! P2 cannot assign synthetic cancellation keys without changing that ordering.

use super::npc_p2_source::NpcP2SourceOutput;
use super::{DecisionResourceSnapshot, DecisionSnapshot, P2CandidateKey};
use crate::account::StoredStrategy;
use crate::session::execution::reconcile_plan::WorkingOrderDecision;
use crate::session::{GameSession, ReconcileScope, RetailDecisionTrace, WorkingOrderSlices};
use crate::strategy::{Intent, StrategyStateError};
use crate::{AccountId, AccountKind, Money, MoneyError, OrderId, Side, StockCode};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(in crate::session) struct ProjectedNpcIntent {
    account: AccountId,
    source_key: Option<P2CandidateKey>,
    source_intent: Option<Intent>,
    projected_intent: Intent,
}

impl ProjectedNpcIntent {
    pub(in crate::session) const fn account(&self) -> AccountId {
        self.account
    }

    pub(in crate::session) const fn source_key(&self) -> Option<&P2CandidateKey> {
        self.source_key.as_ref()
    }

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
        account: AccountId,
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
pub(in crate::session) enum NpcCashCapChange {
    Resized {
        account: AccountId,
        source_key: Option<P2CandidateKey>,
        requested_qty: u32,
        projected_qty: u32,
    },
    Dropped {
        account: AccountId,
        source_key: Option<P2CandidateKey>,
        requested: Intent,
    },
}

impl NpcCashCapChange {
    pub(in crate::session) const fn source_key(&self) -> Option<&P2CandidateKey> {
        match self {
            Self::Resized { source_key, .. } | Self::Dropped { source_key, .. } => {
                source_key.as_ref()
            }
        }
    }

    pub(in crate::session) const fn is_resized(&self) -> bool {
        matches!(self, Self::Resized { .. })
    }

    pub(in crate::session) const fn contract_parts(
        &self,
    ) -> (AccountId, Option<u32>, Option<u32>, Option<&Intent>) {
        match self {
            Self::Resized {
                account,
                requested_qty,
                projected_qty,
                ..
            } => (*account, Some(*requested_qty), Some(*projected_qty), None),
            Self::Dropped {
                account, requested, ..
            } => (*account, None, None, Some(requested)),
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::session) struct NpcP2ProjectionOutput {
    accepted_due_npc_ids: Vec<AccountId>,
    reconciliation_decisions: Vec<NpcReconciliationDecision>,
    residual_intents: Vec<ProjectedNpcIntent>,
    cash_cap_changes: Vec<NpcCashCapChange>,
}

impl NpcP2ProjectionOutput {
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

    pub(in crate::session) fn cash_cap_changes(&self) -> &[NpcCashCapChange] {
        &self.cash_cap_changes
    }
}

#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum NpcP2ProjectionError {
    #[error(
        "P2 NPC projection clock mismatch: shadow tick/phase {shadow_tick}/{shadow_phase:?}, snapshot {snapshot_tick}/{snapshot_phase:?}"
    )]
    ClockMismatch {
        shadow_tick: u64,
        shadow_phase: crate::TradingPhase,
        snapshot_tick: u64,
        snapshot_phase: crate::TradingPhase,
    },
    #[error("P2 NPC projection source accounts do not exactly match the sealed snapshot")]
    AccountOrderMismatch,
    #[error("P2 NPC projection is missing shadow account {0:?}")]
    MissingAccount(AccountId),
    #[error("P2 NPC projection account {0:?} changed kind after capture")]
    AccountKindMismatch(AccountId),
    #[error("P2 NPC projection account {0:?} has no retail experience state")]
    MissingRetailExperience(AccountId),
    #[error("P2 NPC projection parent-order state is invalid for account {account:?}: {reason}")]
    InvalidParentOrder { account: AccountId, reason: String },
    #[error("P2 NPC projection parent-order horizon overflows for account {0:?}")]
    ParentOrderHorizonOverflow(AccountId),
    #[error("P2 NPC projection could not clone its shadow: {0}")]
    ShadowClone(#[source] super::StepFatal),
    #[error("P2 NPC projection cannot hydrate strategy for account {account:?}: {source}")]
    StrategyHydration {
        account: AccountId,
        #[source]
        source: StrategyStateError,
    },
    #[error("P2 NPC projection cash reservation failed for account {account:?}: {source}")]
    CashReservation {
        account: AccountId,
        #[source]
        source: MoneyError,
    },
    #[error("P2 NPC projection cannot read sealed resources for account {account:?}: {source}")]
    ResourceSnapshot {
        account: AccountId,
        #[source]
        source: super::StepFatal,
    },
}

/// Projects the pure P2 result onto a nested shadow and commits only after all
/// accounts, reconciliation plans, parent-order changes and cash caps succeed.
pub(in crate::session) fn project_npc_p2(
    shadow: &mut GameSession,
    snapshot: &DecisionSnapshot,
    source: &NpcP2SourceOutput,
    resources: &DecisionResourceSnapshot,
) -> Result<NpcP2ProjectionOutput, NpcP2ProjectionError> {
    if shadow.tick != snapshot.tick() || shadow.phase() != snapshot.phase() {
        return Err(NpcP2ProjectionError::ClockMismatch {
            shadow_tick: shadow.tick,
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
        return Err(NpcP2ProjectionError::AccountOrderMismatch);
    }

    let mut candidate = shadow
        .clone_for_tick_shadow()
        .map_err(NpcP2ProjectionError::ShadowClone)?;
    let phase = snapshot.phase();
    let market_minute = snapshot.market_minute();
    let (working_continuous, working_auction) = candidate.working_orders_by_account();
    let mut decisions = Vec::new();
    let mut residual = Vec::new();

    for account_output in source.account_outputs() {
        let account = account_output.account();
        let sealed = snapshot
            .account(account)
            .map_err(|_| NpcP2ProjectionError::MissingAccount(account))?;
        let entry = candidate
            .accounts
            .get_mut(&account)
            .ok_or(NpcP2ProjectionError::MissingAccount(account))?;
        let account_kind = entry.kind;
        if account_kind != sealed.kind() {
            return Err(NpcP2ProjectionError::AccountKindMismatch(account));
        }
        let strategy = account_output
            .strategy_state()
            .clone()
            .into_strategy()
            .map_err(|source| NpcP2ProjectionError::StrategyHydration { account, source })?;
        entry.strategy = Some(StoredStrategy::production(strategy));

        if account_kind == AccountKind::Retail {
            if let Some(position_decision) = account_output.position_decision() {
                candidate.last_retail_decisions.push(RetailDecisionTrace {
                    account,
                    decision: position_decision.clone(),
                });
            }
            let held: BTreeSet<_> = entry.positions.keys().cloned().collect();
            let experience = candidate
                .retail_experience
                .get_mut(&account)
                .ok_or(NpcP2ProjectionError::MissingRetailExperience(account))?;
            for code in account_output.reviewed_stocks() {
                experience.observe_stock(code, market_minute);
            }
            experience.prune_watchlist(&held);
        }

        let raw: Vec<_> = source
            .intents()
            .iter()
            .filter(|intent| {
                matches!(intent.key(), P2CandidateKey::Npc { account: owner, .. } if *owner == account)
            })
            .map(|intent| (intent.key().clone(), intent.intent().clone()))
            .collect();
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
            validate_parent_order_materialization(&candidate, account, &desired, market_minute)?;
            desired = candidate.materialize_parent_order_intents(
                account,
                desired,
                market_minute,
                working,
            );
        }
        if account_output.updates_working_quotes() {
            let scope = if !account_output.reviewed_stocks().is_empty() {
                ReconcileScope::ReviewedStocks(account_output.reviewed_stocks().clone())
            } else if account_kind == AccountKind::Retail {
                ReconcileScope::DesiredStockSides
            } else {
                ReconcileScope::AllWorkingOrders
            };
            let plan =
                candidate.plan_npc_working_order_reconciliation(desired, phase, scope, working);
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
                source_intent: source.map(|(_, intent)| intent),
                projected_intent,
            });
        }
    }

    // ADR-0017 divergence #2: Cancel/Replace belongs to the sealed batch, so
    // its release cannot fund another intent in this batch. Cash-cap consumes
    // only P1's immutable available cash plus earlier residuals in this batch.
    let (residual_intents, cash_cap_changes) =
        apply_cash_cap(&candidate.setup.config, resources, residual)?;
    shadow.commit_tick_shadow(candidate);
    Ok(NpcP2ProjectionOutput {
        accepted_due_npc_ids: source.accounts().to_vec(),
        reconciliation_decisions: decisions,
        residual_intents,
        cash_cap_changes,
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
                price: order.price,
                qty: order.qty,
            })
        }),
        crate::TradingPhase::CallAuction => working.auction.iter().find_map(|(code, order)| {
            (OrderId(order.arrival_seq) == order_id).then(|| Intent::PlaceLimit {
                code: code.clone(),
                side: order.side,
                price: order.limit,
                qty: order.qty,
            })
        }),
        crate::TradingPhase::ClosingAuction | crate::TradingPhase::PreOpen => None,
    }
}

pub(super) fn remove_first_matching_raw(
    raw: &mut Vec<(P2CandidateKey, Intent)>,
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
    raw: &mut Vec<(P2CandidateKey, Intent)>,
    projected: &Intent,
) -> Option<(P2CandidateKey, Intent)> {
    raw.iter()
        .position(|(_, intent)| intents_equal(intent, projected))
        .map(|index| raw.remove(index))
}

fn validate_parent_order_materialization(
    session: &GameSession,
    account: AccountId,
    desired: &[Intent],
    market_minute: u64,
) -> Result<(), NpcP2ProjectionError> {
    if desired
        .iter()
        .any(|intent| matches!(intent, Intent::PlaceLimit { qty, .. } if *qty > 0 && qty.is_multiple_of(session.setup.config.lot_size)))
        && market_minute
            .checked_add(crate::session::PARENT_ORDER_HORIZON_MINUTES)
            .is_none()
    {
        return Err(NpcP2ProjectionError::ParentOrderHorizonOverflow(account));
    }
    for (code, plan) in session.parent_orders.get(&account).into_iter().flatten() {
        if &plan.code != code {
            return Err(NpcP2ProjectionError::InvalidParentOrder {
                account,
                reason: format!(
                    "map key {} does not match plan code {}",
                    code.0, plan.code.0
                ),
            });
        }
        let remaining = plan
            .target_qty
            .checked_sub(plan.filled_qty)
            .ok_or_else(|| NpcP2ProjectionError::InvalidParentOrder {
                account,
                reason: format!(
                    "{} filled quantity {} exceeds target {}",
                    code.0, plan.filled_qty, plan.target_qty
                ),
            })?;
        match (plan.active_child_order_id, plan.active_child_remaining_qty) {
            (None, None) => {}
            (Some(_), Some(quantity)) if quantity > 0 && quantity <= remaining => {}
            _ => {
                return Err(NpcP2ProjectionError::InvalidParentOrder {
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
        WorkingOrderDecision::Keep { order_id } => {
            NpcReconciliationDecision::Keep { account, order_id }
        }
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

fn apply_cash_cap(
    config: &crate::GameConfig,
    resources: &DecisionResourceSnapshot,
    pending: Vec<ProjectedNpcIntent>,
) -> Result<(Vec<ProjectedNpcIntent>, Vec<NpcCashCapChange>), NpcP2ProjectionError> {
    let mut planned_by_account = BTreeMap::<AccountId, Money>::new();
    let mut retained = Vec::with_capacity(pending.len());
    let mut changes = Vec::new();
    for mut projected in pending {
        let Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price,
            qty,
        } = projected.projected_intent.clone()
        else {
            retained.push(projected);
            continue;
        };
        let sealed_available = resources
            .available_cash(projected.account)
            .map_err(|source| NpcP2ProjectionError::ResourceSnapshot {
                account: projected.account,
                source,
            })?;
        let planned = *planned_by_account
            .get(&projected.account)
            .unwrap_or(&Money::ZERO);
        let available = sealed_available.sub(planned).map_err(|source| {
            NpcP2ProjectionError::CashReservation {
                account: projected.account,
                source,
            }
        })?;
        let affordable =
            match super::super::affordable_board_lot_buy_qty(config, price, qty, available) {
                Ok(value) => value,
                // Preserve invalid strategy output for P3's visible business rejection.
                Err(_) => {
                    retained.push(projected);
                    continue;
                }
            };
        let Some(affordable) = affordable else {
            changes.push(NpcCashCapChange::Dropped {
                account: projected.account,
                source_key: projected.source_key.clone(),
                requested: projected.projected_intent,
            });
            continue;
        };
        let required = super::super::buy_order_reservation(config, price, affordable, Money::ZERO)
            .map_err(|source| NpcP2ProjectionError::CashReservation {
                account: projected.account,
                source,
            })?;
        add_reservation(&mut planned_by_account, projected.account, required)?;
        if affordable != qty {
            changes.push(NpcCashCapChange::Resized {
                account: projected.account,
                source_key: projected.source_key.clone(),
                requested_qty: qty,
                projected_qty: affordable,
            });
            projected.projected_intent = Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price,
                qty: affordable,
            };
        }
        retained.push(projected);
    }
    Ok((retained, changes))
}

fn add_reservation(
    totals: &mut BTreeMap<AccountId, Money>,
    account: AccountId,
    required: Money,
) -> Result<(), NpcP2ProjectionError> {
    let total = totals.entry(account).or_insert(Money::ZERO);
    *total = total
        .add(required)
        .map_err(|source| NpcP2ProjectionError::CashReservation { account, source })?;
    Ok(())
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
