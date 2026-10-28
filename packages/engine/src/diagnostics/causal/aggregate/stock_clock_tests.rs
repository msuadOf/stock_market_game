//! 纯报告输入的域拒绝首红；不将该输入称为真实跨日委托或 producer 证据。

use super::*;

#[test]
fn d3_aggregate_rejects_unclocked_security_facts_even_when_shared_lifetime_is_valid() {
    let date = crate::CivilDate::from_ymd(2030, 1, 2).unwrap();
    let before = FactTime {
        phase: crate::TradingPhase::Continuous,
        market_minute: 0,
        stock_clock: None,
        civil: CivilInstant::new(date, 9 * 3600 + 30 * 60).unwrap(),
    };
    let after = FactTime {
        market_minute: 8,
        stock_clock: None,
        civil: CivilInstant::new(date, 9 * 3600 + 38 * 60).unwrap(),
        ..before
    };
    let origin = OrderOrigin {
        order: OrderId(1),
        account: AccountId(0),
        code: StockId(0),
        company: None,
        plan: None,
        decision: None,
        side: Side::Buy,
        qty: 100,
    };
    let facts = vec![
        CausalFact {
            sequence: 0,
            time: before,
            kind: CausalFactKind::Submitted(origin),
        },
        CausalFact {
            sequence: 1,
            time: after,
            kind: CausalFactKind::Terminated {
                order: OrderId(1),
                account: AccountId(0),
                code: StockId(0),
                qty: 100,
                reason: Termination::Voluntary,
            },
        },
    ];
    CausalReport::from_facts(7, &facts)
        .expect_err("shared 寿命可相减且数量守恒，也不能接受缺少本股时钟的证券事实");
}

fn clocked_fact(sequence: u64, shared: u64, local: u64, kind: CausalFactKind) -> CausalFact {
    CausalFact {
        sequence,
        time: FactTime {
            phase: crate::TradingPhase::Continuous,
            market_minute: shared,
            stock_clock: kind.stock_code().map(|code| StockFactClock {
                code,
                completed_minutes: local,
            }),
            civil: CivilInstant::new(
                crate::CivilDate::from_ymd(2030, 1, 2).unwrap(),
                34_200 + sequence as u32 * 60,
            )
            .unwrap(),
        },
        kind,
    }
}

#[test]
fn d3_aggregate_checks_every_fact_clock_domain_before_order_arithmetic() {
    let code = StockId(7);
    let quote = Quote {
        code,
        bid_cents: None,
        ask_cents: None,
        bid_depth: 0,
        ask_depth: 0,
    };
    let kinds = [
        CausalFactKind::Submitted(OrderOrigin {
            order: OrderId(1),
            account: AccountId(0),
            code,
            company: None,
            plan: None,
            decision: None,
            side: Side::Buy,
            qty: 100,
        }),
        CausalFactKind::Filled {
            order: OrderId(1),
            account: AccountId(0),
            code,
            qty: 100,
            value_before: 0,
            gross: 100_000,
        },
        CausalFactKind::Terminated {
            order: OrderId(1),
            account: AccountId(0),
            code,
            qty: 100,
            reason: Termination::DayEnd,
        },
        CausalFactKind::Execution {
            code,
            maker: OrderId(1),
            taker: OrderId(2),
            side: Some(Side::Buy),
            qty: 100,
            price_cents: 1_000,
            before: quote.clone(),
        },
        CausalFactKind::Quote(quote),
    ];
    for kind in kinds {
        for actual in [None, Some(StockId(8))] {
            let mut fact = clocked_fact(0, 100, 20, kind.clone());
            fact.time.stock_clock = actual.map(|code| StockFactClock {
                code,
                completed_minutes: 20,
            });
            assert_eq!(
                CausalReport::from_facts(7, &[fact]).unwrap_err(),
                CausalError::StockClock {
                    sequence: 0,
                    expected: Some(code),
                    actual
                }
            );
        }
    }
    let now = CivilInstant::new(crate::CivilDate::from_ymd(2030, 1, 2).unwrap(), 34_200).unwrap();
    for kind in [
        CausalFactKind::Decision {
            account: AccountId(0),
        },
        CausalFactKind::Budget {
            account: AccountId(0),
            available_cents: 0,
            allocated_cents: vec![],
        },
        CausalFactKind::Acquisition {
            account: AccountId(0),
            company: crate::company::CompanyId(0),
            publication: 1,
            published: now,
            acquired: now,
        },
        CausalFactKind::ObservationRestart,
    ] {
        let mut fact = clocked_fact(0, 100, 20, kind);
        fact.time.stock_clock = Some(StockFactClock {
            code,
            completed_minutes: 20,
        });
        assert_eq!(
            CausalReport::from_facts(7, &[fact]).unwrap_err(),
            CausalError::StockClock {
                sequence: 0,
                expected: None,
                actual: Some(code)
            }
        );
    }
}

#[test]
fn d3_aggregate_uses_stock_lifetime_and_rejects_only_local_regression_explicitly() {
    // 纯 consumer 算术输入；不声称这种 shared/local 差发生于真实同日委托。
    let origin = OrderOrigin {
        order: OrderId(1),
        account: AccountId(0),
        code: StockId(7),
        company: None,
        plan: None,
        decision: None,
        side: Side::Buy,
        qty: 100,
    };
    let mut facts = vec![
        clocked_fact(0, 240, 100, CausalFactKind::Submitted(origin)),
        clocked_fact(
            1,
            480,
            107,
            CausalFactKind::Terminated {
                order: OrderId(1),
                account: AccountId(0),
                code: StockId(7),
                qty: 100,
                reason: Termination::Voluntary,
            },
        ),
    ];
    let report = CausalReport::from_facts(7, &facts).unwrap();
    assert_eq!(report.orders[0].lifetime_market_minutes, Some(7));
    assert_eq!(report.orders[0].lifetime_civil_seconds, Some(60));
    facts[1]
        .time
        .stock_clock
        .as_mut()
        .unwrap()
        .completed_minutes = 99;
    assert_eq!(
        CausalReport::from_facts(7, &facts).unwrap_err(),
        CausalError::Time(1)
    );
}
