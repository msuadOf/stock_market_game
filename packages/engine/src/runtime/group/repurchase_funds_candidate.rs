//! 已结日终的回购资金数值候选；不执行资金、不证明公司授权、不创建计划或持久账。

use crate::account::{Account, AccountKind, StockId};
use crate::calendar::CivilDate;
use crate::company::issuer_repurchase::{IssuerRepurchaseBook, IssuerRepurchaseStatus};
use crate::company::share_registry::ShareRegistry;
use crate::company::CompanyId;
use crate::money::Money;
use crate::orderbook::AccountId;
use crate::plans::{PlanBook, PlanId};
use crate::runtime::group::GroupAccount;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RepurchaseFundsFailure {
    WrongAccountKind {
        actual: AccountKind,
    },
    PlanOwnerMismatch {
        plan: PlanId,
        actual: AccountId,
    },
    UnknownPlan {
        plan: PlanId,
    },
    WrongOrigin {
        plan: PlanId,
    },
    PlanIdentityMismatch {
        plan: PlanId,
        expected_stock: StockId,
    },
    RegistryMismatch,
    WrongBookStage,
    RegistryDayMismatch {
        requested: CivilDate,
        actual: CivilDate,
    },
    WindowNotClosed {
        day: CivilDate,
        deadline: CivilDate,
    },
    PendingResources,
    NegativeCash {
        cash: i64,
        held: i64,
    },
    ArithmeticOverflow,
    InsufficientCash {
        required: Money,
        available: Money,
    },
    InvalidBook,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RepurchaseFundsIntent {
    pub event_id: String,
    pub issuer: CompanyId,
    pub account: AccountId,
    pub execution_plan: PlanId,
    pub amount: Money,
}

fn identity_and_resources(
    book: &IssuerRepurchaseBook,
    registry: &ShareRegistry,
    account: &Account,
    plans: &PlanBook,
    execution_plan: PlanId,
    catalog: &crate::stock::StockCatalog,
    actual: &GroupAccount,
) -> Result<(), RepurchaseFundsFailure> {
    if account.kind() != AccountKind::IssuerRepurchase {
        return Err(RepurchaseFundsFailure::WrongAccountKind {
            actual: account.kind(),
        });
    }
    if execution_plan.owner != account.id() {
        return Err(RepurchaseFundsFailure::PlanOwnerMismatch {
            plan: execution_plan,
            actual: account.id(),
        });
    }
    book.validate()
        .map_err(|_| RepurchaseFundsFailure::InvalidBook)?;
    registry
        .validate()
        .map_err(|_| RepurchaseFundsFailure::RegistryMismatch)?;
    if registry.stock() != &book.plan().stock || registry.issuer() != &book.plan().issuer {
        return Err(RepurchaseFundsFailure::RegistryMismatch);
    }
    let expected_stock = catalog
        .id(&book.plan().stock.0)
        .ok_or(RepurchaseFundsFailure::RegistryMismatch)?;
    let entry = plans
        .entry(execution_plan)
        .map_err(|_| RepurchaseFundsFailure::UnknownPlan {
            plan: execution_plan,
        })?;
    let crate::plans::PlanEntry::IssuerRepurchase(canonical) = entry else {
        return Err(RepurchaseFundsFailure::WrongOrigin {
            plan: execution_plan,
        });
    };
    if entry.account() != account.id()
        || canonical.stock() != expected_stock
        || canonical.issuer() != book.plan().issuer
        || entry.is_terminal()
    {
        return Err(RepurchaseFundsFailure::PlanIdentityMismatch {
            plan: execution_plan,
            expected_stock,
        });
    }
    let stock =
        usize::try_from(expected_stock.0).map_err(|_| RepurchaseFundsFailure::RegistryMismatch)?;
    let count = actual.shares.len();
    if stock >= count
        || [
            actual.frozen_today.len(),
            actual.foreign_active.len(),
            actual.pending_sell_qty.len(),
            actual.invested_cents.len(),
            actual.recovered_cents.len(),
            actual.ever_held.len(),
        ]
        .iter()
        .any(|len| *len != count)
    {
        return Err(RepurchaseFundsFailure::RegistryMismatch);
    }
    if actual.cash < 0 || actual.buy_purse_held_cents < 0 {
        return Err(RepurchaseFundsFailure::NegativeCash {
            cash: actual.cash,
            held: actual.buy_purse_held_cents,
        });
    }
    actual
        .cash
        .checked_add(actual.buy_purse_held_cents)
        .ok_or(RepurchaseFundsFailure::ArithmeticOverflow)?;
    for index in 0..count {
        actual.shares[index]
            .checked_add(actual.pending_sell_qty[index])
            .ok_or(RepurchaseFundsFailure::ArithmeticOverflow)?;
        if actual.frozen_today[index] > actual.shares[index] {
            return Err(RepurchaseFundsFailure::InvalidBook);
        }
    }
    Ok(())
}

fn no_pending(actual: &GroupAccount) -> Result<(), RepurchaseFundsFailure> {
    if actual.buy_purse_held_cents != 0
        || actual.foreign_active.iter().any(|count| *count != 0)
        || actual.pending_sell_qty.iter().any(|qty| *qty != 0)
    {
        return Err(RepurchaseFundsFailure::PendingResources);
    }
    Ok(())
}

fn intent(
    book: &IssuerRepurchaseBook,
    account: &Account,
    execution_plan: PlanId,
    amount: Money,
) -> RepurchaseFundsIntent {
    RepurchaseFundsIntent {
        event_id: book.plan().event_id.clone(),
        issuer: book.plan().issuer,
        account: account.id(),
        execution_plan,
        amount,
    }
}

/// canonical company origin 与 owner/issuer/stock 只证明身份，不证明实际批准和注资。
/// 将来批准/执行adapter必须另经已审RP-ORIGIN与公司event绑定门，才能使用此候选实际发资金操作。
pub(super) fn credit_candidate(
    book: &IssuerRepurchaseBook,
    registry: &ShareRegistry,
    account: &Account,
    plans: &PlanBook,
    execution_plan: PlanId,
    catalog: &crate::stock::StockCatalog,
    actual: &GroupAccount,
) -> Result<RepurchaseFundsIntent, RepurchaseFundsFailure> {
    identity_and_resources(
        book,
        registry,
        account,
        plans,
        execution_plan,
        catalog,
        actual,
    )?;
    if book.status() != &IssuerRepurchaseStatus::Approved || !book.fills().is_empty() {
        return Err(RepurchaseFundsFailure::WrongBookStage);
    }
    let cash = actual
        .cash
        .checked_add(book.plan().total_budget.cents())
        .ok_or(RepurchaseFundsFailure::ArithmeticOverflow)?;
    cash.checked_add(actual.buy_purse_held_cents)
        .ok_or(RepurchaseFundsFailure::ArithmeticOverflow)?;
    Ok(intent(
        book,
        account,
        execution_plan,
        book.plan().total_budget,
    ))
}

/// 本片仅支持执行窗口已结束且共享专户所有真实在途资源已清的数值回收；不决定是否批准/自动完成。
pub(super) fn closed_window_recycle_candidate(
    book: &IssuerRepurchaseBook,
    registry: &ShareRegistry,
    account: &Account,
    plans: &PlanBook,
    execution_plan: PlanId,
    catalog: &crate::stock::StockCatalog,
    actual: &GroupAccount,
    day: CivilDate,
) -> Result<RepurchaseFundsIntent, RepurchaseFundsFailure> {
    identity_and_resources(
        book,
        registry,
        account,
        plans,
        execution_plan,
        catalog,
        actual,
    )?;
    if !matches!(
        book.status(),
        IssuerRepurchaseStatus::Announced | IssuerRepurchaseStatus::Executing
    ) {
        return Err(RepurchaseFundsFailure::WrongBookStage);
    }
    if registry.settled_on() != day {
        return Err(RepurchaseFundsFailure::RegistryDayMismatch {
            requested: day,
            actual: registry.settled_on(),
        });
    }
    if day < book.plan().window_deadline_on {
        return Err(RepurchaseFundsFailure::WindowNotClosed {
            day,
            deadline: book.plan().window_deadline_on,
        });
    }
    no_pending(actual)?;
    let amount = book
        .remaining_budget()
        .map_err(|_| RepurchaseFundsFailure::InvalidBook)?;
    if amount < Money::ZERO {
        return Err(RepurchaseFundsFailure::InvalidBook);
    }
    let available = Money::from_cents(actual.cash);
    if amount > available {
        return Err(RepurchaseFundsFailure::InsufficientCash {
            required: amount,
            available,
        });
    }
    Ok(intent(book, account, execution_plan, amount))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::StockCode;
    use crate::company::issuer_repurchase::{
        IssuerRepurchasePlan, RepurchaseCompletionPolicy, RepurchaseFillRecord, RepurchasePurpose,
    };
    use crate::company::share_registry::{
        AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRestriction,
    };
    use crate::orderbook::Side;
    use crate::plans::{OpinionSource, PlanOpen, PlanOpinion, PlanPolicy, PlanTarget, Urgency};

    fn date(s: &str) -> CivilDate {
        CivilDate::from_iso(s).unwrap()
    }
    fn book() -> IssuerRepurchaseBook {
        IssuerRepurchaseBook::new(IssuerRepurchasePlan {
            event_id: "repurchase-cash".into(),
            approval_reference: "approved".into(),
            issuer: CompanyId(2),
            stock: StockCode("600002".into()),
            exchange: crate::calendar::CalendarExchange::Sse,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-02"),
            window_start_on: date("2030-06-03"),
            window_deadline_on: date("2030-06-11"),
            price_cap_per_share: Money::from_cents(1200),
            total_budget: Money::from_cents(3_000_000),
            max_shares: 2500,
            purpose: RepurchasePurpose::ReduceCapital,
            completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
            approval_unit_event_id: None,
        })
        .unwrap()
    }
    fn filled() -> IssuerRepurchaseBook {
        let mut book = book();
        book.announce(date("2030-06-02")).unwrap();
        book.record_fill(
            RepurchaseFillRecord {
                stock: book.plan().stock.clone(),
                day: date("2030-06-03"),
                shares: 100,
                gross: Money::from_cents(100_000),
                fees: Money::from_cents(1000),
            },
            &[],
        )
        .unwrap();
        book
    }
    fn registry() -> ShareRegistry {
        registry_on(date("2030-06-11"))
    }
    fn registry_on(settled_on: CivilDate) -> ShareRegistry {
        ShareRegistry::new(
            book().plan().stock.clone(),
            CompanyId(2),
            100,
            settled_on,
            vec![ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![ShareLot {
                    id: "repurchased".into(),
                    qty: 100,
                    acquired_on: date("2030-06-03"),
                    source: AcquisitionSource::CorporateAction {
                        event: "fixture-stock".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            }],
        )
        .unwrap()
    }
    fn catalog() -> crate::stock::StockCatalog {
        crate::stock::StockCatalog::from_metas(vec![
            crate::stock::StockMeta::new("600001"),
            crate::stock::StockMeta::new("600002"),
        ])
        .unwrap()
    }
    fn identity() -> (Account, PlanBook, PlanId) {
        let account = Account::new(AccountId(17), AccountKind::IssuerRepurchase, Money::ZERO);
        let mut plans = PlanBook::new(PlanPolicy::default()).unwrap();
        // 真实 company entry 只证明身份；资金批准仍由调用事务核验。
        let id = plans
            .create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
                owner: account.id(),
                issuer: CompanyId(2),
                stock: StockId(1),
            })
            .unwrap();
        (account, plans, id)
    }
    #[test]
    fn investor_entry_is_never_a_company_funding_source() {
        let (account, _, _) = identity();
        let mut plans = PlanBook::default();
        let id = plans
            .create(PlanOpen {
                account: account.id(),
                code: StockId(1),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: 0,
                    source: OpinionSource::Fundamental,
                },
                confidence_bp: 0,
                urgency: Urgency::Patient,
                horizon_trading_days: 20,
                created_trading_day: 0,
            })
            .unwrap();
        assert!(
            matches!(credit_candidate(&book(), &registry(), &account, &plans, id, &catalog(),
            &GroupAccount::new(9_000_000, 2)), Err(RepurchaseFundsFailure::WrongOrigin { plan }) if plan == id),
            "Investor 个人判断必须给明确 WrongOrigin，不能冒充发行人资金来源"
        );
    }
    #[test]
    fn recycle_uses_real_book_fees_and_leaves_surplus_and_other_plan_money_untouched() {
        let (account, plans, id) = identity();
        let book = filled();
        let actual = GroupAccount::new(9_000_000, 2);
        let before = actual.clone();
        let intent = closed_window_recycle_candidate(
            &book,
            &registry(),
            &account,
            &plans,
            id,
            &catalog(),
            &actual,
            date("2030-06-11"),
        )
        .unwrap();
        assert_eq!(intent.amount, Money::from_cents(2_899_000));
        assert_eq!(intent.execution_plan.owner, account.id());
        assert_eq!(intent.issuer, CompanyId(2));
        assert_eq!(actual, before);
        assert_eq!(book.remaining_budget().unwrap(), intent.amount);
    }
    #[test]
    fn live_held_foreign_or_pending_resources_block_recycle_even_when_cash_is_large() {
        let (account, plans, id) = identity();
        for field in 0..3 {
            let mut actual = GroupAccount::new(9_000_000, 2);
            match field {
                0 => actual.buy_purse_held_cents = 1,
                1 => actual.foreign_active[0] = 1,
                _ => actual.pending_sell_qty[1] = 1,
            }
            assert_eq!(
                closed_window_recycle_candidate(
                    &filled(),
                    &registry(),
                    &account,
                    &plans,
                    id,
                    &catalog(),
                    &actual,
                    date("2030-06-11")
                ),
                Err(RepurchaseFundsFailure::PendingResources)
            );
        }
    }
    #[test]
    fn wrong_real_account_kind_and_canonical_owner_are_rejected() {
        let (account, plans, id) = identity();
        let actual = GroupAccount::new(9_000_000, 2);
        let ordinary = Account::new(account.id(), AccountKind::Retail, Money::ZERO);
        assert!(matches!(
            credit_candidate(
                &book(),
                &registry(),
                &ordinary,
                &plans,
                id,
                &catalog(),
                &actual
            ),
            Err(RepurchaseFundsFailure::WrongAccountKind { .. })
        ));
        let different = Account::new(AccountId(18), AccountKind::IssuerRepurchase, Money::ZERO);
        assert!(matches!(
            credit_candidate(
                &book(),
                &registry(),
                &different,
                &plans,
                id,
                &catalog(),
                &actual
            ),
            Err(RepurchaseFundsFailure::PlanOwnerMismatch { .. })
        ));
    }
    #[test]
    fn wrong_issuer_stock_or_unknown_canonical_plan_is_not_a_funding_source() {
        let (account, plans, id) = identity();
        let actual = GroupAccount::new(9_000_000, 2);
        let wrong_registry = ShareRegistry::new(
            StockCode("600099".into()),
            CompanyId(99),
            1,
            date("2030-06-11"),
            vec![ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![ShareLot {
                    id: "wrong".into(),
                    qty: 1,
                    acquired_on: date("2030-06-03"),
                    source: AcquisitionSource::CorporateAction {
                        event: "wrong".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            }],
        )
        .unwrap();
        assert_eq!(
            credit_candidate(
                &book(),
                &wrong_registry,
                &account,
                &plans,
                id,
                &catalog(),
                &actual
            ),
            Err(RepurchaseFundsFailure::RegistryMismatch)
        );
        assert!(matches!(
            credit_candidate(
                &book(),
                &registry(),
                &account,
                &plans,
                PlanId {
                    sequence: id.sequence + 1,
                    ..id
                },
                &catalog(),
                &actual
            ),
            Err(RepurchaseFundsFailure::UnknownPlan { .. })
        ));
        assert!(matches!(
            credit_candidate(
                &book(),
                &registry(),
                &account,
                &plans,
                id,
                &crate::stock::StockCatalog::from_metas(vec![crate::stock::StockMeta::new(
                    "600002"
                )])
                .unwrap(),
                &actual
            ),
            Err(RepurchaseFundsFailure::PlanIdentityMismatch { .. })
        ));
    }
    #[test]
    fn raw_cash_shortfall_cannot_be_hidden_by_book_budget_or_held_resources() {
        let (account, plans, id) = identity();
        let actual = GroupAccount::new(2_898_999, 2);
        assert_eq!(
            closed_window_recycle_candidate(
                &filled(),
                &registry(),
                &account,
                &plans,
                id,
                &catalog(),
                &actual,
                date("2030-06-11")
            ),
            Err(RepurchaseFundsFailure::InsufficientCash {
                required: Money::from_cents(2_899_000),
                available: Money::from_cents(2_898_999)
            })
        );
    }
    #[test]
    fn credit_checks_public_cash_sum_overflow_and_preserves_account() {
        let (account, plans, id) = identity();
        let mut actual = GroupAccount::new(i64::MAX - 3_000_000, 2);
        actual.buy_purse_held_cents = 1;
        let before = actual.clone();
        assert_eq!(
            credit_candidate(
                &book(),
                &registry(),
                &account,
                &plans,
                id,
                &catalog(),
                &actual
            ),
            Err(RepurchaseFundsFailure::ArithmeticOverflow)
        );
        assert_eq!(actual, before);
    }
    #[test]
    fn wrong_registry_day_and_already_completed_book_are_rejected() {
        let (account, plans, id) = identity();
        let actual = GroupAccount::new(9_000_000, 2);
        assert_eq!(
            closed_window_recycle_candidate(
                &filled(),
                &registry(),
                &account,
                &plans,
                id,
                &catalog(),
                &actual,
                date("2030-06-10")
            ),
            Err(RepurchaseFundsFailure::RegistryDayMismatch {
                requested: date("2030-06-10"),
                actual: date("2030-06-11"),
            })
        );
        let mut completed = filled();
        completed
            .complete(date("2030-06-11"), Money::from_cents(2_899_000))
            .unwrap();
        assert_eq!(
            closed_window_recycle_candidate(
                &completed,
                &registry(),
                &account,
                &plans,
                id,
                &catalog(),
                &actual,
                date("2030-06-11")
            ),
            Err(RepurchaseFundsFailure::WrongBookStage)
        );
    }
    #[test]
    fn matching_registry_day_does_not_allow_recycle_before_window_deadline() {
        let (account, plans, id) = identity();
        let actual = GroupAccount::new(9_000_000, 2);
        let day = date("2030-06-10");
        assert_eq!(
            closed_window_recycle_candidate(
                &filled(),
                &registry_on(day),
                &account,
                &plans,
                id,
                &catalog(),
                &actual,
                day
            ),
            Err(RepurchaseFundsFailure::WindowNotClosed {
                day,
                deadline: date("2030-06-11"),
            })
        );
    }
    #[test]
    fn credit_for_another_stock_does_not_wait_for_existing_buy_purse_to_close() {
        let (account, plans, id) = identity();
        let mut actual = GroupAccount::new(99, 2);
        actual.buy_purse_held_cents = 8000;
        actual.foreign_active[0] = 1;
        let before = actual.clone();
        let intent = credit_candidate(
            &book(),
            &registry(),
            &account,
            &plans,
            id,
            &catalog(),
            &actual,
        )
        .unwrap();
        assert_eq!(intent.amount, Money::from_cents(3_000_000));
        assert_eq!(actual, before);
    }

    #[test]
    fn credit_is_only_an_approved_empty_book_candidate_and_does_not_mutate_money() {
        let (account, plans, id) = identity();
        let actual = GroupAccount::new(99, 2);
        let before = actual.clone();
        let intent = credit_candidate(
            &book(),
            &registry(),
            &account,
            &plans,
            id,
            &catalog(),
            &actual,
        )
        .unwrap();
        assert_eq!(intent.amount, Money::from_cents(3_000_000));
        assert_eq!(actual, before);
        assert_eq!(
            credit_candidate(
                &filled(),
                &registry(),
                &account,
                &plans,
                id,
                &catalog(),
                &actual
            ),
            Err(RepurchaseFundsFailure::WrongBookStage)
        );
    }
}
