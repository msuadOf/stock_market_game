//! V11 tests-first 验收源码：native读取与实际wasm32相同的公共SaveSlot字节与固定玩家输入脚本。
use engine::session::protocol::{EngineUpdate, ProtocolSession};
use engine::session::{CurrentMinuteHistoryRequest, MarketHistoryRequest};
use engine::{AccountId, Intent, SaveDecodeLimits, StockId};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    intents: Vec<Intent>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    closed: u32,
    open: u32,
    #[serde(rename = "retainedDate")]
    retained_date: String,
    #[serde(rename = "finalRetainedDate")]
    final_retained_date: String,
    steps: Vec<Step>,
}

fn observe(session: &mut ProtocolSession, script: &Script) -> Value {
    let retained_date = engine::calendar::CivilDate::from_iso(&script.retained_date).unwrap();
    let final_date = engine::calendar::CivilDate::from_iso(&script.final_retained_date).unwrap();
    let through = if session.civil_date() > final_date {
        final_date
    } else {
        retained_date
    };
    let mut minute = serde_json::Map::new();
    let mut retained = serde_json::Map::new();
    for stock in [StockId(script.closed), StockId(script.open)] {
        minute.insert(
            stock.0.to_string(),
            serde_json::to_value(
                session
                    .query_current_minute_history_for(
                        AccountId(0),
                        &CurrentMinuteHistoryRequest { stock },
                    )
                    .unwrap(),
            )
            .unwrap(),
        );
        if session.civil_date() > retained_date {
            retained.insert(
                stock.0.to_string(),
                serde_json::to_value(
                    session
                        .query_market_history_for(
                            AccountId(0),
                            &MarketHistoryRequest {
                                stock,
                                date_from: retained_date,
                                date_to: through,
                                after: None,
                                page_size: 2,
                            },
                        )
                        .unwrap(),
                )
                .unwrap(),
            );
        }
    }
    let day_end = session.save().unwrap();
    json!({ "civilDate": session.civil_date().to_iso(), "snapshot": session.snapshot(),
        "runtimeSnapshot": session.runtime_snapshot(), "workingOrders": session.player_working_orders(), "currentMinute": minute, "retained": retained,
        "dayEndConfirmations": day_end.personal_trade_confirmations, "dayEndAccounts": day_end.snapshot.accounts })
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    assert_eq!(
        args.len(),
        3,
        "必须提供同源公共SaveSlot JSON、同字节script JSON和新的trace路径"
    );
    let fixture = std::fs::read(&args[0]).unwrap();
    let save = engine::decode_save_slot(&fixture, &SaveDecodeLimits::default()).unwrap();
    assert_eq!(save.setup.ticks_per_day, 2, "只接固定2拍代表fixture");
    let script: Script = serde_json::from_slice(&std::fs::read(&args[1]).unwrap()).unwrap();
    assert_ne!(script.closed, script.open);
    assert_eq!(
        script.steps.len(),
        6,
        "首mixed日和复市日各2真实市场拍与1次civil"
    );
    let mut session = ProtocolSession::restore(&save).unwrap();
    let initial = observe(&mut session, &script);
    let mut frames = Vec::new();
    for step in &script.steps {
        for intent in &step.intents {
            session
                .enqueue_player_intent(AccountId(0), intent.clone())
                .unwrap();
        }
        let update = if session.civil_day_ready().unwrap() {
            EngineUpdate::CivilUpdate(Box::new(session.end_civil_day_update().unwrap()))
        } else {
            let frame = session.step_frame().unwrap();
            EngineUpdate::TickBatch(Box::new(session.tick_batch(vec![frame]).unwrap()))
        };
        frames.push(json!({ "update": update, "view": observe(&mut session, &script) }));
    }
    let trace = json!({ "closed": script.closed, "open": script.open, "initial": initial, "frames": frames });
    let encoded = serde_json::to_vec_pretty(&trace).unwrap();
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])
        .unwrap();
    std::io::Write::write_all(&mut output, &encoded).unwrap();
    println!("真实native业务trace已保存，bytes={}", encoded.len());
}
