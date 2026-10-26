//! selected CompanySystem 的已提交资本恢复验证；不创建或修改持久事实。

use crate::Money;
use crate::accounting::AccountingAmount;
use crate::calendar::{CivilDate, TradingCalendar};
use crate::company::identity::IssuerRegistry;
use crate::company::{CompanyId, CompanySystem, CompanySystemError};
use crate::session::SessionCorporateActions;
use crate::stock::{StockCatalog, StockId};
use std::collections::BTreeMap;

/// 日期校验借用本局冻结 policy，不在恢复时另建默认日历。
pub(crate) struct CapitalRestoreInput<'a> {
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
    #[error("资本恢复验证尚未实现")]
    NotImplemented,
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
    #[error("资本恢复算术不能表示合法股本：{context:?}")]
    Arithmetic { context: CapitalRestoreContext },
    #[error("资本恢复所需动作验证尚未实现：{context:?}")]
    UnsupportedMovement { context: CapitalRestoreContext },
}

/// 验证结果是恢复过程的派生结论，不进入 SaveSlot。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompanyCapitalReplay {
    pub(crate) stock: Option<StockId>,
    pub(crate) issued_shares: u64,
    pub(crate) receipt_frontier: usize,
    pub(crate) registered_capital: Option<AccountingAmount>,
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
    // 本子片尚未实现股份动作绑定；任何正式动作都必须明确拒绝，不能忽略后验过关。
    reject_pending_capital_implementation(&input)?;
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
        let mut issued_shares = initial.issued_shares;
        let mut receipt_frontier = 0;
        if let Some(registry) = registries.get(company) {
            for (receipt_index, receipt) in registry.receipts().iter().enumerate() {
                let receipt_context = CapitalRestoreContext {
                    event_id: Some(receipt.request.event_id.clone()),
                    receipt_index: Some(receipt_index),
                    day: Some(receipt.request.day),
                    kind: matches!(
                        receipt.request.scope,
                        crate::company::share_registry::MovementScope::PublicMarket
                    )
                    .then_some(CorporateCapitalKind::PublicMarket),
                    ..context.clone()
                };
                if receipt.request.day > input.committed_through {
                    return Err(CompanyCapitalRestoreError::FutureFact {
                        context: receipt_context,
                        committed_through: input.committed_through,
                    });
                }
                if !matches!(
                    receipt.request.scope,
                    crate::company::share_registry::MovementScope::PublicMarket
                ) {
                    return Err(CompanyCapitalRestoreError::UnboundReceipt {
                        context: receipt_context,
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
                if after != issued_shares {
                    return Err(CompanyCapitalRestoreError::IssuedShares {
                        context: receipt_context,
                        expected: issued_shares,
                        actual: after,
                    });
                }
                issued_shares = after;
                receipt_frontier += 1;
            }
            check_shares(issued_shares, registry.issued_shares(), context.clone())?;
        }
        check_shares(issued_shares, current.issued_shares, context.clone())?;
        let registered_capital = input
            .system
            .dividend_legal_facts(company)
            .map_err(|source| CompanyCapitalRestoreError::System {
                context: context.clone(),
                source,
            })?
            .map(|facts| facts.registered_capital);
        companies.insert(
            *company,
            CompanyCapitalReplay {
                stock,
                issued_shares,
                receipt_frontier,
                registered_capital,
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

fn reject_pending_capital_implementation(
    input: &CapitalRestoreInput<'_>,
) -> Result<(), CompanyCapitalRestoreError> {
    let first = input
        .actions
        .stock_distributions
        .first()
        .map(|book| {
            (
                book.plan().issuer,
                &book.plan().event_id,
                &book.plan().stock,
                CorporateCapitalKind::StockDistribution,
            )
        })
        .or_else(|| {
            input.actions.rights_offerings.first().map(|book| {
                (
                    book.plan().issuer,
                    &book.plan().event_id,
                    &book.plan().stock,
                    CorporateCapitalKind::RightsOffering,
                )
            })
        })
        .or_else(|| {
            input.actions.share_splits.first().map(|book| {
                (
                    book.plan().issuer,
                    &book.plan().event_id,
                    &book.plan().stock,
                    CorporateCapitalKind::ShareReDenomination,
                )
            })
        })
        .or_else(|| {
            input.actions.issuer_repurchases.first().map(|book| {
                (
                    book.plan().issuer,
                    &book.plan().event_id,
                    &book.plan().stock,
                    CorporateCapitalKind::IssuerRepurchaseCancellation,
                )
            })
        });
    if let Some((company, event, stock, kind)) = first {
        return Err(CompanyCapitalRestoreError::UnsupportedMovement {
            context: CapitalRestoreContext {
                company: Some(company),
                stock: input.catalog.id(&stock.0),
                kind: Some(kind),
                event_id: Some(event.clone()),
                ..Default::default()
            },
        });
    }
    // 逐公司读取全部正式事实；读取错误必须传播，不能仅凭四个 book 数组为空放行。
    for (company, _) in input.system.issuers().iter() {
        let facts = [
            input.system.stock_distribution_facts(company).map(|facts| {
                facts
                    .into_iter()
                    .map(|fact| fact.event_id)
                    .collect::<Vec<_>>()
            }),
            input
                .system
                .rights_offering_facts(company)
                .map(|facts| facts.into_iter().map(|fact| fact.event_id).collect()),
            input
                .system
                .share_split_facts(company)
                .map(|facts| facts.into_iter().map(|fact| fact.event_id).collect()),
            input
                .system
                .issuer_repurchase_facts(company)
                .map(|facts| facts.into_iter().map(|fact| fact.event_id).collect()),
        ];
        for (kind, events) in [
            CorporateCapitalKind::StockDistribution,
            CorporateCapitalKind::RightsOffering,
            CorporateCapitalKind::ShareReDenomination,
            CorporateCapitalKind::IssuerRepurchaseCancellation,
        ]
        .into_iter()
        .zip(facts)
        {
            let events = events.map_err(|source| CompanyCapitalRestoreError::System {
                context: CapitalRestoreContext {
                    company: Some(*company),
                    kind: Some(kind),
                    ..Default::default()
                },
                source,
            })?;
            if let Some(event) = events.first() {
                return Err(CompanyCapitalRestoreError::UnsupportedMovement {
                    context: CapitalRestoreContext {
                        company: Some(*company),
                        kind: Some(kind),
                        event_id: Some(event.clone()),
                        ..Default::default()
                    },
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "company_capital_restore_tests.rs"]
mod tests;
