extern crate engine;

use engine::runtime::world::BeliefParticipantSnapshot;
use engine::{AccountId, GameSession};

#[path = "common/flip_helpers.rs"]
mod fixture;

pub use engine::runtime::{GroupIdx, LocalIdx};
pub mod world {
    pub use engine::runtime::world::BeliefParticipantSnapshot;
}

mod type_aliases {
    use super::{AccountId, BeliefParticipantSnapshot, GroupIdx, LocalIdx, PersonalPresence};

    // 仅为同一验证器在独立测试 crate 中补齐其所需的类型闭包。
    pub(super) type PersonalPresenceGroup = (GroupIdx, Vec<(AccountId, PersonalPresence)>);
    pub(super) type PersonalFullGroup = (
        GroupIdx,
        Vec<(LocalIdx, AccountId, Option<BeliefParticipantSnapshot>)>,
    );
}

#[path = "../src/runtime/personal_full_validation.rs"]
mod personal_full_validation;

use personal_full_validation::{validate_personal_full, PersonalAbsenceReason, PersonalPresence};

fn snapshot(session: &GameSession, id: AccountId) -> BeliefParticipantSnapshot {
    // runtime_personal_states 的返回类型是 private；外部短夹具只组合现有 public 读面。
    let (watchlist, price_memory, information, belief) = session
        .runtime_belief_states()
        .remove(&id)
        .expect("真实个人态行存在");
    let saved = session.save().expect("未推进会话可保存");
    BeliefParticipantSnapshot {
        watchlist,
        price_memory,
        information,
        belief,
        attention: saved.npc_attention[&id].clone(),
        history_reads: saved.history_reads[&id].clone(),
        retail_experience: session.runtime_retail_experience_for(&id),
    }
}

type Expected = Vec<(GroupIdx, Vec<(AccountId, PersonalPresence)>)>;
type Replies = Vec<(
    GroupIdx,
    Vec<(LocalIdx, AccountId, Option<BeliefParticipantSnapshot>)>,
)>;

/// 七名真实账户分为 3/3/1；尾组只含真实 local=0，不生成 padding。
fn fixture() -> (Expected, Replies) {
    let mut setup = fixture::setup_1stock();
    setup.npcs.retail_count = 6;
    let session = GameSession::new(setup, 42).expect("真实会话应可构造");
    let mut expected = Vec::new();
    let mut replies = Vec::new();
    for (group, ids) in [vec![0, 1, 2], vec![3, 4, 5], vec![6]]
        .into_iter()
        .enumerate()
    {
        let mut names = Vec::new();
        let mut slots = Vec::new();
        for (local, id) in ids.into_iter().enumerate() {
            let id = AccountId(id);
            let presence = if id == AccountId(0) {
                PersonalPresence::Absent(PersonalAbsenceReason::NoStrategy)
            } else {
                PersonalPresence::Required
            };
            let value = match presence {
                PersonalPresence::Required => Some(snapshot(&session, id)),
                PersonalPresence::Absent(_) => None,
            };
            names.push((id, presence));
            slots.push((local as LocalIdx, id, value));
        }
        expected.push((group as GroupIdx, names));
        replies.push((group as GroupIdx, slots));
    }
    (expected, replies)
}

fn rejects_without_mutation(expected: &Expected, replies: &Replies) {
    let before_expected = expected.clone();
    let before_replies = replies.clone();
    assert!(
        validate_personal_full(expected, replies).is_err(),
        "非法完整个人态必须显式返回错误"
    );
    assert_eq!(*expected, before_expected);
    assert_eq!(*replies, before_replies);
}

#[test]
fn accepts_seven_real_accounts_in_both_group_arrival_orders_without_mutation() {
    let (expected, mut replies) = fixture();
    let before = replies.clone();
    assert_eq!(validate_personal_full(&expected, &replies), Ok(()));
    assert_eq!(replies, before);
    replies.reverse();
    let reversed = replies.clone();
    assert_eq!(validate_personal_full(&expected, &replies), Ok(()));
    assert_eq!(replies, reversed);
}

#[test]
fn rejects_missing_group() {
    let (expected, mut replies) = fixture();
    replies.pop();
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_duplicate_group_even_when_reply_count_matches() {
    let (expected, mut replies) = fixture();
    replies[2] = replies[1].clone();
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_unknown_group() {
    let (expected, mut replies) = fixture();
    replies[2].0 = 99;
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_missing_real_slot_instead_of_treating_it_as_none() {
    let (expected, mut replies) = fixture();
    replies[1].1.pop();
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_tail_padding_even_when_value_is_none() {
    let (expected, mut replies) = fixture();
    replies[2].1.push((1, AccountId(7), None));
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_reordered_group_local_slots() {
    let (expected, mut replies) = fixture();
    replies[1].1.swap(0, 1);
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_local_coordinate_from_another_group() {
    let (expected, mut replies) = fixture();
    replies[1].1[0].0 = 3;
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_account_id_from_another_real_slot() {
    let (expected, mut replies) = fixture();
    replies[1].1[0].1 = AccountId(1);
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_missing_required_personal_value() {
    let (expected, mut replies) = fixture();
    replies[1].1[0].2 = None;
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_some_for_explicit_absent_personal_slot() {
    let (expected, mut replies) = fixture();
    replies[0].1[0].2 = replies[0].1[1].2.clone();
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_information_owner_from_another_person() {
    let (expected, mut replies) = fixture();
    let wrong = replies[0].1[1].2.as_ref().unwrap().information.clone();
    replies[1].1[0].2.as_mut().unwrap().information = wrong;
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn rejects_belief_owner_from_another_person() {
    let (expected, mut replies) = fixture();
    let wrong = replies[0].1[1].2.as_ref().unwrap().belief.clone();
    replies[1].1[0].2.as_mut().unwrap().belief = wrong;
    rejects_without_mutation(&expected, &replies);
}

#[test]
fn accepts_complete_rows_when_group_replies_arrive_in_reverse_order_without_mutation() {
    let session = GameSession::new(fixture::setup_1stock(), 42).expect("真实会话应可构造");
    let expected = vec![
        (
            0,
            vec![
                (
                    AccountId(0),
                    PersonalPresence::Absent(PersonalAbsenceReason::NoStrategy),
                ),
                (AccountId(1), PersonalPresence::Required),
            ],
        ),
        (1, vec![(AccountId(2), PersonalPresence::Required)]),
    ];
    let replies = vec![
        (
            1,
            vec![(0, AccountId(2), Some(snapshot(&session, AccountId(2))))],
        ),
        (
            0,
            vec![
                (0, AccountId(0), None),
                (1, AccountId(1), Some(snapshot(&session, AccountId(1)))),
            ],
        ),
    ];
    let expected_before = expected.clone();
    let replies_before = replies.clone();

    assert_eq!(validate_personal_full(&expected, &replies), Ok(()));
    assert_eq!(expected, expected_before);
    assert_eq!(replies, replies_before);
}
