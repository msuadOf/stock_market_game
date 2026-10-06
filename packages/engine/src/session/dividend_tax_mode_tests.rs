//! 新局默认股息税与开局税务模式的 Session 级测试（2026-10-06 产品决策）。
//!
//! 覆盖：开局税务模式（默认=大 A 个人差别化；可选=不扣税）、装配期自动为个人身份
//! 账户开税账、机构/游资保持 TreatmentNotConfigured、税务模式严格持久化进存档契约
//! （新字段、旧档显式拒绝）与恢复后语义不变。

use super::corporate_actions::{AccountStockDividendTaxStatus, TaxpayerIdentity};
use super::*;
use crate::account::Position;
use crate::company::cash_dividend_tax::{CashDividendTaxMode, DividendTaxProfile};
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};

fn tax_mode_setup(mode: CashDividendTaxMode) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.retail_count = 1;
    setup.dividend_tax_mode = mode;
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
        mode,
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
        mode,
        vec![
            (AccountId(0), "player-lot", 5_u64),
            (AccountId(2), "institution-lot", 3),
        ],
    )
}

fn session_with_registry_and_holders(
    mode: CashDividendTaxMode,
    holders: Vec<(AccountId, &'static str, u64)>,
) -> (GameSession, StockCode) {
    let mut session = GameSession::new(tax_mode_setup(mode), 42).unwrap();
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

#[test]
fn default_mode_opens_individual_public_market_books_for_personal_holders_only() {
    let (session, stock) = session_with_registry(CashDividendTaxMode::IndividualPublicMarket);
    let books = &session.state.corporate_actions.dividend_tax_books;
    let mut configured: Vec<(AccountId, StockCode)> = books
        .iter()
        .map(|book| (book.account(), book.stock().clone()))
        .collect();
    configured.sort();
    assert_eq!(
        configured,
        vec![(AccountId(0), stock.clone()), (AccountId(1), stock.clone())],
        "默认模式只为个人身份账户（玩家+散户 NPC）自动开税账"
    );
}

#[test]
fn exempt_mode_opens_no_tax_books_at_registry_assembly() {
    let (session, _stock) = session_with_registry(CashDividendTaxMode::Exempt);
    assert!(
        session
            .state
            .corporate_actions
            .dividend_tax_books
            .is_empty(),
        "不扣税模式在装配期不配置任何税账"
    );
}

#[test]
fn explicit_assembly_time_configuration_still_works_in_exempt_mode() {
    let (mut session, stock) = session_with_registry(CashDividendTaxMode::Exempt);
    session
        .configure_cash_dividend_tax_book(
            AccountId(0),
            stock.clone(),
            DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap();
    assert_eq!(
        session.state.corporate_actions.dividend_tax_books.len(),
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
    let (session, stock) = session_with_registry(CashDividendTaxMode::IndividualPublicMarket);
    let player = session.account_dividend_tax_status(AccountId(0)).unwrap();
    assert_eq!(player.mode, CashDividendTaxMode::IndividualPublicMarket);
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

    let (exempt_session, exempt_stock) = session_with_registry(CashDividendTaxMode::Exempt);
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
    let (session, _stock) = session_with_registry(CashDividendTaxMode::IndividualPublicMarket);
    let error = session
        .account_dividend_tax_status(AccountId(999))
        .unwrap_err();
    assert!(
        error.to_string().contains("999"),
        "未知账户错误须携带账户标识：{error}"
    );
}

#[test]
fn tax_mode_round_trips_through_save_and_restores_identical_semantics() {
    let (session, _stock) =
        session_with_registry_without_retail_holder(CashDividendTaxMode::IndividualPublicMarket);
    let save = session.save().unwrap();
    assert_eq!(
        save.setup.dividend_tax_mode,
        CashDividendTaxMode::IndividualPublicMarket
    );
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

#[test]
fn save_without_tax_mode_field_is_rejected_explicitly() {
    let (session, _stock) =
        session_with_registry_without_retail_holder(CashDividendTaxMode::Exempt);
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
