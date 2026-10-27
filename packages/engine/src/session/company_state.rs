//! Session 公司权威状态的互斥容器；公开库和披露游标由共同 Session 外壳持有。

use crate::accounting::closing::ClosingEngine;
use crate::company::operations::CompanyOperations;
use crate::company::{CompanyRegistry, CompanySystem};
use crate::session::{GroupStructure, SessionCorporateActions};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) enum SessionCompanies {
    LegacyOperations(LegacyCompanyState),
    CompanySystem(SystemCompanyState),
}

#[derive(Clone)]
pub(crate) struct LegacyCompanyState {
    pub(crate) registry: Arc<CompanyRegistry>,
    pub(crate) operations: Arc<CompanyOperations>,
    pub(crate) groups: Vec<GroupStructure>,
    pub(crate) closing: Arc<ClosingEngine>,
}

#[derive(Clone)]
pub(crate) struct SystemCompanyState {
    pub(crate) system: Arc<CompanySystem>,
    pub(crate) corporate_actions: SessionCorporateActions,
}

/// 每个存档仅保存所选模型的实际状态；Legacy registry 沿既有构造事实重建。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SavedSessionCompanies {
    LegacyOperations {
        operations: CompanyOperations,
        closing: ClosingEngine,
        groups: Vec<GroupStructure>,
    },
    CompanySystem {
        system: CompanySystem,
        corporate_actions: SessionCorporateActions,
    },
}

impl SessionCompanies {
    pub(crate) fn save(&self) -> SavedSessionCompanies {
        match self {
            Self::LegacyOperations(state) => SavedSessionCompanies::LegacyOperations {
                operations: state.operations.as_ref().clone(),
                closing: state.closing.as_ref().clone(),
                groups: state.groups.clone(),
            },
            Self::CompanySystem(state) => SavedSessionCompanies::CompanySystem {
                system: state.system.as_ref().clone(),
                corporate_actions: state.corporate_actions.clone(),
            },
        }
    }

    pub(crate) fn company_system(&self) -> Option<&CompanySystem> {
        match self {
            Self::LegacyOperations(_) => None,
            Self::CompanySystem(state) => Some(&state.system),
        }
    }

    pub(crate) fn corporate_actions(&self) -> Option<&SessionCorporateActions> {
        match self {
            Self::LegacyOperations(_) => None,
            Self::CompanySystem(state) => Some(&state.corporate_actions),
        }
    }
}

#[cfg(test)]
#[path = "company_state_tests.rs"]
mod tests;
