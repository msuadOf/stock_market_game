//! Production P2 input capture on the discardable tick shadow.
//!
//! This seam advances only the legacy attention/retail-observation state needed
//! to seal strategy inputs. It never routes an intent or mutates an order book.

use super::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError};
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{
    build_account_risk_observation, build_equal_weight_market_observation, AccountRiskObservation,
    ObservationError, RiskPositionInput,
};
use crate::strategy::{PositionView, SelfView, StrategyStateError};
use crate::{AccountId, AccountKind, ExperienceError, Money, MoneyError, StockCode};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum DecisionSnapshotCaptureError {
    #[error("P2 decision snapshot could not clone its tick shadow: {0}")]
    ShadowClone(#[source] super::StepFatal),
    #[error("P2 decision snapshot accepted missing account {0:?}")]
    MissingAccount(AccountId),
    #[error("P2 decision snapshot accepted player account {0:?} as an NPC")]
    PlayerAccount(AccountId),
    #[error("P2 decision snapshot account {0:?} has no attention state")]
    MissingAttention(AccountId),
    #[error(
        "P2 decision snapshot account {account:?} has invalid attention probability {probability}"
    )]
    InvalidAttentionProbability {
        account: AccountId,
        probability: f64,
    },
    #[error("P2 decision snapshot account {0:?} has no production strategy")]
    MissingStrategy(AccountId),
    #[error("P2 decision snapshot cannot export strategy for account {account:?}: {source}")]
    StrategyState {
        account: AccountId,
        #[source]
        source: StrategyStateError,
    },
    #[error("P2 retail experience observation failed for account {account:?}: {source}")]
    Experience {
        account: AccountId,
        #[source]
        source: ExperienceError,
    },
    #[error("P2 decision snapshot {location} is missing market data for {code:?}")]
    MissingMarketData {
        location: &'static str,
        code: StockCode,
    },
    #[error("P2 decision snapshot {location} money failure for account {account:?}: {source}")]
    Money {
        location: &'static str,
        account: AccountId,
        #[source]
        source: MoneyError,
    },
    #[error(
        "P2 decision snapshot {location} observation failure for account {account:?}: {source}"
    )]
    Observation {
        location: &'static str,
        account: Option<AccountId>,
        #[source]
        source: ObservationError,
    },
    #[error("P2 decision snapshot contract rejected captured input: {0}")]
    Snapshot(#[source] DecisionSnapshotError),
}

/// Advances due-attention and pre-decision retail observations on `shadow`, then
/// returns the immutable input consumed by the pure NPC P2 source.
///
/// Work is first performed on a nested discardable clone. The three affected
/// state families are moved back only after the complete snapshot validates, so
/// every typed failure leaves the caller's shadow byte-for-byte unchanged in
/// those families.
pub(in crate::session) fn capture_decision_snapshot(
    shadow: &mut super::GameSession,
) -> Result<Arc<DecisionSnapshot>, DecisionSnapshotCaptureError> {
    let mut candidate = shadow
        .clone_for_tick_shadow()
        .map_err(DecisionSnapshotCaptureError::ShadowClone)?;
    let tick = candidate.tick;
    let phase = candidate.phase();
    validate_market_view_inputs(&candidate)?;
    let market = candidate.build_market_view();

    if let Some(account) = candidate
        .attention_queue
        .iter()
        .map(|entry| entry.0)
        .filter(|(scheduled, _)| *scheduled <= tick)
        .map(|(_, account)| account)
        .find(|account| !candidate.npc_attention.contains_key(account))
    {
        return Err(DecisionSnapshotCaptureError::MissingAttention(account));
    }

    let mut accepted_due_npc_ids = Vec::new();
    for account in candidate.pop_due_npc_ids(tick) {
        let kind = candidate
            .accounts
            .get(&account)
            .map(|entry| entry.kind)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
        if kind == AccountKind::Player {
            return Err(DecisionSnapshotCaptureError::PlayerAccount(account));
        }
        let attention = candidate
            .npc_attention
            .get(&account)
            .ok_or(DecisionSnapshotCaptureError::MissingAttention(account))?;
        if !attention.base_probability.is_finite()
            || attention.base_probability <= 0.0
            || attention.base_probability > 1.0
        {
            return Err(DecisionSnapshotCaptureError::InvalidAttentionProbability {
                account,
                probability: attention.base_probability,
            });
        }
        if candidate.evaluate_attention_candidate(account, &market) {
            accepted_due_npc_ids.push(account);
        }
    }

    observe_retail_experience_checked(&mut candidate, &accepted_due_npc_ids)?;

    let market_minute = candidate.current_market_minute();
    let (working_continuous, working_auction) = candidate.working_orders_by_account();
    let self_views = build_self_views_checked(
        &candidate,
        &accepted_due_npc_ids,
        phase,
        &working_continuous,
        &working_auction,
    )?;
    let has_retail_observer = accepted_due_npc_ids.iter().any(|account| {
        candidate
            .accounts
            .get(account)
            .is_some_and(|entry| entry.kind == AccountKind::Retail)
    });
    let behavior_market = has_retail_observer
        .then(|| build_behavior_market_checked(&candidate))
        .transpose()?;
    let account_risks = if has_retail_observer {
        build_account_risks_checked(&candidate, &accepted_due_npc_ids)?
    } else {
        BTreeMap::new()
    };

    let mut accounts = BTreeMap::new();
    for account in &accepted_due_npc_ids {
        let entry = candidate
            .accounts
            .get(account)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
        let strategy_state = entry
            .strategy
            .as_ref()
            .ok_or(DecisionSnapshotCaptureError::MissingStrategy(*account))?
            .production_state()
            .map_err(|source| DecisionSnapshotCaptureError::StrategyState {
                account: *account,
                source,
            })?;
        let self_view = self_views
            .get(account)
            .cloned()
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
        accounts.insert(
            *account,
            DecisionAccountInput::new(
                entry.kind,
                self_view,
                strategy_state,
                account_risks.get(account).cloned(),
                candidate.retail_experience.get(account).cloned(),
            ),
        );
    }

    let snapshot = DecisionSnapshot::new(
        tick,
        candidate.seed,
        phase,
        market_minute,
        market,
        behavior_market,
        accepted_due_npc_ids,
        accounts,
    )
    .map_err(DecisionSnapshotCaptureError::Snapshot)?;

    shadow.npc_attention = candidate.npc_attention;
    shadow.attention_queue = candidate.attention_queue;
    shadow.retail_experience = candidate.retail_experience;
    Ok(Arc::new(snapshot))
}

fn validate_market_view_inputs(
    session: &super::GameSession,
) -> Result<(), DecisionSnapshotCaptureError> {
    for code in session.markets.keys() {
        for (location, present) in [
            (
                "market price history",
                session.price_history.contains_key(code),
            ),
            (
                "market-minute history",
                session.market_minute_closes.contains_key(code),
            ),
            ("daily history", session.daily_candles.contains_key(code)),
        ] {
            if !present {
                return Err(DecisionSnapshotCaptureError::MissingMarketData {
                    location,
                    code: code.clone(),
                });
            }
        }
    }
    Ok(())
}

fn observe_retail_experience_checked(
    session: &mut super::GameSession,
    ids: &[AccountId],
) -> Result<(), DecisionSnapshotCaptureError> {
    let market_minute = session.current_market_minute();
    let mut observations = Vec::new();
    for account in ids {
        if !session.retail_experience.contains_key(account) {
            continue;
        }
        let entry = session
            .accounts
            .get(account)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
        let mut equity = entry.cash;
        let mut positions = Vec::new();
        for (code, position) in &entry.positions {
            let price = session
                .markets
                .get(code)
                .ok_or_else(|| DecisionSnapshotCaptureError::MissingMarketData {
                    location: "retail experience position",
                    code: code.clone(),
                })?
                .last_price();
            equity = equity
                .add(price.mul_shares(position.qty).map_err(|source| {
                    DecisionSnapshotCaptureError::Money {
                        location: "retail experience equity",
                        account: *account,
                        source,
                    }
                })?)
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "retail experience equity",
                    account: *account,
                    source,
                })?;
            positions.push((code.clone(), price));
        }
        observations.push((*account, equity, positions));
    }
    for (account, equity, positions) in observations {
        let experience = session
            .retail_experience
            .get_mut(&account)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
        if equity.cents() > 0 {
            experience
                .observe_equity(equity)
                .map_err(|source| DecisionSnapshotCaptureError::Experience { account, source })?;
        }
        let held: BTreeSet<_> = positions.iter().map(|(code, _)| code.clone()).collect();
        for (code, price) in positions {
            experience
                .observe_position(&code, price, market_minute)
                .map_err(|source| DecisionSnapshotCaptureError::Experience { account, source })?;
        }
        experience.prune_watchlist(&held);
    }
    Ok(())
}

fn build_self_views_checked(
    session: &super::GameSession,
    ids: &[AccountId],
    phase: crate::TradingPhase,
    continuous: &super::super::ContinuousOrdersByAccount,
    auction: &super::super::AuctionOrdersByAccount,
) -> Result<BTreeMap<AccountId, SelfView>, DecisionSnapshotCaptureError> {
    let auction_cancelable = phase == crate::TradingPhase::CallAuction
        && session.tick % session.setup.ticks_per_day < session.setup.auction_ticks / 3;
    let mut views = BTreeMap::new();
    for account in ids {
        let entry = session
            .accounts
            .get(account)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
        let mut reserved = Money::ZERO;
        let mut replaceable = Money::ZERO;
        let mut record = |required: Money, may_replace: bool| -> Result<(), MoneyError> {
            reserved = reserved.add(required)?;
            if may_replace {
                replaceable = replaceable.add(required)?;
            }
            Ok(())
        };
        for (_, order) in continuous.get(account).into_iter().flatten() {
            record(
                super::super::live_cash_reservation(
                    &session.setup.config,
                    &session.setup.simulation_policy_id,
                    order.side,
                    order.price,
                    order.qty,
                    order.filled_value,
                )
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "continuous self-view reservation",
                    account: *account,
                    source,
                })?,
                phase == crate::TradingPhase::Continuous,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "continuous self-view reservation sum",
                account: *account,
                source,
            })?;
        }
        for (_, order) in auction.get(account).into_iter().flatten() {
            record(
                super::super::live_cash_reservation(
                    &session.setup.config,
                    &session.setup.simulation_policy_id,
                    order.side,
                    order.limit,
                    order.qty,
                    Money::ZERO,
                )
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "auction self-view reservation",
                    account: *account,
                    source,
                })?,
                auction_cancelable,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "auction self-view reservation sum",
                account: *account,
                source,
            })?;
        }
        let cash = entry
            .cash
            .sub(reserved)
            .and_then(|cash| cash.add(replaceable))
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "available self-view cash",
                account: *account,
                source,
            })?;
        let positions = entry
            .positions
            .iter()
            .map(|(code, position)| {
                (
                    code.clone(),
                    PositionView {
                        qty: position.qty,
                        sellable_qty: position.sellable(),
                        cost_price: position.cost_price(),
                    },
                )
            })
            .collect();
        views.insert(*account, SelfView { cash, positions });
    }
    Ok(views)
}

fn build_behavior_market_checked(
    session: &super::GameSession,
) -> Result<BehaviorMarketObservation, DecisionSnapshotCaptureError> {
    let price_paths = session.market_price_path_observations().map_err(|source| {
        DecisionSnapshotCaptureError::Observation {
            location: "behavior price paths",
            account: None,
            source,
        }
    })?;
    let returns = price_paths
        .iter()
        .map(|(code, path)| (code.clone(), path.thirty_minute.return_ratio))
        .collect();
    let thirty_minute_market =
        build_equal_weight_market_observation(&returns).map_err(|source| {
            DecisionSnapshotCaptureError::Observation {
                location: "behavior equal-weight market",
                account: None,
                source,
            }
        })?;
    Ok(BehaviorMarketObservation {
        price_paths,
        thirty_minute_market,
    })
}

fn build_account_risks_checked(
    session: &super::GameSession,
    ids: &[AccountId],
) -> Result<BTreeMap<AccountId, AccountRiskObservation>, DecisionSnapshotCaptureError> {
    let mut risks = BTreeMap::new();
    for account in ids {
        let entry = session
            .accounts
            .get(account)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
        if entry.kind != AccountKind::Retail {
            continue;
        }
        let experience = session
            .retail_experience
            .get(account)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
        let mut positions = BTreeMap::new();
        for (code, position) in &entry.positions {
            let last_price = session
                .markets
                .get(code)
                .ok_or_else(|| DecisionSnapshotCaptureError::MissingMarketData {
                    location: "account risk position",
                    code: code.clone(),
                })?
                .last_price();
            positions.insert(
                code.clone(),
                RiskPositionInput {
                    qty: position.qty,
                    cost_price: position.cost_price().filter(|price| price.cents() > 0),
                    last_price,
                    peak_price_since_entry: experience
                        .stocks
                        .get(code)
                        .and_then(|stock| stock.peak_price_since_entry),
                },
            );
        }
        let risk = build_account_risk_observation(
            entry.cash,
            &positions,
            experience.reference_equity,
            experience.peak_equity,
        )
        .map_err(|source| DecisionSnapshotCaptureError::Observation {
            location: "account risk",
            account: Some(*account),
            source,
        })?;
        risks.insert(*account, risk);
    }
    Ok(risks)
}
