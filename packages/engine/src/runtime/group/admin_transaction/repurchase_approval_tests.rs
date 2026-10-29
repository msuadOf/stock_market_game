//! APPROVAL-C 首红：真实组 prepare 的完整 book、引用、执行读面与原子回退。
use super::*;
use crate::company::CompanyId;
use crate::plans::{PlanId, RepurchaseExecutionOpen};
use crate::runtime::protocol::{
    AdminPlanReference, AdminTreasuryBinding, RepurchaseExecutionProjection,
    RepurchaseMaterialRestriction, RepurchaseProjectionInput,
};
use crate::{AccountId, Money, StockId};
#[path = "../../information_fixture.rs"]
mod fixture;

fn fixture_state() -> GroupState {
    let mut state = GroupState::new(
        1,
        vec![GroupAccount::new(0, 2); 2],
        8,
        100,
        Arc::new(GameConfig::proposed_defaults()),
        100,
        None,
        Vec::new(),
        Box::new(|_, _| TickPlan::default()),
    );
    state.bind_calendar(fixture::calendar_source(), fixture::calendar_bindings());
    state.root_projection = Some(fixture::projection(
        [11, 37, 51, 79, 113].map(AccountId).to_vec(),
    ));
    state.roster_offset = 2;
    state.admin_treasury_bindings = [0, 1]
        .map(|stock| AdminTreasuryBinding {
            owner: (1, 1),
            account: AccountId(79),
            issuer: CompanyId(stock),
            stock: stock as u16,
        })
        .to_vec();
    let history = state
        .plan_book
        .create(crate::plans::PlanOpen {
            account: AccountId(51),
            code: StockId(1),
            direction: Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(100),
            opinion: crate::plans::PlanOpinion {
                signal_score_bp: 0,
                source: crate::plans::OpinionSource::Fundamental,
            },
            confidence_bp: 0,
            urgency: crate::plans::Urgency::Patient,
            horizon_trading_days: 20,
            created_trading_day: 0,
        })
        .unwrap();
    state
        .plan_book
        .apply(
            history,
            crate::plans::PlanEvent::Terminated {
                reason: crate::plans::TerminationReason::Cancelled,
                trading_day: 0,
            },
        )
        .unwrap();
    let (policy, mut cursors, entries) = state.plan_book.clone().into_parts();
    assert_eq!(history.owner, AccountId(51));
    cursors[0].next_sequence = Some(37);
    state.plan_book = crate::plans::PlanBook::from_parts(policy, cursors, entries).unwrap();
    state
}
fn open(stock: u16) -> AdminOperation {
    AdminOperation::OpenIssuerRepurchase {
        local: 1,
        stock,
        issuer: CompanyId(u32::from(stock)),
    }
}
fn credit(stock: u16, index: u32, amount: i64) -> AdminOperation {
    AdminOperation::SyntheticBudgetCredit {
        local: 1,
        stock,
        execution_plan: AdminPlanReference::CreatedInThisPrepare {
            operation_index: index,
        },
        amount,
    }
}
fn projection(stock: u16, reference: AdminPlanReference, amount: i64) -> RepurchaseProjectionInput {
    let day = fixture::at(9, 0).date();
    RepurchaseProjectionInput {
        local: 1,
        stock,
        issuer: CompanyId(u32::from(stock)),
        execution_plan: reference,
        projection: RepurchaseExecutionProjection {
            account: AccountId(79),
            issuer: CompanyId(u32::from(stock)),
            stock,
            announced: false,
            announced_on: day,
            window_start_on: day,
            window_deadline_on: day,
            exchange: crate::calendar::CalendarExchange::Szse,
            purpose: crate::company::issuer_repurchase::RepurchasePurpose::ReduceCapital,
            funding_amount: Money::from_cents(amount),
            price_cap: Money::from_cents(1234),
            max_shares: 1000,
            unit_as_of: day,
            unit_revision: 0,
            book_through_cursor: 0,
            book_gross: Money::ZERO,
            book_fees: Money::ZERO,
            recycled: Money::ZERO,
            inventory_shares: 0,
            last_order_day: None,
            eligibility_revision: 7,
            eligibility_as_of: fixture::at(9, 0),
            issuance_in_progress: Some(false),
            material_restriction: RepurchaseMaterialRestriction::NotModelled,
        },
    }
}
fn created_projection(stock: u16, index: u32, amount: i64) -> RepurchaseProjectionInput {
    projection(
        stock,
        AdminPlanReference::CreatedInThisPrepare {
            operation_index: index,
        },
        amount,
    )
}
fn rejected(
    mut state: GroupState,
    operations: Vec<AdminOperation>,
    projections: Vec<RepurchaseProjectionInput>,
    reason: &str,
) {
    let accounts = state.accounts.clone();
    let book = state.plan_book.clone();
    let hash = state.state_hash();
    let dirty = state.plan_book_dirty;
    let undo = format!("{:?}", state.undo);
    let old_projection = state.repurchase_projections.clone();
    let error = state
        .prepare_admin_scope(AdminTransactionId(1), operations, None, Some(projections))
        .unwrap_err();
    assert!(
        error.to_string().contains(reason),
        "预期拒绝 {reason}，实际 {error}"
    );
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.plan_book, book);
    assert_eq!(state.state_hash(), hash);
    assert_eq!(state.plan_book_dirty, dirty);
    assert_eq!(format!("{:?}", state.undo), undo);
    assert_eq!(state.repurchase_projections, old_projection);
    assert!(state.admin_pending.is_none());
    assert_eq!(state.admin_last_id, 1);
}
#[test]
fn approval_c_group_01_flat_index_complete_book_receipts_and_abort_retry() {
    let mut state = fixture_state();
    let original = state.plan_book.clone();
    let mut expected = original.clone();
    let ids = [0, 1].map(|stock| {
        expected
            .create_issuer_repurchase(RepurchaseExecutionOpen {
                owner: AccountId(79),
                issuer: CompanyId(stock),
                stock: StockId(stock),
            })
            .unwrap()
    });
    let projections = vec![created_projection(0, 1, 101), created_projection(1, 3, 203)];
    let candidate = state
        .prepare_admin_scope(
            AdminTransactionId(1),
            vec![
                AdminOperation::CashDividend {
                    local: 0,
                    stock: 0,
                    amount: 7,
                },
                open(0),
                credit(0, 1, 101),
                open(1),
                credit(1, 3, 203),
            ],
            None,
            Some(projections.clone()),
        )
        .unwrap();
    let prepared = candidate
        .repurchase
        .as_ref()
        .expect("Prepare 必须回完整计划与投影");
    assert_eq!(prepared.book, expected);
    assert_eq!(state.plan_book, expected);
    assert_eq!(
        prepared.book.owner_sequences().collect::<Vec<_>>(),
        expected.owner_sequences().collect::<Vec<_>>()
    );
    assert_eq!(
        prepared.created,
        vec![
            crate::runtime::protocol::CreatedRepurchasePlan {
                group: 1,
                operation_index: 1,
                local: 1,
                stock: 0,
                issuer: CompanyId(0),
                plan_id: ids[0]
            },
            crate::runtime::protocol::CreatedRepurchasePlan {
                group: 1,
                operation_index: 3,
                local: 1,
                stock: 1,
                issuer: CompanyId(1),
                plan_id: ids[1]
            },
        ]
    );
    assert_eq!(
        prepared.projections,
        ids.into_iter()
            .zip(projections.into_iter().map(|p| p.projection))
            .collect()
    );
    assert_eq!((state.accounts[0].cash, state.accounts[1].cash), (7, 304));
    assert!(candidate.mirror_domains.plans_pending);
    let aborted = state
        .complete_admin(AdminTransactionId(1), AdminPhase::Abort)
        .unwrap();
    assert!(aborted.repurchase.is_none());
    assert!(aborted.information.is_none());
    assert_eq!(state.plan_book, original);
    assert!(state.repurchase_projections.is_empty());
    assert_eq!((state.accounts[0].cash, state.accounts[1].cash), (0, 0));
    state
        .prepare_admin_scope(
            AdminTransactionId(2),
            vec![open(0), credit(0, 0, 101)],
            None,
            Some(vec![created_projection(0, 0, 101)]),
        )
        .unwrap();
    let committed = state
        .complete_admin(AdminTransactionId(2), AdminPhase::Commit)
        .unwrap();
    assert!(committed.repurchase.is_none());
    assert_eq!(
        state
            .plan_book
            .active_entry(AccountId(79), &StockId(0))
            .unwrap()
            .plan_id(),
        ids[0]
    );
    assert_eq!(state.repurchase_projections.len(), 1);
    assert!(state.plan_book_dirty);
}
#[test]
fn approval_c_group_02_reference_pairing_rejections_are_distinct_and_atomic() {
    for (ops, projections, reason) in [
        (
            vec![open(0)],
            vec![created_projection(0, 0, 101)],
            "每个 Open",
        ),
        (
            vec![credit(0, 1, 101), open(0)],
            vec![created_projection(0, 1, 101)],
            "更早",
        ),
        (
            vec![open(0), credit(0, 1, 101)],
            vec![created_projection(0, 0, 101)],
            "更早",
        ),
        (
            vec![
                AdminOperation::CashDividend {
                    local: 0,
                    stock: 0,
                    amount: 7,
                },
                open(0),
                credit(0, 0, 101),
            ],
            vec![created_projection(0, 1, 101)],
            "Open 位置",
        ),
        (
            vec![open(0), credit(1, 0, 101)],
            vec![created_projection(0, 0, 101)],
            "坐标",
        ),
        (
            vec![
                open(0),
                AdminOperation::SyntheticBudgetCredit {
                    local: 0,
                    stock: 0,
                    execution_plan: AdminPlanReference::CreatedInThisPrepare { operation_index: 0 },
                    amount: 101,
                },
            ],
            vec![created_projection(0, 0, 101)],
            "坐标",
        ),
        (
            vec![open(0), credit(0, 0, 0)],
            vec![created_projection(0, 0, 101)],
            "正分额",
        ),
        (
            vec![open(0), credit(0, 0, -1)],
            vec![created_projection(0, 0, 101)],
            "正分额",
        ),
        (
            vec![open(0), credit(0, 0, 101), credit(0, 0, 101)],
            vec![created_projection(0, 0, 101)],
            "重复 Credit",
        ),
    ] {
        rejected(fixture_state(), ops, projections, reason);
    }
}
#[test]
fn approval_c_group_03_existing_original_book_is_usable_but_new_sequence_cannot_bypass_slot() {
    let mut state = fixture_state();
    let id = state
        .plan_book
        .create_issuer_repurchase(RepurchaseExecutionOpen {
            owner: AccountId(79),
            issuer: CompanyId(0),
            stock: StockId(0),
        })
        .unwrap();
    let original = state.plan_book.clone();
    state
        .prepare_admin_scope(
            AdminTransactionId(1),
            vec![AdminOperation::SyntheticBudgetCredit {
                local: 1,
                stock: 0,
                execution_plan: AdminPlanReference::Existing(id),
                amount: 101,
            }],
            None,
            Some(vec![projection(0, AdminPlanReference::Existing(id), 101)]),
        )
        .unwrap();
    assert_eq!(state.accounts[1].cash, 101);
    assert_eq!(state.plan_book, original);
    state
        .complete_admin(AdminTransactionId(1), AdminPhase::Abort)
        .unwrap();
    let guessed = PlanId {
        owner: AccountId(79),
        sequence: 0,
    };
    rejected(
        fixture_state(),
        vec![
            open(0),
            AdminOperation::SyntheticBudgetCredit {
                local: 1,
                stock: 0,
                execution_plan: AdminPlanReference::Existing(guessed),
                amount: 101,
            },
        ],
        vec![created_projection(0, 0, 101)],
        "原 book",
    );
    rejected(
        fixture_state(),
        vec![
            open(0),
            credit(0, 0, 101),
            AdminOperation::SyntheticBudgetRecycle {
                local: 1,
                stock: 0,
                execution_plan: guessed,
                amount: 101,
            },
        ],
        vec![created_projection(0, 0, 101)],
        "新建即回收",
    );
}
#[test]
fn approval_c_group_04_abort_restores_dirty_seal_undo_and_failure_id_watermark() {
    for dirty in [false, true] {
        let mut state = fixture_state();
        state.plan_book_dirty = dirty;
        state.cur_tick = 1;
        state.cur_round = 2;
        state.seal_acked_len = Some(1);
        state.plan_undo_saved = true;
        state.undo.push(GroupUndo::Cash { local: 0, delta: 7 });
        let before = state.plan_book.clone();
        let undo = format!("{:?}", state.undo);
        let published = state.committed_hash;
        state
            .prepare_admin_scope(
                AdminTransactionId(1),
                vec![open(0), credit(0, 0, 101)],
                None,
                Some(vec![created_projection(0, 0, 101)]),
            )
            .unwrap();
        state
            .complete_admin(AdminTransactionId(1), AdminPhase::Abort)
            .unwrap();
        assert_eq!(state.plan_book, before);
        assert_eq!(state.plan_book_dirty, dirty);
        assert_eq!(state.accounts[1].cash, 0);
        assert_eq!(format!("{:?}", state.undo), undo);
        assert!(state.plan_undo_saved);
        assert_eq!(state.seal_acked_len, Some(1));
        assert_eq!(state.committed_hash, published);
        assert_eq!(
            (state.cur_tick, state.committed_tick, state.cur_round),
            (1, 0, 2)
        );
        assert_eq!(state.admin_last_id, 1);
        assert!(state.repurchase_projections.is_empty());
    }
}
#[test]
fn approval_c_group_05_projection_missing_duplicate_extra_and_funding_mismatch_reject() {
    let valid = created_projection(0, 0, 101);
    for (rows, reason) in [
        (vec![], "缺少执行投影"),
        (vec![valid.clone(), valid.clone()], "重复执行投影"),
        (
            vec![valid.clone(), created_projection(1, 0, 101)],
            "投影身份",
        ),
        (vec![created_projection(0, 0, 102)], "额度"),
    ] {
        rejected(
            fixture_state(),
            vec![open(0), credit(0, 0, 101)],
            rows,
            reason,
        );
    }
}
#[test]
fn approval_c_group_06_info_late_failure_reaches_real_missing_day_and_rolls_back_all() {
    let mut state = fixture_state();
    let before = state.plan_book.clone();
    let accounts = state.accounts.clone();
    let library = state.library.clone();
    state
        .install_initial_company_facts(fixture::facts(8000))
        .unwrap();
    let mut payload = fixture::payload(true);
    payload
        .observation
        .as_mut()
        .unwrap()
        .as_of_anchors
        .remove(&StockId(0));
    let error = state
        .prepare_admin_scope(
            AdminTransactionId(1),
            vec![open(0), credit(0, 0, 101)],
            Some(payload),
            Some(vec![created_projection(0, 0, 101)]),
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            RuntimeError::InformationPrepare {
                error:
                    crate::runtime::protocol::InformationPrepareError::MissingInformationAnchor {
                        stock: StockId(0)
                    },
                ..
            }
        ),
        "必须到达真实 INFO 晚错误：{error}"
    );
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.plan_book, before);
    assert!(state.repurchase_projections.is_empty());
    assert!(Arc::ptr_eq(&state.library, &library));
    assert!(state.admin_pending.is_none());
}
#[test]
fn approval_c_group_07_projection_identity_terms_and_open_binding_cannot_be_guessed() {
    for mode in 0..8 {
        let mut row = created_projection(0, 0, 101);
        let mut operations = vec![open(0), credit(0, 0, 101)];
        let reason = match mode {
            0 => {
                row.projection.account = AccountId(51);
                "投影身份"
            }
            1 => {
                row.issuer = CompanyId(99);
                "投影身份"
            }
            2 => {
                row.stock = 1;
                "投影身份"
            }
            3 => {
                row.execution_plan =
                    AdminPlanReference::CreatedInThisPrepare { operation_index: 1 };
                "Open 位置"
            }
            4 => {
                row.projection.price_cap = Money::ZERO;
                "price_cap"
            }
            5 => {
                row.projection.max_shares = 0;
                "max_shares"
            }
            6 => {
                operations[0] = AdminOperation::OpenIssuerRepurchase {
                    local: 1,
                    stock: 0,
                    issuer: CompanyId(99),
                };
                "binding"
            }
            _ => {
                operations = vec![open(0), credit(0, 0, 101), open(0), credit(0, 2, 101)];
                "活动计划"
            }
        };
        rejected(fixture_state(), operations, vec![row], reason);
    }
}
#[test]
fn approval_c_group_08_existing_projection_cannot_regress_cursor_or_change_immutable_funding() {
    let mut state = fixture_state();
    let id = state
        .plan_book
        .create_issuer_repurchase(RepurchaseExecutionOpen {
            owner: AccountId(79),
            issuer: CompanyId(0),
            stock: StockId(0),
        })
        .unwrap();
    let mut row = projection(0, AdminPlanReference::Existing(id), 101);
    row.projection.book_through_cursor = 9;
    row.projection.book_gross = Money::from_cents(17);
    state
        .repurchase_projections
        .insert(id, row.projection.clone());
    for mode in 0..3 {
        let mut candidate = row.clone();
        let reason = match mode {
            0 => {
                candidate.projection.book_through_cursor = 8;
                "cursor"
            }
            1 => {
                candidate.projection.eligibility_revision = 6;
                "revision"
            }
            _ => {
                candidate.projection.funding_amount = Money::from_cents(102);
                "不可变"
            }
        };
        let before = state.repurchase_projections.clone();
        let book = state.plan_book.clone();
        let accounts = state.accounts.clone();
        let error = state
            .prepare_admin_scope(
                AdminTransactionId(mode + 1),
                Vec::new(),
                None,
                Some(vec![candidate]),
            )
            .unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
        assert_eq!(state.repurchase_projections, before);
        assert_eq!(state.plan_book, book);
        assert_eq!(state.accounts, accounts);
        assert!(state.admin_pending.is_none());
    }
}
#[test]
fn approval_c_group_09_projection_only_commit_is_a_plan_write_without_changing_cash_or_observation()
{
    let mut state = fixture_state();
    let id = state
        .plan_book
        .create_issuer_repurchase(RepurchaseExecutionOpen {
            owner: AccountId(79),
            issuer: CompanyId(0),
            stock: StockId(0),
        })
        .unwrap();
    let row = projection(0, AdminPlanReference::Existing(id), 101);
    state
        .repurchase_projections
        .insert(id, row.projection.clone());
    let accounts = state.accounts.clone();
    let book = state.plan_book.clone();
    let library = state.library.clone();
    let old_hash = state.state_hash();
    let mut changed = row.clone();
    changed.projection.eligibility_revision += 1;
    changed.projection.material_restriction = RepurchaseMaterialRestriction::Restricted;
    let candidate = state
        .prepare_admin_scope(
            AdminTransactionId(1),
            Vec::new(),
            None,
            Some(vec![changed.clone()]),
        )
        .unwrap();
    assert!(candidate.mirror_domains.plans_pending);
    assert!(!candidate.mirror_domains.accounts_full);
    assert!(candidate.information.is_none());
    assert_eq!(candidate.accounts.len(), 1);
    assert_eq!(candidate.accounts[0].0, 1);
    assert_eq!(
        candidate.repurchase.as_ref().unwrap().projections.get(&id),
        Some(&changed.projection)
    );
    assert_ne!(
        state.state_hash(),
        old_hash,
        "执行资格实际变更必须进入业务hash"
    );
    state
        .complete_admin(AdminTransactionId(1), AdminPhase::Commit)
        .unwrap();
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.plan_book, book);
    assert!(Arc::ptr_eq(&state.library, &library));
    assert_eq!(
        state.repurchase_projections.get(&id),
        Some(&changed.projection)
    );
}
#[test]
fn approval_c_group_10_commit_retires_old_plan_undo_before_real_ordinary_rollback() {
    let mut state = fixture_state();
    let before = state.plan_book.clone();
    state.cur_tick = 1;
    state.seal_acked_len = Some(0);
    state.undo.push(GroupUndo::PlanBook {
        prev: Box::new(before),
    });
    state.plan_undo_saved = true;
    state
        .prepare_admin_scope(
            AdminTransactionId(1),
            vec![open(0), credit(0, 0, 101)],
            None,
            Some(vec![created_projection(0, 0, 101)]),
        )
        .unwrap();
    state
        .complete_admin(AdminTransactionId(1), AdminPhase::Commit)
        .unwrap();
    let committed_book = state.plan_book.clone();
    let committed_projection = state.repurchase_projections.clone();
    state.rollback();
    assert_eq!(state.plan_book, committed_book);
    assert_eq!(state.repurchase_projections, committed_projection);
    assert_eq!(state.accounts[1].cash, 101);
}
#[test]
fn approval_c_group_11_information_and_projection_abort_then_commit_share_one_pending() {
    let mut state = fixture_state();
    let mut root = fixture::projection([11, 37, 51, 79, 113].map(AccountId).to_vec());
    Arc::get_mut(&mut root)
        .expect("fixture 唯一持有 RootProjection")
        .chain_params[3] = None;
    state.root_projection = Some(root);
    state.belief_states = vec![Some(fixture::personal(AccountId(51))), None];
    state.belief_dirty = vec![true, false];
    state.belief_pending_dirty.push(0);
    state.belief_states[0]
        .as_mut()
        .unwrap()
        .watchlist
        .stocks
        .insert(StockId(0), Default::default());
    let old_dirty = state.belief_dirty.clone();
    let old_pending = state.belief_pending_dirty.clone();
    let old_facts = fixture::facts(8000);
    state
        .install_initial_company_facts(old_facts.clone())
        .unwrap();
    let book = state.plan_book.clone();
    let accounts = state.accounts.clone();
    let personals = state.belief_states.clone();
    let attention = format!(
        "{:?}/{:?}/{:?}",
        state.decision, state.due_heap, state.attention_evolved
    );
    let library = state.library.clone();
    let hash = state.state_hash();
    let dirty = state.plan_book_dirty;
    let mut payload = fixture::payload(true);
    let publication = fixture::announcement(&mut payload, 0);
    let candidate = state
        .prepare_admin_scope(
            AdminTransactionId(1),
            vec![open(0), credit(0, 0, 101)],
            Some(payload.clone()),
            Some(vec![created_projection(0, 0, 101)]),
        )
        .unwrap();
    assert_eq!(candidate.information.as_ref().unwrap().personals.len(), 1);
    assert_eq!(
        candidate.information.as_ref().unwrap().personals[0].1,
        AccountId(51)
    );
    assert!(candidate.repurchase.is_some());
    state
        .complete_admin(AdminTransactionId(1), AdminPhase::Abort)
        .unwrap();
    assert_eq!(state.plan_book, book);
    assert_eq!(state.accounts, accounts);
    assert_eq!(state.belief_states, personals);
    assert_eq!(state.plan_book_dirty, dirty);
    assert_eq!(state.belief_dirty, old_dirty);
    assert_eq!(state.belief_pending_dirty, old_pending);
    assert_eq!(state.state_hash(), hash);
    assert!(state.repurchase_projections.is_empty());
    assert!(Arc::ptr_eq(&state.library, &library));
    assert!(Arc::ptr_eq(
        state.current_company_facts().unwrap(),
        &old_facts
    ));
    assert_eq!(
        format!(
            "{:?}/{:?}/{:?}",
            state.decision, state.due_heap, state.attention_evolved
        ),
        attention
    );
    state
        .prepare_admin_scope(
            AdminTransactionId(2),
            vec![open(0), credit(0, 0, 101)],
            Some(payload.clone()),
            Some(vec![created_projection(0, 0, 101)]),
        )
        .unwrap();
    state
        .complete_admin(AdminTransactionId(2), AdminPhase::Commit)
        .unwrap();
    assert_eq!(state.accounts[1].cash, 101);
    assert_eq!(state.repurchase_projections.len(), 1);
    assert!(state.belief_states[1].is_none(), "专户不得伪造个人观察");
    assert_eq!(
        state.belief_states[0]
            .as_ref()
            .unwrap()
            .information
            .observed_at_of(publication),
        Some(fixture::at(19, 0))
    );
    assert_eq!(
        state.belief_states[0].as_ref().unwrap().watchlist,
        personals[0].as_ref().unwrap().watchlist
    );
    assert!(Arc::ptr_eq(&state.library, &payload.library));
    assert!(Arc::ptr_eq(
        state.current_company_facts().unwrap(),
        &payload.current_facts
    ));
}
#[test]
fn approval_c_group_12_new_open_cannot_claim_already_consumed_facts_or_prior_inventory() {
    for mode in 0..7 {
        let mut row = created_projection(0, 0, 101);
        match mode {
            0 => row.projection.book_through_cursor = 1,
            1 => row.projection.book_gross = Money::from_cents(1),
            2 => row.projection.book_fees = Money::from_cents(1),
            3 => row.projection.recycled = Money::from_cents(1),
            4 => row.projection.inventory_shares = 1,
            5 => row.projection.last_order_day = Some(fixture::at(9, 0).date()),
            _ => row.projection.book_gross = Money::from_cents(-1),
        }
        rejected(
            fixture_state(),
            vec![open(0), credit(0, 0, 101)],
            vec![row],
            "初始基线",
        );
    }
}
#[test]
fn approval_c_group_13_existing_eligibility_source_same_revision_is_immutable_and_time_monotone() {
    let mut state = fixture_state();
    let id = state
        .plan_book
        .create_issuer_repurchase(RepurchaseExecutionOpen {
            owner: AccountId(79),
            issuer: CompanyId(0),
            stock: StockId(0),
        })
        .unwrap();
    let row = projection(0, AdminPlanReference::Existing(id), 101);
    state
        .repurchase_projections
        .insert(id, row.projection.clone());
    for mode in 0..2 {
        let mut changed = row.clone();
        if mode == 0 {
            changed.projection.material_restriction = RepurchaseMaterialRestriction::Restricted;
        } else {
            changed.projection.eligibility_revision += 1;
            changed.projection.eligibility_as_of = fixture::at(8, 59);
        }
        let before = state.repurchase_projections.clone();
        let error = state
            .prepare_admin_scope(
                AdminTransactionId(mode + 1),
                Vec::new(),
                None,
                Some(vec![changed]),
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains(if mode == 0 { "同 revision" } else { "as_of" }),
            "{error}"
        );
        assert_eq!(state.repurchase_projections, before);
        assert!(state.admin_pending.is_none());
    }
}
#[test]
fn approval_c_group_14_hash_covers_each_immutable_term_consumed_baseline_and_eligibility_source() {
    for mode in 0..16 {
        let mut state = fixture_state();
        let id = state
            .plan_book
            .create_issuer_repurchase(RepurchaseExecutionOpen {
                owner: AccountId(79),
                issuer: CompanyId(0),
                stock: StockId(0),
            })
            .unwrap();
        let row = projection(0, AdminPlanReference::Existing(id), 101);
        state.repurchase_projections.insert(id, row.projection);
        let before = state.state_hash();
        let changed = state.repurchase_projections.get_mut(&id).unwrap();
        match mode {
            0 => changed.funding_amount = Money::from_cents(102),
            1 => changed.price_cap = Money::from_cents(1235),
            2 => changed.max_shares = 1100,
            3 => changed.announced = true,
            4 => changed.unit_revision = 1,
            5 => changed.book_through_cursor = 1,
            6 => changed.book_gross = Money::from_cents(1),
            7 => changed.book_fees = Money::from_cents(1),
            8 => changed.recycled = Money::from_cents(1),
            9 => changed.inventory_shares = 100,
            10 => changed.last_order_day = Some(fixture::at(9, 0).date()),
            11 => changed.eligibility_revision = 8,
            12 => changed.eligibility_as_of = fixture::at(9, 1),
            13 => changed.issuance_in_progress = Some(true),
            14 => changed.material_restriction = RepurchaseMaterialRestriction::Restricted,
            _ => {
                changed.purpose =
                    crate::company::issuer_repurchase::RepurchasePurpose::ValueMaintenance
            }
        }
        assert_ne!(
            state.state_hash(),
            before,
            "独立业务字段必须入hash：mode={mode}"
        );
    }
}
