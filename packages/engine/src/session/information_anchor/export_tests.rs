//! 三种真实 Calendar 场景的 Rust serde 产物；供 Web strict-save 读取完整非空 Book。
use super::*;
use crate::observation::information_clock::InformationTradingAnchor;

#[test]
fn export_three_calendar_information_books_from_real_acquisition_and_cause() {
    use InformationTradingAnchor::{DayBefore, NoPriorSession, TradingDay};
    let mut exports = Vec::new();
    for (name, start, weekend, expected) in [
        ("no-prior", "2030-01-01", false, NoPriorSession),
        (
            "trading-zero",
            "2030-01-02",
            false,
            TradingDay { ordinal: 0 },
        ),
        (
            "day-before-zero",
            "2030-01-04",
            true,
            DayBefore { ordinal: 0 },
        ),
    ] {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::CivilDate::from_iso(start).unwrap();
        setup.ticks_per_day = 2;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        let mut session = super::super::GameSession::new(setup, 42).unwrap();
        let account = crate::AccountId(1);
        let stock = crate::StockId(0);
        // 无持仓/关注/到期观察；先走真实市场会话，避免预先产生不同阶段材料。
        let attention = session.state.npc_attention.get_mut(&account).unwrap();
        attention.base_probability = 0.0;
        attention.next_attention_candidate_tick = u64::MAX;
        if weekend {
            session.step().expect("Friday 真实首拍");
            session.step().expect("Friday 真实末拍");
            session.end_civil_day().expect("真实自然日进入 Saturday");
        }
        session
            .state
            .belief_participants
            .get_mut(&account)
            .unwrap()
            .watchlist_mut()
            .record_attention(&stock, 0, 0)
            .unwrap();
        let now = session.runtime_observation_instant();
        let (day, projection) = session.prepare_market_calendar_projection().unwrap();
        let anchors = information_trading_anchors(day.day_model(), &projection).unwrap();
        assert_eq!(anchors[&stock], expected);
        let participant = &session.state.belief_participants[&account];
        let facts = session.runtime_company_read_facts().unwrap();
        let market = session.build_market_view();
        let (_, acquired, book, _) = super::super::notices::deliver_information_for_npc(
            account,
            now,
            &|stock| anchors.get(&stock).copied(),
            &session.state.npc_attention[&account],
            participant.information(),
            participant.belief(),
            [stock].into(),
            &|stock| facts.issuer_of_stock(stock).ok(),
            &|company| facts.company_spec(company).ok(),
            &session.state.library,
            &market,
        )
        .expect("本人真实获取公开材料并形成 cause")
        .expect("观察 cadence 必须到期");
        let entry = book.entry(&stock).expect("必须导出非空完整 Book");
        assert_eq!(entry.anchor, expected);
        assert_eq!(entry.last_cause.as_ref().unwrap().as_of_anchor, expected);
        assert!(
            !entry.used_report_ids.is_empty(),
            "必须使用本人真实已获取报告"
        );
        assert!(acquired.acquired_count() > 0);
        exports.push(serde_json::json!({
            "scenario": name, "civil_date": session.civil_date(), "through": now,
            "tick": session.tick().to_string(), "anchor": anchors[&stock], "book": book,
        }));
    }
    let bytes = serde_json::to_vec_pretty(&exports).unwrap();
    if let Some(path) = std::env::var_os("STOCK_GAME_TYPED_INFO_EXPORT") {
        std::fs::write(path, &bytes).expect("真实 serde fixture 必须成功写出");
    }
    if let Some(path) = std::env::var_os("STOCK_GAME_TYPED_INFO_WEB_READBACK") {
        let returned: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("Web 回写 JSON 必须存在"))
                .expect("Web 回写必须是 JSON");
        let returned = returned.as_array().expect("Web 回写保持三个 scenario 数组");
        assert_eq!(returned.len(), exports.len());
        for (original, web) in exports.iter().zip(returned) {
            assert_eq!(web["scenario"], original["scenario"]);
            let expected: crate::strategy::BeliefBook =
                serde_json::from_value(original["book"].clone()).unwrap();
            let decoded: crate::strategy::BeliefBook = serde_json::from_value(web["book"].clone())
                .expect("Web 回写 Book 必须被 Rust strict serde 接受");
            assert_eq!(
                decoded, expected,
                "Web 往返必须保留完整 Book 与 typed cause/anchor"
            );
        }
    }
    let decoded: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded.as_array().unwrap().len(), 3);
}
