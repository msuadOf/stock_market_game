//! 将选定公司模型的公开身份和股本投影为共同只读事实。

use super::CommittableSessionState;
use crate::company::{CompanyId, CompanyReadFacts, CompanyReadFactsError, CompanySpec};
use crate::{StockCatalog, StockId};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum CompanyFactsSourceError {
    #[error("公司公开事实错误：{0}")]
    Facts(#[from] CompanyReadFactsError),
    #[error("公司身份重复：{company:?}")]
    DuplicateCompany { company: CompanyId },
    #[error("股票发行人重复：{stock:?}，原发行人 {previous:?}，重复发行人 {company:?}")]
    DuplicateIssuer {
        stock: StockId,
        previous: CompanyId,
        company: CompanyId,
    },
    #[error("公司 {company:?} 的上市股票未进入 StockCatalog")]
    UnknownListedStock { company: CompanyId },
    #[error("所选 CompanySystem 缺失，不能使用 legacy 公司事实替代")]
    MissingSelectedSystem,
    #[error("legacy 会话包含未选定的 CompanySystem")]
    UnexpectedSystem,
    #[error("legacy 公司 {company:?} 缺少 registry 身份")]
    MissingLegacyRegistry { company: CompanyId },
    #[error("legacy 公司 {company:?} 缺少 operations 规格")]
    MissingLegacyOperation { company: CompanyId },
    #[error("legacy 公司 {company:?} 的 registry 与 operations 规格不一致")]
    LegacySpecMismatch { company: CompanyId },
}

impl CommittableSessionState {
    pub(super) fn company_read_facts(&self) -> Result<CompanyReadFacts, CompanyFactsSourceError> {
        project_specs(&self.stocks, self.selected_company_specs()?)
    }

    /// 唯一选源边界；借用遍历不为单股查询重新分配全市场快照。
    fn selected_company_specs(
        &self,
    ) -> Result<impl Iterator<Item = (CompanyId, &CompanySpec)>, CompanyFactsSourceError> {
        let (issuers, registry) = match (&self.setup.company_system, &self.company_system) {
            (Some(_), Some(system)) => (Some(system.issuers()), None),
            (Some(_), None) => return Err(CompanyFactsSourceError::MissingSelectedSystem),
            (None, Some(_)) => return Err(CompanyFactsSourceError::UnexpectedSystem),
            (None, None) => {
                for (company, entity) in self.company_registry.iter() {
                    let operating = self.operations.company(company).ok_or(
                        CompanyFactsSourceError::MissingLegacyOperation { company: *company },
                    )?;
                    if entity.spec() != operating.spec() {
                        return Err(CompanyFactsSourceError::LegacySpecMismatch {
                            company: *company,
                        });
                    }
                }
                for company in self.operations.companies.keys() {
                    if self.company_registry.get(company).is_none() {
                        return Err(CompanyFactsSourceError::MissingLegacyRegistry {
                            company: *company,
                        });
                    }
                }
                (None, Some(self.company_registry.as_ref()))
            }
        };
        Ok(issuers.into_iter().flat_map(|issuers| {
            issuers.iter().map(|(company, spec)| (*company, spec))
        }).chain(registry.into_iter().flat_map(|registry| {
            registry.iter().map(|(company, entity)| (*company, entity.spec()))
        })))
    }

    /// 单股身份与股本读取共享选源和勾稽；完整映射校验仍由快照入口负责。
    pub(super) fn company_spec_of_stock(
        &self,
        stock: StockId,
    ) -> Result<(CompanyId, &CompanySpec), CompanyFactsSourceError> {
        let code = self.stocks.code(stock).ok_or(
            CompanyReadFactsError::MissingIssuer { stock },
        )?;
        find_stock_spec(stock, code, self.selected_company_specs()?)
    }
}

fn find_stock_spec<'a>(
    stock: StockId,
    code: &str,
    specs: impl Iterator<Item = (CompanyId, &'a CompanySpec)>,
) -> Result<(CompanyId, &'a CompanySpec), CompanyFactsSourceError> {
    let mut found: Option<(CompanyId, &CompanySpec)> = None;
    for (company, spec) in specs {
        if spec.listed_stock.as_ref().is_some_and(|listed| listed.0 == code) {
            if let Some((previous, _)) = found {
                return Err(CompanyFactsSourceError::DuplicateIssuer { stock, previous, company });
            }
            if spec.issued_shares == 0 {
                return Err(CompanyReadFactsError::ZeroIssuedShares { company }.into());
            }
            found = Some((company, spec));
        }
    }
    found.ok_or_else(|| CompanyReadFactsError::MissingIssuer { stock }.into())
}

fn project_specs<'a>(
    stocks: &StockCatalog,
    specs: impl Iterator<Item = (CompanyId, &'a CompanySpec)>,
) -> Result<CompanyReadFacts, CompanyFactsSourceError> {
    let mut issuers = BTreeMap::new();
    let mut companies = BTreeMap::new();
    for (company, spec) in specs {
        if companies.contains_key(&company) {
            return Err(CompanyFactsSourceError::DuplicateCompany { company });
        }
        companies.insert(company, (spec.kind, spec.issued_shares));
        if let Some(code) = &spec.listed_stock {
            let stock = stocks
                .id(&code.0)
                .ok_or(CompanyFactsSourceError::UnknownListedStock { company })?;
            if let Some(previous) = issuers.get(&stock).copied() {
                return Err(CompanyFactsSourceError::DuplicateIssuer {
                    stock,
                    previous,
                    company,
                });
            }
            issuers.insert(stock, company);
        }
    }
    let facts = CompanyReadFacts::new(issuers, companies)?;
    for (stock, _) in stocks.iter() {
        facts.issuer_of_stock(stock)?;
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::StockCode;
    use crate::company::{CompanyKind, IndustryId};

    fn catalog() -> StockCatalog {
        StockCatalog::from_metas(vec![crate::stock::StockMeta {
            code: "600101".to_owned(),
            display_name: "测试股票".to_owned(),
        }])
        .unwrap()
    }

    fn spec(listed: Option<&str>, kind: CompanyKind, issued_shares: u64) -> CompanySpec {
        CompanySpec {
            code: "C-source-fixture".to_owned(),
            name: "测试公司".to_owned(),
            industry: IndustryId("fixture".to_owned()),
            kind,
            listed_stock: listed.map(|code| StockCode(code.to_owned())),
            issued_shares,
            group_parent: None,
        }
    }

    #[test]
    fn selected_company_source_single_stock_lookup_keeps_id_kind_and_u64() {
        let listed = spec(Some("600101"), CompanyKind::Bank, u64::from(u32::MAX) + 701);
        let unlisted = spec(None, CompanyKind::Insurance, 900);
        let (company, actual) = find_stock_spec(
            StockId(0), "600101",
            [(CompanyId(2), &unlisted), (CompanyId(17), &listed)].into_iter(),
        ).unwrap();
        assert_eq!(company, CompanyId(17));
        assert_eq!(actual, &listed);
    }

    #[test]
    fn selected_company_source_single_stock_lookup_checks_later_duplicate() {
        let listed = spec(Some("600101"), CompanyKind::Bank, 700);
        assert_eq!(find_stock_spec(
            StockId(0), "600101",
            [(CompanyId(17), &listed), (CompanyId(22), &listed)].into_iter(),
        ), Err(CompanyFactsSourceError::DuplicateIssuer {
            stock: StockId(0), previous: CompanyId(17), company: CompanyId(22),
        }));
    }

    #[test]
    fn selected_company_source_single_stock_lookup_rejects_zero_shares() {
        let listed = spec(Some("600101"), CompanyKind::Bank, 0);
        assert_eq!(find_stock_spec(
            StockId(0), "600101", [(CompanyId(17), &listed)].into_iter(),
        ), Err(CompanyFactsSourceError::Facts(CompanyReadFactsError::ZeroIssuedShares {
            company: CompanyId(17),
        })));
    }

    #[test]
    fn selected_company_source_single_stock_lookup_rejects_missing_issuer() {
        let unlisted = spec(None, CompanyKind::Insurance, 900);
        assert_eq!(find_stock_spec(
            StockId(0), "600101", [(CompanyId(2), &unlisted)].into_iter(),
        ), Err(CompanyFactsSourceError::Facts(CompanyReadFactsError::MissingIssuer {
            stock: StockId(0),
        })));
    }

    #[test]
    fn selected_company_source_projection_keeps_independent_ids_unlisted_and_u64() {
        let listed = spec(Some("600101"), CompanyKind::Bank, u64::from(u32::MAX) + 701);
        let unlisted = spec(None, CompanyKind::Insurance, 900);
        let facts = project_specs(
            &catalog(),
            [(CompanyId(17), &listed), (CompanyId(2), &unlisted)].into_iter(),
        )
        .unwrap();
        assert_eq!(facts.issuer_of_stock(StockId(0)).unwrap(), CompanyId(17));
        assert_eq!(
            facts.company_spec(&CompanyId(17)).unwrap(),
            (CompanyKind::Bank, u64::from(u32::MAX) + 701)
        );
        assert_eq!(
            facts.company_spec(&CompanyId(2)).unwrap(),
            (CompanyKind::Insurance, 900)
        );
        assert_eq!(facts.issuers().count(), 1);
        assert_eq!(
            facts.issuer_of_stock(StockId(99)),
            Err(CompanyReadFactsError::MissingIssuer { stock: StockId(99) })
        );
    }

    #[test]
    fn selected_company_source_projection_rejects_duplicate_company_before_map_overwrite() {
        let first = spec(Some("600101"), CompanyKind::Bank, 100);
        let second = spec(None, CompanyKind::Insurance, 200);
        assert_eq!(
            project_specs(
                &catalog(),
                [(CompanyId(17), &first), (CompanyId(17), &second)].into_iter()
            ),
            Err(CompanyFactsSourceError::DuplicateCompany {
                company: CompanyId(17)
            })
        );
    }

    #[test]
    fn selected_company_source_projection_rejects_duplicate_issuer_before_map_overwrite() {
        let first = spec(Some("600101"), CompanyKind::Bank, 100);
        let second = spec(Some("600101"), CompanyKind::Insurance, 200);
        assert_eq!(
            project_specs(
                &catalog(),
                [(CompanyId(17), &first), (CompanyId(2), &second)].into_iter()
            ),
            Err(CompanyFactsSourceError::DuplicateIssuer {
                stock: StockId(0),
                previous: CompanyId(17),
                company: CompanyId(2)
            })
        );
    }

    #[test]
    fn selected_company_source_projection_rejects_unknown_listed_stock() {
        let missing = spec(Some("600102"), CompanyKind::Bank, 100);
        assert_eq!(
            project_specs(&catalog(), [(CompanyId(17), &missing)].into_iter()),
            Err(CompanyFactsSourceError::UnknownListedStock {
                company: CompanyId(17)
            })
        );
    }

    #[test]
    fn selected_company_source_projection_rejects_missing_catalog_issuer() {
        let unlisted = spec(None, CompanyKind::Bank, 100);
        assert_eq!(
            project_specs(&catalog(), [(CompanyId(17), &unlisted)].into_iter()),
            Err(CompanyFactsSourceError::Facts(
                CompanyReadFactsError::MissingIssuer { stock: StockId(0) }
            ))
        );
    }
}
