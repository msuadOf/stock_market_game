//! 唯一市场事务与 publication 的提交门禁：单批 EventFact 只在全部 callback 验证后安装。

use super::market_tick_transaction::prepare_market_tick_with_evidence;
use crate::session::{
    protocol::{EventFact, TickCandidateProjection},
    StepFatal,
};
use crate::GameSession;

pub(in crate::session) struct AuthoritativeTickCommit {
    pub(in crate::session) facts: Vec<EventFact>,
    pub(in crate::session) evidence: Option<super::TickCommitEvidence>,
}

pub(in crate::session) fn execute_authoritative_tick(
    authority: &mut GameSession,
    capture_commit_evidence: bool,
) -> Result<AuthoritativeTickCommit, StepFatal> {
    let (facts, evidence) = execute_authoritative_tick_with_projection(
        authority,
        capture_commit_evidence,
        |_, facts| Ok(facts),
    )?;
    Ok(AuthoritativeTickCommit { facts, evidence })
}

pub(in crate::session) fn execute_authoritative_tick_with_projection<P>(
    authority: &mut GameSession,
    capture_commit_evidence: bool,
    project: impl FnOnce(TickCandidateProjection<'_>, Vec<EventFact>) -> Result<P, StepFatal>,
) -> Result<(P, Option<super::TickCommitEvidence>), StepFatal> {
    crate::verification_evidence::begin_authoritative_tick(
        authority.tick(),
        authority.compute_context(),
    )?;
    let prepared = prepare_market_tick_with_evidence(authority, capture_commit_evidence)
        .map_err(|error| error.into_fatal())?
        .into_commit_plan();
    prepared.project_and_commit(project)
}
