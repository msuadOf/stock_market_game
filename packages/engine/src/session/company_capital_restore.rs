//! selected CompanySystem 的已提交资本恢复验证；不创建或修改持久事实。

use crate::Money;
use crate::accounting::AccountingAmount;
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::company::identity::IssuerRegistry;
use crate::company::share_registry::{HolderId, MovementScope, ShareDayReceipt};
use crate::company::share_split::{ShareSplitReceipt, holder_inherited_restriction};
use crate::company::stock_distribution::{HolderCreditLot, holder_credit_lots};
use crate::company::{CompanyId, CompanySystem, CompanySystemError};
use crate::session::SessionCorporateActions;
use crate::stock::{StockCatalog, StockId};
use std::collections::BTreeMap;

/// 日期校验借用本局冻结 policy，不在恢复时另建默认日历。
pub(crate) struct CapitalRestoreInput<'a> {
    pub(crate) setup_stocks: &'a [crate::session::StockSpec],
    pub(crate) setup_issuers: &'a IssuerRegistry,
    pub(crate) system: &'a CompanySystem,
    pub(crate) actions: &'a SessionCorporateActions,
    pub(crate) catalog: &'a StockCatalog,
    pub(crate) calendar: &'a TradingCalendar,
    pub(crate) committed_through: CivilDate,
    pub(crate) initial_par: Option<Money>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum CorporateCapitalKind {
    PublicMarket,
    StockDistribution,
    RightsOffering,
    ShareReDenomination,
    IssuerRepurchaseCancellation,
}

/// 数值身份与原 receipt 下标都保留；错误文本不承担跨层分类职责。
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub(crate) struct CapitalRestoreContext {
    pub(crate) company: Option<CompanyId>,
    pub(crate) stock: Option<StockId>,
    pub(crate) kind: Option<CorporateCapitalKind>,
    pub(crate) event_id: Option<String>,
    pub(crate) receipt_index: Option<usize>,
    pub(crate) day: Option<CivilDate>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CompanyCapitalRestoreError {
    #[error("资本恢复公司事实校验失败：{context:?}：{source}")]
    System {
        context: CapitalRestoreContext,
        #[source]
        source: CompanySystemError,
    },
    #[error("资本恢复稳定身份不一致：公司 {company:?}，字段 {field}")]
    StableIdentity {
        company: CompanyId,
        field: &'static str,
    },
    #[error("资本恢复发行股数不一致：{context:?}，预期 {expected} 股，实际 {actual} 股")]
    IssuedShares {
        context: CapitalRestoreContext,
        expected: u64,
        actual: u64,
    },
    #[error("资本恢复名册校验失败：{context:?}：{source}")]
    Registry {
        context: CapitalRestoreContext,
        #[source]
        source: crate::company::share_registry::ShareRegistryError,
    },
    #[error("资本恢复缺少名册：{context:?}")]
    MissingRegistry { context: CapitalRestoreContext },
    #[error("资本恢复存在未绑定动作的名册回执：{context:?}")]
    UnboundReceipt { context: CapitalRestoreContext },
    #[error("资本恢复已发生事实晚于提交截止 {committed_through}：{context:?}")]
    FutureFact {
        context: CapitalRestoreContext,
        committed_through: CivilDate,
    },
    #[error("资本恢复输入身份重复：{context:?}")]
    Duplicate { context: CapitalRestoreContext },
    #[error("资本恢复证券目录映射无效：公司 {company:?}，证券 {stock_code}")]
    CatalogMapping {
        company: Option<CompanyId>,
        stock_code: String,
    },
    #[error("资本恢复开局证券映射无效：证券 {stock_code}，原因 {reason:?}")]
    SetupStockMapping {
        stock_code: String,
        reason: SetupStockMappingReason,
    },
    #[error("资本恢复正式计划交易所不一致：{context:?}，预期 {expected:?}，实际 {actual:?}")]
    ExchangeMismatch {
        context: CapitalRestoreContext,
        expected: crate::calendar::CalendarExchange,
        actual: crate::calendar::CalendarExchange,
    },
    #[error("资本恢复算术不能表示合法股本：{context:?}")]
    Arithmetic { context: CapitalRestoreContext },
    #[error("资本恢复正式动作缺少账簿：{context:?}")]
    MissingBook { context: CapitalRestoreContext },
    #[error("资本恢复正式账簿缺少金融事实：{context:?}")]
    MissingFinanceFact { context: CapitalRestoreContext },
    #[error("资本恢复送转账簿无效：{context:?}：{source}")]
    StockDistribution {
        context: CapitalRestoreContext,
        #[source]
        source: crate::company::stock_distribution::StockDistributionError,
    },
    #[error("资本恢复动作事实不一致：{context:?}，字段 {field}")]
    ActionMismatch {
        context: CapitalRestoreContext,
        field: &'static str,
    },
    #[error("资本恢复资本不一致：{context:?}，预期 {expected:?}，实际 {actual:?}")]
    RegisteredCapital {
        context: CapitalRestoreContext,
        expected: AccountingAmount,
        actual: AccountingAmount,
    },
    #[error("资本恢复已生效动作缺少名册回执：{context:?}")]
    MissingReceipt { context: CapitalRestoreContext },
    #[error("资本恢复拆缩股账簿无效：{context:?}：{source}")]
    ShareSplit {
        context: CapitalRestoreContext,
        #[source]
        source: crate::company::share_split::ShareSplitError,
    },
    #[error("资本恢复配股账簿无效：{context:?}：{source}")]
    RightsOffering {
        context: CapitalRestoreContext,
        #[source]
        source: crate::company::rights_offering::RightsOfferingError,
    },
    #[error("资本恢复回购账簿无效：{context:?}：{source}")]
    IssuerRepurchase {
        context: CapitalRestoreContext,
        #[source]
        source: crate::company::issuer_repurchase::IssuerRepurchaseError,
    },
    #[error("资本恢复缺少已批准的前置验证 {required:?}：{context:?}")]
    Prerequisite {
        context: CapitalRestoreContext,
        required: CapitalRestorePrerequisite,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SetupStockMappingReason {
    Missing,
    Unexpected,
    Duplicate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapitalRestorePrerequisite {
    SameDayShareUnits,
    TreasuryShareUnitBuckets,
    RepurchaseLotAttribution,
}

/// 验证结果是恢复过程的派生结论，不进入 SaveSlot。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompanyCapitalReplay {
    pub(crate) stock: Option<StockId>,
    pub(crate) issued_shares: u64,
    pub(crate) receipt_frontier: usize,
    pub(crate) registered_capital: Option<AccountingAmount>,
    pub(crate) initial_registered_capital: Option<AccountingAmount>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CapitalReplaySummary {
    pub(crate) companies: BTreeMap<CompanyId, CompanyCapitalReplay>,
}

pub(crate) fn validate_company_capital_restore(
    input: CapitalRestoreInput<'_>,
) -> Result<CapitalReplaySummary, CompanyCapitalRestoreError> {
    input
        .system
        .validate_restored()
        .map_err(|source| CompanyCapitalRestoreError::System {
            context: CapitalRestoreContext::default(),
            source,
        })?;
    validate_stable_issuers(&input)?;
    let setup_exchanges = validate_setup_stocks(&input)?;
    let mut registries = BTreeMap::new();
    for registry in &input.actions.registries {
        let company = *registry.issuer();
        let stock = input.catalog.id(&registry.stock().0).ok_or_else(|| {
            CompanyCapitalRestoreError::CatalogMapping {
                company: Some(company),
                stock_code: registry.stock().0.clone(),
            }
        })?;
        let context = CapitalRestoreContext {
            company: Some(company),
            stock: Some(stock),
            ..Default::default()
        };
        if input.system.issuers().issuer_of(registry.stock()) != Some(company) {
            return Err(CompanyCapitalRestoreError::CatalogMapping {
                company: Some(company),
                stock_code: registry.stock().0.clone(),
            });
        }
        if registries.insert(company, registry).is_some() {
            return Err(CompanyCapitalRestoreError::Duplicate { context });
        }
        registry
            .validate()
            .map_err(|source| CompanyCapitalRestoreError::Registry {
                context: context.clone(),
                source,
            })?;
    }
    let mut bindings = prepare_stock_distributions(&input, &setup_exchanges)?;
    prepare_share_splits(&input, &setup_exchanges, &mut bindings)?;
    prepare_rights_offerings(&input, &setup_exchanges, &mut bindings)?;
    prepare_repurchase_cancellations(&input, &setup_exchanges, &mut bindings)?;
    let mut companies = BTreeMap::new();
    for (company, initial) in input.setup_issuers.iter() {
        let current = input.system.issuers().get(company).ok_or(
            CompanyCapitalRestoreError::StableIdentity {
                company: *company,
                field: "company-set",
            },
        )?;
        let stock = initial
            .listed_stock
            .as_ref()
            .map(|code| {
                input.catalog.id(&code.0).ok_or_else(|| {
                    CompanyCapitalRestoreError::CatalogMapping {
                        company: Some(*company),
                        stock_code: code.0.clone(),
                    }
                })
            })
            .transpose()?;
        let context = CapitalRestoreContext {
            company: Some(*company),
            stock,
            ..Default::default()
        };
        let registered_capital = input
            .system
            .dividend_legal_facts(company)
            .map_err(|source| CompanyCapitalRestoreError::System {
                context: context.clone(),
                source,
            })?
            .map(|facts| facts.registered_capital);
        let expected_total_delta = bindings
            .movements
            .values()
            .filter(|movement| movement.context.company == Some(*company))
            .try_fold(0_i128, |total, movement| {
                total
                    .checked_add(movement.capital_delta.cents())
                    .ok_or_else(|| CompanyCapitalRestoreError::Arithmetic {
                        context: context.clone(),
                    })
            })?;
        // 手工装配没有 approval snapshot 时，从当前合法资本与已核动作逆推内部根。
        // 该派生根不进入存档，也不用于证明外部资产来源。
        let initial_registered_capital = registered_capital
            .map(|actual| {
                let cents = if let Some(par) = input.initial_par {
                    i128::from(par.cents()).checked_mul(i128::from(initial.issued_shares))
                } else {
                    actual.cents().checked_sub(expected_total_delta)
                };
                cents
                    .filter(|value| *value > 0)
                    .map(AccountingAmount::from_cents)
                    .ok_or_else(|| CompanyCapitalRestoreError::Arithmetic {
                        context: context.clone(),
                    })
            })
            .transpose()?;
        let mut working_capital = initial_registered_capital;
        let mut issued_shares = initial.issued_shares;
        let mut receipt_frontier = 0;
        let mut share_frontiers = vec![issued_shares];
        let mut capital_delta = 0_i128;
        if let Some(registry) = registries.get(company) {
            for (receipt_index, receipt) in registry.receipts().iter().enumerate() {
                let bound_context = bindings
                    .movements
                    .get(&(*company, receipt.request.event_id.clone()))
                    .map(|movement| &movement.context);
                let receipt_context = CapitalRestoreContext {
                    event_id: bound_context
                        .and_then(|bound| bound.event_id.clone())
                        .or_else(|| Some(receipt.request.event_id.clone())),
                    receipt_index: Some(receipt_index),
                    day: Some(receipt.request.day),
                    kind: matches!(
                        receipt.request.scope,
                        crate::company::share_registry::MovementScope::PublicMarket
                    )
                    .then_some(CorporateCapitalKind::PublicMarket)
                    .or_else(|| bound_context.and_then(|bound| bound.kind)),
                    ..context.clone()
                };
                if receipt.request.day > input.committed_through {
                    return Err(CompanyCapitalRestoreError::FutureFact {
                        context: receipt_context,
                        committed_through: input.committed_through,
                    });
                }
                let delta = receipt
                    .request
                    .changes
                    .iter()
                    .try_fold(0_i128, |sum, change| {
                        sum.checked_add(change.change).ok_or_else(|| {
                            CompanyCapitalRestoreError::Arithmetic {
                                context: receipt_context.clone(),
                            }
                        })
                    })?;
                let after = i128::from(issued_shares)
                    .checked_add(delta)
                    .and_then(|value| u64::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| CompanyCapitalRestoreError::Arithmetic {
                        context: receipt_context.clone(),
                    })?;
                if matches!(receipt.request.scope, MovementScope::PublicMarket) {
                    check_shares(issued_shares, after, receipt_context.clone())?;
                } else {
                    let movement = bindings
                        .movements
                        .get_mut(&(*company, receipt.request.event_id.clone()))
                        .ok_or_else(|| CompanyCapitalRestoreError::UnboundReceipt {
                            context: receipt_context.clone(),
                        })?;
                    validate_bound_receipt(
                        receipt,
                        movement,
                        receipt_index,
                        issued_shares,
                        after,
                        working_capital,
                    )?;
                    capital_delta = capital_delta
                        .checked_add(movement.capital_delta.cents())
                        .ok_or_else(|| CompanyCapitalRestoreError::Arithmetic {
                            context: receipt_context.clone(),
                        })?;
                    if let Some(before) = working_capital {
                        working_capital =
                            Some(before.add(movement.capital_delta).map_err(|source| {
                                CompanyCapitalRestoreError::System {
                                    context: receipt_context.clone(),
                                    source: CompanySystemError::Accounting(source),
                                }
                            })?);
                    }
                }
                issued_shares = after;
                receipt_frontier += 1;
                share_frontiers.push(issued_shares);
            }
            // 先保留具体未来 receipt 的下标错误，再拒绝没有 receipt 却超前的登记静点。
            // ShareRegistry::validate 已保证全部登记快照不晚于该 settled_on。
            if registry.settled_on() > input.committed_through {
                return Err(CompanyCapitalRestoreError::FutureFact {
                    context: CapitalRestoreContext {
                        day: Some(registry.settled_on()),
                        ..context.clone()
                    },
                    committed_through: input.committed_through,
                });
            }
            check_shares(issued_shares, registry.issued_shares(), context.clone())?;
        }
        for movement in bindings
            .movements
            .values()
            .filter(|movement| movement.context.company == Some(*company))
        {
            if !movement.consumed {
                let error = if registries.contains_key(company) {
                    CompanyCapitalRestoreError::MissingReceipt {
                        context: movement.context.clone(),
                    }
                } else {
                    CompanyCapitalRestoreError::MissingRegistry {
                        context: movement.context.clone(),
                    }
                };
                return Err(error);
            }
        }
        for frozen in bindings
            .snapshots
            .iter()
            .filter(|frozen| frozen.context.company == Some(*company))
        {
            let frontier = usize::try_from(frozen.settled_receipts).map_err(|_| {
                CompanyCapitalRestoreError::Arithmetic {
                    context: frozen.context.clone(),
                }
            })?;
            let replayed = share_frontiers.get(frontier).ok_or_else(|| {
                CompanyCapitalRestoreError::ActionMismatch {
                    context: frozen.context.clone(),
                    field: "settled_receipts",
                }
            })?;
            check_shares(
                *replayed,
                frozen.issued_shares,
                CapitalRestoreContext {
                    receipt_index: Some(frontier),
                    ..frozen.context.clone()
                },
            )?;
        }
        check_shares(issued_shares, current.issued_shares, context.clone())?;
        if let (Some(par), Some(actual)) = (input.initial_par, registered_capital) {
            let expected = i128::from(par.cents())
                .checked_mul(i128::from(initial.issued_shares))
                .and_then(|capital| capital.checked_add(capital_delta))
                .filter(|capital| *capital > 0)
                .ok_or_else(|| CompanyCapitalRestoreError::Arithmetic {
                    context: context.clone(),
                })?;
            if actual != AccountingAmount::from_cents(expected) {
                return Err(CompanyCapitalRestoreError::RegisteredCapital {
                    context: context.clone(),
                    expected: AccountingAmount::from_cents(expected),
                    actual,
                });
            }
        }
        companies.insert(
            *company,
            CompanyCapitalReplay {
                stock,
                issued_shares,
                receipt_frontier,
                registered_capital,
                initial_registered_capital,
            },
        );
    }
    Ok(CapitalReplaySummary { companies })
}

fn check_shares(
    expected: u64,
    actual: u64,
    context: CapitalRestoreContext,
) -> Result<(), CompanyCapitalRestoreError> {
    if expected != actual {
        return Err(CompanyCapitalRestoreError::IssuedShares {
            context,
            expected,
            actual,
        });
    }
    Ok(())
}

fn check_action_fields(
    context: &CapitalRestoreContext,
    fields: &[(&'static str, bool)],
) -> Result<(), CompanyCapitalRestoreError> {
    for (field, equal) in fields {
        if !equal {
            return Err(CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field,
            });
        }
    }
    Ok(())
}

fn validate_stable_issuers(
    input: &CapitalRestoreInput<'_>,
) -> Result<(), CompanyCapitalRestoreError> {
    let mut mapped_stocks = BTreeMap::new();
    for (company, expected) in input.setup_issuers.iter() {
        let actual = input.system.issuers().get(company).ok_or(
            CompanyCapitalRestoreError::StableIdentity {
                company: *company,
                field: "company-set",
            },
        )?;
        for (field, equal) in [
            ("code", expected.code == actual.code),
            ("name", expected.name == actual.name),
            ("industry", expected.industry == actual.industry),
            ("kind", expected.kind == actual.kind),
            ("listed_stock", expected.listed_stock == actual.listed_stock),
            ("group_parent", expected.group_parent == actual.group_parent),
        ] {
            if !equal {
                return Err(CompanyCapitalRestoreError::StableIdentity {
                    company: *company,
                    field,
                });
            }
        }
        if let Some(code) = &expected.listed_stock {
            let stock = input.catalog.id(&code.0).ok_or_else(|| {
                CompanyCapitalRestoreError::CatalogMapping {
                    company: Some(*company),
                    stock_code: code.0.clone(),
                }
            })?;
            if mapped_stocks.insert(stock, *company).is_some() {
                return Err(CompanyCapitalRestoreError::CatalogMapping {
                    company: Some(*company),
                    stock_code: code.0.clone(),
                });
            }
        }
    }
    for (company, _) in input.system.issuers().iter() {
        if input.setup_issuers.get(company).is_none() {
            return Err(CompanyCapitalRestoreError::StableIdentity {
                company: *company,
                field: "company-set",
            });
        }
    }
    for (stock, meta) in input.catalog.iter() {
        if !mapped_stocks.contains_key(&stock) {
            return Err(CompanyCapitalRestoreError::CatalogMapping {
                company: None,
                stock_code: meta.code.clone(),
            });
        }
    }
    Ok(())
}

/// 仅在恢复过程中索引已校验的开局证券，不创建额外持久权威。
fn validate_setup_stocks(
    input: &CapitalRestoreInput<'_>,
) -> Result<BTreeMap<StockId, CalendarExchange>, CompanyCapitalRestoreError> {
    let mut exchanges = BTreeMap::new();
    for spec in input.setup_stocks {
        let stock = input.catalog.id(&spec.code.0).ok_or_else(|| {
            CompanyCapitalRestoreError::SetupStockMapping {
                stock_code: spec.code.0.clone(),
                reason: SetupStockMappingReason::Unexpected,
            }
        })?;
        let exchange = match spec.exchange {
            crate::session::StockExchange::Shanghai => CalendarExchange::Sse,
            crate::session::StockExchange::Shenzhen => CalendarExchange::Szse,
        };
        if exchanges.insert(stock, exchange).is_some() {
            return Err(CompanyCapitalRestoreError::SetupStockMapping {
                stock_code: spec.code.0.clone(),
                reason: SetupStockMappingReason::Duplicate,
            });
        }
    }
    for (stock, meta) in input.catalog.iter() {
        if !exchanges.contains_key(&stock) {
            return Err(CompanyCapitalRestoreError::SetupStockMapping {
                stock_code: meta.code.clone(),
                reason: SetupStockMappingReason::Missing,
            });
        }
    }
    Ok(exchanges)
}

fn validate_plan_security(
    input: &CapitalRestoreInput<'_>,
    setup_exchanges: &BTreeMap<StockId, CalendarExchange>,
    context: &CapitalRestoreContext,
    stock_code: &crate::StockCode,
    actual: CalendarExchange,
) -> Result<(), CompanyCapitalRestoreError> {
    if input.system.issuers().issuer_of(stock_code) != context.company {
        return Err(CompanyCapitalRestoreError::ActionMismatch {
            context: context.clone(),
            field: "issuer-listed-stock",
        });
    }
    let stock = input.catalog.id(&stock_code.0).ok_or_else(|| {
        CompanyCapitalRestoreError::CatalogMapping {
            company: context.company,
            stock_code: stock_code.0.clone(),
        }
    })?;
    let expected = setup_exchanges.get(&stock).ok_or_else(|| {
        CompanyCapitalRestoreError::SetupStockMapping {
            stock_code: stock_code.0.clone(),
            reason: SetupStockMappingReason::Missing,
        }
    })?;
    if *expected != actual {
        return Err(CompanyCapitalRestoreError::ExchangeMismatch {
            context: context.clone(),
            expected: *expected,
            actual,
        });
    }
    Ok(())
}

struct ExpectedCapitalMovement {
    context: CapitalRestoreContext,
    day: CivilDate,
    scope: MovementScope,
    allocation: ExpectedCapitalAllocation,
    capital_delta: AccountingAmount,
    consumed: bool,
}

enum ExpectedCapitalAllocation {
    PositiveLots(Vec<HolderCreditLot>),
    ReDenomination(ShareSplitReceipt),
    RepurchaseCancellation {
        bought_lots: BTreeMap<String, BoughtRepurchaseLot>,
        cancelled_shares: u64,
    },
}

struct BoughtRepurchaseLot {
    acquired_on: CivilDate,
    qty: u64,
    source: crate::company::share_registry::AcquisitionSource,
    restriction: crate::company::share_registry::ShareRestriction,
}

struct FrozenCapitalRegistration {
    context: CapitalRestoreContext,
    settled_receipts: u64,
    issued_shares: u64,
}

#[derive(Default)]
struct CapitalBindings {
    movements: BTreeMap<(CompanyId, String), ExpectedCapitalMovement>,
    snapshots: Vec<FrozenCapitalRegistration>,
}

/// 按正式 kind 生成完整身份；恢复时只比较该完整值，不解析输入字符串前缀。
pub(crate) fn capital_receipt_event_id(kind: CorporateCapitalKind, event_id: &str) -> String {
    let prefix = match kind {
        CorporateCapitalKind::PublicMarket => return event_id.to_owned(),
        CorporateCapitalKind::StockDistribution => "stock-distribution",
        CorporateCapitalKind::RightsOffering => "rights-offering",
        CorporateCapitalKind::ShareReDenomination => "share-transform",
        CorporateCapitalKind::IssuerRepurchaseCancellation => "issuer-repurchase-cancellation",
    };
    format!("{prefix}:{event_id}")
}

fn prepare_stock_distributions(
    input: &CapitalRestoreInput<'_>,
    setup_exchanges: &BTreeMap<StockId, CalendarExchange>,
) -> Result<CapitalBindings, CompanyCapitalRestoreError> {
    let mut bindings = CapitalBindings::default();
    let mut all_facts = BTreeMap::new();
    for (company, spec) in input.system.issuers().iter() {
        let context = CapitalRestoreContext {
            company: Some(*company),
            stock: spec
                .listed_stock
                .as_ref()
                .and_then(|code| input.catalog.id(&code.0)),
            kind: Some(CorporateCapitalKind::StockDistribution),
            ..Default::default()
        };
        let facts = input
            .system
            .stock_distribution_facts(company)
            .map_err(|source| CompanyCapitalRestoreError::System {
                context: context.clone(),
                source,
            })?;
        for fact in facts {
            let key = (*company, fact.event_id.clone());
            if all_facts.insert(key, fact).is_some() {
                return Err(CompanyCapitalRestoreError::Duplicate { context });
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for book in &input.actions.stock_distributions {
        let plan = book.plan();
        let context = CapitalRestoreContext {
            company: Some(plan.issuer),
            stock: input.catalog.id(&plan.stock.0),
            kind: Some(CorporateCapitalKind::StockDistribution),
            event_id: Some(plan.event_id.clone()),
            day: Some(plan.approved_on),
            ..Default::default()
        };
        validate_plan_security(input, setup_exchanges, &context, &plan.stock, plan.exchange)?;
        if !seen.insert((plan.issuer, plan.event_id.clone())) {
            return Err(CompanyCapitalRestoreError::Duplicate { context });
        }
        book.validate_with_calendar(input.calendar)
            .map_err(|source| CompanyCapitalRestoreError::StockDistribution {
                context: context.clone(),
                source,
            })?;
        let fact = all_facts
            .remove(&(plan.issuer, plan.event_id.clone()))
            .ok_or_else(|| CompanyCapitalRestoreError::MissingFinanceFact {
                context: context.clone(),
            })?;
        check_action_fields(
            &context,
            &[
                ("approved_on", fact.approved_on == plan.approved_on),
                (
                    "approval_reference",
                    fact.approval_reference == plan.approval_reference,
                ),
                ("kind", fact.kind == plan.kind),
                (
                    "approved_total_new_shares",
                    fact.new_shares == plan.approved_total_new_shares,
                ),
                ("credited_on", fact.credited_on == book.credited_on()),
            ],
        )?;
        for occurred in [
            Some(plan.approved_on),
            (!matches!(
                book.status(),
                crate::company::stock_distribution::StockDistributionStatus::Approved
            ))
            .then_some(plan.announced_on),
            book.registration().map(|snapshot| snapshot.registered_on()),
            book.credited_on(),
        ]
        .into_iter()
        .flatten()
        {
            if occurred > input.committed_through {
                return Err(CompanyCapitalRestoreError::FutureFact {
                    context: CapitalRestoreContext {
                        day: Some(occurred),
                        ..context.clone()
                    },
                    committed_through: input.committed_through,
                });
            }
        }
        if let Some(snapshot) = book.registration() {
            let registry = input
                .actions
                .registries
                .iter()
                .find(|registry| {
                    registry.issuer() == &plan.issuer && registry.stock() == &plan.stock
                })
                .ok_or_else(|| CompanyCapitalRestoreError::MissingRegistry {
                    context: context.clone(),
                })?;
            if registry.registration(snapshot.event_id()) != Some(snapshot) {
                return Err(CompanyCapitalRestoreError::ActionMismatch {
                    context,
                    field: "registration",
                });
            }
            bindings.snapshots.push(FrozenCapitalRegistration {
                context: context.clone(),
                settled_receipts: snapshot.settled_receipts(),
                issued_shares: snapshot.issued_shares(),
            });
        }
        if let Some(credited_on) = book.credited_on() {
            let receipt =
                book.receipt()
                    .ok_or_else(|| CompanyCapitalRestoreError::ActionMismatch {
                        context: context.clone(),
                        field: "receipt",
                    })?;
            let lots = holder_credit_lots(receipt, credited_on).map_err(|source| {
                CompanyCapitalRestoreError::StockDistribution {
                    context: context.clone(),
                    source,
                }
            })?;
            let movement = ExpectedCapitalMovement {
                context: CapitalRestoreContext {
                    day: Some(credited_on),
                    ..context.clone()
                },
                day: credited_on,
                scope: MovementScope::NonTradingTransfer {
                    basis: plan.approval_reference.clone(),
                },
                allocation: ExpectedCapitalAllocation::PositiveLots(lots),
                capital_delta: fact.capital_increase,
                consumed: false,
            };
            let id =
                capital_receipt_event_id(CorporateCapitalKind::StockDistribution, &plan.event_id);
            if bindings
                .movements
                .insert((plan.issuer, id), movement)
                .is_some()
            {
                return Err(CompanyCapitalRestoreError::Duplicate { context });
            }
        }
    }
    if let Some(((company, event), _)) = all_facts.first_key_value() {
        let stock = input
            .system
            .issuers()
            .get(company)
            .and_then(|spec| spec.listed_stock.as_ref())
            .and_then(|code| input.catalog.id(&code.0));
        return Err(CompanyCapitalRestoreError::MissingBook {
            context: CapitalRestoreContext {
                company: Some(*company),
                stock,
                kind: Some(CorporateCapitalKind::StockDistribution),
                event_id: Some(event.clone()),
                ..Default::default()
            },
        });
    }
    input
        .system
        .validate_stock_distribution_books(&input.actions.stock_distributions)
        .map_err(|source| CompanyCapitalRestoreError::System {
            context: CapitalRestoreContext {
                kind: Some(CorporateCapitalKind::StockDistribution),
                ..Default::default()
            },
            source,
        })?;
    Ok(bindings)
}

fn prepare_share_splits(
    input: &CapitalRestoreInput<'_>,
    setup_exchanges: &BTreeMap<StockId, CalendarExchange>,
    bindings: &mut CapitalBindings,
) -> Result<(), CompanyCapitalRestoreError> {
    let mut facts = BTreeMap::new();
    for (company, spec) in input.system.issuers().iter() {
        let context = CapitalRestoreContext {
            company: Some(*company),
            stock: spec
                .listed_stock
                .as_ref()
                .and_then(|code| input.catalog.id(&code.0)),
            kind: Some(CorporateCapitalKind::ShareReDenomination),
            ..Default::default()
        };
        for fact in input.system.share_split_facts(company).map_err(|source| {
            CompanyCapitalRestoreError::System {
                context: context.clone(),
                source,
            }
        })? {
            if facts
                .insert((*company, fact.event_id.clone()), fact)
                .is_some()
            {
                return Err(CompanyCapitalRestoreError::Duplicate { context });
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for book in &input.actions.share_splits {
        let plan = book.plan();
        let context = CapitalRestoreContext {
            company: Some(plan.issuer),
            stock: input.catalog.id(&plan.stock.0),
            kind: Some(CorporateCapitalKind::ShareReDenomination),
            event_id: Some(plan.event_id.clone()),
            day: Some(plan.approved_on),
            ..Default::default()
        };
        validate_plan_security(input, setup_exchanges, &context, &plan.stock, plan.exchange)?;
        if !seen.insert((plan.issuer, plan.event_id.clone())) {
            return Err(CompanyCapitalRestoreError::Duplicate { context });
        }
        book.validate_with_calendar(input.calendar)
            .map_err(|source| CompanyCapitalRestoreError::ShareSplit {
                context: context.clone(),
                source,
            })?;
        let fact = facts
            .remove(&(plan.issuer, plan.event_id.clone()))
            .ok_or_else(|| CompanyCapitalRestoreError::MissingFinanceFact {
                context: context.clone(),
            })?;
        check_action_fields(
            &context,
            &[
                ("approved_on", fact.approved_on == plan.approved_on),
                (
                    "approval_reference",
                    fact.approval_reference == plan.approval_reference,
                ),
                ("direction", fact.direction == plan.direction),
                ("ratio", fact.ratio == plan.ratio),
                ("settled_on", fact.settled_on == book.settled_on()),
            ],
        )?;
        for occurred in [
            Some(plan.approved_on),
            (!matches!(
                book.status(),
                crate::company::share_split::ShareSplitStatus::Approved
            ))
            .then_some(plan.announced_on),
            book.registration().map(|snapshot| snapshot.registered_on()),
            book.settled_on(),
        ]
        .into_iter()
        .flatten()
        {
            if occurred > input.committed_through {
                return Err(CompanyCapitalRestoreError::FutureFact {
                    context: CapitalRestoreContext {
                        day: Some(occurred),
                        ..context.clone()
                    },
                    committed_through: input.committed_through,
                });
            }
        }
        if let Some(snapshot) = book.registration() {
            let registry = input
                .actions
                .registries
                .iter()
                .find(|registry| {
                    registry.issuer() == &plan.issuer && registry.stock() == &plan.stock
                })
                .ok_or_else(|| CompanyCapitalRestoreError::MissingRegistry {
                    context: context.clone(),
                })?;
            if registry.registration(snapshot.event_id()) != Some(snapshot) {
                return Err(CompanyCapitalRestoreError::ActionMismatch {
                    context,
                    field: "registration",
                });
            }
            bindings.snapshots.push(FrozenCapitalRegistration {
                context: context.clone(),
                settled_receipts: snapshot.settled_receipts(),
                issued_shares: snapshot.issued_shares(),
            });
        }
        if let Some(day) = book.settled_on() {
            let receipt =
                book.receipt()
                    .ok_or_else(|| CompanyCapitalRestoreError::ActionMismatch {
                        context: context.clone(),
                        field: "receipt",
                    })?;
            check_shares(
                receipt.issued_shares_before,
                fact.issued_shares_before,
                context.clone(),
            )?;
            check_shares(
                receipt.issued_shares_after,
                fact.issued_shares_after,
                context.clone(),
            )?;
            // REG-P 接线前只支持与其他资本动作不交叉、同日无实际市场换手的正式转换。
            if bindings.movements.values().any(|movement| {
                movement.context.company == Some(plan.issuer) && movement.day == day
            }) || input
                .actions
                .registries
                .iter()
                .filter(|registry| registry.issuer() == &plan.issuer)
                .flat_map(|registry| registry.receipts())
                .any(|receipt| {
                    receipt.request.day == day
                        && matches!(receipt.request.scope, MovementScope::PublicMarket)
                        && !receipt.request.changes.is_empty()
                })
            {
                return Err(CompanyCapitalRestoreError::Prerequisite {
                    context,
                    required: CapitalRestorePrerequisite::SameDayShareUnits,
                });
            }
            if receipt.holders.iter().any(|holder| {
                holder.holder == HolderId::IssuerTreasury && holder.original_shares > 0
            }) {
                return Err(CompanyCapitalRestoreError::Prerequisite {
                    context,
                    required: CapitalRestorePrerequisite::TreasuryShareUnitBuckets,
                });
            }
            let capital_delta = fact
                .registered_capital_reduction
                .cents()
                .checked_neg()
                .ok_or_else(|| CompanyCapitalRestoreError::Arithmetic {
                    context: context.clone(),
                })?;
            let movement = ExpectedCapitalMovement {
                context: CapitalRestoreContext {
                    day: Some(day),
                    ..context.clone()
                },
                day,
                scope: MovementScope::ShareReDenomination {
                    basis: plan.approval_reference.clone(),
                },
                allocation: ExpectedCapitalAllocation::ReDenomination(receipt.clone()),
                capital_delta: AccountingAmount::from_cents(capital_delta),
                consumed: false,
            };
            let id =
                capital_receipt_event_id(CorporateCapitalKind::ShareReDenomination, &plan.event_id);
            if bindings
                .movements
                .insert((plan.issuer, id), movement)
                .is_some()
            {
                return Err(CompanyCapitalRestoreError::Duplicate { context });
            }
        }
    }
    if let Some(((company, event), _)) = facts.first_key_value() {
        let stock = input
            .system
            .issuers()
            .get(company)
            .and_then(|spec| spec.listed_stock.as_ref())
            .and_then(|code| input.catalog.id(&code.0));
        return Err(CompanyCapitalRestoreError::MissingBook {
            context: CapitalRestoreContext {
                company: Some(*company),
                stock,
                kind: Some(CorporateCapitalKind::ShareReDenomination),
                event_id: Some(event.clone()),
                ..Default::default()
            },
        });
    }
    input
        .system
        .validate_share_split_books(&input.actions.share_splits)
        .map_err(|source| CompanyCapitalRestoreError::System {
            context: CapitalRestoreContext {
                kind: Some(CorporateCapitalKind::ShareReDenomination),
                ..Default::default()
            },
            source,
        })?;
    Ok(())
}

fn prepare_rights_offerings(
    input: &CapitalRestoreInput<'_>,
    setup_exchanges: &BTreeMap<StockId, CalendarExchange>,
    bindings: &mut CapitalBindings,
) -> Result<(), CompanyCapitalRestoreError> {
    let mut facts = BTreeMap::new();
    for (company, spec) in input.system.issuers().iter() {
        let context = CapitalRestoreContext {
            company: Some(*company),
            stock: spec
                .listed_stock
                .as_ref()
                .and_then(|code| input.catalog.id(&code.0)),
            kind: Some(CorporateCapitalKind::RightsOffering),
            ..Default::default()
        };
        for fact in input
            .system
            .rights_offering_facts(company)
            .map_err(|source| CompanyCapitalRestoreError::System {
                context: context.clone(),
                source,
            })?
        {
            if facts
                .insert((*company, fact.event_id.clone()), fact)
                .is_some()
            {
                return Err(CompanyCapitalRestoreError::Duplicate { context });
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for book in &input.actions.rights_offerings {
        let plan = book.plan();
        let context = CapitalRestoreContext {
            company: Some(plan.issuer),
            stock: input.catalog.id(&plan.stock.0),
            kind: Some(CorporateCapitalKind::RightsOffering),
            event_id: Some(plan.event_id.clone()),
            day: Some(plan.approved_on),
            ..Default::default()
        };
        validate_plan_security(input, setup_exchanges, &context, &plan.stock, plan.exchange)?;
        if !seen.insert((plan.issuer, plan.event_id.clone())) {
            return Err(CompanyCapitalRestoreError::Duplicate { context });
        }
        book.validate_with_calendar(input.calendar)
            .map_err(|source| CompanyCapitalRestoreError::RightsOffering {
                context: context.clone(),
                source,
            })?;
        let fact = facts
            .remove(&(plan.issuer, plan.event_id.clone()))
            .ok_or_else(|| CompanyCapitalRestoreError::MissingFinanceFact {
                context: context.clone(),
            })?;
        check_action_fields(
            &context,
            &[
                ("approved_on", fact.approved_on == plan.approved_on),
                (
                    "approval_reference",
                    fact.approval_reference == plan.approval_reference,
                ),
                (
                    "price_per_share",
                    fact.price_per_share == plan.price_per_share,
                ),
                (
                    "settled_on",
                    fact.settled_on == book.settlement().map(|receipt| receipt.settlement_on),
                ),
            ],
        )?;
        for occurred in [
            Some(plan.approved_on),
            (!matches!(
                book.status(),
                crate::company::rights_offering::RightsOfferingStatus::Approved
            ))
            .then_some(plan.announced_on),
            book.registration().map(|snapshot| snapshot.registered_on()),
            book.settlement().map(|receipt| receipt.settlement_on),
            book.credited_on(),
            matches!(
                book.status(),
                crate::company::rights_offering::RightsOfferingStatus::Closed
                    | crate::company::rights_offering::RightsOfferingStatus::Settled
            )
            .then_some(plan.payment_deadline_on),
        ]
        .into_iter()
        .flatten()
        .chain(
            book.subscriptions()
                .iter()
                .map(|subscription| subscription.submitted_on),
        ) {
            if occurred > input.committed_through {
                return Err(CompanyCapitalRestoreError::FutureFact {
                    context: CapitalRestoreContext {
                        day: Some(occurred),
                        ..context.clone()
                    },
                    committed_through: input.committed_through,
                });
            }
        }
        if let Some(snapshot) = book.registration() {
            let registry = input
                .actions
                .registries
                .iter()
                .find(|registry| {
                    registry.issuer() == &plan.issuer && registry.stock() == &plan.stock
                })
                .ok_or_else(|| CompanyCapitalRestoreError::MissingRegistry {
                    context: context.clone(),
                })?;
            if registry.registration(snapshot.event_id()) != Some(snapshot) {
                return Err(CompanyCapitalRestoreError::ActionMismatch {
                    context,
                    field: "registration",
                });
            }
            bindings.snapshots.push(FrozenCapitalRegistration {
                context: context.clone(),
                settled_receipts: snapshot.settled_receipts(),
                issued_shares: snapshot.issued_shares(),
            });
        }
        if let Some(receipt) = book.settlement() {
            let expected_shares = if receipt.failed {
                0
            } else {
                receipt.total_paid_shares
            };
            let proceeds = if receipt.failed {
                AccountingAmount::ZERO
            } else {
                AccountingAmount::from_money(receipt.total_paid_amount)
            };
            let increase_cents = i128::from(fact.par_value_per_share.cents())
                .checked_mul(i128::from(expected_shares))
                .ok_or_else(|| CompanyCapitalRestoreError::Arithmetic {
                    context: context.clone(),
                })?;
            let increase = AccountingAmount::from_cents(increase_cents);
            if fact.settled_on != Some(receipt.settlement_on)
                || fact.issued_shares != expected_shares
                || fact.proceeds != Some(proceeds)
                || fact.capital_increase != Some(increase)
            {
                return Err(CompanyCapitalRestoreError::ActionMismatch {
                    context,
                    field: "settlement-finance-fact",
                });
            }
            if expected_shares > 0 {
                let day = receipt.settlement_on;
                if bindings.movements.values().any(|movement| {
                    movement.context.company == Some(plan.issuer)
                        && movement.context.kind == Some(CorporateCapitalKind::ShareReDenomination)
                        && movement.day == day
                }) {
                    return Err(CompanyCapitalRestoreError::Prerequisite {
                        context,
                        required: CapitalRestorePrerequisite::SameDayShareUnits,
                    });
                }
                let lots = receipt
                    .holders
                    .iter()
                    .filter(|holder| holder.paid_shares > 0)
                    .map(|holder| HolderCreditLot {
                        holder: holder.holder.clone(),
                        qty: holder.paid_shares,
                        restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
                    })
                    .collect();
                let movement = ExpectedCapitalMovement {
                    context: CapitalRestoreContext {
                        day: Some(day),
                        ..context.clone()
                    },
                    day,
                    scope: MovementScope::NonTradingTransfer {
                        basis: plan.approval_reference.clone(),
                    },
                    allocation: ExpectedCapitalAllocation::PositiveLots(lots),
                    capital_delta: increase,
                    consumed: false,
                };
                let id =
                    capital_receipt_event_id(CorporateCapitalKind::RightsOffering, &plan.event_id);
                if bindings
                    .movements
                    .insert((plan.issuer, id), movement)
                    .is_some()
                {
                    return Err(CompanyCapitalRestoreError::Duplicate { context });
                }
            }
        }
    }
    if let Some(((company, event), _)) = facts.first_key_value() {
        let stock = input
            .system
            .issuers()
            .get(company)
            .and_then(|spec| spec.listed_stock.as_ref())
            .and_then(|code| input.catalog.id(&code.0));
        return Err(CompanyCapitalRestoreError::MissingBook {
            context: CapitalRestoreContext {
                company: Some(*company),
                stock,
                kind: Some(CorporateCapitalKind::RightsOffering),
                event_id: Some(event.clone()),
                ..Default::default()
            },
        });
    }
    input
        .system
        .validate_rights_offering_books(&input.actions.rights_offerings)
        .map_err(|source| CompanyCapitalRestoreError::System {
            context: CapitalRestoreContext {
                kind: Some(CorporateCapitalKind::RightsOffering),
                ..Default::default()
            },
            source,
        })?;
    Ok(())
}

fn prepare_repurchase_cancellations(
    input: &CapitalRestoreInput<'_>,
    setup_exchanges: &BTreeMap<StockId, CalendarExchange>,
    bindings: &mut CapitalBindings,
) -> Result<(), CompanyCapitalRestoreError> {
    let mut facts = BTreeMap::new();
    for (company, spec) in input.system.issuers().iter() {
        let context = CapitalRestoreContext {
            company: Some(*company),
            stock: spec
                .listed_stock
                .as_ref()
                .and_then(|code| input.catalog.id(&code.0)),
            kind: Some(CorporateCapitalKind::IssuerRepurchaseCancellation),
            ..Default::default()
        };
        for fact in input
            .system
            .issuer_repurchase_facts(company)
            .map_err(|source| CompanyCapitalRestoreError::System {
                context: context.clone(),
                source,
            })?
        {
            if facts
                .insert((*company, fact.event_id.clone()), fact)
                .is_some()
            {
                return Err(CompanyCapitalRestoreError::Duplicate { context });
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for book in &input.actions.issuer_repurchases {
        let plan = book.plan();
        let context = CapitalRestoreContext {
            company: Some(plan.issuer),
            stock: input.catalog.id(&plan.stock.0),
            kind: Some(CorporateCapitalKind::IssuerRepurchaseCancellation),
            event_id: Some(plan.event_id.clone()),
            day: Some(plan.approved_on),
            ..Default::default()
        };
        validate_plan_security(input, setup_exchanges, &context, &plan.stock, plan.exchange)?;
        if !seen.insert((plan.issuer, plan.event_id.clone())) {
            return Err(CompanyCapitalRestoreError::Duplicate { context });
        }
        book.validate_with_calendar(input.calendar)
            .map_err(|source| CompanyCapitalRestoreError::IssuerRepurchase {
                context: context.clone(),
                source,
            })?;
        let fact = facts
            .remove(&(plan.issuer, plan.event_id.clone()))
            .ok_or_else(|| CompanyCapitalRestoreError::MissingFinanceFact {
                context: context.clone(),
            })?;
        check_action_fields(
            &context,
            &[
                ("approved_on", fact.approved_on == plan.approved_on),
                (
                    "approval_reference",
                    fact.approval_reference == plan.approval_reference,
                ),
                (
                    "synthetic_funding",
                    fact.synthetic_funding == AccountingAmount::from_money(plan.total_budget),
                ),
                ("purpose", fact.purpose == plan.purpose),
                ("completed_on", fact.completed_on == book.completed_on()),
                ("cancelled_on", fact.cancelled_on == book.cancelled_on()),
                (
                    "cancelled_shares",
                    fact.cancelled_shares == book.cancelled_shares(),
                ),
            ],
        )?;
        let spent = book
            .completed_on()
            .map(|_| book.total_spent())
            .transpose()
            .map_err(|source| CompanyCapitalRestoreError::IssuerRepurchase {
                context: context.clone(),
                source,
            })?
            .map(AccountingAmount::from_money);
        check_action_fields(
            &context,
            &[
                ("spent", fact.spent == spent),
                (
                    "withdrawn_remainder",
                    fact.withdrawn_remainder
                        == book.withdrawn_remainder().map(AccountingAmount::from_money),
                ),
            ],
        )?;
        for occurred in [
            Some(plan.approved_on),
            (!matches!(
                book.status(),
                crate::company::issuer_repurchase::IssuerRepurchaseStatus::Approved
            ))
            .then_some(plan.announced_on),
            book.completed_on(),
            book.cancelled_on(),
        ]
        .into_iter()
        .flatten()
        .chain(book.fills().iter().map(|fill| fill.day))
        {
            if occurred > input.committed_through {
                return Err(CompanyCapitalRestoreError::FutureFact {
                    context: CapitalRestoreContext {
                        day: Some(occurred),
                        ..context.clone()
                    },
                    committed_through: input.committed_through,
                });
            }
        }
        if input
            .actions
            .issuer_repurchases
            .iter()
            .filter(|other| other.plan().issuer == plan.issuer)
            .count()
            != 1
            || plan.approval_unit_event_id.is_some()
            || !book.share_unit_adjustments().is_empty()
            || input
                .actions
                .share_splits
                .iter()
                .any(|other| other.plan().issuer == plan.issuer && other.settled_on().is_some())
        {
            return Err(CompanyCapitalRestoreError::Prerequisite {
                context,
                required: CapitalRestorePrerequisite::RepurchaseLotAttribution,
            });
        }
        book.validate_share_units(&[], input.committed_through)
            .map_err(|source| CompanyCapitalRestoreError::IssuerRepurchase {
                context: context.clone(),
                source,
            })?;
        let mut bought_lots = BTreeMap::new();
        if !book.fills().is_empty() {
            let registry = input
                .actions
                .registries
                .iter()
                .find(|registry| {
                    registry.issuer() == &plan.issuer && registry.stock() == &plan.stock
                })
                .ok_or_else(|| CompanyCapitalRestoreError::MissingRegistry {
                    context: context.clone(),
                })?;
            for fill in book.fills() {
                let matching = registry
                    .receipts()
                    .iter()
                    .filter(|receipt| {
                        receipt.request.day == fill.day
                            && matches!(receipt.request.scope, MovementScope::PublicMarket)
                    })
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    return Err(CompanyCapitalRestoreError::ActionMismatch {
                        context: context.clone(),
                        field: "repurchase-market-receipt",
                    });
                }
                let receipt = matching[0];
                let treasury = receipt
                    .request
                    .changes
                    .iter()
                    .filter(|change| change.holder == HolderId::IssuerTreasury)
                    .collect::<Vec<_>>();
                if treasury.len() != 1 || treasury[0].change != i128::from(fill.shares) {
                    return Err(CompanyCapitalRestoreError::ActionMismatch {
                        context: context.clone(),
                        field: "repurchase-market-shares",
                    });
                }
                let acquisition = treasury[0].acquisition.as_ref().ok_or_else(|| {
                    CompanyCapitalRestoreError::ActionMismatch {
                        context: context.clone(),
                        field: "repurchase-lot-source",
                    }
                })?;
                if acquisition.source
                    != (crate::company::share_registry::AcquisitionSource::SecondaryMarket {
                        settlement: receipt.request.event_id.clone(),
                    })
                {
                    return Err(CompanyCapitalRestoreError::ActionMismatch {
                        context: context.clone(),
                        field: "repurchase-lot-source",
                    });
                }
                if bought_lots
                    .insert(
                        acquisition.lot_id.clone(),
                        BoughtRepurchaseLot {
                            acquired_on: fill.day,
                            qty: fill.shares,
                            source: acquisition.source.clone(),
                            restriction: acquisition.restriction.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(CompanyCapitalRestoreError::Duplicate { context });
                }
            }
        }
        if let Some(day) = book.cancelled_on() {
            let reduction = fact.capital_reduction.ok_or_else(|| {
                CompanyCapitalRestoreError::ActionMismatch {
                    context: context.clone(),
                    field: "capital-reduction",
                }
            })?;
            if fact.cancelled_on != Some(day)
                || fact.cancelled_shares != book.cancelled_shares()
                || !reduction.is_positive()
            {
                return Err(CompanyCapitalRestoreError::ActionMismatch {
                    context,
                    field: "cancellation-finance-fact",
                });
            }
            let capital_delta = reduction.cents().checked_neg().ok_or_else(|| {
                CompanyCapitalRestoreError::Arithmetic {
                    context: context.clone(),
                }
            })?;
            let movement = ExpectedCapitalMovement {
                context: CapitalRestoreContext {
                    day: Some(day),
                    ..context.clone()
                },
                day,
                scope: MovementScope::IssuerRepurchaseCancellation {
                    basis: plan.approval_reference.clone(),
                },
                allocation: ExpectedCapitalAllocation::RepurchaseCancellation {
                    bought_lots,
                    cancelled_shares: book.cancelled_shares(),
                },
                capital_delta: AccountingAmount::from_cents(capital_delta),
                consumed: false,
            };
            let id = capital_receipt_event_id(
                CorporateCapitalKind::IssuerRepurchaseCancellation,
                &plan.event_id,
            );
            if bindings
                .movements
                .insert((plan.issuer, id), movement)
                .is_some()
            {
                return Err(CompanyCapitalRestoreError::Duplicate { context });
            }
        }
    }
    if let Some(((company, event), _)) = facts.first_key_value() {
        let stock = input
            .system
            .issuers()
            .get(company)
            .and_then(|spec| spec.listed_stock.as_ref())
            .and_then(|code| input.catalog.id(&code.0));
        return Err(CompanyCapitalRestoreError::MissingBook {
            context: CapitalRestoreContext {
                company: Some(*company),
                stock,
                kind: Some(CorporateCapitalKind::IssuerRepurchaseCancellation),
                event_id: Some(event.clone()),
                ..Default::default()
            },
        });
    }
    input
        .system
        .validate_issuer_repurchase_books(&input.actions.issuer_repurchases)
        .map_err(|source| CompanyCapitalRestoreError::System {
            context: CapitalRestoreContext {
                kind: Some(CorporateCapitalKind::IssuerRepurchaseCancellation),
                ..Default::default()
            },
            source,
        })?;
    Ok(())
}

fn validate_cancellation_lots(
    receipt: &ShareDayReceipt,
    bought_lots: &BTreeMap<String, BoughtRepurchaseLot>,
    cancelled_shares: u64,
    context: &CapitalRestoreContext,
) -> Result<(), CompanyCapitalRestoreError> {
    if receipt.request.changes.len() != 1 {
        return Err(CompanyCapitalRestoreError::ActionMismatch {
            context: context.clone(),
            field: "cancellation-holder",
        });
    }
    let change = &receipt.request.changes[0];
    if change.holder != HolderId::IssuerTreasury
        || change.change != -i128::from(cancelled_shares)
        || change.acquisition.is_some()
    {
        return Err(CompanyCapitalRestoreError::ActionMismatch {
            context: context.clone(),
            field: "cancellation-holder",
        });
    }
    let consume =
        change
            .consume
            .as_ref()
            .ok_or_else(|| CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "consume",
            })?;
    let mut total = 0_u64;
    for selection in consume {
        let bought = bought_lots.get(&selection.lot_id).ok_or_else(|| {
            CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "repurchase-lot-source",
            }
        })?;
        let disposed = receipt
            .disposals
            .iter()
            .find(|disposed| {
                disposed.holder == HolderId::IssuerTreasury && disposed.lot.id == selection.lot_id
            })
            .ok_or_else(|| CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "repurchase-lot-source",
            })?;
        if selection.qty == 0
            || selection.qty > bought.qty
            || disposed.lot.acquired_on != bought.acquired_on
            || disposed.lot.qty != selection.qty
            || disposed.lot.source != bought.source
            || disposed.lot.restriction != bought.restriction
        {
            return Err(CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "repurchase-lot-source",
            });
        }
        total = total.checked_add(selection.qty).ok_or_else(|| {
            CompanyCapitalRestoreError::Arithmetic {
                context: context.clone(),
            }
        })?;
    }
    check_shares(cancelled_shares, total, context.clone())?;
    Ok(())
}

fn validate_redenomination_holders(
    receipt: &ShareDayReceipt,
    frozen: &ShareSplitReceipt,
    context: &CapitalRestoreContext,
) -> Result<(), CompanyCapitalRestoreError> {
    let expected = frozen
        .holders
        .iter()
        .filter(|holder| holder.original_shares != holder.new_shares)
        .map(|holder| (&holder.holder, holder))
        .collect::<BTreeMap<_, _>>();
    if expected.len() != receipt.request.changes.len() {
        return Err(CompanyCapitalRestoreError::ActionMismatch {
            context: context.clone(),
            field: "holders",
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for change in &receipt.request.changes {
        let holder = expected.get(&change.holder).ok_or_else(|| {
            CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "holders",
            }
        })?;
        let delta = i128::from(holder.new_shares) - i128::from(holder.original_shares);
        if !seen.insert(&change.holder) || change.change != delta {
            return Err(CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "holder-allocation",
            });
        }
        if delta > 0 {
            let acquisition = change.acquisition.as_ref().ok_or_else(|| {
                CompanyCapitalRestoreError::ActionMismatch {
                    context: context.clone(),
                    field: "acquisition",
                }
            })?;
            let restriction =
                holder_inherited_restriction(holder, receipt.request.day).map_err(|source| {
                    CompanyCapitalRestoreError::ShareSplit {
                        context: context.clone(),
                        source,
                    }
                })?;
            if change.consume.is_some()
                || acquisition.restriction != restriction
                || acquisition.source
                    != (crate::company::share_registry::AcquisitionSource::CorporateAction {
                        event: frozen.event_id.clone(),
                    })
            {
                return Err(CompanyCapitalRestoreError::ActionMismatch {
                    context: context.clone(),
                    field: "acquisition",
                });
            }
        } else {
            let mut remaining =
                u64::try_from(-delta).map_err(|_| CompanyCapitalRestoreError::Arithmetic {
                    context: context.clone(),
                })?;
            let mut selection = Vec::new();
            for lot in &holder.original_lots {
                let qty = remaining.min(lot.qty);
                if qty > 0 {
                    selection.push(crate::company::share_registry::LotConsumption {
                        lot_id: lot.id.clone(),
                        qty,
                    });
                    remaining -= qty;
                }
            }
            if remaining != 0
                || change.acquisition.is_some()
                || change.consume.as_ref() != Some(&selection)
            {
                return Err(CompanyCapitalRestoreError::ActionMismatch {
                    context: context.clone(),
                    field: "consume",
                });
            }
        }
    }
    Ok(())
}

fn validate_bound_receipt(
    receipt: &ShareDayReceipt,
    movement: &mut ExpectedCapitalMovement,
    receipt_index: usize,
    issued_before: u64,
    issued_after: u64,
    capital_before: Option<AccountingAmount>,
) -> Result<(), CompanyCapitalRestoreError> {
    let context = CapitalRestoreContext {
        receipt_index: Some(receipt_index),
        ..movement.context.clone()
    };
    if movement.consumed {
        return Err(CompanyCapitalRestoreError::Duplicate { context });
    }
    if receipt.request.day != movement.day || receipt.request.scope != movement.scope {
        return Err(CompanyCapitalRestoreError::ActionMismatch {
            context,
            field: "receipt-scope-or-day",
        });
    }
    match &movement.allocation {
        ExpectedCapitalAllocation::PositiveLots(lots) => {
            validate_positive_lots(receipt, lots, &context)?
        }
        ExpectedCapitalAllocation::ReDenomination(frozen) => {
            check_shares(frozen.issued_shares_before, issued_before, context.clone())?;
            check_shares(frozen.issued_shares_after, issued_after, context.clone())?;
            validate_redenomination_holders(receipt, frozen, &context)?;
        }
        ExpectedCapitalAllocation::RepurchaseCancellation {
            bought_lots,
            cancelled_shares,
        } => {
            validate_cancellation_lots(receipt, bought_lots, *cancelled_shares, &context)?;
            let capital =
                capital_before.ok_or_else(|| CompanyCapitalRestoreError::ActionMismatch {
                    context: context.clone(),
                    field: "registered-capital",
                })?;
            let divisor = i128::from(issued_before);
            if divisor == 0 || capital.cents() % divisor != 0 {
                return Err(CompanyCapitalRestoreError::Arithmetic { context });
            }
            let expected = (capital.cents() / divisor)
                .checked_mul(i128::from(*cancelled_shares))
                .ok_or_else(|| CompanyCapitalRestoreError::Arithmetic {
                    context: context.clone(),
                })?;
            if expected.checked_neg() != Some(movement.capital_delta.cents()) {
                return Err(CompanyCapitalRestoreError::RegisteredCapital {
                    context,
                    expected: AccountingAmount::from_cents(expected),
                    actual: AccountingAmount::from_cents(-movement.capital_delta.cents()),
                });
            }
        }
    }
    movement.consumed = true;
    Ok(())
}

fn validate_positive_lots(
    receipt: &ShareDayReceipt,
    lots: &[HolderCreditLot],
    context: &CapitalRestoreContext,
) -> Result<(), CompanyCapitalRestoreError> {
    let expected = lots
        .iter()
        .map(|lot| (&lot.holder, lot))
        .collect::<BTreeMap<_, _>>();
    if expected.len() != receipt.request.changes.len() {
        return Err(CompanyCapitalRestoreError::ActionMismatch {
            context: context.clone(),
            field: "holders",
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for change in &receipt.request.changes {
        let lot = expected.get(&change.holder).ok_or_else(|| {
            CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "holders",
            }
        })?;
        let acquisition = change.acquisition.as_ref().ok_or_else(|| {
            CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "acquisition",
            }
        })?;
        if !seen.insert(&change.holder)
            || change.holder == HolderId::IssuerTreasury
            || change.change != i128::from(lot.qty)
            || change.consume.is_some()
            || acquisition.restriction != lot.restriction
            || acquisition.source
                != (crate::company::share_registry::AcquisitionSource::CorporateAction {
                    event: context.event_id.clone().ok_or_else(|| {
                        CompanyCapitalRestoreError::ActionMismatch {
                            context: context.clone(),
                            field: "event-id",
                        }
                    })?,
                })
        {
            return Err(CompanyCapitalRestoreError::ActionMismatch {
                context: context.clone(),
                field: "holder-allocation",
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "company_capital_restore_tests.rs"]
mod tests;
