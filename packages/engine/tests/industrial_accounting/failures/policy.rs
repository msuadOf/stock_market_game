//! 非法税率 / 计量政策 → 类型化拒绝（Fixture 合成政策本身也必须通过校验边界：
//! 税率 0..=10000bp、抵扣比例 0..=10000bp、结转年限 ≥ 1、版本 ≥ 1）。

use super::super::{base_config, fixture_policy, yuan};
use super::fresh;
use engine::accounting::{AccountingAmount, TaxPolicy, TaxPolicyError};
use engine::company::industrial::{IndustrialError, OpeningDebtTerms};

fn bad_policy(mutate: impl FnOnce(&mut TaxPolicy)) -> TaxPolicy {
    let mut policy = fixture_policy();
    mutate(&mut policy);
    policy
}

#[test]
fn tax_policy_rejects_out_of_range_rates() {
    assert!(matches!(
        bad_policy(|p| p.vat.output_rate_bp = 10_001).validate(),
        Err(TaxPolicyError::Invalid { .. })
    ));
    assert!(matches!(
        bad_policy(|p| p.vat.input_rate_bp = -1).validate(),
        Err(TaxPolicyError::Invalid { .. })
    ));
    assert!(matches!(
        bad_policy(|p| p.vat.deductible_share_bp = 10_001).validate(),
        Err(TaxPolicyError::Invalid { .. })
    ));
    assert!(matches!(
        bad_policy(|p| p.income_tax.rate_bp = 10_001).validate(),
        Err(TaxPolicyError::Invalid { .. })
    ));
    assert!(matches!(
        bad_policy(|p| p.income_tax.loss_carryforward_years = 0).validate(),
        Err(TaxPolicyError::Invalid { .. })
    ));
    assert!(matches!(
        bad_policy(|p| p.version = 0).validate(),
        Err(TaxPolicyError::Invalid { .. })
    ));
    // 边界值全部合法：0 / 10000bp / 1 年。
    assert!(bad_policy(|p| p.vat.output_rate_bp = 0).validate().is_ok());
    assert!(bad_policy(|p| p.vat.deductible_share_bp = 0)
        .validate()
        .is_ok());
    assert!(bad_policy(|p| p.income_tax.rate_bp = 10_000)
        .validate()
        .is_ok());
    // 非法政策构造公司 → 类型化拒绝。
    let mut cfg = base_config();
    cfg.tax_policy = bad_policy(|p| p.income_tax.rate_bp = -5);
    assert!(matches!(
        engine::company::industrial::IndustrialBooks::new(cfg),
        Err(IndustrialError::Tax(TaxPolicyError::Invalid { .. }))
    ));
}

#[test]
fn asset_measurement_policy_rejects_illegal_lifespan_and_salvage() {
    let mut co = fresh();
    let before = co.clone();
    // 非法计量政策：寿命 0 月、残值 > 成本、零成本（均为过账前拒绝）。
    assert!(matches!(
        co.acquire_asset(
            engine::accounting::FixedAssetCode("M0".to_string()),
            yuan(1_000),
            AccountingAmount::ZERO,
            0,
            super::super::d("2030-01-02"),
        ),
        Err(IndustrialError::Asset(
            engine::accounting::FixedAssetError::AssetPolicyInvalid { .. }
        ))
    ));
    assert!(matches!(
        co.acquire_asset(
            engine::accounting::FixedAssetCode("M0".to_string()),
            yuan(100),
            yuan(200),
            12,
            super::super::d("2030-01-02"),
        ),
        Err(IndustrialError::Asset(
            engine::accounting::FixedAssetError::AssetPolicyInvalid { .. }
        ))
    ));
    // 减值超上限（账面 − 残值）→ 拒绝。
    co.acquire_asset(
        engine::accounting::FixedAssetCode("M1".to_string()),
        yuan(1_000),
        yuan(10),
        12,
        super::super::d("2030-01-02"),
    )
    .expect("valid asset");
    let before_asset = co.clone();
    assert!(matches!(
        co.impair_asset(
            &engine::accounting::FixedAssetCode("M1".to_string()),
            yuan(991),
            super::super::d("2030-01-15")
        ),
        Err(IndustrialError::Asset(
            engine::accounting::FixedAssetError::ImpairmentBeyondFloor { .. }
        ))
    ));
    assert_eq!(co, before_asset);
    let _ = before;
}

#[test]
fn sale_due_date_before_sale_date_is_rejected() {
    let mut co = fresh();
    use engine::company::industrial::Settlement;
    co.purchase(
        &engine::company::CounterpartyId("EXT-SUPP".to_string()),
        engine::accounting::InventoryItemCode("GOODS".to_string()),
        engine::accounting::LedgerAccountId("1405".to_string()),
        1,
        yuan(10),
        Settlement::Cash,
        super::super::d("2030-03-01"),
        super::super::d("2030-01-05"),
    )
    .expect("purchase");
    let before = co.clone();
    assert!(matches!(
        co.sell_credit(
            &engine::company::CounterpartyId("EXT-CUST".to_string()),
            engine::accounting::InventoryItemCode("GOODS".to_string()),
            1,
            yuan(100),
            super::super::d("2030-01-01"),
            super::super::d("2030-01-15"),
        ),
        Err(IndustrialError::InvalidDueDate { .. })
    ));
    assert_eq!(co, before);
    let _ = OpeningDebtTerms {
        lender: engine::company::CounterpartyId("EXT-BANK".to_string()),
        principal: yuan(1),
        annual_rate_bp: 0,
        maturity_date: super::super::d("2031-01-01"),
    };
}
