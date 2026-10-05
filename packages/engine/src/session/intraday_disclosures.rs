use super::*;

impl GameSession {
    pub(super) fn publish_intraday_reports(
        &mut self,
        now: CivilInstant,
    ) -> Result<Vec<Event>, SessionError> {
        if !matches!(
            self.state.setup.report_frequency,
            crate::information::ReportFrequency::Monthly { .. }
        ) {
            return Ok(Vec::new());
        }
        let ids = self
            .state
            .disclosures
            .run_simple_scheduled(disclosures::SimpleScheduledDisclosureCtx {
                report_frequency: self.state.setup.report_frequency,
                through: now,
                system: &self.state.company_system,
                seed: self.state.seed,
                library: std::sync::Arc::make_mut(&mut self.state.library),
            })
            .map_err(SessionError::Disclosure)?;
        let mut events = Vec::with_capacity(ids.len());
        let mut last_delivered = None;
        for id in ids {
            let published = self
                .state
                .library
                .report(id, now)
                .map_err(|error| SessionError::Information(Box::new(error)))?;
            let (company, published_at, report_revision) = (
                published.company.clone(),
                published.published_at,
                published.reports.version.sequence,
            );
            if last_delivered != Some(published_at) {
                self.deliver_public_information(published_at)?;
                last_delivered = Some(published_at);
            }
            events.push(Event::CompanyDisclosurePublished {
                seq: self.next_seq(),
                publication_id: id,
                company,
                published_at,
                kind: CompanyDisclosureKind::Report { report_revision },
            });
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_tick_after_monthly_publication_keeps_authoritative_library_and_beliefs_unchanged() {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.start_date = CivilDate::from_ymd(2030, 1, 2).unwrap();
        setup.ticks_per_day = 120;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        setup.report_frequency = crate::information::ReportFrequency::Monthly {
            schedule: crate::information::MonthlyReportSchedule::Custom {
                day: 2,
                second_of_day: 37800,
                delay: crate::information::MonthlyReportDelay::None,
            },
        };
        let mut session = GameSession::new(setup, 42).unwrap();
        for _ in 0..29 {
            session.step().unwrap();
        }
        let before_hash = session.business_state_hash().unwrap();
        let before_library = session.state.library.clone();
        let before_participants = session
            .state
            .belief_participants
            .iter()
            .map(|(account, participant)| {
                (
                    *account,
                    participant.information().clone(),
                    participant.belief().clone(),
                )
            })
            .collect::<Vec<_>>();
        session.inject_post_shadow_failure(StepFatal::InvariantViolation {
            location: "月报私有candidate已完成".into(),
            description: "模拟commit前失败".into(),
        });
        assert!(
            matches!(session.step(), Err(StepFatal::InvariantViolation { location, .. }) if location == "月报私有candidate已完成")
        );
        assert_eq!(session.state.library, before_library);
        let after_participants = session
            .state
            .belief_participants
            .iter()
            .map(|(account, participant)| {
                (
                    *account,
                    participant.information().clone(),
                    participant.belief().clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(after_participants, before_participants);
        assert_eq!(session.business_state_hash().unwrap(), before_hash);
    }
}
