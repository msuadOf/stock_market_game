use super::*;

type Accumulator = SyncAccumulator<i64, (u64, u64), (u64, u64)>;
fn owner(group: u16, local: u32) -> SyncOwner {
    SyncOwner { group, local }
}
fn accumulator() -> Accumulator {
    Accumulator::new([(0, 2), (1, 1)]).unwrap()
}
fn populate(a: &mut Accumulator, seq: u64) {
    for key in [owner(0, 0), owner(0, 1), owner(1, 0)] {
        assert_eq!(
            a.record_account(key, seq, seq as i64),
            Ok(RecordOutcome::Changed)
        );
        assert_eq!(
            a.record_belief(key, seq, (seq, 7)),
            Ok(RecordOutcome::Changed)
        );
        assert_eq!(
            a.record_attention(key, seq, (seq + 1, 9)),
            Ok(RecordOutcome::Changed)
        );
    }
    a.mark_plans_pending();
}
fn repeated_window(ticks: u64) {
    let mut a = accumulator();
    assert_eq!(a.real_account_count(), 3);
    for seq in 0..ticks {
        populate(&mut a, seq);
        assert_eq!(
            (a.accounts().len(), a.beliefs().len(), a.attentions().len()),
            (3, 3, 3)
        );
        assert_eq!(
            a.accounts().len() + a.beliefs().len() + a.attentions().len(),
            3 * a.real_account_count()
        );
    }
    assert_eq!(
        a.accounts()[&owner(1, 0)],
        SyncValue {
            source_seq: ticks - 1,
            value: (ticks - 1) as i64
        }
    );
    assert_eq!(a.attentions()[&owner(0, 1)].value, (ticks, 9));
    assert!(a.plans_pending());
}
#[test]
fn repeated_n_is_bounded() {
    repeated_window(32);
}
#[test]
fn repeated_2n_is_bounded() {
    repeated_window(64);
}

#[test]
fn each_table_rejects_older_and_conflicting_inputs_without_mutation() {
    let mut a = accumulator();
    populate(&mut a, 5);
    a.require_full(FullReason::ExplicitRequest);
    let before = a.clone();
    for (domain, conflict, older) in [
        (
            SyncDomain::Account,
            a.record_account(owner(0, 0), 5, 999),
            a.record_account(owner(0, 0), 4, 5),
        ),
        (
            SyncDomain::Belief,
            a.record_belief(owner(0, 0), 5, (999, 7)),
            a.record_belief(owner(0, 0), 4, (5, 7)),
        ),
        (
            SyncDomain::Attention,
            a.record_attention(owner(0, 0), 5, (999, 9)),
            a.record_attention(owner(0, 0), 4, (6, 9)),
        ),
    ] {
        assert_eq!(
            conflict,
            Err(SyncError::ConflictingSource {
                domain,
                owner: owner(0, 0),
                source_seq: 5
            })
        );
        assert_eq!(
            older,
            Err(SyncError::OlderSource {
                domain,
                owner: owner(0, 0),
                current: 5,
                incoming: 4
            })
        );
    }
    assert_eq!(a, before);
}

#[test]
fn same_source_same_value_is_noop_in_all_tables() {
    let mut a = accumulator();
    populate(&mut a, 5);
    let before = a.clone();
    assert_eq!(
        a.record_account(owner(1, 0), 5, 5),
        Ok(RecordOutcome::Unchanged)
    );
    assert_eq!(
        a.record_belief(owner(1, 0), 5, (5, 7)),
        Ok(RecordOutcome::Unchanged)
    );
    assert_eq!(
        a.record_attention(owner(1, 0), 5, (6, 9)),
        Ok(RecordOutcome::Unchanged)
    );
    assert_eq!(a, before);
}

#[test]
fn source_sequences_are_independent_per_owner_and_domain() {
    let mut a = accumulator();
    let key = owner(0, 0);
    assert_eq!(a.record_account(key, 100, 10), Ok(RecordOutcome::Changed));
    assert_eq!(a.record_belief(key, 2, (3, 4)), Ok(RecordOutcome::Changed));
    assert_eq!(
        a.record_attention(key, 0, (5, 6)),
        Ok(RecordOutcome::Changed)
    );
    assert_eq!(
        a.record_account(owner(1, 0), 1, 20),
        Ok(RecordOutcome::Changed)
    );
    assert_eq!(a.record_account(key, 101, 10), Ok(RecordOutcome::Changed));
    assert_eq!(a.accounts()[&key].source_seq, 101);
    assert_eq!(a.beliefs()[&key].source_seq, 2);
    assert_eq!(a.attentions()[&key].source_seq, 0);
    assert_eq!(a.accounts()[&owner(1, 0)].source_seq, 1);
}

#[test]
fn unknown_group_out_of_range_and_tail_padding_are_rejected() {
    let mut a = accumulator();
    populate(&mut a, 1);
    let before = a.clone();
    for key in [owner(2, 0), owner(0, 2), owner(1, 1), owner(1, u32::MAX)] {
        let expected = if key.group == 2 {
            SyncError::UnknownGroup { owner: key }
        } else {
            SyncError::LocalOutsideRoster {
                owner: key,
                real_accounts: if key.group == 0 { 2 } else { 1 },
            }
        };
        assert_eq!(a.record_account(key, 2, 22), Err(expected.clone()));
        assert_eq!(a.record_belief(key, 2, (2, 2)), Err(expected.clone()));
        assert_eq!(a.record_attention(key, 2, (2, 2)), Err(expected));
        assert_eq!(a, before);
    }
    assert_eq!(a.real_account_count(), 3);
}

#[test]
fn duplicate_groups_fail_and_empty_groups_have_no_valid_local() {
    assert_eq!(
        Accumulator::new([(0, 1), (0, 2)]),
        Err(SyncError::DuplicateGroup { group: 0 })
    );
    let mut a = Accumulator::new([(7, 0)]).unwrap();
    assert_eq!(a.real_account_count(), 0);
    assert_eq!(
        a.record_account(owner(7, 0), 0, 0),
        Err(SyncError::LocalOutsideRoster {
            owner: owner(7, 0),
            real_accounts: 0
        })
    );
    assert_eq!(
        a.record_account(owner(0, 0), 0, 0),
        Err(SyncError::UnknownGroup { owner: owner(0, 0) })
    );
}

#[test]
fn plans_are_one_idempotent_marker() {
    let mut a = accumulator();
    assert!(!a.plans_pending());
    a.mark_plans_pending();
    let before = a.clone();
    a.mark_plans_pending();
    assert!(a.plans_pending());
    assert_eq!(a, before);
    assert!(a.accounts().is_empty() && a.beliefs().is_empty() && a.attentions().is_empty());
}

#[test]
fn full_reasons_and_initial_baseline_are_consumed_only_on_success() {
    let mut a = accumulator();
    assert_eq!(a.account_source(), AccountSource::Full);
    assert!(!a.has_successful_baseline());
    a.require_full(FullReason::ExplicitRequest);
    a.require_full(FullReason::ExplicitRequest);
    a.require_full(FullReason::DomainMutationWithoutFacts);
    assert_eq!(a.pending_full_reasons().len(), 2);
    let before = a.clone();
    a.complete_flush(FlushOutcome::Failed);
    assert_eq!(a, before);
    a.complete_flush(FlushOutcome::Successful);
    assert!(a.has_successful_baseline());
    assert!(a.pending_full_reasons().is_empty());
    assert_eq!(a.account_source(), AccountSource::SparseAccumulated);
    a.require_full(FullReason::DomainMutationWithoutFacts);
    assert_eq!(a.account_source(), AccountSource::Full);
    a.complete_flush(FlushOutcome::Failed);
    assert!(a.has_successful_baseline());
    assert_eq!(a.pending_full_reasons().len(), 1);
    a.complete_flush(FlushOutcome::Successful);
    assert_eq!(a.account_source(), AccountSource::SparseAccumulated);
}

#[test]
fn success_clears_all_pending_data_and_failure_preserves_exact_state() {
    let mut a = accumulator();
    populate(&mut a, 9);
    a.require_full(FullReason::DomainMutationWithoutFacts);
    let before = a.clone();
    a.complete_flush(FlushOutcome::Failed);
    assert_eq!(a, before);
    a.complete_flush(FlushOutcome::Successful);
    assert!(a.accounts().is_empty() && a.beliefs().is_empty() && a.attentions().is_empty());
    assert!(!a.plans_pending());
    assert!(a.pending_full_reasons().is_empty());
    assert!(a.has_successful_baseline());
    assert_eq!(a.real_account_count(), 3);
    populate(&mut a, 10);
    assert_eq!(a.accounts().len(), 3);
}

#[test]
fn maximum_source_has_no_overflow_or_implicit_increment() {
    let mut a = accumulator();
    assert_eq!(
        a.record_account(owner(0, 0), u64::MAX, i64::MAX),
        Ok(RecordOutcome::Changed)
    );
    assert_eq!(
        a.record_account(owner(0, 0), u64::MAX, i64::MAX),
        Ok(RecordOutcome::Unchanged)
    );
    let before = a.clone();
    assert_eq!(
        a.record_account(owner(0, 0), 0, 0),
        Err(SyncError::OlderSource {
            domain: SyncDomain::Account,
            owner: owner(0, 0),
            current: u64::MAX,
            incoming: 0
        })
    );
    assert_eq!(a, before);
}

/// 只承诺 PartialEq，复现现有完整 belief 类型的 trait 边界。
#[derive(Clone, Debug, PartialEq)]
struct PartialOnly(i64);

#[test]
fn partial_eq_values_support_record_noop_and_conflict_without_mutation() {
    let mut a = SyncAccumulator::<PartialOnly, PartialOnly, PartialOnly>::new([(0, 1)]).unwrap();
    let key = owner(0, 0);
    assert_eq!(
        a.record_account(key, 7, PartialOnly(10)),
        Ok(RecordOutcome::Changed)
    );
    assert_eq!(
        a.record_belief(key, 7, PartialOnly(20)),
        Ok(RecordOutcome::Changed)
    );
    assert_eq!(
        a.record_attention(key, 7, PartialOnly(30)),
        Ok(RecordOutcome::Changed)
    );
    let before = a.clone();
    assert_eq!(
        a.record_account(key, 7, PartialOnly(10)),
        Ok(RecordOutcome::Unchanged)
    );
    assert_eq!(
        a.record_belief(key, 7, PartialOnly(20)),
        Ok(RecordOutcome::Unchanged)
    );
    assert_eq!(
        a.record_attention(key, 7, PartialOnly(30)),
        Ok(RecordOutcome::Unchanged)
    );
    assert_eq!(a, before);
    assert_eq!(
        a.record_account(key, 7, PartialOnly(11)),
        Err(SyncError::ConflictingSource {
            domain: SyncDomain::Account,
            owner: key,
            source_seq: 7
        })
    );
    assert_eq!(a, before);
    assert_eq!(
        a.record_belief(key, 7, PartialOnly(21)),
        Err(SyncError::ConflictingSource {
            domain: SyncDomain::Belief,
            owner: key,
            source_seq: 7
        })
    );
    assert_eq!(a, before);
    assert_eq!(
        a.record_attention(key, 7, PartialOnly(31)),
        Err(SyncError::ConflictingSource {
            domain: SyncDomain::Attention,
            owner: key,
            source_seq: 7
        })
    );
    assert_eq!(a, before);
}
