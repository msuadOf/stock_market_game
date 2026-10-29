//! 玩家提案的 Session 受理接缝；批准候选与 INFO 共用一次行政事务。

use super::capital_approval_context::CapitalApprovalContext;
use super::capital_command_candidates::{
    prepare_rights_approval, prepare_split_approval, CapitalCommandInput,
};
use super::company_state::{SessionCompanies, SystemCompanyState};
use super::player_company_command_candidates::{
    build_player_repurchase_plan, build_player_rights_plan, build_player_split_plan,
    player_holding_context, prepare_player_cash_dividend, prepare_player_stock_distribution,
    PlayerCompanyCommandInput,
};
use super::{GameSession, PlayerCompanyProposal, PlayerProposalKind};
use crate::company::{CompanyErrorClass, CompanyId, CompanySystem};
#[path = "player_company_command_classification.rs"]
mod classification;
use crate::runtime::world::InformationPreparePayload;
use crate::{AccountId, AccountKind, CivilDate};
use std::sync::{atomic::Ordering, Arc};

/// 只证明该公司方案已经批准并安装；不证明未来付款、登记或股份到账。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerCompanyCommandReceipt {
    pub account: AccountId,
    pub company: CompanyId,
    pub kind: PlayerProposalKind,
    pub event_id: String,
    pub approved_on: CivilDate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerCompanyCommandIssue {
    MissingCompanySystem,
    UnknownIssuer,
    UnlistedIssuer,
    MissingSecurity,
    PendingCommandImplementation,
    CandidateConfigurationMismatch,
    CandidateIssuerMismatch,
    RevisionExhausted,
    RevisionChanged,
    RuntimeLockPoisoned,
    RuntimeTickMismatch,
}

#[derive(Debug, thiserror::Error)]
#[error("玩家提案条件不满足：{0:?}")]
struct CommandIssue(PlayerCompanyCommandIssue);

#[derive(Debug, thiserror::Error)]
#[error("本局未启用{mechanism}机制（{setup_flag}=false）")]
struct DisabledCompanyMechanism {
    mechanism: &'static str,
    setup_flag: &'static str,
}

/// Rust 入口保留真实错误链；宿主的数值错误投影须消费此真实结果，不能回填成功。
#[derive(Debug, thiserror::Error)]
#[error("玩家公司命令失败：账户 {account:?}，公司 {company:?}，类别 {kind:?}，日期 {day}：{cause}")]
pub struct PlayerCompanyCommandError {
    pub account: AccountId,
    pub company: CompanyId,
    pub kind: PlayerProposalKind,
    pub day: CivilDate,
    classification: CompanyErrorClass,
    #[source]
    cause: Box<dyn std::error::Error + Send + Sync>,
}

impl PlayerCompanyCommandError {
    /// 分类在原 typed cause 装箱前确定，不替换或消费真实错误链。
    pub const fn classification(&self) -> CompanyErrorClass {
        self.classification
    }

    pub fn account(&self) -> AccountId {
        self.account
    }
}

impl GameSession {
    /// 账户由宿主鉴权绑定；日期、证券和名册只从当前 Session 读取。
    /// 只批准方案；回购资金与执行身份由唯一共同批准入口在同一事务提交。
    pub fn propose_player_company_action(
        &mut self,
        account: AccountId,
        proposal: PlayerCompanyProposal,
    ) -> Result<PlayerCompanyCommandReceipt, PlayerCompanyCommandError> {
        let company = proposal.company();
        let kind = proposal.kind();
        let day = self.civil_date();
        let fail = |classification, cause: Box<dyn std::error::Error + Send + Sync>| {
            PlayerCompanyCommandError {
                account,
                company,
                kind,
                day,
                classification,
                cause,
            }
        };
        let issue = |value| {
            fail(
                classification::command_issue(value),
                Box::new(CommandIssue(value)),
            )
        };
        self.require_healthy()
            .map_err(|error| fail(CompanyErrorClass::SystemState, Box::new(error)))?;
        let requester = self.account(account).ok_or_else(|| {
            fail(
                CompanyErrorClass::InvalidInput,
                Box::new(super::SessionError::UnknownPlayer(account)),
            )
        })?;
        if requester.kind() != AccountKind::Player {
            return Err(fail(
                CompanyErrorClass::InvalidInput,
                Box::new(super::SessionError::NotPlayer(account)),
            ));
        }
        let (system, actions) = match &self.state.companies {
            SessionCompanies::CompanySystem(state) => {
                (state.system.as_ref(), &state.corporate_actions)
            }
            SessionCompanies::LegacyOperations(_) => {
                return Err(issue(PlayerCompanyCommandIssue::MissingCompanySystem))
            }
        };
        let issuer = system
            .issuers()
            .get(&company)
            .ok_or_else(|| issue(PlayerCompanyCommandIssue::UnknownIssuer))?;
        let stock = issuer
            .listed_stock
            .as_ref()
            .ok_or_else(|| issue(PlayerCompanyCommandIssue::UnlistedIssuer))?;
        let security = self
            .state
            .setup
            .stocks
            .iter()
            .find(|security| &security.code == stock)
            .ok_or_else(|| issue(PlayerCompanyCommandIssue::MissingSecurity))?;
        let input = PlayerCompanyCommandInput {
            requester,
            catalog: &self.state.stocks,
            security,
            system,
            actions,
            calendar: self.state.civil_clock.calendar(),
            day,
            proposal,
        };
        if matches!(
            kind,
            PlayerProposalKind::RightsOffering
                | PlayerProposalKind::SecondaryOffering
                | PlayerProposalKind::IssuerRepurchase
                | PlayerProposalKind::ShareSplit
        ) {
            // 尚未执行资产操作的类别也必须先验证真实当前持仓，不能让占位拒绝替代身份门。
            player_holding_context(&input)
                .map_err(|error| fail(classification::player(&error), Box::new(error)))?;
        }
        if matches!(
            kind,
            PlayerProposalKind::RightsOffering | PlayerProposalKind::SecondaryOffering
        ) && !self.state.setup.rights_offering_enabled
        {
            return Err(fail(
                CompanyErrorClass::UnsupportedOperation,
                Box::new(DisabledCompanyMechanism {
                    mechanism: "配股／增发",
                    setup_flag: "rights_offering_enabled",
                }),
            ));
        }
        if kind == PlayerProposalKind::IssuerRepurchase
            && !self.state.setup.issuer_repurchase_enabled
        {
            return Err(fail(
                CompanyErrorClass::UnsupportedOperation,
                Box::new(DisabledCompanyMechanism {
                    mechanism: "发行人回购",
                    setup_flag: "issuer_repurchase_enabled",
                }),
            ));
        }
        if kind == PlayerProposalKind::IssuerRepurchase {
            let plan = build_player_repurchase_plan(input)
                .map_err(|error| fail(classification::player(&error), Box::new(error)))?;
            let approved = self
                .approve_issuer_repurchase(
                    plan,
                    super::issuer_repurchase_approval::RepurchaseApprovalSource::Player {
                        requester: account,
                    },
                )
                .map_err(|error| {
                    fail(classification::repurchase_session(&error), Box::new(error))
                })?;
            let event_id = match approved {
                super::RepurchaseApprovalReceipt::Approved { event_id, .. }
                | super::RepurchaseApprovalReceipt::AlreadyApproved { event_id, .. } => event_id,
            };
            return Ok(PlayerCompanyCommandReceipt {
                account,
                company,
                kind,
                event_id,
                approved_on: day,
            });
        }
        let capital = if matches!(
            kind,
            PlayerProposalKind::StockDistribution
                | PlayerProposalKind::RightsOffering
                | PlayerProposalKind::SecondaryOffering
                | PlayerProposalKind::ShareSplit
        ) {
            Some(
                CapitalApprovalContext::prepare(self, company)
                    .map_err(|error| fail(CompanyErrorClass::SystemState, Box::new(error)))?,
            )
        } else {
            None
        };
        let (candidate_system, candidate_actions, event_id) = match kind {
            PlayerProposalKind::CashDividend => {
                let candidate = prepare_player_cash_dividend(input)
                    .map_err(|error| fail(classification::player(&error), Box::new(error)))?;
                (
                    candidate.company_system,
                    candidate.actions,
                    candidate.receipt.plan_id,
                )
            }
            PlayerProposalKind::StockDistribution => {
                let candidate = prepare_player_stock_distribution(
                    input,
                    capital.as_ref().expect("送转已准备实际资本口径"),
                )
                .map_err(|error| fail(classification::player(&error), Box::new(error)))?;
                (
                    candidate.company_system,
                    candidate.actions,
                    candidate.fact.event_id,
                )
            }
            PlayerProposalKind::RightsOffering | PlayerProposalKind::SecondaryOffering => {
                let exchange = player_holding_context(&input)
                    .map_err(|error| fail(classification::player(&error), Box::new(error)))?
                    .2;
                let plan =
                    build_player_rights_plan(input, self.state.setup.rights_offering_enabled)
                        .map_err(|error| fail(classification::player(&error), Box::new(error)))?;
                let candidate = prepare_rights_approval(
                    CapitalCommandInput {
                        system,
                        actions,
                        calendar: self.state.civil_clock.calendar(),
                        day,
                        exchange,
                        capital: capital.as_ref().expect("配股已准备实际资本口径"),
                    },
                    plan,
                )
                .map_err(|error| fail(classification::capital(&error), Box::new(error)))?;
                (
                    candidate.company_system,
                    candidate.actions,
                    candidate.event_id,
                )
            }
            PlayerProposalKind::ShareSplit => {
                let exchange = player_holding_context(&input)
                    .map_err(|error| fail(classification::player(&error), Box::new(error)))?
                    .2;
                let plan = build_player_split_plan(input)
                    .map_err(|error| fail(classification::player(&error), Box::new(error)))?;
                let candidate = prepare_split_approval(
                    CapitalCommandInput {
                        system,
                        actions,
                        calendar: self.state.civil_clock.calendar(),
                        day,
                        exchange,
                        capital: capital.as_ref().expect("拆缩股已准备实际资本口径"),
                    },
                    plan,
                )
                .map_err(|error| fail(classification::capital(&error), Box::new(error)))?;
                (
                    candidate.company_system,
                    candidate.actions,
                    candidate.event_id,
                )
            }
            _ => {
                return Err(issue(
                    PlayerCompanyCommandIssue::PendingCommandImplementation,
                ))
            }
        };
        // 批准不得改变证券身份或股本；真实到账在后续领域相位执行。
        if candidate_system.issuers() != system.issuers() {
            return Err(issue(PlayerCompanyCommandIssue::CandidateIssuerMismatch));
        }
        if Some(candidate_system.config()) != self.state.setup.company_system {
            return Err(issue(
                PlayerCompanyCommandIssue::CandidateConfigurationMismatch,
            ));
        }
        validate_approval_books(&candidate_system, &candidate_actions)
            .map_err(|error| fail(CompanyErrorClass::SystemState, Box::new(error)))?;
        let payload = InformationPreparePayload {
            library: Arc::clone(&self.state.library),
            current_facts: Arc::new(
                self.state
                    .company_read_facts()
                    .map_err(|error| fail(CompanyErrorClass::SystemState, Box::new(error)))?,
            ),
            observation: None,
        };
        let construct_runtime =
            crate::runtime::step_runtime_enabled() && self.runtime_driver.is_none();
        if capital.as_ref().is_some_and(|context| {
            !context.matches_current(self) || !context.matches_prepared_revision(self)
        }) {
            return Err(issue(PlayerCompanyCommandIssue::RevisionChanged));
        }
        // 首次移交旧簿会消费一个 revision；构造前为移交和本命令分别留位。
        let required_revisions = if construct_runtime { 2 } else { 1 };
        self.external_revision()
            .checked_add(required_revisions)
            .ok_or_else(|| issue(PlayerCompanyCommandIssue::RevisionExhausted))?;
        if construct_runtime {
            self.construct_runtime_driver_lazy()
                .map_err(|error| fail(CompanyErrorClass::SystemState, Box::new(error)))?;
        }
        // 真实初始化已经结束；inspect 只能比较这个同作用域的新鲜来源戳。
        let revision = self.external_revision();
        let next_revision = revision
            .checked_add(1)
            .ok_or_else(|| issue(PlayerCompanyCommandIssue::RevisionExhausted))?;
        if capital
            .as_ref()
            .is_some_and(|context| !context.matches_current(self))
        {
            return Err(issue(PlayerCompanyCommandIssue::RevisionChanged));
        }
        let receipt = PlayerCompanyCommandReceipt {
            account,
            company,
            kind,
            event_id,
            approved_on: day,
        };
        // 所有分配及可失败的模型检查都在首 Commit 前完成；成功尾仅移动预备值。
        let companies = SessionCompanies::CompanySystem(Box::new(SystemCompanyState {
            system: Arc::new(candidate_system),
            corporate_actions: candidate_actions,
        }));
        if crate::runtime::step_runtime_enabled() {
            let outcome = (|| {
                let cell = self.runtime_driver.as_ref().expect("驱动器已经构造");
                let mut driver = cell.lock().map_err(|_| {
                    (
                        Box::new(CommandIssue(PlayerCompanyCommandIssue::RuntimeLockPoisoned))
                            as Box<dyn std::error::Error + Send + Sync>,
                        true,
                    )
                })?;
                if driver.world_committed_tick() != self.tick() {
                    return Err((
                        Box::new(CommandIssue(PlayerCompanyCommandIssue::RuntimeTickMismatch))
                            as Box<dyn std::error::Error + Send + Sync>,
                        true,
                    ));
                }
                driver
                    .commit_company_approval(payload, || {
                        self.external_revision() == revision
                            && capital
                                .as_ref()
                                .is_none_or(|context| context.matches_current(self))
                    })
                    .map_err(|failure| (failure.error, failure.terminal))
            })();
            if let Err((cause, terminal)) = outcome {
                // guard 已释放；业务 prepare/inspect 拒绝保持可继续，terminal 沿同健康门封首错。
                if terminal {
                    self.poison_runtime_step(super::StepFatal::InvariantViolation {
                        location: "GameSession::propose_player_company_action".into(),
                        description: format!("公司命令运行时已经终止：{cause}"),
                    });
                }
                return Err(fail(CompanyErrorClass::SystemState, cause));
            }
        }
        self.revision.store(next_revision, Ordering::Relaxed);
        self.state.companies = companies;
        Ok(receipt)
    }
}

fn validate_approval_books(
    system: &CompanySystem,
    actions: &super::SessionCorporateActions,
) -> Result<(), crate::company::CompanySystemError> {
    system.validate_cash_dividend_books(&actions.dividends)?;
    system.validate_stock_distribution_books(&actions.stock_distributions)?;
    system.validate_rights_offering_books(&actions.rights_offerings)?;
    system.validate_issuer_repurchase_books(&actions.issuer_repurchases)?;
    system.validate_share_split_books(&actions.share_splits)?;
    Ok(())
}

#[cfg(test)]
#[path = "player_company_commands_tests.rs"]
mod tests;
