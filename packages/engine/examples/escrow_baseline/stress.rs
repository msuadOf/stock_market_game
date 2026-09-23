use crate::{frames, scenarios::setup, CorpusError};
use engine::{
    CivilDate, FloatAllocation, GameSession, Money, NpcSetup, SecurityCategory, StockCode,
    StockExchange, StockSpec,
};
use serde_json::{json, Value};

pub fn collect(seed: u64) -> Result<Vec<Value>, CorpusError> {
    let mut config = setup(1_120)?;
    config.stocks = vec![
        stock("600101", 1_120, 8_928_571_429, SecurityCategory::MainBoard)?,
        stock("002156", 2_735, 2_925_045_704, SecurityCategory::MainBoard)?,
        stock("300260", 3_680, 815_217_391, SecurityCategory::ChiNext)?,
        stock("600610", 755, 1_059_602_649, SecurityCategory::MainBoard)?,
        stock("000812", 285, 1_052_631_579, SecurityCategory::StMainBoard)?,
    ];
    config.npcs = NpcSetup {
        retail_count: 12,
        inst_count: 10,
        hot_count: 4,
        retail_cash_median: Money::from_cents(100_000_000),
    };
    config.strategy_params.retail.arrival_rate = 0.4;
    config.strategy_params.retail.chase_prob = 0.3;
    config.strategy_params.retail.tick_cents = 2;
    config.strategy_params.inst.order_size = 5_000;
    config.strategy_params.hot.order_size = 1_000;
    config.strategy_params.hot.lookback = 10;
    config.ticks_per_day = 60;
    config.auction_ticks = 6;
    config.closing_auction_ticks = 3;
    config.float_allocation = FloatAllocation::ByKind {
        retail: 0.4,
        inst: 0.5,
        hot: 0.1,
    };
    config.start_date = CivilDate::from_iso("2030-01-07")
        .map_err(|error| CorpusError::Arguments(error.to_string()))?;
    let mut session = GameSession::new(config, seed)?;
    let initial_shares = total_shares(&session)?;
    let mut records = vec![
        json!({"kind":"configuration","class":"iii-new-engine-only","seed":seed.to_string(),
        "setup":session.save()?.setup,"comparison_scope":"new-engine-only determinism/conservation/safety; not historical fidelity",
        "bounds":{"ticks":180,"accounts":27,"stocks":5}}),
    ];
    let mut chain_witness = false;
    for _ in 0..180 {
        let before = session.save()?;
        if !chain_witness && session.phase() == engine::TradingPhase::Continuous {
            let candidate = before
                .resting_orders
                .iter()
                .flat_map(|(code, orders)| orders.iter().map(move |order| (code, order)))
                .find(|(_, order)| {
                    order.owner.0 >= 13
                        && order.owner.0 < 23
                        && order.side == engine::Side::Sell
                        && order.qty >= 100
                });
            if let Some((code, order)) = candidate {
                let mut treatment = GameSession::new(before.setup.clone(), seed)?;
                let mut control = GameSession::new(before.setup.clone(), seed)?;
                for _ in 0..session.tick() {
                    treatment.step()?;
                    control.step()?;
                }
                treatment.enqueue_player_intent(
                    engine::AccountId(0),
                    engine::Intent::PlaceLimit {
                        code: code.clone(),
                        side: engine::Side::Buy,
                        price: order.price,
                        qty: 100,
                    },
                )?;
                let control_frame = frames::step(&mut control)?;
                let treatment_frame = frames::step(&mut treatment)?;
                records.push(json!({"kind":"divergence_witness","ids":[1],"name":"player-takes-real-institution-child-before-chain",
                    "pre_tick_plans":before.plans,"institution_order":order,"control":control_frame,"treatment":treatment_frame,
                    "control_plans":control.save()?.plans,"treatment_plans":treatment.save()?.plans,
                    "scope":"old-side characterization; new snapshot visibility expectation only, not new engine execution"}));
                chain_witness = true;
            }
        }
        let frame = frames::step(&mut session)?;
        check_state(&session, &initial_shares)?;
        records.push(frame);
    }
    records.push(json!({"kind":"terminal","state":session.save()?,"decision_chain":session.decision_chain_diagnostics(),
        "baseline_invariants":{"checked_ticks":180,"shares_conserved":true,"nonnegative_cash":true,"t1_bounded":true,"reservations_bounded":true,"order_quantities_consistent":true},
        "new_engine_only_gates":["envelope dual-ledger conservation","receipt fee-to-cash reconciliation"]}));
    Ok(records)
}

fn total_shares(
    session: &GameSession,
) -> Result<std::collections::BTreeMap<StockCode, u64>, CorpusError> {
    let mut totals = std::collections::BTreeMap::new();
    for account in session.save()?.snapshot.accounts.values() {
        for (code, position) in &account.positions {
            *totals.entry(code.clone()).or_insert(0) += u64::from(position.qty);
        }
    }
    Ok(totals)
}

fn check_state(
    session: &GameSession,
    shares: &std::collections::BTreeMap<StockCode, u64>,
) -> Result<(), CorpusError> {
    let save = session.save()?;
    if &total_shares(session)? != shares {
        return Err(CorpusError::Invariant(
            "stress share conservation".to_owned(),
        ));
    }
    for account in save.snapshot.accounts.values() {
        if account.cash.cents() < 0
            || account.reserved_cash.cents() < 0
            || account.reserved_cash > account.cash
        {
            return Err(CorpusError::Invariant(
                "stress cash/reservation bounds".to_owned(),
            ));
        }
        for (code, position) in &account.positions {
            let reserved = match account.reserved_sell_qty.get(code) {
                Some(quantity) => *quantity,
                None => 0,
            };
            if position.t1_locked > position.qty || reserved > position.qty - position.t1_locked {
                return Err(CorpusError::Invariant(
                    "stress T+1/share reservation bounds".to_owned(),
                ));
            }
        }
    }
    for order in save.resting_orders.values().flatten() {
        if order.qty == 0
            || order.qty.checked_add(order.filled_qty) != Some(order.original_qty)
            || order.filled_value.cents() < 0
        {
            return Err(CorpusError::Invariant(
                "stress order quantity consistency".to_owned(),
            ));
        }
    }
    Ok(())
}

fn stock(
    code: &str,
    price: i64,
    total: u64,
    category: SecurityCategory,
) -> Result<StockSpec, CorpusError> {
    let float =
        u32::try_from(total * 2 / 5).map_err(|error| CorpusError::Invariant(error.to_string()))?;
    Ok(StockSpec {
        code: StockCode(code.to_owned()),
        exchange: if code.starts_with('6') {
            StockExchange::Shanghai
        } else {
            StockExchange::Shenzhen
        },
        initial_price: Money::from_cents(price),
        category,
        limit_pct: category.limit_pct(),
        tick: Money::from_cents(1),
        total_shares: total,
        float_shares: float,
    })
}
