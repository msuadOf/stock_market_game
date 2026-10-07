//! 开局自动装配公司行为基础的 Session 级测试（N2a，2026-10-08 用户决策
//!「开局自动建名册 + 温和默认偏好 + 后入股东同步补开个人税账」）。
//!
//! 覆盖：正常新局（无任何手工装配）自动构建全流通名册与面值推定的注册资本
//! 法定事实、三层税制模式的开账差异（AShareIndividual 自动开个人税账 /
//! FlatWithholding、Exempt 不开）、温和默认偏好在正常新局的完整闭环
//!（偏好评估 → 自动分红 → 全链路到账）、面值参数编辑生效、后入股东经二级
//! 市场净买入首次入册时的税账同步钩子（Flat 付款日代扣对照）、严格持久化
//! （缺新字段旧档显式拒绝、零面值拒绝）与恢复深等。

use super::*;
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::config::CompanySystemConfig;
use crate::company::share_registry::HolderId;
use crate::company::simple::preferences::{
    SimpleCashDividendPreference, SimpleCompanyPreferences,
};
use crate::money::Money;

fn d(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

/// 温和默认偏好（与 Web DEFAULT_SETUP / CompanyPreferencesInput 预设同一组
/// 游戏化虚拟参数）：现金分红启用，目标派息比例 30%，最小可分配利润门槛
/// 100 万元，每结算周期评估一次；送转默认关闭。
fn mild_default_preferences() -> SimpleCompanyPreferences {
    SimpleCompanyPreferences {
        cash_dividend: Some(SimpleCashDividendPreference {
            target_payout_bp: 3_000,
            min_distributable_profit: Money::from_cents(100_000_000),
            cycles_between_proposals: 1,
        }),
        stock_distribution: None,
    }
}

/// 正常新局 setup：600888 单股、总股本 1000 万股、流通盘 800 万股（其余
/// 200 万为未分配余量）、3 散户 + 1 机构 NPC、月度结算、每交易日 2 tick。
fn auto_foundation_setup(mode: CashDividendTaxMode, par_cents: i64) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.dividend_tax_mode = mode;
    setup.flat_withholding_bp = match mode {
        CashDividendTaxMode::FlatWithholding => Some(1000),
        _ => None,
    };
    setup.config.stamp_tax_rate = if mode == CashDividendTaxMode::Exempt {
        0.0
    } else {
        0.0005
    };
    setup.start_date = d("2030-01-20");
    setup.ticks_per_day = 2;
    setup.stocks[0].float_shares = 8_000_000;
    setup.npcs = NpcSetup {
        retail_count: 3,
        inst_count: 1,
        hot_count: 0,
        retail_cash_median: Money::from_cents(10_000_000),
    };
    setup.par_value_per_share = Money::from_cents(par_cents);
    setup.auto_corporate_foundation = true;
    setup
}

/// 在温和默认偏好基础上把公司基本面放大到「首个结算周期后可分配利润稳定
/// 超过偏好门槛」的量级（收放差额 100 万元/期 × 24 期前史），避免测试
/// 对可分配利润临界值过敏。
fn auto_foundation_setup_with_mild_preferences(
    mode: CashDividendTaxMode,
    par_cents: i64,
) -> SessionSetup {
    let mut setup = auto_foundation_setup(mode, par_cents);
    if let CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.companies[0].preferences = mild_default_preferences();
        config.companies[0].generation.initial_revenue =
            crate::accounting::AccountingAmount::from_cents(1_000_000_000);
        config.companies[0].generation.initial_fixed_expense =
            crate::accounting::AccountingAmount::from_cents(100_000_000);
    }
    setup
}

/// 完成一个自然日：交易日步进当日全部 tick 后日结；休市日直接日结。
fn run_day(session: &mut GameSession) {
    if session.state.civil_clock.phase() == CivilPhase::IntradayTrading {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
    }
    session.end_civil_day().unwrap();
}

/// 从盘中收尾一个交易日：测试已在当日步进过 `stepped` 个 tick（如真实成交
/// 消耗一个 tick），剩余 tick 步进完后日结；休市日直接日结。
fn finish_day_after_steps(session: &mut GameSession, stepped: u64) {
    if session.state.civil_clock.phase() == CivilPhase::IntradayTrading {
        for _ in stepped..session.state.setup.ticks_per_day {
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

/// 账户在某证券上的真实持仓股数。
fn position_qty(session: &GameSession, account: AccountId, stock: &StockCode) -> u64 {
    u64::from(
        session
            .account(account)
            .unwrap()
            .positions()
            .get(stock)
            .map(|position| position.qty())
            .unwrap_or(0),
    )
}

/// 名册某持有人的股数合计；持有人不存在返回 None。
fn holding_qty(
    session: &GameSession,
    stock: &StockCode,
    holder: &HolderId,
) -> Option<u64> {
    session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| registry.stock() == stock)
        .and_then(|registry| {
            registry
                .holdings()
                .iter()
                .find(|holding| &holding.holder == holder)
                .map(|holding| {
                    holding
                        .lots
                        .iter()
                        .fold(0_u64, |total, lot| total + lot.qty)
                })
        })
}

// ---------------------------------------------------------------------------
// 开局自动装配：名册 / 法定事实 / 三层税制开账
// ---------------------------------------------------------------------------

#[test]
fn normal_new_game_auto_assembles_full_float_registry_with_founding_external_holder() {
    let session = GameSession::new(auto_foundation_setup(CashDividendTaxMode::FlatWithholding, 100), 42)
        .unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let actions = &session.state.corporate_actions;
    assert_eq!(actions.registries.len(), 1, "开局自动为每只证券建一个名册");
    let registry = &actions.registries[0];
    assert_eq!(registry.stock(), &stock);
    assert_eq!(registry.issued_shares(), 10_000_000, "发行股数 = 总股本");
    // 名册结算日取开局日前一自然日：开局日自身的成交也要进入名册日结。
    assert_eq!(registry.settled_on(), d("2030-01-19"));
    // 无发行人自持股（全流通口径）。
    assert!(
        !registry
            .holdings()
            .iter()
            .any(|holding| matches!(holding.holder, HolderId::IssuerTreasury)),
        "全流通名册不得出现 IssuerTreasury 自持股"
    );
    // 账户持有人 = 初始筹码分配结果；玩家初始零持仓不入册。
    for (account, holding) in registry
        .holdings()
        .iter()
        .filter_map(|holding| match &holding.holder {
            HolderId::Account(account) => Some((*account, holding)),
            _ => None,
        })
    {
        let expected = position_qty(&session, account, &stock);
        let actual = holding.lots.iter().fold(0_u64, |total, lot| total + lot.qty);
        assert_eq!(
            actual, expected,
            "名册持有人 {account:?} 必须与初始筹码分配结果一致"
        );
        assert!(expected > 0, "零持仓账户不得进入名册");
    }
    assert_eq!(
        holding_qty(&session, &stock, &HolderId::Account(AccountId(0))),
        None,
        "玩家初始零持仓：不得在名册中出现"
    );
    // 未分配余量登记为具名外部股东（非发行人自持股）。
    let account_total: u64 = registry
        .holdings()
        .iter()
        .filter_map(|holding| match &holding.holder {
            HolderId::Account(_) => Some(
                holding
                    .lots
                    .iter()
                    .fold(0_u64, |total, lot| total + lot.qty),
            ),
            _ => None,
        })
        .sum();
    assert_eq!(account_total, 8_000_000, "账户持有人合计 = 流通盘");
    let external = registry
        .holdings()
        .iter()
        .find(|holding| {
            matches!(&holding.holder, HolderId::External(name) if name.starts_with("session-auto:founding:"))
        })
        .expect("未分配余量必须登记为具名外部股东");
    let external_qty = external.lots.iter().fold(0_u64, |total, lot| total + lot.qty);
    assert_eq!(external_qty, 2_000_000, "外部股东承接总股本 − 流通盘余量");
}

#[test]
fn auto_assembly_binds_registered_capital_from_par_value_and_total_shares() {
    let session = GameSession::new(auto_foundation_setup(CashDividendTaxMode::FlatWithholding, 100), 42)
        .unwrap();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&session.state.setup.stocks[0].code)
        .unwrap()
        .clone();
    let facts = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .expect("开局自动装配必须绑定注册资本法定事实");
    assert_eq!(
        facts.registered_capital,
        crate::accounting::AccountingAmount::from_cents(100 * 10_000_000),
        "注册资本 = 面值 1 元/股 × 总股本 1000 万股"
    );
    assert!(
        facts.source_evidence.starts_with("session-auto:registered-capital:"),
        "来源证据必须标明自动装配口径：{}",
        facts.source_evidence
    );
}

#[test]
fn edited_par_value_two_yuan_doubles_auto_registered_capital() {
    let session = GameSession::new(auto_foundation_setup(CashDividendTaxMode::FlatWithholding, 200), 42)
        .unwrap();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&session.state.setup.stocks[0].code)
        .unwrap()
        .clone();
    let facts = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .expect("面值 2 元开局同样自动绑定法定事实");
    assert_eq!(
        facts.registered_capital,
        crate::accounting::AccountingAmount::from_cents(200 * 10_000_000),
        "面值改 2 元/股 → 注册资本 = 2 × 总股本"
    );
}

#[test]
fn ashare_mode_auto_opens_personal_tax_books_for_initial_holders_only() {
    let session = GameSession::new(
        auto_foundation_setup(CashDividendTaxMode::AShareIndividual, 100),
        42,
    )
    .unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let books = &session.state.corporate_actions.dividend_tax_books;
    let personal_expected: Vec<AccountId> = session
        .state
        .accounts
        .iter()
        .filter(|(id, account)| {
            matches!(account.kind(), AccountKind::Retail) && position_qty(&session, **id, &stock) > 0
        })
        .map(|(id, _)| *id)
        .collect();
    assert!(
        !personal_expected.is_empty(),
        "fixture 必须有持有流通盘的自然人散户"
    );
    let mut configured: Vec<AccountId> = books
        .iter()
        .map(|book| book.account())
        .collect();
    configured.sort();
    let mut expected = personal_expected.clone();
    expected.sort();
    assert_eq!(
        configured, expected,
        "大 A 方式只为「个人」身份的初始持有人自动开税账（玩家零持仓、机构不入）"
    );
    for book in books {
        assert_eq!(book.stock(), &stock);
        assert_eq!(
            session.account_dividend_tax_status(book.account()).unwrap().stocks[0].status,
            corporate_actions::DividendTaxStatus::IndividualPublicMarket
        );
    }
    // 玩家零持仓：无税账且状态为未配置（待买入入册后由后入股东钩子补开）。
    assert_eq!(
        session.account_dividend_tax_status(AccountId(0)).unwrap().stocks[0].status,
        corporate_actions::DividendTaxStatus::TreatmentNotConfigured
    );
}

#[test]
fn flat_and_exempt_modes_open_no_tax_books_at_auto_assembly() {
    for mode in [CashDividendTaxMode::FlatWithholding, CashDividendTaxMode::Exempt] {
        let session = GameSession::new(auto_foundation_setup(mode, 100), 42).unwrap();
        assert!(
            session.state.corporate_actions.dividend_tax_books.is_empty(),
            "{mode:?} 开局自动装配不建任何个人税账"
        );
        assert_eq!(
            session.state.corporate_actions.registries.len(),
            1,
            "{mode:?} 同样自动建名册（名册与税账正交）"
        );
    }
}

#[test]
fn disabled_auto_foundation_keeps_explicit_assembly_path() {
    let mut setup = auto_foundation_setup(CashDividendTaxMode::AShareIndividual, 100);
    setup.auto_corporate_foundation = false;
    // 名册勾稽要求账户持仓与名册一致：显式装配场景用零流通盘，全部股本由
    // 玩家 + 具名外部股东承接，保持既有手工装配测试的最小形态。
    setup.stocks[0].float_shares = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    assert!(
        session.state.corporate_actions.registries.is_empty(),
        "关闭开关时保持既有显式装配语义：开局无名册"
    );
    // 既有手工装配入口仍然可用（本组其余测试文件依赖该路径）。
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            session.state.setup.stocks[0].code.clone(),
            crate::account::Position::from_restored_parts(100, 0, 1_000, 0),
        );
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&session.state.setup.stocks[0].code)
        .unwrap()
        .clone();
    session
        .define_dividend_legal_facts(
            &issuer,
            crate::accounting::AccountingAmount::from_cents(100 * 10_000_000),
            "explicit fixture".into(),
        )
        .unwrap();
    let registry = crate::company::share_registry::ShareRegistry::new(
        session.state.setup.stocks[0].code.clone(),
        issuer,
        10_000_000,
        session.civil_date(),
        vec![
            crate::company::share_registry::ShareHolding {
                holder: HolderId::Account(AccountId(0)),
                lots: vec![crate::company::share_registry::ShareLot {
                    id: "player-lot".into(),
                    qty: 100,
                    acquired_on: session.civil_date(),
                    source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                        evidence: "explicit fixture".into(),
                    },
                    restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
                }],
            },
            crate::company::share_registry::ShareHolding {
                holder: HolderId::External("founding".into()),
                lots: vec![crate::company::share_registry::ShareLot {
                    id: "founding-lot".into(),
                    qty: 9_999_900,
                    acquired_on: session.civil_date(),
                    source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                        evidence: "explicit fixture".into(),
                    },
                    restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
                }],
            },
        ],
    )
    .unwrap();
    session.configure_share_registry(registry).unwrap();
    assert_eq!(session.state.corporate_actions.registries.len(), 1);
}

// ---------------------------------------------------------------------------
// 温和默认偏好闭环：正常新局（无手工装配）→ 首个结算周期 → 自动分红 → 到账
// ---------------------------------------------------------------------------

#[test]
fn normal_new_game_mild_preferences_auto_dividend_pays_full_chain_flat_mode() {
    let mut session = GameSession::new(
        auto_foundation_setup_with_mild_preferences(CashDividendTaxMode::FlatWithholding, 100),
        42,
    )
    .unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    // 从 2030-01-20 推进过首个结算周期（月度）末日 2030-01-31 的日结。
    advance_to(&mut session, d("2030-02-01"));
    assert_eq!(
        session.state.corporate_actions.dividends.len(),
        1,
        "温和默认偏好在正常新局首个结算周期后必须自动产生分红方案（不再因无名册跳过）"
    );
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    assert_eq!(
        plan.plan_id,
        "simple-preference:C-600888:dividend:2030-01-31",
        "走与显式 API 完全相同的偏好自动提案确定性 id"
    );
    assert!(plan.gross_per_share.cents() > 0, "派息额应为正");
    // 推进到派发日次日：全链路到账。
    advance_to(&mut session, plan.payable_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
    // 简税模式：每位账户持有人到账恰有一条金额一致的代扣回执（付款日原子代扣）。
    let snapshot_holdings: Vec<AccountId> = session.state.corporate_actions.dividends[0]
        .registration()
        .unwrap()
        .holdings()
        .iter()
        .filter_map(|holding| match &holding.holder {
            HolderId::Account(account) => Some(*account),
            _ => None,
        })
        .collect();
    assert!(!snapshot_holdings.is_empty());
    for account in snapshot_holdings {
        let gross_receipts: Vec<_> = session
            .state
            .corporate_actions
            .account_gross_receipts
            .iter()
            .filter(|receipt| receipt.account == account && receipt.plan_id == plan.plan_id)
            .collect();
        assert_eq!(gross_receipts.len(), 1, "账户 {account:?} 到账凭证恰好一条");
        let withholdings: Vec<_> = session
            .state
            .corporate_actions
            .flat_withholding_receipts
            .iter()
            .filter(|receipt| receipt.account == account && receipt.plan_id == plan.plan_id)
            .collect();
        assert_eq!(
            withholdings.len(),
            1,
            "简税模式付款日每位账户持有人恰有一条代扣回执"
        );
        assert_eq!(withholdings[0].rate_bp, 1000);
        assert_eq!(withholdings[0].gross, gross_receipts[0].gross);
        assert!(withholdings[0].withheld.cents() > 0);
    }
    // 具名外部股东（未分配余量）无游戏账户：留外部回执、不代扣。
    let external_receipts: Vec<_> = session
        .state
        .corporate_actions
        .external_receipts
        .iter()
        .filter(|receipt| receipt.plan_id == plan.plan_id)
        .collect();
    assert_eq!(
        external_receipts.len(),
        1,
        "未分配余量的具名外部股东按持股取得外部回执"
    );
    assert!(external_receipts[0].gross.cents() > 0);
    assert!(stock.0 == "600888");
}

// ---------------------------------------------------------------------------
// 后入股东：二级市场净买入 → 首次入册 → 同步开个人税账（AShare）/ 付款日代扣（Flat）
// ---------------------------------------------------------------------------

/// 让玩家在当日真实买入 100 股：向订单簿挂 NPC 卖单，玩家提交限价买单并
/// 推进一个 tick 产生真实成交（复用既有真实成交 fixture 模式）。
fn player_buys_100_shares_today(session: &mut GameSession) {
    let stock = session.state.setup.stocks[0].code.clone();
    let seller = session
        .state
        .accounts
        .iter()
        .find(|(id, account)| {
            id.0 != 0 && u64::from(account.positions().get(&stock).map(|p| p.qty()).unwrap_or(0)) >= 100
        })
        .map(|(id, _)| *id)
        .expect("fixture 必须有足额持股的 NPC 卖方");
    if let Some(attention) = session.state.npc_attention.get_mut(&seller) {
        attention.next_attention_candidate_tick = u64::MAX;
    }
    session.state.attention_scheduler = [(u64::MAX, seller)].into_iter().collect();
    let order_id = session.state.next_order_id;
    session
        .state
        .markets
        .get_mut(&stock)
        .unwrap()
        .place(crate::Order {
            id: crate::OrderId(order_id),
            side: crate::Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: seller,
            seq: order_id,
        })
        .unwrap();
    session.state.next_order_id += 1;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            crate::Intent::PlaceLimit {
                code: stock.clone(),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    let events = session.step().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::Event::Trade { .. })),
        "fixture 必须产生真实成交"
    );
}

#[test]
fn late_holder_ashare_entry_opens_tax_book_and_next_dividend_registers_gross() {
    let mut session = GameSession::new(
        auto_foundation_setup_with_mild_preferences(CashDividendTaxMode::AShareIndividual, 100),
        42,
    )
    .unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    // 开局日 2030-01-20 是周末：先推进到首个交易日 2030-01-21 再真实买入。
    advance_to(&mut session, d("2030-01-21"));
    player_buys_100_shares_today(&mut session);
    let trade_date = session.civil_date();
    finish_day_after_steps(&mut session, 1);
    // 入册事实：玩家以净买入进入名册。
    assert_eq!(
        holding_qty(&session, &stock, &HolderId::Account(AccountId(0))),
        Some(100),
        "玩家经二级市场净买入首次入册"
    );
    // 同步开个人税账：FIFO 从入册日取得事实起算。
    let book = session
        .state
        .corporate_actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == AccountId(0) && book.stock() == &stock)
        .expect("后入股东首次入册必须同步开个人税账");
    assert_eq!(book.lots().len(), 1);
    assert_eq!(book.lots()[0].qty, 100);
    assert_eq!(book.lots()[0].acquired_on, trade_date);
    assert_eq!(
        session.account_dividend_tax_status(AccountId(0)).unwrap().stocks[0].status,
        corporate_actions::DividendTaxStatus::IndividualPublicMarket
    );
    // 推进过首个结算周期末日：温和默认偏好自动分红。
    advance_to(&mut session, d("2030-02-01"));
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    assert_eq!(
        plan.plan_id,
        "simple-preference:C-600888:dividend:2030-01-31"
    );
    // 推进到派发日次日：大 A 方式付款日不代扣（个人≤1 年持股转让时补缴），
    // 玩家税前全额到账且税账登记该笔分红。
    let cash_before_payment = session.account(AccountId(0)).unwrap().cash();
    advance_to(&mut session, plan.payable_on.next().unwrap());
    assert!(
        session.state.corporate_actions.dividends[0]
            .registration()
            .expect("派发后必须已有登记快照")
            .holdings()
            .iter()
            .any(|holding| holding.holder == HolderId::Account(AccountId(0))),
        "入册后的下次分红登记快照必须包含玩家"
    );
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
    assert_eq!(
        session.account(AccountId(0)).unwrap().cash(),
        cash_before_payment
            .add(Money::from_cents(plan.gross_per_share.cents() * 100))
            .unwrap(),
        "大 A 方式分红到账为税前全额（≤1 年持股付款日不代扣）"
    );
    assert!(
        session.state.corporate_actions.flat_withholding_receipts.is_empty(),
        "大 A 方式不得产生简税代扣回执"
    );
    let book = session
        .state
        .corporate_actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == AccountId(0) && book.stock() == &stock)
        .unwrap();
    let book_json = serde_json::to_value(book).unwrap();
    let registered = book_json
        .get("dividends")
        .and_then(|dividends| dividends.as_array())
        .expect("税账序列化契约含 dividends 字段");
    assert_eq!(
        registered.len(),
        1,
        "后入股东的税账必须登记其入册后的分红（税基按 100 股计）"
    );
}

/// 边界（独立复核补齐）：入册日恰为分红**股权登记日**的当日买入。日终管线
/// 先推进名册（`close_registries_through`）再冻结登记快照
/// （`process_dividends_on_day_end`），登记日当日经二级市场净买入首次入册的
/// 账户按「登记日收盘持有即享有」纳入快照；后入税账 opened_on = 登记日−1、
/// 首条日结（取得日=登记日）紧邻开账日次日，与分红 registered_on=登记日同日
/// 自洽，持有期自登记日起算。
#[test]
fn late_holder_buying_on_registration_day_is_included_in_snapshot_same_day() {
    let mut session = GameSession::new(
        auto_foundation_setup_with_mild_preferences(CashDividendTaxMode::AShareIndividual, 100),
        42,
    )
    .unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    // 推进到首个结算周期末日次日（评估/批准/公告日），产生自动分红方案。
    advance_to(&mut session, d("2030-02-01"));
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    // 测试侧独立推导权益登记日：公告日后首个交易日且不与公告日同日
    //（逐自然日查日历，不与实现共用推导）。
    let calendar = session.state.civil_clock.calendar();
    let mut registration_day = plan.announced_on;
    loop {
        registration_day = registration_day.next().unwrap();
        if calendar
            .is_trading_day(crate::calendar::CalendarExchange::Sse, registration_day)
            .unwrap()
        {
            break;
        }
    }
    assert_ne!(
        registration_day,
        plan.announced_on,
        "登记日不与公告日同日（typed 公告通道约束）"
    );
    // 登记日当日（开市后）真实买入并完成当日日结：名册先入册、快照后冻结。
    advance_to(&mut session, registration_day);
    assert_eq!(
        session.civil_date(),
        registration_day,
        "fixture 必须停在登记日当日盘中"
    );
    player_buys_100_shares_today(&mut session);
    finish_day_after_steps(&mut session, 1);
    // 同日入册 → 纳入登记快照（登记日收盘持有即享有）。
    let snapshot = session.state.corporate_actions.dividends[0]
        .registration()
        .expect("登记日日结后必须已有登记快照");
    assert_eq!(snapshot.registered_on(), registration_day);
    assert!(
        snapshot
            .holdings()
            .iter()
            .any(|holding| holding.holder == HolderId::Account(AccountId(0))),
        "登记日当日买入首次入册的玩家必须进入当日登记快照"
    );
    // 后入税账同日开账并登记分红：取得日=登记日，registered_on=登记日。
    let book = session
        .state
        .corporate_actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == AccountId(0) && book.stock() == &stock)
        .expect("登记日当日首次入册必须同步开个人税账");
    assert_eq!(book.lots().len(), 1);
    assert_eq!(book.lots()[0].qty, 100);
    assert_eq!(book.lots()[0].acquired_on, registration_day);
    let book_json = serde_json::to_value(book).unwrap();
    let registered_dividend = book_json
        .get("dividends")
        .and_then(|dividends| dividends.as_array())
        .expect("税账序列化契约含 dividends 字段");
    assert_eq!(
        registered_dividend.len(),
        1,
        "登记日入册的税账必须登记当日冻结的分红"
    );
    let expected_registered_on = serde_json::to_value(registration_day)
        .unwrap()
        .as_str()
        .map(str::to_string);
    assert_eq!(
        registered_dividend[0].get("registered_on").and_then(|day| day.as_str()),
        expected_registered_on.as_deref(),
        "分红税基登记日必须等于入册日（持有期自该日起算）"
    );
    // 推进到派发日次日：玩家按快照 100 股税前全额到账（≤1 年付款日不代扣）。
    let cash_before_payment = session.account(AccountId(0)).unwrap().cash();
    advance_to(&mut session, plan.payable_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
    assert_eq!(
        session.account(AccountId(0)).unwrap().cash(),
        cash_before_payment
            .add(Money::from_cents(plan.gross_per_share.cents() * 100))
            .unwrap(),
        "登记日当日入册的玩家按快照持股税前全额到账"
    );
}

#[test]
fn late_holder_flat_mode_gets_payment_day_withholding_without_tax_book() {
    let mut session = GameSession::new(
        auto_foundation_setup_with_mild_preferences(CashDividendTaxMode::FlatWithholding, 100),
        42,
    )
    .unwrap();
    // 开局日 2030-01-20 是周末：先推进到首个交易日 2030-01-21 再真实买入。
    advance_to(&mut session, d("2030-01-21"));
    player_buys_100_shares_today(&mut session);
    finish_day_after_steps(&mut session, 1);
    assert_eq!(
        holding_qty(
            &session,
            &session.state.setup.stocks[0].code,
            &HolderId::Account(AccountId(0))
        ),
        Some(100),
        "简税模式后入股东同样经净买入入册（代扣按名册事实执行）"
    );
    assert!(
        session
            .state
            .corporate_actions
            .dividend_tax_books
            .iter()
            .all(|book| book.account() != AccountId(0)),
        "简税模式不为后入股东开个人税账"
    );
    advance_to(&mut session, d("2030-02-01"));
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    let cash_before_payment = session.account(AccountId(0)).unwrap().cash();
    advance_to(&mut session, plan.payable_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
    let withheld: i64 = session
        .state
        .corporate_actions
        .flat_withholding_receipts
        .iter()
        .filter(|receipt| receipt.account == AccountId(0) && receipt.plan_id == plan.plan_id)
        .map(|receipt| receipt.withheld.cents())
        .sum();
    assert_eq!(withheld, 10 * plan.gross_per_share.cents(), "后入股东按 10% 代扣");
    assert_eq!(
        session.account(AccountId(0)).unwrap().cash().cents(),
        cash_before_payment.cents() + plan.gross_per_share.cents() * 100 - withheld,
        "玩家税后到账 = 税前应得 − 付款日代扣"
    );
}

// ---------------------------------------------------------------------------
// 严格持久化：旧档缺新字段显式拒绝 / 零面值拒绝 / 恢复深等
// ---------------------------------------------------------------------------

#[test]
fn zero_par_value_is_rejected_by_setup_validation() {
    let mut setup = auto_foundation_setup(CashDividendTaxMode::FlatWithholding, 100);
    setup.par_value_per_share = Money::from_cents(0);
    let error = GameSession::new(setup, 42)
        .err()
        .expect("零面值必须在 setup 校验显式拒绝");
    assert!(
        error.to_string().contains("par_value_per_share"),
        "拒绝错误须指明字段：{error}"
    );
}

#[test]
fn saves_missing_new_setup_fields_are_rejected_explicitly() {
    for field in ["par_value_per_share", "auto_corporate_foundation"] {
        let session = GameSession::new(
            auto_foundation_setup(CashDividendTaxMode::FlatWithholding, 100),
            42,
        )
        .unwrap();
        let save = session.save().unwrap();
        let mut json = serde_json::to_value(&save).unwrap();
        let setup = json
            .get_mut("setup")
            .and_then(|setup| setup.as_object_mut())
            .expect("存档 JSON 含 setup 对象");
        assert!(
            setup.remove(field).is_some(),
            "新契约存档必须显式写出 {field}"
        );
        let text = serde_json::to_string(&json).unwrap();
        let error = serde_json::from_str::<SaveSlot>(&text)
            .expect_err("缺少新字段的旧档必须被显式拒绝");
        assert!(
            error.to_string().contains(field),
            "拒绝错误须指明缺失字段 {field}：{error}"
        );
    }
}

#[test]
fn auto_assembled_session_round_trips_through_save_and_restore() {
    let mut session = GameSession::new(
        auto_foundation_setup_with_mild_preferences(CashDividendTaxMode::AShareIndividual, 100),
        42,
    )
    .unwrap();
    // 完成两个自然日（含一个交易日）后保存：名册带真实日结回执、税账带日结事实。
    advance_to(&mut session, d("2030-01-22"));
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(&restored.save().unwrap()).unwrap(),
        serde_json::to_value(&save).unwrap(),
        "自动装配局存档恢复后重新保存必须深度相等"
    );
    assert_eq!(restored.state.corporate_actions.registries.len(), 1);
    assert!(
        !restored.state.corporate_actions.dividend_tax_books.is_empty(),
        "恢复后自动开账的个人税账语义不变"
    );
}
