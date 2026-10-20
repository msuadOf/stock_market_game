//! Session 的新局公司装配接缝：只投影发行人事实与必要配置。
//! 现金/负债/固定资产配平、税务与成本参数、账套和前史构造由公司域持有。
//!
//! B4b 扩展：公司系统线（CompanySystem spine）装配入口
//! [`assemble_company_system`] + [`issuer_specs`]（基 18725d07 的同名函数，
//! 数值 CompanyId 适配——配置按代码升序约定引用发行人）。装配时点=WorldSpec
//! 构造前 session 侧串行段（`GameSession::new_configured` 内，pre-spawn 无并发，
//! coupling 盘点映射 #11）。

use super::{SessionError, SessionSetup};
use crate::company::assembly::{CompanyAssemblyContext, IssuerFacts};
use crate::company::operations::CompanyOperations;
use crate::company::{CompanyId, CompanySpec, IndustryId};
use crate::error::ValidationFault;

pub(crate) use crate::company::assembly::CompanyAssembly;

pub(super) fn assemble_companies(
    setup: &SessionSetup,
    seed: u64,
    event_multiplier_bp: u16,
) -> Result<CompanyAssembly, SessionError> {
    let issuers: Vec<_> = setup.stocks.iter().map(|stock| IssuerFacts {
        code: &stock.code,
        total_shares: stock.total_shares,
    }).collect();
    CompanyOperations::assemble_new_game(CompanyAssemblyContext {
        issuers: &issuers,
        custom: setup.company_operations.as_ref(),
        groups: &setup.groups,
        start_date: setup.start_date,
        // 报告频率 B4b 恢复 SessionSetup.report_frequency 后改接配置真值；
        // 缺席（None）= 既有行为（Quarterly），旧局零变化。
        report_frequency: setup.report_frequency.unwrap_or(
            crate::information::ReportFrequency::Quarterly,
        ),
        seed,
        event_multiplier_bp,
    }).map_err(|source| SessionError::InvalidSetup(source.0))
}

/// 公司系统 spine 装配：按 setup 股票清单与 Simple 配置构建
/// [`crate::company::CompanySystem`]（发行人登记 + Simple 基本面状态机）。
/// 发行映射勾稽（上市证券 ↔ 总股本）在装配内显式校验，失败即拒绝建局。
pub(super) fn assemble_company_system(
    setup: &SessionSetup,
    seed: u64,
) -> Result<crate::company::CompanySystem, SessionError> {
    let specs = issuer_specs(setup)?;
    let system = crate::company::CompanySystem::create(
        specs,
        setup
            .company_system
            .clone()
            .expect("调用方已校验 company_system 在场"),
        setup.start_date,
        seed,
    )
    .map_err(|_error| {
        // fault1162: 公司系统装配失败（Simple 基本面/发行人登记构造）。
        // 槽0 待 C 批数值化（原文本为公司域错误文本——域内自由错误面）。
        SessionError::InvalidSetup(ValidationFault::new(1162, vec![0]))
    })?;
    let issuer_pairs: Vec<(crate::account::StockCode, u64)> = setup
        .stocks
        .iter()
        .map(|stock| (stock.code.clone(), stock.total_shares))
        .collect();
    system
        .issuers()
        .validate_issuer_mapping(&issuer_pairs)
        .map_err(|_error| {
            // fault1163: 发行人映射非法（上市证券/总股本与 setup 股票清单不匹配）。
            // 槽0 待 C 批数值化（原文本为内层公司域错误文本）。
            SessionError::InvalidSetup(ValidationFault::new(1163, vec![0]))
        })?;
    Ok(system)
}

/// setup 股票清单 → 发行人规格集合（Simple 线）。每只股票恰好对应一家
/// `C-{code}` 虚构上市发行人；`CompanyKind` 由 Simple 配置显式给定。
///
/// 数值身份约定（B4a）：`CompanyId` 按代码升序分配——配置侧的
/// `SimpleCompanyConfig.company` 即代码升序下标（与 `IssuerRegistry::new`
/// 同款约定，见 `company/identity.rs` 模块注释）。
pub(super) fn issuer_specs(setup: &SessionSetup) -> Result<Vec<CompanySpec>, SessionError> {
    let crate::company::config::CompanySystemConfig::Simple(config) =
        setup.company_system.as_ref().expect("调用方已校验公司系统在场")
    else {
        // fault1049: 公司系统配置非法（Simulation 未实现）。槽0=变体判别值
        // （Simple=0/Simulation=1）。
        return Err(SessionError::InvalidSetup(ValidationFault::new(
            1049,
            vec![1],
        )));
    };
    // 代码升序清单：数值 id 分配序（IssuerRegistry::new 同款）。
    let mut sorted: Vec<&crate::StockSpec> = setup.stocks.iter().collect();
    sorted.sort_by(|left, right| left.code.cmp(&right.code));
    let mut kinds = std::collections::BTreeMap::new();
    for (index, company) in config.companies.iter().enumerate() {
        if kinds.insert(company.company, company.kind).is_some() {
            // fault1049: Simple 配置公司参数重复（同 CompanyId 两条）。
            // 槽0 待 C 批数值化（原文本为公司数值 id "{:?}"）。
            return Err(SessionError::InvalidSetup(ValidationFault::new(
                1049,
                vec![u64::from(company.company.0)],
            )));
        }
        if u64::try_from(index).is_err() {
            // 防御：股数超 u64 索引域（StockIdx u16 已先验限制，此处不可达）。
            return Err(SessionError::InvalidSetup(ValidationFault::new(
                1049,
                vec![0],
            )));
        }
    }
    sorted
        .iter()
        .enumerate()
        .map(|(index, stock)| {
            let kind = kinds.get(&CompanyId(index as u32)).copied().ok_or_else(|| {
                // fault1049: 股票缺少显式 CompanyKind 配置。槽0 待 C 批数值化
                // （原文本为代码串 "{}"）。
                SessionError::InvalidSetup(ValidationFault::new(1049, vec![0]))
            })?;
            Ok(CompanySpec {
                code: format!("C-{}", stock.code.0),
                name: format!("虚构上市公司{}", stock.code.0),
                industry: IndustryId("listed-simple".into()),
                kind,
                listed_stock: Some(stock.code.clone()),
                issued_shares: stock.total_shares,
                group_parent: None,
            })
        })
        .collect()
}
