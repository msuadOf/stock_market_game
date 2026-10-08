use super::fixtures::*;
use engine::company::CompanyId;
use engine::company::operations::{CompanyOperations, CompanyOperationsConfig, FlowParams};

#[test]
fn newly_recognized_insurance_claim_is_payable_not_contractual_overdue() {
    use engine::company::events::PaymentObligationStatus;
    use engine::company::insurance::InsuranceProductKind;
    use engine::company::operations::IndustryBooks;
    use engine::company::{ContractId, CounterpartyId};
    let date = d("2030-01-01");
    let mut company = insurance_c(date.prev().unwrap());
    let IndustryBooks::Insurance(books) = &mut company.books else {
        unreachable!()
    };
    books
        .establish_group(
            InsuranceProductKind::TermProtection,
            ContractId("claim-status".into()),
            &CounterpartyId("EXT-POL".into()),
            yuan(60),
            yuan(50),
            yuan(3),
            date.prev().unwrap(),
            d("2030-01-31"),
        )
        .unwrap();
    let FlowParams::Insurance(params) = &mut company.flow else {
        unreachable!()
    };
    params.daily_groups_base = 0;
    params.claim_every_days = 1;
    params.claim_size = yuan(30_000);
    let mut operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 17,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        date,
    )
    .unwrap();
    let report = operations.advance_civil_day(date).unwrap();
    assert_eq!(report.payment_failures.len(), 1);
    assert_eq!(
        report.payment_failures[0].obligation_status,
        PaymentObligationStatus::ContractualPayable
    );
    assert_eq!(report.payment_failures[0].amount, yuan(30_000));
    let restored: CompanyOperations =
        serde_json::from_value(serde_json::to_value(&operations).unwrap()).unwrap();
    assert_eq!(restored.payment_failures_on(date), report.payment_failures);
}

#[test]
fn failed_payments_are_authoritative_and_survive_restore() {
    let date = d("2030-01-01");
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![industrial_broke(date.prev().unwrap())],
        },
        date,
    )
    .unwrap();
    let report = ops.advance_civil_day(date).unwrap();
    assert!(!report.payment_failures.is_empty());
    assert_eq!(ops.payment_failures_on(date), report.payment_failures);
    let restored: CompanyOperations =
        serde_json::from_slice(&serde_json::to_vec(&ops).unwrap()).unwrap();
    assert_eq!(restored.payment_failures_on(date), report.payment_failures);
}

#[test]
fn failed_operating_payment_is_published_once_at_day_end() {
    use engine::calendar::CalendarExchange;
    use engine::session::{CivilClock, DayEndDisclosureCtx, DisclosureDispatch};
    let date = d("2030-01-05");
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![industrial_broke(date.prev().unwrap())],
        },
        date,
    )
    .unwrap();
    let business = ops.advance_civil_day(date).unwrap();
    let mut clock = CivilClock::new(date, CalendarExchange::Sse).unwrap();
    let report = clock.end_day(date).unwrap();
    let mut closing = engine::accounting::closing::ClosingEngine::new();
    let mut library = engine::information::PublicLibrary::new();
    let mut dispatch = DisclosureDispatch::new(Some(report.disclosure_instant));
    let outcome = dispatch
        .run_day_end(DayEndDisclosureCtx {
            report_frequency: engine::information::ReportFrequency::Quarterly,
            groups: &[],
            report: &report,
            ops: &ops,
            closing: &mut closing,
            library: &mut library,
        })
        .unwrap();
    assert_eq!(
        outcome.announcements_published.len(),
        business.payment_failures.len()
    );
    let saved = library.save();
    for (announcement, failure) in saved.announcements.iter().zip(&business.payment_failures) {
        assert_eq!(announcement.company, failure.company);
        assert_eq!(announcement.occurred_on, date);
        assert_eq!(announcement.published_at, report.disclosure_instant);
        assert_eq!(
            match &announcement.content {
                engine::information::AnnouncementContent::Shock(event) => event.kind.clone(),
                engine::information::AnnouncementContent::CashDividend(_)
                | engine::information::AnnouncementContent::RightsOffering(_)
                | engine::information::AnnouncementContent::IssuerRepurchase(_)
                | engine::information::AnnouncementContent::ShareSplit(_)
                | engine::information::AnnouncementContent::StockDistribution(_) =>
                    panic!("payment failure must be a shock announcement"),
            },
            engine::company::ShockKind::PaymentFailure {
                obligation_status: failure.obligation_status,
                what: failure.what.clone(),
                amount: failure.amount
            }
        );
    }
    let restored = engine::information::PublicLibrary::from_parts(saved).unwrap();
    assert_eq!(restored.save(), library.save());
    let repeated = dispatch
        .run_day_end(DayEndDisclosureCtx {
            report_frequency: engine::information::ReportFrequency::Quarterly,
            groups: &[],
            report: &report,
            ops: &ops,
            closing: &mut closing,
            library: &mut library,
        })
        .unwrap();
    assert!(repeated.announcements_published.is_empty());
}

#[test]
fn insolvent_bank_keeps_due_deposit_and_retries_without_rescue() {
    use engine::company::bank::BankProductKind;
    use engine::company::{ContractId, CounterpartyId};
    let date = d("2030-01-01");
    let mut company = bank_c(date.prev().unwrap());
    if let engine::company::operations::IndustryBooks::Bank(books) = &mut company.books {
        books
            .issue_loan(
                BankProductKind::TermLoan,
                ContractId("DRAIN".into()),
                &CounterpartyId("EXT-BOR".into()),
                amt(4_999_900),
                0,
                date.prev().unwrap(),
                d("2031-01-01"),
            )
            .unwrap();
    }
    if let FlowParams::Bank(params) = &mut company.flow {
        params.deposit_term_days = 1;
        params.deposit_every_days = 100;
        params.lending_every_days = 100;
    }
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![company],
        },
        date,
    )
    .unwrap();
    ops.advance_civil_day(date).unwrap();
    for day in [date.next().unwrap(), date.next().unwrap().next().unwrap()] {
        let report = ops.advance_civil_day(day).unwrap();
        assert!(
            report
                .payment_failures
                .iter()
                .any(|failure| failure.what == "overdue deposit principal:DEP-1"
                    && failure.amount == yuan(1_000))
        );
        let books = ops.bank_books(&CompanyId("C-BANK".into())).unwrap();
        assert_eq!(
            books
                .deposit(&ContractId("DEP-1".into()))
                .unwrap()
                .principal(),
            yuan(1_000)
        );
        assert!(!books.books().ledger().cash_total().unwrap().is_negative());
    }
}

#[test]
fn payment_history_rejects_invalid_amount_or_unknown_company() {
    let date = d("2030-01-01");
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: quiet_params(),
            companies: vec![industrial_broke(date.prev().unwrap())],
        },
        date,
    )
    .unwrap();
    ops.advance_civil_day(date).unwrap();
    ops.validate_payment_history().unwrap();
    for (field, invalid) in [("amount", "0"), ("company", "UNKNOWN"), ("what", "")] {
        let mut value = serde_json::to_value(&ops).unwrap();
        value["payment_failures"][date.to_iso()][0][field] =
            serde_json::Value::String(invalid.into());
        let restored: CompanyOperations = serde_json::from_value(value).unwrap();
        assert!(restored.validate_payment_history().is_err());
    }
    let mut future = serde_json::to_value(&ops).unwrap();
    let failures = future["payment_failures"]
        .as_object_mut()
        .unwrap()
        .remove(&date.to_iso())
        .unwrap();
    future["payment_failures"][date.next().unwrap().to_iso()] = failures;
    let restored: CompanyOperations = serde_json::from_value(future).unwrap();
    assert!(restored.validate_payment_history().is_err());
}

#[test]
fn risk_announcement_restore_rejects_nonpositive_failed_amount() {
    use engine::information::{AnnouncedEvent, AnnouncementRequest, PublicLibrary};
    let date = d("2030-01-01");
    let mut library = PublicLibrary::new();
    library
        .publish_announcement(AnnouncementRequest {
            company: CompanyId("C-IND-A".into()),
            occurred_on: date,
            published_at: engine::calendar::CivilInstant::from_hms(date, 18, 0, 0).unwrap(),
            content: engine::information::AnnouncementContent::Shock(AnnouncedEvent {
                kind: engine::company::ShockKind::PaymentFailure {
                    obligation_status:
                        engine::company::events::PaymentObligationStatus::ContractualOverdue,
                    what: "overdue principal".into(),
                    amount: amt(100),
                },
                amplitude_bp: 0,
                starts_on: date,
                expires_on: date,
            }),
        })
        .unwrap();
    let mut saved = library.save();
    let engine::information::AnnouncementContent::Shock(event) =
        &mut saved.announcements[0].content
    else {
        panic!("payment failure must be a shock announcement");
    };
    event.kind = engine::company::ShockKind::PaymentFailure {
        obligation_status: engine::company::events::PaymentObligationStatus::ContractualOverdue,
        what: "overdue principal".into(),
        amount: amt(0),
    };
    assert!(PublicLibrary::from_parts(saved).is_err());
}
