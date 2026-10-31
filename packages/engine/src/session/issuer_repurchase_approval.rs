//! 回购唯一批准入口；公司、资金、计划与完整镜像沿同一次行政事务提交。
use super::{GameSession, SessionError};
use crate::company::issuer_repurchase::IssuerRepurchasePlan;
use crate::{AccountId, Money};

/// Host 必须从可信会话身份构造 Player；公司偏好由内部模型路由，客户端不直接提交来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepurchaseApprovalSource {
    Player {
        requester: AccountId,
    },
    #[cfg_attr(
        not(all(test, not(target_arch = "wasm32"))),
        expect(
            dead_code,
            reason = "保留经批准的公司偏好来源并校验issuer一致性；当前自动回购方案producer尚未接入，真实方案链不由lint改动接通",
        )
    )]
    CompanyPreference {
        issuer: crate::company::CompanyId,
    },
    #[cfg(test)]
    InternalScenario,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepurchaseApprovalReceipt {
    Approved {
        event_id: String,
        plan_id: crate::plans::PlanId,
        funding_amount: Money,
    },
    AlreadyApproved {
        event_id: String,
        plan_id: crate::plans::PlanId,
        funding_amount: Money,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum RepurchaseApprovalError {
    #[error("本局未启用发行人回购")]
    Disabled,
    #[error("回购命令来源无效：{requester:?}")]
    InvalidSource { requester: AccountId },
    #[error("公司偏好来源与方案发行人不一致：source={source_issuer:?}, plan={plan_issuer:?}")]
    SourceIssuerMismatch {
        source_issuer: crate::company::CompanyId,
        plan_issuer: crate::company::CompanyId,
    },
    #[error("当前持股不足：requester={requester:?}, issuer={issuer:?}")]
    InsufficientHolding {
        requester: AccountId,
        issuer: crate::company::CompanyId,
    },
    #[error("回购 event 输入冲突：{event_id}")]
    EventConflict { event_id: String },
    #[error("该证券已有活动计划：{existing_plan:?}")]
    PlanConflict { existing_plan: crate::plans::PlanId },
    #[error("回购批准条件无效：{detail}")]
    InvalidPlan { detail: String },
    #[error("数量上限 {requested} 超过当前股本 {issued} 或非减资用途的 10% 上限")]
    ShareLimit { requested: u64, issued: u64 },
    #[error("回购执行关联缺失：{event_id}")]
    MissingBinding { event_id: String },
    #[error("回购批准事务失败：{source}")]
    Transaction {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[cfg(test)]
    #[error("测试在真实 {phase} 处注入业务拒绝")]
    InjectedFailure { phase: &'static str },
}
impl GameSession {
    pub(super) fn validate_repurchase_bindings(&self) -> Result<(), RepurchaseApprovalError> {
        let actions = self
            .corporate_actions()
            .ok_or_else(|| invalid_plan("缺少公司行为容器"))?;
        let system = self
            .company_system()
            .ok_or_else(|| invalid_plan("缺少 CompanySystem"))?;
        let mut events = std::collections::BTreeSet::new();
        let mut identities = std::collections::BTreeSet::new();
        for binding in &actions.repurchase_execution_bindings {
            if !events.insert(binding.event_id.clone())
                || !identities.insert(binding.plan_id)
                || !binding.funding_committed
                || binding.plan_id.owner != binding.account
                || self
                    .account(binding.account)
                    .is_none_or(|account| account.kind() != crate::AccountKind::IssuerRepurchase)
            {
                return Err(invalid_plan("回购执行 binding 重复或资金账户不一致"));
            }
            let mut books = actions
                .issuer_repurchases
                .iter()
                .filter(|book| book.plan().event_id == binding.event_id);
            let book = books
                .next()
                .ok_or_else(|| invalid_plan("执行 binding 没有公司方案"))?;
            let plan = book.plan();
            if books.next().is_some()
                || plan.issuer != binding.issuer
                || plan.stock != binding.stock
                || plan.total_budget != binding.funding_amount
            {
                return Err(invalid_plan("执行 binding 与公司方案不一致"));
            }
            let fact = system
                .issuer_repurchase_facts(&binding.issuer)
                .map_err(|error| invalid_plan(error.to_string()))?
                .into_iter()
                .find(|fact| fact.event_id == binding.event_id)
                .ok_or_else(|| invalid_plan("执行 binding 缺少真实 finance 来源"))?;
            if fact.purpose != plan.purpose {
                return Err(invalid_plan(
                    "执行 binding 的 finance purpose 与公司方案不一致",
                ));
            }
            let entry = self
                .runtime_plans_snapshot()
                .entry(binding.plan_id)
                .map_err(|error| invalid_plan(error.to_string()))?;
            let crate::plans::PlanEntry::IssuerRepurchase(execution) = entry else {
                return Err(invalid_plan("执行 binding 不得指向 Investor 计划"));
            };
            if execution.issuer() != binding.issuer
                || Some(execution.stock()) != self.stock_id(&binding.stock.0)
            {
                return Err(invalid_plan("执行 binding 与真实 canonical 计划不一致"));
            }
            let closing = match execution.status() {
                crate::plans::RepurchaseExecutionStatus::Funded => {
                    crate::company::issuer_repurchase::RepurchaseBindingClosingState::Active
                }
                crate::plans::RepurchaseExecutionStatus::Draining => {
                    crate::company::issuer_repurchase::RepurchaseBindingClosingState::Draining
                }
                crate::plans::RepurchaseExecutionStatus::Closed => {
                    crate::company::issuer_repurchase::RepurchaseBindingClosingState::Closed
                }
            };
            if binding.closing_state != closing {
                return Err(invalid_plan("执行 binding 与 canonical closing 状态不一致"));
            }
        }
        if actions
            .issuer_repurchases
            .iter()
            .any(|book| !events.contains(&book.plan().event_id))
            || self.runtime_plans_snapshot().plan_ids().any(|id| {
                matches!(
                    self.runtime_plans_snapshot().entry(id),
                    Ok(crate::plans::PlanEntry::IssuerRepurchase(_))
                ) && !identities.contains(&id)
            })
        {
            return Err(invalid_plan(
                "公司方案或 canonical 公司计划缺少已提交 binding",
            ));
        }
        Ok(())
    }

    pub(crate) fn issuer_repurchase_execution_binding(
        &self,
        event_id: &str,
    ) -> Result<&crate::company::issuer_repurchase::RepurchaseExecutionBinding, SessionError> {
        let mut bindings = self
            .corporate_actions()
            .ok_or_else(|| invalid_plan("缺少公司行为容器"))?
            .repurchase_execution_bindings
            .iter()
            .filter(|binding| binding.event_id == event_id);
        let binding = bindings
            .next()
            .ok_or_else(|| RepurchaseApprovalError::MissingBinding {
                event_id: event_id.into(),
            })?;
        if bindings.next().is_some() {
            return Err(invalid_plan("同一 event 有重复执行关联").into());
        }
        Ok(binding)
    }

    pub(crate) fn approve_issuer_repurchase(
        &mut self,
        plan: IssuerRepurchasePlan,
        source: RepurchaseApprovalSource,
    ) -> Result<RepurchaseApprovalReceipt, SessionError> {
        use crate::company::issuer_repurchase::{
            IssuerRepurchaseBook, IssuerRepurchaseFinanceFact, RepurchaseBindingClosingState,
            RepurchaseExecutionBinding,
        };
        use crate::runtime::world::{
            AdminOperation, AdminPlanReference, InformationPreparePayload,
            RepurchaseExecutionProjection, RepurchaseMaterialRestriction,
        };
        use crate::runtime::{RepurchasePreparePayload, RepurchaseProjectionInput, RuntimeError};
        use std::sync::Arc;
        self.require_healthy()
            .map_err(|source| RepurchaseApprovalError::Transaction {
                source: Box::new(source),
            })?;
        if !self.state.setup.issuer_repurchase_enabled {
            return Err(RepurchaseApprovalError::Disabled.into());
        }
        let stock = self
            .stock_id(&plan.stock.0)
            .ok_or_else(|| invalid_plan("方案证券不在真实 StockCatalog"))?;
        // 权限先于幂等；总持仓包含 T+1 锁定股份，不消耗玩家 pending intent。
        match source {
            RepurchaseApprovalSource::Player { requester } => {
                let account = self
                    .account(requester)
                    .filter(|account| account.kind() == crate::AccountKind::Player)
                    .ok_or(RepurchaseApprovalError::InvalidSource { requester })?;
                if account
                    .position(&stock)
                    .is_none_or(|position| position.qty() == 0)
                {
                    return Err(RepurchaseApprovalError::InsufficientHolding {
                        requester,
                        issuer: plan.issuer,
                    }
                    .into());
                }
            }
            RepurchaseApprovalSource::CompanyPreference { issuer } if issuer != plan.issuer => {
                return Err(RepurchaseApprovalError::SourceIssuerMismatch {
                    source_issuer: issuer,
                    plan_issuer: plan.issuer,
                }
                .into());
            }
            _ => {}
        }
        let system = self
            .company_system()
            .ok_or_else(|| invalid_plan("缺少 CompanySystem"))?;
        let actions = self
            .corporate_actions()
            .ok_or_else(|| invalid_plan("缺少公司行为容器"))?;
        self.validate_repurchase_bindings()?;
        system
            .validate_issuer_repurchase_books(&actions.issuer_repurchases)
            .map_err(|error| invalid_plan(error.to_string()))?;
        if let Some(book) = actions
            .issuer_repurchases
            .iter()
            .find(|book| book.plan().event_id == plan.event_id)
        {
            if book.plan() != &plan {
                return Err(RepurchaseApprovalError::EventConflict {
                    event_id: plan.event_id,
                }
                .into());
            }
            let binding = self.issuer_repurchase_execution_binding(&plan.event_id)?;
            if !binding.funding_committed || binding.funding_amount != plan.total_budget {
                return Err(invalid_plan("已批准方案的资金关联不完整").into());
            }
            return Ok(RepurchaseApprovalReceipt::AlreadyApproved {
                event_id: plan.event_id,
                plan_id: binding.plan_id,
                funding_amount: plan.total_budget,
            });
        }
        plan.validate()
            .map_err(|error| invalid_plan(error.to_string()))?;
        plan.validate_calendar(self.state.civil_clock.calendar())
            .map_err(|error| invalid_plan(error.to_string()))?;
        let day = self.civil_date();
        let _eligibility = self.repurchase_eligibility(
            &plan,
            crate::CivilInstant::new(day, 0).map_err(|error| invalid_plan(error.to_string()))?,
        )?;
        if plan.approved_on != day {
            return Err(invalid_plan("方案批准日必须为当前自然日").into());
        }
        let security = self
            .state
            .setup
            .stocks
            .iter()
            .find(|security| security.code == plan.stock)
            .ok_or_else(|| invalid_plan("证券规格缺失"))?;
        let exchange = match security.exchange {
            super::StockExchange::Shanghai => crate::calendar::CalendarExchange::Sse,
            super::StockExchange::Shenzhen => crate::calendar::CalendarExchange::Szse,
        };
        if plan.exchange != exchange
            || system
                .issuers()
                .get(&plan.issuer)
                .and_then(|issuer| issuer.listed_stock.as_ref())
                != Some(&plan.stock)
        {
            return Err(invalid_plan("方案 issuer/stock/exchange 与实际发行人不一致").into());
        }
        let mut registries = actions
            .registries
            .iter()
            .filter(|registry| registry.stock() == &plan.stock);
        let registry = registries
            .next()
            .ok_or_else(|| invalid_plan("缺少真实 ShareRegistry"))?;
        registry
            .validate()
            .map_err(|error| invalid_plan(error.to_string()))?;
        if registries.next().is_some()
            || registry.issuer() != &plan.issuer
            || registry.settled_on() > day
            || system
                .issuers()
                .get(&plan.issuer)
                .is_none_or(|issuer| issuer.issued_shares != registry.issued_shares())
        {
            return Err(invalid_plan("名册重复、发行人、股本或结算日期无效").into());
        }
        for book in actions
            .issuer_repurchases
            .iter()
            .filter(|book| book.plan().stock == plan.stock)
        {
            if book
                .current_inventory_shares(&[])
                .map_err(|error| invalid_plan(error.to_string()))?
                > 0
            {
                return Err(invalid_plan(
                    "该证券前一方案尚有未注销库存，当前不支持交叠回购库存归属",
                )
                .into());
            }
        }
        // 复用完整资本来源校验，不能凭期末总数绕过既有多 event/split 来源门。
        let _capital =
            super::capital_approval_context::CapitalApprovalContext::prepare(self, plan.issuer)
                .map_err(|error| invalid_plan(error.to_string()))?;
        if plan.max_shares > registry.issued_shares()
            || (plan.purpose != crate::company::issuer_repurchase::RepurchasePurpose::ReduceCapital
                && u128::from(plan.max_shares) * 10 > u128::from(registry.issued_shares()))
        {
            return Err(RepurchaseApprovalError::ShareLimit {
                requested: plan.max_shares,
                issued: registry.issued_shares(),
            }
            .into());
        }
        let anchor = actions
            .share_splits
            .iter()
            .filter(|book| {
                book.plan().stock == plan.stock
                    && book.receipt().is_some()
                    && book.plan().ex_rights_on <= day
                    && book.settled_on().is_some_and(|on| on <= day)
            })
            .max_by_key(|book| book.plan().ex_rights_on)
            .map(|book| book.plan().event_id.as_str());
        if anchor != plan.approval_unit_event_id.as_deref() {
            return Err(invalid_plan("批准单位锚与已应用 split 链不一致").into());
        }
        let mut treasury = self.account_roster().into_iter().filter(|id| {
            self.account(*id)
                .is_some_and(|account| account.kind() == crate::AccountKind::IssuerRepurchase)
        });
        let account = treasury
            .next()
            .ok_or_else(|| invalid_plan("缺少真实回购专户"))?;
        if treasury.next().is_some() {
            return Err(invalid_plan("回购共享专户不唯一").into());
        }
        if let Some(existing) = self.runtime_plans_snapshot().active_entry(account, &stock) {
            return Err(RepurchaseApprovalError::PlanConflict {
                existing_plan: existing.plan_id(),
            }
            .into());
        }
        let mut candidate_system = system.clone();
        let mut candidate_actions = actions.clone();
        candidate_system
            .validate_issuer_repurchase_books(&candidate_actions.issuer_repurchases)
            .map_err(|error| invalid_plan(error.to_string()))?;
        let already = candidate_system
            .declare_issuer_repurchase(
                &plan.issuer,
                IssuerRepurchaseFinanceFact {
                    event_id: plan.event_id.clone(),
                    approval_reference: plan.approval_reference.clone(),
                    approved_on: plan.approved_on,
                    synthetic_funding: crate::accounting::AccountingAmount::from_money(
                        plan.total_budget,
                    ),
                    purpose: plan.purpose.clone(),
                    completed_on: None,
                    spent: None,
                    withdrawn_remainder: None,
                    cancelled_on: None,
                    cancelled_shares: 0,
                    capital_reduction: None,
                },
            )
            .map_err(|error| invalid_plan(error.to_string()))?;
        if already {
            return Err(invalid_plan("finance 已有 event 但缺少公司方案").into());
        }
        candidate_actions.issuer_repurchases.push(
            IssuerRepurchaseBook::new(plan.clone())
                .map_err(|error| invalid_plan(error.to_string()))?,
        );
        candidate_system
            .validate_issuer_repurchase_books(&candidate_actions.issuer_repurchases)
            .map_err(|error| invalid_plan(error.to_string()))?;
        let roster = self.account_roster();
        let index = roster
            .iter()
            .position(|id| *id == account)
            .ok_or_else(|| invalid_plan("专户不在真实名册"))?;
        let per_group = roster.len().div_ceil(self.runtime_group_count);
        let group = crate::runtime::GroupIdx::try_from(index / per_group)
            .map_err(|_| invalid_plan("专户 group 坐标溢出"))?;
        let local = crate::runtime::LocalIdx::try_from(index % per_group)
            .map_err(|_| invalid_plan("专户 local 坐标溢出"))?;
        let stock_coordinate = crate::runtime::StockIdx::try_from(stock.0)
            .map_err(|_| invalid_plan("证券坐标溢出"))?;
        let projection = RepurchaseExecutionProjection {
            account,
            issuer: plan.issuer,
            stock: stock_coordinate,
            announced: false,
            announced_on: plan.announced_on,
            window_start_on: plan.window_start_on,
            window_deadline_on: plan.window_deadline_on,
            exchange: plan.exchange,
            purpose: plan.purpose.clone(),
            funding_amount: plan.total_budget,
            price_cap: plan.price_cap_per_share,
            max_shares: plan.max_shares,
            unit_as_of: day,
            unit_revision: 0,
            book_through_cursor: 0,
            book_gross: Money::ZERO,
            book_fees: Money::ZERO,
            recycled: Money::ZERO,
            inventory_shares: 0,
            last_order_day: None,
            eligibility_revision: 0,
            eligibility_as_of: crate::calendar::CivilInstant::new(day, 0)
                .map_err(|error| invalid_plan(error.to_string()))?,
            // 批准不认证发行实施起点；X 必须以真实 source/cut 替换此未知基线。
            issuance_in_progress: None,
            material_restriction: RepurchaseMaterialRestriction::NotModelled,
        };
        let information = InformationPreparePayload {
            library: Arc::clone(&self.state.library),
            current_facts: Arc::new(self.state.company_read_facts().map_err(|source| {
                RepurchaseApprovalError::Transaction {
                    source: Box::new(source),
                }
            })?),
            observation: None,
        };
        #[cfg(test)]
        let amount = if approval_fault_pending() == Some(ApprovalFault::GroupPrepare) {
            0
        } else {
            plan.total_budget.cents()
        };
        #[cfg(not(test))]
        let amount = plan.total_budget.cents();
        let operations = vec![
            AdminOperation::OpenIssuerRepurchase {
                local,
                stock: stock_coordinate,
                issuer: plan.issuer,
            },
            AdminOperation::SyntheticBudgetCredit {
                local,
                stock: stock_coordinate,
                execution_plan: AdminPlanReference::CreatedInThisPrepare { operation_index: 0 },
                amount,
            },
        ];
        let payload = RepurchasePreparePayload {
            groups: vec![(
                group,
                vec![RepurchaseProjectionInput {
                    local,
                    stock: stock_coordinate,
                    issuer: plan.issuer,
                    execution_plan: AdminPlanReference::CreatedInThisPrepare { operation_index: 0 },
                    projection: projection.clone(),
                }],
            )],
        };
        let result = self.apply_repurchase_administrative_operations(
            vec![(group, operations)],
            Some(information),
            payload,
            |candidate| {
                #[cfg(test)]
                if approval_fault_pending() == Some(ApprovalFault::Inspect) {
                    APPROVAL_FAULT.with(|slot| slot.set(None));
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: "回购批准真实 inspect 测试拒绝".into(),
                    });
                }
                let prepared = candidate
                    .groups
                    .iter()
                    .find(|candidate| candidate.group == group)
                    .and_then(|candidate| candidate.repurchase.as_ref())
                    .ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: "批准缺少实际组回购候选".into(),
                    })?;
                let [created] = prepared.created.as_slice() else {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: "批准必须且仅创建一条公司计划".into(),
                    });
                };
                if created.local != local
                    || created.stock != stock_coordinate
                    || created.issuer != plan.issuer
                    || created.operation_index != 0
                    || created.plan_id.owner != account
                    || prepared.projections.get(&created.plan_id) != Some(&projection)
                {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: "批准候选实际身份或 terms 不一致".into(),
                    });
                }
                candidate_actions
                    .repurchase_execution_bindings
                    .push(RepurchaseExecutionBinding {
                        event_id: plan.event_id.clone(),
                        issuer: plan.issuer,
                        stock: plan.stock.clone(),
                        account,
                        plan_id: created.plan_id,
                        funding_committed: true,
                        funding_amount: plan.total_budget,
                        applied_fact_cursor: 0,
                        closing_state: RepurchaseBindingClosingState::Active,
                    });
                let companies = super::company_state::SessionCompanies::CompanySystem(Box::new(
                    super::company_state::SystemCompanyState {
                        system: Arc::new(candidate_system),
                        corporate_actions: candidate_actions,
                    },
                ));
                let receipt = RepurchaseApprovalReceipt::Approved {
                    event_id: plan.event_id.clone(),
                    plan_id: created.plan_id,
                    funding_amount: plan.total_budget,
                };
                Ok((companies, receipt))
            },
        );
        let (companies, receipt) = match result {
            Ok(value) => value,
            Err(error) => {
                #[cfg(test)]
                {
                    if approval_fault_pending() == Some(ApprovalFault::GroupPrepare)
                        && error.to_string().contains("正分额")
                    {
                        APPROVAL_FAULT.with(|slot| slot.set(None));
                        return Err(RepurchaseApprovalError::InjectedFailure {
                            phase: "GroupPrepare",
                        }
                        .into());
                    }
                    if error.to_string().contains("回购批准真实 inspect 测试拒绝") {
                        return Err(
                            RepurchaseApprovalError::InjectedFailure { phase: "inspect" }.into(),
                        );
                    }
                }
                return Err(RepurchaseApprovalError::Transaction {
                    source: Box::new(error),
                }
                .into());
            }
        };
        self.state.companies = companies;
        Ok(receipt)
    }
}

fn invalid_plan(detail: impl Into<String>) -> RepurchaseApprovalError {
    RepurchaseApprovalError::InvalidPlan {
        detail: detail.into(),
    }
}

/// 一次测试线程内的故障请求；只有真实 Prepare/inspect 消费后才算到达目标门。
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ApprovalFault {
    GroupPrepare,
    Inspect,
}
#[cfg(test)]
std::thread_local! {
    static APPROVAL_FAULT: std::cell::Cell<Option<ApprovalFault>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(super) struct ApprovalFaultGuard;
#[cfg(test)]
impl Drop for ApprovalFaultGuard {
    fn drop(&mut self) {
        APPROVAL_FAULT.with(|slot| slot.set(None));
    }
}
#[cfg(test)]
pub(super) fn set_approval_fault(fault: ApprovalFault) -> ApprovalFaultGuard {
    APPROVAL_FAULT.with(|slot| {
        assert!(slot.replace(Some(fault)).is_none());
    });
    ApprovalFaultGuard
}
#[cfg(test)]
pub(super) fn approval_fault_pending() -> Option<ApprovalFault> {
    APPROVAL_FAULT.with(std::cell::Cell::get)
}
