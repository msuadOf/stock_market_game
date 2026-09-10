//! 金样：四种独立测试实体的平衡开局 + 默认 5 股票的公司映射与股本精确匹配。

use super::*;
use engine::company::CompanyRegistry;

/// 四种 CompanyKind 各一家独立测试实体（与 defaults.rs 的未上市测试实体同数字，
/// 但在此独立构造，证明「四种独立测试实体可以创建」不依赖默认集合）。
fn standalone_entities() -> Vec<CompanyConfig> {
    Vec::from([
        // 工商：现金 4000 万 + 应收 500 万 + 固定资产 6500 万
        //      = 实收资本 1 亿 + 短期借款 1000 万
        bare_config(
            unlisted_spec("T-IND", CompanyKind::Industrial, 100_000_000),
            generic_opening(vec![
                opening_line("1002", PostingSide::Debit, 40_000_000),
                opening_line("1122", PostingSide::Debit, 5_000_000),
                opening_line("1601", PostingSide::Debit, 65_000_000),
                opening_line("4001", PostingSide::Credit, 100_000_000),
                opening_line("2001", PostingSide::Credit, 10_000_000),
            ]),
        ),
        // 银行：现金 65 亿 + 应收 1 亿 + 固定资产 4 亿
        //      = 实收资本 50 亿 + 短期借款 20 亿
        bare_config(
            unlisted_spec("T-BANK", CompanyKind::Bank, 5_000_000_000),
            generic_opening(vec![
                opening_line("1002", PostingSide::Debit, 6_500_000_000),
                opening_line("1122", PostingSide::Debit, 100_000_000),
                opening_line("1601", PostingSide::Debit, 400_000_000),
                opening_line("4001", PostingSide::Credit, 5_000_000_000),
                opening_line("2001", PostingSide::Credit, 2_000_000_000),
            ]),
        ),
        // 保险：现金 80 亿 + 固定资产 20 亿 = 实收资本 100 亿
        bare_config(
            unlisted_spec("T-INS", CompanyKind::Insurance, 10_000_000_000),
            generic_opening(vec![
                opening_line("1002", PostingSide::Debit, 8_000_000_000),
                opening_line("1601", PostingSide::Debit, 2_000_000_000),
                opening_line("4001", PostingSide::Credit, 10_000_000_000),
            ]),
        ),
        // 地产：现金 12 亿 + 应收 1 亿 + 固定资产 17 亿
        //      = 实收资本 20 亿 + 短期借款 10 亿
        bare_config(
            unlisted_spec("T-RE", CompanyKind::RealEstate, 2_000_000_000),
            generic_opening(vec![
                opening_line("1002", PostingSide::Debit, 1_200_000_000),
                opening_line("1122", PostingSide::Debit, 100_000_000),
                opening_line("1601", PostingSide::Debit, 1_700_000_000),
                opening_line("4001", PostingSide::Credit, 2_000_000_000),
                opening_line("2001", PostingSide::Credit, 1_000_000_000),
            ]),
        ),
    ])
}

/// 每种会计类型一家独立测试实体：试算平衡、现金/负债/权益滚动全部对账。
#[test]
fn all_company_kinds_open_balanced() {
    let registry =
        CompanyRegistry::new(standalone_entities()).expect("four standalone entities construct");
    assert_eq!(registry.len(), 4);

    // (kind, 借贷总额, 现金, 负债, 权益滚动) —— 单位：元
    let cases = [
        (
            CompanyKind::Industrial,
            110_000_000,
            40_000_000,
            10_000_000,
            100_000_000,
        ),
        (
            CompanyKind::Bank,
            7_000_000_000,
            6_500_000_000,
            2_000_000_000,
            5_000_000_000,
        ),
        (
            CompanyKind::Insurance,
            10_000_000_000,
            8_000_000_000,
            0,
            10_000_000_000,
        ),
        (
            CompanyKind::RealEstate,
            3_000_000_000,
            1_200_000_000,
            1_000_000_000,
            2_000_000_000,
        ),
    ];
    for (kind, debit_total, cash, liabilities, equity) in cases {
        let of_kind: Vec<_> = registry
            .iter()
            .filter(|(_, company)| company.spec().kind == kind)
            .collect();
        assert_eq!(
            of_kind.len(),
            1,
            "exactly one standalone entity per kind: {kind:?}"
        );

        let ledger = of_kind[0].1.books().ledger();
        let trial = ledger.trial_balance().expect("trial balance");
        assert_eq!(
            trial.total_debits,
            yuan(debit_total),
            "{kind:?} total debits"
        );
        assert_eq!(
            trial.total_credits,
            yuan(debit_total),
            "{kind:?} total credits"
        );
        assert_eq!(
            ledger.cash_total().expect("cash total"),
            yuan(cash),
            "{kind:?} cash"
        );
        assert_eq!(
            ledger.liabilities_total().expect("liabilities"),
            yuan(liabilities),
            "{kind:?} liabilities"
        );
        assert_eq!(
            ledger.equity_rolling().expect("equity rolling"),
            yuan(equity),
            "{kind:?} equity"
        );
        assert_eq!(
            of_kind[0].1.books().journal().entry_count(),
            1,
            "{kind:?} has exactly the opening voucher"
        );
    }
}

/// 默认注册表：公司数、发行股票映射、股本精确匹配、四类测试实体与集团关系。
#[test]
fn default_registry_maps_five_stocks_with_exact_share_capital() {
    let registry = default_registry();
    // 5 家上市公司 + 4 家未上市独立测试实体。
    assert_eq!(registry.len(), 9);

    let stocks = default_stock_specs();
    let stock_refs: Vec<(StockCode, u64)> = stocks
        .iter()
        .map(|stock| (stock.code.clone(), stock.total_shares))
        .collect();
    registry
        .validate_issuer_mapping(&stock_refs)
        .expect("default registry maps the five default stocks exactly");

    // (code, total_shares)：与 apps/web/src/config/defaults.ts 逐字段一致。
    let pinned: [(&str, u64); 5] = [
        ("600101", 8_928_571_429),
        ("002156", 2_925_045_704),
        ("300260", 815_217_391),
        ("600610", 1_059_602_649),
        ("000812", 1_052_631_579),
    ];
    for (code, total_shares) in pinned {
        let issuer = registry
            .issuer_of(&StockCode(code.to_string()))
            .expect("every default stock has an issuer");
        let company = registry.get(issuer).expect("issuer is registered");
        // 股本精确匹配（K2：固定股本与股票总股本一致，流通股与总股本不可互换）。
        assert_eq!(company.spec().issued_shares, total_shares, "{code}");
        // 默认 5 股票全部映射工商语义；银行/保险/地产只作为未上市测试实体存在。
        assert_eq!(company.spec().kind, CompanyKind::Industrial, "{code}");
        // 实收资本 = 面值 1 元 × 总股本（与市价无关：600101 市价 11.20 元，
        // 市值约 1000 亿 ≠ 股本 89.29 亿——不依据初始股价反推资产）。
        assert_eq!(
            company.books().ledger().equity_rolling().expect("equity"),
            yuan(i128::from(total_shares)),
            "{code} paid-in capital = par value 1.00 yuan x shares"
        );
    }

    // 未上市独立测试实体：四种 CompanyKind 各恰好一家。
    for kind in [
        CompanyKind::Industrial,
        CompanyKind::Bank,
        CompanyKind::Insurance,
        CompanyKind::RealEstate,
    ] {
        let unlisted_of_kind = registry
            .iter()
            .filter(|(_, company)| {
                company.spec().kind == kind && company.spec().listed_stock.is_none()
            })
            .count();
        assert_eq!(
            unlisted_of_kind, 1,
            "one unlisted test entity per kind: {kind:?}"
        );
    }

    // 固定集团关系：测试工商实体是 600101 发行人的子公司（合并报表任务 12）。
    let parent = registry
        .issuer_of(&StockCode("600101".to_string()))
        .expect("600101 issuer")
        .clone();
    let subsidiary = registry
        .get(&CompanyId("C-TEST-IND".to_string()))
        .expect("unlisted industrial test entity");
    assert_eq!(subsidiary.spec().group_parent.as_ref(), Some(&parent));
}
