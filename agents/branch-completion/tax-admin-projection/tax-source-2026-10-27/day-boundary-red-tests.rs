use super::*;
use crate::company::share_registry::*;
use crate::company::share_registry::settlement_day::*;
use crate::calendar::{CalendarExchange, TradingCalendar};
fn date(s: &str) -> CivilDate { CivilDate::from_iso(s).unwrap() }
fn fixture() -> (ShareRegistry, CashDividendTaxBook, ShareSettlementDay, TradingCalendar) {
    let registry = ShareRegistry::new(StockCode("600001".into()), crate::company::CompanyId(0), 100,
        date("2026-01-05"), vec![ShareHolding { holder: HolderId::Account(AccountId(1)), lots: vec![ShareLot {
        id: "initial".into(), qty: 100, acquired_on: date("2024-01-02"),
        source: AcquisitionSource::InitialAllocation { evidence: "真实装配期来源".into() }, restriction: ShareRestriction::Unrestricted }] }]).unwrap();
    let book = CashDividendTaxBook::new(AccountId(1), registry.stock().clone(), DividendTaxProfile::IndividualPublicMarket, registry.settled_on(), vec![DividendTaxLot {
        id: "tax:initial".into(), qty: 100, acquired_on: date("2024-01-02"), source: TaxAcquisitionSource::InitialAllocation { evidence: "真实装配期来源".into() }, class: TaxShareClass::PublicMarket }]).unwrap();
    let day = ShareSettlementDay { stock: registry.stock().clone(), day: date("2026-01-06"), predecessor: registry.settlement_frontier().unwrap(), opening: vec![], actual_market_close: ConfirmedShareMarketClose {
        unit: ShareUnitIdentity::Initial, source: ShareMarketSource::FinalConfirmations { receipts: vec![] }, request: ShareDayRequest { event_id: "market:2026-01-06".into(), day: date("2026-01-06"), scope: MovementScope::PublicMarket, changes: vec![] } },
        post_market_actions: vec![SharePostAction::CaptureRegistration { event_id: "actual-plan".into() }] };
    (registry,book,day,TradingCalendar::current_default_calendar().unwrap())
}
fn context<'a>(calendar: &'a TradingCalendar, rows: &'a [ShareMarketConfirmation], through: Option<CivilDate>) -> ShareMarketSourceContext<'a> {
    ShareMarketSourceContext { calendar,exchange: CalendarExchange::Sse,confirmations:rows,treasury_account:None,completed_through:through }
}
#[test]
fn actual_reg_prepare_zero_market_day_advances_tax_and_binds_exact_plan_capture() {
    let (registry,book,day,calendar)=fixture();
    let result=TaxDayBoundaryFacts::prepare(&registry,&book,&day,&context(&calendar,&[],Some(day.day))).expect("已完成普通零日也必须推进税日");
    assert_eq!(result.book().settled_on(),day.day);
    assert_eq!(result.book().lots(),book.lots());
    assert_eq!(result.book().days.len(),1);
    assert_eq!(result.book().days[0].net_change,0);
    assert!(result.book().days[0].acquisition.is_none());
    let capture=&result.registrations()[0];
    assert_eq!(capture.event_id,"actual-plan");
    assert_eq!(capture.settled_receipts,result.registry().receipts().len() as u64);
    assert_eq!(capture.lots,book.lots());
    assert_eq!(capture.after_book_operation,result.book().operation_seq);
    assert_eq!(book.settled_on(),date("2026-01-05"));
}
