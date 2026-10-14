//! Projection 事件记录收集器。
//!
//! worker 移交已经带有源内身份的记录。Projection 只校验数值身份并按实际到达顺序
//! 分配外部 seq，不从 producer 的 seq 推断身份，也不修补 producer 的局部序号。
//! 同一批 `Vec<EventFact>` 继续移动到 commit 与 publication，禁止拆成平行的
//! events/keys 两个 Vec（架构项 #6）。

use super::{Event, EventStableKey, StepFatal};
pub(super) use crate::session::protocol::EventFact;

/// Projection 的纯输出；`next_seq` 是最后分配的外部事件游标。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CollectedEvents {
    pub(super) facts: Vec<EventFact>,
    pub(super) next_seq: u64,
}

/// 校验身份唯一性，并按实际到达顺序分配外部 seq。
///
/// 输入 `next_seq` 是已经提交的游标，第一条事实取得 `next_seq + 1`。游标按值传入，
/// 只有全部验证成功后才返回记录；校验或整数溢出失败均不泄漏部分事实或游标变化。
///
/// 跨实体事件不再按 StableKey 全局排序（用户已授权放宽：无关实体不要求固定次序）；
/// key 唯一性由 BTreeSet 独立校验，seq 按实际到达顺序连续分配。
pub(super) fn collect_events(
    facts: Vec<EventFact>,
    next_seq: u64,
) -> Result<CollectedEvents, StepFatal> {
    for fact in &facts {
        validate_mapping(fact)?;
    }
    validate_unique_keys(&facts)?;

    let mut cursor = next_seq;
    let mut facts = facts;
    for fact in &mut facts {
        cursor = cursor
            .checked_add(1)
            .filter(|seq| *seq <= crate::orderbook::js_safe_u64::MAX)
            .ok_or_else(|| {
                invariant("external event sequence overflow while collecting canonical event facts")
            })?;
        set_seq(&mut fact.event, cursor);
    }
    Ok(CollectedEvents {
        facts,
        next_seq: cursor,
    })
}

fn validate_mapping(fact: &EventFact) -> Result<(), StepFatal> {
    let expected = EventStableKey::for_event(&fact.event, fact.key.local_event_index());
    if expected != fact.key {
        return Err(invariant(
            "worker event key does not match the event variant, entity, phase, or source",
        ));
    }
    Ok(())
}

fn validate_unique_keys(facts: &[EventFact]) -> Result<(), StepFatal> {
    let mut seen = std::collections::BTreeSet::new();
    for fact in facts {
        if !seen.insert(&fact.key) {
            return Err(invariant("duplicate worker event stable key"));
        }
    }
    Ok(())
}

/// 穷举所有 Event 变体，新增变体必须明确接入 seq 分配；身份映射由 for_event 独立校验。
fn set_seq(event: &mut Event, sequence: u64) {
    match event {
        Event::Trade { seq, .. }
        | Event::AuctionTick { seq, .. }
        | Event::AuctionCompleted { seq, .. }
        | Event::PriceTick { seq, .. }
        | Event::DayBoundary { seq, .. }
        | Event::CivilDateAdvanced { seq, .. }
        | Event::CompanyDisclosurePublished { seq, .. }
        | Event::IntentRejected { seq, .. }
        | Event::SettlementError { seq, .. }
        | Event::OrderCanceled { seq, .. }
        | Event::OrderAccepted { seq, .. } => *seq = sequence,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::event_collection::collect_events".to_owned(),
    }
}
