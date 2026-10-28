//! 直接调用 microstructure，不能让 aggregate 的先行校验掩盖消费方漏迁。

use super::*;

#[test]
fn d3_microstructure_rejects_unclocked_quote_even_without_recovery_arithmetic() {
    let facts = vec![CausalFact {
        sequence: 0,
        time: FactTime {
            phase: crate::TradingPhase::Continuous,
            market_minute: 0,
            stock_clock: None,
            civil: CivilInstant::new(
                crate::CivilDate::from_ymd(2030, 1, 2).unwrap(),
                9 * 3600 + 30 * 60,
            )
            .unwrap(),
        },
        kind: CausalFactKind::Quote(Quote {
            code: StockId(0),
            bid_cents: None,
            ask_cents: None,
            bid_depth: 0,
            ask_depth: 0,
        }),
    }];
    analyze(&facts)
        .expect_err("单条 Quote 没有恢复相减，也必须显式拒绝缺少本股时钟；不能只靠 aggregate 拦截");
}

#[test]
fn d3_microstructure_uses_local_recovery_and_validates_future_quote_domain() {
    // 纯 consumer 算术输入；真实 mixed Calendar producer 另由原 Arc 用例验证。
    let mut facts: Vec<_> = [(0, 100, 100), (240, 102, 0), (720, 109, 100)]
        .into_iter()
        .enumerate()
        .map(|(sequence, (shared, local, depth))| CausalFact {
            sequence: sequence as u64,
            time: FactTime {
                phase: crate::TradingPhase::Continuous,
                market_minute: shared,
                stock_clock: Some(StockFactClock {
                    code: StockId(0),
                    completed_minutes: local,
                }),
                civil: CivilInstant::new(
                    crate::CivilDate::from_ymd(2030, 1, 2).unwrap(),
                    34_200 + sequence as u32 * 60,
                )
                .unwrap(),
            },
            kind: CausalFactKind::Quote(Quote {
                code: StockId(0),
                bid_cents: Some(990),
                ask_cents: Some(1_010),
                bid_depth: depth,
                ask_depth: depth,
            }),
        })
        .collect();
    let (_, _, recovery) = analyze(&facts).unwrap();
    assert_eq!(recovery.len(), 1);
    assert_eq!(
        (
            recovery[0].loss_sequence,
            recovery[0].recovered_sequence,
            recovery[0].market_minutes
        ),
        (1, Some(2), Some(7))
    );
    facts[2].time.market_minute = 239;
    assert_eq!(
        analyze(&facts).unwrap_err(),
        CausalError::Time(2),
        "shared 端点也须独立拒绝回退"
    );
    facts[2].time.market_minute = 720;
    facts[2]
        .time
        .stock_clock
        .as_mut()
        .unwrap()
        .completed_minutes = 101;
    assert_eq!(analyze(&facts).unwrap_err(), CausalError::Time(2));
    facts[2].time.stock_clock.as_mut().unwrap().code = StockId(1);
    assert_eq!(
        analyze(&facts).unwrap_err(),
        CausalError::StockClock {
            sequence: 2,
            expected: Some(StockId(0)),
            actual: Some(StockId(1))
        }
    );
    let mut root = facts[0].clone();
    root.kind = CausalFactKind::Decision {
        account: AccountId(0),
    };
    assert_eq!(
        analyze(&[root]).unwrap_err(),
        CausalError::StockClock {
            sequence: 0,
            expected: None,
            actual: Some(StockId(0))
        }
    );
}

#[test]
fn d3_stock_fact_clock_serializes_both_full_u64_minutes_as_decimal_strings() {
    let time = FactTime {
        phase: crate::TradingPhase::Continuous,
        market_minute: u64::MAX,
        stock_clock: Some(StockFactClock {
            code: StockId(0),
            completed_minutes: u64::MAX,
        }),
        civil: CivilInstant::new(crate::CivilDate::from_ymd(2030, 1, 2).unwrap(), 34_200).unwrap(),
    };
    let wire = serde_json::to_value(time).unwrap();
    assert_eq!(wire["market_minute"], u64::MAX.to_string());
    assert_eq!(
        wire["stock_clock"]["completed_minutes"],
        u64::MAX.to_string()
    );
    assert_eq!(wire["stock_clock"]["code"], 0);
}
