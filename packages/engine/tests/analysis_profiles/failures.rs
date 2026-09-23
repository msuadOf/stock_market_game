//! 类型化拒绝：负权重、全零、总和不符、非法方法/行业映射、恢复档案与配置冲突。

use engine::strategy::{
    largest_remainder_normalize, AnalysisProfile, AnalysisProfileError, AnalysisWeights,
    FundamentalMethod, PersistedAnalysisProfile,
};

fn valid_weights_with_fundamental() -> AnalysisWeights {
    AnalysisWeights::new(5000, 1000, 500, 1000, 2500).unwrap()
}

#[test]
fn negative_weight_is_typed_rejected() {
    let error = AnalysisWeights::new(-1, 1000, 500, 1000, 2500).unwrap_err();
    assert_eq!(
        error,
        AnalysisProfileError::NegativeWeight {
            field: "fundamental_bp",
            value: -1
        }
    );
    let error = AnalysisWeights::new(5000, 1000, 500, -7, 2507).unwrap_err();
    assert_eq!(
        error,
        AnalysisProfileError::NegativeWeight {
            field: "technical_bp",
            value: -7
        }
    );
}

#[test]
fn all_zero_weights_are_typed_rejected() {
    assert_eq!(
        AnalysisWeights::new(0, 0, 0, 0, 0).unwrap_err(),
        AnalysisProfileError::AllZeroWeights
    );
}

#[test]
fn weight_sum_mismatch_is_typed_rejected() {
    assert_eq!(
        AnalysisWeights::new(1000, 2000, 3000, 4000, 5000).unwrap_err(),
        AnalysisProfileError::WeightSumMismatch { actual: 15_000 }
    );
    assert_eq!(
        AnalysisWeights::new(0, 1000, 1000, 1000, 6000).unwrap_err(),
        AnalysisProfileError::WeightSumMismatch { actual: 9000 }
    );
}

#[test]
fn equity_roe_in_profile_slot_is_an_illegal_method_kind_mapping() {
    let error = AnalysisProfile::new(
        valid_weights_with_fundamental(),
        Some(FundamentalMethod::EquityRoe),
    )
    .unwrap_err();
    assert_eq!(
        error,
        AnalysisProfileError::IllegalMethodKindMapping {
            method: FundamentalMethod::EquityRoe
        }
    );
}

#[test]
fn method_with_zero_fundamental_weight_is_rejected() {
    let zero_fundamental = AnalysisWeights::new(0, 1000, 1000, 1000, 7000).unwrap();
    assert_eq!(
        AnalysisProfile::new(zero_fundamental, Some(FundamentalMethod::CashFlow)).unwrap_err(),
        AnalysisProfileError::MethodWithoutFundamentalWeight {
            method: FundamentalMethod::CashFlow
        }
    );
}

#[test]
fn fundamental_weight_without_method_is_rejected() {
    assert_eq!(
        AnalysisProfile::new(valid_weights_with_fundamental(), None).unwrap_err(),
        AnalysisProfileError::FundamentalWeightWithoutMethod
    );
}

/// 恢复边界：未知方法 id 必须类型化拒绝，不静默换方法。
#[test]
fn restored_profile_with_unknown_method_id_is_typed_rejected() {
    let persisted = PersistedAnalysisProfile {
        fundamental_bp: 5000,
        trend_bp: 1000,
        price_volume_bp: 500,
        technical_bp: 1000,
        experience_cost_bp: 2500,
        fundamental_method: Some("voodoo_model".to_string()),
    };
    assert_eq!(
        AnalysisProfile::from_persisted(persisted).unwrap_err(),
        AnalysisProfileError::UnknownFundamentalMethod {
            id: "voodoo_model".to_string()
        }
    );
}

/// 恢复边界：负权重/非法 equity_roe id 与配置冲突均显式拒绝。
#[test]
fn restored_profile_with_conflicting_config_is_rejected() {
    let negative = PersistedAnalysisProfile {
        fundamental_bp: -500,
        trend_bp: 2000,
        price_volume_bp: 1000,
        technical_bp: 1000,
        experience_cost_bp: 7500,
        fundamental_method: Some("cash_flow".to_string()),
    };
    assert_eq!(
        AnalysisProfile::from_persisted(negative).unwrap_err(),
        AnalysisProfileError::NegativeWeight {
            field: "fundamental_bp",
            value: -500
        }
    );

    let equity_roe = PersistedAnalysisProfile {
        fundamental_bp: 5000,
        trend_bp: 1000,
        price_volume_bp: 500,
        technical_bp: 1000,
        experience_cost_bp: 2500,
        fundamental_method: Some("equity_roe".to_string()),
    };
    assert_eq!(
        AnalysisProfile::from_persisted(equity_roe).unwrap_err(),
        AnalysisProfileError::IllegalMethodKindMapping {
            method: FundamentalMethod::EquityRoe
        }
    );

    let method_without_weight = PersistedAnalysisProfile {
        fundamental_bp: 0,
        trend_bp: 1000,
        price_volume_bp: 1000,
        technical_bp: 1000,
        experience_cost_bp: 7000,
        fundamental_method: Some("cash_flow".to_string()),
    };
    assert_eq!(
        AnalysisProfile::from_persisted(method_without_weight).unwrap_err(),
        AnalysisProfileError::MethodWithoutFundamentalWeight {
            method: FundamentalMethod::CashFlow
        }
    );

    let weight_without_method = PersistedAnalysisProfile {
        fundamental_bp: 5000,
        trend_bp: 1000,
        price_volume_bp: 500,
        technical_bp: 1000,
        experience_cost_bp: 2500,
        fundamental_method: None,
    };
    assert_eq!(
        AnalysisProfile::from_persisted(weight_without_method).unwrap_err(),
        AnalysisProfileError::FundamentalWeightWithoutMethod
    );
}

/// serde 反序列化路径对非法档案同样显式失败（错误信息携带类型化原因）。
#[test]
fn serde_deserialize_of_illegal_profile_fails_explicitly() {
    let illegal = serde_json::json!({
        "fundamental_bp": -1,
        "trend_bp": 2001,
        "price_volume_bp": 500,
        "technical_bp": 1000,
        "experience_cost_bp": 7500,
        "fundamental_method": "cash_flow"
    });
    let error = serde_json::from_value::<AnalysisProfile>(illegal).unwrap_err();
    assert!(
        error.to_string().contains("negative analysis weight"),
        "serde 边界必须暴露类型化原因：{error}"
    );

    let unknown = serde_json::json!({
        "fundamental_bp": 5000,
        "trend_bp": 1000,
        "price_volume_bp": 500,
        "technical_bp": 1000,
        "experience_cost_bp": 2500,
        "fundamental_method": "pe_ratio_v9"
    });
    assert!(serde_json::from_value::<AnalysisProfile>(unknown)
        .unwrap_err()
        .to_string()
        .contains("unknown fundamental method id"));
}

/// 归一函数拒绝负输入与非正总量（公开契约的显式守卫）。
#[test]
fn normalization_rejects_invalid_input() {
    assert_eq!(
        largest_remainder_normalize([1, -1, 0, 0, 0], 10_000).unwrap_err(),
        AnalysisProfileError::InvalidNormalizationInput { total: 0 }
    );
    assert_eq!(
        largest_remainder_normalize([0, 0, 0, 0, 0], 10_000).unwrap_err(),
        AnalysisProfileError::InvalidNormalizationInput { total: 0 }
    );
}
