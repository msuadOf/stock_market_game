//! §3.3 分录级派生与 §3.6 细项用例：Simple 结算周期必须由收入／固定费用／
//! 变动费用各自生成**逐项分录**（独立 `BusinessKind` + 独立来源），再由同一
//! 账簿事实派生税前利润、净利润、权益、ROE 与账面展示字段；同一费用不得在
//! 分录级重复计入或混入其他条目；结算失败不得留下部分状态。

use super::tests::{amount, date, fixture};
use super::*;
use crate::company::simple::period::{PeriodNoiseConfig, SettlementCycle, TrendSegment};

/// 结算末日账簿中「逐项分录」的期望形状（kind 字符串 + 借贷行）。
/// 用字符串断言以兼容红测阶段（旧实现只有混合摘要分录，红测仍可编译运行）。
fn kind_string(kind: &BusinessKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn line_tuple(line: &JournalLine) -> (String, PostingSide, AccountingAmount) {
    (line.account.0.clone(), line.side, line.amount)
}

#[test]
fn settlement_posts_one_journal_entry_per_explicit_item_with_dedicated_kinds() {
    let mut state = fixture();
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(10_000),
                fixed_expense: amount(3_000),
                variable_expense: amount(4_000),
            },
        )
        .unwrap();
    let entries: Vec<&JournalEntry> = state
        .books()
        .journal()
        .entries()
        .filter(|entry| {
            entry.date == date(1, 31)
                && matches!(
                    kind_string(&entry.kind).as_str(),
                    "SimpleRevenueAccrual"
                        | "SimpleFixedExpenseAccrual"
                        | "SimpleVariableExpenseAccrual"
                )
        })
        .collect();
    let blended: Vec<&JournalEntry> = state
        .books()
        .journal()
        .entries()
        .filter(|entry| {
            entry.date == date(1, 31) && kind_string(&entry.kind).as_str() == "SimplePeriodSummary"
        })
        .collect();
    assert!(blended.is_empty(), "不得再用混合摘要分录承载多项明确项目");
    let expected: Vec<(&str, Vec<(String, PostingSide, AccountingAmount)>)> = vec![
        (
            "SimpleRevenueAccrual",
            vec![
                (
                    "simple_receivable".into(),
                    PostingSide::Debit,
                    amount(10_000),
                ),
                ("simple_revenue".into(), PostingSide::Credit, amount(10_000)),
            ],
        ),
        (
            "SimpleFixedExpenseAccrual",
            vec![
                (
                    "simple_fixed_expense".into(),
                    PostingSide::Debit,
                    amount(3_000),
                ),
                ("simple_payable".into(), PostingSide::Credit, amount(3_000)),
            ],
        ),
        (
            "SimpleVariableExpenseAccrual",
            vec![
                (
                    "simple_variable_expense".into(),
                    PostingSide::Debit,
                    amount(4_000),
                ),
                ("simple_payable".into(), PostingSide::Credit, amount(4_000)),
            ],
        ),
    ];
    assert_eq!(
        entries.len(),
        expected.len(),
        "结算必须按明确项目逐项入账，而不是混合摘要分录：{:?}",
        entries
            .iter()
            .map(|entry| kind_string(&entry.kind))
            .collect::<Vec<_>>()
    );
    let mut previous_source = 0;
    for (entry, (kind, lines)) in entries.iter().zip(expected.iter()) {
        assert_eq!(&kind_string(&entry.kind), kind);
        assert_eq!(entry.cash_flow, CashFlowClass::NonCash);
        assert!(
            entry.source.value() > previous_source,
            "逐项分录来源必须唯一且递增"
        );
        previous_source = entry.source.value();
        assert_eq!(
            entry.lines.iter().map(line_tuple).collect::<Vec<_>>(),
            *lines
        );
    }
}

#[test]
fn settlement_entries_derive_pretax_profit_net_income_equity_and_display_fields() {
    let mut state = fixture();
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(10_000),
                fixed_expense: amount(3_000),
                variable_expense: amount(4_000),
            },
        )
        .unwrap();
    let revenue = state
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId(
            crate::accounting::reports::simple_summary::REVENUE.into(),
        ))
        .unwrap()
        .neg()
        .unwrap();
    let fixed = state
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId(
            crate::accounting::reports::simple_summary::FIXED_EXPENSE.into(),
        ))
        .unwrap();
    let variable = state
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId(
            crate::accounting::reports::simple_summary::VARIABLE_EXPENSE.into(),
        ))
        .unwrap();
    // 税前利润由同一分录事实派生（收入 − 固定 − 变动），不得另设来源。
    assert_eq!(
        revenue.sub(fixed).unwrap().sub(variable).unwrap(),
        amount(3_000)
    );
    let report = state
        .report(
            AccountingPeriod::from_ymd(2030, 1).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report.validate().unwrap();
    // 25% 所得税 → 净利润 2250 分；权益 = 期初 10000 + 2250。
    assert_eq!(report.income.cumulative.net_income, amount(2_250));
    assert_eq!(
        state.books().ledger().equity_rolling().unwrap(),
        amount(12_250)
    );
    // 账面展示字段从同一分录事实派生：cash_book = 累计净利润 − 已付分红（0）；
    // investment_book = 累计收入 × 3000bp。
    assert_eq!(state.cash_book(), amount(2_250));
    assert_eq!(state.investment_book(), amount(3_000));
}

/// §3.6 负例：收入比例规则的费用不得同时携带独立金额增长轨迹（同一费用
/// 不得由金额增长和收入比例重复生成）。守卫存在于 `validate_generated_amounts`，
/// 本用例补齐明确的负例覆盖。
#[test]
fn revenue_ratio_rule_rejects_explanation_with_independent_growth_track() {
    let zero = || PeriodNoiseConfig {
        monthly_bp: 0,
        quarterly_bp: 0,
        half_year_bp: 0,
        annual_bp: 0,
    };
    let config = crate::company::simple::period::PeriodGenerationParameters {
        initial_revenue: amount(100_000),
        initial_fixed_expense: amount(10_000),
        revenue_trend: crate::company::simple::period::AnnualTrendConfig::Fixed {
            annual_growth_bp: 0,
        },
        fixed_expense_trend: crate::company::simple::period::AnnualTrendConfig::Fixed {
            annual_growth_bp: 0,
        },
        revenue_noise: zero(),
        fixed_expense_noise: zero(),
        demand_sensitivity_bp: 0,
        variable_expense: crate::company::simple::period::PeriodVariableExpenseRule::RevenueRatio {
            ratio_bp: 4_000,
            noise: zero(),
        },
    };
    let initial = config
        .initial_state(crate::company::rng::OperatingRng::from_state(1))
        .unwrap();
    let result = crate::company::simple::period::generate_period(
        &config,
        &initial,
        SettlementCycle::Monthly,
        0,
        None,
    )
    .unwrap();
    let mut drifted = result.explanation.clone();
    // 篡改解释：给收入比例规则的费用挂上独立金额增长轨迹。
    drifted.variable_expense_segments = vec![TrendSegment {
        annual_growth_bp: 0,
        months: 1,
    }];
    let error = crate::company::simple::period::validate_generated_amounts(
        &config,
        &result.state.amounts,
        &drifted,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("收入比例费用不得携带独立金额增长轨迹"),
        "{error}"
    );
    // 对照：未篡改的解释必须通过。
    crate::company::simple::period::validate_generated_amounts(
        &config,
        &result.state.amounts,
        &result.explanation,
    )
    .unwrap();
}

/// §3.6 失败原子性：结算期间在逐项过账开始**之后**失败（第二期费用科目累计
/// 溢出），整个状态不得留下任何部分变化——分录、账面展示字段、税账三侧
/// 全部与失败前深度相等。
#[test]
fn deep_failure_after_first_period_posting_leaves_zero_partial_state() {
    let mut state = fixture();
    state
        .apply_month(
            date(1, 31),
            &PeriodAmounts {
                revenue: amount(1_000),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    let before = serde_json::to_vec(&state).unwrap();
    let before_entries = state.books().journal().entry_count();
    let before_display = state.book_display();
    let before_tax = serde_json::to_vec(state.income_tax_position()).unwrap();
    // 第二期收入可正常过账，但税额计算（收入 × 2500bp 中间积）溢出——失败
    // 发生在逐项分录过账开始**之后**，是真正的深部失败点。
    let huge_revenue = AccountingAmount::from_cents(i128::MAX / 1_000);
    let error = state.apply_month(
        date(2, 28),
        &PeriodAmounts {
            revenue: huge_revenue,
            fixed_expense: amount(0),
            variable_expense: amount(0),
        },
    );
    assert!(error.is_err(), "第二期税额计算溢出必须显式失败");
    assert_eq!(
        serde_json::to_vec(&state).unwrap(),
        before,
        "分录侧零部分状态"
    );
    assert_eq!(state.books().journal().entry_count(), before_entries);
    assert_eq!(state.book_display(), before_display, "展示侧零部分状态");
    assert_eq!(
        serde_json::to_vec(state.income_tax_position()).unwrap(),
        before_tax,
        "税账侧零部分状态"
    );
}

// —— CompanySystem 级恢复勾稽（需要 history 与 finance 两侧同时在场）——

fn system_spec() -> crate::company::CompanySpec {
    use crate::company::{CompanyKind, CompanySpec, IndustryId};
    CompanySpec {
        id: CompanyId("A".into()),
        name: "分录级勾稽公司".into(),
        industry: IndustryId("虚拟行业".into()),
        kind: CompanyKind::Industrial,
        listed_stock: Some(crate::StockCode("600101".into())),
        issued_shares: 1000,
        group_parent: None,
    }
}

fn system_config() -> crate::company::simple::SimpleConfig {
    use crate::company::simple::period::{
        AnnualTrendConfig, PeriodGenerationParameters, PeriodVariableExpenseRule,
    };
    use crate::company::simple::{
        SimpleCompanyConfig, SimpleCompanyPreferences, SimpleEnvironmentConfig,
    };
    let zero = || PeriodNoiseConfig {
        monthly_bp: 0,
        quarterly_bp: 0,
        half_year_bp: 0,
        annual_bp: 0,
    };
    crate::company::simple::SimpleConfig {
        environment: SimpleEnvironmentConfig {
            initial_change_bp: 0,
            persistence_bp: 0,
            noise: zero(),
        },
        companies: vec![SimpleCompanyConfig {
            company: CompanyId("A".into()),
            kind: crate::company::CompanyKind::Industrial,
            generation: PeriodGenerationParameters {
                initial_revenue: amount(100_000),
                initial_fixed_expense: amount(30_000),
                revenue_trend: AnnualTrendConfig::Fixed {
                    annual_growth_bp: 0,
                },
                fixed_expense_trend: AnnualTrendConfig::Fixed {
                    annual_growth_bp: 0,
                },
                revenue_noise: zero(),
                fixed_expense_noise: zero(),
                demand_sensitivity_bp: 0,
                variable_expense: PeriodVariableExpenseRule::RevenueRatio {
                    ratio_bp: 4_000,
                    noise: zero(),
                },
            },
            finance: fixture().config().clone(),
            preferences: SimpleCompanyPreferences::none(),
        }],
        prehistory_periods: 1,
        settlement_cycle: SettlementCycle::Monthly,
    }
}

fn create_system() -> crate::company::CompanySystem {
    crate::company::CompanySystem::create(
        vec![system_spec()],
        crate::company::config::CompanySystemConfig::Simple(system_config()),
        date(1, 1),
        19,
    )
    .unwrap()
}

/// 定位存档 JSON 中指定 kind 结算分录所在的 (batch_index, entry_index)。
fn find_settlement_entry(batches: &serde_json::Value, kind: &str) -> Option<(usize, usize)> {
    for (batch_index, batch) in batches.as_array()?.iter().enumerate() {
        for (entry_index, entry) in batch.as_array()?.iter().enumerate() {
            if entry["kind"].as_str() == Some(kind) {
                return Some((batch_index, entry_index));
            }
        }
    }
    None
}

fn batches_of(value: &mut serde_json::Value) -> &mut serde_json::Value {
    &mut value["implementation"]["state"]["companies"]["A"]["finance"]["books"]["journal"]
        ["batches"]
}

/// §3.6 负例（分录级重复计入／形状漂移／缺失）：结算分录是逐项一次性事实，
/// 勾稽按 history 生成的金额重构——把固定费用的行混进变动费用分录（合计
/// 不变，报告重算无法察觉）、整笔复制（重复计入）或删除（缺失），恢复都
/// 必须以分录级错误显式拒绝。
#[test]
fn restore_rejects_drifted_duplicated_or_missing_settlement_entries() {
    let original = serde_json::to_value(create_system()).unwrap();

    // 场景一：形状漂移——固定费用行并入变动费用分录并删除固定费用分录
    //（科目合计不变，报告重算结果相同，只有分录级勾稽能拒绝）。
    let mut drifted = original.clone();
    {
        let batches = batches_of(&mut drifted);
        let fixed = find_settlement_entry(batches, "SimpleFixedExpenseAccrual")
            .expect("前史必须已有固定费用分录");
        let variable = find_settlement_entry(batches, "SimpleVariableExpenseAccrual")
            .expect("前史必须已有变动费用分录");
        let fixed_lines = batches[fixed.0][fixed.1]["lines"].clone();
        let merged = [
            fixed_lines.as_array().unwrap().clone(),
            batches[variable.0][variable.1]["lines"]
                .as_array()
                .unwrap()
                .clone(),
        ]
        .concat();
        batches[variable.0][variable.1]["lines"] = serde_json::Value::Array(merged);
        batches[fixed.0].as_array_mut().unwrap().remove(fixed.1);
    }
    let error = serde_json::from_value::<crate::company::CompanySystem>(drifted)
        .expect_err("形状漂移的结算分录必须被恢复勾稽拒绝");
    assert!(
        error.to_string().contains("结算分录"),
        "必须由分录级勾稽显式拒绝，实际：{error}"
    );

    // 场景二：整笔复制（金额翻倍，属于重复计入负例）。
    let mut duplicated = original.clone();
    {
        let batches = batches_of(&mut duplicated);
        let revenue =
            find_settlement_entry(batches, "SimpleRevenueAccrual").expect("前史必须已有收入分录");
        let mut copy = batches[revenue.0][revenue.1].clone();
        copy["source"] = serde_json::json!(9_000_000_001_u64);
        batches[revenue.0].as_array_mut().unwrap().push(copy);
        let finance = &mut duplicated["implementation"]["state"]["companies"]["A"]["finance"];
        finance["next_event_id"] = serde_json::json!("9000000002");
    }
    let error = serde_json::from_value::<crate::company::CompanySystem>(duplicated)
        .expect_err("复制结算分录（重复计入）必须被恢复勾稽拒绝");
    assert!(
        error.to_string().contains("结算分录"),
        "必须由分录级勾稽显式拒绝，实际：{error}"
    );

    // 场景三：缺失（已生成期间被删去一笔结算分录）。
    let mut missing = original;
    {
        let batches = batches_of(&mut missing);
        let fixed = find_settlement_entry(batches, "SimpleFixedExpenseAccrual")
            .expect("前史必须已有固定费用分录");
        batches[fixed.0].as_array_mut().unwrap().remove(fixed.1);
    }
    let error = serde_json::from_value::<crate::company::CompanySystem>(missing)
        .expect_err("缺失结算分录必须被恢复勾稽拒绝");
    assert!(
        error.to_string().contains("缺少对应结算分录"),
        "必须由分录级勾稽显式拒绝，实际：{error}"
    );
}
