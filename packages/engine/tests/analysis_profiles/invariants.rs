//! 不变量：跨身份组合、方法链接奇偶、零权重路径、归一总和、方法稳定性与 serde 往返。

use engine::company::CompanyKind;
use engine::strategy::{
    derive_analysis_profile, largest_remainder_normalize, AnalysisProfile, FundamentalMethod,
    HotStyle, InstitutionStyle, RetailStyle, StrategyProfile,
};
use engine::{AccountId, SplitMix64};

const ALL_STYLES: [StrategyProfile; 13] = [
    StrategyProfile::Retail(RetailStyle::LongTerm),
    StrategyProfile::Retail(RetailStyle::DipBuyer),
    StrategyProfile::Retail(RetailStyle::Momentum),
    StrategyProfile::Retail(RetailStyle::Noise),
    StrategyProfile::Retail(RetailStyle::Panic),
    StrategyProfile::Retail(RetailStyle::Dormant),
    StrategyProfile::Institution(InstitutionStyle::DeepValue),
    StrategyProfile::Institution(InstitutionStyle::Growth),
    StrategyProfile::Institution(InstitutionStyle::Balanced),
    StrategyProfile::Institution(InstitutionStyle::Defensive),
    StrategyProfile::Institution(InstitutionStyle::ActiveTrader),
    StrategyProfile::Hot(HotStyle::Momentum),
    StrategyProfile::Hot(HotStyle::Reversal),
];

const KINDS: [CompanyKind; 4] = [
    CompanyKind::Industrial,
    CompanyKind::Bank,
    CompanyKind::Insurance,
    CompanyKind::RealEstate,
];

/// 跨身份组合：机构可以没有基本面（ActiveTrader），散户可以有基本面（LongTerm 等）。
/// 能力由档案实体表达，不以账户身份名字推断。
#[test]
fn cross_identity_combinations_are_legal() {
    let institution_without_fundamental = derive_analysis_profile(
        &StrategyProfile::Institution(InstitutionStyle::ActiveTrader),
        AccountId(0),
        &mut SplitMix64::new(0xC1055_0001),
    )
    .unwrap();
    assert_eq!(
        institution_without_fundamental.weights().fundamental_bp(),
        0
    );
    assert_eq!(institution_without_fundamental.fundamental_method(), None);

    for retail in [
        RetailStyle::LongTerm,
        RetailStyle::DipBuyer,
        RetailStyle::Dormant,
    ] {
        let with_fundamental = derive_analysis_profile(
            &StrategyProfile::Retail(retail),
            AccountId(6),
            &mut SplitMix64::new(0xC1055_0002),
        )
        .unwrap();
        assert!(
            with_fundamental.weights().fundamental_bp() > 0,
            "散户可以有基本面权重：{retail:?}"
        );
        assert!(with_fundamental.fundamental_method().is_some());
    }
}

/// 方法链接只在工厂发生一次：DeepValue/Defensive→盈利倍数、Growth→现金流；
/// 其余（Balanced/ActiveTrader 机构与一切散户/游资风格）按稳定 AccountId 奇偶
/// （偶→盈利倍数，奇→现金流）。与 RNG 序列无关。
#[test]
fn fundamental_method_linkage_follows_style_and_account_parity() {
    let even = AccountId(8);
    let odd = AccountId(9);
    let mut rng = SplitMix64::new(0x0DD_5EED);

    let cases: [(StrategyProfile, FundamentalMethod, FundamentalMethod); 8] = [
        (
            StrategyProfile::Institution(InstitutionStyle::DeepValue),
            FundamentalMethod::EarningsMultiple,
            FundamentalMethod::EarningsMultiple,
        ),
        (
            StrategyProfile::Institution(InstitutionStyle::Defensive),
            FundamentalMethod::EarningsMultiple,
            FundamentalMethod::EarningsMultiple,
        ),
        (
            StrategyProfile::Institution(InstitutionStyle::Growth),
            FundamentalMethod::CashFlow,
            FundamentalMethod::CashFlow,
        ),
        (
            StrategyProfile::Institution(InstitutionStyle::Balanced),
            FundamentalMethod::EarningsMultiple,
            FundamentalMethod::CashFlow,
        ),
        (
            StrategyProfile::Retail(RetailStyle::LongTerm),
            FundamentalMethod::EarningsMultiple,
            FundamentalMethod::CashFlow,
        ),
        (
            StrategyProfile::Retail(RetailStyle::DipBuyer),
            FundamentalMethod::EarningsMultiple,
            FundamentalMethod::CashFlow,
        ),
        (
            StrategyProfile::Retail(RetailStyle::Momentum),
            FundamentalMethod::EarningsMultiple,
            FundamentalMethod::CashFlow,
        ),
        (
            StrategyProfile::Retail(RetailStyle::Dormant),
            FundamentalMethod::EarningsMultiple,
            FundamentalMethod::CashFlow,
        ),
    ];
    for (profile, even_method, odd_method) in cases {
        assert_eq!(
            derive_analysis_profile(&profile, even, &mut rng)
                .unwrap()
                .fundamental_method(),
            Some(even_method),
            "{profile:?} 偶数账户"
        );
        assert_eq!(
            derive_analysis_profile(&profile, odd, &mut rng)
                .unwrap()
                .fundamental_method(),
            Some(odd_method),
            "{profile:?} 奇数账户"
        );
    }
}

/// 零权重路径：默认零基本权重的风格，任意 seed 下保持零且对所有行业都不给方法
/// —— 零权重真正不运行该分析，不因银行/保险身份获得能力。
#[test]
fn zero_fundamental_weight_survives_sampling_and_disables_the_analysis() {
    for profile in ALL_STYLES {
        for seed in 0..32u64 {
            let derived =
                derive_analysis_profile(&profile, AccountId(seed), &mut SplitMix64::new(seed))
                    .unwrap();
            let default_zero =
                engine::strategy::default_analysis_weights(&profile).fundamental_bp() == 0;
            assert_eq!(
                derived.weights().fundamental_bp() == 0,
                default_zero,
                "{profile:?} seed {seed}: 零权重必须保持零、非零必须保持非零"
            );
            for kind in KINDS {
                let expected = if derived.weights().fundamental_bp() == 0 {
                    None
                } else {
                    match kind {
                        CompanyKind::Bank | CompanyKind::Insurance => {
                            Some(FundamentalMethod::EquityRoe)
                        }
                        CompanyKind::Industrial | CompanyKind::RealEstate => {
                            derived.fundamental_method()
                        }
                    }
                };
                assert_eq!(
                    derived.method_for_company_kind(kind),
                    expected,
                    "{profile:?} seed {seed} kind {kind:?}"
                );
            }
        }
    }
}

/// 银行/保险行业恒用权益 ROE 法（规则而非抽样）；工商/地产用档案保存的方法。
#[test]
fn bank_and_insurance_kinds_always_map_to_equity_roe() {
    let profile = derive_analysis_profile(
        &StrategyProfile::Retail(RetailStyle::LongTerm),
        AccountId(8),
        &mut SplitMix64::new(0xBA0_0001),
    )
    .unwrap();
    assert_eq!(
        profile.method_for_company_kind(CompanyKind::Bank),
        Some(FundamentalMethod::EquityRoe)
    );
    assert_eq!(
        profile.method_for_company_kind(CompanyKind::Insurance),
        Some(FundamentalMethod::EquityRoe)
    );
    assert_eq!(
        profile.method_for_company_kind(CompanyKind::Industrial),
        Some(FundamentalMethod::EarningsMultiple),
        "偶数账户的工商/地产方法"
    );
    assert_eq!(
        profile.method_for_company_kind(CompanyKind::RealEstate),
        Some(FundamentalMethod::EarningsMultiple)
    );
}

/// 采样后所有风格权重总和恰为 10000bp，且零/非零模式与默认表一致。
#[test]
fn all_styles_sum_to_10000_after_sampling() {
    for profile in ALL_STYLES {
        for seed in 0..32u64 {
            let derived = derive_analysis_profile(
                &profile,
                AccountId(seed),
                &mut SplitMix64::new(0x5EED_9_000 + seed),
            )
            .unwrap();
            assert_eq!(
                derived.weights().total_bp(),
                10_000,
                "{profile:?} seed {seed}"
            );
        }
    }
}

/// 主导风格不是每次观察抽样：方法链接只取决于（风格, 账户奇偶），与 RNG 无关；
/// 零/非零模式也只由风格决定。
#[test]
fn dominant_style_is_not_resampled_per_derivation() {
    for profile in ALL_STYLES {
        for (seed_a, seed_b) in [(1u64, 2u64), (3u64, 4u64)] {
            let a = derive_analysis_profile(&profile, AccountId(5), &mut SplitMix64::new(seed_a))
                .unwrap();
            let b = derive_analysis_profile(&profile, AccountId(5), &mut SplitMix64::new(seed_b))
                .unwrap();
            assert_eq!(
                a.fundamental_method(),
                b.fundamental_method(),
                "{profile:?}: 方法链接不受 RNG 影响"
            );
            assert_eq!(
                a.weights().as_array().map(|w| w == 0),
                b.weights().as_array().map(|w| w == 0),
                "{profile:?}: 零/非零模式是风格属性"
            );
        }
    }
}

/// 最大余数法的破同分与零保持契约（直接以归一函数锁定）：
/// - 同余按字段声明顺序（基本面在前）破同分；
/// - raw 为 0 的字段恒归一为 0。
#[test]
fn largest_remainder_breaks_ties_in_field_order() {
    // 三个等比字段：floor 各 3333（Σ=9999），deficit=1，余数全同 → 基本面先得。
    assert_eq!(
        largest_remainder_normalize([1, 1, 1, 0, 0], 10_000).unwrap(),
        [3334, 3333, 3333, 0, 0]
    );
    // 恰好整除：deficit=0。
    assert_eq!(
        largest_remainder_normalize([0, 5, 0, 5, 0], 10_000).unwrap(),
        [0, 5000, 0, 5000, 0]
    );
}

/// 档案可序列化且往返不变；未知字段被显式拒绝（deny_unknown_fields）。
#[test]
fn profile_serde_roundtrip() {
    let derived = derive_analysis_profile(
        &StrategyProfile::Retail(RetailStyle::LongTerm),
        AccountId(9),
        &mut SplitMix64::new(0x1234_ABCD),
    )
    .unwrap();
    let json = serde_json::to_value(&derived).unwrap();
    let restored: AnalysisProfile = serde_json::from_value(json).unwrap();
    assert_eq!(restored, derived);

    let mut with_extra = serde_json::to_value(&derived).unwrap();
    with_extra["surprise"] = serde_json::json!(1);
    assert!(serde_json::from_value::<AnalysisProfile>(with_extra).is_err());
}
