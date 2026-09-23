//! 获知金样（K4 / 任务 16）：公共曝光与个人已知信息分离。
//!
//! 核心验收（无前视）：改变**未披露事实**、或**已披露但本人未读**的内容，
//! 不改变未观察 NPC 的观察前判断输入——以观察上下文内容的字节投影断言。

use crate::fixture::{
    hour_after, npc_a, npc_b, post_undisclosed_fact, publish_correction, scenario, FixtureMarket,
    COMPANY,
};
use engine::company::CompanyId;
use engine::information::{
    AcquisitionError, AcquisitionOutcome, NpcInformationState, NpcObservationContext, PublicLibrary,
};
use engine::orderbook::AccountId;

pub(crate) fn build_ctx<'a>(
    npc: AccountId,
    state: &'a NpcInformationState,
    library: &'a PublicLibrary,
    market: &'a FixtureMarket,
) -> NpcObservationContext<'a, FixtureMarket> {
    NpcObservationContext::new(npc, state, library, market)
        .expect("owner-consistent context builds")
}

pub(crate) fn state_bytes(state: &NpcInformationState) -> String {
    serde_json::to_string(state).expect("state serializes")
}

/// 判定输入的确定性字节投影：报告在前、公告在后（各按公司序 + id 序），
/// 每条 = 公司|id|首次获知时点 + 内容 serde 字节（上下文可读的全部材料）。
pub(crate) fn judgment_projection(ctx: &NpcObservationContext<'_, FixtureMarket>) -> String {
    let mut parts = Vec::new();
    for entry in ctx.acquired_reports() {
        parts.push(format!(
            "{:?}|{}|{:?}",
            entry.company,
            entry.id.value(),
            entry.observed_at
        ));
        parts.push(
            serde_json::to_string(ctx.report(entry.id).expect("acquired report readable"))
                .expect("report content serializes"),
        );
    }
    for entry in ctx.acquired_announcements() {
        parts.push(format!(
            "{:?}|{}|{:?}",
            entry.company,
            entry.id.value(),
            entry.observed_at
        ));
        parts.push(
            serde_json::to_string(
                ctx.announcement(entry.id)
                    .expect("acquired announcement readable"),
            )
            .expect("announcement content serializes"),
        );
    }
    parts.join("\n")
}

/// 金样 1（核心验收）：逐人延迟获知 + 无前视。甲读了年报 v1 与公告、乙
/// 未读——乙的观察上下文不含它们；改变未披露事实（开放期间新分录）或
/// 已披露但本人未读的内容（更正 v2 公开），乙的观察前判断输入字节不变。
#[test]
fn unread_publication_never_enters_unread_npc_inputs() {
    let mut sc = scenario();
    let (a, b) = (npc_a(), npc_b());
    let mut state_a = NpcInformationState::new(a);
    let mut state_b = NpcInformationState::new(b);
    let market = FixtureMarket::quiet();

    // 甲获知年报 v1 与公告；乙未获知任何内容（公共曝光 ≠ 个人阅读）。
    state_a
        .record_acquisition(
            a,
            &sc.library,
            sc.annual_v1_id,
            hour_after(sc.annual_instant),
        )
        .expect("npc a reads annual v1");
    state_a
        .record_acquisition(
            a,
            &sc.library,
            sc.announcement_id,
            hour_after(sc.announcement_instant),
        )
        .expect("npc a reads announcement");

    let ctx_b = build_ctx(b, &state_b, &sc.library, &market);
    assert!(
        ctx_b.acquired_reports().is_empty() && ctx_b.acquired_announcements().is_empty(),
        "b knows nothing before any acquisition"
    );
    assert!(matches!(
        ctx_b.report(sc.annual_v1_id).unwrap_err(),
        AcquisitionError::NotAcquired { .. }
    ));
    let baseline = judgment_projection(&ctx_b);

    // —— 未披露事实变更：开放期间新分录（总账变了、未公开）——
    post_undisclosed_fact(&mut sc);
    assert_eq!(
        judgment_projection(&build_ctx(b, &state_b, &sc.library, &market)),
        baseline,
        "undisclosed ledger facts must not move b's judgment inputs"
    );

    // —— 已披露但本人未读：更正 v2 公开，乙仍未阅读 ——
    let v2_id = publish_correction(&mut sc);
    assert_eq!(
        judgment_projection(&build_ctx(b, &state_b, &sc.library, &market)),
        baseline,
        "published-but-personally-unread correction must not move b's judgment inputs"
    );
    assert!(matches!(
        build_ctx(b, &state_b, &sc.library, &market)
            .report(v2_id)
            .unwrap_err(),
        AcquisitionError::NotAcquired { .. }
    ));

    // —— 区分力对照：甲获知 v2 后，甲的判断输入集合发生变化 ——
    let before_a = judgment_projection(&build_ctx(a, &state_a, &sc.library, &market));
    state_a
        .record_acquisition(a, &sc.library, v2_id, hour_after(sc.correction_instant))
        .expect("npc a reads correction");
    assert_ne!(
        judgment_projection(&build_ctx(a, &state_a, &sc.library, &market)),
        before_a,
        "a real acquisition must move a's inputs (the test has discriminating power)"
    );

    // —— 逐人延迟获知：乙更晚才首次阅读年报 v1，只改变乙本人的输入 ——
    state_b
        .record_acquisition(
            b,
            &sc.library,
            sc.annual_v1_id,
            hour_after(sc.correction_instant),
        )
        .expect("npc b reads annual v1 late");
    let ctx_b = build_ctx(b, &state_b, &sc.library, &market);
    assert_eq!(ctx_b.acquired_reports().len(), 1);
    assert_ne!(
        judgment_projection(&ctx_b),
        baseline,
        "delayed first read moves only b's inputs"
    );
}

/// 金样 2：重复阅读——一次获知只记一次（幂等，保留首次时点，状态字节不变）。
#[test]
fn repeat_acquisition_is_recorded_once() {
    let sc = scenario();
    let a = npc_a();
    let mut state = NpcInformationState::new(a);
    let t1 = hour_after(sc.annual_instant);

    match state
        .record_acquisition(a, &sc.library, sc.annual_v1_id, t1)
        .expect("first acquisition")
    {
        AcquisitionOutcome::Recorded {
            company,
            observed_at,
        } => {
            assert_eq!(company, sc.company);
            assert_eq!(observed_at, t1);
        }
        AcquisitionOutcome::AlreadyAcquired { .. } => panic!("first acquisition must record"),
    }
    let bytes = state_bytes(&state);

    // 同 id 更晚的重复阅读：合法幂等输入，不重记、不重写首次时点。
    let t2 = hour_after(hour_after(sc.annual_instant));
    match state
        .record_acquisition(a, &sc.library, sc.annual_v1_id, t2)
        .expect("duplicate acquisition is idempotent")
    {
        AcquisitionOutcome::AlreadyAcquired {
            company,
            first_observed_at,
        } => {
            assert_eq!(company, sc.company);
            assert_eq!(first_observed_at, t1, "first observation instant wins");
        }
        AcquisitionOutcome::Recorded { .. } => panic!("duplicate must not re-record"),
    }
    assert_eq!(
        state_bytes(&state),
        bytes,
        "one acquisition = one record; state byte-unchanged across the duplicate"
    );
}

/// 金样 6：存档往返——获知登记 serde 字节往返一致；恢复态可继续构造上下文。
#[test]
fn state_serde_round_trip_preserves_acquisitions() {
    let sc = scenario();
    let a = npc_a();
    let mut state = NpcInformationState::new(a);
    let t1 = hour_after(sc.annual_instant);
    let t2 = hour_after(sc.announcement_instant);
    state
        .record_acquisition(a, &sc.library, sc.annual_v1_id, t1)
        .expect("acquire annual");
    state
        .record_acquisition(a, &sc.library, sc.announcement_id, t2)
        .expect("acquire announcement");

    let bytes = serde_json::to_string(&state).expect("state serializes");
    let restored: NpcInformationState = serde_json::from_str(&bytes).expect("state restores");
    assert_eq!(
        serde_json::to_string(&restored).expect("reserializes"),
        bytes
    );
    assert_eq!(restored.owner(), a);
    assert_eq!(restored.observed_at_of(sc.annual_v1_id), Some(t1));
    assert_eq!(restored.observed_at_of(sc.announcement_id), Some(t2));
    assert_eq!(
        restored
            .records_for_company(&CompanyId(COMPANY.to_string()))
            .len(),
        2,
        "per-company registry keeps both acquisitions"
    );

    // 恢复态直接构造上下文照常工作（引用面不依赖克隆）。
    let market = FixtureMarket::quiet();
    let ctx = NpcObservationContext::new(a, &restored, &sc.library, &market)
        .expect("restored state builds a context");
    assert_eq!(ctx.acquired_reports().len(), 1);
    assert_eq!(ctx.acquired_announcements().len(), 1);
    assert_eq!(
        ctx.report(sc.annual_v1_id).expect("readable").company,
        sc.company
    );
}
