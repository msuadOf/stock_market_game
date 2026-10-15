//! 单个机构的个人状态由同一条目保存；root 期间独占移动，不共享可变认识。

use crate::experience::{PersonalPriceMemory, PersonalWatchlist};
use crate::information::NpcInformationState;
use crate::session::attention::NpcAttentionState;
use crate::strategy::BeliefBook;
use crate::{AccountId, GameSession};

#[derive(Clone)]
pub(in crate::session) struct BeliefParticipantState {
    watchlist: PersonalWatchlist,
    price_memory: PersonalPriceMemory,
    information: NpcInformationState,
    belief: BeliefBook,
}

impl BeliefParticipantState {
    pub(in crate::session) fn new(
        watchlist: PersonalWatchlist,
        price_memory: PersonalPriceMemory,
        information: NpcInformationState,
        belief: BeliefBook,
    ) -> Self {
        Self {
            watchlist,
            price_memory,
            information,
            belief,
        }
    }
    pub(in crate::session) fn belief(&self) -> &BeliefBook {
        &self.belief
    }
    pub(in crate::session) fn belief_mut(&mut self) -> &mut BeliefBook {
        &mut self.belief
    }
    pub(in crate::session) fn information(&self) -> &NpcInformationState {
        &self.information
    }
    pub(in crate::session) fn install_information_update(
        &mut self,
        information: NpcInformationState,
        belief: BeliefBook,
    ) {
        self.information = information;
        self.belief = belief;
    }
    #[cfg(test)]
    pub(in crate::session) fn information_mut(&mut self) -> &mut NpcInformationState {
        &mut self.information
    }
    pub(in crate::session) fn watchlist(&self) -> &PersonalWatchlist {
        &self.watchlist
    }
    #[cfg(test)]
    pub(in crate::session) fn watchlist_mut(&mut self) -> &mut PersonalWatchlist {
        &mut self.watchlist
    }
    pub(in crate::session) fn price_memory(&self) -> &PersonalPriceMemory {
        &self.price_memory
    }
    pub(in crate::session) fn prune_memory(
        &mut self,
        protected: &std::collections::BTreeSet<crate::StockId>,
    ) {
        self.watchlist.prune(protected);
        self.price_memory.prune(protected);
    }
    #[cfg(test)]
    pub(in crate::session) fn price_memory_mut(&mut self) -> &mut PersonalPriceMemory {
        &mut self.price_memory
    }
}

/// 字段 pub(crate)（D3a-2c 件2）：组线程以纯数据包组装/读回（take/install
/// 的组内等价——个人态是纯数据，无会话借用）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlanPersonalState {
    pub(crate) attention: NpcAttentionState,
    pub(crate) watchlist: PersonalWatchlist,
    pub(crate) price_memory: PersonalPriceMemory,
    pub(crate) history_reads: crate::experience::PersonalHistoryReadLedger,
    pub(crate) information: NpcInformationState,
    pub(crate) belief: BeliefBook,
}

impl PlanPersonalState {
    /// T1·两阶段（ADR-0042 §T1）：queue 账户任务在任务内做 attention 判定
    /// （推进个人 RNG 流）——经访问器独占访问，不开放字段本身。
    pub(in crate::session) fn attention(&self) -> &NpcAttentionState {
        &self.attention
    }

    /// 同上，可变访问（evaluate_candidate_with_signal 原地推进状态）。
    pub(in crate::session) fn attention_mut(&mut self) -> &mut NpcAttentionState {
        &mut self.attention
    }

    pub(in crate::session) fn take(session: &mut GameSession, account: AccountId) -> Self {
        // 只需写组，不创建读视图（避免不必要的 lifecycles/accounts 克隆）。
        let mut writes = crate::session::pipeline::npc_split::npc_writes(session);
        Self::take_on(&mut writes, account)
    }

    /// 写组版本：queue 链转换目标形态，take 语义与 session 版逐行同源。
    pub(in crate::session) fn take_on(
        writes: &mut crate::session::pipeline::npc_split::NpcWriteFields<'_>,
        account: AccountId,
    ) -> Self {
        // attention 的缺失仍先报错；机构四成员具有经存档边界校验的同键不变量。
        let attention = writes
            .npc_attention
            .remove(&account)
            .unwrap_or_else(|| panic!("plan account {account:?} has no attention state"));
        let participant = writes
            .belief_participants
            .remove(&account)
            .unwrap_or_else(|| panic!("plan account {account:?} has no watchlist"));
        let history_reads = writes
            .history_reads
            .remove(&account)
            .unwrap_or_else(|| panic!("plan account {account:?} has no history-read ledger"));
        Self {
            attention,
            watchlist: participant.watchlist,
            price_memory: participant.price_memory,
            history_reads,
            information: participant.information,
            belief: participant.belief,
        }
    }

    pub(in crate::session) fn install(self, session: &mut GameSession, account: AccountId) {
        // 同 take：只需写组，不创建读视图。
        let mut writes = crate::session::pipeline::npc_split::npc_writes(session);
        self.install_on(&mut writes, account);
    }

    /// 写组版本：install 语义与 session 版逐行同源。
    pub(in crate::session) fn install_on(
        self,
        writes: &mut crate::session::pipeline::npc_split::NpcWriteFields<'_>,
        account: AccountId,
    ) {
        assert!(
            writes
                .npc_attention
                .insert(account, self.attention)
                .is_none(),
            "plan account {account:?} attention state was installed twice"
        );
        if let Some(mut previous) = writes.belief_participants.remove(&account) {
            // 原路径在 watchlist 重复时已写入新 watchlist，随后立即 panic；其余成员不变。
            previous.watchlist = self.watchlist;
            writes.belief_participants.insert(account, previous);
            panic!("plan account {account:?} watchlist was installed twice");
        }
        writes.belief_participants.insert(
            account,
            BeliefParticipantState::new(
                self.watchlist,
                self.price_memory,
                self.information,
                self.belief,
            ),
        );
        assert!(
            writes
                .history_reads
                .insert(account, self.history_reads)
                .is_none(),
            "plan account {account:?} history-read ledger was installed twice"
        );
    }
}

#[cfg(test)]
mod participant_owner_tests {
    use super::*;
    use crate::{Money, StockId};

    fn four_fields(participant: &BeliefParticipantState) -> serde_json::Value {
        serde_json::to_value((
            participant.watchlist(),
            participant.price_memory(),
            participant.information(),
            participant.belief(),
        ))
        .unwrap()
    }

    fn two_institutions() -> GameSession {
        let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
        setup.npcs.inst_count = 2;
        GameSession::new(setup, 42).unwrap()
    }

    #[test]
    fn moving_one_participant_keeps_its_four_fields_and_other_accounts_isolated() {
        let authority = two_institutions();
        let first = AccountId(1);
        let second = AccountId(2);
        assert_eq!(authority.state.belief_participants.len(), 2);
        let original_first = four_fields(&authority.state.belief_participants[&first]);
        let original_second = four_fields(&authority.state.belief_participants[&second]);
        let mut shadow = authority.clone_for_tick_shadow().unwrap();
        let mut personal = PlanPersonalState::take(&mut shadow, first);
        assert!(!shadow.state.belief_participants.contains_key(&first));
        assert!(!shadow.state.npc_attention.contains_key(&first));
        let code = StockId(0);
        personal.watchlist.record_attention(&code, 1, 1).unwrap();
        personal
            .price_memory
            .observe_price(&code, Money::from_cents(1011), 1)
            .unwrap();
        personal
            .belief
            .experience_mut()
            .observe_equity(Money::from_cents(200_000))
            .unwrap();
        personal.information = NpcInformationState::new(first);
        let publication = shadow.state.library.publication_ids()[0];
        personal
            .information
            .record_acquisition(
                first,
                &shadow.state.library,
                publication,
                shadow.observation_civil_instant(),
            )
            .unwrap();
        let expected = serde_json::to_value((
            &personal.watchlist,
            &personal.price_memory,
            &personal.information,
            &personal.belief,
        ))
        .unwrap();

        personal.install(&mut shadow, first);

        assert_eq!(
            four_fields(&shadow.state.belief_participants[&first]),
            expected
        );
        assert_eq!(
            four_fields(&shadow.state.belief_participants[&second]),
            original_second
        );
        assert_eq!(
            four_fields(&authority.state.belief_participants[&first]),
            original_first
        );
        assert_eq!(
            four_fields(&authority.state.belief_participants[&second]),
            original_second
        );
        assert!(shadow.state.npc_attention.contains_key(&first));
    }

    #[test]
    fn aggregate_preserves_the_four_independent_save_maps_after_restore() {
        let session = two_institutions();
        let saved = session.save().unwrap();
        let restored = GameSession::restore(&saved).unwrap();
        let rebuilt = restored.save().unwrap();
        assert_eq!(
            serde_json::to_value(&rebuilt.information_states).unwrap(),
            serde_json::to_value(&saved.information_states).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&rebuilt.belief_books).unwrap(),
            serde_json::to_value(&saved.belief_books).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&rebuilt.watchlists).unwrap(),
            serde_json::to_value(&saved.watchlists).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&rebuilt.price_memories).unwrap(),
            serde_json::to_value(&saved.price_memories).unwrap()
        );
        assert_eq!(
            session.business_state_hash().unwrap(),
            restored.business_state_hash().unwrap()
        );
    }

    #[test]
    #[should_panic(expected = "has no attention state")]
    fn take_missing_attention_panics_first() {
        let mut session = two_institutions();
        let account = AccountId(1);
        session.state.npc_attention.remove(&account);
        session.state.belief_participants.remove(&account);
        session.state.history_reads.remove(&account);
        PlanPersonalState::take(&mut session, account);
    }

    #[test]
    #[should_panic(expected = "has no watchlist")]
    fn take_missing_participant_panics_after_attention() {
        let mut session = two_institutions();
        let account = AccountId(1);
        session.state.belief_participants.remove(&account);
        session.state.history_reads.remove(&account);
        PlanPersonalState::take(&mut session, account);
    }

    #[test]
    #[should_panic(expected = "has no history-read ledger")]
    fn take_missing_history_panics_after_participant() {
        let mut session = two_institutions();
        let account = AccountId(1);
        session.state.history_reads.remove(&account);
        PlanPersonalState::take(&mut session, account);
    }

    #[test]
    #[should_panic(expected = "attention state was installed twice")]
    fn duplicate_personal_install_still_checks_attention_first() {
        let mut session = two_institutions();
        let account = AccountId(1);
        let personal = PlanPersonalState::take(&mut session, account);
        session
            .state
            .npc_attention
            .insert(account, personal.attention.clone());
        personal.install(&mut session, account);
    }

    #[test]
    #[should_panic(expected = "watchlist was installed twice")]
    fn duplicate_participant_install_still_reports_watchlist_after_attention() {
        let mut session = two_institutions();
        let account = AccountId(1);
        let previous = session.state.belief_participants[&account].clone();
        let personal = PlanPersonalState::take(&mut session, account);
        session.state.belief_participants.insert(account, previous);
        personal.install(&mut session, account);
    }
}

#[cfg(test)]
mod duplicate_install_state_tests {
    use super::*;

    #[test]
    fn duplicate_participant_install_keeps_the_original_partial_write_boundary() {
        let mut session =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        let account = AccountId(1);
        let previous = session.state.belief_participants[&account].clone();
        let old_rest = serde_json::to_value((
            previous.price_memory(),
            previous.information(),
            previous.belief(),
        ))
        .unwrap();
        let mut personal = PlanPersonalState::take(&mut session, account);
        let code = crate::StockId(0);
        personal.watchlist.record_attention(&code, 1, 1).unwrap();
        personal
            .price_memory
            .observe_price(&code, crate::Money::from_cents(1033), 1)
            .unwrap();
        personal
            .belief
            .experience_mut()
            .observe_equity(crate::Money::from_cents(100_000))
            .unwrap();
        personal.attention.next_attention_candidate_tick = 321;
        let expected_watchlist = serde_json::to_value(&personal.watchlist).unwrap();
        session.state.belief_participants.insert(account, previous);

        // 仅测试 harness 观察 invariant panic 的失败残态，生产路径仍直接 panic。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            personal.install(&mut session, account)
        }));

        assert!(result.is_err());
        let installed = &session.state.belief_participants[&account];
        assert_eq!(
            serde_json::to_value(installed.watchlist()).unwrap(),
            expected_watchlist
        );
        assert_eq!(
            serde_json::to_value((
                installed.price_memory(),
                installed.information(),
                installed.belief()
            ))
            .unwrap(),
            old_rest
        );
        assert_eq!(
            session.state.npc_attention[&account].next_attention_candidate_tick,
            321
        );
    }
}
