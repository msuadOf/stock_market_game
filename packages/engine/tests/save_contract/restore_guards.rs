use engine::plans::PlanOpen;
use engine::session::{decode_save_slot, GameSession, SaveDecodeLimits};
use engine::{AccountId, OpinionSource, PlanOpinion, PlanTarget, Side, StockCode, Urgency};

fn baseline() -> serde_json::Value {
    static BASELINE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    BASELINE
        .get_or_init(|| {
            let mut session = GameSession::new(super::continuity_setup(), super::SEED).unwrap();
            super::run_full_day(&mut session);
            let mut save = session.save().unwrap();
            save.plans
                .create(PlanOpen {
                    account: AccountId(1),
                    code: StockCode("600101".into()),
                    direction: Side::Buy,
                    target: PlanTarget::ShareCount(100),
                    opinion: PlanOpinion {
                        signal_score_bp: 2000,
                        source: OpinionSource::Fundamental,
                    },
                    confidence_bp: 6000,
                    urgency: Urgency::Normal,
                    horizon_trading_days: 20,
                    created_trading_day: 0,
                })
                .unwrap();
            GameSession::restore(&save).expect("被修改之前的完整基线必须能恢复");
            serde_json::to_value(save).unwrap()
        })
        .clone()
}

fn decode_error(wire: serde_json::Value) -> String {
    match decode_save_slot(
        &serde_json::to_vec(&wire).unwrap(),
        &SaveDecodeLimits::default(),
    ) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("损坏状态不应通过完整 SaveSlot 解码"),
    }
}

fn restore_error(wire: serde_json::Value) -> String {
    let save = decode_save_slot(
        &serde_json::to_vec(&wire).unwrap(),
        &SaveDecodeLimits::default(),
    )
    .unwrap();
    match GameSession::restore(&save) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("损坏状态不应安装为运行会话"),
    }
}

#[test]
fn audit_credit_complete_save_rejects_invalid_industrial_loan_facts() {
    let baseline = baseline();
    let mut invalid = baseline.clone();
    let company = invalid["company_operations"]["companies"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    let loans = company["books"]["Industrial"]["loans"]
        .as_object_mut()
        .unwrap();
    assert!(!loans.is_empty());
    loans.values_mut().next().unwrap()["outstanding"] =
        serde_json::to_value(engine::accounting::AccountingAmount::from_cents(-1)).unwrap();
    let error = decode_error(invalid);
    assert!(error.contains("industrial credit state"), "{error}");
    let mut overflowing = baseline;
    let company = overflowing["company_operations"]["companies"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    let loans = company["books"]["Industrial"]["loans"]
        .as_object_mut()
        .unwrap();
    let (original_id, original_loan) = loans.iter().next().unwrap();
    let original_id = original_id.clone();
    let mut extra = original_loan.clone();
    loans.get_mut(&original_id).unwrap()["outstanding"] =
        serde_json::to_value(engine::accounting::AccountingAmount::from_cents(1)).unwrap();
    extra["outstanding"] = serde_json::to_value(engine::accounting::AccountingAmount::MAX).unwrap();
    loans.insert("synthetic-overflow-loan".into(), extra);
    let contracts = company["books"]["Industrial"]["contracts"]["contracts"]
        .as_object_mut()
        .unwrap();
    let mut extra_contract = contracts[&original_id].clone();
    extra_contract["id"] = serde_json::json!("synthetic-overflow-loan");
    extra_contract["principal"] =
        serde_json::to_value(engine::accounting::AccountingAmount::MAX).unwrap();
    contracts.insert("synthetic-overflow-loan".into(), extra_contract);
    let error = decode_error(overflowing);
    assert!(error.contains("amount overflow in add"), "{error}");
}

#[test]
fn complete_save_decoding_rejects_zero_and_overflowing_plan_horizons() {
    for (created, horizon, reason) in [
        (0, 0, "invalid horizon"),
        (u64::MAX, 2, "horizon overflows"),
    ] {
        let mut wire = baseline();
        let plan = wire["plans"]["plans"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        plan["created_trading_day"] = serde_json::json!(created);
        plan["horizon_trading_days"] = serde_json::json!(horizon);
        let error = decode_error(wire);
        assert!(error.contains(reason), "{error}");
    }
}

#[test]
fn complete_save_decoding_rejects_duplicate_operating_due_id() {
    let mut wire = baseline();
    let scheduler = &mut wire["company_operations"]["scheduler"];
    let pending = scheduler["pending"].as_array_mut().unwrap();
    assert!(!pending.is_empty());
    let mut duplicate = pending[0].clone();
    duplicate["key"] = serde_json::json!("synthetic-duplicate-id");
    duplicate["due_date"] = serde_json::json!("2030-01-09");
    pending.push(duplicate);
    pending.sort_by_key(|due| {
        (
            due["due_date"].as_str().unwrap().to_owned(),
            due["id"].as_u64().unwrap(),
        )
    });
    let error = decode_error(wire);
    assert!(error.contains("duplicate due id"), "{error}");
}

#[test]
fn complete_save_restore_rejects_duplicate_clock_id_and_future_due_outside_policy() {
    let baseline = baseline();
    let pending = baseline["civil_clock"]["pending_due"].as_array().unwrap();
    assert!(!pending.is_empty());
    let mut duplicate = baseline.clone();
    duplicate["civil_clock"]["pending_due"]
        .as_array_mut()
        .unwrap()
        .push(pending[0].clone());
    let error = restore_error(duplicate);
    assert!(error.contains("duplicate due id"), "{error}");
    let mut outside = baseline;
    let mut future_due = outside["civil_clock"]["pending_due"][0].clone();
    let next_seq = outside["civil_clock"]["next_due_seq"].as_u64().unwrap();
    future_due["id"] = serde_json::json!(next_seq);
    future_due["due_date"] = serde_json::json!("2100-01-01");
    outside["civil_clock"]["next_due_seq"] = serde_json::json!(next_seq + 1);
    outside["civil_clock"]["pending_due"]
        .as_array_mut()
        .unwrap()
        .push(future_due);
    let error = restore_error(outside);
    assert!(error.contains("2100-01-01"), "{error}");
    assert!(
        !error.contains("mirror"),
        "不应由后续镜像校验误核销：{error}"
    );
}
