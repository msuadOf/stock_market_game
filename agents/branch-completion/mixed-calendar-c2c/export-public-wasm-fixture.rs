//! 为 V11 生成公共 restore 合法的零市场拍 fixture；不改生产恢复门。

#[allow(dead_code)]
#[path = "../../../packages/engine/tests/common/mixed_calendar_c2c_fixture.rs"]
mod fixture;

use engine::calendar::{CalendarExchange, CalendarPolicy, TradingCalendar};
use engine::session::protocol::ProtocolSession;
use engine::{GameSession, StockExchange};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let destination = arguments.next().expect("必须显式提供输出 JSON 路径");
    assert!(arguments.next().is_none(), "不接受多余参数");
    let template = fixture::initial_save(CalendarExchange::Sse, StockExchange::Shanghai, false);
    let mut setup = template.setup.clone();
    setup.start_date = fixture::date("2030-01-01");
    let initial = GameSession::new(setup, 73).expect("Jan1 合法全闭市新局");
    let mut initial_save = initial.save().expect("初始零拍存档");
    // 仅在尚未运行市场时冻结同一 synthetic policy；随后不再替换 policy/source。
    initial_save.civil_clock.policy = template.civil_clock.policy;
    let calendar = TradingCalendar::from_policy(
        CalendarPolicy::from_parts(initial_save.civil_clock.policy.clone()).unwrap(),
    )
    .unwrap();
    for exchange in [CalendarExchange::Sse, CalendarExchange::Szse] {
        assert!(!calendar
            .is_trading_day(exchange, fixture::date("2030-01-01"))
            .unwrap());
    }
    let mut session =
        GameSession::restore(&initial_save).expect("冻结 policy 的 Jan1 初始真实 restore");
    let report = session.end_civil_day().expect("全闭市 Jan1 真实自然日日结");
    assert_eq!(report.settled_date, fixture::date("2030-01-01"));
    assert_eq!(session.civil_date(), fixture::date("2030-01-02"));
    assert_eq!((session.tick(), session.day()), (0, 0));
    let public_save = session.save().expect("完整自然日日结后的公共 SaveSlot");
    let protocol =
        ProtocolSession::restore(&public_save).expect("公共 restore 必须真实接受该 fixture");
    assert_eq!((protocol.tick(), protocol.day()), (0, 0));
    assert_eq!(protocol.civil_date(), fixture::date("2030-01-02"));
    let encoded = serde_json::to_vec_pretty(&public_save).expect("合法 fixture 必须可序列化");
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .expect("输出必须是尚不存在的独占文件，禁止覆盖旧证据");
    std::io::Write::write_all(&mut output, &encoded).expect("fixture 写入失败必须显式报告");
    println!(
        "公共 fixture 已通过零市场拍 Jan1 civil commit 和 ProtocolSession::restore，字节数={}",
        encoded.len()
    );
}
