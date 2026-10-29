//! Session 公司权威状态的互斥容器；公开库和披露游标由共同 Session 外壳持有。

use crate::accounting::closing::ClosingEngine;
use crate::company::operations::CompanyOperations;
use crate::company::{CompanyRegistry, CompanySystem};
use crate::session::{GroupStructure, SessionCorporateActions};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) enum SessionCompanies {
    LegacyOperations(LegacyCompanyState),
    CompanySystem(Box<SystemCompanyState>),
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
#[serde(deny_unknown_fields)]
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
    pub(crate) fn legacy(&self) -> Option<&LegacyCompanyState> {
        match self {
            Self::LegacyOperations(state) => Some(state),
            Self::CompanySystem(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn legacy_mut(&mut self) -> Option<&mut LegacyCompanyState> {
        match self {
            Self::LegacyOperations(state) => Some(state),
            Self::CompanySystem(_) => None,
        }
    }

    pub(crate) fn system_arc(&self) -> Option<&Arc<CompanySystem>> {
        match self {
            Self::CompanySystem(state) => Some(&state.system),
            Self::LegacyOperations(_) => None,
        }
    }

    pub(crate) fn system_arc_mut(&mut self) -> Option<&mut Arc<CompanySystem>> {
        match self {
            Self::CompanySystem(state) => Some(&mut state.system),
            Self::LegacyOperations(_) => None,
        }
    }

    pub(crate) fn corporate_actions_mut(&mut self) -> Option<&mut SessionCorporateActions> {
        match self {
            Self::CompanySystem(state) => Some(&mut state.corporate_actions),
            Self::LegacyOperations(_) => None,
        }
    }

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

impl SavedSessionCompanies {
    pub(crate) fn system(&self) -> Option<&CompanySystem> {
        match self {
            Self::CompanySystem { system, .. } => Some(system),
            Self::LegacyOperations { .. } => None,
        }
    }
    pub(crate) fn actions(&self) -> Option<&SessionCorporateActions> {
        match self {
            Self::CompanySystem {
                corporate_actions, ..
            } => Some(corporate_actions),
            Self::LegacyOperations { .. } => None,
        }
    }
    pub(crate) fn issuer_ids(&self) -> std::collections::BTreeSet<crate::company::CompanyId> {
        match self {
            Self::CompanySystem { system, .. } => {
                system.issuers().iter().map(|(id, _)| *id).collect()
            }
            Self::LegacyOperations { operations, .. } => {
                operations.catalog().iter().map(|(id, _)| id).collect()
            }
        }
    }
}

#[cfg(test)]
impl SavedSessionCompanies {
    pub(crate) fn legacy_for_test(
        &self,
    ) -> (&CompanyOperations, &ClosingEngine, &Vec<GroupStructure>) {
        match self {
            Self::LegacyOperations {
                operations,
                closing,
                groups,
            } => (operations, closing, groups),
            Self::CompanySystem { .. } => panic!("测试必须显式构造 LegacyOperations"),
        }
    }
    pub(crate) fn legacy_for_test_mut(
        &mut self,
    ) -> (
        &mut CompanyOperations,
        &mut ClosingEngine,
        &mut Vec<GroupStructure>,
    ) {
        match self {
            Self::LegacyOperations {
                operations,
                closing,
                groups,
            } => (operations, closing, groups),
            Self::CompanySystem { .. } => panic!("测试必须显式构造 LegacyOperations"),
        }
    }
    pub(crate) fn actions_for_test_mut(&mut self) -> &mut SessionCorporateActions {
        match self {
            Self::CompanySystem {
                corporate_actions, ..
            } => corporate_actions,
            Self::LegacyOperations { .. } => panic!("测试必须显式构造 CompanySystem"),
        }
    }
    pub(crate) fn system_for_test_mut(&mut self) -> &mut CompanySystem {
        match self {
            Self::CompanySystem { system, .. } => system,
            Self::LegacyOperations { .. } => panic!("测试必须显式构造 CompanySystem"),
        }
    }
}
