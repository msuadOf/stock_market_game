use super::spec::{CompanyCatalog, CompanyId, CompanySpec};
use super::CompanySystemError;
use crate::account::StockCode;
use std::collections::BTreeMap;

/// 发行人登记表：上市主体规格的权威集合（公司行为对 `issued_shares` 的
/// 增减在此落账）。
///
/// 数值身份约定与 [`crate::company::CompanyRegistry`] 一致：构造时按字符串
/// 代码升序分配数值 `CompanyId`（id 升序 = 代码字典序，BTreeMap 迭代序 =
/// 展示序）；serde 以数值 id 为 key 持久化，恢复后经 `validate_restored`
/// 重建校验（见 [`super::persistence`]）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct IssuerRegistry(
    #[serde(deserialize_with = "super::persistence::unique_map")] BTreeMap<CompanyId, CompanySpec>,
);

impl IssuerRegistry {
    pub fn new(specs: Vec<CompanySpec>) -> Result<Self, CompanySystemError> {
        CompanySpec::validate_set(&specs)
            .map_err(|error| CompanySystemError::Invalid(error.to_string()))?;
        // 代码升序构造目录分配数值 id（重复代码已被 validate_set 拒绝）。
        let metas = specs
            .iter()
            .map(|spec| super::spec::CompanyMeta {
                code: spec.code.clone(),
                name: spec.name.clone(),
            })
            .collect();
        let catalog = CompanyCatalog::from_metas(metas)
            .map_err(|error| CompanySystemError::Invalid(error.to_string()))?;
        let mut issuers = BTreeMap::new();
        for spec in specs {
            let id = catalog
                .id(&spec.code)
                .expect("规格集合内的代码必然在目录内");
            issuers.insert(id, spec);
        }
        Ok(Self(issuers))
    }

    pub fn get(&self, company: &CompanyId) -> Option<&CompanySpec> {
        self.0.get(company)
    }

    /// 公司字符串代码（边界身份；数值 id 由 map key 承载）。
    pub fn code_of(&self, company: &CompanyId) -> Option<&str> {
        self.0.get(company).map(|spec| spec.code.as_str())
    }

    /// 字符串代码 → 数值 id（边界查询翻译；未知代码 = None）。
    pub fn id_of_code(&self, code: &str) -> Option<CompanyId> {
        self.0
            .iter()
            .find(|(_, spec)| spec.code == code)
            .map(|(id, _)| *id)
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
            .ok_or_else(|| CompanySystemError::Invalid(format!("未知公司 {company}")))?;
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
            .ok_or_else(|| CompanySystemError::Invalid(format!("未知公司 {company}")))?;
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

    pub fn issuer_of(&self, stock: &StockCode) -> Option<CompanyId> {
        self.0
            .iter()
            .find(|(_, spec)| spec.listed_stock.as_ref() == Some(stock))
            .map(|(id, _)| *id)
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
                        spec.code
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
