use engine::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide};
use engine::company::bank::{bank_chart_v3, BankBooks, BankConfig, EclPolicy, EclScenario};
use engine::company::operations::{BankFlowParams, FlowParams, IndustryBooks};
use engine::company::{CompanyKind, CounterpartyId};
use engine::session::{GameSession, SaveSlot, SessionError};

fn bank_save() -> serde_json::Value {
    let mut session = GameSession::new(super::continuity_setup(), super::SEED).unwrap();
    super::run_full_day(&mut session);
    let mut wire = serde_json::to_value(session.save().unwrap()).unwrap();
    let scenarios = vec![EclScenario {
        weight_bp: 10000,
        pd_bp: 100,
        lgd_bp: 5000,
    }];
    let bank = BankBooks::new(BankConfig {
        chart: bank_chart_v3(),
        as_of: engine::CivilDate::from_iso("2030-01-07").unwrap(),
        opening_lines: vec![
            JournalLine {
                account: LedgerAccountId("1003".into()),
                side: PostingSide::Debit,
                amount: AccountingAmount::from_cents(100000),
            },
            JournalLine {
                account: LedgerAccountId("4001".into()),
                side: PostingSide::Credit,
                amount: AccountingAmount::from_cents(100000),
            },
        ],
        counterparties: vec![],
        ecl_policy: EclPolicy {
            version: 1,
            stage1_default: scenarios.clone(),
            lifetime_default: scenarios.clone(),
        },
    })
    .unwrap();
    let company = wire["company_operations"]["companies"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    company["books"] = serde_json::to_value(IndustryBooks::Bank(bank)).unwrap();
    company["spec"]["kind"] = serde_json::to_value(CompanyKind::Bank).unwrap();
    company["params"] = serde_json::to_value(FlowParams::Bank(BankFlowParams {
        depositor: CounterpartyId("fixture-depositor".into()),
        borrower: CounterpartyId("fixture-borrower".into()),
        fee_customer: CounterpartyId("fixture-fee".into()),
        deposit_principal: AccountingAmount::from_cents(100),
        deposit_rate_bp: 100,
        deposit_term_days: 30,
        deposit_every_days: 30,
        loan_principal: AccountingAmount::from_cents(100),
        loan_rate_bp: 200,
        loan_term_days: 30,
        lending_every_days: 30,
        daily_fee_income: AccountingAmount::ZERO,
        credit_deterioration_scenarios: scenarios,
    }))
    .unwrap();
    wire
}

#[test]
fn complete_save_restore_rejects_invalid_bank_ecl_with_company_context() {
    let baseline = bank_save();
    let valid: SaveSlot = serde_json::from_value(baseline.clone()).unwrap();
    GameSession::restore(&valid).expect("合法 Bank 政策不能因默认新局是 Industrial 而被拒绝");
    for (table, field, value) in [
        ("stage1_default", None, serde_json::json!([])),
        ("lifetime_default", None, serde_json::json!([])),
        ("stage1_default", Some("weight_bp"), serde_json::json!(9999)),
        ("lifetime_default", Some("pd_bp"), serde_json::json!(-1)),
    ] {
        let mut wire = baseline.clone();
        let companies = wire["company_operations"]["companies"]
            .as_object_mut()
            .unwrap();
        let (id, company) = companies.iter_mut().next().unwrap();
        let id = id.clone();
        let policy = &mut company["books"]["Bank"]["ecl_policy"];
        match field {
            Some(field) => policy[table][0][field] = value,
            None => policy[table] = value,
        }
        let invalid: SaveSlot = serde_json::from_value(wire).expect("库级 serde 接受集合保持不变");
        let error = match GameSession::restore(&invalid) {
            Err(error) => error,
            Ok(_) => panic!("损坏 ECL 政策不得安装为运行会话"),
        };
        assert!(matches!(error, SessionError::InvalidSave(_)), "{error}");
        assert!(error.to_string().contains("ECL"), "{error}");
        assert!(error.to_string().contains(&id), "{error}");
    }
}
