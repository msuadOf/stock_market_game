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

    /// 记录一次真实公司行为新股入账带来的已发行股数增加。
    ///
    /// 调用方必须已按事件身份幂等去重（同一事件只允许调用一次）；本登记表不
    /// 追踪事件身份，重复调用会重复增加股数并破坏守恒。
    pub fn record_share_issuance(
        &mut self,
        company: &CompanyId,
        added: u64,
    ) -> Result<(), CompanySystemError> {
        if added == 0 {
            return Err(CompanySystemError::Invalid(
                "公司行为入账新增股数必须为正数".into(),
            ));
        }
        let spec = self
            .0
            .get_mut(company)
            .ok_or_else(|| CompanySystemError::Invalid(format!("未知公司 {}", company.0)))?;
        spec.issued_shares = spec
            .issued_shares
            .checked_add(added)
            .ok_or_else(|| CompanySystemError::Invalid("已发行股数溢出".into()))?;
        Ok(())
    }
    /// 记录一次回购注销带来的已发行股数减少（守恒：不得减至非正数）。
    ///
    /// 调用方必须已按事件身份幂等去重；本登记表不追踪事件身份。
    pub fn record_share_cancellation(
        &mut self,
        company: &CompanyId,
        cancelled: u64,
    ) -> Result<(), CompanySystemError> {
        if cancelled == 0 {
            return Err(CompanySystemError::Invalid(
                "回购注销股数必须为正数".into(),
            ));
        }
        let spec = self
            .0
            .get_mut(company)
            .ok_or_else(|| CompanySystemError::Invalid(format!("未知公司 {}", company.0)))?;
        if spec.issued_shares <= cancelled {
            return Err(CompanySystemError::Invalid(
                "回购注销后已发行股数必须保持正数".into(),
            ));
        }
        spec.issued_shares -= cancelled;
        Ok(())
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
