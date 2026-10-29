#[path = "publications/session_fixture.rs"]
mod session_fixture;

use engine::session::StockIdentityEntry;
use engine::{CivilDate, GameSession, StockCode, StockId};

#[test]
fn actual_stock_identity_preserves_catalog_ids_after_restore() {
    let mut setup = session_fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    let template = setup.stocks[0].clone();
    setup.stocks = ["600610", "002156"]
        .into_iter()
        .map(|code| {
            let mut stock = template.clone();
            stock.code = StockCode(code.to_owned());
            stock.exchange = if code.starts_with('6') {
                engine::session::StockExchange::Shanghai
            } else {
                engine::session::StockExchange::Shenzhen
            };
            stock.float_shares = 0;
            stock
        })
        .collect();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.history_len = 1;
    let game = GameSession::new(setup, 42).unwrap();
    let expected = vec![
        StockIdentityEntry {
            id: StockId(0),
            code: "002156".to_owned(),
        },
        StockIdentityEntry {
            id: StockId(1),
            code: "600610".to_owned(),
        },
    ];
    assert_eq!(game.stock_catalog_view(), expected);
    let restored = GameSession::restore(&game.save().unwrap()).unwrap();
    assert_eq!(restored.stock_catalog_view(), expected);
}
