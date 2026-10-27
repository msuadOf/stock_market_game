use super::world::BeliefParticipantSnapshot;
use super::{GroupIdx, LocalIdx};
use crate::AccountId;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PersonalFullError {
    NotImplemented,
}

pub(crate) fn validate_personal_full(
    expected: &[(GroupIdx, Vec<(AccountId, PersonalPresence)>)],
    replies: &[(
        GroupIdx,
        Vec<(LocalIdx, AccountId, Option<BeliefParticipantSnapshot>)>,
    )],
) -> Result<(), PersonalFullError> {
    unimplemented!("完整个人态校验尚未实现")
}
