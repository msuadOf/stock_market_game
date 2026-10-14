//! Session 的新局公司装配接缝：只投影发行人事实与必要配置。
//! 现金/负债/固定资产配平、税务与成本参数、账套和前史构造由公司域持有。

use super::{SessionError, SessionSetup};
use crate::company::assembly::{CompanyAssemblyContext, IssuerFacts};
use crate::company::operations::CompanyOperations;

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
        seed,
        event_multiplier_bp,
    }).map_err(|source| SessionError::InvalidSetup(source.0))
}
