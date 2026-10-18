use super::{
    conservation::{ConservationBasis, ConservationRow},
    ledger::ConservationState,
    Envelope, EnvelopeLedger, EnvelopeOrigin, EnvelopeReceipt, JournalRank, ResVec, StepFatal,
};
use crate::compute::parallel::*;

const PARALLEL_ROW_THRESHOLD: usize = 512;
const ROW_CHUNK_SIZE: usize = 64;

#[cfg(test)]
thread_local! {
    pub(super) static ROW_MAPPING_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy)]
pub(super) struct RowTotals {
    basis: ConservationBasis,
    sealed_spent: ResVec,
    sealed_released: ResVec,
    commit_live: ResVec,
}

pub(super) fn record(
    ledger: &mut EnvelopeLedger,
    receipt: &EnvelopeReceipt,
) -> Result<(), StepFatal> {
    let state = ledger
        .conservation
        .get_mut(&receipt.envelope)
        .ok_or_else(|| super::ledger_validation::invariant("missing conservation state"))?;
    record_state(state, receipt)
}

pub(super) fn record_state(
    state: &mut ConservationState,
    receipt: &EnvelopeReceipt,
) -> Result<(), StepFatal> {
    if receipt.local_key.journal() == JournalRank::PreSeal {
        state.expiry_released = state.expiry_released.checked_add(receipt.delta.released)?;
    } else {
        state.sealed_spent = state.sealed_spent.checked_add(receipt.delta.spent)?;
        state.sealed_released = state.sealed_released.checked_add(receipt.delta.released)?;
    }
    Ok(())
}

/// 私有逐股 row 已共同拥有 envelope 和守恒事实，不再从三份独立 map 重复查找。
/// 仍复用相同的逐行方程与原顺序 checked 汇总，保留首错与资源溢出合同。
pub(super) fn validate_owned_rows(
    rows: &[(&Envelope, &ConservationState)],
) -> Result<(), StepFatal> {
    let checked = map_rows(rows, None, |(envelope, state)| check_row(envelope, state));
    let mut totals = [ResVec::ZERO; 5];
    for row in checked {
        accumulate_totals(&mut totals, row?)?;
    }
    validate_aggregate(totals)
}

pub(super) fn validate(ledger: &EnvelopeLedger) -> Result<(), StepFatal> {
    validate_with_observer(ledger, || {})
}

/// 与 [`validate`] 相同的守恒与错误合同；compute 语义同 [`validate_with_observer_in`]。
pub(super) fn validate_in(
    ledger: &EnvelopeLedger,
    compute: Option<&std::sync::Arc<crate::compute::ComputeContext>>,
) -> Result<(), StepFatal> {
    validate_with_observer_in(ledger, compute, || {})
}

pub(super) fn validate_with_observer(
    ledger: &EnvelopeLedger,
    observe_row: impl Fn() + Sync,
) -> Result<(), StepFatal> {
    validate_with_observer_in(ledger, None, observe_row)
}

/// 与 [`validate_with_observer`] 相同的逐行校验与错误合同；显式传入 compute 时，
/// ≥512 行的 dense row 审计归入该 registry（见 [`map_rows`] 的 Some 分支），
/// 未传时按 None 合同在调用线程串行执行，不借用 caller 域的 Rayon registry。
pub(super) fn validate_with_observer_in(
    ledger: &EnvelopeLedger,
    compute: Option<&std::sync::Arc<crate::compute::ComputeContext>>,
    observe_row: impl Fn() + Sync,
) -> Result<(), StepFatal> {
    let mut totals = [ResVec::ZERO; 5];
    if ledger.envelopes.len() < PARALLEL_ROW_THRESHOLD
        && ledger.terminal_envelopes.len() < PARALLEL_ROW_THRESHOLD - ledger.envelopes.len()
    {
        for (key, envelope) in ledger
            .envelopes
            .iter()
            .chain(ledger.terminal_envelopes.iter())
        {
            observe_row();
            let state = ledger
                .conservation
                .get(key)
                .ok_or_else(|| super::ledger_validation::invariant("missing conservation row"))?;
            let row = row(envelope, state)?;
            row.validate_with_preseal(state.expiry_released)?;
            accumulate(&mut totals, row)?;
        }
        return validate_aggregate(totals);
    }

    let rows: Vec<_> = ledger
        .envelopes
        .iter()
        .chain(ledger.terminal_envelopes.iter())
        .map(|(key, envelope)| (key, envelope))
        .collect();
    let validated = map_rows(&rows, compute, |(key, envelope)| {
        observe_row();
        let state = ledger
            .conservation
            .get(key)
            .ok_or_else(|| super::ledger_validation::invariant("missing conservation row"))?;
        check_row(envelope, state)
    });

    for row in validated {
        let row = row?;
        accumulate_totals(
            &mut totals,
            RowTotals {
                basis: row.basis,
                sealed_spent: row.sealed_spent,
                sealed_released: row.sealed_released,
                commit_live: row.commit_live,
            },
        )?;
    }
    validate_aggregate(totals)
}

/// 单行方程仍使用原 ConservationRow；完整门禁复用结果，不重复创建并行映射。
pub(super) fn check_row(
    envelope: &Envelope,
    state: &ConservationState,
) -> Result<RowTotals, StepFatal> {
    let row = row(envelope, state)?;
    row.validate_with_preseal(state.expiry_released)?;
    Ok(RowTotals {
        basis: row.basis,
        sealed_spent: row.sealed_spent,
        sealed_released: row.sealed_released,
        commit_live: row.commit_live,
    })
}

/// 按原始源序执行 checked 汇总；较早总额溢出仍先于较晚行错误。
pub(super) fn validate_checked_rows(
    rows: impl IntoIterator<Item = Result<RowTotals, StepFatal>>,
) -> Result<(), StepFatal> {
    let mut totals = [ResVec::ZERO; 5];
    for row in rows {
        accumulate_totals(&mut totals, row?)?;
    }
    validate_aggregate(totals)
}

fn accumulate(totals: &mut [ResVec; 5], row: ConservationRow) -> Result<(), StepFatal> {
    accumulate_totals(
        totals,
        RowTotals {
            basis: row.basis,
            sealed_spent: row.sealed_spent,
            sealed_released: row.sealed_released,
            commit_live: row.commit_live,
        },
    )
}

fn accumulate_totals(totals: &mut [ResVec; 5], row: RowTotals) -> Result<(), StepFatal> {
    match row.basis {
        ConservationBasis::TickStart(start, released, _) => {
            totals[0] = totals[0].checked_add(start)?;
            totals[1] = totals[1].checked_add(released)?;
        }
        ConservationBasis::CreatedAtValidation(created) => {
            totals[0] = totals[0].checked_add(created)?
        }
    }
    totals[2] = totals[2].checked_add(row.sealed_spent)?;
    totals[3] = totals[3].checked_add(row.sealed_released)?;
    totals[4] = totals[4].checked_add(row.commit_live)?;
    Ok(())
}

fn validate_aggregate(totals: [ResVec; 5]) -> Result<(), StepFatal> {
    if totals[0]
        != totals[1]
            .checked_add(totals[2])?
            .checked_add(totals[3])?
            .checked_add(totals[4])?
    {
        return Err(super::ledger_validation::invariant(
            "aggregate conservation row mismatch",
        ));
    }
    Ok(())
}

pub(super) fn map_rows<T, R>(
    rows: &[T],
    compute: Option<&std::sync::Arc<crate::compute::ComputeContext>>,
    validate_row: impl Fn(&T) -> Result<R, StepFatal> + Sync,
) -> Vec<Result<R, StepFatal>>
where
    T: Sync,
    R: Send,
{
    #[cfg(test)]
    ROW_MAPPING_PASSES.with(|count| count.set(count.get() + 1));
    if rows.len() < PARALLEL_ROW_THRESHOLD {
        return rows.iter().map(validate_row).collect();
    }

    // IndexedParallelIterator 保留源行顺序，直接写入连续结果，避免每个 chunk 单独分配。
    match compute {
        // ReceiptAggregation 链等显式接入的完整校验归入会话 compute registry：
        // record 在 install 内执行，观察到的 executing_workers/owner_worker 即真实执行域。
        Some(context) => context.install(|| {
            context.record(crate::compute::ComputeWork::ReceiptAggregationAudit);
            rows.par_iter()
                .with_min_len(ROW_CHUNK_SIZE)
                .map(&validate_row)
                .collect()
        }),
        // 消灭 caller 域隐式全局入口：不借用 caller 当前的 Rayon registry
        //（串行脊线程上 par_iter 会惰性初始化全局池，即线程数 132→260 的元凶）。
        // 未显式接会话上下文的调用方在 ≥512 行时退化为调用线程串行循环；
        // 逐行校验内容、结果顺序与错误合同与原并行分支完全一致，只改执行域。
        None => rows.iter().map(|row| validate_row(row)).collect(),
    }
}

fn row(envelope: &Envelope, state: &ConservationState) -> Result<ConservationRow, StepFatal> {
    let basis = match envelope.origin() {
        EnvelopeOrigin::TickStart => ConservationBasis::TickStart(
            envelope.basis(),
            state.expiry_released,
            envelope.basis().checked_sub(state.expiry_released)?,
        ),
        EnvelopeOrigin::CreatedAtValidation => {
            ConservationBasis::CreatedAtValidation(envelope.basis())
        }
    };
    Ok(ConservationRow {
        key: envelope.key().clone(),
        basis,
        sealed_spent: state.sealed_spent,
        sealed_released: state.sealed_released,
        commit_live: envelope.live(),
    })
}
