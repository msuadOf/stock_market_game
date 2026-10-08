//! 玩家公司行为提案 Session 级测试（N2b，2026-10-08 用户决策「持仓即可、直接生效」）。
//!
//! 覆盖：六类行为（现金分红/送转/配股/增发/回购/拆股）玩家提案全链路（分红、
//! 送转、拆股走到真实入账，配股/增发/回购到 approve 受理）、无持仓显式拒绝、
//! 制度拒绝（可分配利润不足等）错误显式并携带 F 批四分类、新局开关关闭显式
//! 拒绝（本局未启用）、名册外持仓也算持有、偏好局内编辑下一周期评估生效且
//! 不回滚同周期已产生提案、提案与偏好编辑后的 restore 深等。

use super::*;
use crate::account::Position;
use crate::company::CompanyErrorClass;
use crate::company::config::CompanySystemConfig;
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::company::simple::preferences::SimpleCompanyPreferences;
use crate::money::Money;

use super::player_proposals::{PlayerCompanyProposal, PlayerProposalError, PlayerProposalKind};

fn d(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

/// 本组基线 setup：不扣税模式（与偏好组同口径，无个人税事实干扰）、短交易日、
/// 2030-01-20 开局；机制开关由用例自行覆盖。
fn base_setup() -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.dividend_tax_mode = crate::company::cash_dividend_tax::CashDividendTaxMode::Exempt;
    setup.flat_withholding_bp = None;
    setup.config.stamp_tax_rate = 0.0;
    setup.start_date = d("2030-01-20");
    setup.ticks_per_day = 2;
    setup
}

/// 装配带完整股东名册的会话：玩家持有 `player_qty` 股（同时进名册与账户持仓）、
/// 具名外部股东 1 股、其余为库藏股；注册资本法定事实按 `registered_capital_cents`
/// 显式绑定。`in_registry = false` 时玩家持仓不进名册（名册外持仓口径用例）。
fn session_with_registry(
    setup: SessionSetup,
    player_qty: u64,
    registered_capital_cents: i128,
    in_registry: bool,
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
            "player-proposal fixture legal fact".into(),
        )
        .unwrap();
    if player_qty > 0 && in_registry {
        session
            .state
            .accounts
            .get_mut(&AccountId(0))
            .unwrap()
            .fixture_insert_position(
                stock.clone(),
                Position::from_restored_parts(player_qty.try_into().unwrap(), 0, 1_000, 0),
            );
    }
    let lot = |id: &str, qty: u64| ShareLot {
        id: id.into(),
        qty,
        acquired_on: session.civil_date(),
        source: AcquisitionSource::InitialAllocation {
            evidence: "player-proposal fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    };
    let mut holdings = vec![ShareHolding {
        holder: HolderId::External("external-holder".into()),
        lots: vec![lot("external-lot", 1)],
    }];
    let player_registry_qty = if in_registry { player_qty } else { 0 };
    if player_registry_qty > 0 {
        holdings.insert(
            0,
            ShareHolding {
                holder: HolderId::Account(AccountId(0)),
                lots: vec![lot("player-lot", player_registry_qty)],
            },
        );
    }
    holdings.push(ShareHolding {
        holder: HolderId::IssuerTreasury,
        lots: vec![lot("treasury-lot", total_shares - player_registry_qty - 1)],
    });
    let registry = ShareRegistry::new(
        stock.clone(),
        issuer.clone(),
        total_shares,
        session.civil_date(),
        holdings,
    )
    .unwrap();
    session.configure_share_registry(registry).unwrap();
    if player_qty > 0 && !in_registry {
        // 名册外持仓口径：装配校验（账户持仓 ↔ 名册勾稽）在 configure 时执行，
        // 此后注入持仓模拟「二级市场净买入后、名册日终快照更新前」的瞬态。
        session
            .state
            .accounts
            .get_mut(&AccountId(0))
            .unwrap()
            .fixture_insert_position(
                stock.clone(),
                Position::from_restored_parts(player_qty.try_into().unwrap(), 0, 1_000, 0),
            );
    }
    (session, issuer, stock)
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

/// 推进到 `target`（不含）为止：每个自然日完整步进并日结。
fn advance_to(session: &mut GameSession, target: CivilDate) {
    while session.civil_date() < target {
        run_day(session);
    }
}

fn cash_dividend_proposal(company: &CompanyId, gross_cents: i64) -> PlayerCompanyProposal {
    PlayerCompanyProposal::CashDividend {
        company: company.clone(),
        gross_per_share: Money::from_cents(gross_cents),
    }
}

/// 配股／增发与回购机制开关全开的 setup（默认基线 + 双开关）。
fn mechanisms_setup() -> SessionSetup {
    let mut setup = base_setup();
    setup.rights_offering_enabled = true;
    setup.issuer_repurchase_enabled = true;
    setup
}

#[test]
fn player_cash_dividend_proposal_full_chain_pays_player() {
    // 玩家 5 股在册（另有外部股东 1 股）：提案每股 1 分，总额 6 分；受理后走
    // 既有 announce→登记→除息→派发状态机，玩家按登记持股真实税前到账。
    let (mut session, issuer, stock) = session_with_registry(base_setup(), 5, 10_000_000, true);
    advance_to(&mut session, d("2030-01-28"));
    let player_cash_before = session.account(AccountId(0)).unwrap().cash();
    let receipt = session
        .propose_company_action(AccountId(0), cash_dividend_proposal(&issuer, 1))
        .expect("有持仓玩家的现金分红提案应被受理");
    assert_eq!(receipt.kind, PlayerProposalKind::CashDividend);
    assert_eq!(receipt.approved_on, d("2030-01-28"));
    assert_eq!(receipt.announced_on, d("2030-01-28"));
    assert_eq!(
        session.state.corporate_actions.dividends.len(),
        1,
        "受理后应恰好建立一本分红账簿"
    );
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    assert_eq!(plan.plan_id, receipt.identity, "回执身份即受理的 plan_id");
    assert_eq!(plan.gross_per_share, Money::from_cents(1));
    // 日程推导与偏好自动提案同口径：登记日为公告日后首个交易日，除息=派发日
    // 为登记日次一交易日。
    let calendar = session.state.civil_clock.calendar().clone();
    let exchange = crate::calendar::CalendarExchange::Sse;
    let registered_on = calendar
        .next_trading_day(exchange, d("2030-01-28"))
        .unwrap();
    assert_eq!(plan.registered_on, registered_on);
    assert_eq!(
        plan.ex_dividend_on,
        calendar.next_trading_day(exchange, registered_on).unwrap()
    );
    // 全链路推进过派发日：状态机到 Paid，玩家真实到账 5 分（不扣税模式）。
    advance_to(&mut session, plan.payable_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.dividends[0].status(),
        &crate::company::cash_dividend::CashDividendStatus::Paid
    );
    assert_eq!(
        session.account(AccountId(0)).unwrap().cash(),
        player_cash_before.add(Money::from_cents(5)).unwrap(),
        "玩家按登记持股 5 股真实税前到账"
    );
    assert_eq!(stock.0, "600888");
}

#[test]
fn player_stock_distribution_proposal_full_chain_credits_shares() {
    // 10 送 10：玩家 5 股 + 外部 1 股 = 6 股基数，获批新增 6 股；R+1 非交易
    // 过户入账后玩家持仓翻倍、发行股数守恒演进。
    let (mut session, issuer, stock) = session_with_registry(base_setup(), 5, 10_000_000, true);
    advance_to(&mut session, d("2030-01-28"));
    let receipt = session
        .propose_company_action(
            AccountId(0),
            PlayerCompanyProposal::StockDistribution {
                company: issuer.clone(),
                kind: crate::company::stock_distribution::StockDistributionKind::BonusShares,
                shares_per_existing_share_micros: 1_000_000,
            },
        )
        .expect("有持仓玩家的送转提案应被受理");
    assert_eq!(receipt.kind, PlayerProposalKind::StockDistribution);
    assert_eq!(session.state.corporate_actions.stock_distributions.len(), 1);
    let plan = session.state.corporate_actions.stock_distributions[0]
        .plan()
        .clone();
    assert_eq!(plan.event_id, receipt.identity);
    assert_eq!(
        plan.approved_total_new_shares, 6,
        "6 股基数 10 送 10 = 6 股"
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
}

#[test]
fn player_share_split_proposal_full_chain_doubles_position() {
    // 1 拆 2：面值 2 分（注册资本 20,000,000 分 ÷ 10,000,000 股）整除缩小为
    // 1 分；R+1 重新计值入账后玩家持仓与总股本翻倍、注册资本不变。
    let (mut session, issuer, stock) = session_with_registry(base_setup(), 5, 20_000_000, true);
    advance_to(&mut session, d("2030-01-28"));
    let receipt = session
        .propose_company_action(
            AccountId(0),
            PlayerCompanyProposal::ShareSplit {
                company: issuer.clone(),
                direction: crate::company::share_split::ShareSplitDirection::Split,
                ratio: 2,
            },
        )
        .expect("有持仓玩家的拆股提案应被受理");
    assert_eq!(receipt.kind, PlayerProposalKind::ShareSplit);
    assert_eq!(session.state.corporate_actions.share_splits.len(), 1);
    let plan = session.state.corporate_actions.share_splits[0]
        .plan()
        .clone();
    assert_eq!(plan.event_id, receipt.identity);
    advance_to(&mut session, plan.ex_rights_on.next().unwrap());
    assert_eq!(
        session.state.corporate_actions.share_splits[0].settled_on(),
        Some(plan.ex_rights_on),
        "拆股应在除权日完成重新计值入账"
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
        "玩家 5 股 1 拆 2 后应为 10 股"
    );
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        20_000_000,
        "拆股后总股本翻倍"
    );
}

#[test]
fn player_rights_secondary_and_repurchase_proposals_are_accepted() {
    // 配股（面向全体股东 10 配 1）、增发（定向提案玩家承购）与回购（维护价值
    // 用途）各一例：均以有持仓玩家身份受理到对应 approve 入口建立账簿。
    let (mut session, issuer, _stock) =
        session_with_registry(mechanisms_setup(), 1_000, 10_000_000, true);
    advance_to(&mut session, d("2030-01-28"));

    let rights_receipt = session
        .propose_company_action(
            AccountId(0),
            PlayerCompanyProposal::RightsOffering {
                company: issuer.clone(),
                price_per_share: Money::from_cents(10),
                shares_per_existing_share_micros: 100_000,
                payment_days: 1,
            },
        )
        .expect("有持仓玩家的配股提案应被受理");
    assert_eq!(rights_receipt.kind, PlayerProposalKind::RightsOffering);
    assert_eq!(session.state.corporate_actions.rights_offerings.len(), 1);
    let rights_plan = session.state.corporate_actions.rights_offerings[0]
        .plan()
        .clone();
    assert_eq!(rights_plan.event_id, rights_receipt.identity);
    assert_eq!(
        rights_plan.registered_on,
        d("2030-01-29"),
        "R = 公告日后首个交易日"
    );
    assert_eq!(
        rights_plan.payment_deadline_on,
        d("2030-01-30"),
        "缴款期 1 个交易日：R+1 起至 L"
    );
    assert!(
        matches!(
            rights_plan.mode,
            crate::company::rights_offering::RightsOfferingMode::RightsToAllShareholders {
                shares_per_existing_share_micros: 100_000
            }
        ),
        "配股提案应构造面向全体股东模式"
    );

    // 定向增发与配股同日提案会推导出相同除权日（R 与缴款期同口径），既有
    // 碰撞预检按制度拒绝同日第二起——因此增发用例在独立会话中验证（受理与
    // 定向对象构造本身），碰撞拒绝语义已由既有机制覆盖。
    let (mut offering_session, offering_issuer, _offering_stock) =
        session_with_registry(mechanisms_setup(), 5, 10_000_000, true);
    advance_to(&mut offering_session, d("2030-01-28"));
    let offering_receipt = offering_session
        .propose_company_action(
            AccountId(0),
            PlayerCompanyProposal::SecondaryOffering {
                company: offering_issuer.clone(),
                price_per_share: Money::from_cents(10),
                shares: 100,
            },
        )
        .expect("有持仓玩家的定向增发提案应被受理");
    assert_eq!(offering_receipt.kind, PlayerProposalKind::SecondaryOffering);
    assert_eq!(
        offering_session
            .state
            .corporate_actions
            .rights_offerings
            .len(),
        1
    );
    let offering_plan = offering_session.state.corporate_actions.rights_offerings[0]
        .plan()
        .clone();
    assert_eq!(offering_plan.event_id, offering_receipt.identity);
    assert_eq!(
        offering_plan.registered_on,
        d("2030-01-29"),
        "登记日为公告日后首个交易日"
    );
    match &offering_plan.mode {
        crate::company::rights_offering::RightsOfferingMode::DirectedPlacement { targets } => {
            assert_eq!(targets.len(), 1, "增发提案定向到提案玩家本人");
            match &targets[0] {
                crate::company::rights_offering::DirectedPlacementTarget::NamedHolder {
                    holder,
                    shares,
                    ..
                } => {
                    assert_eq!(*holder, HolderId::Account(AccountId(0)));
                    assert_eq!(*shares, 100);
                }
                other => panic!("增发提案对象应为具名持有人，实际 {other:?}"),
            }
        }
        other => panic!("增发提案应构造定向模式，实际 {other:?}"),
    }

    // 回购：专户事实的 established_on 不得晚于名册最近结算日（M 批既有守卫，
    // approve 语义不变），因此本用例在名册结算当日（开局装配日）提案受理；
    // 盘中已跨结算日的提案会被该守卫显式拒绝（登记为已知边界，见台账）。
    let (mut repurchase_session, repurchase_issuer, _repurchase_stock) =
        session_with_registry(mechanisms_setup(), 1_000, 10_000_000, true);
    let repurchase_receipt = repurchase_session
        .propose_company_action(
            AccountId(0),
            PlayerCompanyProposal::IssuerRepurchase {
                company: repurchase_issuer.clone(),
                price_cap_per_share: Money::from_cents(900),
                total_budget: Money::from_cents(90_000),
                max_shares: 100,
                window_trading_days: 5,
                purpose: crate::company::issuer_repurchase::RepurchasePurpose::ValueMaintenance,
            },
        )
        .expect("有持仓玩家的回购提案应被受理");
    assert_eq!(
        repurchase_receipt.kind,
        PlayerProposalKind::IssuerRepurchase
    );
    assert_eq!(
        repurchase_session
            .state
            .corporate_actions
            .issuer_repurchases
            .len(),
        1,
        "回购受理即建立账簿并合成资金入专用账户"
    );
    let repurchase_plan = repurchase_session
        .state
        .corporate_actions
        .issuer_repurchases[0]
        .plan()
        .clone();
    assert_eq!(repurchase_plan.event_id, repurchase_receipt.identity);
    assert_eq!(
        repurchase_plan.window_start_on,
        d("2030-01-21"),
        "窗口起点为公告日（开局装配日）后首个交易日"
    );
}

#[test]
fn proposal_without_any_holding_is_explicitly_rejected() {
    // 无持仓拒绝：账户对该发行人上市证券持仓为 0（任意数量 > 0 才可发起）；
    // 不进入任何制度校验，也不产生账簿。
    let (mut session, issuer, _stock) = session_with_registry(base_setup(), 0, 10_000_000, true);
    advance_to(&mut session, d("2030-01-28"));
    for proposal in [
        cash_dividend_proposal(&issuer, 1),
        PlayerCompanyProposal::StockDistribution {
            company: issuer.clone(),
            kind: crate::company::stock_distribution::StockDistributionKind::BonusShares,
            shares_per_existing_share_micros: 1_000_000,
        },
        PlayerCompanyProposal::ShareSplit {
            company: issuer.clone(),
            direction: crate::company::share_split::ShareSplitDirection::Split,
            ratio: 2,
        },
    ] {
        let error = session
            .propose_company_action(AccountId(0), proposal)
            .expect_err("无持仓玩家的提案必须被显式拒绝");
        assert!(
            matches!(error, PlayerProposalError::NoHolding { .. }),
            "应显式返回无持仓拒绝，实际 {error:?}"
        );
    }
    assert!(
        session.state.corporate_actions.dividends.is_empty()
            && session
                .state
                .corporate_actions
                .stock_distributions
                .is_empty()
            && session.state.corporate_actions.share_splits.is_empty(),
        "被拒提案不得留下任何账簿"
    );
}

#[test]
fn out_of_registry_holding_still_permits_proposal() {
    // 前置判定用账户持仓：名册外持仓也算持有（二级市场净买入后、名册日终
    // 快照更新前的瞬态——该瞬态无法跨越日终勾稽，故同日提案）。提案照常
    // 进入既有制度校验并按名册事实受理。
    let (mut session, issuer, _stock) = session_with_registry(base_setup(), 5, 10_000_000, false);
    let receipt = session
        .propose_company_action(AccountId(0), cash_dividend_proposal(&issuer, 1))
        .expect("名册外持仓（账户持仓 > 0）也应允许发起提案");
    assert_eq!(receipt.kind, PlayerProposalKind::CashDividend);
    assert_eq!(session.state.corporate_actions.dividends.len(), 1);
}

#[test]
fn institutional_rejection_carries_explicit_detail_and_classification() {
    // 制度拒绝：每股红利远超可分配利润时由既有 approve_cash_dividend 校验拒绝，
    // 错误原样上抛并附加 F 批四分类（业务条件拒绝）。
    let (mut session, issuer, _stock) = session_with_registry(base_setup(), 5, 10_000_000, true);
    advance_to(&mut session, d("2030-01-28"));
    let error = session
        .propose_company_action(AccountId(0), cash_dividend_proposal(&issuer, 500_000_000))
        .expect_err("超过可分配利润的提案必须被制度拒绝");
    match error {
        PlayerProposalError::Institutional {
            kind,
            detail,
            class,
        } => {
            assert_eq!(kind, PlayerProposalKind::CashDividend);
            assert_eq!(class, CompanyErrorClass::BusinessCondition);
            assert!(
                detail.contains("可分配"),
                "拒绝原因应原样指向可分配利润制度校验：{detail}"
            );
        }
        other => panic!("应返回制度拒绝，实际 {other:?}"),
    }
    assert!(
        session.state.corporate_actions.dividends.is_empty(),
        "被拒提案不得建立账簿"
    );
}

#[test]
fn disabled_mechanism_switch_rejects_rights_and_repurchase_proposals() {
    // 开关关闭的行为显式拒绝（本局未启用），分类为未支持操作。
    let (mut session, issuer, _stock) = session_with_registry(base_setup(), 5, 10_000_000, true);
    advance_to(&mut session, d("2030-01-28"));
    let rights_error = session
        .propose_company_action(
            AccountId(0),
            PlayerCompanyProposal::RightsOffering {
                company: issuer.clone(),
                price_per_share: Money::from_cents(10),
                shares_per_existing_share_micros: 100_000,
                payment_days: 1,
            },
        )
        .expect_err("本局未启用配股／增发时提案必须显式拒绝");
    match rights_error {
        PlayerProposalError::Institutional {
            kind,
            detail,
            class,
        } => {
            assert_eq!(kind, PlayerProposalKind::RightsOffering);
            assert_eq!(class, CompanyErrorClass::UnsupportedOperation);
            assert!(
                detail.contains("本局未启用"),
                "拒绝原因应显式说明本局未启用：{detail}"
            );
        }
        other => panic!("应返回制度拒绝，实际 {other:?}"),
    }
    let repurchase_error = session
        .propose_company_action(
            AccountId(0),
            PlayerCompanyProposal::IssuerRepurchase {
                company: issuer.clone(),
                price_cap_per_share: Money::from_cents(900),
                total_budget: Money::from_cents(90_000),
                max_shares: 100,
                window_trading_days: 5,
                purpose: crate::company::issuer_repurchase::RepurchasePurpose::ValueMaintenance,
            },
        )
        .expect_err("本局未启用回购时提案必须显式拒绝");
    match repurchase_error {
        PlayerProposalError::Institutional {
            kind,
            detail,
            class,
        } => {
            assert_eq!(kind, PlayerProposalKind::IssuerRepurchase);
            assert_eq!(class, CompanyErrorClass::UnsupportedOperation);
            assert!(detail.contains("本局未启用"));
        }
        other => panic!("应返回制度拒绝，实际 {other:?}"),
    }
    assert!(
        session.state.corporate_actions.rights_offerings.is_empty()
            && session
                .state
                .corporate_actions
                .issuer_repurchases
                .is_empty(),
        "被拒提案不得建立账簿"
    );
}

#[test]
fn invalid_proposal_parameters_are_rejected_as_invalid_input() {
    // 参数域违反（非正金额、拆股比例 < 2）在构造期显式拒绝，分类为非法输入。
    let (mut session, issuer, _stock) = session_with_registry(base_setup(), 5, 10_000_000, true);
    advance_to(&mut session, d("2030-01-28"));
    let zero_dividend = session
        .propose_company_action(AccountId(0), cash_dividend_proposal(&issuer, 0))
        .expect_err("每股红利为 0 必须显式拒绝");
    match zero_dividend {
        PlayerProposalError::Institutional { class, detail, .. } => {
            assert_eq!(class, CompanyErrorClass::InvalidInput);
            assert!(
                detail.contains("正数"),
                "拒绝原因应说明金额必须为正：{detail}"
            );
        }
        other => panic!("应返回构造期拒绝，实际 {other:?}"),
    }
    let bad_ratio = session
        .propose_company_action(
            AccountId(0),
            PlayerCompanyProposal::ShareSplit {
                company: issuer.clone(),
                direction: crate::company::share_split::ShareSplitDirection::Split,
                ratio: 1,
            },
        )
        .expect_err("拆股比例 1 必须显式拒绝");
    match bad_ratio {
        PlayerProposalError::Institutional { class, detail, .. } => {
            assert_eq!(class, CompanyErrorClass::InvalidInput);
            assert!(detail.contains("2"), "拒绝原因应说明比例下限：{detail}");
        }
        other => panic!("应返回构造期拒绝，实际 {other:?}"),
    }
}

#[test]
fn set_simple_preferences_takes_effect_next_cycle_without_rollback() {
    // 三个口径：①周期末日评估前改为「不自动提案」→ 本周期不评估；②已产生的
    // 自动提案不因偏好修改回滚；③周期中途从无到有开启 → 下一周期评估生效。
    let cash_preferences = SimpleCompanyPreferences {
        cash_dividend: Some(
            crate::company::simple::preferences::SimpleCashDividendPreference {
                target_payout_bp: 3_000,
                min_distributable_profit: Money::from_cents(1),
                cycles_between_proposals: 1,
            },
        ),
        stock_distribution: None,
    };

    // ① 评估前关闭：本周期不产生自动提案。
    let mut setup = base_setup();
    if let CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.companies[0].preferences = cash_preferences.clone();
    }
    let (mut session, issuer, _stock) = session_with_registry(setup, 5, 10_000_000, true);
    advance_to(&mut session, d("2030-01-30"));
    session
        .set_simple_preferences(&issuer, SimpleCompanyPreferences::none())
        .expect("偏好局内编辑应被受理");
    advance_to(&mut session, d("2030-02-01"));
    assert!(
        session.state.corporate_actions.dividends.is_empty(),
        "评估前关闭偏好后本周期不得自动提案"
    );

    // ② 已产生提案不回滚：开启偏好走到评估产生方案后再关闭，方案仍在。
    let mut setup = base_setup();
    if let CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.companies[0].preferences = cash_preferences.clone();
    }
    let (mut session, issuer, _stock) = session_with_registry(setup, 5, 10_000_000, true);
    advance_to(&mut session, d("2030-02-01"));
    assert_eq!(
        session.state.corporate_actions.dividends.len(),
        1,
        "偏好开启的首个周期应产生自动提案"
    );
    session
        .set_simple_preferences(&issuer, SimpleCompanyPreferences::none())
        .unwrap();
    assert_eq!(
        session.state.corporate_actions.dividends.len(),
        1,
        "偏好修改不得回滚同周期已产生的提案"
    );

    // ③ 中途开启：下一周期评估生效。
    let (mut session, issuer, _stock) = session_with_registry(base_setup(), 5, 10_000_000, true);
    advance_to(&mut session, d("2030-01-30"));
    assert!(
        session.state.corporate_actions.dividends.is_empty(),
        "未配置偏好不自动提案"
    );
    session
        .set_simple_preferences(&issuer, cash_preferences.clone())
        .unwrap();
    advance_to(&mut session, d("2030-02-01"));
    assert_eq!(
        session.state.corporate_actions.dividends.len(),
        1,
        "中途开启的偏好应在下一周期评估生效"
    );
    let plan = session.state.corporate_actions.dividends[0].plan().clone();
    assert_eq!(
        plan.plan_id, "simple-preference:C-600888:dividend:2030-01-31",
        "生效后的自动提案沿用既有确定性 id"
    );
}

#[test]
fn set_simple_preferences_validates_domain_and_unknown_company() {
    let (mut session, issuer, _stock) = session_with_registry(base_setup(), 5, 10_000_000, true);
    let invalid = SimpleCompanyPreferences {
        cash_dividend: Some(
            crate::company::simple::preferences::SimpleCashDividendPreference {
                target_payout_bp: 0,
                min_distributable_profit: Money::from_cents(1),
                cycles_between_proposals: 1,
            },
        ),
        stock_distribution: None,
    };
    let error = session
        .set_simple_preferences(&issuer, invalid)
        .expect_err("0bp 派息比例必须在局内编辑时同样被拒绝");
    assert!(error.to_string().contains("bp"));
    let error = session
        .set_simple_preferences(
            &CompanyId("C-unknown".into()),
            SimpleCompanyPreferences::none(),
        )
        .expect_err("未知公司的偏好编辑必须显式拒绝");
    assert!(error.to_string().contains("未知公司"));
}

#[test]
fn restore_deep_equal_after_player_proposal_and_preference_edit() {
    // 提案受理 + 偏好局内编辑后：保存→恢复→再保存深度相等；恢复保留受理的
    // 方案账簿与编辑后的偏好（两侧配置一致，不触发恢复勾稽拒绝）。
    let (mut session, issuer, _stock) = session_with_registry(base_setup(), 5, 10_000_000, true);
    advance_to(&mut session, d("2030-01-28"));
    session
        .propose_company_action(AccountId(0), cash_dividend_proposal(&issuer, 1))
        .unwrap();
    session
        .set_simple_preferences(
            &issuer,
            SimpleCompanyPreferences {
                cash_dividend: Some(
                    crate::company::simple::preferences::SimpleCashDividendPreference {
                        target_payout_bp: 2_000,
                        min_distributable_profit: Money::from_cents(500),
                        cycles_between_proposals: 2,
                    },
                ),
                stock_distribution: None,
            },
        )
        .unwrap();
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        restored.state.corporate_actions.dividends.len(),
        1,
        "恢复后受理的提案账簿仍在"
    );
    assert_eq!(
        restored
            .state
            .company_system
            .simple_preferences(&issuer)
            .unwrap(),
        session
            .state
            .company_system
            .simple_preferences(&issuer)
            .unwrap(),
        "恢复后偏好为编辑后的值"
    );
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(&save).unwrap(),
        "恢复→再保存必须与原保存深度相等"
    );
}
