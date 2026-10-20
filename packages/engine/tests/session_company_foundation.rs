//! B4b 批验收测试：SessionSetup 公司字段族三态校验链 + 构造期公司装配
//! （公司系统/名册/法定事实/回购专户）+ save/restore 公司字段逐位 roundtrip
//! + 无公司配置旧局零行为变化。
//!
//! 范围边界（战役计划 §B4b）：只覆盖构造期与存读面；权益计算/日终推进/
//! 命令通道属 B4c/B4d/B4e，不在本文件断言面内。

include!("../test-support/simple_company.rs");

use engine::company::cash_dividend_tax::CashDividendTaxMode;
use engine::{
    AccountId, AccountKind, FloatAllocation, GameConfig, GameSession, HotParams, InstParams,
    Money, NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange,
    StockSpec, StrategyParams,
};

/// 无公司配置的旧局 setup（公司字段全缺省）。
fn plain_setup() -> SessionSetup {
    SessionSetup {
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![StockSpec {
            code: StockCode("600101".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 100_000,
            float_shares: 100_000,
        }],
        npcs: NpcSetup {
            retail_count: 4,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.8,
                order_size_mean: 200,
                chase_prob: 0.4,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 500,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.01,
                order_size: 300,
            },
        },
        ticks_per_day: 30,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: engine::CivilDate::from_iso("2030-01-02").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
    }
}

/// 含公司配置的新局 setup：Flat 简税 1000bp + 面值 1 元 + 回购启用 +
/// 开局自动装配公司行为基础。
fn company_setup() -> SessionSetup {
    SessionSetup {
        company_system: Some(simple_company_fixture!(engine; ["600101"])),
        dividend_tax_mode: Some(CashDividendTaxMode::FlatWithholding),
        flat_withholding_bp: Some(1000),
        rights_offering_enabled: false,
        issuer_repurchase_enabled: true,
        par_value_per_share: Some(Money::from_cents(100)),
        auto_corporate_foundation: true,
        report_frequency: Some(engine::information::ReportFrequency::Quarterly),
        ..plain_setup()
    }
}

/// 解包构造期校验错误码（SessionError::InvalidSetup 携 ValidationFault）。
fn invalid_setup_code(result: Result<GameSession, engine::SessionError>) -> u16 {
    match result {
        Err(engine::SessionError::InvalidSetup(fault)) => fault.code,
        Err(other) => panic!("期望 InvalidSetup 错误，实际其他错误：{}", other),
        Ok(_) => panic!("期望 InvalidSetup 错误，实际构造成功"),
    }
}

// ── 三态校验链 ─────────────────────────────────────────────────────────────

#[test]
fn company_config_without_company_system_is_rejected() {
    // 公司字段族携带而 company_system 缺席 = 配置残缺，显式拒绝（fault1048）。
    let mut setup = plain_setup();
    setup.dividend_tax_mode = Some(CashDividendTaxMode::FlatWithholding);
    assert_eq!(
        invalid_setup_code(GameSession::new(setup, 7)),
        1048,
        "无公司系统却携带税务模式必须显式拒绝"
    );
}

#[test]
fn flat_withholding_three_state_contract() {
    // Flat 模式缺比例 → fault1052。
    let mut setup = company_setup();
    setup.flat_withholding_bp = None;
    assert_eq!(invalid_setup_code(GameSession::new(setup, 7)), 1052);

    // Flat 模式比例超域（>10000bp）→ fault1053。
    let mut setup = company_setup();
    setup.flat_withholding_bp = Some(10_001);
    assert_eq!(invalid_setup_code(GameSession::new(setup, 7)), 1053);

    // 非 Flat 模式携带比例 → fault1054。
    let mut setup = company_setup();
    setup.dividend_tax_mode = Some(CashDividendTaxMode::AShareIndividual);
    assert_eq!(invalid_setup_code(GameSession::new(setup, 7)), 1054);
}

#[test]
fn exempt_mode_requires_zero_stamp_tax() {
    // 不扣税模式连印花税也免：默认 0.0005 印花税率配 Exempt → fault1055。
    let mut setup = company_setup();
    setup.dividend_tax_mode = Some(CashDividendTaxMode::Exempt);
    setup.flat_withholding_bp = None;
    assert_eq!(invalid_setup_code(GameSession::new(setup, 7)), 1055);
}

#[test]
fn par_value_domain_is_enforced() {
    // 面值非正 → fault1056；公司局缺面值 → fault1056（必填）。
    let mut setup = company_setup();
    setup.par_value_per_share = Some(Money::ZERO);
    assert_eq!(invalid_setup_code(GameSession::new(setup, 7)), 1056);

    let mut setup = company_setup();
    setup.par_value_per_share = None;
    assert_eq!(invalid_setup_code(GameSession::new(setup, 7)), 1056);
}

#[test]
fn missing_dividend_tax_mode_with_company_system_is_rejected() {
    // 公司局必须显式选定税务模式（无 serde 默认的严格持久化语义）→ fault1051。
    let mut setup = company_setup();
    setup.dividend_tax_mode = None;
    setup.flat_withholding_bp = None;
    assert_eq!(invalid_setup_code(GameSession::new(setup, 7)), 1051);
}

// ── 构造期装配 ─────────────────────────────────────────────────────────────

#[test]
fn new_game_with_company_opens_repurchase_account_without_float() {
    let session = GameSession::new(company_setup(), 7).unwrap();
    // NPC 账户 1..=4（retail 4 户）；回购专户 = npc_count + 1 = 5。
    let repurchase = AccountId(5);
    let account = session.account(repurchase).expect("回购专户必须开立");
    assert_eq!(account.kind(), AccountKind::IssuerRepurchase);
    assert!(
        account.positions().is_empty(),
        "回购专户不得参与流通盘分配"
    );
    assert_eq!(account.cash(), Money::ZERO, "回购专户零现金起步");
}

#[test]
fn new_game_without_repurchase_keeps_roster_unchanged() {
    let mut setup = company_setup();
    setup.issuer_repurchase_enabled = false;
    let session = GameSession::new(setup, 7).unwrap();
    assert!(
        session.account(AccountId(5)).is_none(),
        "未启用回购时不得开立专户"
    );
}

#[test]
fn auto_foundation_builds_full_float_registry_and_legal_facts() {
    let session = GameSession::new(company_setup(), 7).unwrap();
    let actions = session.corporate_actions().expect("公司局必有公司行为状态");
    assert_eq!(actions.registries.len(), 1, "每股一份股东名册");
    let registry = &actions.registries[0];
    assert_eq!(registry.stock(), &StockCode("600101".to_owned()));
    assert_eq!(registry.issued_shares(), 100_000);
    // 名册结算日 = 开局自然日前一自然日（开局日自身成交也进入名册日结）。
    assert_eq!(
        registry.settled_on(),
        engine::CivilDate::from_iso("2030-01-01").unwrap()
    );
    // 持有人 = 有真实持仓的账户（含零分配外的全部真实持仓者）+ 未分配余量的
    // 具名外部股东；无发行人自持股。账户面名册记录数与实际持仓户数逐一对应。
    let stock = &StockCode("600101".to_owned());
    let expected_holders: Vec<AccountId> = (1..=4_u64)
        .map(AccountId)
        .filter(|id| {
            session
                .account(*id)
                .expect("NPC 账户在册")
                .positions()
                .values()
                .any(|position| position.qty() > 0)
        })
        .collect();
    let account_holdings: Vec<engine::AccountId> = registry
        .holdings()
        .iter()
        .filter_map(|h| match &h.holder {
            engine::company::share_registry::HolderId::Account(account) => Some(*account),
            _ => None,
        })
        .collect();
    assert_eq!(
        account_holdings, expected_holders,
        "名册账户持有人集合与真实持仓账户逐一对应"
    );
    let granted: u64 = registry
        .holdings()
        .iter()
        .flat_map(|h| h.lots.iter())
        .map(|lot| lot.qty)
        .sum();
    assert_eq!(granted, 100_000, "名册登记总量 = 总股本（含外部股东余量）");
    let _ = stock;
    // 法定事实：注册资本 = 面值 100 分 × 100_000 股 = 10_000_000 分。
    let saved = session.save().unwrap();
    let company_json = serde_json::to_value(&saved.company_system).unwrap();
    assert!(
        serde_json::to_string(&company_json)
            .unwrap()
            .contains("10000000"),
        "注册资本法定事实必须按面值×总股本落账，实际 {company_json:?}"
    );
}

#[test]
fn disabled_foundation_leaves_registry_empty() {
    let mut setup = company_setup();
    setup.auto_corporate_foundation = false;
    let session = GameSession::new(setup, 7).unwrap();
    let actions = session.corporate_actions().expect("公司局必有公司行为状态");
    assert!(actions.registries.is_empty(), "关闭自动装配时名册保持空");
}

// ── save/restore roundtrip ─────────────────────────────────────────────────

#[test]
fn save_restore_roundtrips_company_fields_bitwise() {
    let mut session = GameSession::new(company_setup(), 7).unwrap();
    // 推进若干 tick + 日结，让公司状态跨越存档边界（构造后立即存档是平凡情形）。
    for _ in 0..35 {
        session.step().expect("healthy step");
    }
    let saved = session.save().expect("healthy save");
    let saved_company = saved.company_system.clone().expect("公司局存档必带公司系统");
    let saved_actions = saved.corporate_actions.clone().expect("公司局存档必带公司行为");

    let restored = GameSession::restore(&saved).expect("healthy restore");
    // 逐位比较：restore 后再存档，公司字段与原档完全一致。
    let resaved = restored.save().expect("healthy resave");
    let resaved_company = resaved.company_system.expect("恢复后公司系统必须在档");
    let resaved_actions = resaved.corporate_actions.expect("恢复后公司行为必须在档");
    assert_eq!(saved_company, resaved_company, "公司系统字段逐位一致");
    assert_eq!(saved_actions, resaved_actions, "公司行为字段逐位一致");
    // 领域 Eq 之外再加 wire 形态逐字节（serde 投影面一致）。
    assert_eq!(
        serde_json::to_string(&saved_company).unwrap(),
        serde_json::to_string(&resaved_company).unwrap()
    );
    assert_eq!(
        serde_json::to_string(&saved_actions).unwrap(),
        serde_json::to_string(&resaved_actions).unwrap()
    );
    // 恢复后的 spine 读面与存档一致（名册内容逐位）。
    assert_eq!(
        restored.corporate_actions().expect("读面").registries,
        saved_actions.registries
    );
}

#[test]
fn incomplete_company_save_is_rejected() {
    let session = GameSession::new(company_setup(), 7).unwrap();
    let saved = session.save().unwrap();
    // 只带 company_system 丢 corporate_actions = 不完整档，显式拒绝。
    let mut stripped = saved.clone();
    stripped.corporate_actions = None;
    match GameSession::restore(&stripped) {
        Err(engine::SessionError::InvalidSave(fault)) => assert_eq!(fault.code, 2101),
        Err(other) => panic!("期望 InvalidSave(fault2101)，实际其他错误：{other}"),
        Ok(_) => panic!("期望 InvalidSave(fault2101)，实际恢复成功"),
    }
}

// ── 旧局零行为变化 ─────────────────────────────────────────────────────────

#[test]
fn plain_sessions_save_json_carries_no_company_keys() {
    let mut session = GameSession::new(plain_setup(), 7).unwrap();
    for _ in 0..35 {
        session.step().expect("healthy step");
    }
    let saved = session.save().expect("healthy save");
    let json = serde_json::to_value(&saved).unwrap();
    // SaveSlot 顶层与 setup 子层都不携带公司字段（缺席 = 公司线未激活）。
    for key in ["company_system", "corporate_actions"] {
        assert!(
            json.get(key).is_none(),
            "旧局存档不得携带 {key}（顶层 SaveSlot）"
        );
    }
    let setup_json = serde_json::to_value(&saved.setup).unwrap();
    for key in [
        "company_system",
        "dividend_tax_mode",
        "flat_withholding_bp",
        "rights_offering_enabled",
        "issuer_repurchase_enabled",
        "par_value_per_share",
        "auto_corporate_foundation",
    ] {
        assert!(
            setup_json.get(key).is_none(),
            "旧局 setup 不得携带 {key}"
        );
    }
    // 无公司配置的 spine 读面：公司状态缺席（None）而非空容器。
    assert!(session.corporate_actions().is_none());
    assert!(saved.company_system.is_none());
    assert!(saved.corporate_actions.is_none());
}

#[test]
fn plain_sessions_restore_identically() {
    // 旧局 restore 后行为与原局一致：同 seed 再跑同 tick 数，账户权益与
    // 存档快照逐位一致（构造期公司装配缺席 = 零行为变化的直接证据）。
    let mut session = GameSession::new(plain_setup(), 11).unwrap();
    for _ in 0..35 {
        session.step().expect("healthy step");
    }
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    for id in [AccountId(0), AccountId(1), AccountId(2), AccountId(3), AccountId(4)] {
        let before = session.account(id).expect("账户在册");
        let after = restored.account(id).expect("恢复后账户在册");
        assert_eq!(before.cash(), after.cash(), "账户 {id:?} 现金逐位一致");
        assert_eq!(
            before.positions().len(),
            after.positions().len(),
            "账户 {id:?} 持仓数一致"
        );
    }
}
