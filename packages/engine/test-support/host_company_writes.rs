// 三宿主写入口共用的测试输入；不产生完成事实，也不执行公司算法。
fn company_write_setup(mut setup: engine::SessionSetup) -> engine::SessionSetup {
    setup.start_date = engine::CivilDate::from_iso("2030-06-03").unwrap();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.history_len = 1;
    setup.stocks[0].float_shares = 0;
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode = Some(engine::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000);
    setup.par_value_per_share = Some(engine::Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    setup
}

fn no_holding_cash_proposal() -> serde_json::Value {
    serde_json::to_value(engine::session::PlayerCompanyProposal::CashDividend {
        company: engine::company::CompanyId(0),
        gross_per_share: engine::Money::from_cents(1),
    })
    .unwrap()
}

fn host_correction_request(publications: &serde_json::Value) -> serde_json::Value {
    use engine::accounting::{
        AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
        LedgerAccountId, PostingSide,
    };
    let original = publications["reports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|report| report["source"] == "SimpleGenerated")
        .expect("真实 Simple 新局必须有可更正的公开报告");
    let supersedes = original["id"].as_str().unwrap().parse::<u32>().unwrap();
    let journal = JournalEntry {
        source: BusinessEventId::new(900_000_001),
        date: engine::CivilDate::from_iso("2030-06-03").unwrap(),
        kind: BusinessKind::CashRevenue,
        cash_flow: CashFlowClass::Operating,
        lines: vec![
            JournalLine {
                account: LedgerAccountId("1002".into()),
                side: PostingSide::Debit,
                amount: AccountingAmount::from_cents(1000),
            },
            JournalLine {
                account: LedgerAccountId("6001".into()),
                side: PostingSide::Credit,
                amount: AccountingAmount::from_cents(1000),
            },
        ],
    };
    journal.validate_invariants().unwrap();
    serde_json::json!({
        "operation_id":"host-correction-1", "company":0, "supersedes":supersedes,
        "entries":[journal], "reason":"补记已收现收入，入队不等同公布更正",
    })
}
