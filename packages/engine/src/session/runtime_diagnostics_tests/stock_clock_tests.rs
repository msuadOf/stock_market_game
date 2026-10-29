//! StockFactClock 首红：仅检查真实 native 来源的 wire，不预先依赖新增类型。

use super::*;

#[test]
fn d3_native_real_submission_and_quotes_require_stock_clock_from_original_tick() {
    let mut game = game();
    enqueue(&mut game);
    let mut driver = driver(&mut game);
    let events = game
        .runtime_step_with_install(&mut driver)
        .expect("原合法限价申报必须先真实提交");
    let committed = driver
        .committed_calendar_tick()
        .unwrap()
        .expect("成功拍必须有原提交 TickArc");
    assert!(std::sync::Arc::ptr_eq(
        committed.calendar.day().source(),
        game.market_calendar_source()
    ));
    assert_eq!(committed.calendar.after_input().tick, game.tick());
    let code = StockId(0);
    let order = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                account: AccountId(0),
                code: actual,
                id,
                remaining_qty: 100,
                ..
            } if *actual == code => Some(*id),
            _ => None,
        })
        .expect("必须有原 OrderAccepted 与真实 OrderId，不能以合成事实触发 schema 红");
    let facts = game.causal_facts();
    assert_eq!(
        facts.len(),
        3,
        "原 fixture 恰为真实 before Quote、Submitted 和 after Quote"
    );
    assert!(matches!(&facts[1].kind, CausalFactKind::Submitted(origin)
        if origin.order == order && origin.account == AccountId(0) && origin.code == code && origin.qty == 100));
    assert!(
        matches!(&facts[0].kind, CausalFactKind::Quote(quote) if quote.code == code && quote.bid_depth == 0 && quote.ask_depth == 0)
    );
    assert!(
        matches!(&facts[2].kind, CausalFactKind::Quote(quote) if quote.code == code && quote.bid_depth == 100 && quote.ask_depth == 0)
    );
    let report = game
        .causal_diagnostics()
        .expect("首红前先保原真实生命周期对账");
    assert_eq!(
        (report.submitted_qty, report.filled_qty, report.open_qty),
        (100, 0, 100)
    );
    let projection = committed.calendar.before_projection();
    let stock = projection.stock(code).unwrap();
    assert_eq!(stock.date_session_ordinal(), Some(0));
    let expected = serde_json::json!({
        "code": code,
        "completed_minutes": stock.completed_minute_count().to_string(),
    });
    for fact in facts {
        let wire = serde_json::to_value(fact).unwrap();
        assert_eq!(
            wire["time"]["market_minute"],
            fact.time.market_minute.to_string()
        );
        assert_eq!(
            wire["time"]
                .get("stock_clock")
                .expect("真实证券 fact 必须携 StockFactClock，不得只给 shared minute"),
            &expected
        );
        assert_eq!(
            fact.time.market_minute,
            projection.completed_shared_minute_count()
        );
        assert_eq!(fact.time.phase, committed.calendar.phase_before());
        assert_eq!(
            fact.time.civil,
            committed.calendar.observation_instant_after()
        );
    }
}
