//! 前史生成金样（K2：开局前 2 个完整自然年度 + 当年截至开局前日）。
//!
//! 同一处理器 + 初始化专用 RNG 流；不创建历史证券成交、不组装公开报告、
//! 不触碰任何交易账户（结构保证 + 权益恒等断言）。1998-01-01 下界经日历
//! 政策核对，越界类型化拒绝。

use super::fixtures::*;
use engine::accounting::LedgerAccountId;
use engine::company::events::ShockParams;
use engine::company::operations::CompanyOperations;
use engine::company::CompanyId;

fn net(books: &engine::accounting::Books, code: &str) -> engine::accounting::AccountingAmount {
    books
        .ledger()
        .account_net_debit(&LedgerAccountId(code.to_string()))
        .expect("ledger query")
}

fn history_config(
    seed: u64,
    params: ShockParams,
) -> engine::company::operations::CompanyOperationsConfig {
    // 前史账套 as_of = 前史首日的前一天（2028-01-01 的前一天）。
    // 工商 + 保险两行业足以覆盖「同一处理器」语义；银行/地产走同一条
    // generate_history 循环（确定性金样不重复支付它们的运行成本）。
    engine::company::operations::CompanyOperationsConfig {
        seed,
        shock_params: params,
        companies: vec![industrial_a(d("2027-12-31")), insurance_c(d("2027-12-31"))],
    }
}

/// 2030-01-01 开局：前史恰好覆盖 [2028-01-01, 2029-12-31]；分录全部落在
/// 前史区间，滚动利息到期已排在开局日；同 seed 重放逐字节一致。
#[test]
fn history_spans_two_full_years_before_start() {
    let mut first =
        CompanyOperations::generate_history(history_config(5, ShockParams::default_v1()), d(START))
            .expect("history generates");
    let second =
        CompanyOperations::generate_history(history_config(5, ShockParams::default_v1()), d(START))
            .expect("history replays");

    assert_eq!(first.next_expected_date(), d(START));
    assert_eq!(
        first
            .history_meta()
            .expect("meta present")
            .generated_through,
        d("2029-12-31")
    );

    for (company, opening_equity) in [("C-IND-A", -4_100_000i128), ("C-INS", -2_000_000i128)] {
        let books = first
            .company(&CompanyId(company.to_string()))
            .expect("company present")
            .books()
            .books();
        let mut entries = books.journal().entries();
        let first_entry = entries.next().expect("opening entry exists");
        assert_eq!(first_entry.date, d("2027-12-31"));
        for entry in books.journal().entries() {
            assert!(
                entry.date >= d("2028-01-01") && entry.date <= d("2029-12-31")
                    || entry.date == d("2027-12-31"),
                "entry dated {entry:?} escapes prehistory window"
            );
        }
        // 真实经营前史存在（远多于开局凭证）且无股东分配（权益恒等于开局）。
        assert!(
            books.journal().entry_count() > 50,
            "business history generated"
        );
        assert_eq!(
            net(books, "4001"),
            engine::accounting::AccountingAmount::from_cents(opening_equity)
        );
    }

    // 同 seed 重放：账套与调度队列逐字节一致。
    let a = CompanyId("C-IND-A".to_string());
    assert_eq!(
        first.industrial_books(&a).expect("A present"),
        second.industrial_books(&a).expect("A present")
    );
    assert_eq!(first.scheduler().pending(), second.scheduler().pending());

    // 开局日首个自然日照常推进（前史续live）。
    first.advance_civil_day(d(START)).expect("first live day");
}

/// 最早合法开局 2000-01-01：前史 [1998-01-01, 1999-12-31] 恰好触及初始化下界。
#[test]
fn history_honors_1998_init_floor() {
    let config = engine::company::operations::CompanyOperationsConfig {
        seed: 9,
        shock_params: ShockParams::default_v1(),
        companies: vec![industrial_a(d("1997-12-31"))],
    };
    let ops = CompanyOperations::generate_history(config, d("2000-01-01"))
        .expect("earliest start history generates");
    assert_eq!(ops.next_expected_date(), d("2000-01-01"));
    assert_eq!(
        ops.history_meta().expect("meta").generated_through,
        d("1999-12-31")
    );
}

/// 前史起点早于 1998-01-01 初始化下界：类型化拒绝（不用邻年顶替）。
#[test]
fn history_before_init_floor_is_rejected() {
    let config = engine::company::operations::CompanyOperationsConfig {
        seed: 9,
        shock_params: ShockParams::default_v1(),
        companies: vec![industrial_a(d("1996-12-31"))],
    };
    let error = CompanyOperations::generate_history(config, d("1999-01-01"))
        .expect_err("pre-1998 prehistory must be rejected");
    assert!(matches!(
        error,
        engine::company::operations::OperationsError::HistoryBeforeInitFloor { .. }
    ));
}
