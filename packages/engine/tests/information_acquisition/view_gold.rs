//! 观察上下文金样：历史版本钉死、dev 查看分离、公共曝光
//! 发现面。共享夹具在 `fixture`；字节投影/上下文构造助手在
//! `acquisition_gold`（同一测试 crate 内复用）。

use crate::acquisition_gold::{build_ctx, judgment_projection, state_bytes};
use crate::fixture::{
    hour_after, minute_before, peer_information_npc, primary_information_npc, FixtureMarket,
    Scenario,
};
use engine::accounting::reports::ReportKind;
use engine::accounting::AccountingPeriod;
use engine::information::{
    discovery_candidates, AcquisitionError, NpcInformationState, NpcObservationContext,
};

/// 金样：历史版本——获知时点钉死版本。更正（新 id）公布后，上下文仍
/// 暴露获知时的原报告（内容逐字节不变）；更正报告只有经新的获知事件才可读。
#[test]
fn acquired_version_pinned_across_later_correction() {
    let mut sc = Scenario::new();
    let a = primary_information_npc();
    let mut state = NpcInformationState::new(a);
    let market = FixtureMarket::quiet();
    state
        .record_acquisition(
            a,
            &sc.library,
            sc.original_annual_report_id,
            hour_after(sc.annual_instant),
        )
        .expect("获知原报告");

    let original_publication_bytes = serde_json::to_string(
        build_ctx(a, &state, &sc.library, &market)
            .report(sc.original_annual_report_id)
            .expect("原报告可读取"),
    )
    .expect("原报告可序列化");
    let projection_before = judgment_projection(&build_ctx(a, &state, &sc.library, &market));

    let corrected_publication_id = sc.publish_correction();

    // 更正公开本身不移动甲的判断输入；原报告内容按获知时点钉死（字节不变）。
    let ctx = build_ctx(a, &state, &sc.library, &market);
    assert_eq!(
        serde_json::to_string(
            ctx.report(sc.original_annual_report_id)
                .expect("原报告仍可读取")
        )
        .expect("原报告可序列化"),
        original_publication_bytes
    );
    assert_eq!(
        judgment_projection(&ctx),
        projection_before,
        "a later correction alone must not move a's inputs"
    );
    assert!(matches!(
        ctx.report(corrected_publication_id).unwrap_err(),
        AcquisitionError::NotAcquired { .. }
    ));

    // 新的获知事件后更正报告才可读；原报告引用依然原样（不可变历史，不覆写）。
    state
        .record_acquisition(
            a,
            &sc.library,
            corrected_publication_id,
            hour_after(sc.correction_instant),
        )
        .expect("获知更正报告");
    let ctx = build_ctx(a, &state, &sc.library, &market);
    let acquired_corrected_report = ctx
        .report(corrected_publication_id)
        .expect("新的获知事件后更正报告可读取");
    assert_eq!(
        acquired_corrected_report.supersedes,
        Some(sc.original_annual_report_id)
    );
    assert_eq!(
        ctx.report(sc.original_annual_report_id)
            .expect("原报告不可变")
            .id,
        sc.original_annual_report_id
    );
}

/// 金样：dev 查看分离——宿主/dev 检查面（公开库查询 + 候选索引，全部
/// 只读 &self）不写入任何 NPC 的信息状态（字节对比），也不产生幻影获知。
#[test]
fn dev_reads_leave_every_npc_state_unchanged() {
    let mut sc = Scenario::new();
    let (a, b) = (primary_information_npc(), peer_information_npc());
    let mut state_a = NpcInformationState::new(a);
    let mut state_b = NpcInformationState::new(b);
    let market = FixtureMarket::quiet();
    state_a
        .record_acquisition(
            a,
            &sc.library,
            sc.original_annual_report_id,
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
    let corrected_publication_id = sc.publish_correction();

    let (bytes_a, bytes_b) = (state_bytes(&state_a), state_bytes(&state_b));
    let (primary_npc_judgment_projection, peer_npc_judgment_projection) = (
        judgment_projection(&build_ctx(a, &state_a, &sc.library, &market)),
        judgment_projection(&build_ctx(b, &state_b, &sc.library, &market)),
    );

    // dev/宿主查看面：公开库查询 + 候选索引（含以未来
    // 时点 as_of 的 dev 视角）——全部 &self。
    let far_future = hour_after(sc.correction_instant);
    let _ = sc
        .library
        .report(sc.original_annual_report_id, far_future)
        .expect("dev 读取原报告");
    let _ = sc
        .library
        .report(corrected_publication_id, far_future)
        .expect("dev 读取更正报告");
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
        primary_npc_judgment_projection
    );
    assert_eq!(
        judgment_projection(&build_ctx(b, &state_b, &sc.library, &market)),
        peer_npc_judgment_projection
    );
}

/// 金样：公共曝光只改变发现机会——`discovery_candidates` 按 as_of 投影
/// 公开索引（id 序确定性），且候选**不构成阅读**（状态仍空、上下文不可读）。
#[test]
fn discovery_candidates_index_publications_without_reading() {
    let mut sc = Scenario::new();
    let a = primary_information_npc();
    let market = FixtureMarket::quiet();

    // 公布前：无候选（公共索引不含未来公布）。
    assert!(
        discovery_candidates(&sc.library, &sc.company, minute_before(sc.annual_instant)).is_empty(),
        "no candidates before the first publication"
    );

    // 原报告公布后：原报告是候选（公告尚未发生）。
    assert_eq!(
        discovery_candidates(&sc.library, &sc.company, hour_after(sc.annual_instant)),
        vec![sc.original_annual_report_id]
    );

    // 公告与更正都公开后：三条候选、id 序。
    let corrected_publication_id = sc.publish_correction();
    let far_future = hour_after(sc.correction_instant);
    let mut expected = vec![
        sc.original_annual_report_id,
        sc.announcement_id,
        corrected_publication_id,
    ];
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
        ctx.report(sc.original_annual_report_id).unwrap_err(),
        AcquisitionError::NotAcquired { .. }
    ));
}
