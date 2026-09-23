use crate::{frames, CorpusError};
use engine::{
    AccountId, CivilDate, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Intent,
    Money, NpcSetup, PositionSnap, RetailParams, SecurityCategory, SessionSetup, Side, StockCode,
    StockExchange, StockSpec, StrategyParams,
};
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq)]
pub enum Scenario {
    Equivalence,
    Divergence,
    Representation,
    Stress,
}

impl Scenario {
    pub fn parse(name: &str) -> Result<Self, CorpusError> {
        match name {
            "equivalence" => Ok(Self::Equivalence),
            "divergence-9" => Ok(Self::Divergence),
            "representation" => Ok(Self::Representation),
            "stress" => Ok(Self::Stress),
            _ => Err(CorpusError::Arguments(format!("unknown scenario: {name}"))),
        }
    }
}

pub(super) fn setup(price: i64) -> Result<SessionSetup, CorpusError> {
    Ok(SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600001".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(price),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.1,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.03,
                order_size: 100,
            },
            hot: HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day: 20,
        auction_ticks: 3,
        closing_auction_ticks: 2,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::ByKind {
            retail: 1.0,
            inst: 0.0,
            hot: 0.0,
        },
        start_date: CivilDate::from_iso("2030-01-01")
            .map_err(|error| CorpusError::Arguments(error.to_string()))?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V1.to_owned(),
    })
}

pub(super) fn seeded(price: i64, cash: i64, seed: u64) -> Result<GameSession, CorpusError> {
    let mut config = setup(price)?;
    config.config.starting_cash = Money::from_cents(cash);
    let mut save = GameSession::new(config, seed)?.save()?;
    let player = save
        .snapshot
        .accounts
        .get_mut(&AccountId(0))
        .ok_or_else(|| CorpusError::Invariant("player absent".to_owned()))?;
    player.positions.insert(
        StockCode("600001".to_owned()),
        PositionSnap {
            qty: 2_400,
            t1_locked: 0,
            invested_cents: 2_400 * price,
            recovered_cents: 0,
        },
    );
    Ok(GameSession::restore(&save)?)
}

pub(super) fn order(side: Side, price: i64, qty: u32) -> Intent {
    Intent::PlaceLimit {
        code: StockCode("600001".to_owned()),
        side,
        price: Money::from_cents(price),
        qty,
    }
}

pub(super) fn enqueue(session: &mut GameSession, intents: &[Intent]) -> Result<(), CorpusError> {
    for intent in intents {
        session.enqueue_player_intent(AccountId(0), intent.clone())?;
    }
    Ok(())
}

pub fn collect(scenario: Scenario, seed: u64) -> Result<Vec<Value>, CorpusError> {
    if scenario == Scenario::Stress {
        return crate::stress::collect(seed);
    }
    let (price, quantity, ticks, class) = match scenario {
        Scenario::Equivalence => (1_000, 300, 22, "i-equivalence"),
        Scenario::Divergence => (1, 1_200, 22, "ii-isolated-9"),
        Scenario::Representation => (1, 1_200, 22, "iv-controlled-live-sell"),
        Scenario::Stress => (1_000, 300, 200, "iii-new-engine-only"),
    };
    let mut session = seeded(price, 10_000_000, seed)?;
    let script: Vec<(u64, Vec<Intent>)> = match scenario {
        Scenario::Equivalence => vec![(
            4,
            vec![
                order(Side::Sell, price, quantity),
                order(Side::Buy, price, 100),
                order(Side::Buy, price, 200),
            ],
        )],
        Scenario::Divergence | Scenario::Representation => vec![
            (0, vec![order(Side::Sell, price, quantity)]),
            (4, vec![order(Side::Buy, price, 100)]),
            (5, vec![order(Side::Buy, price, 100)]),
            (6, vec![order(Side::Buy, price, 1_000)]),
        ],
        Scenario::Stress => (0..ticks)
            .filter(|tick| tick % 20 == 4)
            .map(|tick| {
                (
                    tick,
                    vec![order(Side::Sell, price, 100), order(Side::Buy, price, 100)],
                )
            })
            .collect(),
    };
    let mut records = vec![
        json!({"kind":"configuration", "class":class, "seed":seed.to_string(),
        "setup":session.save()?.setup, "initial_state":session.save()?, "sealed_exogenous_script":script,
        "feedback":"no NPC/strategy/plan-generated inputs; explicit player self-trade is permitted by baseline policy",
        "comparison_scope": if scenario == Scenario::Stress { "new-engine-only determinism/conservation/safety; no historical fidelity claim" } else { "per-tick multiset and explicit #9 projections" } }),
    ];
    for tick in 0..ticks {
        if scenario == Scenario::Representation && tick == 5 {
            let saved = session.save()?;
            records.push(json!({"kind":"before_save", "tick":tick, "state":saved}));
            let restored = GameSession::restore(&saved)?;
            if serde_json::to_vec(&saved)? != serde_json::to_vec(&restored.save()?)? {
                return Err(CorpusError::Invariant(
                    "restore changed authoritative state".to_owned(),
                ));
            }
            session = restored;
            records.push(json!({"kind":"after_restore", "tick":tick, "state":session.save()?}));
        }
        if let Some((_, intents)) = script.iter().find(|(at, _)| *at == tick) {
            enqueue(&mut session, intents)?;
        }
        let before = session.save()?;
        let mut frame = frames::step(&mut session)?;
        if matches!(scenario, Scenario::Divergence | Scenario::Representation)
            && (4..=6).contains(&tick)
        {
            let leg = usize::try_from(tick - 4)
                .map_err(|error| CorpusError::Invariant(error.to_string()))?;
            let gross = [100, 100, 1_000][leg];
            let nominal = [500, 0, 1][leg];
            let before_cash = before
                .snapshot
                .accounts
                .get(&AccountId(0))
                .ok_or_else(|| CorpusError::Invariant("player missing before trade".to_owned()))?
                .cash
                .cents();
            let after = session.save()?;
            let after_cash = after
                .snapshot
                .accounts
                .get(&AccountId(0))
                .ok_or_else(|| CorpusError::Invariant("player missing after trade".to_owned()))?
                .cash
                .cents();
            if before_cash - after_cash != nominal + 500 {
                return Err(CorpusError::Invariant(format!(
                    "fee leg {leg} cash mismatch: {}",
                    before_cash - after_cash
                )));
            }
            frame["seller_fee_projection"] = json!({"gross_cents":gross,"old_charged_cents":nominal,
                "old_net_cents":gross-nominal,"new_expected_charged_cents":([100,100,301][leg]),
                "new_expected_net_cents":([0,0,699][leg]),"unchanged_counterparty_buy_fee_cents":500,
                "observed_combined_account_cash_delta":after_cash-before_cash});
        }
        records.push(frame);
    }
    if scenario == Scenario::Divergence {
        records.push(crate::fee_control::collect(seed)?);
        let mut poor = seeded(100, 0, seed)?;
        enqueue(&mut poor, &[order(Side::Sell, 100, 100)])?;
        let rejection = frames::step(&mut poor)?;
        let mut funded = seeded(100, 10_000, seed)?;
        enqueue(&mut funded, &[order(Side::Sell, 100, 100)])?;
        let acceptance = frames::step(&mut funded)?;
        records.push(json!({"kind":"acceptance_boundary", "owned_sellable_shares":2400,
            "old_zero_cash":rejection, "old_funded":acceptance,
            "new_expected":{"accepted":true,"reserved_cash":0},
            "allowed_paths":["events.IntentRejected->OrderAccepted","snapshot.accounts.0.reserved_cash","orders"]}));
        records.push(json!({"kind":"fee_expectation", "gross_legs_cents":[100,100,1000],
            "nominal_cumulative_cents":[500,500,501], "old_charged_legs_cents":[500,0,1],
            "new_charged_legs_cents":[100,100,301], "old_net_legs_cents":[-400,100,999],
            "new_net_legs_cents":[0,0,699], "new_sell_reserved_cash":0,
            "note":"seller leg projection; same player counterparty pays separate unchanged buy fees"}));
    }
    records.push(json!({"kind":"terminal", "state":session.save()?}));
    if scenario == Scenario::Equivalence {
        records.extend(crate::witnesses::collect(seed)?);
    }
    Ok(records)
}
