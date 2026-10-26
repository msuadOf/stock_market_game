//! 公司共同只读事实：不可变输入，不暴露所选模型的私有财务。
use super::{CompanyId, CompanyKind};
use crate::StockId;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyReadFacts {
    issuers: BTreeMap<StockId, CompanyId>,
    specs: BTreeMap<CompanyId, (CompanyKind, u64)>,
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CompanyReadFactsError {
    #[error("缺少发行人：{stock:?}")]
    MissingIssuer { stock: StockId },
    #[error("缺少公司共同规格：{company:?}")]
    MissingCompany { company: CompanyId },
    #[error("已发行普通股总股数必须为正：{company:?}")]
    ZeroIssuedShares { company: CompanyId },
}
impl CompanyReadFacts {
    pub fn new(
        issuers: BTreeMap<StockId, CompanyId>,
        specs: BTreeMap<CompanyId, (CompanyKind, u64)>,
    ) -> Result<Self, CompanyReadFactsError> {
        for &company in issuers.values() {
            if !specs.contains_key(&company) {
                return Err(CompanyReadFactsError::MissingCompany { company });
            }
        }
        for (&company, &(_, shares)) in &specs {
            if shares == 0 {
                return Err(CompanyReadFactsError::ZeroIssuedShares { company });
            }
        }
        Ok(Self { issuers, specs })
    }
    pub fn issuers(&self) -> impl Iterator<Item = (&StockId, &CompanyId)> {
        self.issuers.iter()
    }
    pub fn company_specs(&self) -> impl Iterator<Item = (&CompanyId, &(CompanyKind, u64))> {
        self.specs.iter()
    }
    pub fn issuer_of_stock(&self, stock: StockId) -> Result<CompanyId, CompanyReadFactsError> {
        self.issuers
            .get(&stock)
            .copied()
            .ok_or(CompanyReadFactsError::MissingIssuer { stock })
    }
    pub fn company_spec(
        &self,
        company: &CompanyId,
    ) -> Result<(CompanyKind, u64), CompanyReadFactsError> {
        self.specs
            .get(company)
            .copied()
            .ok_or(CompanyReadFactsError::MissingCompany { company: *company })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn info_facts_validate_missing_spec_zero_denominator_and_allow_unlisted() {
        assert_eq!(
            CompanyReadFacts::new([(StockId(0), CompanyId(3))].into(), BTreeMap::new()),
            Err(CompanyReadFactsError::MissingCompany {
                company: CompanyId(3)
            })
        );
        assert_eq!(
            CompanyReadFacts::new(
                BTreeMap::new(),
                [(CompanyId(3), (CompanyKind::Bank, 0))].into()
            ),
            Err(CompanyReadFactsError::ZeroIssuedShares {
                company: CompanyId(3)
            })
        );
        let facts = CompanyReadFacts::new(
            BTreeMap::new(),
            [(CompanyId(3), (CompanyKind::Bank, 100))].into(),
        )
        .unwrap();
        assert_eq!(
            facts.company_spec(&CompanyId(3)).unwrap(),
            (CompanyKind::Bank, 100)
        );
        assert_eq!(
            facts.issuer_of_stock(StockId(0)),
            Err(CompanyReadFactsError::MissingIssuer { stock: StockId(0) })
        );
    }
}
