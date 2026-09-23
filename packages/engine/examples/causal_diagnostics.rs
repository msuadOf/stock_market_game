#[path = "../tests/diagnostic_parity.rs"]
mod fixture;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut setup = fixture::setup();
    setup.stocks[0].code = engine::StockCode("000812".to_owned());
    setup.stocks[0].exchange = engine::StockExchange::Shenzhen;
    setup.stocks[0].initial_price = engine::Money::from_cents(285);
    setup.stocks[0].total_shares = 1_000_000;
    setup.stocks[0].float_shares = 400_000;
    setup.npcs.inst_count = 20;
    let mut session = engine::GameSession::new(setup, 7)?;
    for _ in 0..600 {
        session.step()?;
    }
    let report = session.causal_diagnostics()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "report": report, "facts": session.causal_facts(),
            "interpretation": "observational execution response, not a causal estimate"
        }))?
    );
    Ok(())
}
