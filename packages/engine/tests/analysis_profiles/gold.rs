//! 金样：K5 默认分布逐字钉住、三类风格完整手算样例、工厂重放确定性。

use super::StrictSeqRng;
use engine::account::AccountKind;
use engine::strategy::{
    default_analysis_weights, derive_analysis_profile, HotStyle, InstitutionStyle, RetailStyle,
    StrategyFactory, StrategyProfile,
};
use engine::{AccountId, SplitMix64};

fn profile_of(style: RetailStyle) -> StrategyProfile {
    StrategyProfile::Retail(style)
}

/// K5 计划第 129 行的逐字副本（顺序：基本面/趋势/量价/技术/成本经历，bp）。
#[test]
fn default_table_matches_plan_verbatim() {
    let cases: [(StrategyProfile, [u32; 5]); 13] = [
        (
            profile_of(RetailStyle::LongTerm),
            [5000, 1000, 500, 1000, 2500],
        ),
        (
            profile_of(RetailStyle::DipBuyer),
            [1500, 1000, 1000, 1500, 5000],
        ),
        (
            profile_of(RetailStyle::Momentum),
            [500, 4500, 2000, 2000, 1000],
        ),
        (profile_of(RetailStyle::Noise), [0, 1000, 1000, 1000, 7000]),
        (profile_of(RetailStyle::Panic), [0, 2000, 1000, 1000, 6000]),
        (
            profile_of(RetailStyle::Dormant),
            [1000, 1000, 500, 500, 7000],
        ),
        (
            StrategyProfile::Institution(InstitutionStyle::DeepValue),
            [7000, 500, 500, 500, 1500],
        ),
        (
            StrategyProfile::Institution(InstitutionStyle::Growth),
            [7000, 1000, 500, 500, 1000],
        ),
        (
            StrategyProfile::Institution(InstitutionStyle::Balanced),
            [6000, 1000, 1000, 1000, 1000],
        ),
        (
            StrategyProfile::Institution(InstitutionStyle::Defensive),
            [6500, 500, 500, 500, 2000],
        ),
        (
            StrategyProfile::Institution(InstitutionStyle::ActiveTrader),
            [0, 4500, 2500, 2000, 1000],
        ),
        (
            StrategyProfile::Hot(HotStyle::Momentum),
            [0, 5000, 2500, 2000, 500],
        ),
        (
            StrategyProfile::Hot(HotStyle::Reversal),
            [0, 1500, 2500, 3000, 3000],
        ),
    ];
    for (profile, expected) in cases {
        let weights = default_analysis_weights(&profile);
        assert_eq!(
            weights.as_array(),
            expected,
            "default table drift: {profile:?}"
        );
        assert_eq!(weights.total_bp(), 10_000);
    }
}

/// 散户类完整手算样例（LongTerm + 账户 9）。
/// 倍率 [6000,14000,10000,8000,12000] → raw [30M,14M,5M,8M,30M]，S=87M；
/// floor [3448,1609,574,919,3448]（Σ=9998），余数 [24M,17M,62M,47M,24M]，
/// deficit=2 依次给量价(62M)与技术(47M) → [3448,1609,575,920,3448]。
#[test]
fn retail_long_term_full_worked_example() {
    let mut rng = StrictSeqRng::new(&[0.0, 0.999_95, 0.5, 0.25, 0.75]);
    let profile =
        derive_analysis_profile(&profile_of(RetailStyle::LongTerm), AccountId(9), &mut rng)
            .unwrap();
    assert_eq!(profile.weights().as_array(), [3448, 1609, 575, 920, 3448]);
    assert_eq!(profile.weights().total_bp(), 10_000);
    // LongTerm 属“其他有基本面权重者”：稳定 AccountId 奇偶，9 为奇 → 现金流法。
    assert_eq!(
        profile.fundamental_method(),
        Some(engine::strategy::FundamentalMethod::CashFlow)
    );
    assert_eq!(rng.draws_used(), 5, "每个非零权重恰好采样一次");
}

/// 机构类完整手算样例（ActiveTrader + 账户 2，零基本权重路径）。
/// 零权重不消耗随机数（严格 RNG 只给 4 个值仍成功）；倍率 [6000,8000,10000,12000] →
/// raw [27M,20M,20M,12M]，S=79M；floor [3417,2531,2531,1518]（Σ=9997），
/// 余数 [57M,51M,51M,78M]，deficit=3 给成本经历(78M)、趋势(57M)、量价(51M，
/// 与技术同余按字段顺序取胜) → [0,3418,2532,2531,1519]。
#[test]
fn institution_active_trader_zero_fundamental_worked_example() {
    let mut rng = StrictSeqRng::new(&[0.0, 0.25, 0.5, 0.75]);
    let profile = derive_analysis_profile(
        &StrategyProfile::Institution(InstitutionStyle::ActiveTrader),
        AccountId(2),
        &mut rng,
    )
    .unwrap();
    assert_eq!(profile.weights().as_array(), [0, 3418, 2532, 2531, 1519]);
    assert_eq!(profile.fundamental_method(), None, "零基本权重 ⇒ 无方法");
    assert_eq!(rng.draws_used(), 4, "零权重字段不采样");
}

/// 游资类完整手算样例（Reversal + 账户 4，恰好整除路径）。
/// 倍率 [6000,10000,14000,8000] → raw [9M,25M,42M,24M]，S=100M，各项整除 →
/// [0,900,2500,4200,2400]，deficit=0。
#[test]
fn hot_reversal_exact_normalization_worked_example() {
    let mut rng = StrictSeqRng::new(&[0.0, 0.5, 0.999_95, 0.25]);
    let profile = derive_analysis_profile(
        &StrategyProfile::Hot(HotStyle::Reversal),
        AccountId(4),
        &mut rng,
    )
    .unwrap();
    assert_eq!(profile.weights().as_array(), [0, 900, 2500, 4200, 2400]);
    assert_eq!(profile.fundamental_method(), None);
    assert_eq!(rng.draws_used(), 4);
}

/// 工厂重放：同 seed 同 ordinal ⇒ 同一主导风格 ⇒ 同一个体分析档案。
/// 既有 ordinal→身份/主导风格映射保持不变（机构 ordinal%5、游资 ordinal%2）。
#[test]
fn factory_replay_same_seed_same_ordinal_yields_identical_profile() {
    let params = super::sample_params();

    // 散户：风格由工厂抽样，重放同 seed 得同风格，再派生两次同 profile。
    for kind in [AccountKind::Retail, AccountKind::Inst, AccountKind::Hot] {
        let ordinals: Vec<u32> = match kind {
            AccountKind::Retail => vec![0, 1, 2],
            AccountKind::Inst => vec![0, 1, 2, 3, 4, 5],
            _ => vec![0, 1, 2],
        };
        for ordinal in ordinals {
            let mut build_rng_a = SplitMix64::new(0xA17_0001);
            let mut build_rng_b = SplitMix64::new(0xA17_0001);
            let strategy_a = StrategyFactory::build_for_market_day_with_ordinal(
                kind,
                &params,
                15_300,
                ordinal,
                &mut build_rng_a,
            )
            .unwrap()
            .unwrap();
            let strategy_b = StrategyFactory::build_for_market_day_with_ordinal(
                kind,
                &params,
                15_300,
                ordinal,
                &mut build_rng_b,
            )
            .unwrap()
            .unwrap();
            assert_eq!(strategy_a.profile(), strategy_b.profile());

            let mut derive_rng_a = SplitMix64::new(0xA17_0002 ^ u64::from(ordinal));
            let mut derive_rng_b = SplitMix64::new(0xA17_0002 ^ u64::from(ordinal));
            let derived_a =
                derive_analysis_profile(&strategy_a.profile(), AccountId(41), &mut derive_rng_a)
                    .unwrap();
            let derived_b =
                derive_analysis_profile(&strategy_b.profile(), AccountId(41), &mut derive_rng_b)
                    .unwrap();
            assert_eq!(derived_a, derived_b, "{kind:?} ordinal {ordinal}");
        }
    }
}

/// 机构 ordinal→风格映射保持原状，且派生走该风格的 K5 默认表。
#[test]
fn institution_ordinal_mapping_is_preserved_and_feeds_the_style_table() {
    let params = super::sample_params();
    let expected = [
        InstitutionStyle::DeepValue,
        InstitutionStyle::Growth,
        InstitutionStyle::Balanced,
        InstitutionStyle::Defensive,
        InstitutionStyle::ActiveTrader,
        InstitutionStyle::DeepValue,
    ];
    for (ordinal, style) in expected.iter().enumerate() {
        let mut build_rng = SplitMix64::new(0x1A57_1700);
        let strategy = StrategyFactory::build_for_market_day_with_ordinal(
            AccountKind::Inst,
            &params,
            15_300,
            ordinal as u32,
            &mut build_rng,
        )
        .unwrap()
        .unwrap();
        assert_eq!(strategy.institution_style(), Some(*style));

        let mut direct_rng = SplitMix64::new(0x5EED_0017);
        let direct = derive_analysis_profile(
            &StrategyProfile::Institution(*style),
            AccountId(3),
            &mut direct_rng,
        )
        .unwrap();
        let mut via_factory_rng = SplitMix64::new(0x5EED_0017);
        let via_factory =
            derive_analysis_profile(&strategy.profile(), AccountId(3), &mut via_factory_rng)
                .unwrap();
        assert_eq!(direct, via_factory, "ordinal {ordinal}");
    }
}
