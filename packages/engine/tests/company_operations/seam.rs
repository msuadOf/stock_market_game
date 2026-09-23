//! 会话接缝金样：调度器 ↔ 自然日时钟到期队列（任务 5 的 DueKind 钩子）。
//!
//! 2030-01-05 是周六：休市起点保持真实起点，两个 ClosedDay 各日结一次，
//! 公司经营在**零市场 tick / 零成交**下照常推进（K4 验收：公司经营不需要
//! 股票有成交）。披露装配不在本接缝（任务 15）。

use super::fixtures::*;
use engine::calendar::CalendarExchange;
use engine::company::operations::CompanyOperations;
use engine::company::{CompanyId, IndustryId};
use engine::session::{CivilClock, CivilPhase, CompanyOperationsClockWiring, DueKind};

#[test]
fn scheduler_wires_into_civil_clock_due_queue_over_closed_weekend() {
    let start = d("2030-01-05");
    let mut clock = CivilClock::new(start, CalendarExchange::Sse).expect("clock assembles");
    assert!(matches!(clock.phase(), CivilPhase::ClosedDay));
    let mut ops = CompanyOperations::new(
        four_company_config(21, quiet_params(), d("2030-01-04")),
        start,
    )
    .expect("ops assembles");
    let mut wiring = CompanyOperationsClockWiring::new();

    // 安装：调度器待办镜像为时钟 due（工商×2 + 银行 + 地产的滚动利息）。
    wiring.install(&mut clock, &ops).expect("install");
    assert_eq!(clock.pending_due().len(), 4);
    assert!(clock
        .pending_due()
        .iter()
        .all(|due| due.kind == DueKind::InterestAccrual));

    // 周六日结：到期派发（恰好一次）→ 经营推进 → 重新排队（4 条滚动利息 +
    // 当日业务新增 3 条到期：A/B 的赊销到期与银行首笔贷款到期）。
    let saturday = clock.end_day(start).expect("saturday ends");
    assert_eq!(saturday.dispatched_due.len(), 4);
    let day_report = wiring
        .run_day_end(&saturday, &mut clock, &mut ops)
        .expect("company day runs");
    assert_eq!(day_report.date, start);
    let a = CompanyId("C-IND-A".to_string());
    let entries_saturday = ops
        .company(&a)
        .expect("A present")
        .books()
        .books()
        .journal()
        .entry_count();
    assert!(entries_saturday > 1, "business posted on closed day");
    assert_eq!(clock.pending_due().len(), 7);

    // 周日再日结一次：仍然只有经营，无行情。
    let sunday = clock.end_day(clock.current_date()).expect("sunday ends");
    assert_eq!(sunday.settled_date, d("2030-01-06"));
    wiring
        .run_day_end(&sunday, &mut clock, &mut ops)
        .expect("company sunday runs");
    assert!(
        ops.company(&a)
            .expect("A present")
            .books()
            .books()
            .journal()
            .entry_count()
            > entries_saturday
    );

    // 行业标签只来自 CompanySpec（不读证券类别）——未上市实体照常经营。
    assert_eq!(
        ops.company(&a).expect("A present").spec().industry,
        IndustryId("home-appliances".to_string())
    );
}
