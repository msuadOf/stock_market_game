use super::*;
use crate::observation::information_clock::InformationTradingAnchor;
use InformationTradingAnchor::{DayBefore, NoPriorSession, TradingDay};

/// 可编辑存档仅注入最少信念事实；不声称此 entry 来自历史交易或真实 cause。
fn edited_entry(
    start: &str,
    anchor: InformationTradingAnchor,
    cause: InformationTradingAnchor,
) -> SaveSlot {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso(start).unwrap();
    let session = GameSession::new(setup, 42).unwrap();
    let save = session.save().unwrap();
    let account = *save.belief_books.keys().next().unwrap();
    let company = save.company_state.issuer_ids().into_iter().next().unwrap();
    let mut encoded = serde_json::to_value(save).unwrap();
    encoded["belief_books"][account.0.to_string()]["entries"]["0"] = serde_json::json!({
        "company": company,
        "method": null,
        "forecast": { "growth_bp": null, "basis": "InitialWithoutHistory" },
        "confidence_bp": 5000,
        "valuation": { "Unavailable": { "reason": "NonPositiveNetIncome" } },
        "used_report_ids": [],
        "anchor": anchor,
        "horizon_trading_days": 1,
        "last_cause": { "cause": "HorizonExpired", "as_of_anchor": cause },
        "applied_experience_orders": []
    });
    serde_json::from_value(encoded).unwrap()
}

#[test]
fn restore_information_anchor_uses_true_closed_frontier_before_first_session() {
    let legal = edited_entry("2030-01-01", NoPriorSession, NoPriorSession);
    GameSession::restore(&legal).expect("首会话前休市的 NoPrior 必须合法");
    for future in [TradingDay { ordinal: 0 }, DayBefore { ordinal: 0 }] {
        let invalid = edited_entry("2030-01-01", future, future);
        let error = GameSession::restore(&invalid)
            .err()
            .expect("非法锚必须被 restore 拒绝");
        assert!(
            matches!(error, SessionError::InvalidSave(ref fault)
            if fault.code == 2790 && fault.context[1] == 0 && fault.context[2] == 1),
            "{error:?}"
        );
    }
}

#[test]
fn restore_information_anchor_rejects_future_variant_and_max_without_banning_history() {
    for past in [NoPriorSession, TradingDay { ordinal: 0 }] {
        let legal = edited_entry("2030-01-02", past, past);
        GameSession::restore(&legal).expect("首开市可保留 NoPrior 或本日 Trading0");
    }
    for future in [
        DayBefore { ordinal: 0 },
        TradingDay { ordinal: u64::MAX },
        DayBefore { ordinal: u64::MAX },
    ] {
        let invalid = edited_entry("2030-01-02", future, future);
        let error = GameSession::restore(&invalid)
            .err()
            .expect("非法锚必须被 restore 拒绝");
        assert!(
            matches!(error, SessionError::InvalidSave(ref fault)
            if fault.code == 2790 && fault.context[1] == 0 && fault.context[2] == 1),
            "{error:?}"
        );
    }
}

#[test]
fn restore_information_anchor_rejects_future_cause_and_cause_before_entry() {
    for (anchor, cause) in [
        (TradingDay { ordinal: 0 }, NoPriorSession),
        (TradingDay { ordinal: 0 }, DayBefore { ordinal: 0 }),
    ] {
        let invalid = edited_entry("2030-01-02", anchor, cause);
        let error = GameSession::restore(&invalid)
            .err()
            .expect("非法锚必须被 restore 拒绝");
        assert!(
            matches!(error, SessionError::InvalidSave(ref fault)
            if fault.code == 2790 && fault.context[1] == 0 && fault.context[2] == 1),
            "{error:?}"
        );
    }
}

#[test]
fn restore_closed_after_real_first_session_accepts_trading_and_day_before_zero() {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso("2030-01-04").unwrap();
    setup.ticks_per_day = 2;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.step().expect("首个真实市场 tick");
    session.step().expect("首会话完成");
    session.end_civil_day().expect("自然日推进到 Saturday");
    let (day, projection) = session.prepare_market_calendar_projection().unwrap();
    let anchors =
        super::super::information_anchor::information_trading_anchors(day.day_model(), &projection)
            .unwrap();
    assert_eq!(anchors[&StockId(0)], DayBefore { ordinal: 0 });
    let mut actual = serde_json::to_value(session.save().unwrap()).unwrap();
    let template = edited_entry(
        "2030-01-04",
        TradingDay { ordinal: 0 },
        TradingDay { ordinal: 0 },
    );
    let account = *template.belief_books.keys().next().unwrap();
    let entry = serde_json::to_value(&template).unwrap()["belief_books"][account.0.to_string()]
        ["entries"]["0"]
        .clone();
    for historical in [TradingDay { ordinal: 0 }, DayBefore { ordinal: 0 }] {
        actual["belief_books"][account.0.to_string()]["entries"]["0"] = entry.clone();
        actual["belief_books"][account.0.to_string()]["entries"]["0"]["anchor"] =
            serde_json::to_value(historical).unwrap();
        actual["belief_books"][account.0.to_string()]["entries"]["0"]["last_cause"]
            ["as_of_anchor"] = serde_json::to_value(historical).unwrap();
        let save: SaveSlot = serde_json::from_value(actual.clone()).unwrap();
        GameSession::restore(&save).expect("Closed frontier 接受先前 Trading0 与当前 DayBefore0");
    }
}

#[test]
fn restore_anchor_fault_preserves_max_ordinal_without_js_number_loss() {
    let invalid = edited_entry(
        "2030-01-02",
        DayBefore { ordinal: u64::MAX },
        DayBefore { ordinal: u64::MAX },
    );
    let account = invalid.belief_books.keys().next().unwrap().0;
    let error = GameSession::restore(&invalid)
        .err()
        .expect("MAX 在小 frontier 下属于未来");
    assert!(
        matches!(error, SessionError::InvalidSave(ref fault)
        if fault.code == 2790 && fault.context == vec![account, 0, 1, 3, u32::MAX as u64, u32::MAX as u64, 3, u32::MAX as u64, u32::MAX as u64, 2, 0, 0]),
        "{error:?}"
    );
}

#[test]
fn restore_information_clock_projection_fault_preserves_expected_and_actual_counters() {
    let mut save = edited_entry("2030-01-01", NoPriorSession, NoPriorSession);
    save.snapshot.tick = 1;
    let catalog = save.setup.stock_catalog().unwrap();
    let error = validate_saved_information_anchors(&save, &catalog)
        .expect_err("Closed 日 tick 漂移必须精确拒绝");
    assert!(
        matches!(error, SessionError::InvalidSave(ref fault)
        if fault.code == 2791 && fault.context == vec![7, 1, save.setup.ticks_per_day, 0, 1, 0, 0]),
        "{error:?}"
    );
}
