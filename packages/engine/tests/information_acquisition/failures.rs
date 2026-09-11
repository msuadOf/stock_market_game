//! 类型化拒绝路径（任务 16，同 QA 命令覆盖）：
//! - 未来 publication / observed_at 早于 published_at（同一 EarlyRead 守卫的
//!   两种表述——报告面与公告面各锁一例）；
//! - 未知 report ID（UnknownPublication 透传）；
//! - 他人信息集注入（OwnerMismatch：登记面 + 上下文构造面）；
//! - 未获知读取（NotAcquired）与篡改存档（InconsistentState）。
//!
//! 注：`AcquisitionError` 透传的 `InformationError` 无 PartialEq（money 链
//! 既有约束，task-19 先例）——断言用 `matches!` 字段绑定，语义等价精确。

use crate::fixture::{hour_after, minute_before, npc_a, npc_b, publish_correction, scenario};
use engine::information::{
    AcquiredKind, AcquisitionError, AcquisitionRecord, NpcInformationState,
    NpcInformationStateSave, NpcObservationContext,
};

/// 未来 publication（报告面）：observed_at 早于 published_at ⇒ EarlyRead
/// 透传（携带公布时点与获知时点的完整上下文）。
#[test]
fn future_report_publication_rejected() {
    let sc = scenario();
    let a = npc_a();
    let mut state = NpcInformationState::new(a);
    let early = minute_before(sc.annual_instant);

    let err = state
        .record_acquisition(a, &sc.library, sc.annual_v1_id, early)
        .expect_err("observed_at before published_at must be rejected");
    assert!(matches!(
        &err,
        AcquisitionError::Library(engine::information::InformationError::EarlyRead {
            id, published_at, as_of
        })
            if *id == sc.annual_v1_id && *published_at == sc.annual_instant && *as_of == early
    ));
    assert!(
        state.records_for_company(&sc.company).is_empty(),
        "rejected acquisition leaves no record"
    );
}

/// 未来 publication（公告面）：同守卫对临时公告同样生效。
#[test]
fn future_announcement_publication_rejected() {
    let sc = scenario();
    let a = npc_a();
    let mut state = NpcInformationState::new(a);
    let early = minute_before(sc.announcement_instant);

    let err = state
        .record_acquisition(a, &sc.library, sc.announcement_id, early)
        .expect_err("announcement early read must be rejected");
    assert!(matches!(
        &err,
        AcquisitionError::Library(engine::information::InformationError::EarlyRead {
            published_at, as_of, ..
        })
            if *published_at == sc.announcement_instant && *as_of == early
    ));
}

/// 未知 report ID：库中不存在（既非报告也非公告）⇒ UnknownPublication 透传。
#[test]
fn unknown_publication_rejected() {
    let sc = scenario();
    let a = npc_a();
    let mut state = NpcInformationState::new(a);
    let unknown = engine::information::PublicationId::new(9_999);

    let err = state
        .record_acquisition(a, &sc.library, unknown, hour_after(sc.annual_instant))
        .expect_err("unknown id must be rejected");
    assert!(matches!(
        &err,
        AcquisitionError::Library(engine::information::InformationError::UnknownPublication { id })
            if *id == unknown
    ));
}

/// 他人信息集注入：状态属主与调用方 NPC 不一致 ⇒ OwnerMismatch（登记面与
/// 上下文构造面同一守卫；不存在跨 NPC 合并/注入 API）。
#[test]
fn cross_npc_injection_rejected() {
    let sc = scenario();
    let (a, b) = (npc_a(), npc_b());
    let mut state_a = NpcInformationState::new(a);
    let market = crate::fixture::FixtureMarket::quiet();

    // 登记面：以乙的名义向甲的状态登记 ⇒ 拒绝（且不产生记录）。
    let err = state_a
        .record_acquisition(
            b,
            &sc.library,
            sc.annual_v1_id,
            hour_after(sc.annual_instant),
        )
        .expect_err("cross-npc injection must be rejected");
    assert!(matches!(
        &err,
        AcquisitionError::OwnerMismatch { state_owner, caller } if *state_owner == a && *caller == b
    ));
    assert_eq!(state_a.acquired_count(), 0);

    // 甲的合法获知之后，构造面仍拒绝以乙的名义使用甲的状态。
    state_a
        .record_acquisition(
            a,
            &sc.library,
            sc.annual_v1_id,
            hour_after(sc.annual_instant),
        )
        .expect("owner-consistent acquisition");
    let err = match NpcObservationContext::new(b, &state_a, &sc.library, &market) {
        Err(err) => err,
        Ok(_) => panic!("building b's context from a's state must be rejected"),
    };
    assert!(matches!(
        &err,
        AcquisitionError::OwnerMismatch { state_owner, caller } if *state_owner == a && *caller == b
    ));

    // 乙自己的状态照常可用且为空（隔离，无泄漏）。
    let state_b = NpcInformationState::new(b);
    let ctx_b = NpcObservationContext::new(b, &state_b, &sc.library, &market)
        .expect("b's own state builds a context");
    assert!(ctx_b.acquired_reports().is_empty());
}

/// 未获知读取：库中存在、但本人未登记获知 ⇒ NotAcquired（无前视：候选
/// 索引/公开 ≠ 本人已读）。
#[test]
fn unacquired_read_rejected() {
    let mut sc = scenario();
    let a = npc_a();
    let state = NpcInformationState::new(a);
    let market = crate::fixture::FixtureMarket::quiet();
    let v2_id = publish_correction(&mut sc);

    let ctx = NpcObservationContext::new(a, &state, &sc.library, &market)
        .expect("owner-consistent context");
    let err = ctx
        .report(v2_id)
        .expect_err("unacquired report must be unreadable");
    assert!(matches!(
        &err,
        AcquisitionError::NotAcquired { npc, id } if *npc == a && *id == v2_id
    ));
}

/// 篡改存档：公司内 id 非严格递增（重复）或跨公司重复 ⇒ InconsistentState
/// （严格递增是二分查找正确性的前提，恢复边界必须显式拒绝，绝不静默）。
#[test]
fn tampered_restore_rejected() {
    let sc = scenario();
    let a = npc_a();
    let t = hour_after(sc.annual_instant);
    let record = || AcquisitionRecord {
        id: sc.annual_v1_id,
        observed_at: t,
        kind: AcquiredKind::Report,
    };
    let company = sc.company.clone();

    // 公司内重复 id。
    let duplicated = NpcInformationStateSave {
        owner: a,
        companies: [(company.clone(), vec![record(), record()])]
            .into_iter()
            .collect(),
    };
    assert!(matches!(
        NpcInformationState::from_parts(duplicated).unwrap_err(),
        AcquisitionError::InconsistentState { .. }
    ));

    // 公司内乱序（非严格递增）。
    let later = AcquisitionRecord {
        id: engine::information::PublicationId::new(sc.annual_v1_id.value() + 1),
        observed_at: t,
        kind: AcquiredKind::Report,
    };
    let unsorted = NpcInformationStateSave {
        owner: a,
        companies: [(company.clone(), vec![later, record()])]
            .into_iter()
            .collect(),
    };
    assert!(matches!(
        NpcInformationState::from_parts(unsorted).unwrap_err(),
        AcquisitionError::InconsistentState { .. }
    ));

    // 跨公司重复 id（公开库 id 全局唯一 ⇒ 同 id 挂两公司 = 篡改）。
    let other = engine::company::CompanyId("C-OTHER".to_string());
    let cross = NpcInformationStateSave {
        owner: a,
        companies: [(company, vec![record()]), (other, vec![record()])]
            .into_iter()
            .collect(),
    };
    assert!(matches!(
        NpcInformationState::from_parts(cross).unwrap_err(),
        AcquisitionError::InconsistentState { .. }
    ));

    // serde 边界同样显式失败（自定义 Deserialize → from_parts）。
    let bad = serde_json::to_string(&{
        let mut save = NpcInformationStateSave {
            owner: a,
            companies: [(
                sc.company.clone(),
                vec![AcquisitionRecord {
                    id: sc.announcement_id,
                    observed_at: t,
                    kind: AcquiredKind::Announcement,
                }],
            )]
            .into_iter()
            .collect(),
        };
        // announcement id < annual id ⇒ 非严格递增（按 id 排序应为 annual 在前）。
        save.companies
            .get_mut(&sc.company)
            .unwrap()
            .push(AcquisitionRecord {
                id: sc.annual_v1_id,
                observed_at: t,
                kind: AcquiredKind::Report,
            });
        save
    })
    .expect("tampered save serializes");
    assert!(
        serde_json::from_str::<NpcInformationState>(&bad).is_err(),
        "serde boundary surfaces the typed rejection"
    );
}

/// 上下文构造的最小面（类型面不可达 CompanyState/Books：构造输入只有
/// 属主 + 获知状态 + 公开库 + 行情快照四样，见 npc_view 模块文档）。
#[test]
fn context_surface_only_consumes_information_state_and_library() {
    let sc = scenario();
    let a = npc_a();
    let state = NpcInformationState::new(a);
    let market = crate::fixture::FixtureMarket::quiet();
    let ctx = NpcObservationContext::new(a, &state, &sc.library, &market)
        .expect("context builds from (npc, state, library, market) only");
    assert_eq!(ctx.npc(), a);
    assert_eq!(ctx.market(), &market);
    assert_eq!(
        ctx.market().last_close_cents.get(crate::fixture::COMPANY),
        Some(&1_000),
        "visible market inputs are carried by reference"
    );
}
