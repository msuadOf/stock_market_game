//! 一次权威读取捕获公开基线；账户授权仍属于远端部署边界。
use super::{
    authority::HostFailure,
    protocol::{PlayerWorkingOrder, ProtocolSession, SaveCandidateKey},
    CompanyCatalogEntry, CompanyCatalogViewError, StockIdentityEntry,
};
use crate::{AccountId, CivilDate, Snapshot};

pub struct CapturedBaseline {
    pub snapshot: Snapshot,
    pub working_orders: Vec<PlayerWorkingOrder>,
    pub stock_catalog: Vec<StockIdentityEntry>,
    pub company_catalog: Vec<CompanyCatalogEntry>,
    pub civil_date: CivilDate,
    pub latest_checkpoint: Option<SaveCandidateKey>,
    pub public_report_ids: Vec<String>,
    pub failure: Option<HostFailure>,
    pub discovered_failure: bool,
}

impl CapturedBaseline {
    pub fn capture(
        session: &ProtocolSession,
        account: Option<AccountId>,
        failure: Option<HostFailure>,
    ) -> Result<Self, CompanyCatalogViewError> {
        Self::with_catalog(session, account, failure, session.company_catalog_view())
    }

    /// 已有目录故障注入入口也经过同一捕获策略，不另写宿主分支。
    pub fn with_catalog(
        session: &ProtocolSession,
        account: Option<AccountId>,
        failure: Option<HostFailure>,
        catalog: Result<Vec<CompanyCatalogEntry>, CompanyCatalogViewError>,
    ) -> Result<Self, CompanyCatalogViewError> {
        let company_catalog = catalog?;
        let discovered = session.require_healthy().err().map(HostFailure::step);
        let discovered_failure =
            discovered.is_some() && failure.as_ref().is_none_or(|failure| failure.recoverable);
        let failure = if discovered_failure {
            discovered
        } else {
            failure
        };
        // 当前 working-order 投影的业务合同仅支持 0 号账户；其他成员不能借用其订单。
        let (snapshot, working_orders) = if account == Some(AccountId(0))
            && failure.as_ref().is_none_or(|failure| failure.recoverable)
        {
            session.observer_baseline()
        } else {
            (session.snapshot(), Vec::new())
        };
        Ok(Self {
            snapshot,
            working_orders,
            stock_catalog: session.stock_catalog_view(),
            company_catalog,
            civil_date: session.civil_date(),
            latest_checkpoint: session.latest_day_end_key(),
            public_report_ids: session
                .publication_ids()
                .into_iter()
                .map(|id| id.value().to_string())
                .collect(),
            failure,
            discovered_failure,
        })
    }
}

#[cfg(test)]
mod tests;
