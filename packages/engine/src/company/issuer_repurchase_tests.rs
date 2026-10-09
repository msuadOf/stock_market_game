//! 发行人回购基础测试：方案校验、成交额度约束、计划完成回收与注销边界
//! （ADR-0038，2026-10-07 M 批）。

use super::{
    IssuerRepurchaseBook, IssuerRepurchaseError, IssuerRepurchasePlan, RepurchaseCompletionPolicy,
    RepurchaseFillRecord, RepurchasePurpose,
};
use crate::company::CompanyId;
use crate::{account::StockCode, calendar::CivilDate, money::Money};

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn plan() -> IssuerRepurchasePlan {
    IssuerRepurchasePlan {
        event_id: "repurchase-1".to_owned(),
        approval_reference: "board-resolution-r1".to_owned(),
        issuer: CompanyId("issuer-1".to_owned()),
        stock: StockCode("600001".to_owned()),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: date("2030-06-01"),
        announced_on: date("2030-06-02"),
        window_start_on: date("2030-06-03"),
        window_deadline_on: date("2030-06-11"),
        price_cap_per_share: Money::from_cents(1_200),
        total_budget: Money::from_cents(3_000_000),
        max_shares: 2_500,
        purpose: RepurchasePurpose::ReduceCapital,
        completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
        approval_unit_event_id: None,
    }
}

fn fill(day: &str, shares: u64, gross_cents: i64, fees_cents: i64) -> RepurchaseFillRecord {
    RepurchaseFillRecord {
        stock: StockCode("600001".to_owned()),
        day: date(day),
        shares,
        gross: Money::from_cents(gross_cents),
        fees: Money::from_cents(fees_cents),
    }
}

#[test]
fn plan_validation_rejects_misordered_dates_and_nonpositive_amounts() {
    plan().validate().unwrap();
    let mut bad = plan();
    bad.window_deadline_on = date("2030-06-02");
    assert!(bad.validate().is_err());
    let mut bad = plan();
    bad.total_budget = Money::from_cents(0);
    assert!(bad.validate().is_err());
    let mut bad = plan();
    bad.max_shares = 0;
    assert!(bad.validate().is_err());
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let mut bad = plan();
    bad.window_start_on = date("2030-06-08");
    assert!(matches!(
        bad.validate_calendar(&calendar),
        Err(IssuerRepurchaseError::InvalidTradingDate { .. })
    ));
    plan().validate_calendar(&calendar).unwrap();
}

#[test]
fn fills_are_bounded_by_budget_share_cap_and_window() {
    let mut book = IssuerRepurchaseBook::new(plan()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    book.record_fill(fill("2030-06-03", 1_000, 1_100_000, 110), &[])
        .unwrap();
    // 同日重复拒绝。
    assert!(book
        .record_fill(fill("2030-06-03", 1, 1_100, 1), &[])
        .is_err());
    // 窗口外拒绝。
    assert!(book
        .record_fill(fill("2030-06-12", 100, 110_000, 11), &[])
        .is_err());
    // 超额度拒绝：已花 1,100,110，额度 3,000,000；再花 1,900,000 → 超出。
    assert!(matches!(
        book.record_fill(fill("2030-06-04", 1_600, 1_900_000, 1), &[]),
        Err(IssuerRepurchaseError::FillExceedsBudget { .. })
    ));
    // 超数量上限拒绝（1,000 + 1,600 > 2,500 → 换算后执行上限同向超出）。
    assert!(book
        .record_fill(fill("2030-06-04", 1_600, 1_600_000, 16), &[])
        .is_err());
    book.record_fill(fill("2030-06-04", 500, 550_000, 55), &[])
        .unwrap();
    assert_eq!(book.total_filled_shares(), 1_500);
    assert_eq!(
        book.remaining_budget().unwrap(),
        Money::from_cents(3_000_000 - 1_100_110 - 550_055)
    );
    book.validate().unwrap();
}

#[test]
fn completion_withdraws_exact_remainder_and_cancellation_bounded_by_fills() {
    let mut book = IssuerRepurchaseBook::new(plan()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    book.record_fill(fill("2030-06-03", 1_000, 1_100_000, 110), &[])
        .unwrap();
    // 完成日前置拒绝。
    assert!(book.complete(date("2030-06-10"), Money::from_cents(1)).is_err());
    let remainder = Money::from_cents(3_000_000 - 1_100_110);
    // 回收额与额度—支出不一致拒绝。
    assert!(book.complete(date("2030-06-11"), Money::from_cents(1)).is_err());
    book.complete(date("2030-06-11"), remainder).unwrap();
    book.complete(date("2030-06-11"), remainder).unwrap();
    // 注销超过累计成交拒绝。
    assert!(matches!(
        book.record_cancellation(date("2030-06-12"), 1_001, &[]),
        Err(IssuerRepurchaseError::CancellationExceedsInventory { .. })
    ));
    book.record_cancellation(date("2030-06-12"), 1_000, &[])
        .unwrap();
    book.record_cancellation(date("2030-06-12"), 1_000, &[])
        .unwrap();
    book.validate().unwrap();
    // 完成后不再受理成交。
    assert!(book
        .record_fill(fill("2030-06-05", 1, 1_100, 1), &[])
        .is_err());
    // 严格 JSON 往返深等。
    let json = serde_json::to_string(&book).unwrap();
    let restored: IssuerRepurchaseBook = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, book);
}

/// 单位换算与库存重放（Task174）：锚→日期比例、执行上限取整、跨单位成交上限、
/// 注销不超当前库存、调整事实数量篡改拒绝与严格持久化缺字段拒绝。
#[test]
fn unit_conversion_factor_skips_anchor_epoch_and_multiplies_exactly() {
    use super::{convert_shares_floor, unit_conversion_factor, SplitUnitContext};
    use crate::company::share_split::ShareSplitDirection;
    let ctx = |entries: &[(u64, u8, u8)]| -> Vec<SplitUnitContext> {
        entries
            .iter()
            .map(|(ex_day, kind, ratio)| SplitUnitContext {
                event_id: format!("e{ex_day}"),
                ex_on: date(&format!("2030-06-{ex_day:02}")),
                direction: if *kind == 0 {
                    ShareSplitDirection::Split
                } else {
                    ShareSplitDirection::Consolidate
                },
                ratio: *ratio as u64,
            })
            .collect()
    };
    let chain = ctx(&[(3, 0, 2), (5, 1, 2)]);
    // 锚 None、through 06-04：只含 06-03 拆股 → ×2/1。
    assert_eq!(
        unit_conversion_factor(None, date("2030-06-04"), &chain).unwrap(),
        (2, 1)
    );
    // 锚 e3、through 06-06：只含 06-05 缩股 → ÷2。
    assert_eq!(
        unit_conversion_factor(Some("e3"), date("2030-06-06"), &chain).unwrap(),
        (1, 2)
    );
    // 锚 e5（链尾）、through 06-06：无后续事件 → 1/1。
    assert_eq!(
        unit_conversion_factor(Some("e5"), date("2030-06-06"), &chain).unwrap(),
        (1, 1)
    );
    // 未知锚拒绝。
    assert!(unit_conversion_factor(Some("nope"), date("2030-06-06"), &chain).is_err());
    assert_eq!(convert_shares_floor(101, 1, 2).unwrap(), 50);
}

#[test]
fn fills_are_bounded_by_converted_execution_cap_across_units() {
    use crate::company::issuer_repurchase::SplitUnitContext;
    use crate::company::share_split::ShareSplitDirection;
    let ctx = vec![SplitUnitContext {
        event_id: "s1".into(),
        ex_on: date("2030-06-05"),
        direction: ShareSplitDirection::Consolidate,
        ratio: 2,
    }];
    let mut book = IssuerRepurchaseBook::new(plan()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    // 旧单位成交 99 股（max 2500 锚单位）。
    book.record_fill(fill("2030-06-03", 99, 108_900, 11), &ctx)
        .unwrap();
    // 缩股换算：99 → 49（无奖励）。
    book.record_share_unit_adjustment(
        crate::company::issuer_repurchase::ShareUnitAdjustment {
            split_event_id: "s1".into(),
            shares_before: 99,
            shares_after: 49,
        },
        &ctx,
    )
    .unwrap();
    // 新单位执行上限 = 2500 ÷ 2 = 1250；49 + 1202 > 1250 拒绝。
    assert!(matches!(
        book.record_fill(fill("2030-06-06", 1_202, 1_322_200, 132), &ctx),
        Err(IssuerRepurchaseError::FillExceedsExecutionCap { .. })
    ));
    book.record_fill(fill("2030-06-06", 1_201, 1_321_100, 132), &ctx)
        .unwrap();
    assert_eq!(book.current_inventory_shares(&ctx).unwrap(), 1_250);
    assert_eq!(
        book.plan()
            .execution_max_shares(date("2030-06-06"), &ctx)
            .unwrap(),
        1_250
    );
}

#[test]
fn cancellation_is_bounded_by_current_inventory_after_unit_adjustment() {
    use crate::company::issuer_repurchase::SplitUnitContext;
    use crate::company::share_split::ShareSplitDirection;
    let ctx = vec![SplitUnitContext {
        event_id: "s1".into(),
        ex_on: date("2030-06-05"),
        direction: ShareSplitDirection::Consolidate,
        ratio: 2,
    }];
    let mut book = IssuerRepurchaseBook::new(plan()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    book.record_fill(fill("2030-06-03", 100, 110_000, 11), &ctx)
        .unwrap();
    book.record_share_unit_adjustment(
        crate::company::issuer_repurchase::ShareUnitAdjustment {
            split_event_id: "s1".into(),
            shares_before: 100,
            shares_after: 50,
        },
        &ctx,
    )
    .unwrap();
    let remainder = Money::from_cents(3_000_000 - 110_011);
    book.complete(date("2030-06-11"), remainder).unwrap();
    // 历史成交合计 100，但当前库存 50：注销 51 必须按当前库存拒绝。
    assert!(matches!(
        book.record_cancellation(date("2030-06-12"), 51, &ctx),
        Err(IssuerRepurchaseError::CancellationExceedsInventory {
            requested: 51,
            available: 50
        })
    ));
    book.record_cancellation(date("2030-06-12"), 50, &ctx)
        .unwrap();
    assert_eq!(book.current_inventory_shares(&ctx).unwrap(), 0);
}

#[test]
fn share_unit_adjustment_tampering_is_rejected_by_replay() {
    use crate::company::issuer_repurchase::SplitUnitContext;
    use crate::company::share_split::ShareSplitDirection;
    let ctx = vec![SplitUnitContext {
        event_id: "s1".into(),
        ex_on: date("2030-06-05"),
        direction: ShareSplitDirection::Consolidate,
        ratio: 2,
    }];
    let mut book = IssuerRepurchaseBook::new(plan()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    book.record_fill(fill("2030-06-03", 100, 110_000, 11), &ctx)
        .unwrap();
    // 换算前库存与重放值不一致（数量篡改）拒绝。
    assert!(book
        .record_share_unit_adjustment(
            crate::company::issuer_repurchase::ShareUnitAdjustment {
                split_event_id: "s1".into(),
                shares_before: 99,
                shares_after: 50,
            },
            &ctx,
        )
        .is_err());
    // 换算结果不等于 base（±合法奖励）拒绝。
    assert!(book
        .record_share_unit_adjustment(
            crate::company::issuer_repurchase::ShareUnitAdjustment {
                split_event_id: "s1".into(),
                shares_before: 100,
                shares_after: 52,
            },
            &ctx,
        )
        .is_err());
    // 未知事件拒绝；同事件重复拒绝。
    assert!(book
        .record_share_unit_adjustment(
            crate::company::issuer_repurchase::ShareUnitAdjustment {
                split_event_id: "ghost".into(),
                shares_before: 100,
                shares_after: 50,
            },
            &ctx,
        )
        .is_err());
    book.record_share_unit_adjustment(
        crate::company::issuer_repurchase::ShareUnitAdjustment {
            split_event_id: "s1".into(),
            shares_before: 100,
            shares_after: 50,
        },
        &ctx,
    )
    .unwrap();
    assert!(book
        .record_share_unit_adjustment(
            crate::company::issuer_repurchase::ShareUnitAdjustment {
                split_event_id: "s1".into(),
                shares_before: 100,
                shares_after: 50,
            },
            &ctx,
        )
        .is_err());
}

#[test]
fn strict_persistence_rejects_missing_share_unit_fields() {
    let mut book = IssuerRepurchaseBook::new(plan()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    book.record_fill(fill("2030-06-03", 100, 110_000, 11), &[])
        .unwrap();
    let json = serde_json::to_value(&book).unwrap();
    // 账簿缺 share_unit_adjustments 键显式拒绝（无兼容默认）。
    let mut missing_book = json.clone();
    missing_book
        .as_object_mut()
        .unwrap()
        .remove("share_unit_adjustments");
    assert!(
        serde_json::from_value::<IssuerRepurchaseBook>(missing_book).is_err(),
        "缺 share_unit_adjustments 的档必须显式拒绝"
    );
    // 方案缺 approval_unit_event_id 键显式拒绝（必填 nullable）。
    let mut missing_plan = json;
    missing_plan
        .as_object_mut()
        .unwrap()
        .get_mut("plan")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("approval_unit_event_id");
    assert!(
        serde_json::from_value::<IssuerRepurchaseBook>(missing_plan).is_err(),
        "缺 approval_unit_event_id 的档必须显式拒绝"
    );
}

/// P1 回归（非作者门禁）：缩股合法碎股奖励经后续拆股放大后，奖励上界必须
/// 随单位调整同放——按事件数计数会误拒合法库存（max 94 缩 10:1 fill 94
/// 获奖 → 10；再拆 1:2 → 20 > floor(94×2/10)+1=19 被误拒）。
#[test]
fn consolidation_award_must_scale_through_later_split() {
    use crate::company::issuer_repurchase::{ShareUnitAdjustment, SplitUnitContext};
    use crate::company::share_split::ShareSplitDirection;
    let ctx = vec![
        SplitUnitContext {
            event_id: "consol".into(),
            ex_on: date("2030-06-05"),
            direction: ShareSplitDirection::Consolidate,
            ratio: 10,
        },
        SplitUnitContext {
            event_id: "splitb".into(),
            ex_on: date("2030-06-08"),
            direction: ShareSplitDirection::Split,
            ratio: 2,
        },
    ];
    let mut capped = plan();
    capped.max_shares = 94;
    let mut book = IssuerRepurchaseBook::new(capped).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    // 旧单位成交 94 股（max 94 锚单位；预算充足）。
    book.record_fill(fill("2030-06-03", 94, 103_400, 11), &ctx)
        .unwrap();
    book.record_share_unit_adjustment(
        ShareUnitAdjustment {
            split_event_id: "consol".into(),
            shares_before: 94,
            shares_after: 10, // 94/10=9 余 4 → 奖励 +1 = 10
        },
        &ctx,
    )
    .unwrap();
    book.record_share_unit_adjustment(
        ShareUnitAdjustment {
            split_event_id: "splitb".into(),
            shares_before: 10,
            shares_after: 20,
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(book.current_inventory_shares(&ctx).unwrap(), 20);
    // 上界随调整同放：执行上限 floor(94×2/10)=18 + 奖励股 1×2=2 = 20 恰容库存。
    // 修复前按事件数计 awards=1 → 20 > 19 误拒（红）；修复后必须通过。
    book.validate_share_units(&ctx, date("2030-06-10")).unwrap();
    // 上限已到顶：再加 1 股仍须拒绝（边界不被奖励口径放宽到无限）。
    assert!(book
        .record_fill(fill("2030-06-09", 1, 1_100, 1), &ctx)
        .is_err());
}

#[test]
fn converted_approved_cap_overflow_is_an_explicit_error() {
    let mut value = plan();
    value.max_shares = u64::MAX;
    let context = [super::SplitUnitContext {
        event_id: "large-split".into(),
        ex_on: date("2030-06-05"),
        direction: crate::company::share_split::ShareSplitDirection::Split,
        ratio: 2,
    }];
    assert!(value
        .execution_max_shares(date("2030-06-06"), &context)
        .is_err());
}

#[test]
fn zero_unit_denominator_is_an_explicit_error() {
    let context = [super::SplitUnitContext {
        event_id: "bad-zero".into(),
        ex_on: date("2030-06-05"),
        direction: crate::company::share_split::ShareSplitDirection::Consolidate,
        ratio: 0,
    }];
    assert!(plan()
        .execution_max_shares(date("2030-06-06"), &context)
        .is_err());
}
