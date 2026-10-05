use crate::accounting::closing::{ClosingEngine, ClosingError, CorrectionRequest, ReportHandle};
use crate::accounting::consolidation::MemberId;
use crate::accounting::reports::IndustryPresentation;
use crate::accounting::{AccountingError, Books, IncomeTaxPolicy};
use crate::calendar::CivilDate;
use crate::company::income_tax::{IncomeTaxOwnerError, IncomeTaxPosition};
use crate::information::{InformationError, PublicLibrary, PublicationId, PublicationRequest};

#[derive(Debug, thiserror::Error)]
pub enum CompanyCorrectionError {
    #[error("company correction failed for reason '{reason}': Simple finance: {cause}")]
    SimpleFinance {
        reason: String,
        #[source]
        cause: Box<crate::company::simple::SimpleFinanceError>,
    },
    #[error("company correction failed for reason '{reason}': owner: {cause}")]
    Owner {
        reason: String,
        #[source]
        cause: Box<crate::company::operations::OperationsError>,
    },
    #[error("company correction failed for reason '{reason}': owner restoration: {cause}")]
    Serialization {
        reason: String,
        #[source]
        cause: Box<serde_json::Error>,
    },
    #[error("company correction failed for reason '{reason}': accounting: {cause}")]
    Accounting {
        reason: String,
        #[source]
        cause: Box<AccountingError>,
    },
    #[error("company correction failed for reason '{reason}': tax state: {cause}")]
    TaxState {
        reason: String,
        #[source]
        cause: Box<IncomeTaxOwnerError>,
    },
    #[error("company correction failed for reason '{reason}': closing: {cause}")]
    Closing {
        reason: String,
        #[source]
        cause: Box<ClosingError>,
    },
    #[error("company correction failed for reason '{reason}': publication: {cause}")]
    Information {
        reason: String,
        #[source]
        cause: Box<InformationError>,
    },
    #[error("company correction rejected for reason '{reason}': {detail}")]
    InvalidInput { reason: String, detail: String },
}

pub(crate) struct CompanyCorrectionInput<'a> {
    pub(crate) books: &'a Books,
    pub(crate) position: &'a IncomeTaxPosition,
    pub(crate) policy: &'a IncomeTaxPolicy,
    pub(crate) next_event_id: u64,
    pub(crate) closing: &'a ClosingEngine,
    pub(crate) library: &'a PublicLibrary,
    pub(crate) member: &'a MemberId,
    pub(crate) industry: IndustryPresentation,
    pub(crate) correction: CorrectionRequest,
    pub(crate) publication: PublicationRequest,
    pub(crate) posted_on: CivilDate,
}

pub(crate) struct CorrectionScratch {
    pub(crate) books: Books,
    pub(crate) position: IncomeTaxPosition,
    pub(crate) next_event_id: u64,
    pub(crate) closing: ClosingEngine,
    pub(crate) library: PublicLibrary,
    pub(crate) report: ReportHandle,
    pub(crate) publication: PublicationId,
    pub(crate) publications: Vec<PublicationId>,
}

pub(crate) fn prepare_correction(
    input: CompanyCorrectionInput<'_>,
) -> Result<CorrectionScratch, CompanyCorrectionError> {
    compute_correction(input)
}

fn compute_correction(
    input: CompanyCorrectionInput<'_>,
) -> Result<CorrectionScratch, CompanyCorrectionError> {
    use crate::accounting::consolidation::ScopeId;
    use crate::accounting::reports::{generate_report_set, ReportRequest, ReportSource};
    use crate::company::income_tax::preview_tax_cascade;
    use crate::information::PublicationOrigin;
    use std::collections::BTreeMap;

    let reason = input.correction.reason;
    let invalid = |detail: &str| CompanyCorrectionError::InvalidInput {
        reason: reason.clone(),
        detail: detail.into(),
    };
    let accounting = |cause| CompanyCorrectionError::Accounting {
        reason: reason.clone(),
        cause: Box::new(cause),
    };
    let tax_error = |cause| CompanyCorrectionError::TaxState {
        reason: reason.clone(),
        cause: Box::new(cause),
    };
    let information = |cause| CompanyCorrectionError::Information {
        reason: reason.clone(),
        cause: Box::new(cause),
    };
    let scope = ScopeId::Standalone(input.member.clone());
    if input.publication.scope != scope || input.publication.company.0 != input.member.0 {
        return Err(invalid("更正公司与真实 Standalone owner 不一致"));
    }
    let worksheet = input
        .closing
        .restatement_periods(&scope)
        .cloned()
        .unwrap_or_default();
    if input.position.restatements() != &worksheet {
        return Err(tax_error(IncomeTaxOwnerError::StateInconsistent {
            detail: "税务与结账重述身份不一致，必须使用同一组合更正入口".into(),
        }));
    }
    input.position.validate(input.policy).map_err(&tax_error)?;
    input
        .position
        .validate_books(input.books)
        .map_err(&tax_error)?;
    let mut corrections = input.correction.entries;
    if input.posted_on > input.publication.approved_at.date()
        || corrections.iter().any(|entry| entry.date > input.posted_on)
    {
        return Err(invalid(
            "更正凭证实际日期不得晚于税务过账日，过账日不得晚于批准披露日",
        ));
    }
    if input
        .publication
        .supersedes
        .and_then(|id| input.library.publication_instant(id))
        .is_some_and(|instant| instant > input.publication.published_at)
    {
        return Err(invalid("更正公布时点不得早于被更正报告"));
    }
    let mut books = input.books.clone();
    books.post_batch(corrections.clone()).map_err(&accounting)?;
    let mut next_event_id = input.next_event_id;
    let mut effective = BTreeMap::new();
    let mut prospective = worksheet;
    for entry in &corrections {
        effective.insert(entry.source, input.publication.period);
        prospective.insert(entry.source, input.publication.period);
        next_event_id = next_event_id.max(
            entry
                .source
                .value()
                .checked_add(1)
                .ok_or_else(|| invalid("更正凭证来源身份空间耗尽"))?,
        );
    }
    let preview = preview_tax_cascade(
        &books,
        input.position,
        input.policy,
        next_event_id,
        input.publication.period.year(),
        input.posted_on,
        &prospective,
    )
    .map_err(&tax_error)?;
    for entry in &preview.entries {
        let period = preview
            .position
            .restatements()
            .get(&entry.source)
            .copied()
            .unwrap_or_else(|| entry.period());
        effective.insert(entry.source, period);
    }
    corrections.extend(preview.entries);
    books = input.books.clone();
    let mut closing = input.closing.clone();
    let mut library = input.library.clone();
    let target_key = (input.publication.period, input.publication.kind);
    let approved_at = input.publication.approved_at;
    let published_at = input.publication.published_at;
    let (report, publication) = library
        .correct_and_publish_with_periods(
            &mut closing,
            &mut books,
            input.member,
            input.industry,
            CorrectionRequest {
                entries: corrections,
                reason: reason.clone(),
            },
            input.publication,
            &effective,
        )
        .map_err(convert_publication_error)?;
    let mut publications = vec![publication];
    let mut latest_reports = BTreeMap::new();
    for published in library.save().reports {
        let key = (published.reports.period, published.reports.kind);
        if published.reports.scope == scope
            && key != target_key
            && published.reports.period >= target_key.0
        {
            latest_reports.insert(key, published);
        }
    }
    for ((period, kind), original) in &latest_reports {
        let candidate = generate_report_set(ReportRequest {
            period: *period,
            kind: *kind,
            source: ReportSource::Standalone {
                id: input.member.clone(),
                books: &books,
                industry: input.industry,
            },
            version: original.reports.version.clone(),
            adjustments: preview.position.restatements(),
        })
        .map_err(|cause| CompanyCorrectionError::Closing {
            reason: reason.clone(),
            cause: Box::new(ClosingError::Report(Box::new(cause))),
        })?;
        if candidate == original.reports {
            continue;
        }
        if original.published_at > published_at {
            return Err(invalid("后续报告的更正公布时点不得早于原公开报告"));
        }
        let sequence = closing
            .versions(&scope, *period, *kind)
            .last()
            .ok_or_else(|| invalid("后续公开报告缺少结账版本"))?
            .version
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("后续更正报告版本序号耗尽"))?;
        let (_, publication) = library
            .correct_and_publish_with_periods(
                &mut closing,
                &mut books,
                input.member,
                input.industry,
                CorrectionRequest {
                    entries: Vec::new(),
                    reason: reason.clone(),
                },
                PublicationRequest {
                    company: original.company.clone(),
                    scope: scope.clone(),
                    period: *period,
                    kind: *kind,
                    sequence,
                    policy: original.policy.clone(),
                    approved_at,
                    published_at,
                    origin: PublicationOrigin::Correction,
                    supersedes: Some(original.id),
                },
                &BTreeMap::new(),
            )
            .map_err(convert_publication_error)?;
        publications.push(publication);
    }
    for private in closing.latest_reports_for_scope(&scope) {
        let key = (private.period, private.kind);
        if key != target_key && private.period >= target_key.0 && !latest_reports.contains_key(&key)
        {
            crate::information::ensure_original_registered(
                &mut closing,
                &books,
                input.member,
                input.industry,
                private.period,
                private.kind,
            )
            .map_err(&information)?;
        }
    }
    preview
        .position
        .validate(input.policy)
        .map_err(&tax_error)?;
    preview
        .position
        .validate_books(&books)
        .map_err(&tax_error)?;
    if closing.restatement_periods(&scope) != Some(preview.position.restatements()) {
        return Err(tax_error(IncomeTaxOwnerError::StateInconsistent {
            detail: "候选税务与结账重述身份不一致".into(),
        }));
    }
    Ok(CorrectionScratch {
        books,
        position: preview.position,
        next_event_id: preview.next_event_id,
        closing,
        library,
        report,
        publication,
        publications,
    })
}

fn convert_publication_error(
    cause: crate::information::CorrectionPublicationError,
) -> CompanyCorrectionError {
    use crate::information::CorrectionPublicationError;
    match cause {
        CorrectionPublicationError::Accounting { reason, cause } => {
            CompanyCorrectionError::Accounting { reason, cause }
        }
        CorrectionPublicationError::Closing { reason, cause } => {
            CompanyCorrectionError::Closing { reason, cause }
        }
        CorrectionPublicationError::Information { reason, cause } => {
            CompanyCorrectionError::Information { reason, cause }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::consolidation::ScopeId;
    use crate::accounting::reports::ReportKind;
    use crate::accounting::{
        AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
        LedgerAccountId, PostingSide,
    };
    use crate::calendar::CivilInstant;
    use crate::company::CompanyId;
    use crate::information::{
        scheduled_instant, AccountingPolicyRef, PublicationOrigin, ScheduledReportKind,
    };

    fn date(value: &str) -> CivilDate {
        CivilDate::from_iso(value).unwrap()
    }
    fn line(account: &str, side: PostingSide, amount: i128) -> JournalLine {
        JournalLine {
            account: LedgerAccountId(account.into()),
            side,
            amount: AccountingAmount::from_cents(amount),
        }
    }

    #[test]
    fn shared_correction_rejects_tax_and_closing_identity_drift_with_typed_source() {
        use std::error::Error;
        let mut books = Books::new(crate::company::industrial::industrial_account_chart());
        let position = IncomeTaxPosition::new(&books).unwrap();
        let member = MemberId("IDENTITY".into());
        let mut closing = ClosingEngine::new();
        let mut library = PublicLibrary::new();
        closing
            .close_year(&mut books, &member, IndustryPresentation::Industrial, 2030)
            .unwrap();
        let period = crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap();
        let scope = ScopeId::Standalone(member.clone());
        let initial_at = scheduled_instant(ScheduledReportKind::Annual, 2030, 0).unwrap();
        let original = library
            .publish_closed(
                &closing,
                PublicationRequest {
                    company: CompanyId(member.0.clone()),
                    scope: scope.clone(),
                    period,
                    kind: ReportKind::Annual,
                    sequence: 1,
                    policy: AccountingPolicyRef { chart_version: 2 },
                    approved_at: CivilInstant::from_hms(initial_at.date(), 8, 0, 0).unwrap(),
                    published_at: initial_at,
                    origin: PublicationOrigin::ScheduledDisclosure {
                        fiscal_year: 2030,
                        kind: ScheduledReportKind::Annual,
                        offset_days: 0,
                    },
                    supersedes: None,
                },
            )
            .unwrap();
        let posted_on = date("2031-05-01");
        library
            .correct_and_publish(
                &mut closing,
                &mut books,
                &member,
                IndustryPresentation::Industrial,
                CorrectionRequest {
                    reason: "低层入口刻意制造不同身份".into(),
                    entries: vec![JournalEntry {
                        source: BusinessEventId::new(2),
                        date: posted_on,
                        kind: BusinessKind::CashRevenue,
                        cash_flow: CashFlowClass::Operating,
                        lines: vec![
                            line("1002", PostingSide::Debit, 400),
                            line("6001", PostingSide::Credit, 400),
                        ],
                    }],
                },
                PublicationRequest {
                    company: CompanyId(member.0.clone()),
                    scope: scope.clone(),
                    period,
                    kind: ReportKind::Annual,
                    sequence: 2,
                    policy: AccountingPolicyRef { chart_version: 2 },
                    approved_at: CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
                    published_at: CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap(),
                    origin: PublicationOrigin::Correction,
                    supersedes: Some(original),
                },
            )
            .unwrap();
        let before_books = books.clone();
        let before_closing = serde_json::to_value(&closing).unwrap();
        let before_library = library.clone();
        let result = prepare_correction(CompanyCorrectionInput {
            books: &books,
            position: &position,
            policy: &IncomeTaxPolicy {
                rate_bp: 2500,
                loss_carryforward_years: 5,
            },
            next_event_id: 3,
            closing: &closing,
            library: &library,
            member: &member,
            industry: IndustryPresentation::Industrial,
            correction: CorrectionRequest {
                reason: "保持 TaxState 因果".into(),
                entries: Vec::new(),
            },
            publication: PublicationRequest {
                company: CompanyId(member.0.clone()),
                scope,
                period,
                kind: ReportKind::Annual,
                sequence: 3,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
                published_at: CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap(),
                origin: PublicationOrigin::Correction,
                supersedes: Some(PublicationId::new(1)),
            },
            posted_on,
        });
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("不一致更正身份必须拒绝"),
        };
        assert!(
            matches!(&error, CompanyCorrectionError::TaxState { reason, cause } if reason == "保持 TaxState 因果" && matches!(cause.as_ref(), IncomeTaxOwnerError::StateInconsistent { .. }))
        );
        assert!(error.source().is_some());
        assert_eq!(books, before_books);
        assert_eq!(serde_json::to_value(&closing).unwrap(), before_closing);
        assert_eq!(library, before_library);
    }

    #[test]
    fn shared_correction_restates_all_four_presentations_without_mutating_inputs() {
        run_four_kind_scenario(Scenario::Success);
    }

    #[derive(Copy, Clone, Eq, PartialEq)]
    enum Scenario {
        Success,
        PrivateAnnual,
        FuturePublication,
        ExhaustedSource,
        DuplicateSource,
    }

    #[test]
    fn shared_correction_appends_private_annual_versions_without_rewriting_originals() {
        run_four_kind_scenario(Scenario::PrivateAnnual);
    }

    #[test]
    fn shared_correction_rolls_back_when_later_publication_would_be_backdated() {
        run_four_kind_scenario(Scenario::FuturePublication);
    }

    #[test]
    fn shared_correction_rejects_exhausted_and_duplicate_sources_without_partial_state() {
        run_four_kind_scenario(Scenario::ExhaustedSource);
        run_four_kind_scenario(Scenario::DuplicateSource);
    }

    fn run_four_kind_scenario(scenario: Scenario) {
        for (chart, industry, cash, revenue) in [
            (
                crate::company::industrial::industrial_account_chart(),
                IndustryPresentation::Industrial,
                "1002",
                "6001",
            ),
            (
                crate::company::bank::bank_account_chart(),
                IndustryPresentation::Bank,
                "1003",
                "6021",
            ),
            (
                crate::company::insurance::insurance_account_chart(),
                IndustryPresentation::Insurance,
                "1002",
                "6051",
            ),
            (
                crate::company::real_estate::real_estate_account_chart(),
                IndustryPresentation::RealEstate,
                "1002",
                "6001",
            ),
        ] {
            let mut books = Books::new(chart);
            books
                .post_batch(vec![JournalEntry {
                    source: BusinessEventId::new(1),
                    date: date("2029-12-31"),
                    kind: BusinessKind::OpeningBalance,
                    cash_flow: CashFlowClass::Financing,
                    lines: vec![
                        line(cash, PostingSide::Debit, 10000),
                        line("4001", PostingSide::Credit, 10000),
                    ],
                }])
                .unwrap();
            let position = IncomeTaxPosition::new(&books).unwrap();
            let policy = IncomeTaxPolicy {
                rate_bp: 2500,
                loss_carryforward_years: 5,
            };
            let member = MemberId("FOUR-KIND".into());
            let mut closing = ClosingEngine::new();
            closing
                .close_year(&mut books, &member, industry, 2030)
                .unwrap();
            let mut library = PublicLibrary::new();
            let initial_at = scheduled_instant(ScheduledReportKind::Annual, 2030, 0).unwrap();
            let period = crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap();
            let scope = ScopeId::Standalone(member.clone());
            let policy_ref = AccountingPolicyRef {
                chart_version: books.ledger().chart().version(),
            };
            let original = library
                .publish_closed(
                    &closing,
                    PublicationRequest {
                        company: CompanyId(member.0.clone()),
                        scope: scope.clone(),
                        period,
                        kind: ReportKind::Annual,
                        sequence: 1,
                        policy: policy_ref.clone(),
                        approved_at: CivilInstant::from_hms(initial_at.date(), 8, 0, 0).unwrap(),
                        published_at: initial_at,
                        origin: PublicationOrigin::ScheduledDisclosure {
                            fiscal_year: 2030,
                            kind: ScheduledReportKind::Annual,
                            offset_days: 0,
                        },
                        supersedes: None,
                    },
                )
                .unwrap();
            let next_period = crate::accounting::AccountingPeriod::from_ymd(2031, 12).unwrap();
            if matches!(
                scenario,
                Scenario::PrivateAnnual | Scenario::FuturePublication
            ) {
                closing
                    .close_year(&mut books, &member, industry, 2031)
                    .unwrap();
            }
            if scenario == Scenario::FuturePublication {
                let next_at = scheduled_instant(ScheduledReportKind::Annual, 2031, 0).unwrap();
                library
                    .publish_closed(
                        &closing,
                        PublicationRequest {
                            company: CompanyId(member.0.clone()),
                            scope: scope.clone(),
                            period: next_period,
                            kind: ReportKind::Annual,
                            sequence: 1,
                            policy: policy_ref.clone(),
                            approved_at: CivilInstant::from_hms(next_at.date(), 8, 0, 0).unwrap(),
                            published_at: next_at,
                            origin: PublicationOrigin::ScheduledDisclosure {
                                fiscal_year: 2031,
                                kind: ScheduledReportKind::Annual,
                                offset_days: 0,
                            },
                            supersedes: None,
                        },
                    )
                    .unwrap();
            }
            let before_private = closing
                .version(&scope, next_period, ReportKind::Annual, 1)
                .cloned();
            let before_books = books.clone();
            let before_closing = serde_json::to_value(&closing).unwrap();
            let before_library = library.clone();
            let posted_on = match scenario {
                Scenario::PrivateAnnual => date("2032-05-01"),
                Scenario::FuturePublication => date("2032-03-01"),
                _ => date("2031-05-01"),
            };
            let result = prepare_correction(CompanyCorrectionInput {
                books: &books,
                position: &position,
                policy: &policy,
                next_event_id: 2,
                closing: &closing,
                library: &library,
                member: &member,
                industry,
                correction: CorrectionRequest {
                    reason: "补记前期服务收入".into(),
                    entries: vec![JournalEntry {
                        source: BusinessEventId::new(match scenario {
                            Scenario::ExhaustedSource => u64::MAX,
                            Scenario::DuplicateSource => 1,
                            _ => 2,
                        }),
                        date: posted_on,
                        kind: BusinessKind::CashRevenue,
                        cash_flow: CashFlowClass::Operating,
                        lines: vec![
                            line(cash, PostingSide::Debit, 400),
                            line(revenue, PostingSide::Credit, 400),
                        ],
                    }],
                },
                publication: PublicationRequest {
                    company: CompanyId(member.0.clone()),
                    scope: scope.clone(),
                    period,
                    kind: ReportKind::Annual,
                    sequence: 2,
                    policy: policy_ref,
                    approved_at: CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
                    published_at: CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap(),
                    origin: PublicationOrigin::Correction,
                    supersedes: Some(original),
                },
                posted_on,
            });
            assert_eq!(books, before_books);
            assert_eq!(serde_json::to_value(&closing).unwrap(), before_closing);
            assert_eq!(library, before_library);
            if matches!(
                scenario,
                Scenario::FuturePublication | Scenario::ExhaustedSource | Scenario::DuplicateSource
            ) {
                let error = match result {
                    Err(error) => error,
                    Ok(_) => panic!("非法更正边界必须拒绝"),
                };
                match scenario {
                    Scenario::FuturePublication => assert!(
                        matches!(error, CompanyCorrectionError::InvalidInput { detail, .. } if detail.contains("后续报告"))
                    ),
                    Scenario::ExhaustedSource => assert!(
                        matches!(error, CompanyCorrectionError::InvalidInput { detail, .. } if detail.contains("身份空间"))
                    ),
                    Scenario::DuplicateSource => {
                        assert!(matches!(error, CompanyCorrectionError::Accounting { .. }))
                    }
                    _ => unreachable!("此分支仅验证拒绝场景"),
                }
                continue;
            }
            let result = result.unwrap();
            if scenario == Scenario::PrivateAnnual {
                assert_eq!(
                    result
                        .closing
                        .version(&scope, next_period, ReportKind::Annual, 1),
                    before_private.as_ref()
                );
                let revised = result
                    .closing
                    .version(&scope, next_period, ReportKind::Annual, 2)
                    .expect("受影响的未公开年报追加实际序号");
                assert_eq!(
                    revised.version.kind,
                    crate::accounting::reports::VersionKind::Original
                );
                assert_eq!(revised.version.supersedes, None);
                assert_ne!(Some(revised), before_private.as_ref());
                assert_eq!(result.library.save().reports.len(), 2);
            }
            assert_eq!(
                result
                    .books
                    .ledger()
                    .account_net_debit(&LedgerAccountId(cash.into()))
                    .unwrap(),
                AccountingAmount::from_cents(10400)
            );
            assert_eq!(
                result
                    .books
                    .ledger()
                    .account_net_debit(&LedgerAccountId("222104".into()))
                    .unwrap(),
                AccountingAmount::from_cents(-100)
            );
            assert_eq!(result.next_event_id, 4);
            result.position.validate_books(&result.books).unwrap();
            let reports = result.library.save().reports;
            assert_eq!(reports[0], before_library.save().reports[0]);
            assert_eq!(reports[1].supersedes, Some(original));
            assert_eq!(
                reports[1].reports.income.cumulative.net_income,
                AccountingAmount::from_cents(300)
            );
            assert_eq!(result.publication, reports[1].id);
            assert_eq!(result.report.sequence, 2);
        }
    }
}
