use engine::session::pipeline::*;
use engine::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1);
    if mode.as_deref() == Some("--help") {
        println!("pipeline_contract [--duplicate]: compatibility traversal and identity contracts");
        return Ok(());
    }
    if mode.is_some() && mode.as_deref() != Some("--duplicate") {
        return Err("unknown argument; use --help".into());
    }
    let setup = SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600001".into()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
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
        float_allocation: FloatAllocation::Random,
        start_date: CivilDate::from_iso("2030-01-02")?,
        simulation_policy_id: SIMULATION_POLICY_ID_V1.into(),
    };
    let mut game = GameSession::new(setup, 42)?;
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockCode("600001".into()),
            side: Side::Buy,
            price: Money::from_cents(1000),
            qty: 100,
        },
    )?;
    let plan = plan_tick(PhaseInput { session: &game })?;
    println!("planned compatibility trace: {:?}", plan.trace());
    drop(plan);
    let events = game.step()?;
    println!(
        "public step committed tick {} through P9; actual single-commit trace covered by unit test",
        game.snapshot().tick
    );
    let mut keyed = EventKeyStream::default().attach_legacy_emission(&events)?;
    println!("stable identities: {keyed:?}");
    if mode.as_deref() == Some("--duplicate") {
        if let Some(first) = keyed.first().cloned() {
            keyed.push(first);
        }
        validate_event_keys(&keyed)?;
        return Err("duplicate probe unexpectedly accepted".into());
    }
    let mut expected = keyed.clone();
    expected.sort_by(|left, right| left.key.cmp(&right.key));
    keyed.reverse();
    keyed.sort_by(|left, right| left.key.cmp(&right.key));
    println!(
        "reversed attached fact multiset equal: {}",
        keyed == expected
    );
    let envelope = EnvelopeKey {
        account: AccountId(0),
        stock: StockCode("600001".into()),
        order: OrderId(1),
        side: Side::Buy,
    };
    let mut receipts = [
        ReceiptSource::DayEnd(0),
        ReceiptSource::Auction(0),
        ReceiptSource::SealedIntent(0),
        ReceiptSource::P0Expiry(0),
    ]
    .into_iter()
    .map(|source| {
        ReceiptLocalKey::new(
            source.journal(),
            source,
            ReceiptTransition {
                envelope: envelope.clone(),
                ordinal: 0,
            },
        )
    })
    .collect::<Result<Vec<_>, _>>()?;
    receipts.sort();
    println!("contract-only receipt key samples, no settlement: {receipts:?}");
    Ok(())
}
