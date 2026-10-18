//! 有类型事件事实记录（架构项 #6 的单 owner）。
//!
//! `EventFact { key, event }` 是事件与身份的唯一组合载体：producer 创建一次身份，
//! 收集、提交、发布与 wire 全程只传递这一批记录；不再并列维护 `Vec<Event>` 与
//! `Vec<EventStableKey>`，也不在运行时生成 canonical payload 字符串作为第二份权威。
//! 去重与续号只依赖有类型数值身份（`EventStableKey` 的域与局部序号）。

use super::ProtocolError;
use crate::session::pipeline::{EntityTag, EventSourceIndex, EventStableKey};
use crate::session::Event;
use std::collections::{BTreeMap, BTreeSet};

/// 一条已提交事件的唯一事实记录：数值身份 + 事件载荷。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct EventFact {
    pub key: EventStableKey,
    pub event: Event,
}

/// 事实身份域：(phase_rank, entity, source)。
type FactDomain = (u8, EntityTag, EventSourceIndex);

fn domain_of(key: &EventStableKey) -> FactDomain {
    (key.phase_rank(), key.entity().clone(), key.source())
}

/// 发布流的数值身份游标：只记录每个事实域已见最大局部序号与全局 seq 截点。
///
/// 旧实现把同 tick 的 preceding facts（含完整事件副本与 canonical payload）存进
/// 游标并在每次续号时全量重新校验；现在游标只持数值，事件本体始终只由
/// producer→outbox→candidate→commit→frame 的单批记录拥有。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionFactCursor {
    tick: Option<u64>,
    last_seq: Option<u64>,
    domains: BTreeMap<FactDomain, u64>,
}

impl SessionFactCursor {
    /// 游标当前记录的 tick；发布流尚未开始时为 `None`。
    pub fn tick(&self) -> Option<u64> {
        self.tick
    }

    /// 进入新 tick 时重置 Session 发出流域；同一 tick 内的多次发布保持续号。
    pub fn begin_tick(&mut self, tick: u64) {
        if self.tick != Some(tick) {
            self.tick = Some(tick);
            self.domains.clear();
            self.last_seq = None;
        }
    }

    /// 把一批已提交事实记入数值游标。无关事件可任意到达；Account/Stock 的局部身份
    /// 大小不表示先后（特别是 QuoteExpiry 高段会先于 Projection 低段）。只有跨发布的
    /// Session 发出流需要接续；整个批次验证成功后才推进游标。
    pub fn record_committed_facts(&mut self, facts: &[EventFact]) -> Result<(), ProtocolError> {
        validate_facts(facts)?;
        for fact in facts {
            if self.last_seq.is_some_and(|seq| fact.event.seq() <= seq) {
                return Err(ProtocolError::FactIdentity);
            }
            let domain = domain_of(&fact.key);
            if matches!(fact.key.entity(), EntityTag::Session)
                && self
                    .domains
                    .get(&domain)
                    .is_some_and(|index| *index >= fact.key.local_event_index())
            {
                return Err(ProtocolError::FactIdentity);
            }
        }
        for fact in facts {
            // 后续 civil 只接续 Session 域；不为无关 Account/Stock 身份建立持久缓存。
            if matches!(fact.key.entity(), EntityTag::Session) {
                let domain = domain_of(&fact.key);
                let index = fact.key.local_event_index();
                self.domains
                    .entry(domain)
                    .and_modify(|last| *last = (*last).max(index))
                    .or_insert(index);
            }
            self.last_seq = Some(
                self.last_seq
                    .map_or(fact.event.seq(), |last| last.max(fact.event.seq())),
            );
        }
        Ok(())
    }

    /// 在已提交游标之上为自然日 Session 事件一次分配 typed 身份。
    ///
    /// 只读续号：分配不推进游标；调用方（`end_civil_day_update`）发布成功后必须用
    /// [`Self::record_committed_facts`] 提交，下一批分配才会接续（与旧
    /// attach_after→append_committed_facts 的两段语义一致）。同批内同域事件链式递增。
    pub fn assign_session_facts(
        &self,
        events: Vec<Event>,
    ) -> Result<Vec<EventFact>, ProtocolError> {
        let mut assigned = Vec::with_capacity(events.len());
        let mut batch_last_seq = self.last_seq;
        // 批内每域的"下一个待分配 index"；未出现的域从已提交链的下一位置起步。
        let mut batch_next = BTreeMap::<FactDomain, u64>::new();
        for event in events {
            if batch_last_seq.is_some_and(|seq| event.seq() <= seq) {
                return Err(ProtocolError::FactIdentity);
            }
            let key = EventStableKey::for_event(&event, 0);
            let domain = domain_of(&key);
            let committed_next = self
                .domains
                .get(&domain)
                .map_or(Some(0), |last| last.checked_add(1))
                .filter(|index| *index <= crate::orderbook::js_safe_u64::MAX)
                .ok_or(ProtocolError::FactIdentity)?;
            let next = batch_next.entry(domain).or_insert(committed_next);
            let index = *next;
            if index > crate::orderbook::js_safe_u64::MAX {
                return Err(ProtocolError::FactIdentity);
            }
            *next = index.checked_add(1).ok_or(ProtocolError::FactIdentity)?;
            batch_last_seq = Some(event.seq());
            assigned.push(EventFact {
                key: key.with_local_event_index(index),
                event,
            });
        }
        Ok(assigned)
    }
}

/// 校验一批事实记录的数值身份：key 与事件变体/实体/相位映射一致、key 全局唯一。
///
/// 不再对事件做 JSON 规范化序列化比较；序列覆盖由 `validate_sequence` 单独校验。
pub(in crate::session::protocol) fn validate_facts(
    facts: &[EventFact],
) -> Result<(), ProtocolError> {
    let mut keys = BTreeSet::new();
    let mut seqs = BTreeSet::new();
    for fact in facts {
        if fact.key.local_event_index() > crate::orderbook::js_safe_u64::MAX
            || EventStableKey::for_event(&fact.event, fact.key.local_event_index()) != fact.key
            || !keys.insert(&fact.key)
            || !seqs.insert(fact.event.seq())
        {
            return Err(ProtocolError::FactIdentity);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn civil_event(seq: u64) -> Event {
        Event::CivilDateAdvanced {
            seq,
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        }
    }

    #[test]
    fn cursor_continues_session_domain_and_resets_per_tick_without_keeping_events() {
        let mut cursor = SessionFactCursor::default();
        cursor.begin_tick(12);
        let first = cursor.assign_session_facts(vec![civil_event(1)]).unwrap();
        assert_eq!(first[0].key.local_event_index(), 0);
        // 分配只读：发布成功后 record 才推进游标，下一批在此基础上续号。
        cursor.record_committed_facts(&first).unwrap();
        cursor.begin_tick(12);
        let second = cursor.assign_session_facts(vec![civil_event(2)]).unwrap();
        assert_eq!(second[0].key.local_event_index(), 1);
        cursor.record_committed_facts(&second).unwrap();

        let checkpoint = cursor.clone();
        cursor.begin_tick(13);
        assert_eq!(
            cursor.assign_session_facts(vec![civil_event(3)]).unwrap()[0]
                .key
                .local_event_index(),
            0
        );
        cursor = checkpoint;
        assert_eq!(
            cursor.assign_session_facts(vec![civil_event(3)]).unwrap()[0]
                .key
                .local_event_index(),
            2
        );
    }

    #[test]
    fn cursor_rejects_non_increasing_seq_or_domain_regression() {
        let mut cursor = SessionFactCursor::default();
        cursor.begin_tick(1);
        let facts = cursor.assign_session_facts(vec![civil_event(5)]).unwrap();
        cursor.record_committed_facts(&facts).unwrap();
        // seq 必须严格递增
        assert!(matches!(
            cursor.assign_session_facts(vec![civil_event(5)]),
            Err(ProtocolError::FactIdentity)
        ));
        // 已记录域的局部序号不得回退
        let mut forged = facts.clone();
        forged[0].key = EventStableKey::for_event(&forged[0].event, 0);
        forged[0].event = civil_event(6);
        assert!(matches!(
            cursor.record_committed_facts(&forged),
            Err(ProtocolError::FactIdentity)
        ));
        // 重复提交同一批也必须被拒绝（seq 不再前进）
        assert!(matches!(
            cursor.record_committed_facts(&facts),
            Err(ProtocolError::FactIdentity)
        ));
    }
}
