//! 为已有完整身份的 producer 事实提供 Projection adapters。
//!
//! 不检查 GameSession、不分配外部事件序号、不从 worker vector 位置推导身份。
//! AccountValidation 结果须先与规范 IntentCandidateBatch sealed identity 对照，之后才能投影。
//! Continuous 股票处理事实通过其输出契约携带显式 sealed identity 与逐股 trade identity。

use super::{
    event_collection::OwnedEventFact, CandidateValidationResult, EventStableKey, IntentCandidate,
    IntentCandidateBatch, IntentCandidateKey, StepFatal,
};
use crate::{Event, Intent, StockCode};
use std::collections::{BTreeMap, BTreeSet};

/// 把普通 AccountValidation 拒绝转换为 Projection 拥有的事件事实。
///
/// 拒绝结果保留 sealed identity；不可变 IntentCandidateBatch 提供公共拒绝事件所需账户与 intent code。
/// 产生任何事实前，先校验整个候选批次与账户校验结果的对应关系。
pub(super) fn adapt_account_validation_rejection_facts(
    candidates: &IntentCandidateBatch,
    results: &[CandidateValidationResult],
) -> Result<Vec<OwnedEventFact>, StepFatal> {
    let candidates_by_key = index_candidates(candidates)?;
    validate_account_validation_result_contract(&candidates_by_key, results)?;

    let mut facts = Vec::new();
    for result in results {
        match result {
            CandidateValidationResult::Rejected {
                key,
                sealed_index,
                reason,
            } => {
                let binding = candidates_by_key.get(key).ok_or_else(|| {
                    invariant("AccountValidation 拒绝结果未引用 DecisionShadow 候选")
                })?;
                let candidate = binding.candidate;
                let event = Event::IntentRejected {
                    seq: 0,
                    account: candidate.owner(),
                    code: candidate_code(candidate)?.clone(),
                    reason: reason.clone(),
                };
                facts.push(OwnedEventFact {
                    key: EventStableKey::for_event(&event, *sealed_index),
                    event,
                });
            }
            CandidateValidationResult::Accepted { .. } => {}
        }
    }
    Ok(facts)
}

struct CandidateBinding<'a> {
    candidate: &'a IntentCandidate,
    sealed_index: u64,
}

fn index_candidates(
    candidates: &IntentCandidateBatch,
) -> Result<BTreeMap<IntentCandidateKey, CandidateBinding<'_>>, StepFatal> {
    let mut indexed = BTreeMap::new();
    for (canonical_ordinal, candidate) in candidates.candidates().iter().enumerate() {
        let sealed_index = u64::try_from(canonical_ordinal)
            .map_err(|_| invariant("DecisionShadow 批次超出 sealed identity 域"))?;
        if indexed
            .insert(
                candidate.key().clone(),
                CandidateBinding {
                    candidate,
                    sealed_index,
                },
            )
            .is_some()
        {
            return Err(invariant("DecisionShadow 批次包含重复候选 key"));
        }
    }
    Ok(indexed)
}

fn validate_account_validation_result_contract(
    candidates: &BTreeMap<IntentCandidateKey, CandidateBinding<'_>>,
    results: &[CandidateValidationResult],
) -> Result<(), StepFatal> {
    if results.len() != candidates.len() {
        return Err(invariant(
            "AccountValidation 结果数量与不可变 DecisionShadow 候选批次不一致",
        ));
    }

    let mut result_keys = BTreeSet::new();
    let mut sealed_indices = BTreeSet::new();
    for result in results {
        let binding = candidates
            .get(result.key())
            .ok_or_else(|| invariant("AccountValidation 结果未引用 DecisionShadow 候选"))?;
        if result.sealed_index() != binding.sealed_index {
            return Err(invariant(
                "AccountValidation 结果的 sealed identity 与规范 DecisionShadow 批次的 sealed identity 不一致",
            ));
        }
        if !result_keys.insert(result.key()) {
            return Err(invariant("AccountValidation 结果包含重复候选 key"));
        }
        if !sealed_indices.insert(result.sealed_index()) {
            return Err(invariant("AccountValidation 结果包含重复 sealed identity"));
        }
    }

    Ok(())
}

fn candidate_code(candidate: &IntentCandidate) -> Result<&StockCode, StepFatal> {
    match candidate.intent() {
        Intent::PlaceLimit { code, .. }
        | Intent::PlaceMarket { code, .. }
        | Intent::Cancel { code, .. } => Ok(code),
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::session_fact_producers".to_owned(),
    }
}
