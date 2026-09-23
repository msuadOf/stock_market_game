//! 排期金样（K4）：基准日期、稳定偏移、窗口合规、年报先于 Q1、同 seed
//! 排期/ID 确定。全部为确定性断言（无 RNG：偏移是 seed+公司 id 的纯函数）。

use crate::fixture::{BANK_ID, INDUSTRIAL_ID, OPS_SEED};
use engine::calendar::CivilDate;
use engine::company::CompanyId;
use engine::information::{scheduled_instant, stable_company_offset, ScheduledReportKind};

fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test schedule dates are valid")
}

/// K4 基准日期表（+偏移前）：年报次年 3-20、Q1 4-20、半年 8-15、Q3 10-20。
#[test]
fn schedule_base_dates_match_k4() {
    let cases = [
        (ScheduledReportKind::Annual, 2029, "2030-03-20"),
        (ScheduledReportKind::Q1, 2030, "2030-04-20"),
        (ScheduledReportKind::HalfYear, 2030, "2030-08-15"),
        (ScheduledReportKind::Q3, 2030, "2030-10-20"),
    ];
    for (kind, fiscal_year, base) in cases {
        let instant = scheduled_instant(kind, fiscal_year, 0).expect("offset 0 always legal");
        assert_eq!(instant.date(), d(base), "{kind:?} base date at offset 0");
        assert_eq!(
            instant.second_of_day(),
            18 * 3600,
            "publication phase 18:00"
        );
    }
}

/// 稳定偏移域：0..=7，且同 seed 同公司恒等（纯函数，两次调用一致）。
#[test]
fn stable_offsets_bounded_and_deterministic() {
    for id in [INDUSTRIAL_ID, BANK_ID] {
        let company = CompanyId(id.to_string());
        let offset = stable_company_offset(OPS_SEED, &company);
        assert!(offset <= 7, "offset {offset} out of 0..=7 for {id}");
        assert_eq!(
            offset,
            stable_company_offset(OPS_SEED, &company),
            "same seed + same company must re-derive the same offset"
        );
    }
}

/// 同 seed 逐公司排期一致（instant 层面）；不同 seed 产生不同偏移向量
/// （种子 7 vs 202——固定种子固定结果，无随机性）。
#[test]
fn same_seed_same_schedule_across_companies() {
    let companies: Vec<CompanyId> = [INDUSTRIAL_ID, BANK_ID]
        .iter()
        .map(|id| CompanyId((*id).to_string()))
        .collect();
    let schedule_of = |seed: u64| -> Vec<(u8, engine::calendar::CivilInstant)> {
        companies
            .iter()
            .map(|company| {
                let offset = stable_company_offset(seed, company);
                let instant = scheduled_instant(ScheduledReportKind::Annual, 2030, offset)
                    .expect("annual 2030 legal");
                (offset, instant)
            })
            .collect()
    };
    assert_eq!(
        schedule_of(OPS_SEED),
        schedule_of(OPS_SEED),
        "same seed determinism"
    );
    assert_ne!(
        schedule_of(OPS_SEED),
        schedule_of(202),
        "different seeds must yield a different offset vector (fixed seeds, verified)"
    );
}

/// 窗口合规（任务 2 已核验法定窗：年报 ≤4-30、半年报 ≤8-31）：
/// 全部偏移 0..=7 × 抽样年份都落在法定窗口内，且年报先于同年 Q1、
/// Q1 不早于上一年年报（K4 排期校验契约）。
#[test]
fn all_windows_legal_and_annual_precedes_q1() {
    for offset in 0u8..=7 {
        for year in [2000i32, 2029, 2030, 2097] {
            let annual = scheduled_instant(ScheduledReportKind::Annual, year, offset)
                .expect("annual window legal");
            let q1 =
                scheduled_instant(ScheduledReportKind::Q1, year, offset).expect("q1 window legal");
            let half = scheduled_instant(ScheduledReportKind::HalfYear, year, offset)
                .expect("half-year window legal");
            let q3 =
                scheduled_instant(ScheduledReportKind::Q3, year, offset).expect("q3 window legal");
            // 法定窗口：年报披露 ≤ 次年 4-30；半年报 ≤ 8-31。
            assert!(
                annual.date() <= d(&format!("{}-04-30", year + 1)),
                "annual within legal window"
            );
            assert!(
                half.date() <= d(&format!("{}-08-31", year)),
                "half-year within legal window"
            );
            // 排期节奏：Q1 < 半年 < Q3 < 次年年报（同一会计年度的季报节奏）。
            assert!(q1 < half && half < q3, "quarterly cadence ordered");
            assert!(
                q3 < annual,
                "Q3 precedes the same fiscal year's annual (next March)"
            );
            // 年报先于 Q1（K4 明文契约）：上一年年报 < 本年 Q1。
            let prior_annual = scheduled_instant(ScheduledReportKind::Annual, year - 1, offset)
                .expect("prior annual legal");
            assert!(
                q1 > prior_annual,
                "Q1 {q1:?} must not precede prior-year annual {prior_annual:?}"
            );
        }
    }
}

/// 年报排期跨年语义（自然日加法）：fiscal 2031 的年报落在 2032-03-20+偏移；
/// Q1/半年/Q3 落在同年。偏移 7 的具体日期逐字钉死。
#[test]
fn schedule_dates_pinned_at_offset_seven() {
    let annual = scheduled_instant(ScheduledReportKind::Annual, 2031, 7).expect("legal");
    assert_eq!(
        annual.date(),
        d("2032-03-27"),
        "annual rolls into the next civil year"
    );
    let q1 = scheduled_instant(ScheduledReportKind::Q1, 2031, 7).expect("legal");
    assert_eq!(q1.date(), d("2031-04-27"));
    let half = scheduled_instant(ScheduledReportKind::HalfYear, 2031, 7).expect("legal");
    assert_eq!(half.date(), d("2031-08-22"));
    let q3 = scheduled_instant(ScheduledReportKind::Q3, 2031, 7).expect("legal");
    assert_eq!(q3.date(), d("2031-10-27"));
}

/// 偏移上界拒绝（域外偏移 = 类型化错误，不静默钳位）。
#[test]
fn offset_out_of_range_rejected() {
    let err = scheduled_instant(ScheduledReportKind::Annual, 2030, 8).unwrap_err();
    assert!(matches!(
        err,
        engine::information::InformationError::IllegalScheduleOffset { offset: 8 }
    ));
}
