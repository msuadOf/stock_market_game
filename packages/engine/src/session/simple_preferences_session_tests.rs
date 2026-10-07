//! 公司行为偏好自动提案的 Session 级测试（ADR-0037，P 批）。
//!
//! 覆盖：结算周期完成后按偏好自动产生现金分红/送转方案并走与显式 API 完全
//! 相同的 approve/announce/登记/入账状态机（含全链路到账/入账）、偏好提案被
//! 制度拒绝时如实记录且不重试风暴、同周期恢复重放幂等、未配置不产生方案、
//! 严格持久化（新字段缺失的旧档显式拒绝）与恢复后不重复产生历史方案。

use super::*;
use crate::account::Position;
use crate::company::config::CompanySystemConfig;
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::company::simple::preferences::{
    SimpleCashDividendPreference, SimpleCompanyPreferences, SimplePreferenceProposalKind,
    SimpleStockDistributionPreference,
};
use crate::company::{CompanyId, DividendDeclaration};
use crate::money::Money;

fn d(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn preference_setup(preferences: SimpleCompanyPreferences) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.start_date = d("2030-01-20");
    // 缩短每个交易日的 tick 数：本组只验证跨自然日的公司行为链路，
    // 无需完整 100 tick 行情日（单 case 10 秒纪律）。
    setup.ticks_per_day = 2;
    if let CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.companies[0].preferences = preferences;
    }
    setup
}

fn cash_preference(payout_bp: u16, min_cents: i64, cycles: u16) -> SimpleCompanyPreferences {
    SimpleCompanyPreferences {
        cash_dividend: Some(SimpleCashDividendPreference {
            target_payout_bp: payout_bp,
            min_distributable_profit: Money::from_cents(min_cents),
            cycles_between_proposals: cycles,
        }),
        stock_distribution: None,
    }
}

fn stock_preference(
    min_cents: i64,
    ratio_micros: u64,
    max_expansion_micros: u64,
    cycles: u16,
) -> SimpleCompanyPreferences {
    SimpleCompanyPreferences {
        cash_dividend: None,
        stock_distribution: Some(SimpleStockDistributionPreference {
            min_distributable_profit: Money::from_cents(min_cents),
            shares_per_existing_share_micros: ratio_micros,
            max_cumulative_expansion_micros: max_expansion_micros,
            cycles_between_proposals: cycles,
        }),
    }
}

/// 装配带完整股东名册的会话：玩家持有 `player_qty` 股、具名外部股东 1 股、
/// 其余为库藏股。注册资本法定事实按 `registered_capital_cents` 显式绑定。
fn session_with_registry(
    preferences: SimpleCompanyPreferences,
    player_qty: u64,
    registered_capital_cents: i128,
) -> (GameSession, CompanyId, StockCode) {
    session_with_registry_setup(
        preference_setup(preferences),
        player_qty,
        registered_capital_cents,
    )
}

/// [`session_with_registry`] 的可定制 setup 变体：测试按需覆盖初始价等
/// `SessionSetup` 字段后传入（其余装配与名册/法定事实注入完全一致）。
fn session_with_registry_setup(
    setup: SessionSetup,
    player_qty: u64,
    registered_capital_cents: i128,
) -> (GameSession, CompanyId, StockCode) {
    let mut session = GameSession::new(setup, 42).unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let total_shares = session.state.setup.stocks[0].total_shares;
    session
        .define_dividend_legal_facts(
            &issuer,
            crate::accounting::AccountingAmount::from_cents(registered_capital_cents),
            "simple-preference fixture legal fact".into(),
        )
        .unwrap();
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            stock.clone(),
            Position::from_restored_parts(player_qty.try_into().unwrap(), 0, 1_000, 0),
        );
    let lot = |id: &str, qty: u64| ShareLot {
        id: id.into(),
        qty,
        acquired_on: session.civil_date(),
        source: AcquisitionSource::InitialAllocation {
            evidence: "simple-preference fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    };
    let registry = ShareRegistry::new(
        stock.clone(),
        issuer.clone(),
        total_shares,
        session.civil_date(),
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(0)),
                lots: vec![lot("player-lot", player_qty)],
            },
            ShareHolding {
                holder: HolderId::External("external-holder".into()),
                lots: vec![lot("external-lot", 1)],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![lot("treasury-lot", total_shares - player_qty - 1)],
            },
        ],
    )
    .unwrap();
    session.configure_share_registry(registry).unwrap();
    (session, issuer, stock)
}

/// 完成一个自然日：交易日步进当日全部 tick 后日结；休市日直接日结
/// （全部交易所休市的自然日不能推进市场 tick）。
fn run_day(session: &mut GameSession) {
    if session.state.civil_clock.phase() == CivilPhase::IntradayTrading {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
    }
    session.end_civil_day().unwrap();
}

/// 推进到 `target`（不含）为止：每个自然日完整步进并日结。
fn advance_to(session: &mut GameSession, target: CivilDate) {
    while session.civil_date() < target {
        run_day(session);
    }
}

/// 从 2030-01-20 起推进到首个结算周期（月度）末日 2030-01-31 完成日结之后。
fn advance_through_first_period_end(session: &mut GameSession) {
    advance_to(session, d("2030-02-01"));
}

fn rejections(
    session: &GameSession,
    issuer: &CompanyId,
) -> Vec<crate::company::simple::preferences::SimplePreferenceRejection> {
    session
        .state
        .company_system
        .preference_rejections(issuer)
        .unwrap()
        .to_vec()
}

#[test]
fn cash_preference_auto_proposes_after_settlement_and_pays_full_chain() {
    let (mut session, issuer, stock) =
        session_with_registry(cash_preference(3_000, 1, 1), 6_000_000, 10_000_000);
    let player_cash_before = session.account(AccountId(0)).unwrap().cash();
    advance_through_first_period_end(&mut session);
    // 周期末日 2030-01-31 结算完成后自动产生方案：确定性 id、批准=公告=次日。
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    assert_eq!(
        session.state.corporate_actions.dividends.len(),
        1,
        "应恰好产生一个自动分红方案"
    );
    assert_eq!(
        plan.plan_id,
        "simple-preference:C-600888:dividend:2030-01-31"
    );
    assert_eq!(plan.approved_on, d("2030-02-01"));
    assert_eq!(plan.announced_on, d("2030-02-01"));
    assert!(
        plan.gross_per_share.cents() > 0,
        "派息额应为正：可分配利润充足且非库藏股基数庞大"
    );
    assert!(rejections(&session, &issuer).is_empty());
    // 与显式方案同一状态机：Simple 批准事实同步存在。
    let facts = session
        .state
        .company_system
        .dividend_plan_facts(&issuer)
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].plan_id, plan.plan_id);
    // 推进过公告、登记、除息与派发：公告走既有 typed 通道，玩家真实税前到账。
    advance_to(&mut session, plan.payable_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
    assert_eq!(
        session.account(AccountId(0)).unwrap().cash(),
        player_cash_before
            .add(Money::from_cents(plan.gross_per_share.cents() * 6_000_000))
            .unwrap(),
        "玩家按登记持股 6,000,000 股真实税前到账（Exempt 模式不扣税）"
    );
    let receipts: Vec<_> = session
        .state
        .corporate_actions
        .account_gross_receipts
        .iter()
        .filter(|receipt| receipt.account == AccountId(0) && receipt.plan_id == plan.plan_id)
        .collect();
    assert_eq!(receipts.len(), 1, "玩家到账凭证恰好一条");
    assert_eq!(
        receipts[0].gross,
        Money::from_cents(plan.gross_per_share.cents() * 6_000_000)
    );
    assert_eq!(stock.0, "600888");
    let instant = crate::calendar::CivilInstant::new(plan.announced_on, 64_800)
        .expect("18:00:00 是合法自然日瞬间");
    let announcements = session
        .state
        .library
        .announcements_for_company(&issuer, instant);
    assert!(
        announcements.iter().any(|announcement| matches!(
            &announcement.content,
            crate::information::AnnouncementContent::CashDividend(typed)
                if typed.plan.plan_id == plan.plan_id
        )),
        "偏好自动方案必须走既有 typed 现金分红公告通道"
    );
}

#[test]
fn cash_preference_records_rejection_when_payout_dilutes_below_one_cent() {
    // 派息 1bp 摊到近全部流通股（约 10^7 股）：目标派息额每股不足一分，
    // 构造失败被如实记录为拒绝，日结不失败，且同一结算周期不重复记录。
    let (mut session, issuer, _stock) =
        session_with_registry(cash_preference(1, 1, 1), 9_999_998, 10_000_000);
    advance_through_first_period_end(&mut session);
    assert!(
        session.state.corporate_actions.dividends.is_empty(),
        "无法成案时不得产生分红账簿"
    );
    let rows = rejections(&session, &issuer);
    assert_eq!(rows.len(), 1, "同周期恰好一条拒绝记录");
    assert_eq!(rows[0].company, issuer);
    assert_eq!(rows[0].evaluated_on, d("2030-01-31"));
    assert_eq!(rows[0].kind, SimplePreferenceProposalKind::CashDividend);
    assert!(
        rows[0].detail.contains("每股不足一分"),
        "拒绝原因应如实保留：{}",
        rows[0].detail
    );
    // 恢复重放同一日结：不新增记录。
    let save = session.save().unwrap();
    let mut replayed = GameSession::restore(&save).unwrap();
    advance_to(&mut replayed, d("2030-02-02"));
    let rows = rejections(&replayed, &issuer);
    assert_eq!(rows.len(), 1, "下一周期（02-28）前不得重复评估记录");
}

#[test]
fn stock_preference_rejection_is_recorded_when_par_value_exceeds_profit() {
    // 注册资本法定事实取 10^17 分（面值 10^10 分/股）：送股 6 股面值总额远超
    // 可分配利润，制度校验拒绝提案并如实记录，日结照常成功。
    let (mut session, issuer, _stock) = session_with_registry(
        stock_preference(1, 1_000_000, 1_000_000_000, 1),
        5,
        100_000_000_000_000_000,
    );
    advance_through_first_period_end(&mut session);
    assert!(
        session
            .state
            .corporate_actions
            .stock_distributions
            .is_empty(),
        "送转被拒时不得产生送转账簿"
    );
    let rows = rejections(&session, &issuer);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].kind,
        SimplePreferenceProposalKind::StockDistribution
    );
    assert!(
        rows[0].detail.contains("超过可分配利润"),
        "拒绝原因应指向可分配利润制度校验：{}",
        rows[0].detail
    );
}

#[test]
fn stock_preference_auto_proposes_and_credits_shares_full_chain() {
    // 10 送 10：玩家 5 股 + 外部 1 股 = 6 股基数，获批新增 6 股；复用 B1 的
    // R 日冻结分配与 R+1 非交易过户入账、发行股数守恒与注册资本演进语义。
    let (mut session, issuer, stock) = session_with_registry(
        stock_preference(1, 1_000_000, 1_000_000_000, 1),
        5,
        10_000_000,
    );
    advance_through_first_period_end(&mut session);
    assert_eq!(
        session.state.corporate_actions.stock_distributions.len(),
        1,
        "应恰好产生一个自动送转方案"
    );
    let plan = session.state.corporate_actions.stock_distributions[0]
        .plan()
        .clone();
    assert_eq!(
        plan.event_id,
        "simple-preference:C-600888:stock-distribution:2030-01-31"
    );
    assert_eq!(plan.approved_total_new_shares, 6);
    assert_eq!(
        plan.kind,
        crate::company::stock_distribution::StockDistributionKind::BonusShares
    );
    advance_to(&mut session, plan.ex_rights_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.stock_distributions[0].status(),
        &crate::company::stock_distribution::StockDistributionStatus::Credited
    );
    assert_eq!(
        u64::from(
            session
                .account(AccountId(0))
                .unwrap()
                .position(&stock)
                .unwrap()
                .qty()
        ),
        10_u64,
        "玩家 5 股按 1:1 送转后应为 10 股"
    );
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        10_000_000 + 6,
        "发行股数守恒演进"
    );
    let legal = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap();
    assert_eq!(
        legal.registered_capital,
        crate::accounting::AccountingAmount::from_cents(10_000_000 + 6),
        "注册资本按面值×新增股数演进"
    );
}

#[test]
fn same_period_replay_after_restore_produces_single_proposal() {
    // 同日幂等：日结前保存、恢复、完成周期末日结——两次独立重放各产生恰好
    // 一个同 id 方案。市场轨迹允许因并发受理调度合法不同（ADR-0017 修订），
    // 因此深等断言只覆盖确定性的公司系统子树（偏好配置、台账与批准事实）。
    let mut first = session_with_registry(cash_preference(3_000, 1, 1), 6_000_000, 10_000_000).0;
    advance_to(&mut first, d("2030-01-31"));
    let save = first.save().unwrap();
    let mut run_one = GameSession::restore(&save).unwrap();
    run_day(&mut run_one);
    assert_eq!(run_one.state.corporate_actions.dividends.len(), 1);
    let encoded_one = serde_json::to_value(run_one.save().unwrap()).unwrap();

    let mut run_two = GameSession::restore(&save).unwrap();
    run_day(&mut run_two);
    assert_eq!(run_two.state.corporate_actions.dividends.len(), 1);
    assert_eq!(
        run_two.state.corporate_actions.dividends[0].plan().plan_id,
        run_one.state.corporate_actions.dividends[0].plan().plan_id,
        "同周期重放必须复用确定性方案 id"
    );
    assert_eq!(
        run_two.state.corporate_actions.dividends[0]
            .plan()
            .gross_per_share,
        run_one.state.corporate_actions.dividends[0]
            .plan()
            .gross_per_share
    );
    let encoded_two = serde_json::to_value(run_two.save().unwrap()).unwrap();
    assert_eq!(
        encoded_one["company_system"], encoded_two["company_system"],
        "同一保存点的两次日结重放必须产生深度相等的公司系统状态"
    );
}

#[test]
fn unconfigured_preferences_produce_no_proposals_or_rejections() {
    let (mut session, issuer, _stock) =
        session_with_registry(SimpleCompanyPreferences::none(), 5, 10_000_000);
    advance_to(&mut session, d("2030-03-01"));
    assert!(
        session.state.corporate_actions.dividends.is_empty(),
        "未配置偏好不得自动产生现金分红方案"
    );
    assert!(
        session
            .state
            .corporate_actions
            .stock_distributions
            .is_empty(),
        "未配置偏好不得自动产生送转方案"
    );
    assert!(
        rejections(&session, &issuer).is_empty(),
        "未配置偏好不评估、不产生拒绝记录"
    );
}

#[test]
fn restore_preserves_ledger_and_does_not_duplicate_historical_proposals() {
    let (mut session, issuer, _stock) =
        session_with_registry(cash_preference(3_000, 1, 1), 6_000_000, 10_000_000);
    advance_through_first_period_end(&mut session);
    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    // 恢复深等：再次保存与原保存深度相等（偏好配置与台账进入存档契约）。
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(&save).unwrap()
    );
    // 恢复后不重复产生历史方案；下一周期产生新的确定性方案。
    advance_to(&mut restored, d("2030-03-01"));
    let books = &restored.state.corporate_actions.dividends;
    assert_eq!(books.len(), 2, "两个结算周期各一个方案");
    assert_eq!(
        books[0].plan().plan_id,
        "simple-preference:C-600888:dividend:2030-01-31"
    );
    assert_eq!(
        books[1].plan().plan_id,
        "simple-preference:C-600888:dividend:2030-02-28"
    );
    assert!(rejections(&restored, &issuer).is_empty());
}

#[test]
fn old_save_without_preferences_fields_is_explicitly_rejected() {
    let (session, _issuer, _stock) =
        session_with_registry(cash_preference(3_000, 1, 1), 6_000_000, 10_000_000);
    let encoded = serde_json::to_value(session.save().unwrap()).unwrap();

    // 1) setup.company_system.config.companies[0] 缺 preferences。
    let mut legacy = encoded.clone();
    legacy["setup"]["company_system"]["config"]["companies"][0]
        .as_object_mut()
        .unwrap()
        .remove("preferences");
    assert!(
        serde_json::from_value::<SaveSlot>(legacy).is_err(),
        "旧档 setup 缺 preferences 字段必须显式拒绝"
    );

    // 2) company_system 状态内 config.companies[0] 缺 preferences。
    let mut legacy = encoded.clone();
    legacy["company_system"]["implementation"]["state"]["config"]["companies"][0]
        .as_object_mut()
        .unwrap()
        .remove("preferences");
    assert!(
        serde_json::from_value::<SaveSlot>(legacy).is_err(),
        "旧档公司系统状态缺 preferences 字段必须显式拒绝"
    );

    // 3) 公司状态缺 preference_ledger。
    let mut legacy = encoded.clone();
    let company_key = legacy["company_system"]["implementation"]["state"]["companies"]
        .as_object_mut()
        .unwrap()
        .keys()
        .next()
        .cloned()
        .unwrap();
    legacy["company_system"]["implementation"]["state"]["companies"][&company_key]
        .as_object_mut()
        .unwrap()
        .remove("preference_ledger");
    assert!(
        serde_json::from_value::<SaveSlot>(legacy).is_err(),
        "旧档公司状态缺 preference_ledger 字段必须显式拒绝"
    );

    // 对照：完整档可解析（正例）。
    assert!(serde_json::from_value::<SaveSlot>(encoded).is_ok());
}

/// 测试侧独立推导：公告日（=批准日）后的（登记日, 除息/除权日）。
/// 独立于评估函数实现（直接逐步查日历），避免与实现共用同一推导而循环自证。
fn schedule_after_announcement(
    calendar: &crate::calendar::TradingCalendar,
    exchange: crate::calendar::CalendarExchange,
    announced_on: CivilDate,
) -> (CivilDate, CivilDate) {
    let registered = calendar.next_trading_day(exchange, announced_on).unwrap();
    let ex = calendar.next_trading_day(exchange, registered).unwrap();
    (registered, ex)
}

/// 测试侧独立计数：(from_exclusive, to_inclusive] 区间内的交易所交易日数，
/// 即提案锚收盘后到登记日收盘之间各自最多再跌一个跌停幅度的敞露日数。
fn exposed_trading_days(
    calendar: &crate::calendar::TradingCalendar,
    exchange: crate::calendar::CalendarExchange,
    from_exclusive: CivilDate,
    to_inclusive: CivilDate,
) -> u32 {
    let mut count = 0_u32;
    let mut cursor = from_exclusive;
    while cursor < to_inclusive {
        cursor = cursor.next().unwrap();
        if calendar.is_trading_day(exchange, cursor).unwrap() {
            count += 1;
        }
    }
    count
}

/// 测试侧独立复刻：单日跌停价的整数推导（与实现不共用代码，依据已登记的
/// 交易规则语义重写）——昨收 × (1−p) 按正数四舍五入取至最小价位；**跌停
/// 幅度不足一个价位时（舍入结果 ≥ 昨收）强制至少下移一个价位**；最低不
/// 低于一个价位（tick = 1 分）。
fn independent_down_stop_cents(reference_cents: i64, limit_bps: u32) -> i64 {
    let numerator = i128::from(reference_cents) * i128::from(10_000_u32 - limit_bps);
    let rounded = i64::try_from((numerator + 5_000) / 10_000).unwrap();
    let bound = rounded.max(1);
    if bound >= reference_cents {
        (reference_cents - 1).max(1)
    } else {
        bound
    }
}

/// 测试侧独立复刻：从锚收盘连续 `days` 个交易日一字跌停后的最小可能收盘
/// （封顶的权威口径；期望值一律由本函数推导，不与实现共用代码）。
fn independent_limit_down_chain_floor_cents(anchor_cents: i64, limit_bps: u32, days: u32) -> i64 {
    let mut current = anchor_cents;
    for _ in 0..days {
        current = independent_down_stop_cents(current, limit_bps);
    }
    current
}

/// 在当日盘中制造一笔按 `price` 的真实成交：机构账户挂一笔直接置入订单簿的
/// 限价买单（复用既有税务用例的 fixture 手法，冻结 NPC 注意力避免策略干扰），
/// 玩家卖出 `qty` 股，步进**一个** tick 并断言真实成交发生。当日剩余 tick 由
/// 调用方步进（保持每交易日恰好 `ticks_per_day` 次 step）。
fn force_trade_at_price(session: &mut GameSession, stock: &StockCode, price: Money, qty: u32) {
    let institution = crate::orderbook::AccountId(1);
    session
        .state
        .npc_attention
        .get_mut(&institution)
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    session.state.attention_scheduler = [(u64::MAX, institution)].into_iter().collect();
    let buyer_order_id = session.state.next_order_id;
    session
        .state
        .markets
        .get_mut(stock)
        .unwrap()
        .place(crate::Order {
            id: crate::OrderId(buyer_order_id),
            side: crate::Side::Buy,
            price,
            qty,
            original_qty: qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: institution,
            seq: buyer_order_id,
        })
        .unwrap();
    session.state.next_order_id += 1;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    session
        .enqueue_player_intent(
            crate::orderbook::AccountId(0),
            crate::Intent::PlaceLimit {
                code: stock.clone(),
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(price),
                qty,
            },
        )
        .unwrap();
    let events = session.step().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::Event::Trade { .. })),
        "fixture 必须在 {price:?} 制造一笔真实成交"
    );
    assert_eq!(
        session.state.markets.get(stock).unwrap().last_price(),
        price,
        "成交价必须更新最新价"
    );
}

#[test]
fn auto_proposal_at_cap_survives_consecutive_limit_downs_to_ex_date() {
    // 门禁复核 major 边界用例：提案按「日程跌停敞口余量」封顶生成后，
    // 公告→登记之间每个敞露交易日连续一字跌停，走到除息日时除息参考价仍为正、
    // 日结（含派发）正常、会话不 poison。旧封顶口径「收盘价 − 1 分」在本场景
    // 以 NonPositiveReferencePrice 使除息日首 tick 致命失败（红测证据见台账）。
    let (mut session, issuer, stock) =
        session_with_registry(cash_preference(10_000, 1, 1), 205, 10_000_000);
    let exchange = crate::calendar::CalendarExchange::Sse;
    let calendar = session.state.civil_clock.calendar().clone();
    // 冻结唯一 NPC（机构）注意力：01-20 → 01-31 无任何成交，提案锚 = 初始价。
    let institution = crate::orderbook::AccountId(1);
    session
        .state
        .npc_attention
        .get_mut(&institution)
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    session.state.attention_scheduler = [(u64::MAX, institution)].into_iter().collect();
    advance_through_first_period_end(&mut session);
    assert_eq!(session.state.corporate_actions.dividends.len(), 1);
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    assert_eq!(plan.approved_on, d("2030-02-01"));
    // 敞露交易日数 n 由日程推导（本日程：公告日 2030-02-01 为交易日 + 登记日）。
    let n = exposed_trading_days(&calendar, exchange, d("2030-01-31"), plan.registered_on);
    assert_eq!(n, 2, "公告→登记间应有 2 个敞露交易日");
    // 封顶 = 迭代真实跌停链 n 步的最小可能收盘 − 1 分（10% 板：1000→900→810，
    // 封顶 = 809）；期望值由测试侧独立复刻链推导，不与实现共用代码。
    let expected_cap = i128::from(independent_limit_down_chain_floor_cents(1_000, 1_000, n)) - 1;
    assert_eq!(
        i128::from(plan.gross_per_share.cents()),
        expected_cap,
        "目标派息（30 万bp 远超余量）应被日程跌停余量封顶"
    );
    assert!(rejections(&session, &issuer).is_empty());
    // 公告→登记间逐个敞露交易日以真实成交打到跌停价（一字跌停）。
    // 卖出数量遵守现行申报数量规则：首日两整手 200 股，末日余额零股 5 股
    // 一次性卖出（沪市《交易规则（2026 年修订）》3.3.8）。
    let mut remaining_sells = [200_u32, 5_u32].into_iter();
    while session.civil_date() < plan.ex_dividend_on {
        let today = session.civil_date();
        let exposed = (today == plan.approved_on
            && calendar.is_trading_day(exchange, today).unwrap())
            || today == plan.registered_on;
        let mut stepped = 0_u64;
        let mut exposed_stop = None;
        if exposed && session.state.civil_clock.phase() == CivilPhase::IntradayTrading {
            let down_stop = session
                .state
                .markets
                .get(&stock)
                .unwrap()
                .down_stop()
                .unwrap();
            let qty = remaining_sells.next().expect("敞露交易日数与卖出批次一致");
            force_trade_at_price(&mut session, &stock, down_stop, qty);
            exposed_stop = Some(down_stop);
            stepped = 1;
        }
        if session.state.civil_clock.phase() == CivilPhase::IntradayTrading {
            for _ in stepped..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
        if let Some(stop) = exposed_stop {
            // 日终把昨收对齐到当日最新成交：敞露日收盘恰好为当日跌停价。
            assert_eq!(
                session.state.markets.get(&stock).unwrap().last_close(),
                stop,
                "敞露交易日收盘应恰好为当日跌停价"
            );
        }
    }
    assert_eq!(session.civil_date(), plan.ex_dividend_on);
    // 除息日首 tick 安装除息参考价：登记日收盘（两日连续一字跌停恰为迭代链
    // 下界 810 分）− 每股红利 809 分 = 1 分，仍为正；迭代封顶与真实链一致，
    // 最坏参考价恰好压到 1 分。旧口径（999 分）在此 poison。
    let first_step = session.step();
    assert!(
        first_step.is_ok(),
        "除息日首 tick 不得致命失败：{:?}",
        first_step.err()
    );
    for _ in 1..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    assert!(
        session.poison_reason().is_none(),
        "除息日日结不得 poison：{:?}",
        session.poison_reason()
    );
    let reference = session
        .state
        .markets
        .get(&stock)
        .unwrap()
        .last_cash_ex_reference()
        .expect("除息日必须安装除息参考价");
    assert_eq!(reference.ex_date, plan.ex_dividend_on);
    assert_eq!(reference.reference_price, Money::from_cents(1));
    // 除息日 = 派发日：日结完成真实派发，状态机照常推进。
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
}

#[test]
fn auto_proposal_survives_low_price_double_limit_downs_where_linear_bound_was_falsified() {
    // 门禁复核修复轮 2 边界用例（低价域证伪反例本体）：4 分锚 @10% 板时
    // 锚×limit_bps = 4000 ≤ 5000，每日跌停幅度不足一个价位，跌停价守卫强制
    // 整价位下移（4→3→2）。旧闭式线性下界 floor(4×0.8) = 3 会给出封顶 3−1 = 2，
    // 两日连续一字跌停后除息参考价 2 − 2 = 0 → poison。迭代引擎真实跌停链的
    // 封顶 = 2 − 1 = 1，最坏参考价恰为 1 分，日结照常、无 poison。
    let mut setup = preference_setup(cash_preference(10_000, 1, 1));
    setup.stocks[0].initial_price = Money::from_cents(4);
    let (mut session, issuer, stock) = session_with_registry_setup(setup, 205, 10_000_000);
    let exchange = crate::calendar::CalendarExchange::Sse;
    let calendar = session.state.civil_clock.calendar().clone();
    // 冻结唯一 NPC（机构）注意力：01-20 → 01-31 无任何成交，提案锚 = 初始价 4 分。
    let institution = crate::orderbook::AccountId(1);
    session
        .state
        .npc_attention
        .get_mut(&institution)
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    session.state.attention_scheduler = [(u64::MAX, institution)].into_iter().collect();
    advance_through_first_period_end(&mut session);
    assert_eq!(session.state.corporate_actions.dividends.len(), 1);
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    assert_eq!(plan.approved_on, d("2030-02-01"));
    let n = exposed_trading_days(&calendar, exchange, d("2030-01-31"), plan.registered_on);
    assert_eq!(n, 2, "公告→登记间应有 2 个敞露交易日");
    // 封顶 = 迭代真实跌停链（4→3→2）− 必留 1 分 = 1 分；旧闭式口径会给出 2 分
    // 并使最坏参考价为 0。期望值由测试侧独立复刻链推导。
    let expected_cap = i128::from(independent_limit_down_chain_floor_cents(4, 1_000, n)) - 1;
    assert_eq!(expected_cap, 1, "4 分锚两日一字跌停链应为 4→3→2");
    assert_eq!(
        i128::from(plan.gross_per_share.cents()),
        expected_cap,
        "目标派息应被迭代跌停链余量封顶到 1 分"
    );
    assert!(rejections(&session, &issuer).is_empty());
    // 公告→登记间逐个敞露交易日以真实成交打到跌停价（一字跌停）：首日两整手
    // 200 股、登记日余额零股 5 股一次性卖出（沪市《交易规则（2026 年修订）》3.3.8）。
    let mut remaining_sells = [200_u32, 5_u32].into_iter();
    while session.civil_date() < plan.ex_dividend_on {
        let today = session.civil_date();
        let exposed = (today == plan.approved_on
            && calendar.is_trading_day(exchange, today).unwrap())
            || today == plan.registered_on;
        let mut stepped = 0_u64;
        let mut exposed_stop = None;
        if exposed && session.state.civil_clock.phase() == CivilPhase::IntradayTrading {
            let down_stop = session
                .state
                .markets
                .get(&stock)
                .unwrap()
                .down_stop()
                .unwrap();
            let qty = remaining_sells.next().expect("敞露交易日数与卖出批次一致");
            force_trade_at_price(&mut session, &stock, down_stop, qty);
            exposed_stop = Some(down_stop);
            stepped = 1;
        }
        if session.state.civil_clock.phase() == CivilPhase::IntradayTrading {
            for _ in stepped..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
        if let Some(stop) = exposed_stop {
            assert_eq!(
                session.state.markets.get(&stock).unwrap().last_close(),
                stop,
                "敞露交易日收盘应恰好为当日跌停价（守卫强制整价位下移）"
            );
        }
    }
    assert_eq!(session.civil_date(), plan.ex_dividend_on);
    // 登记日收盘 = 迭代链下界 2 分；除息日首 tick 安装参考价 2 − 1 = 1 分 ≥ 1 分。
    assert_eq!(
        session.state.markets.get(&stock).unwrap().last_close(),
        Money::from_cents(2),
        "两日一字跌停后登记日收盘应恰为迭代链下界 2 分"
    );
    let first_step = session.step();
    assert!(
        first_step.is_ok(),
        "除息日首 tick 不得致命失败：{:?}",
        first_step.err()
    );
    for _ in 1..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    assert!(
        session.poison_reason().is_none(),
        "低价域两日一字跌停走到除息日不得 poison：{:?}",
        session.poison_reason()
    );
    let reference = session
        .state
        .markets
        .get(&stock)
        .unwrap()
        .last_cash_ex_reference()
        .expect("除息日必须安装除息参考价");
    assert_eq!(reference.ex_date, plan.ex_dividend_on);
    assert!(
        reference.reference_price >= Money::from_cents(1),
        "最坏除息参考价必须 ≥1 分，实际 {:?}",
        reference.reference_price
    );
    assert_eq!(reference.reference_price, Money::from_cents(1));
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
}

#[test]
fn auto_proposal_rejected_when_same_ex_date_explicit_plan_exhausts_margin() {
    // 门禁复核 major 边界用例：既有显式方案与自动提案同除息日且已占满跌停
    // 余量封顶时，新自动提案被拒并如实记入台账（同除息日 gross 合并口径）。
    let (mut session, issuer, stock) =
        session_with_registry(cash_preference(10_000, 1, 1), 5, 10_000_000);
    let exchange = crate::calendar::CalendarExchange::Sse;
    let calendar = session.state.civil_clock.calendar().clone();
    // 显式方案须在自动提案的**上一个**结算周期批准（同月显式批准会以频率
    // 归并口径抑制本周期自动评估），除息日程与第二周期自动提案重合。
    advance_to(&mut session, d("2030-01-21"));
    let (registered_on, ex_on) = schedule_after_announcement(&calendar, exchange, d("2030-03-01"));
    // 显式方案每股 809 分：恰好占满迭代跌停链封顶（1000→900→810 再留 1 分）。
    let explicit_gross_cents = independent_limit_down_chain_floor_cents(1_000, 1_000, 2) - 1;
    assert_eq!(explicit_gross_cents, 809);
    let explicit_plan = crate::company::cash_dividend::CashDividendPlan::new(
        "explicit-same-ex-date".into(),
        issuer.clone(),
        stock.clone(),
        exchange,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        d("2030-01-21"),
        d("2030-03-01"),
        registered_on,
        ex_on,
        ex_on,
        Money::from_cents(explicit_gross_cents),
        Money::from_cents(10_000_000),
        &calendar,
    )
    .unwrap();
    session
        .approve_cash_dividend(
            DividendDeclaration {
                plan_id: explicit_plan.plan_id.clone(),
                approved_on: d("2030-01-21"),
                total_gross: crate::accounting::AccountingAmount::from_cents(
                    i128::from(explicit_gross_cents) * 6,
                ),
                registered_capital: crate::accounting::AccountingAmount::from_cents(10_000_000),
            },
            explicit_plan,
        )
        .unwrap();
    // 推进过第二个月度周期末日（2030-02-28）日结：自动评估在 03-01 提案。
    advance_to(&mut session, d("2030-03-01"));
    assert_eq!(session.civil_date(), d("2030-03-01"));
    // 一月内的显式批准同时以频率归并口径抑制第一周期自动评估，第二周期
    //（02-28）到期评估被合并封顶拒绝：全程只保留显式方案一本账簿。
    assert_eq!(
        session.state.corporate_actions.dividends.len(),
        1,
        "余量被同除息日显式方案占满时不得再产生自动方案"
    );
    assert!(
        !session
            .state
            .corporate_actions
            .dividends
            .iter()
            .any(|book| book.plan().plan_id.starts_with("simple-preference:")),
        "被拒周期不得留下自动方案"
    );
    let rows = rejections(&session, &issuer);
    assert_eq!(rows.len(), 1, "同周期恰好一条拒绝记录");
    assert_eq!(rows[0].kind, SimplePreferenceProposalKind::CashDividend);
    assert!(
        rows[0].detail.contains("封顶"),
        "拒绝原因应指向合并封顶：{}",
        rows[0].detail
    );
    // 拒绝不令日结失败；显式方案自身满足封顶口径，日程照常走完不 poison。
    advance_to(&mut session, ex_on.next().unwrap());
    assert!(session.poison_reason().is_none());
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
}

#[test]
fn auto_proposal_merges_same_ex_date_explicit_gross_into_cap() {
    // 同除息日合并口径的正例：显式方案每股 500 分后，自动提案每股被压到
    // 迭代跌停链封顶 809 − 500 = 309 分；合并 gross 恰好回到封顶。
    let (mut session, issuer, stock) =
        session_with_registry(cash_preference(10_000, 1, 1), 5, 10_000_000);
    let exchange = crate::calendar::CalendarExchange::Sse;
    let calendar = session.state.civil_clock.calendar().clone();
    // 同测试 B：显式方案上一周期批准，除息日程与第二周期自动提案重合。
    advance_to(&mut session, d("2030-01-21"));
    let (registered_on, ex_on) = schedule_after_announcement(&calendar, exchange, d("2030-03-01"));
    let explicit_plan = crate::company::cash_dividend::CashDividendPlan::new(
        "explicit-partial-margin".into(),
        issuer.clone(),
        stock.clone(),
        exchange,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        d("2030-01-21"),
        d("2030-03-01"),
        registered_on,
        ex_on,
        ex_on,
        Money::from_cents(500),
        Money::from_cents(10_000_000),
        &calendar,
    )
    .unwrap();
    session
        .approve_cash_dividend(
            DividendDeclaration {
                plan_id: explicit_plan.plan_id.clone(),
                approved_on: d("2030-01-21"),
                total_gross: crate::accounting::AccountingAmount::from_cents(500 * 6),
                registered_capital: crate::accounting::AccountingAmount::from_cents(10_000_000),
            },
            explicit_plan,
        )
        .unwrap();
    advance_to(&mut session, d("2030-03-01"));
    // 一月内的显式批准抑制第一周期自动评估；第二周期（02-28）自动提案被
    // 压到 309 分并批准：显式 + 自动共两本。
    assert_eq!(session.state.corporate_actions.dividends.len(), 2);
    let auto_plan = session
        .state
        .corporate_actions
        .dividends
        .iter()
        .map(|book| book.plan())
        .find(|plan| plan.plan_id == "simple-preference:C-600888:dividend:2030-02-28")
        .unwrap()
        .clone();
    assert_eq!(auto_plan.gross_per_share, Money::from_cents(309));
    assert_eq!(auto_plan.ex_dividend_on, ex_on);
    assert!(rejections(&session, &issuer).is_empty());
    // 无跌停行情下合并参考价 = 1000 − 809 = 191 分，日程走完不 poison。
    advance_to(&mut session, ex_on.next().unwrap());
    assert!(session.poison_reason().is_none());
}

#[test]
fn auto_stock_proposal_rejected_when_same_ex_date_explicit_event_exists() {
    // 同除息/除权日既存在显式送转事件时，自动送转提案被拒：两起送转事件的
    // 合并除权口径未核实，除权日日结会显式失败，自动提案不得叠加。
    let (mut session, issuer, stock) = session_with_registry(
        stock_preference(1, 100_000, 1_000_000_000, 1),
        205,
        10_000_000,
    );
    let exchange = crate::calendar::CalendarExchange::Sse;
    let calendar = session.state.civil_clock.calendar().clone();
    // 同上：显式送转事件上一周期批准，除权日程与第二周期自动提案重合。
    advance_to(&mut session, d("2030-01-21"));
    let (registered_on, ex_on) = schedule_after_announcement(&calendar, exchange, d("2030-03-01"));
    let explicit_event = crate::company::stock_distribution::StockDistributionEventPlan {
        event_id: "explicit-same-ex-rights".into(),
        approval_reference: "explicit fixture event".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange,
        kind: crate::company::stock_distribution::StockDistributionKind::BonusShares,
        approved_on: d("2030-01-21"),
        announced_on: d("2030-03-01"),
        registered_on,
        ex_rights_on: ex_on,
        shares_per_existing_share_micros: 100_000,
        approved_total_new_shares: 1,
    };
    session.approve_stock_distribution(explicit_event).unwrap();
    advance_to(&mut session, d("2030-03-01"));
    assert_eq!(
        session.state.corporate_actions.stock_distributions.len(),
        1,
        "同除权日已有显式送转事件时不得再叠加自动送转"
    );
    let rows = rejections(&session, &issuer);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].kind,
        SimplePreferenceProposalKind::StockDistribution
    );
    assert!(
        rows[0].detail.contains("同除权日"),
        "拒绝原因应指向同除权日送转叠加：{}",
        rows[0].detail
    );
    assert!(session.poison_reason().is_none());
}

#[test]
fn auto_stock_proposal_rejected_when_same_ex_date_rights_offering_exists() {
    // 同除权日既存在配股事件时，自动送转提案被拒：配股×送转同日的合并除权
    // 口径未核实，除权日日结会显式失败（prepare_ex_references_for_current_date
    // 的 StepFatal），自动提案不得叠加；拒绝如实记入偏好台账且日结不 poison。
    let mut setup = preference_setup(stock_preference(1, 100_000, 1_000_000_000, 1));
    setup.rights_offering_enabled = true;
    let (mut session, issuer, stock) = session_with_registry_setup(setup, 205, 10_000_000);
    let exchange = crate::calendar::CalendarExchange::Sse;
    let calendar = session.state.civil_clock.calendar().clone();
    // 显式配股方案上一周期批准：R=2030-03-01、缴款期 1 个交易日、除权日
    // 2030-03-05，与第二周期自动送转提案推导的除权日重合（测试侧独立推导
    // 对拍，避免与实现共用同一推导而循环自证）。
    advance_to(&mut session, d("2030-01-21"));
    let (_registered_on, ex_on) = schedule_after_announcement(&calendar, exchange, d("2030-03-01"));
    let mut rights_plan = crate::company::rights_offering::RightsOfferingEventPlan {
        event_id: "rights-same-ex-collision".into(),
        approval_reference: "rights collision fixture".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange,
        approved_on: d("2030-01-21"),
        announced_on: d("2030-01-21"),
        registered_on: d("2030-03-01"),
        payment_start_on: d("2030-03-01"),
        payment_deadline_on: d("2030-03-01"),
        ex_rights_on: d("2030-03-01"),
        settlement_on: d("2030-03-01"),
        listing_on: d("2030-03-01"),
        price_per_share: Money::from_cents(10),
        mode: crate::company::rights_offering::RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        },
        npc_subscription_strategy:
            crate::company::rights_offering::RightsSubscriptionStrategy::FullByDefault,
    };
    rights_plan.derive_schedule(&calendar, exchange, 1).unwrap();
    assert_eq!(
        rights_plan.ex_rights_on, ex_on,
        "配股除权日必须与第二周期自动送转提案除权日重合"
    );
    session.approve_rights_offering(rights_plan).unwrap();
    advance_to(&mut session, d("2030-03-01"));
    // 第一周期自动送转（除权日 2030-02-05）不受影响；第二周期提案与配股同
    // 除权日必须被拒：不新增第二本送转账簿，且现存账簿除权日不与配股重合。
    assert_eq!(
        session.state.corporate_actions.stock_distributions.len(),
        1,
        "同除权日已有配股事件时不得再叠加自动送转"
    );
    assert_ne!(
        session.state.corporate_actions.stock_distributions[0]
            .plan()
            .ex_rights_on,
        ex_on,
        "被拒提案不得入账"
    );
    let rows = rejections(&session, &issuer);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].kind,
        SimplePreferenceProposalKind::StockDistribution
    );
    assert!(
        rows[0].detail.contains("同除权日") && rows[0].detail.contains("配股"),
        "拒绝原因应指向同除权日配股叠加：{}",
        rows[0].detail
    );
    // 日程走完除权日不 poison：无人认购的配股不构成除权分量，被拒的自动
    // 送转也不入账簿，除权日无碰撞组。
    advance_to(&mut session, ex_on.next().unwrap());
    assert!(session.poison_reason().is_none());
}

#[test]
fn auto_stock_proposal_rejected_when_same_ex_date_split_event_exists() {
    // 同除权日既存在拆股／缩股事件时，自动送转提案被拒：重新计值×送转同日
    // 的合并除权口径未核实，除权日日结会显式失败，自动提案不得叠加；拒绝
    // 如实记入偏好台账且日结不 poison。
    let (mut session, issuer, stock) = session_with_registry(
        stock_preference(1, 100_000, 1_000_000_000, 1),
        205,
        10_000_000,
    );
    let exchange = crate::calendar::CalendarExchange::Sse;
    let calendar = session.state.civil_clock.calendar().clone();
    // 显式缩股方案上一周期批准：R=2030-03-04、除权日 2030-03-05，与第二周期
    // 自动送转提案推导的除权日重合（测试侧独立推导对拍，避免与实现共用同一
    // 推导而循环自证）。面值权威：注册资本 10,000,000 分 ÷ 10,000,000 股 =
    // 1 分；2 并 1 → 新面值 2 分（整除放大成立）。
    advance_to(&mut session, d("2030-01-21"));
    let (registered_on, ex_on) = schedule_after_announcement(&calendar, exchange, d("2030-03-01"));
    let split_plan = crate::company::share_split::ShareSplitEventPlan {
        event_id: "split-same-ex-collision".into(),
        approval_reference: "split collision fixture".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange,
        direction: crate::company::share_split::ShareSplitDirection::Consolidate,
        ratio: 2,
        approved_on: d("2030-01-21"),
        announced_on: d("2030-03-01"),
        registered_on,
        ex_rights_on: ex_on,
    };
    session.approve_share_split(split_plan).unwrap();
    advance_to(&mut session, d("2030-03-01"));
    // 第一周期自动送转（除权日 2030-02-05）不受影响；第二周期提案与缩股同
    // 除权日必须被拒：不新增第二本送转账簿，且现存账簿除权日不与缩股重合。
    assert_eq!(
        session.state.corporate_actions.stock_distributions.len(),
        1,
        "同除权日已有拆股／缩股事件时不得再叠加自动送转"
    );
    assert_ne!(
        session.state.corporate_actions.stock_distributions[0]
            .plan()
            .ex_rights_on,
        ex_on,
        "被拒提案不得入账"
    );
    let rows = rejections(&session, &issuer);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].kind,
        SimplePreferenceProposalKind::StockDistribution
    );
    assert!(
        rows[0].detail.contains("同除权日") && rows[0].detail.contains("拆股／缩股"),
        "拒绝原因应指向同除权日拆股／缩股叠加：{}",
        rows[0].detail
    );
    // 日程走完除权日不 poison：缩股正常入账（重新计值独立成立），被拒的
    // 自动送转不入账簿，除权日无碰撞组。
    advance_to(&mut session, ex_on.next().unwrap());
    assert!(session.poison_reason().is_none());
    assert_eq!(
        session.state.corporate_actions.share_splits.len(),
        1,
        "显式缩股按自身日程正常入账"
    );
    assert_eq!(
        session.state.corporate_actions.share_splits[0].settled_on(),
        Some(ex_on)
    );
}

#[test]
fn preference_configuration_validates_through_setup() {
    let mut setup = preference_setup(cash_preference(0, 1, 1));
    assert!(setup.validate().is_err(), "0bp 派息比例必须在装配期被拒绝");
    let mut setup = preference_setup(cash_preference(3_000, 1, 1));
    assert!(setup.validate().is_ok());
    // 显式 API 与偏好自动提案共享入口：显式构造仍可正常提交（并存语义）。
    let (mut session, issuer, stock) =
        session_with_registry(SimpleCompanyPreferences::none(), 5, 10_000_000);
    advance_to(&mut session, d("2030-01-28"));
    let approved_on = session.civil_date();
    let calendar = session.state.civil_clock.calendar().clone();
    let registered_on = if calendar
        .is_trading_day(
            crate::calendar::CalendarExchange::Sse,
            approved_on.next().unwrap(),
        )
        .unwrap()
    {
        approved_on.next().unwrap()
    } else {
        calendar
            .next_trading_day(
                crate::calendar::CalendarExchange::Sse,
                approved_on.next().unwrap(),
            )
            .unwrap()
    };
    let ex_date = calendar
        .next_trading_day(crate::calendar::CalendarExchange::Sse, registered_on)
        .unwrap();
    let plan = crate::company::cash_dividend::CashDividendPlan::new(
        "explicit-plan-coexists".into(),
        issuer.clone(),
        stock.clone(),
        crate::calendar::CalendarExchange::Sse,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        approved_on,
        approved_on.next().unwrap(),
        registered_on,
        ex_date,
        ex_date,
        Money::from_cents(1),
        Money::from_cents(10_000_000),
        &calendar,
    )
    .unwrap();
    session
        .approve_cash_dividend(
            DividendDeclaration {
                plan_id: plan.plan_id.clone(),
                approved_on,
                total_gross: crate::accounting::AccountingAmount::from_cents(6),
                registered_capital: crate::accounting::AccountingAmount::from_cents(10_000_000),
            },
            plan,
        )
        .unwrap();
    assert_eq!(session.state.corporate_actions.dividends.len(), 1);
}
