use super::*;
use crate::company::{CompanyId, CompanySpec, CompanySystem, IndustryId};

#[cfg(test)]
pub(crate) mod financial_fixture_tests;

pub(crate) struct CompanyAssembly {
    pub system: CompanySystem,
    pub library: crate::information::PublicLibrary,
}

pub(super) fn assemble_companies(setup: &SessionSetup, seed: u64) -> Result<CompanyAssembly, SessionError> {
    let specs = issuer_specs(setup)?;
    let system = CompanySystem::create(specs, setup.company_system.clone(), setup.start_date, seed)
        .map_err(|error| SessionError::InvalidSetup(format!("公司系统初始化失败：{error}")))?;
    system.issuers().validate_issuer_mapping(&setup.stocks.iter().map(|stock| (stock.code.clone(), stock.total_shares)).collect::<Vec<_>>())
        .map_err(|error| SessionError::InvalidSetup(format!("发行人映射非法：{error}")))?;
    let mut library = crate::information::PublicLibrary::new();
    let through = CivilInstant::new(setup.start_date.prev().map_err(crate::calendar::CalendarError::from)?, 86399)
        .map_err(crate::calendar::CalendarError::from)?;
    crate::information::publish_simple_scheduled(&system, setup.report_frequency, seed, None, through, &mut library, true)
        .map_err(|error| SessionError::Information(Box::new(error)))?;
    Ok(CompanyAssembly { system, library })
}

pub(super) fn issuer_specs(setup: &SessionSetup) -> Result<Vec<CompanySpec>, SessionError> {
    let crate::company::config::CompanySystemConfig::Simple(config) = &setup.company_system else {
        return Err(SessionError::InvalidSetup("Simulation 将在独立分支实现，当前不能装配发行人".into()));
    };
    let mut kinds = BTreeMap::new();
    for company in &config.companies {
        if kinds.insert(company.company.clone(), company.kind).is_some() {
            return Err(SessionError::InvalidSetup(format!("公司 {} 参数重复，无法确定发行人 CompanyKind", company.company.0)));
        }
    }
    setup.stocks.iter().map(|stock| {
        let id = CompanyId(format!("C-{}", stock.code.0));
        let kind = kinds.get(&id).copied().ok_or_else(|| SessionError::InvalidSetup(format!("股票 {} 缺少公司 {} 的显式 CompanyKind 配置", stock.code.0, id.0)))?;
        Ok(CompanySpec {
            id,
            name: format!("虚构上市公司{}", stock.code.0),
            industry: IndustryId("listed-simple".into()),
            kind,
            listed_stock: Some(stock.code.clone()),
            issued_shares: stock.total_shares,
            group_parent: None,
        })
    }).collect()
}
