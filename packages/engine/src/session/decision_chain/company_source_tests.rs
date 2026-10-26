// SRC-A 的真实 consumer 选源与权限边界夹具；由既有 chain 测试模块引入。
include!("../../../test-support/simple_company.rs");

/// 同 CompanyId 的 Simple 与 legacy 故意使用不同 kind 与股本，防止选错源假绿。
fn divergent_company_source_session(kind: crate::company::CompanyKind) -> GameSession {
    divergent_company_source_session_with_counts(kind, 1)
}

fn divergent_company_source_session_with_counts(
    kind: crate::company::CompanyKind,
    inst_count: u32,
) -> GameSession {
    let mut setup = probe_session().state.setup.clone();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = inst_count;
    let mut config = simple_company_fixture!(crate; ["000812"]);
    let crate::company::config::CompanySystemConfig::Simple(simple) = &mut config else {
        unreachable!("fixture 必须是 Simple");
    };
    simple.companies[0].kind = kind;
    simple.companies[0].finance.opening_lines[0].account = crate::accounting::LedgerAccountId(
        if kind == crate::company::CompanyKind::Bank {
            "1003"
        } else {
            "1002"
        }
        .to_owned(),
    );
    simple.prehistory_periods = 1;
    setup.company_system = Some(config);
    setup.dividend_tax_mode =
        Some(crate::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    let mut session = GameSession::new(setup, 42).unwrap();
    // 仅制造初始 metadata 分歧；不把本夹具视为真实资产公司行为或动态刷新验收。
    let system = std::sync::Arc::make_mut(session.state.company_system.as_mut().unwrap());
    system
        .issuers
        .record_share_issuance(&crate::company::CompanyId(0), u64::from(u32::MAX) + 777)
        .unwrap();
    session
}

/// 原始个人态在构造 native 世界前定版；不依赖只向组 0 发送的 ForceDue fixture。
fn prepare_company_source_personal_inputs(
    session: &mut GameSession,
    observe_at_start: bool,
) -> Vec<AccountId> {
    use crate::strategy::{AnalysisProfile, AnalysisWeights, FundamentalMethod};
    let stock = session.stock_id("000812").unwrap();
    let now = session.chain_observation_instant();
    let ids: Vec<_> = session.state.belief_participants.keys().copied().collect();
    for id in &ids {
        let mut personal = PlanPersonalState::take(session, *id);
        let profile = personal.belief.profile().clone();
        let mut rng = SplitMix64::new(17 + id.0);
        personal.belief = BeliefBook::new(
            *id,
            profile,
            AnalysisProfile::new(
                AnalysisWeights::new(10_000, 0, 0, 0, 0).unwrap(),
                Some(FundamentalMethod::EarningsMultiple),
            )
            .unwrap(),
            &mut rng,
        );
        personal.information = crate::information::NpcInformationState::new(*id);
        personal.attention.base_probability = 1.0;
        personal.attention.next_attention_candidate_tick =
            if observe_at_start { 0 } else { u64::MAX };
        personal.attention.information_cadence = crate::session::NpcInformationCadence::Immediate;
        personal.attention.next_information_check = now;
        personal.watchlist.record_attention(&stock, 0, 0).unwrap();
        personal.install(session, *id);
    }
    ids
}

#[test]
fn selected_company_source_strategy_review_cap_reads_current_issuer_before_u32_boundary() {
    let session = divergent_company_source_session(crate::company::CompanyKind::Bank);
    let stock = session.stock_id("000812").unwrap();
    let owner = session
        .state
        .accounts
        .iter()
        .find_map(|(id, account)| (account.kind() == AccountKind::Inst).then_some(*id))
        .unwrap();
    let host = SessionReviewHost {
        session: &session,
        account: owner,
    };
    assert_eq!(
        host.max_holdable_shares(&stock),
        u64::from(u32::MAX),
        "策略边界保留 u32 上限，但输入必须来自当前 issuer 股本；旧 setup 仅100万股"
    );
}

fn assert_serial_company_notice_source(
    mut session: GameSession,
    method: crate::strategy::FundamentalMethod,
) {
    let stock = session.stock_id("000812").unwrap();
    let ids = prepare_company_source_personal_inputs(&mut session, false);
    let now = session.chain_observation_instant();
    assert!(
        !session.state.runtime_information_authority,
        "本用例明确执行串行/WASM 生产接缝"
    );
    session.deliver_public_information(now).unwrap();
    for id in ids {
        let participant = &session.state.belief_participants[&id];
        assert!(
            participant.information().acquired_count() > 0,
            "必须实际交付本人公开报告"
        );
        assert_eq!(
            participant
                .belief()
                .entry(&stock)
                .expect("真实投递产生本人条目")
                .method,
            Some(method)
        );
    }
}

#[test]
fn selected_company_source_serial_notice_uses_simple_kind() {
    assert_serial_company_notice_source(
        divergent_company_source_session(crate::company::CompanyKind::Bank),
        crate::strategy::FundamentalMethod::EquityRoe,
    );
}

#[test]
fn selected_company_source_serial_notice_none_keeps_legacy_kind() {
    assert_serial_company_notice_source(
        probe_session(),
        crate::strategy::FundamentalMethod::EarningsMultiple,
    );
}

fn assert_native_company_notice_source(
    mut session: GameSession,
    method: crate::strategy::FundamentalMethod,
) {
    let stock = session.stock_id("000812").unwrap();
    let ids = prepare_company_source_personal_inputs(&mut session, false);
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    driver.step_frame_equivalent(&mut session).unwrap();
    assert!(
        driver
            .collect_belief()
            .unwrap()
            .iter()
            .flat_map(|(_, rows)| rows)
            .flatten()
            .all(|row| row.information.acquired_count() == 0),
        "首拍只准备真实市场观察，不经 root 获知材料"
    );
    assert_eq!(
        driver.deliver_information_for_test(&mut session).unwrap(),
        ids.len()
    );
    let actual = driver.collect_group_belief_for_test().unwrap();
    let mut owners = BTreeSet::new();
    for (_, rows) in actual {
        for (_, row) in rows {
            assert!(
                row.information.acquired_count() > 0,
                "实际 native notice 必须获知公开材料"
            );
            assert_eq!(
                row.belief
                    .entry(&stock)
                    .expect("组内投递产生本人条目")
                    .method,
                Some(method)
            );
            owners.insert(row.information.owner());
        }
    }
    assert_eq!(
        owners,
        ids.into_iter().collect(),
        "检查组端真实个人态而非尚未回写的 session 镜像"
    );
}

#[test]
fn selected_company_source_native_single_group_notice_uses_simple_kind() {
    assert_native_company_notice_source(
        divergent_company_source_session(crate::company::CompanyKind::Bank),
        crate::strategy::FundamentalMethod::EquityRoe,
    );
}

#[test]
fn selected_company_source_native_single_group_notice_none_keeps_legacy_kind() {
    assert_native_company_notice_source(
        probe_session(),
        crate::strategy::FundamentalMethod::EarningsMultiple,
    );
}

/// 独立旧 route 的身份门，不计入 SRC-A 来源红灯；取证与修复须单列。
#[test]
fn legacy_notice_three_group_global_local_route_keeps_personal_owner() {
    let mut setup = probe_session().state.setup.clone();
    setup.npcs.inst_count = 5;
    setup.npcs.retail_count = 1;
    let mut session = GameSession::new(setup, 42).unwrap();
    let ids = prepare_company_source_personal_inputs(&mut session, false);
    let mut driver =
        crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut session, 3).unwrap();
    assert_eq!(driver.id_map.len(), 7);
    assert_eq!(driver.accounts_per_group(), 3);
    driver.step_frame_equivalent(&mut session).unwrap();
    assert_eq!(
        driver.deliver_information_for_test(&mut session).unwrap(),
        ids.len(),
        "全局 attention 必须按 Owner 分组并转换 LocalIdx，不能广播成另一组本人"
    );
    let actual = driver.collect_group_belief_for_test().unwrap();
    let owners: BTreeSet<_> = actual
        .iter()
        .flat_map(|(_, rows)| rows)
        .map(|(_, row)| {
            assert!(row.information.acquired_count() > 0);
            row.information.owner()
        })
        .collect();
    assert_eq!(owners, ids.into_iter().collect());
}

#[test]
fn selected_company_source_root_parts_keep_independent_numeric_namespaces_and_typed_missing() {
    let company = crate::company::CompanyId(17);
    let stock = StockId(0);
    let shares = u64::from(u32::MAX) + 701;
    let facts = crate::company::CompanyReadFacts::new(
        [(stock, company)].into(),
        [(company, (crate::company::CompanyKind::Bank, shares))].into(),
    )
    .unwrap();
    let root = RootReadContext::from_parts(
        crate::session::AccountBook::default(),
        std::sync::Arc::new(facts),
        std::sync::Arc::new(crate::information::PublicLibrary::new()),
        PlanBook::default(),
        crate::CivilDate::from_iso("2030-01-07").unwrap(),
        0,
        0,
    );
    assert_eq!(root.issuer_of_stock(stock), Ok(company));
    assert_eq!(
        root.company_spec(&company).unwrap(),
        (crate::company::CompanyKind::Bank, shares)
    );
    assert_eq!(
        root.issuer_of_stock(StockId(17)),
        Err(crate::company::CompanyReadFactsError::MissingIssuer { stock: StockId(17) })
    );
}

#[test]
fn selected_company_source_exposure_uses_numeric_issuer_and_publication_time() {
    let company = crate::company::CompanyId(17);
    let stock = StockId(0);
    let facts = crate::company::CompanyReadFacts::new(
        [(stock, company)].into(),
        [(company, (crate::company::CompanyKind::Industrial, 100_000))].into(),
    )
    .unwrap();
    let date = crate::CivilDate::from_iso("2030-01-07").unwrap();
    let published = crate::calendar::CivilInstant::from_hms(date, 18, 0, 0).unwrap();
    let mut library = crate::information::PublicLibrary::new();
    library
        .publish_announcement(crate::information::AnnouncementRequest {
            company,
            occurred_on: date,
            published_at: published,
            content: crate::information::AnnouncementContent::Shock(
                crate::information::AnnouncedEvent {
                    kind: crate::company::ShockKind::ContractWon,
                    amplitude_bp: 100,
                    starts_on: date,
                    expires_on: date.next().unwrap(),
                },
            ),
        })
        .unwrap();
    let library = std::sync::Arc::new(library);
    assert!(
        exposed_stocks_shared(
            &library,
            &facts,
            crate::calendar::CivilInstant::from_hms(date, 17, 59, 59).unwrap()
        )
        .is_empty(),
        "尚未公布材料不能产生曝光"
    );
    assert_eq!(
        exposed_stocks_shared(&library, &facts, published),
        BTreeSet::from([stock]),
        "CompanyId17必须映射StockId0，不能按数值相等关联"
    );
}

#[test]
fn selected_company_source_root_uses_simple_kind_and_full_u64_shares() {
    let session = divergent_company_source_session(crate::company::CompanyKind::Bank);
    let stock = session.stock_id("000812").unwrap();
    let company = session
        .company_system()
        .unwrap()
        .issuers()
        .issuer_of(&StockCode("000812".to_owned()))
        .unwrap();
    let old = session.state.operations.company(&company).unwrap().spec();
    let expected = (
        crate::company::CompanyKind::Bank,
        1_000_000 + u64::from(u32::MAX) + 777,
    );
    assert_ne!((old.kind, old.issued_shares), expected);
    let root = RootReadContext::capture(&session).unwrap();
    assert_eq!(root.issuer_of_stock(stock), Ok(company));
    assert_eq!(root.company_spec(&company).unwrap(), expected);
}

#[test]
fn selected_company_source_retail_uses_simple_kind_and_full_u64_shares() {
    let session = divergent_company_source_session(crate::company::CompanyKind::Bank);
    let retail = session
        .state
        .accounts
        .iter()
        .find_map(|(id, account)| (account.kind() == AccountKind::Retail).then_some(*id))
        .unwrap();
    let view = session.read_view();
    let ctx = GameSession::retail_analysis_ctx_from_view(&view, retail);
    assert_eq!(
        ctx.company_spec(&crate::company::CompanyId(0)).unwrap(),
        (
            crate::company::CompanyKind::Bank,
            1_000_000 + u64::from(u32::MAX) + 777
        ),
    );
}

#[test]
fn selected_company_source_runtime_metadata_and_cap_use_current_u64_shares() {
    let session = divergent_company_source_session(crate::company::CompanyKind::Bank);
    let stock = session.stock_id("000812").unwrap();
    let company = crate::company::CompanyId(0);
    let issued_shares = 1_000_000 + u64::from(u32::MAX) + 777;
    assert_eq!(session.runtime_issuer_map().get(&stock), Some(&company));
    assert_eq!(
        session.runtime_company_specs().get(&company),
        Some(&(crate::company::CompanyKind::Bank, issued_shares))
    );
    assert_eq!(
        session.runtime_total_shares().get(&stock),
        Some(&issued_shares)
    );
}

#[test]
fn selected_company_source_insurance_root_uses_selected_kind() {
    let session = divergent_company_source_session(crate::company::CompanyKind::Insurance);
    let root = RootReadContext::capture(&session).unwrap();
    assert_eq!(
        root.company_spec(&crate::company::CompanyId(0)).unwrap(),
        (
            crate::company::CompanyKind::Insurance,
            1_000_000 + u64::from(u32::MAX) + 777
        )
    );
}

#[test]
fn selected_company_source_real_estate_retail_uses_selected_kind() {
    let session = divergent_company_source_session(crate::company::CompanyKind::RealEstate);
    let retail = session
        .state
        .accounts
        .iter()
        .find_map(|(id, account)| (account.kind() == AccountKind::Retail).then_some(*id))
        .unwrap();
    let view = session.read_view();
    let ctx = GameSession::retail_analysis_ctx_from_view(&view, retail);
    assert_eq!(
        ctx.company_spec(&crate::company::CompanyId(0)).unwrap(),
        (
            crate::company::CompanyKind::RealEstate,
            1_000_000 + u64::from(u32::MAX) + 777
        )
    );
}

#[test]
fn selected_company_source_private_reports_do_not_grant_acquisition_or_valuation() {
    let mut session = divergent_company_source_session(crate::company::CompanyKind::Bank);
    let stock = session.stock_id("000812").unwrap();
    let company = crate::company::CompanyId(0);
    assert!(!session
        .company_system()
        .unwrap()
        .closed_reports(&company)
        .unwrap()
        .is_empty());
    session.state.library = std::sync::Arc::new(crate::information::PublicLibrary::new());
    let account = session
        .state
        .accounts
        .iter()
        .find_map(|(id, account)| (account.kind() == AccountKind::Inst).then_some(*id))
        .unwrap();
    let root = RootReadContext::capture(&session).unwrap();
    let observation =
        DecisionChainObservation::capture_for_roots(&session, session.build_market_view()).unwrap();
    assert!(observation.exposed.is_empty());
    let personal = PlanPersonalState::take(&mut session, account);
    let (_, out, _, _) =
        InstitutionDecisionRoot::new(account, personal).observe(&root, &observation);
    assert_eq!(out.information.acquired_count(), 0);
    assert!(!out
        .belief
        .entry(&stock)
        .is_some_and(|entry| matches!(entry.valuation, ValuationOutcome::Available { .. })));
}

#[test]
fn selected_company_source_native_three_groups_tail_roots_use_simple() {
    use crate::strategy::FundamentalMethod;
    let mut session =
        divergent_company_source_session_with_counts(crate::company::CompanyKind::Bank, 5);
    // EchoOpenDerivation 只接受机构 root；此局用6机构+玩家保留原7名册/3组/尾组。
    let mut setup = session.state.setup.clone();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 6;
    session = GameSession::new(setup, 42).unwrap();
    std::sync::Arc::make_mut(session.state.company_system.as_mut().unwrap())
        .issuers
        .record_share_issuance(&crate::company::CompanyId(0), u64::from(u32::MAX) + 777)
        .unwrap();
    let stock = session.stock_id("000812").unwrap();
    let _ids = prepare_company_source_personal_inputs(&mut session, true);
    let mut driver =
        crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut session, 3).unwrap();
    assert_eq!(driver.id_map.len(), 7, "玩家、六个机构");
    assert_eq!(driver.accounts_per_group(), 3, "3/3/1 的尾组必须存在");
    driver.step_frame_equivalent(&mut session).unwrap();
    driver.install_market_views(&mut session).unwrap();
    driver.install_candles(&mut session).unwrap();
    let derived = driver.collect_open_derivations().unwrap();
    assert_eq!(derived.len(), 3);
    for (group, _, _, records) in &derived {
        assert!(!records.is_empty(), "组 {group} 必须真实执行机构 root");
        for record in records {
            let entry = record
                .personal_out
                .belief
                .entry(&stock)
                .expect("本人获知公开报告后形成条目");
            assert_eq!(
                entry.method,
                Some(FundamentalMethod::EquityRoe),
                "Bank 规则必须来自所选 Simple kind，不能用 legacy 工商方法"
            );
        }
    }
    let snapshots = driver.collect_belief().unwrap();
    assert_eq!(snapshots[2].1.len(), 1, "非零 roster offset 的尾组");
}

/// mixed 世界的真实 retail Start 读面独立取证，不让机构专用 Echo helper 误处理 retail。
#[test]
fn selected_company_source_native_three_groups_tail_retail_uses_simple() {
    use crate::strategy::FundamentalMethod;
    let mut session =
        divergent_company_source_session_with_counts(crate::company::CompanyKind::Bank, 5);
    let stock = session.stock_id("000812").unwrap();
    prepare_company_source_personal_inputs(&mut session, true);
    let mut driver =
        crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut session, 3).unwrap();
    assert_eq!(driver.id_map.len(), 7, "玩家、一个 retail、五个机构");
    assert_eq!(driver.accounts_per_group(), 3, "保留3/3/1尾组");
    driver.step_frame_equivalent(&mut session).unwrap();
    let snapshots = driver.collect_belief().unwrap();
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[2].1.len(), 1, "非零 roster offset 的尾组");
    let retail = driver
        .id_map
        .iter()
        .copied()
        .find(|id| session.state.accounts[id].kind() == AccountKind::Retail)
        .unwrap();
    let retail_state = snapshots
        .iter()
        .flat_map(|(_, rows)| rows)
        .flatten()
        .find(|row| row.information.owner() == retail)
        .unwrap();
    assert_eq!(
        retail_state
            .belief
            .entry(&stock)
            .expect("真实 retail 观察形成条目")
            .method,
        Some(FundamentalMethod::EquityRoe)
    );
}

#[test]
fn selected_company_source_legacy_missing_operation_is_explicit_error() {
    let mut session = probe_session();
    let company = session
        .state
        .issuer_of_stock(session.stock_id("000812").unwrap())
        .unwrap();
    std::sync::Arc::make_mut(&mut session.state.operations)
        .companies
        .remove(&company)
        .unwrap();
    assert_eq!(
        session.state.company_read_facts(),
        Err(
            crate::session::company_facts::CompanyFactsSourceError::MissingLegacyOperation {
                company
            }
        )
    );
}

#[test]
fn selected_company_source_legacy_spec_mismatch_is_explicit_error() {
    let mut session = probe_session();
    let company = session
        .state
        .issuer_of_stock(session.stock_id("000812").unwrap())
        .unwrap();
    std::sync::Arc::make_mut(&mut session.state.operations)
        .company_mut(&company)
        .unwrap()
        .spec
        .issued_shares += 100;
    assert_eq!(
        session.state.company_read_facts(),
        Err(crate::session::company_facts::CompanyFactsSourceError::LegacySpecMismatch { company })
    );
}

#[test]
fn selected_company_source_simple_does_not_read_corrupt_legacy_operation() {
    let mut session = divergent_company_source_session(crate::company::CompanyKind::Bank);
    std::sync::Arc::make_mut(&mut session.state.operations)
        .companies
        .clear();
    let facts = session.state.company_read_facts().unwrap();
    assert_eq!(
        facts.company_spec(&crate::company::CompanyId(0)).unwrap(),
        (
            crate::company::CompanyKind::Bank,
            1_000_000 + u64::from(u32::MAX) + 777
        )
    );
    let root = RootReadContext::capture(&session).unwrap();
    assert_eq!(
        root.company_spec(&crate::company::CompanyId(0)).unwrap(),
        (
            crate::company::CompanyKind::Bank,
            1_000_000 + u64::from(u32::MAX) + 777
        )
    );
}

#[test]
fn selected_company_source_legacy_extra_operation_is_explicit_error() {
    let mut session = probe_session();
    let extra = session
        .state
        .operations
        .company(&crate::company::CompanyId(0))
        .unwrap()
        .clone();
    std::sync::Arc::make_mut(&mut session.state.operations)
        .companies
        .insert(crate::company::CompanyId(99), extra);
    assert_eq!(
        session.state.company_read_facts(),
        Err(
            crate::session::company_facts::CompanyFactsSourceError::MissingLegacyRegistry {
                company: crate::company::CompanyId(99)
            }
        )
    );
}

#[test]
fn selected_company_source_missing_simple_system_does_not_fallback_to_legacy() {
    let mut session = divergent_company_source_session(crate::company::CompanyKind::Bank);
    session.state.company_system = None;
    assert_eq!(
        session.state.company_read_facts(),
        Err(crate::session::company_facts::CompanyFactsSourceError::MissingSelectedSystem)
    );
}

#[test]
fn selected_company_source_none_rejects_unselected_system() {
    let mut session = divergent_company_source_session(crate::company::CompanyKind::Bank);
    session.state.setup.company_system = None;
    assert_eq!(
        session.state.company_read_facts(),
        Err(crate::session::company_facts::CompanyFactsSourceError::UnexpectedSystem)
    );
}

#[test]
fn selected_company_source_none_keeps_legacy_root_spec() {
    let session = probe_session();
    let stock = session.stock_id("000812").unwrap();
    let company = session.state.issuer_of_stock(stock).unwrap();
    let spec = session.state.operations.company(&company).unwrap().spec();
    let root = RootReadContext::capture(&session).unwrap();
    assert_eq!(
        root.company_spec(&company).unwrap(),
        (spec.kind, spec.issued_shares)
    );
    assert!(root
        .company_spec(&crate::company::CompanyId(u32::MAX))
        .is_err());
}
