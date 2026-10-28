//! 内部 World 的真实新增获知必须与同次 Acquisition 完整对应；不借外持 driver 串行分支。
use super::*;
use crate::diagnostics::causal::{CausalFactKind, FactTime};
use crate::information::{AcquiredKind, NpcInformationState, PublicLibrary};

// 由真实 restore 产生不含证券 root 的 ObservationRestart；Closed 日不伪造 TickArc/报价事实。
fn closed_runtime_information_session_with_restart() -> GameSession {
    let original = closed_runtime_information_session();
    let save = original.save().unwrap();
    drop(original);
    let mut game = GameSession::restore(&save).unwrap();
    let prefix = game.causal_facts();
    assert_eq!(
        prefix.len(),
        1,
        "真实 restore 只产生一次 ObservationRestart prefix"
    );
    assert!(matches!(
        &prefix[0].kind,
        CausalFactKind::ObservationRestart
    ));
    assert!(
        prefix[0].time.stock_clock.is_none(),
        "非证券 restore root 不伪造证券时钟"
    );
    game.construct_runtime_driver_lazy_for_test().unwrap();
    game
}

fn rows(game: &GameSession) -> Vec<(u64, FactTime, CausalFactKind)> {
    game.causal_facts()
        .iter()
        .map(|fact| (fact.sequence, fact.time, fact.kind.clone()))
        .collect()
}

fn expected_acquisitions(
    before: &NpcInformationState,
    after: &NpcInformationState,
    library: &PublicLibrary,
    through: crate::CivilInstant,
) -> Vec<CausalFactKind> {
    assert_eq!(before.owner(), after.owner());
    let mut expected = Vec::new();
    for (company, records) in after.companies() {
        for record in records {
            if before.observed_at_of(record.id).is_some() {
                continue;
            }
            assert_eq!(
                record.observed_at, through,
                "新增获知必须来自本次真实披露窗口"
            );
            let published = match record.kind {
                AcquiredKind::Report => library.report(record.id, through).unwrap().published_at,
                AcquiredKind::Announcement => {
                    library
                        .announcement(record.id, through)
                        .unwrap()
                        .published_at
                }
            };
            expected.push(CausalFactKind::Acquisition {
                account: after.owner(),
                company: *company,
                publication: u64::from(record.id.value()),
                published,
                acquired: record.observed_at,
            });
        }
    }
    expected
}

fn assert_new_rows(
    actual: &[(u64, FactTime, CausalFactKind)],
    prefix: &[(u64, FactTime, CausalFactKind)],
    kinds: Vec<CausalFactKind>,
    mut time: FactTime,
    through: crate::CivilInstant,
) {
    assert!(actual.len() >= prefix.len(), "原 causal prefix 不得消失");
    assert_eq!(
        &actual[..prefix.len()],
        prefix,
        "原 causal prefix 必须逐条保持"
    );
    time.civil = through;
    let expected: Vec<_> = kinds
        .into_iter()
        .enumerate()
        .map(|(index, kind)| ((prefix.len() + index) as u64, time, kind))
        .collect();
    assert_eq!(
        &actual[prefix.len()..],
        expected.as_slice(),
        "内部 native INFO 的真实新增获知必须逐条发布完整 Acquisition"
    );
}

#[test]
fn native_closed_disclosure_publishes_exact_acquisitions_from_real_group_delta() {
    let mut game = closed_runtime_information_session_with_restart();
    assert!(game.runtime_driver.is_some());
    // 真实 restore prefix 必须在 INFO 追加后逐条保留。
    let prefix = rows(&game);
    assert!(!prefix.is_empty());
    let before = actual_group_personal(&game);
    let time = game
        .causal_static_time()
        .expect("合法 Closed 日 civil 时间不需要证券 TickArc");
    let report = game.end_civil_day().unwrap();
    let after = actual_group_personal(&game);
    let expected = expected_acquisitions(
        &before.information,
        &after.information,
        game.state.library.as_ref(),
        report.disclosure_instant,
    );
    assert!(
        !expected.is_empty(),
        "必须先在真实 Group 观察到本次非空新增获知"
    );
    assert_eq!(
        game.state.belief_participants[&AccountId(1)].information(),
        &after.information
    );
    assert_new_rows(
        &rows(&game),
        &prefix,
        expected,
        time,
        report.disclosure_instant,
    );

    // 下一真实休市日只允许追加相对原个人态的新增获知，不去重掩盖重复发布。
    let prefix = rows(&game);
    let time = game
        .causal_static_time()
        .expect("合法 Closed 日 civil 时间不需要证券 TickArc");
    let next = game.end_civil_day().unwrap();
    let next_personal = actual_group_personal(&game);
    let expected = expected_acquisitions(
        &after.information,
        &next_personal.information,
        game.state.library.as_ref(),
        next.disclosure_instant,
    );
    assert_new_rows(
        &rows(&game),
        &prefix,
        expected,
        time,
        next.disclosure_instant,
    );
}

#[test]
fn native_civil_acquisitions_are_visible_in_candidate_and_abort_publishes_nothing() {
    let mut game = closed_runtime_information_session_with_restart();
    let prefix = rows(&game);
    assert!(!prefix.is_empty());
    let before = actual_group_personal(&game);
    let before_hash = game.business_state_hash().unwrap();
    let publication = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();
    let time = game
        .causal_static_time()
        .expect("合法 Closed 日 civil 时间不需要证券 TickArc");
    let fault = ValidationFault::new(1034, vec![73, 11]);
    let mut reached = false;
    let result: Result<(), SessionError> = game.with_prepared_civil_day(|candidate, report| {
        reached = true;
        let information = candidate.state.belief_participants[&AccountId(1)].information();
        let expected = expected_acquisitions(
            &before.information,
            information,
            candidate.state.library.as_ref(),
            report.disclosure_instant,
        );
        assert!(
            !expected.is_empty(),
            "callback 必须先收到真实非空 INFO 候选"
        );
        assert_new_rows(
            &rows(candidate),
            &prefix,
            expected,
            time,
            report.disclosure_instant,
        );
        Err(SessionError::InvalidSave(fault.clone()))
    });
    assert!(reached);
    assert!(matches!(result, Err(SessionError::InvalidSave(actual)) if actual == fault));
    assert_eq!(rows(&game), prefix, "Abort 不得发布候选 Acquisition");
    assert_eq!(actual_group_personal(&game), before);
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
    assert!(game.poison_reason().is_none());
    let after_publication = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();
    assert!(std::sync::Arc::ptr_eq(&publication, &after_publication));

    let report = game
        .end_civil_day()
        .expect("可逆 callback 拒绝后应允许同日正常重试");
    let after = actual_group_personal(&game);
    let expected = expected_acquisitions(
        &before.information,
        &after.information,
        game.state.library.as_ref(),
        report.disclosure_instant,
    );
    assert!(!expected.is_empty());
    assert_new_rows(
        &rows(&game),
        &prefix,
        expected,
        time,
        report.disclosure_instant,
    );
}
