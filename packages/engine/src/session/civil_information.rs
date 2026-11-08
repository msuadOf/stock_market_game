//! 自然日经营与 Group INFO 共用唯一 Admin 事务；公开投影在首 Commit 前完成。

use super::*;
use crate::runtime::world::{
    InformationMemoryRetention, InformationObservation, InformationPreparePayload,
};
use std::sync::Arc;

/// 只能从已完成经营、披露及同次 Admin overlay 的 shadow 提取，字段不接受外部拼装。
pub(super) struct PreparedCivilFields {
    companies: SessionCompanies,
    civil_clock: CivilClock,
    disclosures: DisclosureDispatch,
    report_corrections: ReportCorrectionStatus,
    seq: u64,
    retained: Option<Arc<crate::runtime::publication::RetainedDaySnapshot>>,
}

impl PreparedCivilFields {
    fn from_shadow(
        shadow: GameSession,
        next_date: crate::CivilDate,
        seq_from: u64,
        retained: Option<Arc<crate::runtime::publication::RetainedDaySnapshot>>,
    ) -> Result<Self, SessionError> {
        if shadow.civil_date() != next_date
            || shadow.seq() < seq_from
            || shadow.seq() > crate::orderbook::js_safe_u64::MAX
        {
            return Err(SessionError::Step(StepFatal::InvariantViolation {
                location: "PreparedCivilFields::from_shadow".to_owned(),
                description: "civil 候选日期或 seq 与预验证 report 不一致".to_owned(),
            }));
        }
        Ok(Self {
            companies: shadow.state.companies,
            civil_clock: shadow.state.civil_clock,
            disclosures: shadow.state.disclosures,
            report_corrections: shadow.state.report_corrections,
            seq: shadow.state.seq,
            retained,
        })
    }

    /// 首 Commit 后只移动预验证五域；不接受 driver、callback 或可失败工作。
    pub(super) fn install_into(
        self,
        session: &mut GameSession,
    ) -> (
        crate::CivilDate,
        Option<Arc<crate::runtime::publication::RetainedDaySnapshot>>,
    ) {
        session.state.companies = self.companies;
        session.state.civil_clock = self.civil_clock;
        session.state.disclosures = self.disclosures;
        session.state.report_corrections = self.report_corrections;
        session.state.seq = self.seq;
        if let Some(retained) = &self.retained {
            session.set_runtime_retained_day(Arc::clone(retained));
        }
        session.market_calendar_day = None;
        (session.state.civil_clock.current_date(), self.retained)
    }
}

impl GameSession {
    pub(super) fn poison_civil_authority(&mut self, error: SessionError) -> SessionError {
        match error {
            SessionError::Step(fatal) => SessionError::Step(self.poison_runtime_step(fatal)),
            error => {
                self.poison_runtime_step(StepFatal::InvariantViolation {
                    location: "GameSession::with_prepared_runtime_civil_day".to_owned(),
                    description: format!("civil authority 无法继续运行：{error}"),
                });
                // 封存终态但首个返回仍保留原 Calendar／Information／数值错误类型与上下文。
                error
            }
        }
    }
    #[cfg(test)]
    pub(super) fn with_prepared_runtime_civil_day<P>(
        &mut self,
        project: impl FnOnce(&GameSession, CivilDayEndReport) -> Result<P, SessionError>,
    ) -> Result<P, SessionError> {
        self.with_prepared_runtime_civil_day_classified(project)
            .map_err(protocol::CivilDayEndFailure::into_error)
    }

    pub(super) fn with_prepared_runtime_civil_day_classified<P>(
        &mut self,
        project: impl FnOnce(&GameSession, CivilDayEndReport) -> Result<P, SessionError>,
    ) -> Result<P, protocol::CivilDayEndFailure> {
        // 当前未持有 World 的真实已提交收盘记录；候选业务拒绝不能先改变 authority/revision。
        let probe = match self.runtime_driver.as_ref() {
            None => Ok(false),
            Some(cell) => match cell.lock() {
                Ok(driver) => driver
                    .has_committed_market_close_for_civil(self.tick(), self.civil_date())
                    .map_err(|source| {
                        SessionError::CivilInformation(
                            super::administrative_mirror::CivilInformationError::new(
                                self.civil_date(),
                                source,
                            ),
                        )
                    }),
                Err(_) => Err(SessionError::Step(StepFatal::InvariantViolation {
                    location: "GameSession::with_prepared_runtime_civil_day".to_owned(),
                    description: "civil 入口 driver 锁被 poison".to_owned(),
                })),
            },
        };
        let market_installed = match probe {
            Ok(installed) => installed,
            // 此探测只读 committed record；错误仅可能来自已死亡 World 或已 poison 的锁。
            Err(error) => return Err(self.poison_civil_authority(error).into()),
        };
        let mut repurchase_inspect_rejected = false;
        let mut correction_prepare_rejected = false;
        let mut new_due_horizon_rejected = false;
        let prepared: Result<_, SessionError> = (|| {
            let mut shadow = self
                .clone_for_tick_shadow()
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(1034, vec![0])))?;
            let report = shadow
                .prepare_civil_day_disclosures()
                .map_err(|failure| match failure {
                    CivilDayPreparationFailure::Ordinary(error) => error,
                    CivilDayPreparationFailure::NewlyScheduledDueBeyondHorizon(error) => {
                        new_due_horizon_rejected = true;
                        error
                    }
                })
                .inspect_err(|error| {
                    correction_prepare_rejected = self.poison_reason().is_none()
                        && matches!(error, SessionError::ReportCorrectionDayEnd(cause)
                        if matches!(cause.classification(),
                            crate::company::CompanyErrorClass::InvalidInput
                            | crate::company::CompanyErrorClass::BusinessCondition));
                })?;

            let instant = report.disclosure_instant;
            let next_date = report.next_date;
            let seq_from = self.seq();
            if report.settled_date != self.civil_date()
                || report.events.iter().enumerate().any(|(index, event)| {
                    seq_from.checked_add(index as u64 + 1) != Some(event.seq())
                })
                || seq_from.checked_add(report.events.len() as u64) != Some(shadow.seq())
            {
                return Err(SessionError::Step(StepFatal::InvariantViolation {
                    location: "GameSession::with_prepared_runtime_civil_day".to_owned(),
                    description: "civil report 日期或事件 seq 未绑定原 Session".to_owned(),
                }));
            }
            // 经营、更正及 report 校验已成功；首次接管仍使用原 live，后续共用唯一 Admin。
            let mut initial_full = None;
            if self.runtime_driver.is_none() {
                // 税账业务拒绝必须先于静点接管，避免失败时发布盘口镜像和序号。
                shadow.validate_ordinary_tax_book_coverage(report.settled_date)?;
                initial_full = Some(self.construct_runtime_driver_for_civil()?);
                // 静态接管也会更新 desk/ledger 镜像；callback 必须使用接管后的同版非 civil 基线。
                // 已准备的六域只 move 到新 shadow，不重复经营或更正 Prepare；live 仍只由固定尾安装。
                let mut adopted = self
                    .clone_for_tick_shadow()
                    .map_err(|_| SessionError::InvalidSave(ValidationFault::new(1034, vec![0])))?;
                adopted.state.companies = shadow.state.companies;
                adopted.state.civil_clock = shadow.state.civil_clock;
                adopted.state.disclosures = shadow.state.disclosures;
                adopted.state.report_corrections = shadow.state.report_corrections;
                adopted.state.seq = shadow.state.seq;
                adopted.state.library = shadow.state.library;
                adopted.market_calendar_day = None;
                shadow = adopted;
            }
            let baseline = self.prepare_civil_administrative_baseline(initial_full)?;
            shadow.prepare_repurchase_sources(report.settled_date, &baseline)?;
            shadow.prepare_ordinary_tax_day_end(report.settled_date, market_installed)?;
            let (repurchase_stages, repurchase) =
                shadow.prepare_repurchase_closing(report.settled_date, &baseline)?;
            let repurchase_sources = baseline.repurchase.clone();
            let (mut stages, mut cash) =
                shadow.prepare_civil_cash_distributions(report.settled_date, &baseline)?;
            cash.include_repurchase(&baseline, &stages, &repurchase_stages, repurchase.as_ref())?;
            stages.extend(repurchase_stages);
            let mut desks = shadow.prepare_rights_ex_anchors(next_date)?;
            desks.extend(shadow.prepare_stock_ex_anchors(next_date)?);
            desks.extend(shadow.prepare_split_ex_anchors(next_date)?);
            // 统一使用真实 Desk 的新旧参考价比率；余数分配不产生逐户价格单位。
            let price_unit_changes = desks
                .iter()
                .filter(|request| {
                    shadow.corporate_actions().is_some_and(|actions| {
                        actions.applied_ex_reference_groups.iter().any(|group| {
                            group.date == next_date
                                && shadow
                                    .state
                                    .stocks
                                    .id(&group.stock.0)
                                    .is_some_and(|stock| stock.0 == u32::from(request.stock))
                                && (!group.stock_event_ids.is_empty()
                                    || !group.split_event_ids.is_empty())
                        })
                    })
                })
                .copied()
                .collect();
            let expected_desks = desks.clone();
            let clock =
                crate::runtime::InformationClockContext::for_civil_disclosure(self, instant)?;
            let as_of_anchors = clock.anchors().map_err(|source| {
                SessionError::CivilInformation(
                    super::administrative_mirror::CivilInformationError::new(
                        self.civil_date(),
                        source,
                    ),
                )
            })?;
            let facts = shadow.runtime_company_read_facts()?;
            let payload = InformationPreparePayload {
                price_unit_changes,
                library: Arc::clone(&shadow.state.library),
                current_facts: Arc::clone(&facts),
                observation: Some(InformationObservation {
                    clock,
                    through: instant,
                    as_of_anchors,
                    market: Arc::new(self.build_market_view()),
                    company_facts: facts,
                    memory_retention: InformationMemoryRetention::CivilDayEnd,
                }),
            };
            let repurchase_rejected = &mut repurchase_inspect_rejected;
            self.apply_civil_information_administrative_operations(
                baseline,
                stages,
                payload,
                desks,
                repurchase,
                move |candidate, prepared, stages| {
                    cash.inspect(candidate, stages)?;
                    super::issuer_repurchase_execution::inspect_repurchase_sources(
                        candidate,
                        &repurchase_sources,
                    )
                    .inspect_err(|_| *repurchase_rejected = true)?;
                    if candidate
                        .desks
                        .iter()
                        .map(|desk| desk.request)
                        .collect::<Vec<_>>()
                        != expected_desks
                    {
                        return Err(SessionError::UnsupportedCapitalBoundary(
                            "交易部除权回执与原请求不一致".into(),
                        ));
                    }
                    prepared.clone_into_civil_shadow(&mut shadow);
                    shadow.market_calendar_day = None;
                    let previous_len = shadow.retained_market_days().len();
                    shadow.freeze_serial_closed_retained_day_if_needed(report.settled_date)?;
                    let retained =
                        (shadow.retained_market_days().len() > previous_len).then(|| {
                            Arc::clone(
                                &shadow
                                    .retained_market_days()
                                    .last()
                                    .expect("新增保留日必须存在")
                                    .0,
                            )
                        });
                    let publication = project(&shadow, report)?;
                    let fields =
                        PreparedCivilFields::from_shadow(shadow, next_date, seq_from, retained)?;
                    Ok((fields, (instant, publication)))
                },
            )
        })();
        let (instant, publication) = match prepared {
            Ok(value) => value,
            Err(error) => {
                // 已封存首错或 World terminal 优先，不能被更正业务拒绝掩盖。
                if let Some(fatal) = self.poison_reason() {
                    return Err(SessionError::Step(fatal.clone()).into());
                }
                let terminal = match self.runtime_driver.as_ref() {
                    None => Ok(false),
                    Some(cell) => match cell.lock() {
                        Ok(driver) => Ok(driver.administrative_world_is_terminal()),
                        Err(_) => Err(SessionError::Step(StepFatal::InvariantViolation {
                            location: "GameSession::with_prepared_runtime_civil_day".to_owned(),
                            description: "civil 失败处理时 driver 锁被 poison".to_owned(),
                        })),
                    },
                };
                let terminal = match terminal {
                    Ok(value) => value,
                    Err(error) => return Err(self.poison_civil_authority(error).into()),
                };
                if terminal {
                    return Err(self.poison_civil_authority(error).into());
                }
                if new_due_horizon_rejected && self.poison_reason().is_none() {
                    return Err(error.into());
                }
                if let SessionError::ReportCorrectionDayEnd(cause) = &error {
                    if cause.classification() == crate::company::CompanyErrorClass::SystemState {
                        return Err(self.poison_civil_authority(error).into());
                    }
                    if correction_prepare_rejected {
                        // 仅真实更正 prepare 的两类业务拒绝允许取消后重试 civil。
                        return Err(protocol::CivilDayEndFailure::correction_prepare_rejected(
                            error,
                        ));
                    }
                }
                if repurchase_inspect_rejected {
                    // 同次 Admin 已确认健康 Abort；保留真实收盘来源，可重试 civil 候选。
                    return Err(error.into());
                }
                if market_installed {
                    return Err(self.poison_civil_authority(error).into());
                }
                return Err(error.into());
            }
        };
        for observer in self.state.civil_clock.disclosure_observers() {
            observer(instant);
        }
        Ok(publication)
    }
}
