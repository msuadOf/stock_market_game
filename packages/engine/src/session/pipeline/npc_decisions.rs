//! 运行纯 NPC 决策源。
use super::{DecisionSnapshot, DecisionSnapshotError, IntentCandidateKey};
use crate::account::StoredStrategy;
use crate::strategy::{Intent, StrategyDecision, StrategyState, StrategyStateError};
use crate::{AccountId, SplitMix64};
use std::collections::BTreeSet;
use std::sync::{mpsc, Arc};

/// NPC raw intent 携带身份；候选 composition 必须保留该身份。
#[derive(Clone, Debug, serde::Serialize)]
pub(in crate::session) struct NpcDecisionIntent {
    key: IntentCandidateKey,
    intent: Intent,
}

impl NpcDecisionIntent {
    pub(in crate::session) const fn key(&self) -> &IntentCandidateKey {
        &self.key
    }

    pub(in crate::session) const fn intent(&self) -> &Intent {
        &self.intent
    }
}

/// The prospective, non-routing decision result for one NPC account.
#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionAccountOutput {
    account: AccountId,
    #[cfg(test)]
    strategy_state: Option<StrategyState>,
    strategy: Option<NpcStrategyUpdate>,
    reviewed_stocks: BTreeSet<crate::StockCode>,
    position_decision: Option<crate::behavior::PositionDecision>,
    execution_urgency: crate::plans::urgency::risk::RiskUrgencyAssessment,
    updates_working_quotes: bool,
    uses_parent_order_execution: bool,
}

#[derive(Clone, Debug)]
pub(in crate::session) enum NpcStrategyUpdate {
    Unchanged,
    Replace(StoredStrategy),
}

impl NpcDecisionAccountOutput {
    pub(in crate::session) const fn account(&self) -> AccountId {
        self.account
    }

    #[cfg(test)]
    pub(in crate::session) const fn strategy_state(&self) -> Option<&StrategyState> {
        self.strategy_state.as_ref()
    }

    pub(in crate::session) fn take_strategy(&mut self) -> Option<NpcStrategyUpdate> {
        #[cfg(test)]
        self.strategy_state.take()?;
        self.strategy.take()
    }

    pub(in crate::session) const fn reviewed_stocks(&self) -> &BTreeSet<crate::StockCode> {
        &self.reviewed_stocks
    }

    pub(in crate::session) const fn position_decision(
        &self,
    ) -> Option<&crate::behavior::PositionDecision> {
        self.position_decision.as_ref()
    }

    pub(in crate::session) fn execution_urgency(
        &self,
    ) -> &crate::plans::urgency::risk::RiskUrgencyAssessment {
        &self.execution_urgency
    }

    pub(in crate::session) const fn updates_working_quotes(&self) -> bool {
        self.updates_working_quotes
    }

    pub(in crate::session) const fn uses_parent_order_execution(&self) -> bool {
        self.uses_parent_order_execution
    }
}

/// 纯 NPC 决策结果只含 prospective strategy states 与 raw intents。
///
/// 不持有事件、router、订单簿、envelope 或会话句柄。
#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionSourceOutput {
    account_ids: Vec<AccountId>,
    accounts: Vec<NpcDecisionAccountOutput>,
    intents: Vec<NpcDecisionIntent>,
}

impl NpcDecisionSourceOutput {
    pub(in crate::session) fn accounts(&self) -> &[AccountId] {
        &self.account_ids
    }

    pub(in crate::session) fn intents(&self) -> &[NpcDecisionIntent] {
        &self.intents
    }

    pub(in crate::session) fn account_outputs(&self) -> &[NpcDecisionAccountOutput] {
        &self.accounts
    }

    pub(in crate::session) fn projection_parts(
        &mut self,
    ) -> (&mut [NpcDecisionAccountOutput], &[NpcDecisionIntent]) {
        (&mut self.accounts, &self.intents)
    }

    #[cfg(test)]
    pub(in crate::session) fn strategy_state(
        &self,
        account: AccountId,
    ) -> Result<&StrategyState, NpcDecisionSourceError> {
        self.accounts
            .iter()
            .find(|output| output.account == account)
            .and_then(NpcDecisionAccountOutput::strategy_state)
            .ok_or(NpcDecisionSourceError::MissingOutputState(account))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(in crate::session) enum NpcDecisionSourceError {
    #[error("DecisionShadow NPC 账户 {account:?} 的 risk urgency 失败：{source}")]
    RiskUrgency {
        account: AccountId,
        source: crate::plans::UrgencyError,
    },
    #[error("DecisionShadow NPC 来源快照错误：{0}")]
    Snapshot(DecisionSnapshotError),
    #[error("DecisionShadow NPC 来源无法恢复账户 {account:?} 的 strategy：{source}")]
    StrategyHydration {
        account: AccountId,
        source: StrategyStateError,
    },
    #[error("DecisionShadow NPC 来源账户 {account:?} 的 intent ordinal 溢出")]
    IntentOrdinalOverflow { account: AccountId },
    #[error("DecisionShadow NPC 来源账户 {0:?} 缺少输出 strategy state")]
    #[cfg(test)]
    MissingOutputState(AccountId),
}

pub(in crate::session) fn npc_rng_seed(base: u64, tick: u64, account: AccountId) -> u64 {
    base ^ tick.wrapping_mul(0x9E3779B97F4A7C15) ^ account.0.wrapping_mul(0x6A09E667F3BCC908)
}

pub(in crate::session) struct NpcAccountDecision {
    #[cfg(test)]
    pub(in crate::session) account: AccountId,
    pub(in crate::session) output: NpcDecisionSourceOutput,
}

#[derive(Debug)]
pub(in crate::session) enum NpcDecisionStreamError<E> {
    Decision(NpcDecisionSourceError),
    Consumer(E),
}

fn decide_npc_account(
    snapshot: &DecisionSnapshot,
    config: &crate::GameConfig,
    account: AccountId,
) -> Result<NpcDecisionSourceOutput, NpcDecisionSourceError> {
    let input = snapshot
        .account(account)
        .map_err(NpcDecisionSourceError::Snapshot)?;
    let mut strategy = input
        .strategy_state()
        .clone()
        .into_strategy()
        .map_err(|source| NpcDecisionSourceError::StrategyHydration { account, source })?;
    let npc_seed = npc_rng_seed(snapshot.npc_seed_base(), snapshot.tick(), account);
    let mut rng = SplitMix64::new(npc_seed);
    let dated_experience = input.failure_influence().map(|influence| {
        let mut experience = input
            .retail_experience()
            .expect("dated failure influence requires retail experience")
            .clone();
        experience.consecutive_failed_buys = influence;
        experience
    });
    let experience = dated_experience
        .as_ref()
        .or_else(|| input.retail_experience());
    let mut decision = strategy.decide_with_experience(
        snapshot.market(),
        input.self_view(),
        (input.kind() == crate::AccountKind::Retail)
            .then(|| snapshot.behavior_market())
            .flatten(),
        (input.kind() == crate::AccountKind::Retail)
            .then(|| input.account_risk())
            .flatten(),
        experience,
        snapshot.market_minute(),
        &mut rng,
        config,
    );
    if let Some(analysis) = input.retail_analysis() {
        let StrategyState::ZiNoise(retail) = input.strategy_state() else {
            panic!("retail analysis requires the retail execution strategy for {account:?}");
        };
        retail.apply_personal_analysis(
            &mut decision,
            analysis,
            snapshot.market(),
            input.self_view(),
            input
                .account_risk()
                .expect("retail analysis requires account risk"),
            experience.expect("retail analysis requires personal experience"),
            snapshot.market_minute(),
            &mut rng,
            config,
        );
    }
    let execution_urgency = crate::plans::urgency::risk::assess_personal_risk_urgency(
        decision.position_decision.as_ref(),
        input.account_risk(),
        snapshot.urgency_policy(),
    )
    .map_err(|source| NpcDecisionSourceError::RiskUrgency { account, source })?;
    let strategy_state = StrategyState::from_strategy(strategy.as_ref())
        .map_err(|source| NpcDecisionSourceError::StrategyHydration { account, source })?;
    let unchanged = &strategy_state == input.strategy_state();
    let StrategyDecision {
        intents: local_intents,
        reviewed_stocks,
        position_decision,
    } = decision;
    let updates_working_quotes = !local_intents.is_empty()
        || !reviewed_stocks.is_empty()
        || strategy.updates_working_quotes_on_empty_decision();
    let uses_parent_order_execution = strategy.uses_parent_order_execution();
    let mut intents = Vec::with_capacity(local_intents.len());
    for (npc_local_index, intent) in local_intents.into_iter().enumerate() {
        let npc_local_index = u64::try_from(npc_local_index)
            .map_err(|_| NpcDecisionSourceError::IntentOrdinalOverflow { account })?;
        intents.push(NpcDecisionIntent {
            key: IntentCandidateKey::npc(account, npc_local_index),
            intent,
        });
    }
    let account_output = NpcDecisionAccountOutput {
        account,
        #[cfg(test)]
        strategy_state: Some(strategy_state.clone()),
        strategy: Some(if unchanged {
            NpcStrategyUpdate::Unchanged
        } else {
            NpcStrategyUpdate::Replace(StoredStrategy::production_validated(
                strategy,
                strategy_state,
            ))
        }),
        reviewed_stocks,
        position_decision,
        execution_urgency,
        updates_working_quotes,
        uses_parent_order_execution,
    };
    Ok(NpcDecisionSourceOutput {
        account_ids: vec![account],
        accounts: vec![account_output],
        intents,
    })
}

pub(in crate::session) fn stream_npc_decisions<E>(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
    receive: impl FnMut(NpcAccountDecision) -> Result<(), E> + Send,
) -> Result<(), NpcDecisionStreamError<E>>
where
    E: Send,
{
    stream_npc_decisions_with(snapshot, config, |_| {}, receive)
}

pub(super) fn stream_npc_decisions_with<E, B, R>(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
    before_decide: B,
    receive: R,
) -> Result<(), NpcDecisionStreamError<E>>
where
    E: Send,
    B: Fn(AccountId) + Sync + Send,
    R: FnMut(NpcAccountDecision) -> Result<(), E> + Send,
{
    let inline = rayon::current_num_threads() == 1
        && (rayon::current_thread_index().is_some() || cfg!(target_arch = "wasm32"));
    #[cfg(target_arch = "wasm32")]
    let inline = inline || rayon::current_thread_index().is_some();
    if inline {
        return stream_npc_decisions_inline(snapshot, config, before_decide, receive);
    }

    let (sender, receiver) = mpsc::channel();
    #[cfg(not(target_arch = "wasm32"))]
    {
        return std::thread::scope(|threads| {
            let account_count = snapshot.due_npc_ids().len();
            let consumer = threads
                .spawn(move || consume_npc_decision_results(receiver, account_count, receive));
            rayon::scope(|scope| {
                for account in snapshot.due_npc_ids().iter().copied() {
                    let sender = sender.clone();
                    let snapshot = snapshot.clone();
                    let before_decide = &before_decide;
                    scope.spawn(move |_| {
                        before_decide(account);
                        let result = decide_npc_account(&snapshot, config, account);
                        let _ = sender.send((account, result));
                    });
                }
                drop(sender);
            });
            consumer
                .join()
                .expect("NPC completion consumer thread must not panic")
        });
    }
    #[cfg(target_arch = "wasm32")]
    rayon::in_place_scope(|scope| {
        for account in snapshot.due_npc_ids().iter().copied() {
            let sender = sender.clone();
            let snapshot = snapshot.clone();
            let before_decide = &before_decide;
            scope.spawn(move |_| {
                before_decide(account);
                let result = decide_npc_account(&snapshot, config, account);
                let _ = sender.send((account, result));
            });
        }
        drop(sender);
        consume_npc_decision_results(receiver, snapshot.due_npc_ids().len(), receive)
    })
}

fn stream_npc_decisions_inline<E>(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
    before_decide: impl Fn(AccountId),
    mut receive: impl FnMut(NpcAccountDecision) -> Result<(), E>,
) -> Result<(), NpcDecisionStreamError<E>> {
    let mut errors = Vec::new();
    for account in snapshot.due_npc_ids().iter().copied() {
        before_decide(account);
        record_stream_result(
            account,
            decide_npc_account(&snapshot, config, account),
            &mut receive,
            &mut errors,
        );
    }
    select_stream_error(errors)
}

fn consume_npc_decision_results<E>(
    receiver: mpsc::Receiver<(
        AccountId,
        Result<NpcDecisionSourceOutput, NpcDecisionSourceError>,
    )>,
    count: usize,
    mut receive: impl FnMut(NpcAccountDecision) -> Result<(), E>,
) -> Result<(), NpcDecisionStreamError<E>> {
    let mut errors = Vec::new();
    for _ in 0..count {
        let (account, result) = receiver
            .recv()
            .expect("all NPC decision workers send one result");
        record_stream_result(account, result, &mut receive, &mut errors);
    }
    select_stream_error(errors)
}

fn record_stream_result<E>(
    account: AccountId,
    result: Result<NpcDecisionSourceOutput, NpcDecisionSourceError>,
    receive: &mut impl FnMut(NpcAccountDecision) -> Result<(), E>,
    errors: &mut Vec<(AccountId, NpcDecisionStreamError<E>)>,
) {
    match result {
        Ok(output) => {
            if let Err(error) = receive(NpcAccountDecision {
                #[cfg(test)]
                account,
                output,
            }) {
                errors.push((account, NpcDecisionStreamError::Consumer(error)));
            }
        }
        Err(error) => errors.push((account, NpcDecisionStreamError::Decision(error))),
    }
}

fn select_stream_error<E>(
    mut errors: Vec<(AccountId, NpcDecisionStreamError<E>)>,
) -> Result<(), NpcDecisionStreamError<E>> {
    errors.sort_by_key(|(account, _)| *account);
    match errors.into_iter().next() {
        Some((_, error)) => Err(error),
        None => Ok(()),
    }
}

/// 测试适配器：并行评估密封的 NPC 策略，且不修改权威状态。
///
/// 每账户在同一并行任务中恢复策略并计算决策；结果按账户规范顺序聚合，
/// 因此错误选择稳定，且局部结果不会提前逃逸。每账户 RNG 不依赖调度顺序。
#[cfg(test)]
pub(in crate::session) fn run_npc_decisions(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
) -> Result<NpcDecisionSourceOutput, NpcDecisionSourceError> {
    let mut outputs = Vec::with_capacity(snapshot.due_npc_ids().len());
    stream_npc_decisions(snapshot, config, |result| {
        outputs.push(result.output);
        Ok::<_, std::convert::Infallible>(())
    })
    .map_err(|error| match error {
        NpcDecisionStreamError::Decision(error) => error,
        NpcDecisionStreamError::Consumer(never) => match never {},
    })?;
    outputs.sort_by_key(|output| output.accounts[0].account());
    let mut accounts = Vec::with_capacity(outputs.len());
    let mut intents = Vec::new();
    for mut output in outputs {
        accounts.append(&mut output.accounts);
        intents.append(&mut output.intents);
    }
    let account_ids = accounts
        .iter()
        .map(NpcDecisionAccountOutput::account)
        .collect();
    Ok(NpcDecisionSourceOutput {
        account_ids,
        accounts,
        intents,
    })
}
