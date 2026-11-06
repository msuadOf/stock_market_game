//! 仅验证prepared索引join，使用真实AdminOperation，不模拟World或资产apply。
pub use engine::runtime;
#[path = "../../../packages/engine/src/runtime/admin_stage_evidence.rs"]
mod admin_stage_evidence;
use admin_stage_evidence::{
    join_prepared_admin_operations as join, PreparedAdminEvidenceError as E,
    PreparedAdminOperation as P,
};
use engine::runtime::world::AdminOperation as A;

fn receipt(stage_index: u32, operation_index: u32) -> P {
    P {
        stage_index,
        operation_index,
    }
}
fn ops() -> Vec<Vec<A>> {
    vec![
        vec![],
        vec![
            A::CashDividend {
                local: 2,
                stock: 0,
                amount: 100,
            },
            A::WithholdingTax {
                local: 2,
                stock: 0,
                amount: 100,
            },
        ],
        vec![],
        vec![A::ShareSplit {
            local: 1,
            stock: 0,
            bonus_qty: u32::MAX,
        }],
    ]
}
fn complete() -> Vec<P> {
    vec![receipt(1, 0), receipt(1, 1), receipt(3, 0)]
}

#[test]
fn stage_identity_01_exact_original_operands_are_borrowed_without_netting() {
    let sent = ops();
    let prepared = complete();
    let result = join(&sent, &prepared).unwrap();
    assert_eq!(result, vec![&sent[1][0], &sent[1][1], &sent[3][0]]);
    assert!(std::ptr::eq(result[0], &sent[1][0]));
    assert!(std::ptr::eq(result[2], &sent[3][0]));
}
#[test]
fn stage_identity_02_no_stages_and_no_receipts_is_a_legal_empty_join() {
    assert!(join(&[], &[]).unwrap().is_empty());
}
#[test]
fn stage_identity_03_empty_stages_need_no_fabricated_receipt() {
    assert!(join(&[vec![], vec![]], &[]).unwrap().is_empty());
}
#[test]
fn stage_identity_04_last_missing_receipt_is_exactly_identified() {
    assert_eq!(
        join(&ops(), &complete()[..2]),
        Err(E::Missing {
            expected: receipt(3, 0)
        })
    );
}
#[test]
fn stage_identity_05_middle_omission_does_not_sort_or_guess() {
    assert_eq!(
        join(&ops(), &[receipt(1, 0), receipt(3, 0)]),
        Err(E::Mismatch {
            position: 1,
            expected: receipt(1, 1),
            actual: receipt(3, 0)
        })
    );
}
#[test]
fn stage_identity_06_duplicate_receipt_does_not_pass_by_count() {
    assert_eq!(
        join(&ops(), &[receipt(1, 0), receipt(1, 0), receipt(3, 0)]),
        Err(E::Mismatch {
            position: 1,
            expected: receipt(1, 1),
            actual: receipt(1, 0)
        })
    );
}
#[test]
fn stage_identity_07_reordered_receipts_do_not_pass_by_set() {
    assert_eq!(
        join(&ops(), &[receipt(1, 1), receipt(1, 0), receipt(3, 0)]),
        Err(E::Mismatch {
            position: 0,
            expected: receipt(1, 0),
            actual: receipt(1, 1)
        })
    );
}
#[test]
fn stage_identity_08_extra_receipt_is_rejected_at_its_real_position() {
    let mut rows = complete();
    rows.push(receipt(3, 0));
    assert_eq!(
        join(&ops(), &rows),
        Err(E::Unexpected {
            position: 3,
            actual: receipt(3, 0)
        })
    );
}
#[test]
fn stage_identity_09_receipt_for_empty_stage_is_not_accepted() {
    assert_eq!(
        join(&[vec![]], &[receipt(0, 0)]),
        Err(E::Unexpected {
            position: 0,
            actual: receipt(0, 0)
        })
    );
}
#[test]
fn stage_identity_10_unknown_stage_is_typed_and_not_indexed() {
    assert_eq!(
        join(&ops(), &[receipt(u32::MAX, 0)]),
        Err(E::Mismatch {
            position: 0,
            expected: receipt(1, 0),
            actual: receipt(u32::MAX, 0)
        })
    );
}
#[test]
fn stage_identity_11_unknown_operation_is_typed_and_not_indexed() {
    assert_eq!(
        join(&ops(), &[receipt(1, u32::MAX)]),
        Err(E::Mismatch {
            position: 0,
            expected: receipt(1, 0),
            actual: receipt(1, u32::MAX)
        })
    );
}
