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

#[test]
fn missing_watermark_and_omitted_confirmations_reject_without_mutating_inputs() {
    let (registry,book,day,calendar)=fixture();
    let before=(registry.clone(),book.clone());
    assert!(TaxDayBoundaryFacts::prepare(&registry,&book,&day,&context(&calendar,&[],None)).is_err());
    let rows=vec![ShareMarketConfirmation {account:AccountId(1),receipt_id:70,stock:registry.stock().clone(),day:day.day,side:crate::orderbook::Side::Sell,qty:10}];
    assert!(TaxDayBoundaryFacts::prepare(&registry,&book,&day,&context(&calendar,&rows,Some(day.day))).is_err());
    assert_eq!((registry,book),before);
}
#[test]
fn complete_lot_attribution_rejects_90_physical_100_tax_and_equal_total_spoofs() {
    let (registry,book,day,calendar)=fixture();
    for variant in 0..5 {
        let mut changed=book.clone();
        let mut lot=changed.lots()[0].clone();
        match variant {
            0=>lot.qty=110,
            1=>lot.id="tax:impostor".into(),
            2=>lot.acquired_on=date("2024-01-03"),
            3=>lot.source=TaxAcquisitionSource::InitialAllocation {evidence:"无受控对应来源".into()},
            4=>{ changed=CashDividendTaxBook::new(AccountId(2),registry.stock().clone(),DividendTaxProfile::IndividualPublicMarket,registry.settled_on(),changed.lots().to_vec()).unwrap(); },
            _=>unreachable!(),
        }
        if variant!=4 { changed=CashDividendTaxBook::new(changed.account(),changed.stock().clone(),DividendTaxProfile::IndividualPublicMarket,changed.settled_on(),vec![lot]).unwrap(); }
        let failure=TaxDayBoundaryFacts::prepare(&registry,&changed,&day,&context(&calendar,&[],Some(day.day))).expect_err("完整 lot 对应不能由等总量、前缀或比例猜出");
        assert!(matches!(failure,TaxBoundaryError::MissingAttribution {..}),"{failure:?}");
    }
    let physical90=ShareRegistry::new(registry.stock().clone(),crate::company::CompanyId(0),90,registry.settled_on(),vec![ShareHolding {holder:HolderId::Account(AccountId(1)),lots:vec![ShareLot {qty:90,..registry.holdings()[0].lots[0].clone()}]}]).unwrap();
    let mut day90=day.clone(); day90.predecessor=physical90.settlement_frontier().unwrap();
    assert!(matches!(TaxDayBoundaryFacts::prepare(&physical90,&book,&day90,&context(&calendar,&[],Some(day.day))),Err(TaxBoundaryError::MissingAttribution {..})));
}
#[test]
fn calendar_closed_and_absent_holder_change_still_record_exactly_one_zero_tax_day() {
    let (registry,book,mut day,calendar)=fixture();
    day.day=date("2026-01-10");
    let registry=ShareRegistry::new(registry.stock().clone(),crate::company::CompanyId(0),100,date("2026-01-09"),registry.holdings().to_vec()).unwrap();
    let book=CashDividendTaxBook::new(book.account(),book.stock().clone(),DividendTaxProfile::IndividualPublicMarket,registry.settled_on(),book.lots().to_vec()).unwrap();
    day.predecessor=registry.settlement_frontier().unwrap();
    day.actual_market_close.request.day=day.day;
    day.actual_market_close.source=ShareMarketSource::CalendarClosed {exchange:CalendarExchange::Sse,policy_digest:calendar.policy().content_digest().into()};
    let result=TaxDayBoundaryFacts::prepare(&registry,&book,&day,&context(&calendar,&[],Some(day.day))).unwrap();
    assert_eq!(result.book().days.len(),1); assert_eq!(result.book().days[0].net_change,0);
    assert_eq!(result.book().settled_on(),day.day); assert_eq!(result.registrations().len(),1);
}
#[test]
fn any_current_or_historical_nonmarket_movement_is_explicitly_unsupported() {
    let (registry,book,mut day,calendar)=fixture();
    day.post_market_actions.push(SharePostAction::Movement(ShareDayRequest {event_id:"transfer".into(),day:day.day,scope:MovementScope::NonTradingTransfer {basis:"真实过户另需税来源".into()},changes:vec![]}));
    assert!(TaxDayBoundaryFacts::prepare(&registry,&book,&day,&context(&calendar,&[],Some(day.day))).is_err());
    let mut historical=registry.clone();
    historical.close_day(ShareDayRequest {event_id:"historical-transfer".into(),day:day.day,scope:MovementScope::NonTradingTransfer {basis:"不能忽略历史".into()},changes:vec![]}).unwrap();
    let book=CashDividendTaxBook::new(book.account(),book.stock().clone(),DividendTaxProfile::IndividualPublicMarket,historical.settled_on(),book.lots().to_vec()).unwrap();
    let mut later=day.clone(); later.day=date("2026-01-07");later.predecessor=historical.settlement_frontier().unwrap();later.actual_market_close.request.day=later.day;later.actual_market_close.request.event_id="later-market".into();later.post_market_actions.clear();
    assert!(TaxDayBoundaryFacts::prepare(&historical,&book,&later,&context(&calendar,&[],Some(later.day))).is_err());
}
