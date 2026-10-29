//! B4b 批验收测试：SessionSetup 公司字段族三态校验链 + 构造期公司装配
//! （公司系统/名册/法定事实/回购专户）+ save/restore 公司字段逐位 roundtrip
//! + 无公司配置旧局零行为变化。
//!
//! 范围边界（战役计划 §B4b）：只覆盖构造期与存读面；权益计算/日终推进/
//! 命令通道属 B4c/B4d/B4e，不在本文件断言面内。

include!("../test-support/company_state/system_saved.rs");
include!("../test-support/company_state/system_saved_mut.rs");
include!("../test-support/simple_company.rs");

use engine::company::cash_dividend_tax::CashDividendTaxMode;
use engine::{
    AccountId, AccountKind, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
    NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams,
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
        runtime_sync_policy: Default::default(),
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

mod tax_mode_stamp_tax_contract {
    use super::*;

    fn setup(mode: CashDividendTaxMode, stamp_tax_rate: f64) -> SessionSetup {
        let mut setup = company_setup();
        setup.dividend_tax_mode = Some(mode);
        setup.flat_withholding_bp = (mode == CashDividendTaxMode::FlatWithholding).then_some(1000);
        setup.config.stamp_tax_rate = stamp_tax_rate;
        setup
    }

    #[test]
    fn flat_withholding_accepts_current_a_share_stamp_tax() {
        GameSession::new(setup(CashDividendTaxMode::FlatWithholding, 0.0005), 7)
            .expect("FlatWithholding 必须接受现行印花税率");
    }

    #[test]
    fn a_share_individual_accepts_current_a_share_stamp_tax() {
        GameSession::new(setup(CashDividendTaxMode::AShareIndividual, 0.0005), 7)
            .expect("AShareIndividual 必须接受现行印花税率");
    }

    #[test]
    fn exempt_accepts_zero_stamp_tax_and_preserves_other_fees_on_restore() {
        // Exempt 是 ADR-0040 批准的游戏模式；佣金与过户费仍按原配置收取。
        let setup = setup(CashDividendTaxMode::Exempt, 0.0);
        let config = setup.config.clone();
        let session = GameSession::new(setup, 7).expect("Exempt 必须接受零印花税");
        let saved = session.save().expect("Exempt 必须可保存");
        let restored = GameSession::restore(&saved).expect("Exempt 必须可恢复");
        let resaved = restored.save().expect("恢复后必须可保存");
        assert_eq!(resaved.setup.config, config);
        assert_eq!(
            resaved.setup.dividend_tax_mode,
            Some(CashDividendTaxMode::Exempt)
        );
        let gross = Money::from_cents(100_000);
        assert_eq!(resaved.setup.config.stamp_tax(gross).unwrap(), Money::ZERO);
        assert_eq!(
            resaved.setup.config.commission(gross).unwrap(),
            Money::from_cents(500)
        );
        assert_eq!(
            resaved.setup.config.transfer_fee(gross).unwrap(),
            Money::from_cents(1)
        );
        // 极小正税率在大额合法金额上仍会收费，免税不能依赖舍入恰好为零。
        let large_gross = Money::from_cents(1_000_000_000_000_000_000);
        assert_eq!(
            resaved.setup.config.stamp_tax(large_gross).unwrap(),
            Money::ZERO
        );
    }

    #[test]
    fn exempt_rejects_even_smallest_positive_stamp_tax_at_setup_and_construction() {
        for actual in [f64::from_bits(1), 1e-16, f64::EPSILON] {
            let setup = setup(CashDividendTaxMode::Exempt, actual);
            let errors = [
                setup.validate().expect_err("Exempt 必须精确零税率"),
                GameSession::new(setup, 7)
                    .err()
                    .expect("新局必须拒绝非零税率"),
            ];
            for error in errors {
                match error {
                    engine::SessionError::InvalidSetup(fault) => {
                        assert_eq!(fault.code, 1055);
                        assert_eq!(fault.context, vec![0.0_f64.to_bits(), actual.to_bits()]);
                    }
                    error => panic!("税率 {actual:?} 必须返回配对错误：{error:?}"),
                }
            }
        }
    }

    #[test]
    fn exempt_restore_rejects_saved_positive_stamp_tax_including_epsilon() {
        let session =
            GameSession::new(setup(CashDividendTaxMode::Exempt, 0.0), 7).expect("合法 Exempt 新局");
        let saved = session.save().expect("真实保存生成合法存档");
        for actual in [f64::from_bits(1), 1e-16, f64::EPSILON, 0.0005] {
            let mut edited = saved.clone();
            edited.setup.config.stamp_tax_rate = actual;
            let json = serde_json::to_string(&edited).expect("存档外部输入可序列化");
            let edited = serde_json::from_str(&json).expect("恢复外部输入形态");
            match GameSession::restore(&edited) {
                Err(engine::SessionError::InvalidSave(fault)) => {
                    // 现有恢复契约以 fault2002 包装非法 setup，不扩展存档错误协议。
                    assert_eq!(fault.code, 2002, "税率 {actual:?}");
                    assert!(fault.context.is_empty());
                }
                Err(error) => panic!("税率 {actual:?} 必须返回存档校验错误：{error:?}"),
                Ok(_) => panic!("Exempt 存档必须拒绝非零印花税率 {actual:?}"),
            }
        }
    }

    #[test]
    fn company_modes_reject_mismatched_stamp_tax_with_expected_and_actual_context() {
        for (mode, expected, actual) in [
            (CashDividendTaxMode::FlatWithholding, 0.0005_f64, 0.0_f64),
            (CashDividendTaxMode::AShareIndividual, 0.0005, 0.0),
            (CashDividendTaxMode::Exempt, 0.0, 0.0005),
            (CashDividendTaxMode::Exempt, 0.0, 0.001),
        ] {
            match setup(mode, actual).validate() {
                Err(engine::SessionError::InvalidSetup(fault)) => {
                    assert_eq!(fault.code, 1055, "税务模式 {mode:?}");
                    assert_eq!(fault.context, vec![expected.to_bits(), actual.to_bits()]);
                }
                result => panic!("税务模式 {mode:?} 必须显式拒绝错误税率：{result:?}"),
            }
        }
    }

    #[test]
    fn plain_game_still_rejects_zero_stamp_tax_with_actual_context() {
        let mut setup = plain_setup();
        setup.config.stamp_tax_rate = 0.0;
        match setup.validate() {
            Err(engine::SessionError::InvalidSetup(fault)) => {
                assert_eq!(fault.code, 1013);
                assert_eq!(fault.context, vec![0.0_f64.to_bits()]);
            }
            result => panic!("无公司税务模式时必须保留 A 股税率要求：{result:?}"),
        }
    }
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
    assert!(account.positions().is_empty(), "回购专户不得参与流通盘分配");
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
    // 法定事实：注册资本 = 面值 100 分 × 100_000 股 = 10_000_000 分——按
    // 精确 JSON 路径取值断言（Simple 公司 0 的 finance.legal_facts）。
    let saved = session.save().unwrap();
    let company_json = serde_json::to_value(system_saved(&saved).0).unwrap();
    let capital = &company_json["implementation"]["state"]["companies"]["0"]["finance"]
        ["legal_facts"]["registered_capital"];
    assert_eq!(
        capital, &serde_json::json!("100000.00"),
        "注册资本法定事实必须按面值×总股本落账（AccountingAmount 元两位小数 wire），实际路径值 {capital:?}"
    );
}

#[test]
fn flat_withholding_boundary_values_are_accepted() {
    // 合法域闭边界：0bp（零代扣）与 10000bp（全额代扣）都应通过构造。
    for boundary in [0_u32, 10_000] {
        let mut setup = company_setup();
        setup.flat_withholding_bp = Some(boundary);
        GameSession::new(setup, 7)
            .unwrap_or_else(|error| panic!("边界 bp={boundary} 必须合法：{error}"));
    }
}

#[test]
fn incomplete_company_config_residue_is_isolated_per_field() {
    // 残缺位图逐 bit 隔离：单独携带每个公司字段（company_system 缺席）都
    // 拒绝且 fault 码一致（1048）——杜绝「位图合并掩盖单字段拒绝」。
    type ResidueCase = (String, Box<dyn FnOnce(&mut SessionSetup)>);
    let residue_cases: Vec<ResidueCase> = vec![
        (
            "dividend_tax_mode".to_owned(),
            Box::new(|setup: &mut SessionSetup| {
                setup.dividend_tax_mode = Some(CashDividendTaxMode::FlatWithholding)
            }),
        ),
        (
            "flat_withholding_bp".to_owned(),
            Box::new(|setup: &mut SessionSetup| setup.flat_withholding_bp = Some(1000)),
        ),
        (
            "rights_offering_enabled".to_owned(),
            Box::new(|setup: &mut SessionSetup| setup.rights_offering_enabled = true),
        ),
        (
            "issuer_repurchase_enabled".to_owned(),
            Box::new(|setup: &mut SessionSetup| setup.issuer_repurchase_enabled = true),
        ),
        (
            "par_value_per_share".to_owned(),
            Box::new(|setup: &mut SessionSetup| {
                setup.par_value_per_share = Some(Money::from_cents(100))
            }),
        ),
        (
            "auto_corporate_foundation".to_owned(),
            Box::new(|setup: &mut SessionSetup| setup.auto_corporate_foundation = true),
        ),
    ];
    for (field, mutate) in residue_cases {
        let mut setup = plain_setup();
        mutate(&mut setup);
        assert_eq!(
            invalid_setup_code(GameSession::new(setup, 7)),
            1048,
            "无公司系统携带 {field} 必须以 fault1048 拒绝"
        );
    }
}

#[test]
fn restore_rejects_company_system_diverging_from_setup_issuers() {
    // 篡改档的发行人登记与 setup 重建不一致（fault1063）：直接替换存档
    // company_system 为「另一份配置」构造局的系统状态。
    let session = GameSession::new(company_setup(), 7).unwrap();
    let saved = session.save().unwrap();
    // 用两家股（不同 setup 形状）构造一份不同发行人集的公司系统。
    let mut two_stock = company_setup();
    let second = StockCode("600889".to_owned());
    let mut spec = two_stock.stocks[0].clone();
    spec.code = second.clone();
    two_stock.stocks.push(spec);
    two_stock.company_system = Some(simple_company_fixture!(engine; ["600101", "600889"]));
    let other = GameSession::new(two_stock, 7).unwrap();
    let other_saved = other.save().unwrap();
    let mut tampered = saved.clone();
    *system_saved_mut(&mut tampered).0 = system_saved(&other_saved).0.clone();
    match GameSession::restore(&tampered) {
        Err(engine::SessionError::InvalidSave(fault)) => assert_eq!(fault.code, 1063),
        Err(other_err) => panic!("期望 InvalidSave(fault1063)，实际 {other_err}"),
        Ok(_) => panic!("发行人登记与 setup 不一致的档必须显式拒绝"),
    }
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
    let saved_company = system_saved(&saved).0.clone();
    let saved_actions = system_saved(&saved).1.clone();

    let restored = GameSession::restore(&saved).expect("healthy restore");
    // 逐位比较：restore 后再存档，公司字段与原档完全一致。
    let resaved = restored.save().expect("healthy resave");
    let resaved_company = system_saved(&resaved).0.clone();
    let resaved_actions = system_saved(&resaved).1.clone();
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
    let mut malformed = serde_json::to_value(&saved).unwrap();
    malformed["company_state"]["CompanySystem"]
        .as_object_mut()
        .unwrap()
        .remove("corporate_actions");
    assert!(
        serde_json::from_value::<engine::SaveSlot>(malformed).is_err(),
        "真实required字段不能缺席"
    );
    stripped.company_state = GameSession::new(plain_setup(), 7)
        .unwrap()
        .save()
        .unwrap()
        .company_state;
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
        assert!(setup_json.get(key).is_none(), "旧局 setup 不得携带 {key}");
    }
    // 无公司配置的 spine 读面：公司状态缺席（None）而非空容器。
    assert!(session.corporate_actions().is_none());
    assert!(matches!(
        &saved.company_state,
        engine::session::SavedSessionCompanies::LegacyOperations { .. }
    ));
    assert!(!matches!(
        &saved.company_state,
        engine::session::SavedSessionCompanies::CompanySystem { .. }
    ));
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
    for id in [
        AccountId(0),
        AccountId(1),
        AccountId(2),
        AccountId(3),
        AccountId(4),
    ] {
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
