//! 将 NPC 决策结果投影到可丢弃 tick candidate。
//!
//! 工作单 reconciliation 决策先于 residual intents 路由，二者保持分离。
//! candidate composer 在投影后为每个账户分配连续局部序号，
//! 保留 reconciliation 在 residual 之前的顺序，不重用 raw strategy 身份。

use super::decision_snapshot_capture::CapturedDecisionSnapshot;
use super::npc_decisions::{NpcDecisionSourceOutput, NpcStrategyUpdate};
use super::IntentCandidateKey;
use crate::session::execution::reconcile_plan::WorkingOrderDecision;
use crate::session::{GameSession, ReconcileScope, RetailDecisionTrace, WorkingOrderSlices};
use crate::strategy::Intent;
use crate::{AccountId, AccountKind, LimitPrice, OrderId, StockId};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub(in crate::session) struct ProjectedNpcIntent {
    account: AccountId,
    source_key: Option<IntentCandidateKey>,
    #[cfg(test)]
    source_intent: Option<Intent>,
    projected_intent: Intent,
}

impl ProjectedNpcIntent {
    pub(in crate::session) const fn account(&self) -> AccountId {
        self.account
    }

    pub(in crate::session) const fn source_key(&self) -> Option<&IntentCandidateKey> {
        self.source_key.as_ref()
    }

    #[cfg(test)]
    pub(in crate::session) const fn source_intent(&self) -> Option<&Intent> {
        self.source_intent.as_ref()
    }

    pub(in crate::session) const fn projected_intent(&self) -> &Intent {
        &self.projected_intent
    }
}

#[derive(Clone, Debug)]
pub(in crate::session) enum NpcReconciliationDecision {
    Keep {
        #[cfg(test)]
        account: AccountId,
        #[cfg(test)]
        order_id: OrderId,
    },
    Cancel {
        account: AccountId,
        order_id: OrderId,
        code: StockId,
    },
    Replace {
        account: AccountId,
        old_order_id: OrderId,
        new_intent: Intent,
    },
}

impl NpcReconciliationDecision {
    #[cfg(test)]
    pub(in crate::session) const fn contract_parts(
        &self,
    ) -> (AccountId, OrderId, Option<&StockId>, Option<&Intent>) {
        match self {
            Self::Keep { account, order_id } => (*account, *order_id, None, None),
            Self::Cancel {
                account,
                order_id,
                code,
            } => (*account, *order_id, Some(code), None),
            Self::Replace {
                account,
                old_order_id,
                new_intent,
            } => (*account, *old_order_id, None, Some(new_intent)),
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionProjectionOutput {
    #[cfg(test)]
    accepted_due_npc_ids: Vec<AccountId>,
    reconciliation_decisions: Vec<NpcReconciliationDecision>,
    residual_intents: Vec<ProjectedNpcIntent>,
}

impl NpcDecisionProjectionOutput {
    #[cfg(test)]
    pub(in crate::session) fn accepted_due_npc_ids(&self) -> &[AccountId] {
        &self.accepted_due_npc_ids
    }

    /// Must be consumed before [`Self::residual_intents`].
    pub(in crate::session) fn reconciliation_decisions(&self) -> &[NpcReconciliationDecision] {
        &self.reconciliation_decisions
    }

    pub(in crate::session) fn residual_intents(&self) -> &[ProjectedNpcIntent] {
        &self.residual_intents
    }
}

#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum NpcDecisionProjectionError {
    #[error(
        "DecisionAndContinuation NPC 投影时钟不一致：shadow tick/phase {shadow_tick}/{shadow_phase:?}，snapshot {snapshot_tick}/{snapshot_phase:?}"
    )]
    ClockMismatch {
        shadow_tick: u64,
        shadow_phase: crate::TradingPhase,
        snapshot_tick: u64,
        snapshot_phase: crate::TradingPhase,
    },
    #[error("DecisionAndContinuation NPC 投影来源账户或 intent 顺序与密封快照不一致")]
    AccountOrderMismatch,
    #[error("DecisionAndContinuation NPC 投影缺少 shadow 账户 {0:?}")]
    MissingAccount(AccountId),
    #[error("DecisionAndContinuation NPC 投影账户 {0:?} 的 kind 在捕获后发生变化")]
    AccountKindMismatch(AccountId),
    #[error("DecisionAndContinuation NPC 投影账户 {0:?} 缺少 retail experience state")]
    MissingRetailExperience(AccountId),
    #[error("DecisionAndContinuation NPC 投影已移交账户 {0:?} 的 strategy state")]
    ConsumedStrategyState(AccountId),
}

/// 把纯 NPC 决策结果投影到调用方可丢弃的 tick shadow。
/// 失败时调用方必须丢弃整个 tick candidate，不能继续使用部分投影。
/// T1·两阶段（ADR-0042 §T1）：queue 链账户任务的共享投影上下文——协调线程
/// 捕获一次（工作单切片 O(到期账户×工作单)、竞价可撤窗口布尔），不随池宽
/// 放大；行情与持仓由任务经读视图直接读取，不在此复制。
pub(in crate::session) struct NpcAccountProjectionContext {
    pub(in crate::session) working_continuous: super::super::ContinuousOrdersByAccount,
    pub(in crate::session) working_auction: super::super::AuctionOrdersByAccount,
    pub(in crate::session) cancelable_auction_window: bool,
}

/// T1：账户任务的纯输出 patch——任务内完成本账户 attention 判定、个人
/// 观察/分析、策略决策与 quote/plan 个人投影的**全部纯计算**；协调线程
/// 归并阶段只做安装（接口规格：ADR-0042 §T1/审计 §五）。
pub(in crate::session) struct NpcAccountPatch {
    pub(in crate::session) account: AccountId,
    /// attention 判定是否受理观察。
    pub(in crate::session) observes: bool,
    /// 判定后的下一次 attention 候选 tick（个人 RNG 流已在任务内推进）。
    pub(in crate::session) next_attention_candidate_tick: u64,
    /// 机构/游资 attention 状态终值（归并时安装回写组；零售经 personal 携带）。
    pub(in crate::session) attention: Option<crate::session::attention::NpcAttentionState>,
    /// 零售个人计划状态终值（watchlist/价格记忆/信息/信念/历史读；内含
    /// attention；归并时 `install_on`）。
    pub(in crate::session) personal:
        Option<crate::session::decision_chain::personal_state::PlanPersonalState>,
    /// 策略替换（None=Unchanged）。归并时 `restore_strategy`。
    pub(in crate::session) strategy: Option<crate::account::StoredStrategy>,
    /// 散户决策 trace（Retail 且有 position_decision）。归并时 push。
    pub(in crate::session) retail_trace: Option<RetailDecisionTrace>,
    /// 个人经验行**终值**（观察更新 + 复核股票 observe + prune 均在任务内
/// 完成；归并时整体安装，无逐股计算）。
    pub(in crate::session) experience_row: Option<crate::RetailExperienceState>,
    /// 本账户有序 intents（reconciliation 撤单在前、residual 在后——与
    /// projected_ordered_intents 的账户内语义一致）。
    pub(in crate::session) ordered_intents: Vec<Intent>,
    /// 本账户内局部依赖（下标对；归并时按全局偏移平移）。
    pub(in crate::session) local_dependencies: Vec<(usize, usize)>,
}

/// T1：单账户决策+quote/plan 个人投影的纯计算部分（由账户任务在 decide
/// 之后调用）。不借用会话写组、不等待其他任务；错误显式字符串化（与
/// queue 链错误聚合口径一致）。
#[allow(clippy::too_many_arguments)]
pub(in crate::session) fn plan_npc_account_decision(
    shared: &super::decision_snapshot::DecisionSharedInputs<'_>,
    account: AccountId,
    kind: AccountKind,
    mut source: super::npc_decisions::NpcDecisionSourceOutput,
    ctx: &NpcAccountProjectionContext,
    view: &super::npc_split::SessionReadView<'_>,
    experience_row: &mut Option<crate::RetailExperienceState>,
) -> Result<NpcAccountDecisionPatch, String> {
    let phase = shared.phase;
    let market_minute = shared.market_minute;
    let (account_outputs, _raw_intents) = source.projection_parts();
    if account_outputs.len() != 1 || account_outputs[0].account() != account {
        return Err(NpcDecisionProjectionError::AccountOrderMismatch.to_string());
    }
    let account_output = &mut account_outputs[0];
    let strategy = match account_output
        .take_strategy()
        .ok_or(NpcDecisionProjectionError::ConsumedStrategyState(account).to_string())?
    {
        super::npc_decisions::NpcStrategyUpdate::Unchanged => None,
        super::npc_decisions::NpcStrategyUpdate::Replace(next) => Some(next),
    };
    let retail_trace = (kind == AccountKind::Retail)
        .then(|| {
            account_output.position_decision().map(|decision| RetailDecisionTrace {
                account,
                decision: decision.clone(),
                execution_urgency: account_output.execution_urgency().clone(),
            })
        })
        .flatten();
    // 经验行终值（任务内）：复核股票 observe_stock + 未持仓修剪——归并只安装。
    if kind == AccountKind::Retail {
        let row = experience_row
            .as_mut()
            .ok_or(NpcDecisionProjectionError::MissingRetailExperience(account).to_string())?;
        for code in account_output.reviewed_stocks() {
            row.observe_stock(code, market_minute);
        }
        let held: BTreeSet<StockId> = view
            .accounts
            .get(&account)
            .ok_or(NpcDecisionProjectionError::MissingAccount(account).to_string())?
            .positions()
            .keys()
            .cloned()
            .collect();
        row.prune_watchlist(&held);
    }
    // 本账户 raw intents + reconciliation（纯参数版本）+ 残差配对。
    let raw_intents = _raw_intents;
    let mut desired: Vec<Intent> = raw_intents
        .iter()
        .filter(|intent| {
            matches!(
                intent.key(),
                IntentCandidateKey::Npc { account: owner, .. } if *owner == account
            )
        })
        .map(|intent| intent.intent().clone())
        .collect();
    let mut cancel_intents: Vec<(StockId, OrderId)> = Vec::new();
    let working = WorkingOrderSlices {
        continuous: ctx
            .working_continuous
            .get(&account)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        auction: ctx
            .working_auction
            .get(&account)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    };
    if account_output.updates_working_quotes() {
        let scope = if kind == AccountKind::Retail {
            ReconcileScope::ReviewedStocks(account_output.reviewed_stocks().clone())
        } else {
            ReconcileScope::AllWorkingOrders
        };
        let plan = crate::session::execution::reconcile_plan::plan_npc_working_order_reconciliation_standalone(
            ctx.cancelable_auction_window,
            view.markets,
            desired,
            phase,
            scope,
            working,
        );
        cancel_intents = plan
            .decisions
            .iter()
            .filter_map(|decision| match decision {
                WorkingOrderDecision::Cancel { order_id, code } => Some((code.clone(), *order_id)),
                WorkingOrderDecision::Replace { old_order_id, new_intent } => {
                    let code = match new_intent {
                        Intent::PlaceLimit { code, .. }
                        | Intent::PlaceMarket { code, .. }
                        | Intent::Cancel { code, .. } => code.clone(),
                    };
                    Some((code, *old_order_id))
                }
                WorkingOrderDecision::Keep { .. } => None,
            })
            .collect();
        desired = plan.residual_intents;
    }
    // 有序化（projected_ordered_intents 的账户内语义）：撤单在前并作为
    // 同股 residual Place 的前驱；residual 保序追加。
    let mut ordered: Vec<Intent> = Vec::with_capacity(cancel_intents.len() + desired.len());
    let mut reconciliation_cancels: BTreeMap<StockId, Vec<usize>> = BTreeMap::new();
    for (code, order_id) in cancel_intents {
        reconciliation_cancels
            .entry(code.clone())
            .or_default()
            .push(ordered.len());
        ordered.push(Intent::Cancel { code, id: order_id });
    }
    let mut local_dependencies: Vec<(usize, usize)> = Vec::new();
    for projected_intent in desired {
        if let Intent::PlaceLimit { code, .. } | Intent::PlaceMarket { code, .. } =
            &projected_intent
        {
            if let Some(cancellations) = reconciliation_cancels.get(code) {
                local_dependencies
                    .extend(cancellations.iter().map(|before| (*before, ordered.len())));
            }
        }
        ordered.push(projected_intent);
    }
    Ok(NpcAccountDecisionPatch {
        strategy,
        retail_trace,
        experience_row: experience_row.take(),
        ordered_intents: ordered,
        local_dependencies,
    })
}

/// T1：`plan_npc_account_decision` 的输出（任务侧与 attention/个人状态
/// 合成最终 `NpcAccountPatch`）。
pub(in crate::session) struct NpcAccountDecisionPatch {
    pub(in crate::session) strategy: Option<crate::account::StoredStrategy>,
    pub(in crate::session) retail_trace: Option<RetailDecisionTrace>,
    pub(in crate::session) experience_row: Option<crate::RetailExperienceState>,
    pub(in crate::session) ordered_intents: Vec<Intent>,
    pub(in crate::session) local_dependencies: Vec<(usize, usize)>,
}

/// T1：归并侧安装（**纯安装，零逐账户计算**）——attention 重排、个人计划
/// 状态、attention 状态、策略、trace、经验行终值一次落位；ingress 与批次
/// 组装由 queue 链在确定性安装序中完成。
pub(in crate::session) fn install_npc_account_patch(
    writes: &mut super::npc_split::NpcWriteFields<'_>,
    patch: &mut NpcAccountPatch,
) -> Result<(), NpcDecisionProjectionError> {
    // 观察与未观察候选都重排（个人 RNG 流已在任务内推进）。
    writes
        .attention_scheduler
        .enqueue(patch.next_attention_candidate_tick, patch.account);
    if let Some(personal) = patch.personal.take() {
        personal.install_on(writes, patch.account);
    }
    if let Some(attention) = patch.attention.take() {
        assert!(
            writes.npc_attention.insert(patch.account, attention).is_none(),
            "queue 链账户 {account:?} 的 attention 状态必须恰安装一次",
            account = patch.account
        );
    }
    if let Some(next_strategy) = patch.strategy.take() {
        writes
            .accounts
            .get_mut(&patch.account)
            .ok_or(NpcDecisionProjectionError::MissingAccount(patch.account))?
            .restore_strategy(Some(next_strategy));
    }
    if let Some(trace) = patch.retail_trace.take() {
        writes.last_retail_decisions.push(trace);
    }
    if let Some(row) = patch.experience_row.take() {
        // 观察路径已 take 原行；此处回装终值（恰一次）。
        writes.retail_experience.insert(patch.account, row);
    }
    Ok(())
}

pub(in crate::session) fn project_npc_state(
    shadow: &mut GameSession,
    captured: &CapturedDecisionSnapshot,
    source: &mut NpcDecisionSourceOutput,
) -> Result<NpcDecisionProjectionOutput, NpcDecisionProjectionError> {
    let snapshot = &captured.snapshot;
    if shadow.state.tick != snapshot.tick() || shadow.phase() != snapshot.phase() {
        return Err(NpcDecisionProjectionError::ClockMismatch {
            shadow_tick: shadow.state.tick,
            shadow_phase: shadow.phase(),
            snapshot_tick: snapshot.tick(),
            snapshot_phase: snapshot.phase(),
        });
    }
    if snapshot.due_npc_ids() != source.accounts()
        || source
            .account_outputs()
            .iter()
            .map(|output| output.account())
            .ne(source.accounts().iter().copied())
    {
        return Err(NpcDecisionProjectionError::AccountOrderMismatch);
    }

    project_npc_state_in_place(shadow, captured, source, false)
}

/// 将一个已完成的 NPC 账户结果投影到可丢弃 tick shadow。
/// 调用方拥有账户完成 channel；任一账户投影失败都必须丢弃整个 candidate。
pub(in crate::session) fn project_npc_account(
    shadow: &mut GameSession,
    captured: &CapturedDecisionSnapshot,
    source: &mut NpcDecisionSourceOutput,
) -> Result<NpcDecisionProjectionOutput, NpcDecisionProjectionError> {
    let (view, mut writes) = super::npc_split::npc_split(shadow);
    project_npc_account_on(&view, &mut writes, captured, source)
}

/// 读视图+写组版本（queue 链转换目标形态）。
pub(in crate::session) fn project_npc_account_on(
    view: &super::npc_split::SessionReadView<'_>,
    writes: &mut super::npc_split::NpcWriteFields<'_>,
    captured: &CapturedDecisionSnapshot,
    source: &mut NpcDecisionSourceOutput,
) -> Result<NpcDecisionProjectionOutput, NpcDecisionProjectionError> {
    let snapshot = &captured.snapshot;
    if view.tick != snapshot.tick() || view.phase() != snapshot.phase() {
        return Err(NpcDecisionProjectionError::ClockMismatch {
            shadow_tick: view.tick,
            shadow_phase: view.phase(),
            snapshot_tick: snapshot.tick(),
            snapshot_phase: snapshot.phase(),
        });
    }
    if source.accounts().len() != 1
        || source.account_outputs().len() != 1
        || source.accounts()[0] != source.account_outputs()[0].account()
        || !snapshot.due_npc_ids().contains(&source.accounts()[0])
    {
        return Err(NpcDecisionProjectionError::AccountOrderMismatch);
    }
    project_npc_state_in_place_on(view, writes, captured, source, true)
}

fn project_npc_state_in_place(
    shadow: &mut GameSession,
    captured: &CapturedDecisionSnapshot,
    source: &mut NpcDecisionSourceOutput,
    project_single_account: bool,
) -> Result<NpcDecisionProjectionOutput, NpcDecisionProjectionError> {
    let (view, mut writes) = super::npc_split::npc_split(shadow);
    project_npc_state_in_place_on(&view, &mut writes, captured, source, project_single_account)
}

/// 读视图+写组版本。
fn project_npc_state_in_place_on(
    view: &super::npc_split::SessionReadView<'_>,
    writes: &mut super::npc_split::NpcWriteFields<'_>,
    captured: &CapturedDecisionSnapshot,
    source: &mut NpcDecisionSourceOutput,
    project_single_account: bool,
) -> Result<NpcDecisionProjectionOutput, NpcDecisionProjectionError> {
    let snapshot = &captured.snapshot;
    let phase = snapshot.phase();
    let market_minute = snapshot.market_minute();
    let working_continuous = &captured.working_continuous;
    let working_auction = &captured.working_auction;
    let mut decisions = Vec::new();
    let mut residual = Vec::new();
    let mut retail_reviews = Vec::new();
    let (account_outputs, raw_intents) = source.projection_parts();
    let mut source_intents = raw_intents.iter().peekable();
    for account_output in account_outputs {
        let account = account_output.account();
        let sealed = snapshot
            .account(account)
            .map_err(|_| NpcDecisionProjectionError::MissingAccount(account))?;
        let entry = view
            .accounts
            .get(&account)
            .ok_or(NpcDecisionProjectionError::MissingAccount(account))?;
        let account_kind = entry.kind();
        if account_kind != sealed.kind() {
            return Err(NpcDecisionProjectionError::AccountKindMismatch(account));
        }
        match account_output
            .take_strategy()
            .ok_or(NpcDecisionProjectionError::ConsumedStrategyState(account))?
        {
            NpcStrategyUpdate::Unchanged => {}
            NpcStrategyUpdate::Replace(next_strategy) => {
                writes
                    .accounts
                    .get_mut(&account)
                    .expect("the account was checked above")
                    .restore_strategy(Some(next_strategy));
            }
        }
        if account_kind == AccountKind::Retail {
            if let Some(position_decision) = account_output.position_decision() {
                writes.last_retail_decisions.push(RetailDecisionTrace {
                    account,
                    decision: position_decision.clone(),
                    execution_urgency: account_output.execution_urgency().clone(),
                });
            }
            retail_reviews.push((account, account_output.reviewed_stocks().clone()));
        }
        let mut raw = Vec::new();
        while matches!(
            source_intents.peek().map(|intent| intent.key()),
            Some(IntentCandidateKey::Npc { account: owner, .. }) if *owner == account
        ) {
            let intent = source_intents
                .next()
                .expect("peeked source intent is present");
            raw.push((intent.key().clone(), intent.intent().clone()));
        }
        let mut desired: Vec<_> = raw.iter().map(|(_, intent)| intent.clone()).collect();
        let mut unused_raw = raw;
        let working = WorkingOrderSlices {
            continuous: working_continuous
                .get(&account)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            auction: working_auction
                .get(&account)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
        };
        if account_output.updates_working_quotes() {
            let scope = if account_kind == AccountKind::Retail {
                ReconcileScope::ReviewedStocks(account_output.reviewed_stocks().clone())
            } else {
                ReconcileScope::AllWorkingOrders
            };
            let plan = crate::GameSession::plan_npc_working_order_reconciliation_on(
                view, desired, phase, scope, working,
            );
            for decision in &plan.decisions {
                let WorkingOrderDecision::Keep { order_id } = decision else {
                    continue;
                };
                if let Some(kept) = working_intent(*order_id, phase, working) {
                    remove_first_matching_raw(&mut unused_raw, &kept);
                }
            }
            decisions.extend(
                plan.decisions
                    .into_iter()
                    .map(|decision| project_reconciliation_decision(account, decision)),
            );
            desired = plan.residual_intents;
        }

        for projected_intent in desired {
            let source = take_exact_raw_source(&mut unused_raw, &projected_intent);
            residual.push(ProjectedNpcIntent {
                account,
                source_key: source.as_ref().map(|(key, _)| key.clone()),
                #[cfg(test)]
                source_intent: source.map(|(_, intent)| intent),
                projected_intent,
            });
        }
    }
    if source_intents.next().is_some() {
        return Err(NpcDecisionProjectionError::AccountOrderMismatch);
    }
    if project_single_account {
        for (account, reviewed) in retail_reviews {
            let held: BTreeSet<_> = view.accounts[&account]
                .positions()
                .keys()
                .cloned()
                .collect();
            let experience = writes
                .retail_experience
                .get_mut(&account)
                .ok_or(NpcDecisionProjectionError::MissingRetailExperience(account))?;
            for code in &reviewed {
                experience.observe_stock(code, market_minute);
            }
            experience.prune_watchlist(&held);
        }
    } else {
        let review_ids = retail_reviews
            .iter()
            .map(|(account, _)| *account)
            .collect::<Vec<_>>();
        let reviews = retail_reviews
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        let accounts = &view.accounts;
        view.compute.parallel(|| {
            writes.retail_experience.mutate_existing_parallel(
                &review_ids,
                NpcDecisionProjectionError::MissingRetailExperience,
                |account, experience| {
                    for code in &reviews[&account] {
                        experience.observe_stock(code, market_minute);
                    }
                    let held: BTreeSet<_> =
                        accounts[&account].positions().keys().cloned().collect();
                    experience.prune_watchlist(&held);
                    Ok(())
                },
            )
        })?;
    }
    Ok(NpcDecisionProjectionOutput {
        #[cfg(test)]
        accepted_due_npc_ids: source.accounts().to_vec(),
        reconciliation_decisions: decisions,
        residual_intents: residual,
    })
}

fn working_intent(
    order_id: OrderId,
    phase: crate::TradingPhase,
    working: WorkingOrderSlices<'_>,
) -> Option<Intent> {
    match phase {
        crate::TradingPhase::Continuous => working.continuous.iter().find_map(|(code, order)| {
            (order.id == order_id).then(|| Intent::PlaceLimit {
                code: code.clone(),
                side: order.side,
                price: LimitPrice::Fixed(order.price),
                qty: order.qty,
            })
        }),
        crate::TradingPhase::CallAuction => working.auction.iter().find_map(|(code, order)| {
            (OrderId(order.order_id) == order_id).then(|| Intent::PlaceLimit {
                code: code.clone(),
                side: order.side,
                price: LimitPrice::Fixed(order.limit),
                qty: order.qty,
            })
        }),
        crate::TradingPhase::ClosingAuction | crate::TradingPhase::PreOpen => None,
    }
}

pub(super) fn remove_first_matching_raw(
    raw: &mut Vec<(IntentCandidateKey, Intent)>,
    consumed: &Intent,
) {
    if let Some(index) = raw
        .iter()
        .position(|(_, intent)| intents_equal(intent, consumed))
    {
        raw.remove(index);
    }
}

pub(super) fn take_exact_raw_source(
    raw: &mut Vec<(IntentCandidateKey, Intent)>,
    projected: &Intent,
) -> Option<(IntentCandidateKey, Intent)> {
    raw.iter()
        .position(|(_, intent)| intents_equal(intent, projected))
        .map(|index| raw.remove(index))
}

fn project_reconciliation_decision(
    account: AccountId,
    decision: WorkingOrderDecision,
) -> NpcReconciliationDecision {
    match decision {
        WorkingOrderDecision::Keep {
            #[cfg(test)]
            order_id,
            #[cfg(not(test))]
                order_id: _,
        } => NpcReconciliationDecision::Keep {
            #[cfg(test)]
            account,
            #[cfg(test)]
            order_id,
        },
        WorkingOrderDecision::Cancel { order_id, code } => NpcReconciliationDecision::Cancel {
            account,
            order_id,
            code,
        },
        WorkingOrderDecision::Replace {
            old_order_id,
            new_intent,
        } => NpcReconciliationDecision::Replace {
            account,
            old_order_id,
            new_intent,
        },
    }
}

fn intents_equal(left: &Intent, right: &Intent) -> bool {
    match (left, right) {
        (
            Intent::PlaceLimit {
                code: left_code,
                side: left_side,
                price: left_price,
                qty: left_qty,
            },
            Intent::PlaceLimit {
                code: right_code,
                side: right_side,
                price: right_price,
                qty: right_qty,
            },
        ) => {
            left_code == right_code
                && left_side == right_side
                && left_price == right_price
                && left_qty == right_qty
        }
        (
            Intent::PlaceMarket {
                code: left_code,
                side: left_side,
                qty: left_qty,
            },
            Intent::PlaceMarket {
                code: right_code,
                side: right_side,
                qty: right_qty,
            },
        ) => left_code == right_code && left_side == right_side && left_qty == right_qty,
        (
            Intent::Cancel {
                code: left_code,
                id: left_id,
            },
            Intent::Cancel {
                code: right_code,
                id: right_id,
            },
        ) => left_code == right_code && left_id == right_id,
        _ => false,
    }
}

#[cfg(test)]
mod auction_identity_tests {
    use super::*;
    use crate::session::AuctionOrderSnap;
    use crate::TradingPhase;
    use crate::{Money, Side};

    #[test]
    fn kept_auction_order_consumes_its_raw_quote_by_order_id() {
        let account = AccountId(1);
        let code = StockId(0);
        let auction = [(
            code.clone(),
            AuctionOrderSnap {
                owner: account,
                side: Side::Buy,
                limit: Money::from_cents(1_000),
                qty: 100,
                order_id: 77,
            },
        )];
        let working = || WorkingOrderSlices {
            continuous: &[],
            auction: &auction,
        };
        let kept = working_intent(OrderId(77), TradingPhase::CallAuction, working())
            .expect("the queued order ID must find the matching quote");
        assert!(working_intent(OrderId(0), TradingPhase::CallAuction, working()).is_none());
        let mut raw = vec![(
            IntentCandidateKey::npc(account, 0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )];

        remove_first_matching_raw(&mut raw, &kept);

        assert!(raw.is_empty(), "a kept quote cannot be submitted again");
    }
}
