//! 账户局部 PlanId 的身份、游标与严格边界契约。
use engine::plans::{OwnerSequence, PlanBook, PlanError, PlanId, PlanOpen, PlanPolicy};
use engine::{AccountId, OpinionSource, PlanOpinion, PlanTarget, Side, StockId, Urgency};

fn open(owner: u64, stock: u32) -> PlanOpen {
    PlanOpen {
        account: AccountId(owner),
        code: StockId(stock),
        direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion {
            signal_score_bp: 3000,
            source: OpinionSource::Blended,
        },
        confidence_bp: 8000,
        urgency: Urgency::Normal,
        horizon_trading_days: 5,
        created_trading_day: 0,
    }
}

#[test]
fn independent_owners_allocate_zero_and_keep_identity_after_round_trip() {
    let mut book = PlanBook::default();
    let a = book.create(open(1, 0)).unwrap();
    let b = book.create(open(2, 0)).unwrap();
    assert_eq!(
        a,
        PlanId {
            owner: AccountId(1),
            sequence: 0
        }
    );
    assert_eq!(
        b,
        PlanId {
            owner: AccountId(2),
            sequence: 0
        }
    );
    let mut restored: PlanBook =
        serde_json::from_slice(&serde_json::to_vec(&book).unwrap()).unwrap();
    assert_eq!(restored, book);
    assert_eq!(restored.create(open(1, 1)).unwrap().sequence, 1);
    assert_eq!(restored.plan(a).unwrap().account(), AccountId(1));
    assert_eq!(restored.plan(b).unwrap().account(), AccountId(2));
}

#[test]
fn maximum_sequence_is_allocated_once_and_failure_does_not_publish() {
    let mut book = PlanBook::from_parts(
        PlanPolicy::default(),
        vec![OwnerSequence {
            owner: AccountId(1),
            next_sequence: Some(u64::MAX),
        }],
        vec![],
    )
    .unwrap();
    let mut bad = open(1, 0);
    bad.horizon_trading_days = 0;
    let before = book.clone();
    assert!(book.create(bad).is_err());
    assert_eq!(book, before);
    assert_eq!(book.create(open(1, 0)).unwrap().sequence, u64::MAX);
    let before = book.clone();
    assert_eq!(
        book.create(open(1, 1)),
        Err(PlanError::PlanSequenceExhausted)
    );
    assert_eq!(book, before);
    let mut restored: PlanBook =
        serde_json::from_slice(&serde_json::to_vec(&book).unwrap()).unwrap();
    assert_eq!(
        restored.create(open(1, 1)),
        Err(PlanError::PlanSequenceExhausted)
    );
}

#[test]
fn strict_identity_wire_accepts_full_u64_and_rejects_legacy_or_noncanonical() {
    let id = PlanId {
        owner: AccountId(u64::MAX),
        sequence: u64::MAX,
    };
    let encoded = serde_json::json!({"owner":u64::MAX.to_string(),"sequence":u64::MAX.to_string()});
    assert_eq!(serde_json::to_value(id).unwrap(), encoded);
    assert_eq!(serde_json::from_value::<PlanId>(encoded).unwrap(), id);
    for invalid in [
        serde_json::json!(0),
        serde_json::json!({"owner":"1","sequence":0}),
        serde_json::json!({"owner":"01","sequence":"0"}),
        serde_json::json!({"owner":"1","sequence":"01"}),
        serde_json::json!({"owner":"1","sequence":"18446744073709551616"}),
        serde_json::json!({"owner":"1","sequence":"0","extra":0}),
    ] {
        assert!(serde_json::from_value::<PlanId>(invalid).is_err());
    }
}

#[test]
fn restore_rejects_duplicate_identity_cursor_missing_cursor_and_owner_mismatch() {
    let mut book = PlanBook::default();
    book.create(open(1, 0)).unwrap();
    let original = serde_json::to_value(&book).unwrap();
    for field in ["plans", "owner_sequences"] {
        let mut bad = original.clone();
        let value = bad[field][0].clone();
        bad[field].as_array_mut().unwrap().push(value);
        assert!(serde_json::from_value::<PlanBook>(bad).is_err());
    }
    let mut missing = original.clone();
    missing["owner_sequences"] = serde_json::json!([]);
    assert!(serde_json::from_value::<PlanBook>(missing).is_err());
    let mut mismatch = original.clone();
    mismatch["plans"][0]["plan_id"]["owner"] = serde_json::json!("2");
    assert!(serde_json::from_value::<PlanBook>(mismatch).is_err());
    for bad in [
        serde_json::json!({"policy":PlanPolicy::default(),"next_plan_seq":1,"plans":{}}),
        serde_json::json!({"policy":PlanPolicy::default(),"owner_sequences":[{"owner":"1"}],"plans":[]}),
    ] {
        assert!(serde_json::from_value::<PlanBook>(bad).is_err());
    }
    let mut behind = original;
    behind["owner_sequences"][0]["next_sequence"] = serde_json::json!("0");
    assert!(serde_json::from_value::<PlanBook>(behind).is_err());
}

#[test]
fn terminal_history_and_high_cursors_survive_complete_parts_transfer() {
    let policy = PlanPolicy {
        reverse_revision_threshold_bp: 1234,
        review_signal_delta_bp: 432,
        review_price_change_bp: 123,
    };
    let mut book = PlanBook::from_parts(
        policy,
        vec![
            OwnerSequence {
                owner: AccountId(7),
                next_sequence: Some(u64::MAX - 1),
            },
            OwnerSequence {
                owner: AccountId(9),
                next_sequence: None,
            },
        ],
        vec![],
    )
    .unwrap();
    let first = book.create(open(7, 0)).unwrap();
    book.apply(
        first,
        engine::PlanEvent::Terminated {
            reason: engine::TerminationReason::Cancelled,
            trading_day: 0,
        },
    )
    .unwrap();
    let (saved_policy, sequences, plans) = book.clone().into_parts();
    assert_eq!(saved_policy, policy);
    let mut restored = PlanBook::from_parts(saved_policy, sequences, plans).unwrap();
    assert_eq!(restored, book);
    assert!(restored.plan(first).unwrap().is_terminal());
    let second = restored.create(open(7, 0)).unwrap();
    assert_eq!(second.sequence, u64::MAX);
    assert_ne!(second, first);
    assert_eq!(
        restored.create(open(9, 0)),
        Err(PlanError::PlanSequenceExhausted)
    );
}

#[test]
fn constructor_and_restore_validate_identity_before_building_maps() {
    let policy = PlanPolicy::default();
    let id = PlanId {
        owner: AccountId(1),
        sequence: 0,
    };
    assert_eq!(
        engine::TradingPlan::from_open(id, open(2, 0), &policy),
        Err(PlanError::PlanOwnerMismatch {
            plan_id: id,
            account: AccountId(2)
        })
    );
    let plan = engine::TradingPlan::from_open(id, open(1, 0), &policy).unwrap();
    let cursors = vec![OwnerSequence {
        owner: AccountId(1),
        next_sequence: Some(1),
    }];
    assert_eq!(
        PlanBook::from_parts(policy, cursors.clone(), vec![plan.clone(), plan.clone()]),
        Err(PlanError::DuplicatePlanIdentity { plan_id: id })
    );
    assert_eq!(
        PlanBook::from_parts(policy, vec![], vec![plan.clone()]),
        Err(PlanError::InvalidPlanSequence {
            owner: AccountId(1),
            next_sequence: None
        })
    );
    assert_eq!(
        PlanBook::from_parts(
            policy,
            vec![
                OwnerSequence {
                    owner: AccountId(1),
                    next_sequence: None
                },
                cursors[0]
            ],
            vec![]
        ),
        Err(PlanError::PlanWriterOwnershipViolation {
            owner: AccountId(1)
        })
    );
    let mut encoded = serde_json::to_value(plan).unwrap();
    encoded["account"] = serde_json::json!("2");
    assert!(serde_json::from_value::<engine::TradingPlan>(encoded).is_err());
}

#[test]
fn identity_order_and_error_context_keep_both_full_width_components() {
    let before = PlanId {
        owner: AccountId(0),
        sequence: u64::MAX,
    };
    let after = PlanId {
        owner: AccountId(1),
        sequence: 0,
    };
    assert!(before < after);
    let id = PlanId {
        owner: AccountId(u64::MAX),
        sequence: (1 << 63) + 1,
    };
    assert_eq!(
        id.error_context(),
        vec![u32::MAX as u64, u32::MAX as u64, 1 << 31, 1]
    );
    assert_eq!(
        id.error_context_with([7]),
        vec![u32::MAX as u64, u32::MAX as u64, 1 << 31, 1, 7]
    );
}

#[test]
fn export_bindings_plan_identity() {
    let config = ts_rs::Config::from_env();
    <PlanBook as ts_rs::TS>::export_all(&config).unwrap();
}

#[test]
fn identity_and_owner_cursor_wire_cover_javascript_boundary_without_caps() {
    for value in [0, (1_u64 << 53) - 1, 1 << 53, (1 << 53) + 1, u64::MAX] {
        let id = PlanId {
            owner: AccountId(value),
            sequence: value,
        };
        let wire = serde_json::json!({"owner":value.to_string(),"sequence":value.to_string()});
        assert_eq!(serde_json::to_value(id).unwrap(), wire);
        assert_eq!(serde_json::from_value::<PlanId>(wire).unwrap(), id);
        let cursor = OwnerSequence {
            owner: AccountId(value),
            next_sequence: Some(value),
        };
        let wire = serde_json::json!({"owner":value.to_string(),"next_sequence":value.to_string()});
        assert_eq!(serde_json::to_value(cursor).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<OwnerSequence>(wire).unwrap(),
            cursor
        );
    }
    let exhausted = OwnerSequence {
        owner: AccountId(u64::MAX),
        next_sequence: None,
    };
    assert_eq!(
        serde_json::to_value(exhausted).unwrap(),
        serde_json::json!({"owner":u64::MAX.to_string(),"next_sequence":null})
    );
    assert_eq!(
        serde_json::from_value::<OwnerSequence>(serde_json::to_value(exhausted).unwrap()).unwrap(),
        exhausted
    );
    for bad in [
        serde_json::json!(0),
        serde_json::json!(""),
        serde_json::json!("+1"),
        serde_json::json!("-1"),
        serde_json::json!("-0"),
        serde_json::json!(" 1"),
        serde_json::json!("1 "),
        serde_json::json!("1.0"),
        serde_json::json!("1e3"),
        serde_json::json!("01"),
        serde_json::json!("18446744073709551616"),
    ] {
        assert!(serde_json::from_value::<PlanId>(
            serde_json::json!({"owner":"1","sequence":bad.clone()})
        )
        .is_err());
        assert!(serde_json::from_value::<OwnerSequence>(
            serde_json::json!({"owner":"1","next_sequence":bad})
        )
        .is_err());
    }
    for bad in [
        serde_json::json!({"owner":"1"}),
        serde_json::json!({"next_sequence":"0"}),
        serde_json::json!({"owner":"1","next_sequence":"0","extra":0}),
    ] {
        assert!(serde_json::from_value::<OwnerSequence>(bad).is_err());
    }
}
