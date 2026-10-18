//! 为 Continuous worker、receipt 与结算构建独立原子 candidate。
//!
//! 本模块消费股票 worker 输出，在私有权威 shadows 上运行 ReceiptAggregation 与 Settlement，
//! 返回供后续 CommitTick 共同安装的 candidate；不修改输入权威状态。

use super::{
    account_settlement::{
        prepare_settlement_transaction_with_beliefs, SettlementTransactionError,
        SettlementTransactionOutput,
    },
    continuous_matching::{
        ContinuousCancelFact, ContinuousPlaceFact, ContinuousStockOutput, ContinuousTradeFact,
    },
    receipt_aggregation::apply_owned_receipt_transaction,
    retail_projection::RetailProjectionSeen,
    EnvelopeLedger, EnvelopeReceipt, StepFatal,
};
use crate::session::{account_book::AccountBook, account_paged_map::AccountPagedMap};
use crate::{Account, AccountId, Market, RetailExperienceState, StockId};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub(super) enum StockExecutionTransactionError {
    #[error("StockProcessing 为股票 {code:?} 产生了多个 Continuous worker 输出")]
    DuplicateStockWorker { code: StockId },
    #[error("ReceiptAggregation receipt 事务失败：{0}")]
    ReceiptAggregation(#[source] StepFatal),
    #[error("SettlementShadow 结算事务失败：{0}")]
    Settlement(#[source] SettlementTransactionError),
}

/// 由确定性身份标识的同一个逐股 worker 拥有股票处理事实及其市场结果。
/// Luna 快检（T5 八十·1）：本载体与下方输出载体经生产汇合点（八十）收编后
/// **仅测试组合面使用**——纳入 cfg(test)（非测试构建不编译）。
#[cfg(test)]
pub(super) struct StockExecutionOutput {
    pub(super) market: Market,
    pub(super) trades: Vec<ContinuousTradeFact>,
    pub(super) place_facts: Vec<ContinuousPlaceFact>,
    pub(super) cancel_facts: Vec<ContinuousCancelFact>,
}

/// 同一 detached candidate 覆盖 StockProcessing、ReceiptAggregation 与 Settlement。
///
/// 全部容器必须由 CommitTick 共同安装。返回 candidate 而不修改输入，
/// 确保 Settlement 失败时，已成功的 ReceiptAggregation 账本更新不会对外可见。
#[cfg(test)]
pub(super) struct StockExecutionTransactionOutput {
    pub(super) ledger: EnvelopeLedger,
    pub(super) account_patch: BTreeMap<AccountId, Account>,
    pub(super) retail_patch: BTreeMap<AccountId, RetailExperienceState>,
    pub(super) belief_patch: BTreeMap<AccountId, crate::strategy::BeliefBook>,
    pub(super) seen: RetailProjectionSeen,
    pub(super) stocks: BTreeMap<StockId, StockExecutionOutput>,
    pub(super) receipts: super::fill_fact::ReceiptBatch,
    pub(super) settlement: SettlementTransactionOutput,
}

#[cfg(test)]
pub(super) struct SettlementApplicationContext<'receipt> {
    moment: crate::experience::ExperienceMoment,
    preceding_receipts: &'receipt [EnvelopeReceipt],
    t1_enabled: bool,
}

#[cfg(test)]
impl<'receipt> SettlementApplicationContext<'receipt> {
    pub(super) const fn new(
        moment: crate::experience::ExperienceMoment,
        preceding_receipts: &'receipt [EnvelopeReceipt],
        t1_enabled: bool,
    ) -> Self {
        Self {
            moment,
            preceding_receipts,
            t1_enabled,
        }
    }
}

/// 在私有 candidate 上依次应用 Continuous worker 输出、receipt 聚合与结算。
///
/// 股票身份来自 worker 结果的 Market，而非 workers 中的位置。
/// ReceiptAggregation 是唯一 receipt index authority；Settlement 是唯一账户结算与散户投影 authority。
/// T5（批次八十）：生产包装层退役后，测试组合面**自足**——同构内联三段
/// （worker 拆包→receipt 聚合→结算）的参数化组合核，供
/// `session_execution_transaction`（测试面）与简参测试入口共用。
#[cfg(test)]
pub(super) fn compose_stock_execution_transaction_for_test(
    compute: &std::sync::Arc<crate::ComputeContext>,
    ledger: &EnvelopeLedger,
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    belief_participants: &AccountPagedMap<
        crate::session::decision_chain::personal_state::BeliefParticipantState,
    >,
    seen: &RetailProjectionSeen,
    workers: Vec<ContinuousStockOutput>,
    context: SettlementApplicationContext<'_>,
) -> Result<StockExecutionTransactionOutput, StockExecutionTransactionError> {
    use super::account_settlement::prepare_settlement_transaction_with_beliefs;
    use super::receipt_aggregation::apply_owned_receipt_transaction;
    let mut stocks = BTreeMap::new();
    let mut created_envelopes = Vec::new();
    let mut worker_batches = Vec::new();
    let mut terminal_keys = Vec::new();
    for worker in workers {
        let code = worker.market.code().clone();
        if stocks.contains_key(&code) {
            return Err(StockExecutionTransactionError::DuplicateStockWorker { code });
        }
        created_envelopes.extend(worker.created_envelopes);
        worker_batches.push(worker.receipts);
        terminal_keys.extend(worker.terminal_keys);
        stocks.insert(
            code,
            StockExecutionOutput {
                market: worker.market,
                trades: worker.trades,
                place_facts: worker.place_facts,
                cancel_facts: worker.cancel_facts,
            },
        );
    }
    let (ledger_candidate, receipts) = apply_owned_receipt_transaction(
        ledger.clone(),
        created_envelopes,
        worker_batches,
        terminal_keys,
        compute,
    )
    .map_err(StockExecutionTransactionError::ReceiptAggregation)?;
    let settlement_input = receipts
        .prepend_releases(context.preceding_receipts)
        .map_err(StockExecutionTransactionError::ReceiptAggregation)?;
    let prepared = prepare_settlement_transaction_with_beliefs(
        compute,
        accounts,
        retail_experience,
        belief_participants,
        seen,
        context.moment,
        &settlement_input,
        context.t1_enabled,
    )
    .map_err(StockExecutionTransactionError::Settlement)?;
    Ok(StockExecutionTransactionOutput {
        ledger: ledger_candidate,
        account_patch: prepared.account_patch,
        retail_patch: prepared.retail_patch,
        belief_patch: prepared.belief_patch,
        seen: prepared.seen,
        stocks,
        receipts,
        settlement: prepared.output,
    })
}

/// 简参测试入口（既有 6 例收据/结算语义钉）。
#[cfg(test)]
pub(super) fn apply_stock_execution_transaction(
    ledger: &EnvelopeLedger,
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    seen: &RetailProjectionSeen,
    market_minute: u64,
    workers: Vec<ContinuousStockOutput>,
    t1_enabled: bool,
) -> Result<StockExecutionTransactionOutput, StockExecutionTransactionError> {
    compose_stock_execution_transaction_for_test(
        &crate::ComputeContext::shared_default().expect("测试计算上下文"),
        ledger,
        accounts,
        retail_experience,
        &AccountPagedMap::default(),
        seen,
        workers,
        SettlementApplicationContext::new(test_experience_moment(market_minute), &[], t1_enabled),
    )
}

fn test_experience_moment(market_minute: u64) -> crate::experience::ExperienceMoment {
    crate::experience::ExperienceMoment {
        civil_date: crate::calendar::CivilDate::from_ymd(2030, 1, 1).expect("test date is valid"),
        market_minute,
        trading_day: market_minute,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::stock_execution_transaction".to_owned(),
    }
}
