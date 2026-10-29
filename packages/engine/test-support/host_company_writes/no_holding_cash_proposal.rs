fn no_holding_cash_proposal() -> serde_json::Value {
    serde_json::to_value(engine::session::PlayerCompanyProposal::CashDividend {
        company: engine::company::CompanyId(0),
        gross_per_share: engine::Money::from_cents(1),
    })
    .unwrap()
}
