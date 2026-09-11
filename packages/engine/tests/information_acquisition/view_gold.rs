//! 观察上下文金样（K4 / 任务 16）：历史版本钉死、dev 查看分离、公共曝光
//! 发现面。共享夹具在 `fixture`；字节投影/上下文构造助手在
//! `acquisition_gold`（同一测试 crate 内复用）。

use crate::acquisition_gold::{build_ctx, judgment_projection, state_bytes};
use crate::fixture::{
    hour_after, minute_before, npc_a, npc_b, publish_correction, scenario, FixtureMarket,
};
use engine::accounting::reports::ReportKind;
use engine::accounting::AccountingPeriod;
use engine::information::{
    discovery_candidates, AcquisitionError, NpcInformationState, NpcObservationContext,
};

/// 金样：历史版本——获知时点钉死版本。更正（新 id）公布后，上下文仍
/// 暴露获知时的 v1（内容逐字节不变）；v2 只有经新的获知事件才可读。
#[test]
fn acquired_version_pinned_across_later_correction() {
    let mut sc = scenario();
    let a = npc_a();
    let mut state = NpcInformationState::new(a);
    let market = FixtureMarket::quiet();
    state
        .record_acquisition(
            a,
            &sc.library,
            sc.annual_v1_id,
            hour_after(sc.annual_instant),
        )
        .expect("acquire v1");

    let v1_bytes = serde_json::to_string(
        build_ctx(a, &state, &sc.library, &market)
            .report(sc.annual_v1_id)
            .expect("v1 readable"),
    )
    .expect("v1 serializes");
    let projection_before = judgment_projection(&build_ctx(a, &state, &sc.library, &market));

    let v2_id = publish_correction(&mut sc);

    // 更正公开本身不移动甲的判断输入；v1 内容按获知时点钉死（字节不变）。
    let ctx = build_ctx(a, &state, &sc.library, &market);
    assert_eq!(
        serde_json::to_string(ctx.report(sc.annual_v1_id).expect("v1 still readable"))
            .expect("v1 serializes"),
        v1_bytes
    );
    assert_eq!(
        judgment_projection(&ctx),
        projection_before,
        "a later correction alone must not move a's inputs"
    );
    assert!(matches!(
        ctx.report(v2_id).unwrap_err(),
        AcquisitionError::NotAcquired { .. }
    ));

    // 新的获知事件后 v2 才可读；v1 引用依然原样（不可变历史，不覆写）。
    state
        .record_acquisition(a, &sc.library, v2_id, hour_after(sc.correction_instant))
        .expect("acquire v2");
    let ctx = build_ctx(a, &state, &sc.library, &market);
    let v2_report = ctx
        .report(v2_id)
        .expect("v2 readable after new acquisition");
    assert_eq!(v2_report.supersedes, Some(sc.annual_v1_id));
    assert_eq!(
        ctx.report(sc.annual_v1_id).expect("v1 immutable").id,
        sc.annual_v1_id
    );
}

/// 金样：dev 查看分离——宿主/dev 检查面（公开库查询 + 候选索引，全部
/// 只读 &self）不写入任何 NPC 的信息状态（字节对比），也不产生幻影获知。
#[test]
fn dev_reads_leave_every_npc_state_unchanged() {
    let mut sc = scenario();
    let (a, b) = (npc_a(), npc_b());
    let mut state_a = NpcInformationState::new(a);
    let mut state_b = NpcInformationState::new(b);
    let market = FixtureMarket::quiet();
    state_a
        .record_acquisition(
            a,
            &sc.library,
            sc.annual_v1_id,
            hour_after(sc.annual_instant),
        )
        .expect("a reads annual");
    state_b
        .record_acquisition(
            b,
            &sc.library,
            sc.announcement_id,
            hour_after(sc.announcement_instant),
        )
        .expect("b reads announcement");
    let v2_id = publish_correction(&mut sc);

    let (bytes_a, bytes_b) = (state_bytes(&state_a), state_bytes(&state_b));
    let (proj_a, proj_b) = (
        judgment_projection(&build_ctx(a, &state_a, &sc.library, &market)),
        judgment_projection(&build_ctx(b, &state_b, &sc.library, &market)),
    );

    // dev/宿主查看面：任务 15 公开库查询 + 任务 16 候选索引（含以未来
    // 时点 as_of 的 dev 视角）——全部 &self。
    let far_future = hour_after(sc.correction_instant);
    let _ = sc
        .library
        .report(sc.annual_v1_id, far_future)
        .expect("dev reads v1");
    let _ = sc.library.report(v2_id, far_future).expect("dev reads v2");
    let _ = sc
        .library
        .announcement(sc.announcement_id, far_future)
        .expect("dev reads announcement");
    let _ = sc.library.reports_for_company(&sc.company, far_future);
    let _ = sc
        .library
        .announcements_for_company(&sc.company, far_future);
    let _ = sc.library.latest_report(
        &sc.company,
        ReportKind::Annual,
        AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
        far_future,
    );
    let _ = discovery_candidates(&sc.library, &sc.company, far_future);

    assert_eq!(
        (state_bytes(&state_a), state_bytes(&state_b)),
        (bytes_a, bytes_b),
        "dev views never write npc information state"
    );
    assert_eq!(
        judgment_projection(&build_ctx(a, &state_a, &sc.library, &market)),
        proj_a
    );
    assert_eq!(
        judgment_projection(&build_ctx(b, &state_b, &sc.library, &market)),
        proj_b
    );
}

/// 金样：公共曝光只改变发现机会——`discovery_candidates` 按 as_of 投影
/// 公开索引（id 序确定性），且候选**不构成阅读**（状态仍空、上下文不可读）。
#[test]
fn discovery_candidates_index_publications_without_reading() {
    let mut sc = scenario();
    let a = npc_a();
    let market = FixtureMarket::quiet();

    // 公布前：无候选（公共索引不含未来公布）。
    assert!(
        discovery_candidates(&sc.library, &sc.company, minute_before(sc.annual_instant)).is_empty(),
        "no candidates before the first publication"
    );

    // v1 公布后：v1 是候选（公告尚未发生）。
    assert_eq!(
        discovery_candidates(&sc.library, &sc.company, hour_after(sc.annual_instant)),
        vec![sc.annual_v1_id]
    );

    // 公告与更正都公开后：三条候选、id 序。
    let v2_id = publish_correction(&mut sc);
    let far_future = hour_after(sc.correction_instant);
    let mut expected = vec![sc.annual_v1_id, sc.announcement_id, v2_id];
    expected.sort_unstable_by_key(|id| id.value());
    assert_eq!(
        discovery_candidates(&sc.library, &sc.company, far_future),
        expected
    );

    // 候选 ≠ 阅读：状态为空 ⇒ 上下文无已知条目、候选不可读。
    let state = NpcInformationState::new(a);
    let ctx = NpcObservationContext::new(a, &state, &sc.library, &market).expect("context builds");
    assert!(ctx.acquired_reports().is_empty() && ctx.acquired_announcements().is_empty());
    assert!(matches!(
        ctx.report(sc.annual_v1_id).unwrap_err(),
        AcquisitionError::NotAcquired { .. }
    ));
}
