//! 资金边界金样：公司初始化不给任何投资者（NPC/玩家）付一分钱；默认 5 股票
//! 的交易规格（类别/交易所/总股本/流通盘）在初始化前后逐字段不变。
//!
//! 公司注册表在本任务中独立于 GameSession 构建（接线在任务 26）；因此测试
//! 分别构造两者，再对交易账户逐户对比 + 存档字节对比，锁死「公司初始化不
//! 触碰交易域任何账户状态」的资金边界。外部商业对手方承担全部开局资金流
//! （股本注入/借款对端是公司域对手方，不是证券 NPC）。

use super::*;
use engine::orderbook::AccountId;
use engine::session::{GameSession, NpcSetup, SessionSetup};
use engine::strategy::{HotParams, InstParams, RetailParams};
use engine::{FloatAllocation, GameConfig, StrategyParams};

/// 小规模会话：默认 5 股票 + 少量 NPC。资金边界是结构性性质（类型隔离 +
/// 独立构建），与账户规模无关；这里用 70 户保持测试轻量。
fn trading_session() -> GameSession {
    let stocks = default_stock_specs();
    let setup = SessionSetup {
        stocks,
        npcs: NpcSetup {
            retail_count: 64,
            inst_count: 3,
            hot_count: 2,
            retail_cash_median: Money::from_cents(20_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.3,
                order_size_mean: 300,
                chase_prob: 0.4,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 200_000,
            },
            hot: HotParams {
                lookback: 20,
                trend_threshold: 0.03,
                order_size: 100_000,
            },
        },
        ticks_per_day: 300,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: d("2030-01-01"),
    };
    GameSession::new(setup, 42).expect("compact default-stock session must be valid")
}

/// 逐户快照：账户 id、现金与全部持仓（代码/数量/T+1 锁定/投入/回收）。
type AccountStates = Vec<(u64, Money, Vec<(String, u32, u32, i64, i64)>)>;

fn account_states(session: &GameSession) -> AccountStates {
    (0..u64::try_from(session.account_count()).expect("account count fits u64"))
        .map(|ordinal| {
            let account = session
                .account(AccountId(ordinal))
                .expect("player id 0 plus sequential NPC ids are contiguous");
            (
                ordinal,
                account.cash,
                account
                    .positions
                    .iter()
                    .map(|(code, position)| {
                        (
                            code.0.clone(),
                            position.qty,
                            position.t1_locked,
                            position.invested_cents,
                            position.recovered_cents,
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

/// 公司初始化不向任何投资者支付：每个交易账户（NPC/玩家）现金与持仓在
/// 初始化前后逐字节一致；会话存档字节不变；默认 5 股票交易规格不变。
#[test]
fn initialization_does_not_pay_investors() {
    let session = trading_session();
    assert_eq!(session.account_count(), 1 + 64 + 3 + 2);

    let before_accounts = account_states(&session);
    let before_save = serde_json::to_vec(&session.save()).expect("save slot serializes");

    // 公司初始化：独立于会话构建默认注册表（5 上市工商 + 4 未上市测试实体）。
    let registry = default_registry();
    let stock_refs: Vec<(StockCode, u64)> = session
        .save()
        .setup
        .stocks
        .iter()
        .map(|stock| (stock.code.clone(), stock.total_shares))
        .collect();
    registry
        .validate_issuer_mapping(&stock_refs)
        .expect("default registry maps the session stocks exactly");

    // 资金边界：公司初始化后，交易域每个账户状态与存档字节完全不变。
    assert_eq!(
        account_states(&session),
        before_accounts,
        "company initialization must not change any trading account"
    );
    assert_eq!(
        serde_json::to_vec(&session.save()).expect("save slot serializes"),
        before_save,
        "company initialization must not change the trading save slot"
    );

    // 默认 5 股票交易规格不变（类别/交易所/总股本/流通盘逐字段钉住）。
    let stocks = &session.save().setup.stocks;
    assert_eq!(stocks.len(), 5);
    let pinned = [
        (
            "600101",
            StockExchange::Shanghai,
            SecurityCategory::MainBoard,
            8_928_571_429u64,
            3_571_428_571u32,
        ),
        (
            "002156",
            StockExchange::Shenzhen,
            SecurityCategory::MainBoard,
            2_925_045_704,
            2_047_531_993,
        ),
        (
            "300260",
            StockExchange::Shenzhen,
            SecurityCategory::ChiNext,
            815_217_391,
            611_413_043,
        ),
        (
            "600610",
            StockExchange::Shanghai,
            SecurityCategory::MainBoard,
            1_059_602_649,
            847_682_119,
        ),
        (
            "000812",
            StockExchange::Shenzhen,
            SecurityCategory::StMainBoard,
            1_052_631_579,
            842_105_263,
        ),
    ];
    for (index, (code, exchange, category, total_shares, float_shares)) in pinned.iter().enumerate()
    {
        let stock = &stocks[index];
        assert_eq!(stock.code.0, *code);
        assert_eq!(stock.exchange, *exchange);
        assert_eq!(stock.category, *category, "{code} category unchanged");
        assert_eq!(
            stock.total_shares, *total_shares,
            "{code} total shares unchanged"
        );
        assert_eq!(
            stock.float_shares, *float_shares,
            "{code} float shares unchanged"
        );
    }

    // 开局资金流由外部商业对手方吸收：600101 发行人的开局借款有贷款人对手方
    // （公司域 CounterpartyId，独立于交易域 AccountId 命名空间——编译期不可混用），
    // 公司现金在公司账套内而非任何交易账户。
    let issuer = registry
        .issuer_of(&StockCode("600101".to_string()))
        .expect("600101 issuer")
        .clone();
    let company = registry.get(&issuer).expect("issuer registered");
    let lenders: Vec<_> = company
        .counterparties()
        .iter()
        .filter(|(_, counterparty)| counterparty.kind == CounterpartyKind::Lender)
        .collect();
    assert!(!lenders.is_empty(), "opening debt has an external lender");
    // 公司经营现金在公司账套（600101 开局现金 60 亿元），不在交易账户。
    assert_eq!(
        company.books().ledger().cash_total().expect("cash"),
        yuan(6_000_000_000)
    );
    // 公司域无对手方授信兜底之外的隐藏资金源：开局后无收付流水（前史任务 14）。
    assert_eq!(company.counterparties().flow_count(), 0);
}
