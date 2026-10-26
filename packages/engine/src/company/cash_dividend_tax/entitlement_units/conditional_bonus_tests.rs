use super::tests::{cash_book,current,date,fraction,fragment,lot,origin,q};
use super::*;
fn empty_lineage(book: &CashDividendTaxBook) -> TaxUnitLineage {
    TaxUnitLineage { account:book.account(),stock:book.stock().clone(),transitions:vec![] }
}
fn registration(event:&str, prefix:u64, day:&str, ratio:u64) -> ConditionalBonusTaxEvent {
    ConditionalBonusTaxEvent {after_book_operation:prefix,day:date(day),action:ConditionalBonusTaxAction::RegisterBonus {
        event_id:event.into(),shares_per_existing_share_micros:ratio,par_value_per_share:Money::from_cents(100),
    }}
}
fn realization(book:&CashDividendTaxBook,event:&str,prefix:u64,day:&str,qty:u64,acquisition:Option<u64>) -> ConditionalBonusTaxEvent {
    ConditionalBonusTaxEvent {after_book_operation:prefix,day:date(day),action:ConditionalBonusTaxAction::RealizeBonus {
        event_id:event.into(),actual_credit:ConditionalBonusCredit { event_id:event.into(),account:book.account(),stock:book.stock().clone(),credited_qty:qty },tax_acquisition:acquisition,
    }}
}
fn bonus_book(acquired:&str) -> CashDividendTaxBook {
    CashDividendTaxBook::new(AccountId(1),StockCode("600001".into()),DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),vec![lot("original",100,acquired)]).unwrap()
}
fn credit(book:&mut CashDividendTaxBook,qty:u64) {
    book.record_net_day("credit".into(),date("2030-01-02"),i128::from(qty),Some(DividendTaxLot {
        id:"new".into(),qty,acquired_on:date("2030-01-02"),source:TaxAcquisitionSource::CorporateAction {event:"bonus".into()},class:TaxShareClass::PublicMarket,
    })).unwrap();
}
#[test]
fn long_original_bonus_stays_exempt_when_new_short_lot_is_sold() {
    let mut book=bonus_book("2028-12-01");
    credit(&mut book,100);
    let facts=ConditionalBonusTaxFacts {events:vec![registration("bonus",0,"2030-01-01",1_000_000),realization(&book,"bonus",1,"2030-01-02",100,Some(1))]};
    book.record_net_day("sell-original".into(),date("2030-01-03"),-100,None).unwrap();
    book.record_net_day("sell-new".into(),date("2030-01-04"),-100,None).unwrap();
    let result=book.assessment_with_conditional_bonus(&empty_lineage(&book),&facts).unwrap();
    assert_eq!(result.outstanding.numerator(),0);
    assert_eq!(result.consumptions.len(),1);
    assert_eq!(result.consumptions[0].key,TaxEntitlementKey {kind:TaxIncomeKind::StockBonus,event_id:"bonus".into()});
    assert_eq!(result.consumptions[0].original,origin("original",q(0),q(100)));
}
#[test]
fn short_original_partial_disposals_assess_bonus_once_without_waiting_for_new_lot() {
    let mut book=bonus_book("2029-12-25");
    credit(&mut book,100);
    let facts=ConditionalBonusTaxFacts {events:vec![registration("bonus",0,"2030-01-01",1_000_000),realization(&book,"bonus",1,"2030-01-02",100,Some(1))]};
    book.record_net_day("sell20".into(),date("2030-01-03"),-20,None).unwrap();
    assert_eq!(book.assessment_with_conditional_bonus(&empty_lineage(&book),&facts).unwrap().outstanding.numerator(),400);
    book.record_net_day("sell80".into(),date("2030-01-04"),-80,None).unwrap();
    assert_eq!(book.assessment_with_conditional_bonus(&empty_lineage(&book),&facts).unwrap().outstanding.numerator(),2000);
    book.record_net_day("sell-new".into(),date("2030-01-05"),-100,None).unwrap();
    assert_eq!(book.assessment_with_conditional_bonus(&empty_lineage(&book),&facts).unwrap().outstanding.numerator(),2000);
}
#[test]
fn no_tax_acquisition_is_needed_to_realize_net_negative_stock_bonus_income() {
    let mut book=bonus_book("2029-12-25");
    // 给定同一真实税日：market-100、actual credit30、tax net-70。
    book.record_net_day("tax-net-minus70".into(),date("2030-01-02"),-70,None).unwrap();
    let registered=ConditionalBonusTaxFacts {events:vec![registration("bonus",0,"2030-01-01",300_000)]};
    assert_eq!(book.assessment_with_conditional_bonus(&empty_lineage(&book),&registered).unwrap().outstanding.numerator(),0);
    let mut realized=registered;
    realized.events.push(realization(&book,"bonus",1,"2030-01-02",30,None));
    let before=book.clone();
    let result=book.assessment_with_conditional_bonus(&empty_lineage(&book),&realized).unwrap();
    assert_eq!(result.outstanding.numerator(),420);
    assert_eq!(book,before);
    book.record_net_day("sell-last30".into(),date("2030-01-03"),-30,None).unwrap();
    assert_eq!(book.assessment_with_conditional_bonus(&empty_lineage(&book),&realized).unwrap().outstanding.numerator(),600);
}
#[test]
fn net_positive_thirty_tax_shares_do_not_shrink_actual_hundred_share_income() {
    let mut book=bonus_book("2029-12-25");
    credit(&mut book,30);
    let facts=ConditionalBonusTaxFacts {events:vec![registration("bonus",0,"2030-01-01",1_000_000),realization(&book,"bonus",1,"2030-01-02",100,Some(1))]};
    book.record_net_day("sell-original".into(),date("2030-01-03"),-100,None).unwrap();
    assert_eq!(book.assessment_with_conditional_bonus(&empty_lineage(&book),&facts).unwrap().outstanding.numerator(),2000);
}
