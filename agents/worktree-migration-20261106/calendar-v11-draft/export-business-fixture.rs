//! V11 tests-first 验收源码：单个真实 production Panic NPC 与玩家对手的公共日终输入。
#[allow(dead_code)]
#[path = "../../../../packages/engine/tests/common/mixed_calendar_c2c_fixture.rs"]
mod fixture;
use engine::calendar::CalendarExchange;
use engine::session::protocol::ProtocolSession;
use engine::strategy::{RetailStyle, StrategyProfile};
use engine::{AccountId, GameSession, Intent, LimitPrice, Money, Side, StockExchange};

fn write_new(path: &std::path::Path, value: &impl serde::Serialize) {
    let bytes = serde_json::to_vec_pretty(value).unwrap();
    let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap();
    std::io::Write::write_all(&mut output, &bytes).unwrap();
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert_eq!(args.len(), 1, "必须提供新的独占fixture目录");
    let out = std::path::Path::new(&args[0]);std::fs::create_dir(out).unwrap();
    let template = fixture::initial_save(CalendarExchange::Sse, StockExchange::Shanghai, false);
    let mut setup = template.setup.clone();
    setup.start_date = fixture::date("2030-01-01");setup.npcs.retail_count = 1;
    // 已登记的可选price cage简化；两端同日限价均等于该股真实日跌停价。
    setup.config.price_cage_enabled = false;
    for stock in &mut setup.stocks { stock.float_shares = 1_000; }
    let initial = GameSession::new(setup, 40).unwrap();
    let mut save = initial.save().unwrap();
    let npc = AccountId(1);
    assert_eq!(save.runtime_state.strategy_states[&npc].profile(), StrategyProfile::Retail(RetailStyle::Panic),
        "固定seed40必须由真实工厂产生Panic；不符合即失败，不自动换seed");
    // 只在首次restore前编辑完整存档事实；保原production策略身份、个人态和独立RNG。
    let mut state = serde_json::to_value(&save.runtime_state.strategy_states[&npc]).unwrap();
    assert!(state["ZiNoise"].get("base_observation_probability").is_some());
    state["ZiNoise"]["base_observation_probability"] = serde_json::json!("3ff0000000000000");
    save.runtime_state.strategy_states.insert(npc, serde_json::from_value(state).unwrap());
    let attention = save.npc_attention.get_mut(&npc).unwrap();
    attention.base_probability = 1.0;attention.next_attention_candidate_tick = 0;
    for position in save.snapshot.accounts.get_mut(&npc).unwrap().positions.values_mut() {
        assert_eq!((position.qty, position.t1_locked), (1_000, 0));
        position.invested_cents = i64::from(position.qty) * 100;
        assert_eq!(position.recovered_cents, 0);
    }
    save.civil_clock.policy = template.civil_clock.policy;
    let mut game = GameSession::restore(&save).expect("完整合法Panic/成本/概率事实必须经真实strict restore");
    let closed = game.stock_id("600101").unwrap();let open = game.stock_id("000101").unwrap();
    let day = game.end_civil_day().expect("Jan1全闭市必须真实civil提交，不手填settled_through");
    assert_eq!(day.settled_date, fixture::date("2030-01-01"));
    assert_eq!(game.civil_date(), fixture::date("2030-01-02"));
    assert_eq!((game.tick(), game.day()), (0, 0));
    let public = game.save().unwrap();
    ProtocolSession::restore(&public).expect("唯一导出字节的真实公共Protocol恢复必须先成功");
    write_new(&out.join("fixture.json"), &public);
    let buy = |code, price| Intent::PlaceLimit { code, side: Side::Buy, price: LimitPrice::Fixed(Money::from_cents(price)), qty: 100 };
    write_new(&out.join("script.json"), &serde_json::json!({ "closed": closed.0, "open": open.0, "retainedDate": "2030-01-02", "finalRetainedDate": "2030-01-03", "steps": [
        { "intents": [buy(closed, 900), buy(open, 900)] }, { "intents": [] }, { "intents": [] },
        { "intents": [buy(open, 810)] }, { "intents": [] }, { "intents": [] }
    ] }));
    println!("已导出真实公共日终fixture与固定六步输入；尚未证明Trade或WASM对拍");
}
