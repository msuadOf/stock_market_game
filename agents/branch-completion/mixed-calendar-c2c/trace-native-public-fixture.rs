//! V11 的 native Protocol 对照记录器；公共输入与 WASM restore_json 完全相同。

use engine::session::protocol::{EngineUpdate, ProtocolSession};
use engine::session::{CurrentMinuteHistoryRequest, MarketHistoryRequest};
use engine::{AccountId, SaveDecodeLimits, StockId};
use serde_json::{json, Value};

fn observe(session: &mut ProtocolSession, closed: StockId, open: StockId) -> Value {
    let mut minute = serde_json::Map::new();
    for stock in [closed, open] {
        let response = session
            .query_current_minute_history_for(AccountId(0), &CurrentMinuteHistoryRequest { stock })
            .expect("健康公共分钟查询必须成功");
        minute.insert(stock.0.to_string(), serde_json::to_value(response).unwrap());
    }
    let mut retained = serde_json::Map::new();
    if session.civil_date() > engine::calendar::CivilDate::from_iso("2030-01-02").unwrap() {
        for stock in [closed, open] {
            let response = session
                .query_market_history_for(
                    AccountId(0),
                    &MarketHistoryRequest {
                        stock,
                        date_from: engine::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
                        date_to: engine::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
                        after: None,
                        page_size: 1,
                    },
                )
                .expect("健康公共 retained 查询必须成功");
            retained.insert(stock.0.to_string(), serde_json::to_value(response).unwrap());
        }
    }
    json!({
        "civilDate": session.civil_date().to_iso(),
        "snapshot": session.snapshot(),
        "runtimeSnapshot": session.runtime_snapshot(),
        "currentMinute": minute,
        "retained": retained,
    })
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let input = arguments.next().expect("必须提供同源公共 SaveSlot JSON");
    let destination = arguments.next().expect("必须提供独占 trace JSON 路径");
    assert!(arguments.next().is_none(), "不接受多余参数");
    let encoded = std::fs::read(input).expect("输入文件必须可读");
    let save = engine::decode_save_slot(&encoded, &SaveDecodeLimits::default())
        .expect("实际输入字节必须通过 strict SaveSlot decode");
    assert_eq!(
        save.setup.ticks_per_day, 2,
        "本记录器只接两拍代表性 fixture"
    );
    let mut session = ProtocolSession::restore(&save).expect("真实公共 restore 必须成功");
    let closed = session.stock_id("600101").expect("必须含 SSE fixture 股");
    let open = session.stock_id("000101").expect("必须含 SZSE fixture 股");
    assert_eq!(session.civil_date().to_iso(), "2030-01-02");
    assert_eq!((session.tick(), session.day()), (0, 0));
    let initial = observe(&mut session, closed, open);
    let mut frames = Vec::new();
    // 与 SessionRegistry::step_update 相同公共编排：两拍→civil→复市两拍→civil。
    for _ in 0..6 {
        let update = if session.civil_day_ready().expect("真实 civil readiness") {
            EngineUpdate::CivilUpdate(Box::new(
                session.end_civil_day_update().expect("真实自然日日结"),
            ))
        } else {
            let frame = session.step_frame().expect("真实 Protocol 市场拍");
            EngineUpdate::TickBatch(Box::new(
                session.tick_batch(vec![frame]).expect("真实协议 TickBatch"),
            ))
        };
        frames.push(json!({ "update": update, "view": observe(&mut session, closed, open) }));
    }
    let trace = json!({ "closed": closed.0, "open": open.0, "initial": initial, "frames": frames });
    let encoded = serde_json::to_vec_pretty(&trace).expect("trace 必须可序列化");
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .expect("只能写入尚不存在的独占 trace，禁止覆盖旧证据");
    std::io::Write::write_all(&mut output, &encoded).expect("trace 写入失败必须显式报告");
    println!("native Protocol trace 已记录，字节数={}", encoded.len());
}
