use super::*;

pub(in crate::session) struct PlayerCandidateBatch {
    pub(in crate::session) intents: Vec<(AccountId, Intent)>,
}

impl GameSession {
    pub(in crate::session) fn capture_player_candidate_batch(&mut self) -> PlayerCandidateBatch {
        PlayerCandidateBatch {
            intents: std::mem::take(&mut self.pending_player),
        }
    }
}
