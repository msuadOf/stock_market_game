use super::{GroupIdx, LocalIdx};
use crate::AccountId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PersonalAbsenceReason {
    NoStrategy,
    NoBeliefParticipant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PersonalPresence {
    Required,
    Absent(PersonalAbsenceReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PersonalFullError {
    InvalidRoster {
        detail: String,
    },
    UnknownGroup {
        group: GroupIdx,
    },
    DuplicateGroup {
        group: GroupIdx,
    },
    MissingGroup {
        group: GroupIdx,
    },
    SlotCount {
        group: GroupIdx,
        expected: usize,
        actual: usize,
    },
    LocalOrder {
        group: GroupIdx,
        expected: usize,
        actual: LocalIdx,
    },
    AccountMismatch {
        group: GroupIdx,
        local: LocalIdx,
        expected: AccountId,
        actual: AccountId,
    },
    MissingPersonal {
        account: AccountId,
    },
    UnexpectedPersonal {
        account: AccountId,
    },
    InformationOwnerMismatch {
        account: AccountId,
        actual: AccountId,
    },
    BeliefOwnerMismatch {
        account: AccountId,
        actual: AccountId,
    },
}

pub(crate) fn validate_personal_full(
    expected: &[super::type_aliases::PersonalPresenceGroup],
    replies: &[super::type_aliases::PersonalFullGroup],
) -> Result<(), PersonalFullError> {
    let mut roster = BTreeMap::new();
    let mut owners = BTreeSet::new();
    for (index, (group, members)) in expected.iter().enumerate() {
        if usize::from(*group) != index || roster.insert(*group, members).is_some() {
            return Err(PersonalFullError::InvalidRoster {
                detail: format!("完整个人态名册组号不连续或重复：index={index} group={group}"),
            });
        }
        for (account, _) in members {
            if !owners.insert(*account) {
                return Err(PersonalFullError::InvalidRoster {
                    detail: format!("完整个人态名册账户重复：{account:?}"),
                });
            }
        }
    }
    let mut seen = BTreeSet::new();
    for (group, slots) in replies {
        let members = roster
            .get(group)
            .ok_or(PersonalFullError::UnknownGroup { group: *group })?;
        if !seen.insert(*group) {
            return Err(PersonalFullError::DuplicateGroup { group: *group });
        }
        if slots.len() != members.len() {
            return Err(PersonalFullError::SlotCount {
                group: *group,
                expected: members.len(),
                actual: slots.len(),
            });
        }
        for (index, ((local, account, personal), (expected_account, presence))) in
            slots.iter().zip(members.iter()).enumerate()
        {
            if usize::try_from(*local) != Ok(index) {
                return Err(PersonalFullError::LocalOrder {
                    group: *group,
                    expected: index,
                    actual: *local,
                });
            }
            if account != expected_account {
                return Err(PersonalFullError::AccountMismatch {
                    group: *group,
                    local: *local,
                    expected: *expected_account,
                    actual: *account,
                });
            }
            match (presence, personal) {
                (PersonalPresence::Required, None) => {
                    return Err(PersonalFullError::MissingPersonal { account: *account });
                }
                (PersonalPresence::Absent(_), Some(_)) => {
                    return Err(PersonalFullError::UnexpectedPersonal { account: *account });
                }
                (_, Some(value)) => {
                    if value.information.owner() != *account {
                        return Err(PersonalFullError::InformationOwnerMismatch {
                            account: *account,
                            actual: value.information.owner(),
                        });
                    }
                    if value.belief.npc() != *account {
                        return Err(PersonalFullError::BeliefOwnerMismatch {
                            account: *account,
                            actual: value.belief.npc(),
                        });
                    }
                }
                (PersonalPresence::Absent(_), None) => {}
            }
        }
    }
    for group in roster.keys() {
        if !seen.contains(group) {
            return Err(PersonalFullError::MissingGroup { group: *group });
        }
    }
    Ok(())
}
