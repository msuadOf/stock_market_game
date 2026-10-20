use engine::plans::PlanOpen;
use engine::session::{GameSession, SaveDecodeLimits, SessionError, decode_save_slot};
use engine::{AccountId, OpinionSource, PlanOpinion, PlanTarget, Side, StockId, Urgency};

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
                    code: StockId(0),
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
            let wire = serde_json::to_value(save).unwrap();
            // 夹具完整性：未篡改基线也必须通过完整 JSON 解码——后续各例的
            // fault2080 断言才可归因于本例篡改而非基线腐化。
            decode_save_slot(
                &serde_json::to_vec(&wire).unwrap(),
                &SaveDecodeLimits::default(),
            )
            .expect("未篡改基线必须通过完整 SaveSlot JSON 解码");
            wire
        })
        .clone()
}

/// 完整解码边界断言：语义篡改（serde 自定义校验拒绝）统一落 fault2080
/// （8447ecd6 错误契约数值化；语义错误的专用码登记为 C 批待办）。
fn assert_decode_rejects_with_fault2080(wire: &serde_json::Value) {
    match decode_save_slot(
        &serde_json::to_vec(wire).unwrap(),
        &SaveDecodeLimits::default(),
    ) {
        Err(SessionError::InvalidSave(fault)) => assert_eq!(
            fault.code, 2080,
            "语义篡改须在完整解码边界以 fault2080 拒绝：{fault}"
        ),
        Err(other) => panic!("完整解码须以 InvalidSave 拒绝，实际：{other}"),
        Ok(_) => panic!("损坏状态不应通过完整 SaveSlot 解码"),
    }
}

/// 库级 serde 定位断言：语义校验在 Deserialize 内以自定义错误拒绝，其文本
/// 只在库级 `serde_json` 错误面保留——在此钉住"是哪条校验拒绝"，替代
/// 数值化前的边界文本断言（不弱化定位）。
fn serde_rejection_text(wire: &serde_json::Value) -> String {
    serde_json::from_value::<engine::session::SaveSlot>(wire.clone())
        .expect_err("库级 serde 必须拒绝语义篡改")
        .to_string()
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
    assert_decode_rejects_with_fault2080(&invalid);
    assert!(serde_rejection_text(&invalid).contains("industrial credit state"));
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
    assert_decode_rejects_with_fault2080(&overflowing);
    assert!(serde_rejection_text(&overflowing).contains("amount overflow in add"));
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
        assert_decode_rejects_with_fault2080(&wire);
        assert!(serde_rejection_text(&wire).contains(reason));
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
    assert_decode_rejects_with_fault2080(&wire);
    assert!(serde_rejection_text(&wire).contains("duplicate due id"));
}

#[test]
fn complete_save_decoding_rejects_civil_clock_due_mirror_fields() {
    // 公司 due 单一 authority：时钟侧携带旧镜像字段（pending_due/next_due_seq）
    // 在解码边界显式失败，不接受静默忽略的旧格式。
    for (field, value) in [
        ("pending_due", serde_json::json!([])),
        ("next_due_seq", serde_json::json!(1)),
    ] {
        let mut wire = baseline();
        wire["civil_clock"][field] = value;
        let decoded = decode_save_slot(
            &serde_json::to_vec(&wire).unwrap(),
            &SaveDecodeLimits::default(),
        );
        assert!(
            matches!(decoded, Err(engine::session::SessionError::InvalidSave(ref fault)) if fault.code == 2080),
            "旧公司镜像字段 {field} 必须在 JSON 解码边界以结构化 2080 拒绝"
        );
    }
}

#[test]
fn complete_save_restore_rejects_scheduler_due_beyond_runtime_ceiling() {
    // 公司 due 单一 authority 后，日历适用范围校验必须由会话边界承担：
    // 待办 due 晚于冻结政策 runtime_max_end（2099-12-31）= 失去恰好一次
    // 派发机会的越界待办，恢复显式拒绝（fault2234）。
    let mut wire = baseline();
    let scheduler = &mut wire["company_operations"]["scheduler"];
    let pending = scheduler["pending"].as_array_mut().unwrap();
    assert!(pending.len() >= 2, "基线存档须有多条待办");
    // 取日期最大的一条（排序仍保持：2100-01-01 仍是最大）。
    pending
        .iter_mut()
        .max_by_key(|due| due["due_date"].as_str().unwrap().to_owned())
        .unwrap()["due_date"] = serde_json::json!("2100-01-01");
    let save = decode_save_slot(
        &serde_json::to_vec(&wire).unwrap(),
        &SaveDecodeLimits::default(),
    )
    .unwrap();
    match GameSession::restore(&save) {
        Err(engine::session::SessionError::InvalidSave(fault)) => {
            assert_eq!(fault.code, 2234, "越界 due 用 fault2234 拒绝：{fault}");
        }
        Ok(_) => panic!("越界待办必须被恢复边界拒绝，实际恢复成功"),
        Err(other) => panic!("越界 due 应以 fault2234 拒绝，实际：{other}"),
    }
}
