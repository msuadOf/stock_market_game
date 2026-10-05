use super::{
    finance::SimpleFinanceConfig,
    period::{PeriodGenerationParameters, PeriodNoiseConfig, SettlementCycle},
};
use crate::company::CompanyId;

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct SimpleConfig {
    pub environment: SimpleEnvironmentConfig,
    pub companies: Vec<SimpleCompanyConfig>,
    pub prehistory_periods: u16,
    pub settlement_cycle: SettlementCycle,
}

impl SimpleConfig {
    pub fn validate_for_issuers(
        &self,
        issuers: &crate::company::identity::IssuerRegistry,
    ) -> Result<(), crate::company::CompanySystemError> {
        use crate::company::CompanySystemError;
        self.environment.noise.validate()?;
        if !(0..=10_000).contains(&self.environment.persistence_bp) {
            return Err(CompanySystemError::Invalid(
                "环境持续系数必须为 0..10000 bp，扰动幅度不得为负".into(),
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for config in &self.companies {
            if !ids.insert(&config.company) || issuers.get(&config.company).is_none() {
                return Err(CompanySystemError::Invalid(
                    "公司参数重复或引用未知发行人".into(),
                ));
            }
            if issuers
                .get(&config.company)
                .is_some_and(|issuer| issuer.kind != config.kind)
            {
                return Err(CompanySystemError::Invalid(
                    "Simple 配置 CompanyKind 与发行人不一致".into(),
                ));
            }
            config.generation.validate()?;
            config
                .finance
                .tax_policy
                .validate()
                .map_err(|error| CompanySystemError::Invalid(error.to_string()))?;
        }
        if ids.len() != issuers.iter().count() {
            return Err(CompanySystemError::Invalid(
                "每个发行人都必须明确配置基本面参数".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct SimpleEnvironmentConfig {
    pub initial_change_bp: i32,
    pub persistence_bp: i32,
    pub noise: PeriodNoiseConfig,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct SimpleCompanyConfig {
    pub company: CompanyId,
    #[ts(type = "'Industrial' | 'Bank' | 'Insurance' | 'RealEstate'")]
    pub kind: crate::company::CompanyKind,
    pub generation: PeriodGenerationParameters,
    pub finance: SimpleFinanceConfig,
}
