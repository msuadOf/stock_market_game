//! 三层税制（2026-10-08 用户产品决策）的 Session 级测试。
//!
//! 覆盖：`CashDividendTaxMode` 三变体（`FlatWithholding` 简税默认 / `AShareIndividual`
//! 大 A 个人差别化 / `Exempt` 不扣税）的 setup 严格契约（简税比例三态、印花税按模式
//! 门禁、旧档三向显式拒绝）、Flat 模式付款日代扣精确断言（默认比例、改比例、机构
//! 持有人、舍入口径）、AShare 模式既有自动开账行为保持、Exempt 卖出印花税为零
//! 且佣金照收、存档往返与恢复深等。

use super::corporate_actions::{AccountStockDividendTaxStatus, TaxpayerIdentity};
use super::*;
use crate::account::Position;
use crate::company::cash_dividend_tax::{CashDividendTaxMode, DividendTaxProfile};
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::orderbook::AccountId;

/// 组装指定税务模式的 setup；`stamp_override` 仅 Exempt 用（必须 0.0）。
fn tax_mode_setup(mode: CashDividendTaxMode) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.retail_count = 1;
    setup.dividend_tax_mode = mode;
    setup.flat_withholding_bp = None;
    setup
}

fn tax_mode_setup_with_rate(mode: CashDividendTaxMode, rate_bp: u32) -> SessionSetup {
    let mut setup = tax_mode_setup(mode);
    setup.flat_withholding_bp = Some(rate_bp);
    setup
}

fn exempt_setup() -> SessionSetup {
    let mut setup = tax_mode_setup(CashDividendTaxMode::Exempt);
    // 不扣税模式按 2026-10-08 决策连印花税也免：setup 契约强制 stamp=0。
    setup.config.stamp_tax_rate = 0.0;
    setup
}

fn fixture_lot(id: &str, qty: u64, acquired_on: CivilDate) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on,
        source: AcquisitionSource::InitialAllocation {
            evidence: "tax-mode fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

/// 装配一个带完整股东名册的会话：玩家(0)、散户 NPC(1) 为个人持有人
/// （populate 顺序为 Retail 先于 Inst），机构 NPC(2) 为机构持有人，
/// 另含 Treasury 与具名 External 持有人。
fn session_with_registry(mode: CashDividendTaxMode) -> (GameSession, StockCode) {
    session_with_registry_and_holders(
        tax_mode_setup(mode),
        vec![
            (AccountId(0), "player-lot", 5_u64),
            (AccountId(1), "retail-lot", 3),
            (AccountId(2), "institution-lot", 2),
        ],
    )
}

/// 存档往返用变体：不含散户持有人（fixture 直插持仓不生成自然人经历，
/// 会触发存档校验「retail account 缺少持仓经历」）。
fn session_with_registry_without_retail_holder(
    mode: CashDividendTaxMode,
) -> (GameSession, StockCode) {
    session_with_registry_and_holders(
        tax_mode_setup(mode),
        vec![
            (AccountId(0), "player-lot", 5_u64),
            (AccountId(2), "institution-lot", 3),
        ],
    )
}

fn session_with_registry_and_holders(
    setup: SessionSetup,
    holders: Vec<(AccountId, &'static str, u64)>,
) -> (GameSession, StockCode) {
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
    let date = CivilDate::from_iso("2030-01-02").unwrap();
    let held: u64 = holders.iter().map(|(_, _, qty)| *qty).sum();
    for (account, _, qty) in &holders {
        session
            .state
            .accounts
            .get_mut(account)
            .unwrap()
            .fixture_insert_position(
                stock.clone(),
                Position::from_restored_parts((*qty).try_into().unwrap(), 0, 1_000, 0),
            );
    }
    let mut holdings: Vec<ShareHolding> = holders
        .into_iter()
        .map(|(account, lot_id, qty)| ShareHolding {
            holder: HolderId::Account(account),
            lots: vec![fixture_lot(lot_id, qty, date)],
        })
        .collect();
    holdings.push(ShareHolding {
        holder: HolderId::IssuerTreasury,
        lots: vec![fixture_lot("treasury-lot", total_shares - held - 1, date)],
    });
    holdings.push(ShareHolding {
        holder: HolderId::External("external-holder".into()),
        lots: vec![fixture_lot("external-lot", 1, date)],
    });
    let registry = ShareRegistry::new(stock.clone(), issuer, total_shares, date, holdings).unwrap();
    session.configure_share_registry(registry).unwrap();
    (session, stock)
}

// ---------------------------------------------------------------------------
// Setup 严格契约：简税比例三态 + 印花税模式门禁 + 旧档三向拒绝
// ---------------------------------------------------------------------------

#[test]
fn flat_mode_requires_rate_and_other_modes_reject_it() {
    let error = tax_mode_setup(CashDividendTaxMode::FlatWithholding)
        .validate()
        .unwrap_err();
    assert!(
        error.to_string().contains("flat_withholding_bp"),
        "Flat 缺比例必须显式拒绝并指明字段：{error}"
    );
    for mode in [
        CashDividendTaxMode::AShareIndividual,
        CashDividendTaxMode::Exempt,
    ] {
        let mut setup = if mode == CashDividendTaxMode::Exempt {
            exempt_setup()
        } else {
            tax_mode_setup(mode)
        };
        setup.flat_withholding_bp = Some(1000);
        let error = setup.validate().unwrap_err();
        assert!(
            error.to_string().contains("flat_withholding_bp"),
            "非 Flat 模式携带比例必须显式拒绝：{error}"
        );
    }
    tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, 1000)
        .validate()
        .unwrap();
}

#[test]
fn flat_mode_rate_is_bounded_to_full_withholding() {
    for rate_bp in [0_u32, 10_000] {
        tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, rate_bp)
            .validate()
            .unwrap_or_else(|error| panic!("边界比例 {rate_bp}bp 必须合法：{error}"));
    }
    let error = tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, 10_001)
        .validate()
        .unwrap_err();
    assert!(
        error.to_string().contains("flat_withholding_bp"),
        "超过全额代扣的比例必须显式拒绝：{error}"
    );
}

#[test]
fn stamp_tax_rate_is_gated_by_tax_mode() {
    // 不扣税模式连印花税也免：非零印花税被拒绝。
    let mut setup = tax_mode_setup(CashDividendTaxMode::Exempt);
    let error = setup.validate().unwrap_err();
    assert!(
        error.to_string().contains("stamp_tax_rate"),
        "Exempt 携带 0.0005 印花税必须被拒绝：{error}"
    );
    exempt_setup().validate().unwrap();
    // 简税与大 A 方式保持正式 A 股印花税基线。
    tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, 1000)
        .validate()
        .unwrap();
    tax_mode_setup(CashDividendTaxMode::AShareIndividual)
        .validate()
        .unwrap();
    let mut flat = tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, 1000);
    flat.config.stamp_tax_rate = 0.0;
    let error = flat.validate().unwrap_err();
    assert!(
        error.to_string().contains("stamp_tax_rate"),
        "Flat 模式免除印花税必须被拒绝：{error}"
    );
}

/// 旧档三向拒绝：旧两变体枚举值、Flat 缺比例、非 Flat 携带比例。
#[test]
fn legacy_two_variant_enum_value_is_rejected_explicitly() {
    let (session, _stock) =
        session_with_registry_without_retail_holder(CashDividendTaxMode::AShareIndividual);
    let save = session.save().unwrap();
    let mut json = serde_json::to_value(&save).unwrap();
    let setup = json
        .get_mut("setup")
        .and_then(|setup| setup.as_object_mut())
        .expect("存档 JSON 含 setup 对象");
    setup.insert(
        "dividend_tax_mode".into(),
        serde_json::json!("IndividualPublicMarket"),
    );
    let text = serde_json::to_string(&json).unwrap();
    let error = serde_json::from_str::<SaveSlot>(&text)
        .expect_err("旧两变体枚举值必须被显式拒绝，不静默映射到新模式");
    assert!(
        error.to_string().contains("IndividualPublicMarket")
            && error.to_string().contains("unknown variant"),
        "拒绝错误须指明被拒的旧枚举值：{error}"
    );
}

/// 旧档三向拒绝之二与之三：Flat 模式缺比例、非 Flat 模式携带比例。
/// 比例三态属 setup 语义契约，拒绝发生在恢复装配（`GameSession::new` 的
/// `setup.validate()`），与 serde 缺字段拒绝（见 tax mode 用例）分层。
#[test]
fn flat_save_without_rate_and_non_flat_save_with_rate_are_rejected() {
    let (session, _stock) =
        session_with_registry_without_retail_holder(CashDividendTaxMode::AShareIndividual);
    let save = session.save().unwrap();
    let mut json = serde_json::to_value(&save).unwrap();
    {
        let setup = json
            .get_mut("setup")
            .and_then(|setup| setup.as_object_mut())
            .expect("存档 JSON 含 setup 对象");
        setup.insert("dividend_tax_mode".into(), serde_json::json!("FlatWithholding"));
        setup.insert("flat_withholding_bp".into(), serde_json::Value::Null);
    }
    let save = serde_json::from_value::<SaveSlot>(json.clone()).unwrap();
    let error = match GameSession::restore(&save) {
        Ok(_) => panic!("Flat 模式缺比例的档必须在恢复装配时被显式拒绝"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("flat_withholding_bp"),
        "拒绝错误须指明字段：{error}"
    );

    {
        let setup = json
            .get_mut("setup")
            .and_then(|setup| setup.as_object_mut())
            .expect("存档 JSON 含 setup 对象");
        setup.insert("dividend_tax_mode".into(), serde_json::json!("AShareIndividual"));
        setup.insert("flat_withholding_bp".into(), serde_json::json!(1000));
    }
    let save = serde_json::from_value::<SaveSlot>(json).unwrap();
    let error = match GameSession::restore(&save) {
        Ok(_) => panic!("非 Flat 模式携带比例的档必须在恢复装配时被显式拒绝"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("flat_withholding_bp"),
        "拒绝错误须指明字段：{error}"
    );
}

#[test]
fn save_without_tax_mode_field_is_rejected_explicitly() {
    let (session, _stock) = session_with_registry_and_holders(
        exempt_setup(),
        vec![(AccountId(0), "player-lot", 5_u64), (AccountId(2), "institution-lot", 3)],
    );
    let save = session.save().unwrap();
    let mut json = serde_json::to_value(&save).unwrap();
    let setup = json
        .get_mut("setup")
        .and_then(|setup| setup.as_object_mut())
        .expect("存档 JSON 含 setup 对象");
    assert!(
        setup.remove("dividend_tax_mode").is_some(),
        "新契约存档必须显式写出税务模式"
    );
    let text = serde_json::to_string(&json).unwrap();
    let error = serde_json::from_str::<SaveSlot>(&text)
        .expect_err("缺少 dividend_tax_mode 的旧档必须被显式拒绝");
    assert!(
        error.to_string().contains("dividend_tax_mode"),
        "拒绝错误须指明缺失字段：{error}"
    );
}

// ---------------------------------------------------------------------------
// 装配期行为：三模式自动开账差异 + 显式配置门禁
// ---------------------------------------------------------------------------

#[test]
fn a_share_mode_opens_individual_public_market_books_for_personal_holders_only() {
    let (session, stock) = session_with_registry(CashDividendTaxMode::AShareIndividual);
    let books = &session.state.corporate_actions.dividend_tax_books;
    let mut configured: Vec<(AccountId, StockCode)> = books
        .iter()
        .map(|book| (book.account(), book.stock().clone()))
        .collect();
    configured.sort();
    assert_eq!(
        configured,
        vec![(AccountId(0), stock.clone()), (AccountId(1), stock.clone())],
        "大 A 方式只为个人身份账户（玩家+散户 NPC）自动开税账"
    );
}

#[test]
fn flat_and_exempt_modes_open_no_tax_books_at_registry_assembly() {
    for mode in [
        CashDividendTaxMode::FlatWithholding,
        CashDividendTaxMode::Exempt,
    ] {
        let (session, _stock) = if mode == CashDividendTaxMode::FlatWithholding {
            session_with_registry_and_holders(
                tax_mode_setup_with_rate(mode, 1000),
                vec![
                    (AccountId(0), "player-lot", 5_u64),
                    (AccountId(1), "retail-lot", 3),
                    (AccountId(2), "institution-lot", 2),
                ],
            )
        } else {
            session_with_registry_and_holders(
                exempt_setup(),
                vec![
                    (AccountId(0), "player-lot", 5_u64),
                    (AccountId(1), "retail-lot", 3),
                    (AccountId(2), "institution-lot", 2),
                ],
            )
        };
        assert!(
            session
                .state
                .corporate_actions
                .dividend_tax_books
                .is_empty(),
            "{mode:?} 模式在装配期不配置任何税账"
        );
    }
}

#[test]
fn flat_mode_rejects_explicit_tax_book_configuration_but_exempt_keeps_it() {
    let (mut flat_session, stock) = session_with_registry_and_holders(
        tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, 1000),
        vec![
            (AccountId(0), "player-lot", 5_u64),
            (AccountId(2), "institution-lot", 3),
        ],
    );
    let error = flat_session
        .configure_cash_dividend_tax_book(
            AccountId(0),
            stock.clone(),
            DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("FlatWithholding"),
        "简税模式显式配置个人差别化税账必须被拒绝（防双重计税）：{error}"
    );

    let (mut exempt_session, exempt_stock) =
        session_with_registry_and_holders(
        exempt_setup(),
        vec![(AccountId(0), "player-lot", 5_u64), (AccountId(2), "institution-lot", 3)],
    );
    exempt_session
        .configure_cash_dividend_tax_book(
            AccountId(0),
            exempt_stock.clone(),
            DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap();
    assert_eq!(
        exempt_session.state.corporate_actions.dividend_tax_books.len(),
        1,
        "不扣税模式只是不自动配置；宿主显式装配期配置仍是既有入口"
    );
}

#[test]
fn taxpayer_identity_maps_player_and_retail_to_personal_and_institutions_to_pending() {
    use super::corporate_actions::taxpayer_identity_of_kind;
    use crate::account::AccountKind;
    assert_eq!(
        taxpayer_identity_of_kind(AccountKind::Player),
        TaxpayerIdentity::Personal
    );
    assert_eq!(
        taxpayer_identity_of_kind(AccountKind::Retail),
        TaxpayerIdentity::Personal
    );
    assert_eq!(
        taxpayer_identity_of_kind(AccountKind::Inst),
        TaxpayerIdentity::NonIndividualPending
    );
    assert_eq!(
        taxpayer_identity_of_kind(AccountKind::Hot),
        TaxpayerIdentity::NonIndividualPending
    );
}

#[test]
fn account_dividend_tax_status_reports_mode_identity_and_per_stock_treatment() {
    let (session, stock) = session_with_registry(CashDividendTaxMode::AShareIndividual);
    let player = session.account_dividend_tax_status(AccountId(0)).unwrap();
    assert_eq!(player.mode, CashDividendTaxMode::AShareIndividual);
    assert_eq!(player.identity, TaxpayerIdentity::Personal);
    assert_eq!(
        player.stocks,
        vec![AccountStockDividendTaxStatus {
            stock: stock.clone(),
            status: corporate_actions::DividendTaxStatus::IndividualPublicMarket,
        }]
    );
    let institution = session.account_dividend_tax_status(AccountId(2)).unwrap();
    assert_eq!(institution.identity, TaxpayerIdentity::NonIndividualPending);
    assert_eq!(
        institution.stocks,
        vec![AccountStockDividendTaxStatus {
            stock,
            status: corporate_actions::DividendTaxStatus::TreatmentNotConfigured,
        }]
    );

    let (flat_session, flat_stock) = session_with_registry_and_holders(
        tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, 1000),
        vec![
            (AccountId(0), "player-lot", 5_u64),
            (AccountId(1), "retail-lot", 3),
            (AccountId(2), "institution-lot", 2),
        ],
    );
    for account in [AccountId(0), AccountId(2)] {
        let flat = flat_session.account_dividend_tax_status(account).unwrap();
        assert_eq!(flat.mode, CashDividendTaxMode::FlatWithholding);
        assert_eq!(
            flat.stocks,
            vec![AccountStockDividendTaxStatus {
                stock: flat_stock.clone(),
                status: corporate_actions::DividendTaxStatus::FlatWithholding,
            }],
            "简税模式对名册上每位持有人（含机构）呈现同一代扣口径"
        );
    }

    let (exempt_session, exempt_stock) =
        session_with_registry_and_holders(
        exempt_setup(),
        vec![(AccountId(0), "player-lot", 5_u64), (AccountId(2), "institution-lot", 3)],
    );
    let exempt = exempt_session
        .account_dividend_tax_status(AccountId(0))
        .unwrap();
    assert_eq!(exempt.mode, CashDividendTaxMode::Exempt);
    assert_eq!(exempt.identity, TaxpayerIdentity::Personal);
    assert_eq!(
        exempt.stocks,
        vec![AccountStockDividendTaxStatus {
            stock: exempt_stock,
            status: corporate_actions::DividendTaxStatus::TreatmentNotConfigured,
        }]
    );
}

#[test]
fn account_dividend_tax_status_rejects_unknown_account_explicitly() {
    let (session, _stock) = session_with_registry(CashDividendTaxMode::AShareIndividual);
    let error = session
        .account_dividend_tax_status(AccountId(999))
        .unwrap_err();
    assert!(
        error.to_string().contains("999"),
        "未知账户错误须携带账户标识：{error}"
    );
}

// ---------------------------------------------------------------------------
// Flat 模式付款日代扣
// ---------------------------------------------------------------------------

/// Flat 模式分红 fixture：按传入 setup（须为 Flat 模式并携带比例）装配
/// 玩家(0)、散户(1)、机构(2) 与具名外部持有人入册，每股 `per_share_cents`
/// 现金分红，2030-01-08 付款。返回（会话、玩家税前应得、散户税前应得、
/// 机构税前应得、外部持有人税前应得）。
fn flat_dividend_fixture(
    setup: SessionSetup,
    per_share_cents: i64,
    holders: &[(AccountId, &'static str, u64)],
) -> (GameSession, i64, i64, i64, i64) {
    use crate::accounting::AccountingAmount;
    use crate::company::{
        cash_dividend::CashDividendPlan, ex_reference_price::CashDividendFormula,
    };

    let mut session = GameSession::new(setup, 42).unwrap();
    // 冻结 NPC 注意力，保证断言期间现金只受分红与代扣影响。
    for account in [AccountId(1), AccountId(2)] {
        if let Some(attention) = session.state.npc_attention.get_mut(&account) {
            attention.next_attention_candidate_tick = u64::MAX;
        }
    }
    session.state.attention_scheduler = [(u64::MAX, AccountId(1)), (u64::MAX, AccountId(2))]
        .into_iter()
        .collect();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let date = |value: &str| CivilDate::from_iso(value).unwrap();
    let approved_on = date("2030-01-02");
    let capital = AccountingAmount::from_cents(1_000_000);
    session
        .define_dividend_legal_facts(&issuer, capital, "flat tax fixture legal fact".into())
        .unwrap();
    let total_shares = session.state.setup.stocks[0].total_shares;
    let held: u64 = holders.iter().map(|(_, _, qty)| *qty).sum();
    for (account, _, qty) in holders {
        session
            .state
            .accounts
            .get_mut(account)
            .unwrap()
            .fixture_insert_position(
                stock.clone(),
                Position::from_restored_parts((*qty).try_into().unwrap(), 0, 1_000, 0),
            );
    }
    let mut holdings: Vec<ShareHolding> = holders
        .iter()
        .map(|(account, lot_id, qty)| ShareHolding {
            holder: HolderId::Account(*account),
            lots: vec![fixture_lot(lot_id, *qty, approved_on)],
        })
        .collect();
    holdings.push(ShareHolding {
        holder: HolderId::IssuerTreasury,
        lots: vec![fixture_lot(
            "treasury-lot",
            total_shares - held - 1,
            approved_on,
        )],
    });
    holdings.push(ShareHolding {
        holder: HolderId::External("external-holder".into()),
        lots: vec![fixture_lot("external-lot", 1, approved_on)],
    });
    let registry =
        ShareRegistry::new(stock.clone(), issuer.clone(), total_shares, approved_on, holdings)
            .unwrap();
    session.configure_share_registry(registry).unwrap();
    let eligible_gross_cents = per_share_cents * i64::try_from(held + 1).unwrap();
    let plan = CashDividendPlan::new(
        "flat-tax-test".into(),
        issuer.clone(),
        stock,
        crate::calendar::CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        approved_on,
        date("2030-01-03"),
        date("2030-01-04"),
        date("2030-01-07"),
        date("2030-01-08"),
        Money::from_cents(per_share_cents),
        Money::from_cents(eligible_gross_cents),
        session.state.civil_clock.calendar(),
    )
    .unwrap();
    session
        .approve_cash_dividend(
            crate::company::DividendDeclaration {
                plan_id: plan.plan_id.clone(),
                approved_on,
                total_gross: AccountingAmount::from_cents(i128::from(eligible_gross_cents)),
                registered_capital: capital,
            },
            plan,
        )
        .unwrap();
    let gross_of = |account: AccountId| -> i64 {
        holders
            .iter()
            .find(|(holder, _, _)| *holder == account)
            .map(|(_, _, qty)| per_share_cents * i64::try_from(*qty).unwrap())
            .unwrap_or(0)
    };
    (
        session,
        gross_of(AccountId(0)),
        gross_of(AccountId(1)),
        gross_of(AccountId(2)),
        per_share_cents,
    )
}

/// 推进到 2030-01-08 付款日日结完成（账户持有人到账即代扣）。
fn advance_until_dividend_paid(session: &mut GameSession) {
    let payable = CivilDate::from_iso("2030-01-08").unwrap();
    while session.civil_date() <= payable {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
        if session
            .corporate_actions()
            .account_gross_receipts
            .iter()
            .any(|receipt| receipt.account == AccountId(0))
        {
            return;
        }
    }
    panic!(
        "fixture 必须在 2030-01-08 付款日到达；当前 {}",
        session.civil_date().to_iso()
    );
}

fn flat_fixture_setup(rate_bp: u32) -> SessionSetup {
    let mut setup = tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, rate_bp);
    setup.npcs.inst_count = 1;
    setup.ticks_per_day = 1;
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.prehistory_periods = 24;
        config.settlement_cycle = crate::company::simple::period::SettlementCycle::Monthly;
    }
    setup
}

#[test]
fn flat_withholding_deducts_default_ten_percent_from_every_registered_account_holder() {
    // 手算基准：每股 10 分。玩家 5 股（50 分）、散户 3 股（30 分）、机构 2 股
    //（20 分）各按默认 1000bp 代扣 10%；外部具名持有人 10 分全额到账
    //（无游戏账户，不代扣——与 AShare 模式同一边界）。
    let holders = vec![
        (AccountId(0), "player-lot", 5_u64),
        (AccountId(1), "retail-lot", 3),
        (AccountId(2), "institution-lot", 2),
    ];
    let (mut session, player_gross, retail_gross, inst_gross, external_gross) =
        flat_dividend_fixture(flat_fixture_setup(1000), 10, &holders);
    assert_eq!((player_gross, retail_gross, inst_gross, external_gross), (50, 30, 20, 10));
    let before: Vec<Money> = [AccountId(0), AccountId(1), AccountId(2)]
        .iter()
        .map(|account| session.account(*account).unwrap().cash())
        .collect();
    advance_until_dividend_paid(&mut session);

    let receipts = &session.corporate_actions().flat_withholding_receipts;
    let mut by_account: Vec<(AccountId, i64, i64)> = receipts
        .iter()
        .map(|receipt| (receipt.account, receipt.gross.cents(), receipt.withheld.cents()))
        .collect();
    by_account.sort_by_key(|(account, _, _)| account.0);
    assert_eq!(
        by_account,
        vec![
            (AccountId(0), 50, 5),
            (AccountId(1), 30, 3),
            (AccountId(2), 20, 2),
        ],
        "每位账户持有人（含机构与散户 NPC）按 1000bp 代扣，比例随回执冻结"
    );
    for (index, account) in [AccountId(0), AccountId(1), AccountId(2)].into_iter().enumerate() {
        let expected_net = by_account[index].1 - by_account[index].2;
        assert_eq!(
            session.account(account).unwrap().cash(),
            before[index].add(Money::from_cents(expected_net)).unwrap(),
            "账户 {account:?} 到账净额 = 税前应得 − 代扣"
        );
    }
    assert!(
        session
            .corporate_actions()
            .dividend_tax_books
            .is_empty(),
        "简税模式不创建、不持久化任何个人差别化税账"
    );
    assert!(
        session
            .corporate_actions()
            .external_receipts
            .iter()
            .all(|receipt| receipt.gross == Money::from_cents(10)
                && receipt.tax_status == corporate_actions::DividendTaxStatus::TreatmentNotConfigured),
        "外部具名持有人全额到账且不产生个人税事实"
    );
    assert!(
        session
            .corporate_actions()
            .account_gross_receipts
            .iter()
            .all(|receipt| receipt.tax_status == corporate_actions::DividendTaxStatus::FlatWithholding),
        "账户持有人的到账回执统一标记 FlatWithholding"
    );
    assert!(
        session.dividend_tax_outstanding_views().unwrap().is_empty(),
        "原子代扣不存在未清税额追缴事实"
    );
}

#[test]
fn flat_withholding_respects_edited_rate_and_rounds_half_up_to_cents() {
    // 改比例 5000bp、每股 7 分：3 股 21 分 × 50% = 10.5 → half-up 11 分；
    // 1 股 7 分 × 50% = 3.5 → half-up 4 分。
    let holders = vec![
        (AccountId(0), "player-lot", 3_u64),
        (AccountId(1), "retail-lot", 1),
        (AccountId(2), "institution-lot", 1),
    ];
    let (mut session, player_gross, retail_gross, _inst_gross, _external_gross) =
        flat_dividend_fixture(flat_fixture_setup(5000), 7, &holders);
    assert_eq!((player_gross, retail_gross), (21, 7));
    advance_until_dividend_paid(&mut session);
    let mut by_account: Vec<(AccountId, i64)> = session
        .corporate_actions()
        .flat_withholding_receipts
        .iter()
        .map(|receipt| (receipt.account, receipt.withheld.cents()))
        .collect();
    by_account.sort_by_key(|(account, _)| account.0);
    assert_eq!(
        by_account,
        vec![(AccountId(0), 11), (AccountId(1), 4), (AccountId(2), 4)],
        "代扣按持有人税前应得 × 比例四舍五入（half-up）到分"
    );
}

#[test]
fn flat_withholding_never_collects_supplementary_tax_on_disposal() {
    let holders = vec![
        (AccountId(0), "player-lot", 5_u64),
        (AccountId(1), "retail-lot", 3),
        (AccountId(2), "institution-lot", 2),
    ];
    let (mut session, _p, _r, _i, _e) = flat_dividend_fixture(flat_fixture_setup(1000), 10, &holders);
    advance_until_dividend_paid(&mut session);
    // 继续若干日终：简税模式无税账、无收缴回执、无追缴。
    for _ in 0..3 {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
    }
    assert!(session.corporate_actions().dividend_tax_books.is_empty());
    assert_eq!(
        session.corporate_actions().flat_withholding_receipts.len(),
        3,
        "后续日终不追加任何代扣或收缴事实"
    );
    assert!(session.dividend_tax_outstanding_views().unwrap().is_empty());
}

#[test]
fn flat_mode_round_trips_through_save_and_restores_identical_semantics() {
    let holders = vec![
        (AccountId(0), "player-lot", 5_u64),
        (AccountId(2), "institution-lot", 3),
    ];
    let (mut session, _p, _r, _i, _e) = flat_dividend_fixture(flat_fixture_setup(1000), 10, &holders);
    advance_until_dividend_paid(&mut session);
    let save = session.save().unwrap();
    assert_eq!(save.setup.dividend_tax_mode, CashDividendTaxMode::FlatWithholding);
    assert_eq!(save.setup.flat_withholding_bp, Some(1000));
    assert!(save.corporate_actions.dividend_tax_books.is_empty());
    assert_eq!(save.corporate_actions.flat_withholding_receipts.len(), 2);
    let encoded = serde_json::to_value(&save).unwrap();
    let restored = GameSession::restore(&save).unwrap();
    let resaved = serde_json::to_value(restored.save().unwrap()).unwrap();
    // 信念/观察域在恢复后存在与税无关的重启差异（既有引擎边界，非本批语义），
    // 税务契约只断言 setup 与 corporate_actions 两个子树深度相等。
    for section in ["setup", "corporate_actions"] {
        assert_eq!(
            resaved.get(section),
            encoded.get(section),
            "恢复后重新保存的 {section} 子树与原档深度相等"
        );
    }
    assert_eq!(
        restored
            .account_dividend_tax_status(AccountId(0))
            .unwrap()
            .stocks,
        session
            .account_dividend_tax_status(AccountId(0))
            .unwrap()
            .stocks,
        "恢复后代扣语义不变"
    );
    assert_eq!(
        restored.corporate_actions().flat_withholding_receipts,
        session.corporate_actions().flat_withholding_receipts,
        "代扣回执事实完整恢复"
    );
}

// ---------------------------------------------------------------------------
// 印花税三模式差异（真实卖出成交断言）
// ---------------------------------------------------------------------------

/// 直插持仓并真实卖出，返回该笔卖出的成交确认（含 actual_fees）。
fn sell_from_player_today(
    session: &mut GameSession,
    qty: u32,
) -> PersonalTradeConfirmation {
    let stock = session.state.setup.stocks[0].code.clone();
    let buyer = AccountId(1);
    if let Some(attention) = session.state.npc_attention.get_mut(&buyer) {
        attention.next_attention_candidate_tick = u64::MAX;
    }
    session.state.attention_scheduler = [(u64::MAX, buyer)].into_iter().collect();
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(stock.clone(), Position::from_restored_parts(qty, 0, 1_000, 0));
    let buyer_order_id = session.state.next_order_id;
    session
        .state
        .markets
        .get_mut(&stock)
        .unwrap()
        .place(crate::Order {
            id: crate::OrderId(buyer_order_id),
            side: crate::Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: buyer,
            seq: buyer_order_id,
        })
        .unwrap();
    session.state.next_order_id += 1;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            crate::Intent::PlaceLimit {
                code: stock.clone(),
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty,
            },
        )
        .unwrap();
    let sale_date = session.civil_date();
    let events = session.step().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::Event::Trade { .. })),
        "fixture 必须产生真实成交"
    );
    session
        .personal_trade_confirmations(AccountId(0))
        .iter()
        .find(|confirmation| {
            confirmation.civil_date == sale_date
                && confirmation.code == stock
                && confirmation.side == crate::Side::Sell
                && confirmation.quantity_shares == qty
        })
        .cloned()
        .expect("真实卖出必须留下个人成交确认")
}

#[test]
fn exempt_mode_charges_no_stamp_tax_while_commission_stays() {
    let mut setup = exempt_setup();
    setup.ticks_per_day = 4;
    // 默认开局日 2030-01-01 是休市日；改到 2030-01-02 交易日内在盘中真实成交。
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = GameSession::new(setup, 42).unwrap();
    let confirmation = sell_from_player_today(&mut session, 5);
    assert_eq!(
        confirmation.actual_fees.stamp_tax,
        Money::ZERO,
        "不扣税模式连印花税也免（2026-10-08 决策）"
    );
    assert!(
        confirmation.actual_fees.commission.cents() > 0,
        "佣金照付：{confirmation:?}"
    );
    assert!(
        confirmation.actual_fees.transfer_fee.cents() >= 0,
        "过户费照付（费率 0.01‰，小成交额四舍五入后可为 0 分）：{confirmation:?}"
    );
}

#[test]
fn flat_and_a_share_modes_keep_charging_formal_stamp_tax_on_sells() {
    let mut flat = tax_mode_setup_with_rate(CashDividendTaxMode::FlatWithholding, 1000);
    flat.ticks_per_day = 4;
    flat.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    let mut flat_session = GameSession::new(flat, 42).unwrap();
    let flat_confirmation = sell_from_player_today(&mut flat_session, 5);
    assert!(
        flat_confirmation.actual_fees.stamp_tax.cents() > 0,
        "简税模式不免印花税：{flat_confirmation:?}"
    );

    let mut a_share = tax_mode_setup(CashDividendTaxMode::AShareIndividual);
    a_share.ticks_per_day = 4;
    a_share.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    let mut a_share_session = GameSession::new(a_share, 42).unwrap();
    let a_share_confirmation = sell_from_player_today(&mut a_share_session, 5);
    assert!(
        a_share_confirmation.actual_fees.stamp_tax.cents() > 0,
        "大 A 方式保持正式印花税：{a_share_confirmation:?}"
    );
}

#[test]
fn a_share_mode_round_trips_through_save_and_restores_identical_semantics() {
    let (session, _stock) =
        session_with_registry_without_retail_holder(CashDividendTaxMode::AShareIndividual);
    let save = session.save().unwrap();
    assert_eq!(
        save.setup.dividend_tax_mode,
        CashDividendTaxMode::AShareIndividual
    );
    assert_eq!(save.setup.flat_withholding_bp, None);
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        restored
            .account_dividend_tax_status(AccountId(0))
            .unwrap()
            .stocks,
        session
            .account_dividend_tax_status(AccountId(0))
            .unwrap()
            .stocks,
        "恢复后个人税账语义不变"
    );
    let resaved = restored.save().unwrap();
    assert_eq!(
        serde_json::to_value(&save.setup).unwrap(),
        serde_json::to_value(&resaved.setup).unwrap()
    );


}
