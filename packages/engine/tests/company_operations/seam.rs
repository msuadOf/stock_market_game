//! 会话接缝金样：公司 due 单一 authority（`OperatingScheduler`）↔ 自然日时钟驱动。
//!
//! `CivilClock` 不镜像逐项公司 due：时钟只负责自然日推进合法性（重复日结/
//! 回拨/跳日/运行上界/会话同步），公司到期的恰好一次派发由调度器在
//! `CompanyOperations::advance_civil_day` 内部按 `(due_date, id)` 稳定序完成；
//! 会话日结按公司 `next_due_date` 与时钟当前自然日的合法时点调用经营入口。
//! 公司休市仍运营（不需要股票有成交），同日次序依赖保留为提交序而非公司排序。
//!
//! 2030-01-05 是周六：休市起点保持真实起点，两个 ClosedDay 各日结一次。
//! 披露装配不在本接缝（信息披露）。

use super::fixtures::*;
use engine::calendar::CalendarExchange;
use engine::company::operations::CompanyOperations;
use engine::company::scheduler::{OperatingScheduler, SchedulerRequest};
use engine::company::{CompanyId, IndustryId};
use engine::session::{CivilClock, CivilPhase};

#[test]
fn clock_save_holds_no_company_due_mirror() {
    let start = d("2030-01-05");
    let mut clock = CivilClock::new(start, CalendarExchange::Sse).expect("clock assembles");
    let mut ops = CompanyOperations::new(
        four_company_config(21, quiet_params(), d("2030-01-04")),
        start,
    )
    .expect("ops assembles");

    // 调度器是公司 due 的单一 authority：最早待办就是当日滚动利息。
    assert_eq!(
        ops.next_due_date(),
        Some(start),
        "公司侧必须自己暴露下一到期自然日"
    );
    // 时钟存档不再携带逐项公司 due 镜像，也不再有镜像序号游标。
    let wire = serde_json::to_value(clock.save()).expect("clock save serializes");
    assert!(
        wire.get("pending_due").is_none(),
        "CivilClockSave 不得再镜像公司 due：{wire}"
    );
    assert!(
        wire.get("next_due_seq").is_none(),
        "CivilClockSave 不得再携带 due 序号游标：{wire}"
    );

    // 推进一个自然日后，下一批待办仍只由调度器承载。
    clock.end_day(start).expect("saturday ends");
    ops.advance_civil_day(start).expect("company day runs");
    assert_eq!(ops.next_due_date(), Some(d("2030-01-06")));
    let wire_after = serde_json::to_value(clock.save()).expect("clock save serializes");
    assert!(
        wire_after.get("pending_due").is_none(),
        "日结后时钟仍不得镜像公司 due：{wire_after}"
    );
}

#[test]
fn closed_weekend_runs_company_days_through_scheduler_authority() {
    let start = d("2030-01-05");
    let mut clock = CivilClock::new(start, CalendarExchange::Sse).expect("clock assembles");
    assert!(matches!(clock.phase(), CivilPhase::ClosedDay));
    let mut ops = CompanyOperations::new(
        four_company_config(21, quiet_params(), d("2030-01-04")),
        start,
    )
    .expect("ops assembles");

    // 周六日结：时钟只推进自然日；经营日由会话按 next_due 的合法时点运行。
    let pending_before = ops.scheduler().pending_len();
    assert!(pending_before > 0, "开局滚动利息/到期必须已在调度器排队");
    let saturday = clock.end_day(start).expect("saturday ends");
    assert_eq!(saturday.settled_date, start);
    let day_report = ops.advance_civil_day(start).expect("company day runs");
    assert_eq!(day_report.date, start);
    assert!(
        day_report.dispatched_due > 0,
        "当日到期的滚动利息/合同由调度器恰好一次派发"
    );
    // four_company 目录代码序：C-IND-A = 1。
    let a = CompanyId(1);
    let entries_saturday = ops
        .company(&a)
        .expect("A present")
        .books()
        .books()
        .journal()
        .entry_count();
    assert!(entries_saturday > 1, "business posted on closed day");
    // 次日滚动利息重新排队：待办仍由调度器独立承载。
    assert_eq!(ops.next_due_date(), Some(d("2030-01-06")));
    assert!(ops.scheduler().pending_len() > 0);

    // 周日再日结一次：仍然只有经营，无行情。
    let sunday = clock.end_day(clock.current_date()).expect("sunday ends");
    assert_eq!(sunday.settled_date, d("2030-01-06"));
    ops.advance_civil_day(d("2030-01-06"))
        .expect("company sunday runs");
    let entries_sunday = ops
        .company(&a)
        .expect("A present")
        .books()
        .books()
        .journal()
        .entry_count();
    assert!(
        entries_sunday > entries_saturday,
        "closed Sunday still advances company flows"
    );

    // 行业标签只来自 CompanySpec（不读证券类别）——未上市实体照常经营。
    assert_eq!(
        ops.company(&a).expect("A present").spec().industry,
        IndustryId("home-appliances".to_string())
    );
}

#[test]
fn same_date_dues_dispatch_in_submission_order_not_company_order() {
    // 同一 due_date 内按提交序（id 单调）派发：真实同日次序依赖保留，
    // 不按 CompanyId 重排成与提交无关的固定公司顺序。
    use engine::company::scheduler::ScheduledAction;
    let mut scheduler = OperatingScheduler::new();
    let request = |key: &str, company: u32| SchedulerRequest::Due {
        key: key.to_string(),
        due_date: d("2030-01-07"),
        action: ScheduledAction::InterestAccrual {
            company: CompanyId(company),
        },
    };
    // 提交顺序刻意与公司 id 顺序相反：后提交的大公司 id 先入队。
    scheduler
        .submit(request("bank-second", 9))
        .expect("first submission");
    let second_id = scheduler
        .submit(request("industrial-first", 1))
        .expect("second submission")
        .value();
    let dispatched = scheduler
        .pop_due_on(d("2030-01-07"))
        .expect("same-date dispatch");
    assert_eq!(dispatched.len(), 2);
    assert_eq!(
        dispatched[1].id.value(),
        second_id,
        "同日到期按提交序排尾，不按公司 id 重排"
    );
    assert_eq!(scheduler.next_due_date(), None, "队列排空后无下一到期");
}

#[test]
fn interest_phase_precedes_maturity_even_when_maturity_was_submitted_first() {
    // 真实跨阶段同日依赖：银行放款在经营日内提交到期 due（id 较小），
    // 次日滚动利息在次日末排队（id 较大）。到期当日两案同挂时，派发仍先
    // 全部计息、再处理合同到期——阶段次序不由提交顺序决定。
    use engine::accounting::BusinessKind;
    use engine::company::scheduler::ScheduledAction;
    let start = d("2030-01-05");
    let mut ops = CompanyOperations::new(
        four_company_config(21, quiet_params(), d("2030-01-04")),
        start,
    )
    .expect("ops assembles");
    let bank = ops
        .catalog()
        .id("C-BANK")
        .expect("fixture bank is catalogued");

    // 逐日推进，直到某日待办里同时挂着「先提交的银行合同到期」与
    // 「后提交的当日计息」（maturity id < interest id）。
    let mut target = None;
    let mut day = start;
    for _ in 0..10 {
        let pending = ops.scheduler().pending();
        let maturity = pending.iter().find(|due| {
            due.due_date == day
                && matches!(
                    &due.action,
                    ScheduledAction::ContractMaturity { company, .. } if *company == bank
                )
        });
        let interest = pending.iter().find(|due| {
            due.due_date == day
                && matches!(
                    &due.action,
                    ScheduledAction::InterestAccrual { company, .. } if *company == bank
                )
        });
        if let (Some(maturity), Some(interest)) = (maturity, interest) {
            assert!(
                maturity.id.value() < interest.id.value(),
                "前置条件：合同到期先于当日计息提交（maturity id {} < interest id {}）",
                maturity.id.value(),
                interest.id.value()
            );
            target = Some(day);
            break;
        }
        ops.advance_civil_day(day).expect("company day runs");
        day = day.next().expect("next civil day");
    }
    let target = target.expect("10 日内必出现银行贷款到期日（3 日一贷、5 日期限）");

    let entries_before = ops
        .bank_books(&bank)
        .expect("bank present")
        .books()
        .journal()
        .entry_count();
    let report = ops
        .advance_civil_day(target)
        .expect("maturity day runs");
    assert!(report.dispatched_due >= 2, "计息与合同到期同日派发");

    // 当日新增分录里，计息分录必须先于收本/收息分录入账。
    let journal = ops
        .bank_books(&bank)
        .expect("bank present")
        .books()
        .journal();
    let day_entries: Vec<BusinessKind> = journal
        .entries()
        .skip(entries_before)
        .map(|entry| entry.kind)
        .collect();
    let last_interest = day_entries
        .iter()
        .rposition(|kind| {
            matches!(
                kind,
                BusinessKind::LoanInterestAccrued | BusinessKind::DepositInterestAccrued
            )
        })
        .expect("maturity day accrues interest");
    let first_collection = day_entries
        .iter()
        .position(|kind| {
            matches!(
                kind,
                BusinessKind::LoanPrincipalCollected | BusinessKind::LoanInterestCollected
            )
        })
        .expect("maturity day collects the matured loan");
    assert!(
        last_interest < first_collection,
        "先全部计息、再处理合同到期（阶段次序），实际分录序：{day_entries:?}"
    );
}
