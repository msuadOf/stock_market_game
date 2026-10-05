use super::{CompanyId, CompanySpec, CompanySystemError};
use crate::account::StockCode;
use std::collections::BTreeMap;

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct IssuerRegistry(
    #[serde(deserialize_with = "super::persistence::unique_map")] BTreeMap<CompanyId, CompanySpec>,
);

impl IssuerRegistry {
    pub fn new(specs: Vec<CompanySpec>) -> Result<Self, CompanySystemError> {
        CompanySpec::validate_set(&specs)
            .map_err(|error| CompanySystemError::Invalid(error.to_string()))?;
        Ok(Self(
            specs
                .into_iter()
                .map(|spec| (spec.id.clone(), spec))
                .collect(),
        ))
    }

    pub fn get(&self, company: &CompanyId) -> Option<&CompanySpec> {
        self.0.get(company)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&CompanyId, &CompanySpec)> {
        self.0.iter()
    }
    pub fn issuer_of(&self, stock: &StockCode) -> Option<&CompanyId> {
        self.0
            .values()
            .find(|spec| spec.listed_stock.as_ref() == Some(stock))
            .map(|spec| &spec.id)
    }
    pub fn validate_issuer_mapping(
        &self,
        stocks: &[(StockCode, u64)],
    ) -> Result<(), CompanySystemError> {
        for spec in self.0.values() {
            if let Some(stock) = &spec.listed_stock {
                if !stocks
                    .iter()
                    .any(|(code, shares)| code == stock && *shares == spec.issued_shares)
                {
                    return Err(CompanySystemError::Invalid(format!(
                        "发行人 {} 的股票及总股本不匹配",
                        spec.id.0
                    )));
                }
            }
        }
        if stocks
            .iter()
            .any(|(code, _)| self.issuer_of(code).is_none())
        {
            return Err(CompanySystemError::Invalid("存在没有发行人的股票".into()));
        }
        Ok(())
    }
}
