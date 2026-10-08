// 由当前 SaveSlot 构造真实 Simple 开账报告；传入输入与输出路径，不手补 JSON 四表。
include!("../../packages/engine/test-support/simple_company.rs");
use engine::accounting::AccountingAmount;
use engine::company::config::CompanySystemConfig;
use engine::company::PublicReportQuery;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("必须提供当前 SaveSlot 输入和报告输出路径".into());
    }
    let slot: engine::SaveSlot = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let mut setup = slot.setup;
    setup.stocks.truncate(1);
    let code = setup.stocks.first().ok_or("setup 缺证券")?.code.0.clone();
    setup.stocks[0].float_shares = 0;
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.auto_corporate_foundation = false;
    setup.start_date = engine::CivilDate::from_iso("2030-01-02")?;
    setup.company_system = simple_company_fixture!(engine; codes = [code.as_str()]);
    let CompanySystemConfig::Simple(config) = &mut setup.company_system else {
        unreachable!()
    };
    let company = &mut config.companies[0];
    company.generation.initial_revenue = AccountingAmount::ZERO;
    company.generation.initial_fixed_expense = AccountingAmount::ZERO;
    for line in &mut company.finance.opening_lines {
        line.amount = AccountingAmount::from_cents(900_719_925_474_099_301);
    }
    let session = engine::GameSession::new(setup, 77)?;
    let page = session.query_public_reports(&PublicReportQuery {
        company_id: format!("C-{code}"),
        cursor: None,
        page_size: Some(1),
    })?;
    let report = page.reports.first().ok_or("真实 Engine 未生成已公开报告")?;
    if report.accounting.total_assets != "9007199254740993.01" {
        return Err(format!("超 JS-safe 开账金额漂移：{}", report.accounting.total_assets).into());
    }
    std::fs::write(&args[2], serde_json::to_vec_pretty(report)?)?;
    Ok(())
}
