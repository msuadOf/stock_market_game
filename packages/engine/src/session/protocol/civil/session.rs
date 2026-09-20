use super::{CivilUpdate, TickBatch, TickFrame};
use crate::session::{GameSession, SessionError, SessionSetup, StepFatal};

pub struct ProtocolSession {
    game: GameSession,
    intraday: Vec<TickFrame>,
    #[cfg(test)]
    malformed_civil: bool,
}

pub struct ProtocolCheckpoint {
    slot: crate::SaveSlot,
    intraday: Vec<TickFrame>,
}

impl ProtocolSession {
    pub fn checkpoint(&self) -> Result<ProtocolCheckpoint, StepFatal> {
        Ok(ProtocolCheckpoint {
            slot: self.game.save()?,
            intraday: self.intraday.clone(),
        })
    }

    pub fn rollback(&mut self, checkpoint: ProtocolCheckpoint) -> Result<(), SessionError> {
        let game = GameSession::restore(&checkpoint.slot)?;
        self.game = game;
        self.intraday = checkpoint.intraday;
        Ok(())
    }
    pub fn civil_day_ready(&self) -> Result<bool, SessionError> {
        Ok(self.game.day()
            == self
                .game
                .civil_clock()
                .completed_trading_sessions_expected()?)
    }

    pub fn enqueue_player_intent(
        &mut self,
        player: crate::AccountId,
        intent: crate::Intent,
    ) -> Result<(), SessionError> {
        self.game.enqueue_player_intent(player, intent)
    }

    pub fn restore(slot: &crate::SaveSlot) -> Result<Self, SessionError> {
        Ok(Self {
            game: GameSession::restore(slot)?,
            intraday: Vec::new(),
            #[cfg(test)]
            malformed_civil: false,
        })
    }

    pub fn new(setup: SessionSetup, seed: u64) -> Result<Self, SessionError> {
        Ok(Self {
            game: GameSession::new(setup, seed)?,
            intraday: Vec::new(),
            #[cfg(test)]
            malformed_civil: false,
        })
    }

    pub fn game(&self) -> &GameSession {
        &self.game
    }

    pub fn step_frame(&mut self) -> Result<TickFrame, StepFatal> {
        let frame = self.game.step_frame()?;
        frame.validate().map_err(protocol_fatal)?;
        self.intraday.push(frame.clone());
        Ok(frame)
    }

    pub fn end_civil_day_update(&mut self) -> Result<CivilUpdate, SessionError> {
        #[cfg(test)]
        let malformed = std::mem::take(&mut self.malformed_civil);
        let checkpoint = self.checkpoint()?;
        let result = self
            .game
            .end_civil_day_update(&self.intraday)
            .and_then(|update| {
                #[cfg(test)]
                let update = {
                    let mut update = update;
                    if malformed {
                        update.seq_to += 1;
                    }
                    update
                };
                update.validate().map_err(protocol_fatal)?;
                Ok(update)
            });
        let update = match result {
            Ok(update) => update,
            Err(error) => {
                self.rollback(checkpoint)?;
                return Err(error);
            }
        };
        self.intraday.clear();
        Ok(update)
    }

    pub fn tick_batch(&self, frames: Vec<TickFrame>) -> Result<TickBatch, StepFatal> {
        let batch = TickBatch {
            frames,
            runtime_snapshot: Some(self.game.runtime_snapshot()),
        };
        batch.validate().map_err(protocol_fatal)?;
        Ok(batch)
    }
}

impl std::ops::Deref for ProtocolSession {
    type Target = GameSession;

    fn deref(&self) -> &Self::Target {
        &self.game
    }
}

fn protocol_fatal(error: super::ProtocolError) -> StepFatal {
    StepFatal::InvariantViolation {
        location: "ProtocolSession publication".to_owned(),
        description: error.to_string(),
    }
}

#[cfg(test)]
mod rollback_tests {
    use super::*;

    #[test]
    fn malformed_civil_after_mutation_rolls_back_and_can_retry() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 41).unwrap();
        while !session.civil_day_ready().unwrap() {
            session.step_frame().unwrap();
        }
        let before = serde_json::to_value(session.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.intraday).unwrap();
        session.malformed_civil = true;

        let error = session.end_civil_day_update().unwrap_err();

        assert!(matches!(error, SessionError::Step(_)));
        assert_eq!(
            serde_json::to_value(session.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(serde_json::to_value(&session.intraday).unwrap(), history);
        let update = session.end_civil_day_update().unwrap();
        update.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&update.refresh.intraday).unwrap(),
            history
        );
    }
}
