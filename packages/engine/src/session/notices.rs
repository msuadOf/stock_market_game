use super::*;
use crate::calendar::{CivilDateError, CivilInstant};
use crate::information::{
    AcquiredKind, NpcInformationState, NpcObservationContext, PublicLibrary, PublicationId,
    PublicationOrigin,
};
use crate::strategy::{BeliefBook, BeliefCause, BeliefInputs};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum NpcInformationCadence {
    Immediate,
    Daily { checks: u8 },
    EveryDays { days: u16 },
    Monthly,
}

impl NpcInformationCadence {
    pub(super) fn for_profile(
        profile: &crate::strategy::StrategyProfile,
        account: AccountId,
    ) -> Self {
        use crate::strategy::{RetailStyle, StrategyProfile};
        match profile {
            StrategyProfile::Institution(_) | StrategyProfile::Hot(_) => Self::Immediate,
            StrategyProfile::Retail(RetailStyle::Momentum | RetailStyle::Panic) => Self::Immediate,
            StrategyProfile::Retail(RetailStyle::Dormant) => {
                if account.0.is_multiple_of(2) {
                    Self::Monthly
                } else {
                    Self::EveryDays { days: 7 }
                }
            }
            StrategyProfile::Retail(RetailStyle::LongTerm) => Self::EveryDays {
                days: 1 + (account.0 % 5) as u16,
            },
            StrategyProfile::Retail(_) => Self::Daily {
                checks: 1 + (account.0 % 2) as u8,
            },
        }
    }

    pub(super) fn validate(self) -> Result<(), String> {
        match self {
            Self::Daily { checks } if !(1..=2).contains(&checks) => {
                Err("Daily checks 必须为1或2".into())
            }
            Self::EveryDays { days: 0 } => Err("EveryDays days 必须为正数".into()),
            _ => Ok(()),
        }
    }

    fn next_after(self, now: CivilInstant) -> Result<CivilInstant, CivilDateError> {
        let seconds = match self {
            Self::Immediate => return Ok(now),
            Self::Daily { checks: 2 } => {
                if now.second_of_day() < 9 * 3600 {
                    return CivilInstant::new(now.date(), 9 * 3600);
                }
                if now.second_of_day() < 18 * 3600 {
                    return CivilInstant::new(now.date(), 18 * 3600);
                }
                return CivilInstant::new(now.date().next()?, 9 * 3600);
            }
            Self::Daily { .. } => return CivilInstant::new(now.date().next()?, 18 * 3600),
            Self::EveryDays { days } => u32::from(days) * 86400,
            Self::Monthly => {
                let date = now.date();
                let (year, month) = if date.month() == 12 {
                    (date.year() + 1, 1)
                } else {
                    (date.year(), date.month() + 1)
                };
                let first = crate::CivilDate::from_ymd(year, month, 1)?;
                let after = if month == 12 {
                    crate::CivilDate::from_ymd(year + 1, 1, 1)?
                } else {
                    crate::CivilDate::from_ymd(year, month + 1, 1)?
                };
                let last_day = after.prev()?.day();
                return CivilInstant::new(
                    crate::CivilDate::from_ymd(
                        first.year(),
                        first.month(),
                        date.day().min(last_day),
                    )?,
                    now.second_of_day(),
                );
            }
        };
        let total = now.second_of_day() + seconds;
        let mut date = now.date();
        for _ in 0..total / 86400 {
            date = date.next()?;
        }
        CivilInstant::new(date, total % 86400)
    }
}

impl NpcAttentionState {
    pub(super) fn information_check_due(&self, now: CivilInstant) -> bool {
        self.information_cadence == NpcInformationCadence::Immediate
            || now >= self.next_information_check
    }

    pub(super) fn record_information_check(
        &mut self,
        now: CivilInstant,
    ) -> Result<(), CivilDateError> {
        self.next_information_check = self.information_cadence.next_after(now)?;
        Ok(())
    }
}

pub(super) fn material_cause(
    library: &PublicLibrary,
    report: PublicationId,
    now: CivilInstant,
) -> Result<BeliefCause, crate::information::InformationError> {
    Ok(
        if matches!(
            library.report(report, now)?.origin,
            PublicationOrigin::Correction
        ) {
            BeliefCause::Correction { report }
        } else {
            BeliefCause::NewMaterial { report }
        },
    )
}

pub(super) fn credit_default_cause(
    library: &PublicLibrary,
    id: PublicationId,
    now: CivilInstant,
) -> Result<Option<BeliefCause>, crate::information::InformationError> {
    let announcement = library.announcement(id, now)?;
    Ok(matches!(
        announcement.event.kind,
        crate::company::ShockKind::PaymentFailure {
            obligation_status: crate::company::events::PaymentObligationStatus::ContractualOverdue,
            ..
        }
    )
    .then_some(BeliefCause::CreditDefault { announcement: id }))
}

impl GameSession {
    pub(super) fn deliver_public_information(
        &mut self,
        now: CivilInstant,
    ) -> Result<(), SessionError> {
        let market = self.build_market_view();
        let ids: Vec<_> = self.state.belief_participants.keys().copied().collect();
        let results = ids
            .into_par_iter()
            .map(|id| {
                let attention = &self.state.npc_attention[&id];
                if !attention.information_check_due(now) {
                    return Ok(None);
                }
                let participant = &self.state.belief_participants[&id];
                let mut information = participant.information().clone();
                let mut belief = participant.belief().clone();
                let mut codes: BTreeSet<_> = self.state.accounts[&id]
                    .positions()
                    .keys()
                    .cloned()
                    .collect();
                codes.extend(participant.watchlist().stocks.keys().cloned());
                codes.extend(self.state.plans.active_codes(id).cloned());
                for code in &codes {
                    let Some(company) = self.state.company_system.issuers().issuer_of(code) else {
                        continue;
                    };
                    let mut new_reports = false;
                    let mut corrections = Vec::new();
                    let mut defaults = Vec::new();
                    for publication in
                        crate::information::discovery_candidates(&self.state.library, company, now)
                    {
                        if information.observed_at_of(publication).is_some() {
                            continue;
                        }
                        information
                            .record_acquisition(id, &self.state.library, publication, now)
                            .map_err(|error| {
                                SessionError::InvalidSave(format!(
                                    "NPC {id:?} 公告送达失败: {error}"
                                ))
                            })?;
                        match information
                            .records_for_company(company)
                            .iter()
                            .find(|record| record.id == publication)
                            .expect("刚登记的本人材料存在")
                            .kind
                        {
                            AcquiredKind::Report => {
                                new_reports = true;
                                if matches!(
                                    self.state
                                        .library
                                        .report(publication, now)
                                        .expect("刚获知报告可读取")
                                        .origin,
                                    PublicationOrigin::Correction
                                ) {
                                    corrections.push(publication);
                                }
                            }
                            AcquiredKind::Announcement => {
                                if let Some(cause) =
                                    credit_default_cause(&self.state.library, publication, now)
                                        .map_err(|error| {
                                            SessionError::Information(Box::new(error))
                                        })?
                                {
                                    defaults.push(cause);
                                }
                            }
                        }
                    }
                    let ctx =
                        NpcObservationContext::new(id, &information, &self.state.library, &market)
                            .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
                    let spec = self.state.company_system.issuers().get(company)
                        .expect("发行人身份存在");
                    let inputs = BeliefInputs {
                        ctx: &ctx,
                        company: company.clone(),
                        kind: spec.kind,
                        total_issued_shares: spec.issued_shares,
                        as_of_trading_day: self.stock_trading_day(code)?,
                    };
                    if new_reports {
                        let report = crate::strategy::preferred_own_report(&ctx, company)
                            .map_err(|error| SessionError::InvalidSave(error.to_string()))?
                            .expect("新获知报告存在");
                        belief
                            .apply_cause(
                                code,
                                material_cause(&self.state.library, report, now)
                                    .map_err(|error| SessionError::Information(Box::new(error)))?,
                                &inputs,
                            )
                            .map_err(|error| {
                                SessionError::InvalidSave(format!(
                                    "NPC {id:?} {code:?} 公告重估失败: {error}"
                                ))
                            })?;
                    }
                    for report in corrections {
                        belief
                            .apply_cause(code, BeliefCause::Correction { report }, &inputs)
                            .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
                    }
                    for cause in defaults {
                        if belief.entry(code).is_some() {
                            belief
                                .apply_cause(code, cause, &inputs)
                                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
                        }
                    }
                }
                let mut attention = attention.clone();
                attention
                    .record_information_check(now)
                    .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
                Ok::<_, SessionError>(Some((id, information, belief, attention)))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        for result in results.into_iter().flatten() {
            let (id, information, belief, attention) = result;
            #[cfg(feature = "simulation-diagnostics")]
            {
                let mut facts = Vec::new();
                for (company, records) in information.companies() {
                    for record in records {
                        if self.state.belief_participants[&id]
                            .information()
                            .observed_at_of(record.id)
                            .is_some()
                        {
                            continue;
                        }
                        let published = match record.kind {
                            AcquiredKind::Report => {
                                self.state
                                    .library
                                    .report(record.id, now)
                                    .expect("正式获知报告存在")
                                    .published_at
                            }
                            AcquiredKind::Announcement => {
                                self.state
                                    .library
                                    .announcement(record.id, now)
                                    .expect("正式获知公告存在")
                                    .published_at
                            }
                        };
                        facts.push(crate::diagnostics::causal::CausalFactKind::Acquisition {
                            account: id,
                            company: company.clone(),
                            publication: u64::from(record.id.value()),
                            published,
                            acquired: now,
                        });
                    }
                }
                let mut time = self.causal_time();
                time.civil = now;
                for fact in facts {
                    self.causal_record_at(time, fact);
                }
            }
            self.state
                .belief_participants
                .get_mut(&id)
                .expect("本人状态仍存在")
                .install_information_update(information, belief);
            *self
                .state
                .npc_attention
                .get_mut(&id)
                .expect("本人注意力仍存在") = attention;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_end_information_subscription_requires_current_holding_watchlist_or_active_plan() {
        for qualification in ["faded", "held", "watched", "active_plan"] {
            let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
            setup.stocks[0].float_shares = if qualification == "held" { 100 } else { 0 };
            setup.start_date = crate::CivilDate::from_ymd(2030, 1, 5).unwrap();
            let mut session = GameSession::new(setup, 42).unwrap();
            let account = AccountId(1);
            let code = session.state.setup.stocks[0].code.clone();
            let company = session
                .state
                .company_system.issuers()
                .issuer_of(&code)
                .unwrap()
                .clone();
            session
                .state
                .belief_participants
                .get_mut(&account)
                .unwrap()
                .watchlist_mut()
                .record_attention(&code, 0, 0)
                .unwrap();
            session
                .deliver_public_information(session.observation_civil_instant())
                .unwrap();
            let old_belief = session.state.belief_participants[&account]
                .belief()
                .entry(&code)
                .unwrap()
                .clone();
            let old_information = session.state.belief_participants[&account]
                .information()
                .clone();
            if qualification != "watched" {
                session
                    .state
                    .belief_participants
                    .get_mut(&account)
                    .unwrap()
                    .watchlist_mut()
                    .stocks
                    .remove(&code);
            }
            if qualification == "active_plan" {
                session
                    .state
                    .plans
                    .create(crate::plans::PlanOpen {
                        account,
                        code: code.clone(),
                        direction: Side::Buy,
                        target: crate::plans::PlanTarget::ShareCount(100),
                        opinion: crate::plans::PlanOpinion {
                            signal_score_bp: 8000,
                            source: crate::plans::OpinionSource::Blended,
                        },
                        confidence_bp: 6000,
                        urgency: crate::plans::Urgency::Patient,
                        horizon_trading_days: 20,
                        created_trading_day: 0,
                    })
                    .unwrap();
            }
            match qualification {
                "held" => assert_eq!(
                    session.state.accounts[&account]
                        .position(&code)
                        .unwrap()
                        .qty(),
                    100
                ),
                "watched" => assert!(session.state.belief_participants[&account]
                    .watchlist()
                    .stock(&code)
                    .is_some()),
                "active_plan" => assert!(session.state.plans.active_plan(account, &code).is_some()),
                "faded" => {
                    assert!(!session.state.accounts[&account]
                        .positions()
                        .contains_key(&code));
                    assert!(session.state.belief_participants[&account]
                        .watchlist()
                        .stock(&code)
                        .is_none());
                    assert!(session.state.plans.active_plan(account, &code).is_none());
                }
                _ => unreachable!(),
            }
            let date = session.civil_date();
            let (library, confirmed, _) = crate::session::company_assembly::financial_fixture_tests::confirmed_announcement_fixture(
                &session, crate::company::ActiveShock {
                    kind: crate::company::ShockKind::ContractWon,
                    amplitude_bp: 500,
                    starts_on: date,
                    expires_on: date.next().unwrap(),
                },
            );
            session.state.library = std::sync::Arc::new(library);
            let mut day_end = session.end_civil_day().unwrap();
            session.record_company_disclosure_events(&mut day_end, confirmed).unwrap();
            let publication = day_end
                .events
                .iter()
                .find_map(|event| {
                    let Event::CompanyDisclosurePublished {
                        publication_id,
                        company: issuer,
                        kind: CompanyDisclosureKind::Announcement,
                        ..
                    } = event
                    else {
                        return None;
                    };
                    let announcement = session
                        .state
                        .library
                        .announcement(*publication_id, day_end.disclosure_instant)
                        .unwrap();
                    (issuer == &company
                        && matches!(
                            announcement.event.kind,
                            crate::company::ShockKind::ContractWon
                        ))
                    .then_some(*publication_id)
                })
                .expect("真实日终经营公告必须已公开");
            let participant = &session.state.belief_participants[&account];
            if qualification == "faded" {
                assert!(!session.state.accounts[&account]
                    .positions()
                    .contains_key(&code));
                assert!(participant.watchlist().stock(&code).is_none());
                assert!(session.state.plans.active_plan(account, &code).is_none());
                assert_eq!(
                    participant.information().observed_at_of(publication),
                    None,
                    "历史belief不能恢复已经淡出的订阅资格"
                );
                assert_eq!(participant.information().save(), old_information.save());
                assert_eq!(participant.belief().entry(&code), Some(&old_belief));
            } else {
                assert_eq!(
                    participant.information().observed_at_of(publication),
                    Some(day_end.disclosure_instant),
                    "有效订阅资格 {qualification} 必须继续接收相关公开公告"
                );
                assert!(participant.belief().entry(&code).is_some());
            }
            let saved = session.save().unwrap();
            let restored = GameSession::restore(&saved).unwrap();
            assert_eq!(
                restored.state.belief_participants[&account]
                    .information()
                    .save(),
                participant.information().save()
            );
            assert_eq!(
                restored.state.belief_participants[&account].belief(),
                participant.belief()
            );
        }
    }

    #[test]
    fn information_check_cadence_preserves_personal_natural_month_and_half_day_schedule() {
        let january = crate::CivilDate::from_ymd(2030, 1, 31).unwrap();
        let now = crate::CivilInstant::from_hms(january, 10, 0, 0).unwrap();
        let next = NpcInformationCadence::Monthly.next_after(now).unwrap();
        assert_eq!(next.date().to_iso(), "2030-02-28");
        assert_eq!(next.second_of_day(), now.second_of_day());
        let half_day = NpcInformationCadence::Daily { checks: 2 }
            .next_after(now)
            .unwrap();
        assert_eq!(half_day.date(), january);
        assert_eq!(half_day.second_of_day(), 18 * 3600);
    }

    #[test]
    fn disclosure_delivery_updates_only_relevant_subscriber_without_order_or_price_experience() {
        let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
        setup.npcs.inst_count = 2;
        let mut session = GameSession::new(setup, 42).unwrap();
        let id = AccountId(1);
        let code = StockCode("600888".to_owned());
        session
            .state
            .belief_participants
            .get_mut(&id)
            .unwrap()
            .watchlist_mut()
            .record_attention(&code, 0, 0)
            .unwrap();
        let before = session.save().unwrap();
        let now = session.observation_civil_instant();
        session.deliver_public_information(now).unwrap();
        let participant = &session.state.belief_participants[&id];
        assert!(participant.information().acquired_count() > 0);
        assert!(participant.belief().entry(&code).is_some());
        assert_eq!(participant.price_memory(), &before.price_memories[&id]);
        assert_eq!(session.state.history_reads[&id], before.history_reads[&id]);
        assert_eq!(session.state.next_order_id, before.next_order_id);
        assert_eq!(
            session.state.accounts[&id].cash(),
            before.snapshot.accounts[&id].cash
        );
    }

    #[test]
    fn delayed_information_check_is_personal_and_survives_save_restore() {
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.npcs.retail_count = 2;
        let mut session = GameSession::new(setup, 41).unwrap();
        let code = session.state.setup.stocks[0].code.clone();
        let now = session.observation_civil_instant();
        let tomorrow = crate::CivilInstant::new(now.date().next().unwrap(), 18 * 3600).unwrap();
        for id in [AccountId(1), AccountId(2)] {
            session
                .state
                .belief_participants
                .get_mut(&id)
                .unwrap()
                .watchlist_mut()
                .record_attention(&code, 0, 0)
                .unwrap();
            let attention = session.state.npc_attention.get_mut(&id).unwrap();
            attention.information_cadence = NpcInformationCadence::EveryDays { days: 7 };
            attention.next_information_check = tomorrow;
        }
        session
            .state
            .npc_attention
            .get_mut(&AccountId(1))
            .unwrap()
            .information_cadence = NpcInformationCadence::Immediate;
        session.deliver_public_information(now).unwrap();
        assert!(
            session.state.belief_participants[&AccountId(1)]
                .information()
                .acquired_count()
                > 0
        );
        assert_eq!(
            session.state.belief_participants[&AccountId(2)]
                .information()
                .acquired_count(),
            0
        );
        let mut restored = GameSession::restore(&session.save().unwrap()).unwrap();
        assert_eq!(
            restored.state.npc_attention[&AccountId(2)].next_information_check,
            tomorrow
        );
        restored.deliver_public_information(tomorrow).unwrap();
        assert!(
            restored.state.belief_participants[&AccountId(2)]
                .information()
                .acquired_count()
                > 0
        );
        assert!(restored.state.npc_attention[&AccountId(2)].next_information_check > tomorrow);
    }

    #[test]
    fn public_payment_classification_does_not_turn_expense_or_deterioration_into_credit_default() {
        use crate::company::events::PaymentObligationStatus;
        use crate::information::{AnnouncedEvent, AnnouncementRequest};
        let date = crate::CivilDate::from_ymd(2030, 1, 5).unwrap();
        let now = crate::CivilInstant::new(date, 18 * 3600).unwrap();
        let mut library = PublicLibrary::new();
        for (kind, expected) in [
            (crate::company::ShockKind::CreditDeterioration, false),
            (
                crate::company::ShockKind::PaymentFailure {
                    what: "采购未执行".into(),
                    amount: crate::accounting::AccountingAmount::from_cents(10000),
                    obligation_status: PaymentObligationStatus::UncommittedExpense,
                },
                false,
            ),
            (
                crate::company::ShockKind::PaymentFailure {
                    what: "到期本金未付".into(),
                    amount: crate::accounting::AccountingAmount::from_cents(10000),
                    obligation_status: PaymentObligationStatus::ContractualOverdue,
                },
                true,
            ),
        ] {
            let id = library
                .publish_announcement(AnnouncementRequest {
                    company: crate::company::CompanyId("C-1".into()),
                    occurred_on: date,
                    published_at: now,
                    event: AnnouncedEvent {
                        kind,
                        amplitude_bp: 0,
                        starts_on: date,
                        expires_on: date,
                    },
                })
                .unwrap();
            assert_eq!(
                credit_default_cause(&library, id, now).unwrap().is_some(),
                expected
            );
            assert!(credit_default_cause(
                &library,
                id,
                crate::CivilInstant::new(date, 17 * 3600).unwrap()
            )
            .is_err());
        }
    }

    #[test]
    fn relevant_correction_is_delivered_as_direct_revaluation_not_ordinary_material() {
        let mut session =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        let id = AccountId(1);
        let code = session.state.setup.stocks[0].code.clone();
        session
            .state
            .belief_participants
            .get_mut(&id)
            .unwrap()
            .watchlist_mut()
            .record_attention(&code, 0, 0)
            .unwrap();
        let now = session.observation_civil_instant();
        session.deliver_public_information(now).unwrap();
        let prior = session.state.belief_participants[&id]
            .belief()
            .entry(&code)
            .unwrap()
            .used_report_ids[0];
        assert_eq!(
            session
                .state
                .library
                .report(prior, now)
                .unwrap()
                .reports
                .kind,
            crate::accounting::reports::ReportKind::Annual
        );
        assert!(
            session.state.belief_participants[&id]
                .belief()
                .entry(&code)
                .unwrap()
                .used_report_ids
                .len()
                > 1,
            "本case必须先使用较新的中期报告，不能退化成年报到年报"
        );
        let mut saved = session.state.library.save();
        let mut correction = saved
            .reports
            .iter()
            .find(|report| report.id == prior)
            .unwrap()
            .clone();
        let corrected = PublicationId::new(saved.next_seq);
        saved.next_seq += 1;
        correction.id = corrected;
        correction.origin = PublicationOrigin::Correction;
        correction.supersedes = Some(prior);
        correction.reports.version.sequence += 1;
        correction.reports.version.supersedes = Some(correction.reports.version.sequence - 1);
        correction.reports.version.kind = crate::accounting::reports::VersionKind::Correction {
            reason: "本人的公开更正重估测试".into(),
        };
        saved.reports.push(correction);
        session.state.library = std::sync::Arc::new(PublicLibrary::from_parts(saved).unwrap());
        session.deliver_public_information(now).unwrap();
        let entry = session.state.belief_participants[&id]
            .belief()
            .entry(&code)
            .unwrap();
        assert_eq!(
            entry.last_cause.as_ref().unwrap().cause,
            BeliefCause::Correction { report: corrected }
        );
        assert!(entry.used_report_ids.contains(&corrected));
    }

    #[test]
    fn successful_nontrading_day_end_delivers_at_real_disclosure_boundary() {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::CivilDate::from_ymd(2030, 1, 5).unwrap();
        let mut session = GameSession::new(setup, 42).unwrap();
        let id = AccountId(1);
        let code = session.state.setup.stocks[0].code.clone();
        let company = session
            .state
            .company_system.issuers()
            .issuer_of(&code)
            .unwrap()
            .clone();
        session
            .state
            .belief_participants
            .get_mut(&id)
            .unwrap()
            .watchlist_mut()
            .record_attention(&code, 0, 0)
            .unwrap();
        let date = session.civil_date();
        let (library, confirmed, _) = crate::session::company_assembly::financial_fixture_tests::confirmed_announcement_fixture(
            &session, crate::company::ActiveShock {
                kind: crate::company::ShockKind::ContractWon,
                amplitude_bp: 500,
                starts_on: date,
                expires_on: date.next().unwrap(),
            },
        );
        session.state.library = std::sync::Arc::new(library);
        let cash = session.state.accounts[&id].cash();
        let next_order = session.state.next_order_id;
        assert_eq!(
            session.state.belief_participants[&id]
                .information()
                .acquired_count(),
            0
        );
        let mut day_end = session.end_civil_day().unwrap();
        session.record_company_disclosure_events(&mut day_end, confirmed).unwrap();
        let information = session.state.belief_participants[&id].information();
        assert!(information.acquired_count() > 0);
        assert!(information
            .companies()
            .flat_map(|(_, records)| records)
            .all(|record| record.observed_at == day_end.disclosure_instant));
        assert_eq!(session.state.accounts[&id].cash(), cash);
        assert_eq!(session.state.next_order_id, next_order);
        assert!(session.state.history_reads[&id].stocks.is_empty());
        #[cfg(feature = "simulation-diagnostics")]
        assert!(session.causal_facts().iter().any(|fact| matches!(&fact.kind, crate::diagnostics::causal::CausalFactKind::Acquisition { account, published, acquired, .. } if *account == id && *published == day_end.disclosure_instant && *acquired == day_end.disclosure_instant)));
        let saved = session.save().unwrap();
        assert_eq!(
            serde_json::to_value(GameSession::restore(&saved).unwrap().save().unwrap()).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
    }

    #[test]
    fn information_revaluation_failure_rolls_back_entire_day_end() {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::CivilDate::from_ymd(2032, 1, 31).unwrap();
        let mut session = GameSession::new(setup, 42).unwrap();
        assert_eq!(session.civil_clock().phase(), CivilPhase::ClosedDay);
        let id = AccountId(1);
        let code = session.state.setup.stocks[0].code.clone();
        let company = session.state.company_system.issuers().issuer_of(&code).unwrap().clone();
        let settled = session.civil_date();
        let period = crate::accounting::AccountingPeriod::of_date(settled);
        let scope = crate::accounting::consolidation::ScopeId::Standalone(
            crate::accounting::consolidation::MemberId(company.0.clone()),
        );
        let finance = session.state.company_system.finance(&company).unwrap();
        assert!(!finance.books().journal().entries().any(|entry| entry.date == settled));
        assert!(finance.closing().versions(&scope, period, crate::accounting::reports::ReportKind::Monthly).is_empty());
        session
            .state
            .belief_participants
            .get_mut(&id)
            .unwrap()
            .watchlist_mut()
            .record_attention(&code, 0, 0)
            .unwrap();
        let original_library = session.state.library.clone();
        let mut public = original_library.save();
        public
            .reports
            .retain(|report| report.reports.kind != crate::accounting::reports::ReportKind::Annual);
        session.state.library = std::sync::Arc::new(PublicLibrary::from_parts(public).unwrap());
        let before = session.business_state_hash().unwrap();
        let save = serde_json::to_value(session.save().unwrap()).unwrap();
        let error = session.end_civil_day().unwrap_err();
        assert!(
            matches!(&error, SessionError::InvalidSave(message)
                if message.contains("公告重估失败") && message.contains("no own-known annual material")),
            "{error}"
        );
        assert_eq!(session.business_state_hash().unwrap(), before);
        assert_eq!(serde_json::to_value(session.save().unwrap()).unwrap(), save);
        session.state.library = original_library;
        let completed = session.end_civil_day().unwrap();
        assert_eq!(completed.settled_date, settled);
        let finance = session.state.company_system.finance(&company).unwrap();
        let summaries = finance.books().journal().entries().filter(|entry| entry.date == settled && entry.kind == crate::accounting::BusinessKind::SimplePeriodSummary).collect::<Vec<_>>();
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].lines.iter().all(|line| line.amount.is_positive()));
        let taxes = finance.books().journal().entries().filter(|entry| entry.date == settled && entry.kind == crate::accounting::BusinessKind::TaxAccrual).collect::<Vec<_>>();
        assert_eq!(taxes.len(), 1);
        assert!(taxes[0].lines.iter().all(|line| line.amount.is_positive()));
        assert_eq!(finance.closing().versions(&scope, period, crate::accounting::reports::ReportKind::Monthly).len(), 1);
        let report = finance.report(period, crate::accounting::reports::ReportKind::Monthly).unwrap();
        assert!(report.income.quarter.line_amount(crate::accounting::reports::IncomeLine::OperatingRevenue).unwrap().is_positive());
        assert!(report.income.quarter.income_tax.is_positive());
        let committed = session.business_state_hash().unwrap();
        let mut repeated = session.state.company_system.export_state();
        let repeated_before = serde_json::to_value(&repeated).unwrap();
        assert!(matches!(repeated.advance_day(settled), Err(crate::company::CompanySystemError::Invalid(message))
            if message.contains("日期必须连续推进") && message.contains(&settled.to_iso())));
        assert_eq!(serde_json::to_value(repeated).unwrap(), repeated_before);
        assert_eq!(session.business_state_hash().unwrap(), committed);
    }

    #[test]
    fn ignored_old_correction_without_entry_and_foreign_material_are_typed_errors() {
        let session =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        let id = AccountId(1);
        let code = session.state.setup.stocks[0].code.clone();
        let company = session.state.company_system.issuers().issuer_of(&code).unwrap();
        let now = session.observation_civil_instant();
        let older = session
            .state
            .library
            .reports_for_company(company, now)
            .into_iter()
            .min_by_key(|report| crate::strategy::own_known_report_priority(report))
            .unwrap()
            .id;
        let mut information = NpcInformationState::new(id);
        for report in session.state.library.reports_for_company(company, now) {
            information
                .record_acquisition(id, &session.state.library, report.id, now)
                .unwrap();
        }
        let market = session.build_market_view();
        let ctx =
            NpcObservationContext::new(id, &information, &session.state.library, &market).unwrap();
        let spec = session.state.company_system.issuers().get(company).unwrap();
        let mut inputs = BeliefInputs {
            ctx: &ctx,
            company: company.clone(),
            kind: spec.kind,
            total_issued_shares: spec.issued_shares,
            as_of_trading_day: 0,
        };
        let mut belief = session.state.belief_participants[&id].belief().clone();
        assert!(matches!(
            belief.apply_cause(&code, BeliefCause::Correction { report: older }, &inputs),
            Err(crate::strategy::BeliefError::NoBeliefEntry)
        ));
        inputs.company = crate::company::CompanyId("foreign-company".into());
        assert!(matches!(
            belief.apply_cause(&code, BeliefCause::Correction { report: older }, &inputs),
            Err(crate::strategy::BeliefError::MaterialNotForCompany { .. })
        ));
    }
}
