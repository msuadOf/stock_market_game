//! NPC 决策管线与只读准备工作的字段级借用拆分（X1 前置，见 agents/engine-max-performance/x1-progress.md）。
//!
//! [`npc_split`] 把 [`GameSession`] 的可变状态按"NPC 管线专写域 / 共享只读域"切成两组
//! 不相交借用，使 commit 尾部可以在 `rayon::join` 中一边排 NPC 队列、一边投机准备
//! 下一 tick 的 read-only start。唯一同时被两侧触碰的字段是 `accounts`：
//! 写侧只改 strategy 子域，读侧只读 cash/positions 等账务事实；读侧持有
//! [`AccountBook`] 的浅克隆（页 Arc 计数 +1），写侧 `Arc::make_mut` 因此必然先克隆
//! 页再写入，读侧永远观察不到写侧的原地变更——并发安全由 Arc 语义结构性保证。

use crate::{
    GameSession, MarketMinuteClose, NpcAttentionState, PendingNpcBatch, RetailExperienceState,
    StockCatalog, StockId, SessionSetup,
};
use std::collections::{BTreeMap, VecDeque};

use super::EnvelopeLedger;
use crate::experience::PersonalHistoryReadLedger;
use crate::session::candles::SessionCandleBook;
use crate::plans::{PlanBook, UrgencyPolicy};
use crate::session::attention::NpcAttentionScheduler;
use crate::session::shared_ingress::IngressBinding;
use crate::session::decision_chain::personal_state::BeliefParticipantState;
use crate::session::{
    AccountBook, AccountPagedMap, CivilClock, IngressReceiptCursors, NpcOrderLifecycle,
    ParentOrderPlan, RetailDecisionTrace,
};

/// 共享只读域：NPC 排队与投机准备都不写（除 accounts 的 strategy 子域，见模块文档）。
///
/// `accounts` 为 owned 浅克隆快照：构造成本是页 Arc 计数自增（微秒级），
/// 同时强制写侧走 copy-on-write。后续棒转换 prepare/queue 链时按需增删字段；
/// 新增字段必须先确认 queue 侧对应字段确属只读。
pub(in crate::session) struct SessionReadView<'a> {
    pub(in crate::session) setup: &'a SessionSetup,
    pub(in crate::session) stocks: &'a StockCatalog,
    pub(in crate::session) seed: u64,
    pub(in crate::session) markets: &'a BTreeMap<StockId, crate::Market>,
    pub(in crate::session) auction_orders: &'a BTreeMap<StockId, Vec<crate::AuctionOrderSnap>>,
    pub(in crate::session) envelope_ledger: &'a EnvelopeLedger,
    pub(in crate::session) npc_order_lifecycles: &'a [NpcOrderLifecycle],
    pub(in crate::session) price_history: &'a BTreeMap<StockId, VecDeque<crate::Money>>,
    pub(in crate::session) market_minute_closes: &'a BTreeMap<StockId, Vec<MarketMinuteClose>>,
    pub(in crate::session) candle_book: &'a SessionCandleBook,
    pub(in crate::session) urgency_policy: &'a UrgencyPolicy,
    pub(in crate::session) civil_clock: &'a CivilClock,
    /// 公司域只读（issuer 翻译与公开信息库；observe_retail_analysis 消费）。
    pub(in crate::session) company_registry:
        &'a std::sync::Arc<crate::company::CompanyRegistry>,
    pub(in crate::session) operations:
        &'a std::sync::Arc<crate::company::operations::CompanyOperations>,
    pub(in crate::session) library: &'a std::sync::Arc<crate::information::PublicLibrary>,
    pub(in crate::session) tick: u64,
    pub(in crate::session) day: u32,
    /// 浅克隆快照；写侧对 strategy 的更新不会在此可见（页面 COW）。
    pub(in crate::session) accounts: AccountBook,
}

impl GameSession {
    /// 普通（非投机）路径的读视图构造：同样经浅克隆快照消费 accounts，
    /// 保证两条路径读到的形态一致。
    pub(in crate::session) fn read_view(&self) -> SessionReadView<'_> {
        SessionReadView {
            setup: &self.state.setup,
            stocks: &self.state.stocks,
            seed: self.state.seed,
            markets: &self.state.markets,
            auction_orders: &self.state.auction_orders,
            envelope_ledger: &self.state.envelope_ledger,
            npc_order_lifecycles: &self.state.npc_order_lifecycles,
            price_history: &self.state.price_history,
            market_minute_closes: &self.state.market_minute_closes,
            candle_book: &self.state.candle_book,
            urgency_policy: &self.state.urgency_policy,
            civil_clock: &self.state.civil_clock,
            company_registry: &self.state.company_registry,
            operations: &self.state.operations,
            library: &self.state.library,
            tick: self.state.tick,
            day: self.state.day,
            accounts: self.state.accounts.clone(),
        }
    }
}

/// NPC 管线专写域（读集之外的全部写点，已逐函数核实，见 x1-progress.md）。
pub(in crate::session) struct NpcWriteFields<'a> {
    pub(in crate::session) attention_scheduler: &'a mut NpcAttentionScheduler,
    pub(in crate::session) npc_attention: &'a mut AccountPagedMap<NpcAttentionState>,
    pub(in crate::session) retail_experience: &'a mut AccountPagedMap<RetailExperienceState>,
    pub(in crate::session) last_retail_decisions: &'a mut Vec<RetailDecisionTrace>,
    pub(in crate::session) pending_npc: &'a mut Option<PendingNpcBatch>,
    pub(in crate::session) ingress_receipt_cursors: &'a mut IngressReceiptCursors,
    /// strategy 子域写点（npc_state_projection 的 restore_strategy）。
    pub(in crate::session) accounts: &'a mut AccountBook,
    /// GameSession 顶层 ingress 绑定（receive_private_intent 的外部半边）。
    pub(in crate::session) ingress: &'a mut Option<IngressBinding>,
    /// 决策链个人状态 take/install 的另两份存储（npc_attention 已在上方）。
    pub(in crate::session) belief_participants: &'a mut AccountPagedMap<BeliefParticipantState>,
    pub(in crate::session) history_reads: &'a mut AccountPagedMap<PersonalHistoryReadLedger>,
    /// 个人计划记忆（capture_retail_analysis 的 PlanPersonalState take/install）。
    pub(in crate::session) plans: &'a mut PlanBook,
    /// 机构母单物化（materialize_parent_order_intents）。
    pub(in crate::session) parent_orders:
        &'a mut BTreeMap<crate::AccountId, BTreeMap<StockId, ParentOrderPlan>>,
}

/// 不相交字段借用拆分。返回的两组可同时安全使用；`accounts` 读写两侧各持一份
/// （读侧为浅克隆快照，写侧为原 map）。
pub(in crate::session) fn npc_split(
    session: &mut GameSession,
) -> (SessionReadView<'_>, NpcWriteFields<'_>) {
    let state = &mut session.state;
    let read = SessionReadView {
        setup: &state.setup,
        stocks: &state.stocks,
        seed: state.seed,
        markets: &state.markets,
        auction_orders: &state.auction_orders,
        envelope_ledger: &state.envelope_ledger,
        npc_order_lifecycles: &state.npc_order_lifecycles,
        price_history: &state.price_history,
        market_minute_closes: &state.market_minute_closes,
        candle_book: &state.candle_book,
        urgency_policy: &state.urgency_policy,
        civil_clock: &state.civil_clock,
        company_registry: &state.company_registry,
        operations: &state.operations,
        library: &state.library,
        tick: state.tick,
        day: state.day,
        accounts: state.accounts.clone(),
    };
    let write = NpcWriteFields {
        attention_scheduler: &mut state.attention_scheduler,
        npc_attention: &mut state.npc_attention,
        retail_experience: &mut state.retail_experience,
        last_retail_decisions: &mut state.last_retail_decisions,
        pending_npc: &mut state.pending_npc,
        ingress_receipt_cursors: &mut state.ingress_receipt_cursors,
        accounts: &mut state.accounts,
        ingress: &mut session.ingress,
        belief_participants: &mut state.belief_participants,
        history_reads: &mut state.history_reads,
        plans: &mut state.plans,
        parent_orders: &mut state.parent_orders,
    };
    (read, write)
}

impl<'a> SessionReadView<'a> {
    /// 与 [`GameSession::phase`] 相同的相位推导（tick+setup 纯函数）。
    pub(in crate::session) fn phase(&self) -> crate::TradingPhase {
        let day_tick = self.tick % self.setup.ticks_per_day;
        let auction_entry_ticks = self.setup.auction_ticks - self.setup.auction_ticks / 3;
        let closing_auction_start_tick =
            self.setup.ticks_per_day - self.setup.closing_auction_ticks;
        if day_tick < auction_entry_ticks {
            crate::TradingPhase::CallAuction
        } else if day_tick < self.setup.auction_ticks {
            crate::TradingPhase::PreOpen
        } else if day_tick >= closing_auction_start_tick {
            crate::TradingPhase::ClosingAuction
        } else {
            crate::TradingPhase::Continuous
        }
    }

    pub(in crate::session) fn civil_date(&self) -> crate::calendar::CivilDate {
        self.civil_clock.current_date()
    }

    /// 与 GameSession::issuer_of_stock 相同的数值 id → 发行人翻译。
    pub(in crate::session) fn issuer_of_stock(
        &self,
        id: StockId,
    ) -> Option<crate::company::CompanyId> {
        let code = self.stocks.code(id)?;
        self.company_registry
            .issuer_of(&crate::account::StockCode(code.to_string()))
    }

    /// 与 [`GameSession::current_market_minute`] 相同的分钟推导。
    pub(in crate::session) fn current_market_minute(&self) -> u64 {
        let day_start = u64::from(self.day)
            .checked_mul(u64::from(crate::GAME_INTRADAY_MINUTES_PER_DAY))
            .expect("u32 session day times 240 fits u64");
        let day_tick = self.tick % self.setup.ticks_per_day;
        let continuous_ticks_per_day = self
            .setup
            .ticks_per_day
            .saturating_sub(self.setup.auction_ticks)
            .saturating_sub(self.setup.closing_auction_ticks);
        let completed = crate::observation::completed_market_minute_count(
            day_tick
                .saturating_sub(self.setup.auction_ticks)
                .min(continuous_ticks_per_day),
            continuous_ticks_per_day,
        )
        .expect("validated session timing maps to market minutes");
        day_start + u64::from(completed)
    }
}

/// 只取专写组（不需要读视图时的窄构造）。
pub(in crate::session) fn npc_writes(session: &mut GameSession) -> NpcWriteFields<'_> {
    let state = &mut session.state;
    NpcWriteFields {
        attention_scheduler: &mut state.attention_scheduler,
        npc_attention: &mut state.npc_attention,
        retail_experience: &mut state.retail_experience,
        last_retail_decisions: &mut state.last_retail_decisions,
        pending_npc: &mut state.pending_npc,
        ingress_receipt_cursors: &mut state.ingress_receipt_cursors,
        accounts: &mut state.accounts,
        ingress: &mut session.ingress,
        belief_participants: &mut state.belief_participants,
        history_reads: &mut state.history_reads,
        plans: &mut state.plans,
        parent_orders: &mut state.parent_orders,
    }
}

/// receive_private_intent 的字段组版本（模式范例）：行为与
/// [`GameSession::receive_private_intent`] 完全一致。
pub(in crate::session) fn receive_private_intent_on(
    ingress: &Option<IngressBinding>,
    cursors: &mut IngressReceiptCursors,
    owner: crate::AccountId,
    intent: crate::Intent,
) -> Result<crate::ReceiptBearingIntent, crate::SessionError> {
    match ingress {
        Some(binding) => {
            let received = binding.source.receive_private(owner, intent)?;
            cursors
                .next_account_ordinal
                .insert(owner, received.account_ordinal + 1);
            let code = match &received.intent {
                crate::Intent::PlaceLimit { code, .. }
                | crate::Intent::PlaceMarket { code, .. }
                | crate::Intent::Cancel { code, .. } => code.clone(),
            };
            cursors
                .next_stock_ordinal
                .insert(code, received.stock_ordinal + 1);
            Ok(received)
        }
        None => cursors.receive(owner, intent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::npc_working_quote_tests;
    use crate::{AccountId, Money};
    

    fn session() -> GameSession {
        let setup = npc_working_quote_tests::quote_setup(0);
        GameSession::new(setup, 42).unwrap()
    }

    /// 拆分后两组借用必须能在 rayon::join 中并存，且写侧对 accounts 的变更
    /// 因页面 COW 不进入读侧快照。
    #[test]
    fn split_allows_concurrent_write_and_snapshot_read() {
        let mut session = session();
        let account = AccountId(1);
        let cash_before = session.state.accounts.get(&account).unwrap().cash();
        let tick_before = session.state.tick;
        let seed_before = session.state.seed;
        let (view, mut writes) = npc_split(&mut session);
        let live_cash = rayon::join(
            || {
                let entry = writes.accounts.get_mut(&account).unwrap();
                entry.fixture_set_cash(Money::from_cents(cash_before.cents() + 100));
                entry.cash()
            },
            || view.accounts.get(&account).unwrap().cash(),
        );
        // 写侧在 live map 上生效，读侧快照保持拆分时刻的值。
        assert_eq!(live_cash.0.cents(), cash_before.cents() + 100);
        assert_eq!(live_cash.1, cash_before);
        // 读视图的其余字段与拆分时刻一致（抽样）。
        assert_eq!(view.tick, tick_before);
        assert_eq!(view.seed, seed_before);
    }

    /// 字段组版 receive 与既有方法行为一致（无 ingress 绑定时走 cursors 本地受理）。
    #[test]
    fn receive_on_fields_matches_session_method() {
        let mut session = session();
        let account = AccountId(1);
        let intent = crate::Intent::Cancel {
            code: StockId(0),
            id: crate::OrderId(7),
        };
        let expected = session.receive_private_intent(account, intent.clone()).unwrap();
        let tick_before = session.state.tick;
        let (view, writes) = npc_split(&mut session);
        let through_fields =
            receive_private_intent_on(writes.ingress, writes.ingress_receipt_cursors, account, intent)
                .unwrap();
        assert_eq!(
            (through_fields.account_ordinal, through_fields.stock_ordinal),
            (expected.account_ordinal + 1, expected.stock_ordinal + 1)
        );
        assert_eq!(view.tick, tick_before);
    }
}
