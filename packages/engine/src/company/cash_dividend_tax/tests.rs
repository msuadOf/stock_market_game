use super::*;

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}
fn lot(id: &str, qty: u64, acquired_on: &str) -> DividendTaxLot {
    DividendTaxLot {
        id: id.into(),
        qty,
        acquired_on: date(acquired_on),
        source: TaxAcquisitionSource::InitialAllocation {
            evidence: id.into(),
        },
        class: TaxShareClass::PublicMarket,
    }
}
fn book() -> CashDividendTaxBook {
    CashDividendTaxBook::new(
        AccountId(u64::MAX),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("old", 10, "2029-12-01"), lot("new", 10, "2029-12-20")],
    )
    .unwrap()
}

#[test]
fn natural_month_and_year_boundaries_use_disposal_day_exclusive_holding() {
    for (sale, rate) in [
        ("2030-02-01", 20),
        ("2030-02-02", 10),
        ("2031-01-01", 10),
        ("2031-01-02", 0),
    ] {
        assert_eq!(
            personal_cash_dividend_rate(date("2030-01-01"), date(sale)).unwrap(),
            rate
        );
    }
}

#[test]
fn fifo_partial_sale_assesses_only_paid_dividend_and_collection_is_atomic() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    assert_eq!(book.lots()[0].qty, 5);
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-02"),
        Money::from_cents(2000),
        "actual-cash-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(50, 1).unwrap()
    );
    assert_eq!(
        book.collect_due("collect1".into(), date("2030-01-02"), Money::from_cents(20))
            .unwrap()
            .collected,
        Money::from_cents(20)
    );
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(30, 1).unwrap()
    );
    let before = book.clone();
    assert_eq!(
        book.collect_due("collect1".into(), date("2030-01-02"), Money::from_cents(20))
            .unwrap()
            .collected,
        Money::from_cents(20)
    );
    assert_eq!(book, before);
    assert!(book
        .collect_due("collect1".into(), date("2030-01-02"), Money::from_cents(21))
        .is_err());
    assert_eq!(book, before);
}

#[test]
fn nonindividual_profile_is_not_silently_taxed_as_individual() {
    assert!(matches!(
        CashDividendTaxBook::new(
            AccountId(1),
            StockCode("600001".into()),
            DividendTaxProfile::ResidentEnterprise,
            date("2030-01-01"),
            vec![]
        ),
        Err(DividendTaxError::UnsupportedProfile { .. })
    ));
}

#[test]
fn per_dividend_tax_rounds_half_up_to_whole_cents_at_collection() {
    // 财税〔2012〕85号口径下的常见组合：每股 7 分 × 3 股 × 20% = 4.2 分，
    // 按持有人每笔分红合计应纳税额四舍五入到分 → 收缴 4 分，不再以亚分证据拒绝。
    let mut book = CashDividendTaxBook::new(
        AccountId(u64::MAX),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("fractional", 3, "2029-12-15")],
    )
    .unwrap();
    book.register_dividend(
        "fraction".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(7, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -3, None)
        .unwrap();
    book.record_payment(
        "fraction",
        "actual-cash-receipt".into(),
        date("2030-01-02"),
        Money::from_cents(21),
        "actual-cash-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(4, 1).unwrap(),
        "7 cents x 3 shares x 20% = 4.2 cents rounds half-down to 4 cents"
    );
    assert_eq!(
        book.collect_due("collect".into(), date("2030-01-02"), Money::from_cents(100))
            .unwrap()
            .collected,
        Money::from_cents(4)
    );
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
}

#[test]
fn half_cent_per_dividend_tax_rounds_up_and_sub_half_cent_rounds_to_zero() {
    // 5 分/股 × 1 股 × 10% = 0.5 分 → 四舍五入到分为 1 分（half-up）。
    let mut half_up = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("half", 1, "2029-12-01")],
    )
    .unwrap();
    half_up
        .register_dividend(
            "half".into(),
            date("2030-01-01"),
            ExactDividendTaxAmount::new(5, 1).unwrap(),
        )
        .unwrap();
    half_up
        .record_net_day("sell".into(), date("2030-01-02"), -1, None)
        .unwrap();
    half_up
        .record_payment(
            "half",
            "actual-receipt".into(),
            date("2030-01-02"),
            Money::from_cents(5),
            "actual-cash-receipt".into(),
        )
        .unwrap();
    assert_eq!(
        half_up.outstanding().unwrap(),
        ExactDividendTaxAmount::new(1, 1).unwrap(),
        "0.5 cent rounds half-up to 1 cent"
    );

    // 每股 1/3 分 × 1 股 × 10% = 1/30 分 < 0.5 分 → 该笔分红合计税额四舍五入为 0 分。
    let mut tiny = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![lot("tiny", 3, "2029-12-01")],
    )
    .unwrap();
    tiny.register_dividend(
        "tiny".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(1, 3).unwrap(),
    )
    .unwrap();
    tiny.record_net_day("sell".into(), date("2030-01-02"), -1, None)
        .unwrap();
    tiny.record_payment(
        "tiny",
        "actual-one-cent".into(),
        date("2030-01-02"),
        Money::from_cents(1),
        "actual-cash-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        tiny.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap(),
        "1/30 cent rounds to zero under the registered per-dividend rounding rule"
    );
    assert_eq!(
        tiny.collect_due("collect".into(), date("2030-01-02"), Money::from_cents(100))
            .unwrap()
            .collected,
        Money::from_cents(0)
    );
    let restored: CashDividendTaxBook =
        serde_json::from_value(serde_json::to_value(&tiny).unwrap()).unwrap();
    assert_eq!(restored, tiny);
}

#[test]
fn later_same_day_payment_does_not_rewrite_earlier_collection() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "cash",
        "partial1".into(),
        date("2030-01-02"),
        Money::from_cents(1000),
        "actual-receipt1".into(),
    )
    .unwrap();
    let first = book
        .collect_due("tax1".into(), date("2030-01-02"), Money::from_cents(25))
        .unwrap();
    assert_eq!(first.collected, Money::from_cents(25));
    assert_eq!(
        first.outstanding,
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
    book.record_payment(
        "cash",
        "partial2".into(),
        date("2030-01-02"),
        Money::from_cents(1000),
        "actual-receipt2".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(25, 1).unwrap()
    );
    assert_eq!(
        book.collect_due("tax1".into(), date("2030-01-02"), Money::from_cents(25))
            .unwrap(),
        first
    );
    let second = book
        .collect_due("tax2".into(), date("2030-01-02"), Money::from_cents(25))
        .unwrap();
    assert_eq!(second.collected, Money::from_cents(25));
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
    let restored: CashDividendTaxBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
}

#[test]
fn sale_on_statutory_release_day_has_short_term_tax_not_t1_rejection() {
    let mut restricted = lot("restricted", 10, "2029-01-01");
    restricted.class = TaxShareClass::StatutoryRestricted {
        release_on: date("2030-01-02"),
        basis: StatutoryRestrictedBasis::FinanceTax2009167,
        qualification_evidence: "explicit-qualifying-shares".into(),
    };
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![restricted],
    )
    .unwrap();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell-on-release".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "cash",
        "actual-paid-after-release".into(),
        date("2030-01-02"),
        Money::from_cents(1000),
        "actual-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(100, 1).unwrap()
    );
    assert_eq!(
        book.collect_due("tax".into(), date("2030-01-02"), Money::from_cents(100))
            .unwrap()
            .collected,
        Money::from_cents(100)
    );
}

#[test]
fn restore_cannot_replace_registered_tax_disposition_identity_to_erase_tax() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-02"),
        Money::from_cents(2000),
        "real-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(50, 1).unwrap()
    );
    let mut corrupt = serde_json::to_value(&book).unwrap();
    corrupt["days"][0]["dispositions"][0]["lot"]["id"] = serde_json::json!("unknown-lot");
    assert!(serde_json::from_value::<CashDividendTaxBook>(corrupt).is_err());
}

#[test]
fn tax_restore_rejects_conflicting_metadata_sequence_and_cash_and_failures_are_atomic() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-02"),
        Money::from_cents(2000),
        "real-receipt".into(),
    )
    .unwrap();
    book.collect_due("tax".into(), date("2030-01-02"), Money::from_cents(20))
        .unwrap();
    let valid = serde_json::to_value(&book).unwrap();
    assert!(valid.to_string().contains("\"18446744073709551615\""));
    for kind in [
        "metadata", "quantity", "snapshot", "sequence", "cash", "nullable", "version",
    ] {
        let mut corrupt = valid.clone();
        match kind {
            "metadata" => {
                corrupt["days"][0]["dispositions"][0]["lot"]["acquired_on"] =
                    serde_json::json!("2029-11-01")
            }
            "quantity" => corrupt["lots"][0]["qty"] = serde_json::json!(6),
            "snapshot" => corrupt["dividends"][0]["lots"][0]["qty"] = serde_json::json!(11),
            "sequence" => {
                corrupt["dividends"][0]["payments"][0]["operation_seq"] = serde_json::json!(2)
            }
            "cash" => corrupt["collections"][0]["collected"] = serde_json::json!("21"),
            "nullable" => {
                corrupt["days"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("acquisition");
            }
            "version" => {
                corrupt
                    .as_object_mut()
                    .unwrap()
                    .insert("version".into(), serde_json::json!(3));
            }
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<CashDividendTaxBook>(corrupt).is_err(),
            "accepted {kind}"
        );
    }
    let before = book.clone();
    assert!(book
        .record_payment(
            "cash",
            "overpaid".into(),
            date("2030-01-02"),
            Money::from_cents(1),
            "real-extra".into()
        )
        .is_err());
    assert_eq!(book, before);
    assert!(book
        .record_net_day("oversell".into(), date("2030-01-03"), -16, None)
        .is_err());
    assert_eq!(book, before);
}

#[test]
fn mixed_restricted_tax_disposal_is_explicitly_unsupported() {
    let mut restricted = lot("restricted", 5, "2029-01-01");
    restricted.class = TaxShareClass::StatutoryRestricted {
        release_on: date("2031-01-01"),
        basis: StatutoryRestrictedBasis::FinanceTax201070,
        qualification_evidence: "explicit-qualifying".into(),
    };
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![restricted, lot("public", 5, "2029-12-01")],
    )
    .unwrap();
    let before = book.clone();
    assert_eq!(
        book.record_net_day("mixed-sell".into(), date("2030-01-02"), -1, None),
        Err(DividendTaxError::UnsupportedMixedRestrictedTaxLots)
    );
    assert_eq!(book, before);
}

#[test]
fn month_end_acquisition_clamps_period_boundary_to_target_month_end() {
    // 1月31日取得：一个月边界钳制为2月28/29日（对应日不存在时取目标月最后一日），
    // 而不是对整类月末取得批次报 NeedHoldingPeriodBoundaryEvidence 卡死日结。
    for (acquired, disposed, rate) in [
        ("2030-01-31", "2030-02-27", 20),
        ("2030-01-31", "2030-02-28", 20),
        ("2030-01-31", "2030-03-01", 10),
        ("2030-01-31", "2031-01-31", 10),
        ("2030-01-31", "2031-02-01", 0),
        // 闰年 1月31日取得的月边界钳制到 2月29日。
        ("2028-01-31", "2028-02-29", 20),
        ("2028-01-31", "2028-03-01", 10),
        // 8月31日取得 → 9月无31日，钳制到 9月30日；一年边界 2030-08-31 存在。
        ("2029-08-31", "2029-09-30", 20),
        ("2029-08-31", "2029-10-01", 10),
        ("2029-08-31", "2030-08-31", 10),
        ("2029-08-31", "2030-09-01", 0),
    ] {
        assert_eq!(
            personal_cash_dividend_rate(date(acquired), date(disposed)).unwrap(),
            rate,
            "{acquired} -> {disposed}"
        );
    }
}

#[test]
fn leap_day_acquisition_clamps_year_boundary_across_non_leap_years() {
    // 2月29日取得：平年一年边界无对应日，钳制到 2月28日。
    for (acquired, disposed, rate) in [
        ("2028-02-29", "2028-03-29", 20),
        ("2028-02-29", "2028-03-30", 10),
        ("2028-02-29", "2029-02-28", 10),
        ("2028-02-29", "2029-03-01", 0),
        // 跨非闰年 2月：1月31日取得，一年边界 2030-01-31 之后卖出免税。
        ("2029-01-31", "2030-02-28", 0),
    ] {
        assert_eq!(
            personal_cash_dividend_rate(date(acquired), date(disposed)).unwrap(),
            rate,
            "{acquired} -> {disposed}"
        );
    }
}

#[test]
fn payment_retry_does_not_allocate_sequence_at_counter_limit() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-01"),
        Money::from_cents(2000),
        "real-receipt".into(),
    )
    .unwrap();
    book.operation_seq = u64::MAX;
    let before = book.clone();
    assert!(book
        .record_payment(
            "cash",
            "paid".into(),
            date("2030-01-01"),
            Money::from_cents(2000),
            "real-receipt".into()
        )
        .is_ok());
    assert_eq!(book, before);
    assert!(matches!(
        book.record_payment(
            "cash",
            "new-payment".into(),
            date("2030-01-01"),
            Money::from_cents(1),
            "new-receipt".into()
        ),
        Err(DividendTaxError::Overflow {
            operation: "operation sequence"
        })
    ));
    assert_eq!(book, before);
}

#[test]
fn dividend_received_before_statutory_unlock_is_ten_percent_and_not_retaxed() {
    let mut restricted = lot("restricted", 10, "2028-01-01");
    restricted.class = TaxShareClass::StatutoryRestricted {
        release_on: date("2030-01-02"),
        basis: StatutoryRestrictedBasis::FinanceTax2009167,
        qualification_evidence: "explicit-qualifying".into(),
    };
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![restricted],
    )
    .unwrap();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_payment(
        "cash",
        "before-unlock".into(),
        date("2030-01-01"),
        Money::from_cents(1000),
        "real-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(100, 1).unwrap()
    );
    assert_eq!(
        book.collect_due(
            "preunlock-tax".into(),
            date("2030-01-01"),
            Money::from_cents(100)
        )
        .unwrap()
        .collected,
        Money::from_cents(100)
    );
    book.record_net_day("sell-on-unlock".into(), date("2030-01-02"), -5, None)
        .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap()
    );
    let restored: CashDividendTaxBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
}

/// 同日「先公开市场后送转」的税账日结：送转到账作为同日正向续记合法入账，
/// 事件身份独立于公开市场日结；同日负向或零净变动不得作为续记。
#[test]
fn same_day_positive_continuation_records_distribution_credit_after_market_day() {
    let mut book = book();
    book.record_net_day("market-2030-01-02".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let credit = DividendTaxLot {
        id: "tax:stock-distribution:d1:account-0".into(),
        qty: 3,
        acquired_on: date("2030-01-02"),
        source: TaxAcquisitionSource::CorporateAction { event: "d1".into() },
        class: TaxShareClass::PublicMarket,
    };
    book.record_net_day(
        "stock-distribution:d1:0".into(),
        date("2030-01-02"),
        3,
        Some(credit),
    )
    .unwrap();
    assert_eq!(book.settled_on(), date("2030-01-02"));
    assert_eq!(book.tax_day_receipts().len(), 2);
    assert_eq!(book.lots().iter().map(|lot| lot.qty).sum::<u64>(), 23);
    // 幂等重放同事实不报错、不重复。
    book.record_net_day(
        "stock-distribution:d1:0".into(),
        date("2030-01-02"),
        3,
        Some(DividendTaxLot {
            id: "tax:stock-distribution:d1:account-0".into(),
            qty: 3,
            acquired_on: date("2030-01-02"),
            source: TaxAcquisitionSource::CorporateAction { event: "d1".into() },
            class: TaxShareClass::PublicMarket,
        }),
    )
    .unwrap();
    assert_eq!(book.tax_day_receipts().len(), 2);
    // 次日正常推进不受同日续记影响。
    book.record_net_day("market-2030-01-03".into(), date("2030-01-03"), 0, None)
        .unwrap();
    assert_eq!(book.tax_day_receipts().len(), 3);
}

/// 同日续记的防御边界：首条日结不得等于开账日；同日零或负净变动、
/// 事实不一致的同日复用、跳日都必须显式拒绝。
#[test]
fn same_day_continuation_rejects_nonpositive_net_and_first_day_equals_opening() {
    let mut book = book();
    let rejection = book
        .record_net_day("market-opening-day".into(), date("2030-01-01"), 0, None)
        .unwrap_err();
    assert!(
        rejection.to_string().contains("tax day"),
        "开账日当日不得作为首个日结：{rejection}"
    );
    book.record_net_day("market-2030-01-02".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let rejection = book
        .record_net_day("ghost-zero".into(), date("2030-01-02"), 0, None)
        .unwrap_err();
    assert!(
        rejection.to_string().contains("tax day"),
        "同日零净变动续记必须拒绝：{rejection}"
    );
    let rejection = book
        .record_net_day("ghost-negative".into(), date("2030-01-02"), -1, None)
        .unwrap_err();
    assert!(
        rejection.to_string().contains("tax day"),
        "同日负净变动续记必须拒绝（处置每日只按公开市场净额一次）：{rejection}"
    );
    let rejection = book
        .record_net_day("skip-day".into(), date("2030-01-04"), 0, None)
        .unwrap_err();
    assert!(
        rejection.to_string().contains("tax day"),
        "跳日日结必须拒绝：{rejection}"
    );
}

// ---------- 拆股／缩股税账路径 ----------

#[test]
fn redenomination_reduction_reduces_fifo_lots_without_taxable_disposition() {
    let mut book = book();
    book.register_dividend(
        "cash".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    // 公开市场日结先落账（推进 settled_on），同日缩股核减 8 股：FIFO 从最旧
    // lot 开始消耗（old 10 → 2），不产生应税处置。
    book.record_net_day("market".into(), date("2030-01-02"), 0, None)
        .unwrap();
    book.record_redenomination_reduction("split:consolidation-1".into(), date("2030-01-02"), 8)
        .unwrap();
    assert_eq!(book.lots().len(), 2);
    assert_eq!(book.lots()[0].id, "old");
    assert_eq!(book.lots()[0].qty, 2);
    assert_eq!(book.lots()[1].qty, 10);
    // 缩股核减不触发补税：税额保持为零（同日无卖出处置）。
    book.record_payment(
        "cash",
        "paid".into(),
        date("2030-01-02"),
        Money::from_cents(2000),
        "actual-cash-receipt".into(),
    )
    .unwrap();
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(0, 1).unwrap(),
        "缩股核减不是转让，不得对应纳税所得"
    );
    // 幂等：同一事件身份重复提交不重复核减。
    book.record_redenomination_reduction("split:consolidation-1".into(), date("2030-01-02"), 8)
        .unwrap();
    assert_eq!(book.lots()[0].qty, 2);
    // 恢复（serde 往返 + 重放校验）保留核减事实。
    let restored: CashDividendTaxBook =
        serde_json::from_str(&serde_json::to_string(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
}

#[test]
fn redenomination_reduction_validates_boundaries() {
    let mut book = book();
    // 超出持有数量拒绝。
    assert!(
        book.record_redenomination_reduction("c1".into(), date("2030-01-02"), 21)
            .is_err()
    );
    // 跳日拒绝（须同日续记或次一自然日）。
    assert!(
        book.record_redenomination_reduction("c2".into(), date("2030-01-05"), 1)
            .is_err()
    );
    // 零数量拒绝。
    assert!(
        book.record_redenomination_reduction("c3".into(), date("2030-01-02"), 0)
            .is_err()
    );
    book.record_redenomination_reduction("c4".into(), date("2030-01-02"), 10)
        .unwrap();
    // 事件身份复用但数量不同 → 冲突。
    assert!(
        book.record_redenomination_reduction("c4".into(), date("2030-01-02"), 1)
            .is_err()
    );
    // 篡改核减数量在恢复校验中被拒。
    let mut tampered = serde_json::to_value(&book).unwrap();
    tampered["redenominations"][0]["net_change"] = serde_json::json!("-9");
    assert!(serde_json::from_value::<CashDividendTaxBook>(tampered).is_err());
}

// ---------- 送股个税税基（2026-10-10，任务175） ----------
//
// 语义：个人取得送股（未分配利润转股本的股票股利）按股票面值计入股息税基，
// 持股期限自 R+1 到账日起算（财税〔2012〕85号第六条（八）+第一条第二款，
// 取得日口径已由既有同日续记实现）；税款在股东后续卖出时按差别化档位从
// 卖出款扣缴。资本公积（股本溢价）转增对个人不征税——本引擎资本公积只有
// 股本溢价单一来源口径，故登记简化为「送股应税、转增一律免税」。

/// 空开账税账 + R+1 送股入账 + 送股税基：返回已记账的 book。
fn book_with_bonus_basis(
    qty: u64,
    par_cents: i64,
    restriction: TaxShareClass,
) -> CashDividendTaxBook {
    let mut book = CashDividendTaxBook::new(
        AccountId(0),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![],
    )
    .unwrap();
    book.record_net_day("market-2030-01-02".into(), date("2030-01-02"), 0, None)
        .unwrap();
    let credit_lot = DividendTaxLot {
        id: "tax:stock-distribution:d1:account-0".into(),
        qty,
        acquired_on: date("2030-01-02"),
        source: TaxAcquisitionSource::CorporateAction { event: "d1".into() },
        class: restriction,
    };
    book.record_net_day(
        "stock-distribution:d1:0".into(),
        date("2030-01-02"),
        i128::try_from(qty).unwrap(),
        Some(credit_lot),
    )
    .unwrap();
    book.record_stock_bonus_basis(
        "d1".into(),
        date("2030-01-02"),
        "tax:stock-distribution:d1:account-0".into(),
        qty,
        Money::from_cents(par_cents),
    )
    .unwrap();
    book
}

/// 逐日推进零净额日结到卖出日前一自然日，再在卖出日全额卖出送转批次。
fn dispose_bonus_on(book: CashDividendTaxBook, sale: &str, qty: u64) -> CashDividendTaxBook {
    let mut book = book;
    let sale_day = date(sale);
    while book.settled_on().next().as_ref() != Ok(&sale_day) {
        let next = book
            .settled_on()
            .next()
            .expect("civil date always has a successor");
        book.record_net_day(format!("market-{next}"), next, 0, None)
            .unwrap();
    }
    book.record_net_day(
        format!("sell-{sale}"),
        sale_day,
        -i128::try_from(qty).unwrap(),
        None,
    )
    .unwrap();
    book
}

/// 送股税基按面值 × 股数 × 差别化档位在处置时计税（取得日 2030-01-02；
/// 登记口径：卖出日=周年日仍归上一档）：
/// 面值 100 分 × 3 股，卖出 2030-02-02（满月当日，≤1月）→ 20% → 60 分；
/// 2030-02-03（1月-1年）→ 10% → 30 分；2031-01-03（>1年）→ 0 分。
/// 同账并入一批无税基的转增批次：卖出不产生任何税（转增一律免税的登记简化）。
#[test]
fn stock_bonus_basis_withholds_par_value_tax_at_statutory_tiers_on_disposal() {
    for (sale, expected_cents) in [("2030-02-02", 60), ("2030-02-03", 30), ("2031-01-03", 0)] {
        let mut book = CashDividendTaxBook::new(
            AccountId(0),
            StockCode("600001".into()),
            DividendTaxProfile::IndividualPublicMarket,
            date("2030-01-01"),
            vec![],
        )
        .unwrap();
        book.record_net_day("market-2030-01-02".into(), date("2030-01-02"), 0, None)
            .unwrap();
        // 同日两批：送股批次（有税基）与转增批次（无税基，转增免税）。
        book.record_net_day(
            "stock-distribution:bonus-1:0".into(),
            date("2030-01-02"),
            3,
            Some(DividendTaxLot {
                id: "tax:stock-distribution:bonus-1:account-0".into(),
                qty: 3,
                acquired_on: date("2030-01-02"),
                source: TaxAcquisitionSource::CorporateAction {
                    event: "bonus-1".into(),
                },
                class: TaxShareClass::PublicMarket,
            }),
        )
        .unwrap();
        book.record_stock_bonus_basis(
            "bonus-1".into(),
            date("2030-01-02"),
            "tax:stock-distribution:bonus-1:account-0".into(),
            3,
            Money::from_cents(100),
        )
        .unwrap();
        book.record_net_day(
            "stock-distribution:conversion-1:0".into(),
            date("2030-01-02"),
            2,
            Some(DividendTaxLot {
                id: "tax:stock-distribution:conversion-1:account-0".into(),
                qty: 2,
                acquired_on: date("2030-01-02"),
                source: TaxAcquisitionSource::CorporateAction {
                    event: "conversion-1".into(),
                },
                class: TaxShareClass::PublicMarket,
            }),
        )
        .unwrap();
        let book = dispose_bonus_on(book, sale, 5);
        assert_eq!(
            book.outstanding().unwrap(),
            ExactDividendTaxAmount::new(expected_cents, 1).unwrap(),
            "送股税基在 {sale} 卖出应评估 {expected_cents} 分（转增 2 股不计税）"
        );
    }
}

/// 限售送股批次：解禁前到账按 10% 立即计税（与现金分红限售口径同构，
/// 85号第四条），无现金故留未清税额由后续日终追缴。
#[test]
fn restricted_stock_bonus_basis_assesses_ten_percent_immediately() {
    let restricted = TaxShareClass::StatutoryRestricted {
        release_on: date("2030-06-01"),
        basis: StatutoryRestrictedBasis::FinanceTax2009167,
        qualification_evidence: "nonfloat-lock".into(),
    };
    let book = book_with_bonus_basis(3, 100, restricted);
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(30, 1).unwrap(),
        "解禁前到账的送股税基 = 面值 100 分 × 3 股 × 10%"
    );
}

/// 每笔送股税基合计应纳税额四舍五入（half-up）到分（与每笔分红同一登记口径）：
/// 面值 5 分 × 1 股 × 10% = 0.5 分 → 1 分。
#[test]
fn stock_bonus_basis_rounds_half_up_per_event() {
    let book = book_with_bonus_basis(1, 5, TaxShareClass::PublicMarket);
    let book = dispose_bonus_on(book, "2030-02-03", 1);
    assert_eq!(
        book.outstanding().unwrap(),
        ExactDividendTaxAmount::new(1, 1).unwrap(),
        "0.5 分按 half-up 取整为 1 分"
    );
}

/// 送股税基入账的防御边界：重复入账（同事实幂等、异事实拒绝）、零数量、
/// 非正面值、未知批次、批次日期不符、同一批次二次计基、非送转来源批次。
#[test]
fn record_stock_bonus_basis_rejects_conflicting_duplicates_and_invalid_facts() {
    let mut book = book_with_bonus_basis(3, 100, TaxShareClass::PublicMarket);
    let before = book.clone();
    // 同事件同事实幂等：不报错、不重复。
    book.record_stock_bonus_basis(
        "d1".into(),
        date("2030-01-02"),
        "tax:stock-distribution:d1:account-0".into(),
        3,
        Money::from_cents(100),
    )
    .unwrap();
    assert_eq!(book, before);
    // 同事件异事实（数量不同）显式拒绝。
    assert!(book
        .record_stock_bonus_basis(
            "d1".into(),
            date("2030-01-02"),
            "tax:stock-distribution:d1:account-0".into(),
            2,
            Money::from_cents(100),
        )
        .is_err());
    // 同事件异面值拒绝。
    assert!(book
        .record_stock_bonus_basis(
            "d1".into(),
            date("2030-01-02"),
            "tax:stock-distribution:d1:account-0".into(),
            3,
            Money::from_cents(50),
        )
        .is_err());
    // 未知批次拒绝。
    assert!(book
        .record_stock_bonus_basis(
            "d2".into(),
            date("2030-01-02"),
            "tax:stock-distribution:d2:account-0".into(),
            1,
            Money::from_cents(100),
        )
        .is_err());
    // 记基日期必须等于当前结算日（与到账同日）。
    assert!(book
        .record_stock_bonus_basis(
            "d3".into(),
            date("2030-01-03"),
            "tax:stock-distribution:d1:account-0".into(),
            3,
            Money::from_cents(100),
        )
        .is_err());
    // 同一批次二次计基（不同事件）拒绝。
    assert!(book
        .record_stock_bonus_basis(
            "d4".into(),
            date("2030-01-02"),
            "tax:stock-distribution:d1:account-0".into(),
            3,
            Money::from_cents(100),
        )
        .is_err());
    // 零数量与零面值拒绝。
    let mut zero_day = CashDividendTaxBook::new(
        AccountId(0),
        StockCode("600001".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![],
    )
    .unwrap();
    zero_day
        .record_net_day(
            "market-2030-01-02".into(),
            date("2030-01-02"),
            1,
            Some(DividendTaxLot {
                id: "secondary-lot".into(),
                qty: 1,
                acquired_on: date("2030-01-02"),
                source: TaxAcquisitionSource::SecondaryMarket {
                    settlement: "s".into(),
                },
                class: TaxShareClass::PublicMarket,
            }),
        )
        .unwrap();
    assert!(zero_day
        .record_stock_bonus_basis(
            "d5".into(),
            date("2030-01-02"),
            "secondary-lot".into(),
            0,
            Money::from_cents(100),
        )
        .is_err());
    assert!(zero_day
        .record_stock_bonus_basis(
            "d6".into(),
            date("2030-01-02"),
            "secondary-lot".into(),
            1,
            Money::from_cents(0),
        )
        .is_err());
    // 非送转来源（二级市场）批次不得计送股税基。
    assert!(zero_day
        .record_stock_bonus_basis(
            "d7".into(),
            date("2030-01-02"),
            "secondary-lot".into(),
            1,
            Money::from_cents(100),
        )
        .is_err());
}

/// 送股税基事实严格 serde 往返保持；篡改数量、日期与重复批次在账本级恢复
/// 校验被拒。面值篡改账本级不可检（无公司事实可比对），由 Session 级
/// 与送转声明面值的勾稽承载（见 session 测试）。
#[test]
fn stock_bonus_basis_survives_strict_serde_and_tampering_is_rejected() {
    let book = book_with_bonus_basis(3, 100, TaxShareClass::PublicMarket);
    let restored: CashDividendTaxBook =
        serde_json::from_value(serde_json::to_value(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
    let valid = serde_json::to_value(&book).unwrap();
    let mut tampered = valid.clone();
    tampered["stock_bonus_bases"][0]["qty"] = serde_json::json!(2);
    assert!(serde_json::from_value::<CashDividendTaxBook>(tampered).is_err());
    let mut tampered = valid.clone();
    tampered["stock_bonus_bases"][0]["credited_on"] = serde_json::json!("2030-01-03");
    assert!(serde_json::from_value::<CashDividendTaxBook>(tampered).is_err());
    let mut duplicated = valid.clone();
    let cloned = duplicated["stock_bonus_bases"][0].clone();
    duplicated["stock_bonus_bases"]
        .as_array_mut()
        .unwrap()
        .push(cloned);
    assert!(
        serde_json::from_value::<CashDividendTaxBook>(duplicated).is_err(),
        "同一批次/事件身份的税基重复入账必须在恢复时拒绝"
    );
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("stock_bonus_bases");
    assert!(
        serde_json::from_value::<CashDividendTaxBook>(missing).is_err(),
        "严格契约：缺 stock_bonus_bases 键的旧形状必须显式拒绝"
    );
}
