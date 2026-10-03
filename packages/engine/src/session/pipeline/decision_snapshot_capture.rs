//! Production P2 input capture on the discardable tick shadow.
//!
//! This seam advances only the attention/retail-observation state needed
//! to seal strategy inputs. It never routes an intent or mutates an order book.

use super::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError};
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{
    build_account_risk_observation, build_equal_weight_market_observation, AccountRiskObservation,
    ObservationError, RiskPositionInput,
};
use crate::session::{AuctionOrdersByAccount, ContinuousOrdersByAccount};
use crate::strategy::{PositionView, SelfView, StrategyStateError};
use crate::{
    AccountId, AccountKind, ExperienceError, Money, MoneyError, RetailExperienceState, StockCode,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug)]
pub(in crate::session) struct CapturedDecisionSnapshot {
    pub(super) snapshot: Arc<DecisionSnapshot>,
    pub(super) working_continuous: ContinuousOrdersByAccount,
    pub(super) working_auction: AuctionOrdersByAccount,
}

#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum DecisionSnapshotCaptureError {
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
    #[error("P2 urgency policy validation failed: {0}")]
    UrgencyPolicy(#[from] crate::plans::UrgencyError),
}

/// Advances due-attention and pre-decision retail observations on `shadow`, then
/// returns the immutable input consumed by the pure NPC P2 source.
///
/// Only due attention and accepted retail observations can change here. The
/// caller owns a discardable tick candidate and drops it if capture fails.
pub(in crate::session) fn capture_decision_snapshot(
    shadow: &mut super::GameSession,
) -> Result<CapturedDecisionSnapshot, DecisionSnapshotCaptureError> {
    capture_decision_snapshot_in_place(shadow)
}

fn capture_decision_snapshot_in_place(
    shadow: &mut super::GameSession,
) -> Result<CapturedDecisionSnapshot, DecisionSnapshotCaptureError> {
    let tick = shadow.state.tick;
    let phase = shadow.phase();
    validate_market_view_inputs(shadow)?;
    let market = shadow.build_market_view();

    let (due_npc_ids, popped) = shadow.pop_due_npc_ids(tick);
    let attention_signal = super::super::attention::market_attention_signal(&market);
    if let Some(account) = popped
        .iter()
        .map(|(_, account)| *account)
        .find(|account| !shadow.state.npc_attention.contains_key(account))
    {
        return Err(DecisionSnapshotCaptureError::MissingAttention(account));
    }
    let accounts = &shadow.state.accounts;
    let attention_results = shadow.state.npc_attention.mutate_existing_parallel(
        &due_npc_ids,
        DecisionSnapshotCaptureError::MissingAttention,
        |account, attention| {
            let kind = accounts
                .get(&account)
                .map(|entry| entry.kind())
                .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
            if kind == AccountKind::Player {
                return Err(DecisionSnapshotCaptureError::PlayerAccount(account));
            }
            if !attention.base_probability.is_finite()
                || attention.base_probability <= 0.0
                || attention.base_probability > 1.0
            {
                return Err(DecisionSnapshotCaptureError::InvalidAttentionProbability {
                    account,
                    probability: attention.base_probability,
                });
            }
            let observes = attention.evaluate_candidate_with_signal(kind, attention_signal, tick);
            Ok((observes, attention.next_attention_candidate_tick))
        },
    )?;
    let mut accepted_due_npc_ids = Vec::new();
    let mut scheduled = Vec::with_capacity(attention_results.len());
    for (account, (observes, next_tick)) in attention_results {
        scheduled.push((next_tick, account));
        if observes {
            accepted_due_npc_ids.push(account);
        }
    }

    let market_minute = shadow.current_market_minute();
    let (working_continuous, working_auction) = if accepted_due_npc_ids.is_empty() {
        (BTreeMap::new(), BTreeMap::new())
    } else {
        shadow.working_orders_for_accounts(&accepted_due_npc_ids.iter().copied().collect())
    };
    let has_retail_observer = accepted_due_npc_ids.iter().any(|account| {
        shadow
            .state
            .accounts
            .get(account)
            .is_some_and(|entry| entry.kind() == AccountKind::Retail)
    });
    let behavior_market = has_retail_observer
        .then(|| build_behavior_market_checked(shadow))
        .transpose()?;
    let prepared = accepted_due_npc_ids
        .par_iter()
        .map(|account| {
            let entry = shadow
                .state
                .accounts
                .get(account)
                .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
            let observation =
                CapturedExperienceObservation::capture(shadow, *account, market_minute)?;
            let CapturedAccountObservation {
                self_view,
                account_risk,
                experience,
            } = observation.consume(*account, entry.kind(), entry.cash(), |self_positions| {
                build_self_view_for(
                    shadow,
                    *account,
                    phase,
                    &working_continuous,
                    &working_auction,
                    self_positions,
                )
            })?;
            let strategy_state = entry
                .strategy()
                .ok_or(DecisionSnapshotCaptureError::MissingStrategy(*account))?
                .production_state()
                .map_err(|source| DecisionSnapshotCaptureError::StrategyState {
                    account: *account,
                    source,
                })?;
            let input = DecisionAccountInput::new_shared(
                entry.kind(),
                self_view,
                strategy_state,
                account_risk,
                experience.clone(),
            );
            Ok((*account, input, experience))
        })
        .collect::<Vec<Result<_, DecisionSnapshotCaptureError>>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let mut accounts = BTreeMap::new();
    let mut experience_updates = Vec::new();
    for (account, input, experience) in prepared {
        accounts.insert(account, input);
        if let Some(experience) = experience {
            experience_updates.push((account, experience));
        }
    }

    let snapshot = DecisionSnapshot::new(
        tick,
        shadow.state.seed,
        phase,
        market_minute,
        market,
        behavior_market,
        accepted_due_npc_ids,
        accounts,
    )
    .map_err(DecisionSnapshotCaptureError::Snapshot)?
    .with_urgency_policy(shadow.state.urgency_policy)?;

    shadow
        .state
        .retail_experience
        .replace_existing_shared_parallel(experience_updates)
        .map_err(DecisionSnapshotCaptureError::MissingAccount)?;
    for (tick, account) in scheduled {
        shadow.state.attention_scheduler.enqueue(tick, account);
    }
    Ok(CapturedDecisionSnapshot {
        snapshot: Arc::new(snapshot),
        working_continuous,
        working_auction,
    })
}

fn validate_market_view_inputs(
    session: &super::GameSession,
) -> Result<(), DecisionSnapshotCaptureError> {
    for code in session.state.markets.keys() {
        for (location, present) in [
            (
                "market price history",
                session.state.price_history.contains_key(code),
            ),
            (
                "market-minute history",
                session.state.market_minute_closes.contains_key(code),
            ),
            (
                "daily history",
                session.state.candle_book.histories().contains_key(code),
            ),
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

/// 同一次账户观察的派生事实；非 Retail 的已有 experience 仍可不携带风险输入。
enum CapturedExperienceObservation {
    NoExperience,
    Captured {
        experience: RetailExperienceState,
        risk_positions: Option<BTreeMap<StockCode, RiskPositionInput>>,
        self_positions: BTreeMap<StockCode, PositionView>,
    },
}

struct CapturedAccountObservation {
    self_view: SelfView,
    account_risk: Option<AccountRiskObservation>,
    experience: Option<Arc<RetailExperienceState>>,
}

impl CapturedExperienceObservation {
    fn capture(
        session: &super::GameSession,
        account: AccountId,
        market_minute: u64,
    ) -> Result<Self, DecisionSnapshotCaptureError> {
        let Some(mut experience) = session.state.retail_experience.get(&account).cloned() else {
            return Ok(Self::NoExperience);
        };
        let entry = session
            .state
            .accounts
            .get(&account)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
        let mut equity = entry.cash();
        let mut positions = Vec::with_capacity(entry.positions().len());
        let mut self_positions = BTreeMap::new();
        for (code, position) in entry.positions() {
            let price = session
                .state
                .markets
                .get(code)
                .ok_or_else(|| DecisionSnapshotCaptureError::MissingMarketData {
                    location: "retail experience position",
                    code: code.clone(),
                })?
                .last_price();
            equity = equity
                .add(price.mul_shares(position.qty()).map_err(|source| {
                    DecisionSnapshotCaptureError::Money {
                        location: "retail experience equity",
                        account,
                        source,
                    }
                })?)
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "retail experience equity",
                    account,
                    source,
                })?;
            positions.push((code.clone(), price, position.qty(), position.cost_price()));
            self_positions.insert(
                code.clone(),
                PositionView {
                    qty: position.qty(),
                    sellable_qty: position.sellable(),
                    cost_price: position.cost_price(),
                },
            );
        }
        if equity.cents() > 0 {
            experience
                .observe_equity(equity)
                .map_err(|source| DecisionSnapshotCaptureError::Experience { account, source })?;
        }
        let held: BTreeSet<_> = self_positions.keys().cloned().collect();
        let mut risk_positions = BTreeMap::new();
        for (code, price, qty, cost_price) in positions {
            experience
                .observe_position(&code, price, market_minute)
                .map_err(|source| DecisionSnapshotCaptureError::Experience { account, source })?;
            if entry.kind() == AccountKind::Retail {
                risk_positions.insert(
                    code.clone(),
                    RiskPositionInput {
                        qty,
                        cost_price: cost_price.filter(|price| price.cents() > 0),
                        last_price: price,
                        peak_price_since_entry: experience
                            .stocks
                            .get(&code)
                            .and_then(|stock| stock.peak_price_since_entry),
                    },
                );
            }
        }
        experience.prune_watchlist(&held);
        let risk_positions = (entry.kind() == AccountKind::Retail).then_some(risk_positions);
        Ok(Self::Captured {
            experience,
            risk_positions,
            self_positions,
        })
    }

    fn consume(
        self,
        account: AccountId,
        kind: AccountKind,
        cash: Money,
        build_self_view: impl FnOnce(
            Option<BTreeMap<StockCode, PositionView>>,
        ) -> Result<SelfView, DecisionSnapshotCaptureError>,
    ) -> Result<CapturedAccountObservation, DecisionSnapshotCaptureError> {
        match self {
            Self::NoExperience => {
                let self_view = build_self_view(None)?;
                if kind == AccountKind::Retail {
                    return Err(DecisionSnapshotCaptureError::MissingAccount(account));
                }
                Ok(CapturedAccountObservation {
                    self_view,
                    account_risk: None,
                    experience: None,
                })
            }
            Self::Captured {
                experience,
                risk_positions,
                self_positions,
            } => {
                // SelfView 的 Money 错误仍先于风险观察错误；不得在 capture 中提前计算风险。
                let self_view = build_self_view(Some(self_positions))?;
                let account_risk = risk_positions
                    .map(|positions| {
                        build_account_risk_observation(
                            cash,
                            &positions,
                            experience.reference_equity,
                            experience.peak_equity,
                        )
                        .map_err(|source| {
                            DecisionSnapshotCaptureError::Observation {
                                location: "account risk",
                                account: Some(account),
                                source,
                            }
                        })
                    })
                    .transpose()?;
                Ok(CapturedAccountObservation {
                    self_view,
                    account_risk,
                    experience: Some(Arc::new(experience)),
                })
            }
        }
    }
}

/// SelfView 的报价现金累计；可替换部分不构成本轮 P1 执行预算。
#[derive(Default)]
struct SelfViewCashReservations {
    reserved: Money,
    replaceable: Money,
}

impl SelfViewCashReservations {
    fn record(&mut self, required: Money, may_replace: bool) -> Result<(), MoneyError> {
        self.reserved = self.reserved.add(required)?;
        if may_replace {
            self.replaceable = self.replaceable.add(required)?;
        }
        Ok(())
    }

    fn available_cash(&self, raw_cash: Money) -> Result<Money, MoneyError> {
        raw_cash.sub(self.reserved)?.add(self.replaceable)
    }
}

fn build_self_view_for(
    session: &super::GameSession,
    account: AccountId,
    phase: crate::TradingPhase,
    continuous: &super::super::ContinuousOrdersByAccount,
    auction: &super::super::AuctionOrdersByAccount,
    self_positions: Option<BTreeMap<StockCode, PositionView>>,
) -> Result<SelfView, DecisionSnapshotCaptureError> {
    let auction_cancelable = phase == crate::TradingPhase::CallAuction
        && session.state.tick % session.state.setup.ticks_per_day
            < session.state.setup.auction_ticks / 3;
    let entry = session
        .state
        .accounts
        .get(&account)
        .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
    let mut reservations = SelfViewCashReservations::default();
    for (_, order) in continuous.get(&account).into_iter().flatten() {
        reservations
            .record(
                super::super::live_cash_reservation(
                    &session.state.setup.config,
                    order.side,
                    order.price,
                    order.qty,
                    order.filled_value,
                )
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "continuous self-view reservation",
                    account,
                    source,
                })?,
                phase == crate::TradingPhase::Continuous,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "continuous self-view reservation sum",
                account,
                source,
            })?;
    }
    for (_, order) in auction.get(&account).into_iter().flatten() {
        reservations
            .record(
                super::super::live_cash_reservation(
                    &session.state.setup.config,
                    order.side,
                    order.limit,
                    order.qty,
                    Money::ZERO,
                )
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "auction self-view reservation",
                    account,
                    source,
                })?,
                auction_cancelable,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "auction self-view reservation sum",
                account,
                source,
            })?;
    }
    let cash = reservations
        .available_cash(entry.cash())
        .map_err(|source| DecisionSnapshotCaptureError::Money {
            location: "available self-view cash",
            account,
            source,
        })?;
    let positions = self_positions.unwrap_or_else(|| {
        entry
            .positions()
            .iter()
            .map(|(code, position)| {
                (
                    code.clone(),
                    PositionView {
                        qty: position.qty(),
                        sellable_qty: position.sellable(),
                        cost_price: position.cost_price(),
                    },
                )
            })
            .collect()
    });
    Ok(SelfView { cash, positions })
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
