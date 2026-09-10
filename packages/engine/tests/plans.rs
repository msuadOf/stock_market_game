//! W3-Task 21：可跨日的个人交易计划状态机（K6 契约）集成测试。
//!
//! 金样覆盖：无变化观察保持方向与真实成交、正向/反向（跨门槛）修订、暂停/恢复、
//! 各终止原因、真实完成、跨日存档往返、日终仅结束子单生命周期。
//! 失败路径覆盖：超目标成交、低于已成交的修订缺理由、反向未跨门槛、过期复活、
//! 接受当成交、未知 PlanId、重复活跃计划、时间回拨、超有效期、非法开户字段。

use std::collections::BTreeMap;

use engine::plans::{PlanBook, PlanEvent, PlanOpen};
use engine::{
    AccountId, OpinionSource, OrderId, PauseReason, PlanError, PlanId, PlanOpinion, PlanPolicy,
    PlanRevision, PlanStatus, PlanTarget, ResumeReason, RevisionReason, Side, StockCode,
    TerminationReason, TradingPlan, Urgency,
};

fn buy_open() -> PlanOpen {
    PlanOpen {
        account: AccountId(7),
        code: StockCode("600101".into()),
        direction: Side::Buy,
        target: PlanTarget::ShareCount(1000),
        opinion: PlanOpinion {
            signal_score_bp: 2500,
            source: OpinionSource::Fundamental,
        },
        confidence_bp: 6000,
        urgency: Urgency::Normal,
        horizon_trading_days: 20,
        created_trading_day: 0,
    }
}

fn sell_open(code: &str) -> PlanOpen {
    PlanOpen {
        account: AccountId(9),
        code: StockCode(code.into()),
        direction: Side::Sell,
        target: PlanTarget::ShareCount(800),
        opinion: PlanOpinion {
            signal_score_bp: -2600,
            source: OpinionSource::Technical,
        },
        confidence_bp: 5000,
        urgency: Urgency::Patient,
        horizon_trading_days: 5,
        created_trading_day: 0,
    }
}

fn book_with_buy_plan() -> (PlanBook, PlanId) {
    let mut book = PlanBook::default();
    let id = book.create(buy_open()).expect("open buy plan");
    (book, id)
}

fn link_and_fill(book: &mut PlanBook, id: PlanId, order_seq: u64, qty: u32, trading_day: u64) {
    book.apply(
        id,
        PlanEvent::ChildOrderAccepted {
            order_id: OrderId(order_seq),
            trading_day,
        },
    )
    .expect("accept child order");
    book.apply(
        id,
        PlanEvent::ChildOrderFilled {
            order_id: OrderId(order_seq),
            qty,
            trading_day,
        },
    )
    .expect("record real fill");
}

fn forward_revision(target: u32, trading_day: u64) -> PlanRevision {
    PlanRevision {
        reason: RevisionReason::SignalShift,
        trading_day,
        direction: Side::Buy,
        target: PlanTarget::ShareCount(target),
        opinion: PlanOpinion {
            signal_score_bp: 3600,
            source: OpinionSource::Fundamental,
        },
        confidence_bp: 6500,
        urgency: Urgency::Normal,
        below_filled_rationale: None,
    }
}

// ---------------------------------------------------------------------------
// 金样：continue / revise / pause / resume / terminate / 真实完成 / 跨日
// ---------------------------------------------------------------------------

/// K6：平静且信息/风险/约束无变化时，连续观察保持方向、目标与累计真实成交，
/// 不在每观察时重抽买卖，也不推进版本。
#[test]
fn no_change_observations_keep_direction_and_filled_progress() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 400, 0);

    for day in 1..=3u64 {
        book.apply(id, PlanEvent::ObservedNoChange { trading_day: day })
            .expect("calm observation is always applicable");
    }

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.direction, Side::Buy);
    assert_eq!(plan.target, PlanTarget::ShareCount(1000));
    assert_eq!(plan.filled_qty, 400);
    assert_eq!(plan.status, PlanStatus::Active);
    assert_eq!(plan.version, 1);
    assert_eq!(plan.last_revision, None);
}

/// 同向修订只更新目标与观点，版本 +1 并记录原因；已成交进度保持。
#[test]
fn forward_revision_raises_target_with_version_and_reason() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 400, 0);

    book.apply(
        id,
        PlanEvent::Revised {
            revision: forward_revision(1600, 1),
        },
    )
    .expect("forward revision applies");

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.target, PlanTarget::ShareCount(1600));
    assert_eq!(
        plan.filled_qty, 400,
        "forward revision must keep real fills"
    );
    assert_eq!(plan.version, 2);
    assert_eq!(
        plan.last_revision.as_ref().map(|r| r.reason),
        Some(RevisionReason::SignalShift)
    );
    assert_eq!(plan.opinion.signal_score_bp, 3600);
    assert_eq!(plan.review.last_review_signal_score_bp, 3600);
    assert_eq!(plan.status, PlanStatus::Active);
}

/// K5a 迟滞：反向修订必须越过另一侧门槛（默认 ±2000bp）。跨过则翻转方向、
/// 以新方向重新累计成交进度；旧方向的真实成交仍在账户里，不在此重复记账。
#[test]
fn reverse_revision_crossing_opposite_threshold_flips_direction_and_restarts_leg_progress() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 400, 0);

    let revision = PlanRevision {
        reason: RevisionReason::SignalShift,
        trading_day: 2,
        direction: Side::Sell,
        target: PlanTarget::ShareCount(1200),
        opinion: PlanOpinion {
            signal_score_bp: -2500,
            source: OpinionSource::Blended,
        },
        confidence_bp: 5500,
        urgency: Urgency::Urgent,
        below_filled_rationale: None,
    };
    book.apply(id, PlanEvent::Revised { revision })
        .expect("reverse revision crossing -2000bp applies");

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.direction, Side::Sell);
    assert_eq!(plan.target, PlanTarget::ShareCount(1200));
    assert_eq!(plan.filled_qty, 0, "new leg progress starts at zero");
    assert_eq!(plan.urgency, Urgency::Urgent);
    assert_eq!(plan.version, 2);
    assert_eq!(plan.status, PlanStatus::Active);
    assert_eq!(
        plan.active_child_order_id, None,
        "reverse revision drops the old child"
    );
}

/// 修订目标恰好等于已成交时，剩余工作为零：这是真实完成，标 Completed。
#[test]
fn revision_to_exactly_filled_completes_the_plan() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 400, 0);

    book.apply(
        id,
        PlanEvent::Revised {
            revision: forward_revision(400, 1),
        },
    )
    .expect("revision to exactly filled applies");

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.status, PlanStatus::Completed);
    assert_eq!(plan.filled_qty, 400);
}

/// 暂停与恢复都要求显式原因；恢复后可继续成交。
#[test]
fn pause_and_resume_record_explicit_reasons() {
    let (mut book, id) = book_with_buy_plan();

    book.apply(
        id,
        PlanEvent::Paused {
            reason: PauseReason::IntradayDropAcceleration,
            trading_day: 0,
        },
    )
    .expect("pause with reason");
    assert_eq!(
        book.plan(id).unwrap().status,
        PlanStatus::Paused {
            reason: PauseReason::IntradayDropAcceleration,
        }
    );

    // 暂停期间子单仍可能真实成交（现实不可拒绝），但计划不会因此复活报价。
    link_and_fill(&mut book, id, 100, 300, 0);
    assert_eq!(book.plan(id).unwrap().filled_qty, 300);

    book.apply(
        id,
        PlanEvent::Resumed {
            reason: ResumeReason::TriggerCleared,
            trading_day: 1,
        },
    )
    .expect("resume with reason");
    let plan = book.plan(id).unwrap();
    assert_eq!(plan.status, PlanStatus::Active);
    assert_eq!(plan.last_resume, Some(ResumeReason::TriggerCleared));
}

/// 有效期届满：覆盖最后一个有效交易日的日终将计划以 HorizonExpired 终止；
/// 中途日终不终止。
#[test]
fn horizon_expiry_at_day_end_terminates_with_explicit_reason() {
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            horizon_trading_days: 5,
            ..buy_open()
        })
        .expect("open plan");

    book.apply(id, PlanEvent::TradingDayEnded { trading_day: 3 })
        .expect("mid-horizon day end");
    assert_eq!(book.plan(id).unwrap().status, PlanStatus::Active);

    book.apply(id, PlanEvent::TradingDayEnded { trading_day: 4 })
        .expect("day end of the last valid day");
    assert_eq!(
        book.plan(id).unwrap().status,
        PlanStatus::Terminated {
            reason: TerminationReason::HorizonExpired,
        }
    );
}

/// 无资金与主动撤销都以 Terminated + 具体原因落地，绝不冒充 Completed。
#[test]
fn funds_unavailable_and_cancelled_terminate_with_explicit_reasons() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 400, 0);

    book.apply(
        id,
        PlanEvent::Terminated {
            reason: TerminationReason::FundsUnavailable,
            trading_day: 2,
        },
    )
    .expect("terminate for funds");
    assert_eq!(
        book.plan(id).unwrap().status,
        PlanStatus::Terminated {
            reason: TerminationReason::FundsUnavailable,
        }
    );

    let (mut book2, id2) = book_with_buy_plan();
    book2
        .apply(
            id2,
            PlanEvent::Terminated {
                reason: TerminationReason::Cancelled,
                trading_day: 1,
            },
        )
        .expect("terminate by cancel");
    assert_eq!(
        book2.plan(id2).unwrap().status,
        PlanStatus::Terminated {
            reason: TerminationReason::Cancelled,
        }
    );
    assert_eq!(book2.plan(id2).unwrap().filled_qty, 0);
}

/// 真实完成：累计真实成交到达目标才标 Completed；委托被接受绝不推进。
#[test]
fn real_fills_reaching_share_target_complete_the_plan() {
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            target: PlanTarget::ShareCount(600),
            ..buy_open()
        })
        .expect("open plan");

    link_and_fill(&mut book, id, 100, 200, 0);
    assert_eq!(book.plan(id).unwrap().status, PlanStatus::Active);
    // 旧子单生命周期结束后换下一张子单继续执行。
    link_and_fill(&mut book, id, 101, 400, 1);

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.filled_qty, 600);
    assert_eq!(plan.status, PlanStatus::Completed);
}

/// 日终只结束子单生命周期：清除子单引用，但计划保持原状态、方向与成交进度，
/// 绝不把个人计划标 Completed。
#[test]
fn day_end_ends_only_child_order_lifecycle_not_the_plan() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 400, 0);

    book.apply(id, PlanEvent::TradingDayEnded { trading_day: 0 })
        .expect("day end");

    let plan = book.plan(id).unwrap();
    assert_eq!(
        plan.active_child_order_id, None,
        "child order link ends at day end"
    );
    assert_eq!(plan.status, PlanStatus::Active);
    assert_eq!(plan.direction, Side::Buy);
    assert_eq!(plan.filled_qty, 400);
    assert_eq!(plan.target, PlanTarget::ShareCount(1000));
}

/// 跨日状态持久化：序列化往返后状态逐字段一致，且后续转移语义完全相同。
#[test]
fn cross_day_state_persists_through_serde_round_trip() {
    let mut book = PlanBook::default();
    let id = book.create(buy_open()).expect("open buy plan");
    link_and_fill(&mut book, id, 100, 400, 0);
    let sell_id = book.create(sell_open("000812")).expect("open sell plan");
    book.apply(
        sell_id,
        PlanEvent::Paused {
            reason: PauseReason::RiskPressure,
            trading_day: 0,
        },
    )
    .expect("pause sell plan");

    let bytes = serde_json::to_vec(&book).expect("serialize plan book");
    let restored: PlanBook = serde_json::from_slice(&bytes).expect("deserialize plan book");
    assert_eq!(restored, book, "round trip must preserve every field");

    // 恢复后的书对同一后续事件产生同一结果（跨日语义不漂移）。
    let mut restored = restored;
    let status = restored
        .apply(
            id,
            PlanEvent::ChildOrderAccepted {
                order_id: OrderId(200),
                trading_day: 1,
            },
        )
        .expect("post-restore acceptance");
    book.apply(
        id,
        PlanEvent::ChildOrderAccepted {
            order_id: OrderId(200),
            trading_day: 1,
        },
    )
    .expect("original acceptance");
    assert_eq!(status, book.plan(id).unwrap().status);
    assert_eq!(restored, book);

    // 未知字段拒绝：存档校验不静默吞掉多余字段。
    let mut value = serde_json::to_value(book.plan(id).unwrap()).unwrap();
    value
        .as_object_mut()
        .expect("plan serializes to an object")
        .insert("mystery_field".into(), serde_json::Value::Bool(true));
    serde_json::from_value::<TradingPlan>(value).expect_err("unknown fields must be rejected");
}

/// 超目标真实成交（零股卖出等）：真实成交如实入账，但计划必须以显式原因终止。
#[test]
fn excess_real_fill_is_recorded_honestly_and_terminates_with_reason() {
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            target: PlanTarget::ShareCount(500),
            ..buy_open()
        })
        .expect("open plan");
    link_and_fill(&mut book, id, 100, 400, 0);

    book.apply(
        id,
        PlanEvent::ChildOrderExcessFilled {
            order_id: OrderId(100),
            qty: 200,
            trading_day: 0,
        },
    )
    .expect("excess fill with explicit handling");

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.filled_qty, 600, "real fill is recorded honestly");
    assert_eq!(
        plan.status,
        PlanStatus::Terminated {
            reason: TerminationReason::FilledBeyondTarget,
        }
    );
}

/// 修订目标低于已成交且携带终止理由：目标如实下调，计划以该理由终止。
#[test]
fn revision_below_filled_with_rationale_applies_and_terminates() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 600, 0);

    book.apply(
        id,
        PlanEvent::Revised {
            revision: PlanRevision {
                reason: RevisionReason::RiskTriggered,
                trading_day: 1,
                direction: Side::Buy,
                target: PlanTarget::ShareCount(400),
                opinion: PlanOpinion {
                    signal_score_bp: 2100,
                    source: OpinionSource::Fundamental,
                },
                confidence_bp: 3000,
                urgency: Urgency::Normal,
                below_filled_rationale: Some(TerminationReason::FundsUnavailable),
            },
        },
    )
    .expect("below-filled revision with rationale");

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.target, PlanTarget::ShareCount(400));
    assert_eq!(plan.version, 2);
    assert_eq!(
        plan.status,
        PlanStatus::Terminated {
            reason: TerminationReason::FundsUnavailable,
        }
    );
}

/// 计划终止后同一账户+股票可开新计划；PlanId 不复用、单调递增，旧计划仍可追溯。
#[test]
fn terminated_plan_slot_allows_a_new_plan_with_a_new_stable_plan_id() {
    let (mut book, id) = book_with_buy_plan();
    let first_id = id;
    book.apply(
        id,
        PlanEvent::Terminated {
            reason: TerminationReason::Cancelled,
            trading_day: 0,
        },
    )
    .expect("terminate first plan");
    assert!(
        book.active_plan(AccountId(7), &StockCode("600101".into()))
            .is_none(),
        "terminated plan is not the active plan"
    );

    let second_id = book.create(buy_open()).expect("re-open after termination");
    assert_ne!(second_id, first_id, "plan ids are never reused");
    assert_eq!(second_id, PlanId(1));
    assert_eq!(
        book.plan(first_id).unwrap().status,
        PlanStatus::Terminated {
            reason: TerminationReason::Cancelled,
        }
    );
    assert_eq!(
        book.active_plan(AccountId(7), &StockCode("600101".into()))
            .map(|p| p.plan_id),
        Some(second_id)
    );

    // 新计划活跃期间，同一账户+股票再开计划仍被拒绝。
    assert!(matches!(
        book.create(buy_open()).unwrap_err(),
        PlanError::DuplicateActivePlan { .. }
    ));
}

// ---------------------------------------------------------------------------
// 失败路径：全部为类型化拒绝，不静默 fallback
// ---------------------------------------------------------------------------

#[test]
fn fill_beyond_target_is_rejected() {
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            target: PlanTarget::ShareCount(500),
            ..buy_open()
        })
        .expect("open plan");
    link_and_fill(&mut book, id, 100, 400, 0);

    let err = book
        .apply(
            id,
            PlanEvent::ChildOrderFilled {
                order_id: OrderId(100),
                qty: 200,
                trading_day: 0,
            },
        )
        .expect_err("fill beyond target must be rejected");
    assert!(matches!(err, PlanError::FillExceedsTarget { .. }));

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.filled_qty, 400, "rejected fill leaves state unchanged");
    assert_eq!(plan.status, PlanStatus::Active);
}

#[test]
fn revision_below_filled_without_termination_rationale_is_rejected() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 600, 0);

    let err = book
        .apply(
            id,
            PlanEvent::Revised {
                revision: forward_revision(400, 1),
            },
        )
        .expect_err("below-filled revision without rationale must be rejected");
    assert!(matches!(
        err,
        PlanError::RevisionBelowFilledRequiresRationale { .. }
    ));
    let plan = book.plan(id).unwrap();
    assert_eq!(plan.version, 1, "rejected revision does not bump version");
    assert_eq!(plan.target, PlanTarget::ShareCount(1000));
}

#[test]
fn reverse_revision_below_opposite_threshold_is_rejected() {
    let (mut book, id) = book_with_buy_plan();

    let err = book
        .apply(
            id,
            PlanEvent::Revised {
                revision: PlanRevision {
                    reason: RevisionReason::SignalShift,
                    trading_day: 1,
                    direction: Side::Sell,
                    target: PlanTarget::ShareCount(1200),
                    opinion: PlanOpinion {
                        signal_score_bp: -1500,
                        source: OpinionSource::Blended,
                    },
                    confidence_bp: 5500,
                    urgency: Urgency::Normal,
                    below_filled_rationale: None,
                },
            },
        )
        .expect_err("reverse without crossing -2000bp must be rejected");
    assert!(matches!(
        err,
        PlanError::ReverseRevisionBelowThreshold { .. }
    ));
    let plan = book.plan(id).unwrap();
    assert_eq!(plan.direction, Side::Buy, "direction is not flipped");
    assert_eq!(plan.version, 1);
}

/// 重复相同输入的修订（方向/目标/紧迫度/信心/观点全一致）被拒绝：不在零附近翻单。
#[test]
fn unchanged_revision_is_rejected() {
    let (mut book, id) = book_with_buy_plan();

    let err = book
        .apply(
            id,
            PlanEvent::Revised {
                revision: PlanRevision {
                    reason: RevisionReason::SignalShift,
                    trading_day: 1,
                    direction: Side::Buy,
                    target: PlanTarget::ShareCount(1000),
                    opinion: PlanOpinion {
                        signal_score_bp: 2500,
                        source: OpinionSource::Fundamental,
                    },
                    confidence_bp: 6000,
                    urgency: Urgency::Normal,
                    below_filled_rationale: None,
                },
            },
        )
        .expect_err("identical revision is a no-op and must be rejected");
    assert!(matches!(err, PlanError::UnchangedRevision { .. }));
}

#[test]
fn expired_plan_cannot_revive() {
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            horizon_trading_days: 5,
            ..buy_open()
        })
        .expect("open plan");
    book.apply(id, PlanEvent::TradingDayEnded { trading_day: 4 })
        .expect("horizon expires at day end");

    for event in [
        PlanEvent::ObservedNoChange { trading_day: 5 },
        PlanEvent::Resumed {
            reason: ResumeReason::TriggerCleared,
            trading_day: 5,
        },
        PlanEvent::Revised {
            revision: forward_revision(2000, 5),
        },
        PlanEvent::ChildOrderAccepted {
            order_id: OrderId(300),
            trading_day: 5,
        },
    ] {
        let err = book
            .apply(id, event)
            .expect_err("terminal plan rejects every event");
        assert!(
            matches!(err, PlanError::InvalidTransition { .. }),
            "expected InvalidTransition, got {err:?}"
        );
    }
    // 显式到期事件只接受真正越过有效期末日的调用；提前调用同样拒绝。
    let (mut book2, id2) = book_with_buy_plan();
    let err = book2
        .apply(id2, PlanEvent::Expired { trading_day: 0 })
        .expect_err("expire before horizon end is rejected");
    assert!(matches!(err, PlanError::ExpireBeforeHorizonEnd { .. }));
}

#[test]
fn order_acceptance_never_advances_fill_and_bogus_fills_are_rejected() {
    let (mut book, id) = book_with_buy_plan();

    book.apply(
        id,
        PlanEvent::ChildOrderAccepted {
            order_id: OrderId(100),
            trading_day: 0,
        },
    )
    .expect("acceptance");
    assert_eq!(
        book.plan(id).unwrap().filled_qty,
        0,
        "acceptance is not fill progress"
    );

    // 零数量“成交”不是真实成交。
    let zero = book
        .apply(
            id,
            PlanEvent::ChildOrderFilled {
                order_id: OrderId(100),
                qty: 0,
                trading_day: 0,
            },
        )
        .expect_err("zero-qty fill is rejected");
    assert!(matches!(zero, PlanError::ZeroFillQuantity { .. }));

    // 未与计划关联的订单不得推进进度。
    let unlinked = book
        .apply(
            id,
            PlanEvent::ChildOrderFilled {
                order_id: OrderId(999),
                qty: 100,
                trading_day: 0,
            },
        )
        .expect_err("fill for an unlinked order is rejected");
    assert!(matches!(
        unlinked,
        PlanError::FillFromUnknownChildOrder { .. }
    ));

    assert_eq!(book.plan(id).unwrap().filled_qty, 0);
}

#[test]
fn unknown_plan_id_reference_is_rejected() {
    let (mut book, _id) = book_with_buy_plan();

    let err = book
        .apply(PlanId(999), PlanEvent::ObservedNoChange { trading_day: 0 })
        .expect_err("unknown plan id must be rejected");
    assert!(matches!(
        err,
        PlanError::UnknownPlan {
            plan_id: PlanId(999)
        }
    ));
    assert!(book.plan(PlanId(999)).is_err());
}

#[test]
fn duplicate_active_plan_for_account_and_stock_is_rejected() {
    let (mut book, _id) = book_with_buy_plan();

    let err = book
        .create(buy_open())
        .expect_err("second active plan for the same account+stock is rejected");
    assert!(matches!(err, PlanError::DuplicateActivePlan { .. }));

    // 不同账户或不同股票不受影响。
    book.create(PlanOpen {
        account: AccountId(8),
        ..buy_open()
    })
    .expect("different account is fine");
    book.create(PlanOpen {
        code: StockCode("002156".into()),
        ..buy_open()
    })
    .expect("different stock is fine");
}

#[test]
fn event_time_going_backwards_is_rejected() {
    let (mut book, id) = book_with_buy_plan();
    book.apply(id, PlanEvent::ObservedNoChange { trading_day: 5 })
        .expect("observe day 5");

    let err = book
        .apply(id, PlanEvent::ObservedNoChange { trading_day: 3 })
        .expect_err("time cannot go backwards");
    assert!(matches!(err, PlanError::EventTimeWentBackwards { .. }));
}

#[test]
fn event_beyond_horizon_is_rejected() {
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            horizon_trading_days: 5,
            ..buy_open()
        })
        .expect("open plan");

    book.apply(
        id,
        PlanEvent::ChildOrderAccepted {
            order_id: OrderId(100),
            trading_day: 4,
        },
    )
    .expect("last valid day still accepts events");

    let err = book
        .apply(
            id,
            PlanEvent::ChildOrderFilled {
                order_id: OrderId(100),
                qty: 100,
                trading_day: 5,
            },
        )
        .expect_err("events beyond the horizon are rejected");
    assert!(matches!(err, PlanError::EventBeyondHorizon { .. }));
}

#[test]
fn invalid_open_fields_are_rejected() {
    let mut book = PlanBook::default();

    let cases = [
        PlanOpen {
            target: PlanTarget::ShareCount(0),
            ..buy_open()
        },
        PlanOpen {
            target: PlanTarget::PositionFractionBp(10_001),
            ..buy_open()
        },
        PlanOpen {
            confidence_bp: 10_001,
            ..buy_open()
        },
        PlanOpen {
            opinion: PlanOpinion {
                signal_score_bp: 10_001,
                source: OpinionSource::Fundamental,
            },
            ..buy_open()
        },
        PlanOpen {
            horizon_trading_days: 0,
            ..buy_open()
        },
    ];
    for open in cases {
        assert!(
            book.create(open).is_err(),
            "invalid open fields must be rejected"
        );
    }
    assert!(book.create(buy_open()).is_ok());
    // 合法仓位比例目标可以表达（0..=10000bp）。
    assert!(book
        .create(PlanOpen {
            code: StockCode("300260".into()),
            target: PlanTarget::PositionFractionBp(6000),
            ..buy_open()
        })
        .is_ok());
}

#[test]
fn inconsistent_plan_book_state_is_rejected_on_restore() {
    let policy = PlanPolicy::default();
    // 同一账户+股票出现两个计划。
    let mut plans = BTreeMap::new();
    plans.insert(
        PlanId(0),
        TradingPlan::from_open(PlanId(0), buy_open(), &policy).unwrap(),
    );
    plans.insert(
        PlanId(1),
        TradingPlan::from_open(PlanId(1), buy_open(), &policy).unwrap(),
    );
    let err = PlanBook::from_parts(policy, 2, plans)
        .expect_err("duplicate account+stock must fail restore");
    assert!(matches!(err, PlanError::SaveInconsistent { .. }));

    // 计划 id 超出分配序：存档损坏。
    let mut plans = BTreeMap::new();
    plans.insert(
        PlanId(5),
        TradingPlan::from_open(PlanId(5), buy_open(), &policy).unwrap(),
    );
    let err =
        PlanBook::from_parts(policy, 2, plans).expect_err("id out of range must fail restore");
    assert!(matches!(err, PlanError::SaveInconsistent { .. }));
}

/// 目标以仓位比例表达时：成交照常累计，但份额语义的自动完成/超额判定不适用，
/// 需要份额目标的显式路径。
#[test]
fn fraction_targets_accumulate_fills_without_share_completion_semantics() {
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            code: StockCode("300260".into()),
            target: PlanTarget::PositionFractionBp(6000),
            ..buy_open()
        })
        .expect("open fraction plan");
    link_and_fill(&mut book, id, 100, 500, 0);

    let plan = book.plan(id).unwrap();
    assert_eq!(plan.filled_qty, 500);
    assert_eq!(plan.status, PlanStatus::Active, "no share-level completion");

    let err = book
        .apply(
            id,
            PlanEvent::ChildOrderExcessFilled {
                order_id: OrderId(100),
                qty: 100,
                trading_day: 0,
            },
        )
        .expect_err("excess semantics need a share target");
    assert!(matches!(
        err,
        PlanError::FractionTargetHasNoShareExcess { .. }
    ));
}

/// 迟滞谓词是 K5a 契约的显式数学：买向需 S >= +threshold，卖向需 S <= -threshold。
#[test]
fn reverse_threshold_predicate_encodes_k5a_hysteresis() {
    let policy = PlanPolicy::default();
    use engine::plans::reverse_crosses_threshold;
    assert!(reverse_crosses_threshold(
        Side::Sell,
        Side::Buy,
        2000,
        policy.reverse_revision_threshold_bp
    ));
    assert!(!reverse_crosses_threshold(
        Side::Sell,
        Side::Buy,
        1999,
        policy.reverse_revision_threshold_bp
    ));
    assert!(reverse_crosses_threshold(
        Side::Buy,
        Side::Sell,
        -2000,
        policy.reverse_revision_threshold_bp
    ));
    assert!(!reverse_crosses_threshold(
        Side::Buy,
        Side::Sell,
        -1999,
        policy.reverse_revision_threshold_bp
    ));
    // 同向修订不受反向门槛约束。
    assert!(reverse_crosses_threshold(
        Side::Buy,
        Side::Buy,
        500,
        policy.reverse_revision_threshold_bp
    ));
    assert_eq!(policy.reverse_revision_threshold_bp, 2000);
    assert_eq!(policy.review_signal_delta_bp, 1000);
    assert_eq!(policy.review_price_change_bp, 200);
}

/// 剩余数量与到期边界：份额目标给出剩余；比例目标显式无此语义。
#[test]
fn remaining_and_horizon_helpers_are_explicit() {
    let (mut book, id) = book_with_buy_plan();
    link_and_fill(&mut book, id, 100, 400, 0);
    let plan = book.plan(id).unwrap();
    assert_eq!(plan.remaining_share_qty(), Some(600));
    assert_eq!(plan.last_valid_trading_day(), 19);

    let frac_id = book
        .create(PlanOpen {
            code: StockCode("300260".into()),
            target: PlanTarget::PositionFractionBp(6000),
            horizon_trading_days: 5,
            ..buy_open()
        })
        .unwrap();
    let frac = book.plan(frac_id).unwrap();
    assert_eq!(frac.remaining_share_qty(), None);
    assert_eq!(frac.last_valid_trading_day(), 4);

    // 暂停中的计划仍可被显式终止（风险路径）。
    let mut book2 = PlanBook::default();
    let pid = book2.create(buy_open()).unwrap();
    book2
        .apply(
            pid,
            PlanEvent::Paused {
                reason: PauseReason::RiskPressure,
                trading_day: 0,
            },
        )
        .unwrap();
    book2
        .apply(
            pid,
            PlanEvent::Terminated {
                reason: TerminationReason::FundsUnavailable,
                trading_day: 0,
            },
        )
        .expect("paused plan can be terminated");
}

// ---------------------------------------------------------------------------
// 修复补测（fix pass）：到期/日终事件在有效期过后必须仍然可达。
// 缺陷：守卫先于到期语义拒绝一切 `trading_day > last_valid` 的事件，导致
// `Expired` 的成功路径不可达、错过的日终把计划永久搁浅在 Active。
// ---------------------------------------------------------------------------

/// 显式到期事件在有效期过后真正可达：越过 last_valid 即终止并清除子单引用。
#[test]
fn expired_event_after_horizon_is_reachable_and_terminates() {
    let (mut book, id) = book_with_buy_plan(); // horizon 20 → last_valid 19
    book.apply(
        id,
        PlanEvent::ChildOrderAccepted {
            order_id: OrderId(100),
            trading_day: 0,
        },
    )
    .expect("accept child");

    book.apply(id, PlanEvent::Expired { trading_day: 20 })
        .expect("explicit expiry after the horizon must succeed");
    let plan = book.plan(id).unwrap();
    assert_eq!(
        plan.status,
        PlanStatus::Terminated {
            reason: TerminationReason::HorizonExpired,
        }
    );
    assert_eq!(plan.active_child_order_id, None);
}

/// 错过的日终补账：day-end 在有效期过后到达也不报错，直接补终止。
#[test]
fn missed_day_end_after_horizon_terminates_on_catch_up() {
    // horizon 5、created day 0 → last_valid 4；该计划的第 4 天日终被跳过。
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            horizon_trading_days: 5,
            ..buy_open()
        })
        .expect("open plan");
    book.apply(
        id,
        PlanEvent::ChildOrderAccepted {
            order_id: OrderId(100),
            trading_day: 4,
        },
    )
    .expect("accept child on the last valid day");

    book.apply(id, PlanEvent::TradingDayEnded { trading_day: 5 })
        .expect("a late day end must not strand the plan");
    let plan = book.plan(id).unwrap();
    assert_eq!(
        plan.status,
        PlanStatus::Terminated {
            reason: TerminationReason::HorizonExpired,
        }
    );
    assert_eq!(plan.active_child_order_id, None);
    assert_eq!(plan.filled_qty, 0);
}

/// 经新路径（有效期过后的显式到期）终止的计划同样不可复活。
#[test]
fn plan_terminated_by_late_expiry_cannot_revive() {
    let mut book = PlanBook::default();
    let id = book
        .create(PlanOpen {
            horizon_trading_days: 5,
            ..buy_open()
        })
        .expect("open plan");
    book.apply(id, PlanEvent::Expired { trading_day: 5 })
        .expect("late expiry terminates");

    for event in [
        PlanEvent::ObservedNoChange { trading_day: 6 },
        PlanEvent::ChildOrderFilled {
            order_id: OrderId(1),
            qty: 100,
            trading_day: 6,
        },
        PlanEvent::Paused {
            reason: PauseReason::RiskPressure,
            trading_day: 6,
        },
    ] {
        let err = book
            .apply(id, event)
            .expect_err("terminal plan rejects every event");
        assert!(
            matches!(err, PlanError::InvalidTransition { .. }),
            "expected InvalidTransition, got {err:?}"
        );
    }
}

/// 时间回拨守卫对到期/日终仍然生效（豁免的只是有效期上限）。
#[test]
fn time_backwards_guard_still_fires_for_expiry_and_day_end() {
    let (mut book, id) = book_with_buy_plan();
    book.apply(id, PlanEvent::ObservedNoChange { trading_day: 3 })
        .expect("observe day 3");

    let day_end = book
        .apply(id, PlanEvent::TradingDayEnded { trading_day: 2 })
        .expect_err("day end before the last event day is rejected");
    assert!(matches!(day_end, PlanError::EventTimeWentBackwards { .. }));

    let expired = book
        .apply(id, PlanEvent::Expired { trading_day: 2 })
        .expect_err("expiry before the last event day is rejected");
    assert!(matches!(expired, PlanError::EventTimeWentBackwards { .. }));
}

/// 其余事件越过有效期仍然拒绝：豁免不扩大到观察/成交/修订/暂停。
#[test]
fn other_events_beyond_horizon_remain_rejected_after_fix() {
    let (mut book, id) = book_with_buy_plan(); // last_valid 19
    for event in [
        PlanEvent::ObservedNoChange { trading_day: 20 },
        PlanEvent::ChildOrderFilled {
            order_id: OrderId(1),
            qty: 100,
            trading_day: 20,
        },
        PlanEvent::Revised {
            revision: forward_revision(2000, 20),
        },
        PlanEvent::Paused {
            reason: PauseReason::RiskPressure,
            trading_day: 20,
        },
    ] {
        let err = book
            .apply(id, event)
            .expect_err("non-lifecycle events stay horizon-bound");
        assert!(
            matches!(err, PlanError::EventBeyondHorizon { .. }),
            "expected EventBeyondHorizon, got {err:?}"
        );
    }
}
