//! #15 的 Session 组合边界：领域关账失败不能安装 candidate，也不能提前披露。

use super::*;
use crate::accounting::{AccountingPeriod, PeriodStatus};
use std::sync::atomic::{AtomicUsize, Ordering};

fn closed_month_end() -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::calendar::CivilDate::from_iso("2030-03-31").unwrap();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 1;
    setup.npcs.hot_count = 1;
    setup.ticks_per_day = 6;
    setup.history_len = 1;
    GameSession::new(setup, 42).unwrap()
}

#[test]
fn company_close_failure_discards_closed_books_registry_and_disclosure_candidate() {
    static OBSERVED: AtomicUsize = AtomicUsize::new(0);
    fn observer(_: crate::calendar::CivilInstant) { OBSERVED.fetch_add(1, Ordering::SeqCst); }
    OBSERVED.store(0, Ordering::SeqCst);
    let mut session = closed_month_end();
    session.state.civil_clock.add_disclosure_observer(observer);
    // 真实集团入口在单体已关账后失败：未知 root 不是人工吞错/伪造成功。
    session.state.groups.push(GroupStructure { root: crate::company::CompanyId(u32::MAX), holdings: vec![] });
    let before = serde_json::to_vec(&session.save().unwrap()).unwrap();
    let old_ops = session.state.operations.clone();
    let old_closing = session.state.closing.clone();
    let old_library = session.state.library.clone();
    let old_date = session.civil_date();
    let period = AccountingPeriod::of_date(old_date);
    let error = session.end_civil_day().expect_err("集团关账错误必须丢弃整个日结 candidate");
    assert!(matches!(error, SessionError::Information(_)), "真实领域原因必须透传：{error}");
    assert_eq!(serde_json::to_vec(&session.save().unwrap()).unwrap(), before,
        "公司分录/封账登记/时钟/RNG/个人信息/公共材料必须全部回滚");
    assert_eq!(session.civil_date(), old_date);
    assert_eq!(OBSERVED.load(Ordering::SeqCst), 0, "失败 candidate 不能触发18:00观察者");
    assert_eq!(serde_json::to_vec(session.state.operations.as_ref()).unwrap(), serde_json::to_vec(old_ops.as_ref()).unwrap());
    assert_eq!(serde_json::to_vec(session.state.closing.as_ref()).unwrap(), serde_json::to_vec(old_closing.as_ref()).unwrap());
    assert_eq!(serde_json::to_vec(session.state.library.as_ref()).unwrap(), serde_json::to_vec(old_library.as_ref()).unwrap());
    for (_, company) in &old_ops.companies {
        assert_eq!(company.books().books().journal().period_status(period), PeriodStatus::Open,
            "旧 snapshot 仍处于开放期间，不能被失败关账改写");
    }
    session.state.groups.clear();
    let report = session.end_civil_day().expect("修正上下文后可正常收口同一天");
    assert_eq!(report.settled_date, old_date);
    assert_eq!(OBSERVED.load(Ordering::SeqCst), 1, "成功关账后披露相位恰好一次");
    for (_, company) in &session.state.operations.companies {
        assert_eq!(company.books().books().journal().period_status(period), PeriodStatus::Closed);
    }
    assert!(report.events.iter().all(|event| !matches!(event, Event::Trade { .. } | Event::PriceTick { .. })),
        "休市经营/关账/披露不能伪造市场 tick 或成交");
}

#[test]
fn domain_report_preparation_does_not_publish_or_grant_npc_knowledge() {
    let mut session = closed_month_end();
    let company = session.state.operations.catalog().id("C-600888").unwrap();
    let period = AccountingPeriod::from_ymd(2029, 12).unwrap();
    let library_before = serde_json::to_vec(session.state.library.as_ref()).unwrap();
    let known_before: Vec<_> = session.state.belief_participants.iter().map(|(id, participant)|
        (*id, serde_json::to_value(participant.information()).unwrap())
    ).collect();
    let registered = session.state.operations.prepare_report_for_publication(
        company, period, crate::accounting::reports::ReportKind::Annual,
        std::sync::Arc::make_mut(&mut session.state.closing),
    ).unwrap();
    assert_eq!(registered.scope, crate::accounting::consolidation::ScopeId::Standalone(
        crate::accounting::consolidation::MemberId(company.0)));
    assert_eq!(serde_json::to_vec(session.state.library.as_ref()).unwrap(), library_before,
        "公司定稿准备不得把材料自动变成公开披露");
    let known_after: Vec<_> = session.state.belief_participants.iter().map(|(id, participant)|
        (*id, serde_json::to_value(participant.information()).unwrap())
    ).collect();
    assert_eq!(known_after, known_before, "尚未发布的定稿不能成为 NPC 本人获知");
    match session.state.operations.prepare_report_for_publication(
        crate::company::CompanyId(u32::MAX), period, crate::accounting::reports::ReportKind::Annual,
        std::sync::Arc::make_mut(&mut session.state.closing),
    ) {
        Err(crate::company::operations::CompanyCloseError::Fault(fault)) => {
            assert_eq!(fault.code, 4002);
            assert_eq!(fault.context, vec![u64::from(u32::MAX)]);
        }
        _ => panic!("未知公司必须以数值 fault4002 拒绝"),
    }
}

#[test]
fn civil_company_close_preserves_all_four_trader_identities_and_resources() {
    let mut session = closed_month_end();
    let before: Vec<_> = session.state.accounts.iter().map(|(id, account)|
        (*id, account.kind(), account.cash(), serde_json::to_value(account.positions()).unwrap())
    ).collect();
    assert_eq!(before.len(), 4, "必须真实覆盖玩家/散户/机构/游资四类账户");
    session.end_civil_day().unwrap();
    let after: Vec<_> = session.state.accounts.iter().map(|(id, account)|
        (*id, account.kind(), account.cash(), serde_json::to_value(account.positions()).unwrap())
    ).collect();
    assert_eq!(after, before, "公司现金、应计费用和债务关账不能覆盖投资者现金/股份或账户身份");
    assert_eq!(session.tick(), 0, "自然日日结不是市场 tick");
}
