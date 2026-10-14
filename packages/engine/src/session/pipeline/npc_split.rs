//! NPC 决策管线与只读准备工作的字段级借用拆分（X1 前置，见 agents/engine-max-performance/x1-progress.md）。
//!
//! [`npc_split`] 把 [`GameSession`] 的可变状态按"NPC 管线专写域 / 共享只读域"切成两组
//! 不相交借用，使 commit 尾部可以在 `rayon::join` 中一边排 NPC 队列、一边投机准备
//! 下一 tick 的 read-only start。`accounts` 与 `npc_order_lifecycles` 同时被两侧触碰：
//! `npc_split` 对它们持有 owned snapshot；账户写侧由 Arc COW 隔离，lifecycle 写侧
//! 与独立 Vec 隔离。普通 `read_view(&self)` 则借用这两域，Rust 共享借用禁止同期间
//! 持有它们的可写 authority，不需要复制目录或 Vec。

use crate::{
    GameSession, MarketMinuteClose, NpcAttentionState, RetailExperienceState,
    SessionSetup, StockCatalog, StockId,
};
use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};

use super::EnvelopeLedger;
use crate::experience::PersonalHistoryReadLedger;
use crate::plans::{PlanBook, UrgencyPolicy};
use crate::session::attention::NpcAttentionScheduler;
use crate::session::candles::SessionCandleBook;
use crate::session::decision_chain::personal_state::BeliefParticipantState;
use crate::session::shared_ingress::IngressBinding;
use crate::session::{
    AccountBook, AccountPagedMap, CivilClock, IngressReceiptCursors, NpcOrderLifecycle,
    RetailDecisionTrace,
};

/// 读取域。普通消费方借用 accounts/lifecycles；与 NpcWriteFields 并存时必须 owned。
///
/// markets、账本、行情和公司公开信息只共享不可变借用；个人状态与计划只在写组。
/// 新消费方若需要在 view 生存期内修改某域，必须先隔离该域，不能借用可写 authority。
pub(in crate::session) struct SessionReadView<'a> {
    pub(in crate::session) compute: &'a std::sync::Arc<crate::compute::ComputeContext>,
    pub(in crate::session) setup: &'a SessionSetup,
    /// 创建/恢复时固定的费率快照（任务65 #1 前置条件守卫；与 setup.config 比对）。
    pub(in crate::session) validated_config: &'a crate::GameConfig,
    pub(in crate::session) stocks: &'a StockCatalog,
    pub(in crate::session) seed: u64,
    pub(in crate::session) markets: &'a BTreeMap<StockId, crate::Market>,
    pub(in crate::session) envelope_ledger: &'a EnvelopeLedger,
    /// npc_split 的 queue 侧会移除 lifecycle；该构造必须保留 owned Vec。
    pub(in crate::session) npc_order_lifecycles: Cow<'a, [NpcOrderLifecycle]>,
    pub(in crate::session) price_history: &'a BTreeMap<StockId, VecDeque<crate::Money>>,
    pub(in crate::session) market_minute_closes: &'a BTreeMap<StockId, Vec<MarketMinuteClose>>,
    pub(in crate::session) candle_book: &'a SessionCandleBook,
    pub(in crate::session) urgency_policy: &'a UrgencyPolicy,
    pub(in crate::session) civil_clock: &'a CivilClock,
    /// 公司域只读（issuer 翻译与公开信息库；observe_retail_analysis 消费）。
    pub(in crate::session) company_registry: &'a std::sync::Arc<crate::company::CompanyRegistry>,
    pub(in crate::session) operations:
        &'a std::sync::Arc<crate::company::operations::CompanyOperations>,
    pub(in crate::session) library: &'a std::sync::Arc<crate::information::PublicLibrary>,
    pub(in crate::session) tick: u64,
    pub(in crate::session) day: u32,
    /// npc_split 持有独立目录 snapshot，账户页/strategy 的写入由 COW 隔离。
    pub(in crate::session) accounts: Cow<'a, AccountBook>,
}

impl GameSession {
    /// 普通 &self 消费方：全部借用，不复制账户目录或 lifecycle Vec。
    pub(in crate::session) fn read_view(&self) -> SessionReadView<'_> {
        SessionReadView {
            compute: &self.compute,
            setup: &self.state.setup,
            validated_config: &self.state.validated_config,
            stocks: &self.state.stocks,
            seed: self.state.seed,
            markets: &self.state.markets,
            envelope_ledger: &self.state.envelope_ledger,
            npc_order_lifecycles: Cow::Borrowed(&self.state.npc_order_lifecycles),
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
            accounts: Cow::Borrowed(&self.state.accounts),
        }
    }
}

/// NPC 管线专写域（读集之外的全部写点，已逐函数核实，见 x1-progress.md）。
pub(in crate::session) struct NpcWriteFields<'a> {
    pub(in crate::session) attention_scheduler: &'a mut NpcAttentionScheduler,
    pub(in crate::session) npc_attention: &'a mut AccountPagedMap<NpcAttentionState>,
    pub(in crate::session) retail_experience: &'a mut AccountPagedMap<RetailExperienceState>,
    pub(in crate::session) last_retail_decisions: &'a mut Vec<RetailDecisionTrace>,
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
    /// 母单成交后续发时移除对应生命周期（orders.rs:163，第 12 写字段）。
    pub(in crate::session) npc_order_lifecycles: &'a mut Vec<NpcOrderLifecycle>,
}

/// 不相交字段借用拆分。返回的两组可同时安全使用；`accounts` 读写两侧各持一份
/// （读侧为浅克隆快照，写侧为原 map）。
pub(in crate::session) fn npc_split(
    session: &mut GameSession,
) -> (SessionReadView<'_>, NpcWriteFields<'_>) {
    let state = &mut session.state;
    let read = SessionReadView {
        compute: &session.compute,
        setup: &state.setup,
        validated_config: &state.validated_config,
        stocks: &state.stocks,
        seed: state.seed,
        markets: &state.markets,
        envelope_ledger: &state.envelope_ledger,
        npc_order_lifecycles: Cow::Owned(state.npc_order_lifecycles.clone()),
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
        accounts: Cow::Owned(state.accounts.clone()),
    };
    let write = NpcWriteFields {
        attention_scheduler: &mut state.attention_scheduler,
        npc_attention: &mut state.npc_attention,
        retail_experience: &mut state.retail_experience,
        last_retail_decisions: &mut state.last_retail_decisions,
        ingress_receipt_cursors: &mut state.ingress_receipt_cursors,
        accounts: &mut state.accounts,
        ingress: &mut session.ingress,
        belief_participants: &mut state.belief_participants,
        history_reads: &mut state.history_reads,
        plans: &mut state.plans,
        npc_order_lifecycles: &mut state.npc_order_lifecycles,
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

    /// 与 [`GameSession::observation_civil_instant`] 相同的观察时刻推导。
    pub(in crate::session) fn observation_civil_instant(&self) -> crate::calendar::CivilInstant {
        let day_tick = self.tick - u64::from(self.day) * self.setup.ticks_per_day;
        let continuous =
            self.setup.ticks_per_day - self.setup.auction_ticks - self.setup.closing_auction_ticks;
        let second = if day_tick < self.setup.auction_ticks {
            9 * 3600 + 15 * 60 + day_tick * 900 / self.setup.auction_ticks
        } else if self.setup.closing_auction_ticks > 0
            && day_tick >= self.setup.ticks_per_day - self.setup.closing_auction_ticks
        {
            14 * 3600
                + 57 * 60
                + (day_tick - (self.setup.ticks_per_day - self.setup.closing_auction_ticks)) * 180
                    / self.setup.closing_auction_ticks
        } else {
            let duration = if self.setup.closing_auction_ticks > 0 {
                14_220
            } else {
                14_400
            };
            let seconds = (day_tick - self.setup.auction_ticks) * duration / continuous;
            9 * 3600 + 30 * 60 + seconds + if seconds >= 7200 { 5400 } else { 0 }
        };
        crate::calendar::CivilInstant::new(
            self.civil_date(),
            u32::try_from(second).expect("validated intraday seconds"),
        )
        .expect("validated civil session date")
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
        ingress_receipt_cursors: &mut state.ingress_receipt_cursors,
        accounts: &mut state.accounts,
        ingress: &mut session.ingress,
        belief_participants: &mut state.belief_participants,
        history_reads: &mut state.history_reads,
        plans: &mut state.plans,
        npc_order_lifecycles: &mut state.npc_order_lifecycles,
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

    /// 普通 &self 消费方必须借用原目录与 lifecycle slice，不能制造 COW snapshot。
    #[test]
    fn ordinary_view_borrows_account_directory_and_lifecycle_storage() {
        let mut session = session();
        session.state.npc_order_lifecycles.push(NpcOrderLifecycle {
            account: AccountId(1),
            code: StockId(0),
            order_id: crate::OrderId(999),
            placed_market_minute: 0,
            expires_market_minute: 1,
        });
        let revision = session.revision.load(std::sync::atomic::Ordering::Relaxed);
        let view = session.read_view();
        let accounts: &AccountBook = &view.accounts;
        let lifecycles: &[NpcOrderLifecycle] = &view.npc_order_lifecycles;
        assert!(std::ptr::eq(accounts, &session.state.accounts), "普通 view 复制了账户目录");
        assert!(std::ptr::eq(lifecycles, session.state.npc_order_lifecycles.as_slice()), "普通 view 复制了 lifecycle Vec");
        assert_eq!(session.revision.load(std::sync::atomic::Ordering::Relaxed), revision);
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
        let live_cash = crate::compute::join(
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
        // lifecycles 快照与写侧隔离：join 内 push 不进读侧。
        let lifecycles_before = view.npc_order_lifecycles.len();
        let (pushed, seen) = crate::compute::join(
            || {
                writes
                    .npc_order_lifecycles
                    .push(crate::session::NpcOrderLifecycle {
                        account: account,
                        code: StockId(0),
                        order_id: crate::OrderId(999),
                        placed_market_minute: 0,
                        expires_market_minute: 1,
                    });
                writes.npc_order_lifecycles.len()
            },
            || view.npc_order_lifecycles.len(),
        );
        assert_eq!(pushed, lifecycles_before + 1);
        assert_eq!(seen, lifecycles_before);
        // 读视图的其余字段与拆分时刻一致（抽样）。
        assert_eq!(view.tick, tick_before);
        assert_eq!(view.seed, seed_before);
    }

    /// 隔离 view 与写组并发时，strategy 替换与已有 lifecycle 修改不进入 snapshot。
    #[test]
    fn split_isolates_strategy_and_existing_lifecycle_changes() {
        let mut session = session();
        let revision = session.external_revision();
        let account = AccountId(1);
        session.state.npc_order_lifecycles.push(NpcOrderLifecycle {
            account,
            code: StockId(0),
            order_id: crate::OrderId(321),
            placed_market_minute: 0,
            expires_market_minute: 9,
        });
        assert!(session.state.accounts[&account].has_strategy());
        let (view, writes) = npc_split(&mut session);
        assert!(matches!(view.accounts, Cow::Owned(_)));
        assert!(matches!(view.npc_order_lifecycles, Cow::Owned(_)));
        let ((), observed) = crate::compute::join(
            || {
                writes.accounts.get_mut(&account).unwrap().restore_strategy(None);
                writes.npc_order_lifecycles[0].expires_market_minute = 99;
                writes.npc_order_lifecycles.clear();
            },
            || {
                (view.accounts[&account].has_strategy(), view.npc_order_lifecycles[0].expires_market_minute)
            },
        );
        assert_eq!(observed, (true, 9));
        assert!(view.accounts[&account].has_strategy());
        assert_eq!(view.npc_order_lifecycles.len(), 1);
        drop(view);
        assert!(!session.state.accounts[&account].has_strategy());
        assert!(session.state.npc_order_lifecycles.is_empty());
        assert_eq!(session.external_revision(), revision);
    }

    /// 即使普通 view 本身借用，资源对象也要独立拥有 snapshot，不能跟随 authority 更新。
    #[test]
    fn sealed_resources_keep_cash_and_partially_t1_locked_position_after_view_drop() {
        let mut session = session();
        let account = AccountId(1);
        let stock = StockId(0);
        // quote_setup(0) 不分配流通股；显式建立合法 Position，禁止条件分支空转。
        session.state.accounts.get_mut(&account).unwrap().fixture_insert_position(
            stock,
            crate::account::Position::from_restored_parts(300, 100, 300_000, 0),
        );
        let cash = session.state.accounts[&account].cash();
        let position = session.state.accounts[&account]
            .position(&stock)
            .expect("fixture 必须包含非零持仓，不能跳过 T+1 断言");
        assert_eq!(position.qty(), 300);
        assert_eq!(position.sellable(), 200);
        assert_eq!(position.t1_locked(), 100);

        let view = session.read_view();
        let borrowed_position = view.accounts[&account].position(&stock).unwrap();
        assert!(std::ptr::eq(position, borrowed_position));
        let resources = super::super::decision_resources::DecisionResourceSnapshot::seal_on(&view).unwrap();
        assert_eq!(resources.total_held_qty(account, &stock).unwrap(), borrowed_position.qty());
        assert_eq!(resources.sellable_before_reservation(account, &stock).unwrap(), borrowed_position.sellable());
        assert_eq!(resources.total_held_qty(account, &stock).unwrap() - resources.sellable_before_reservation(account, &stock).unwrap(), borrowed_position.t1_locked());
        drop(view);

        let entry = session.state.accounts.get_mut(&account).unwrap();
        entry.fixture_set_cash(Money::from_cents(cash.cents() + 123));
        entry.fixture_insert_position(stock, crate::account::Position::from_restored_parts(500, 200, 500_000, 0));
        assert_eq!(resources.raw_cash(account).unwrap(), cash);
        assert_eq!(resources.total_held_qty(account, &stock).unwrap(), 300);
        assert_eq!(resources.sellable_before_reservation(account, &stock).unwrap(), 200);
        assert_eq!(resources.available_sell_qty(account, &stock).unwrap(), 200);
        assert_eq!(resources.reserved_sell_qty(account, &stock).unwrap(), 0);
        assert_eq!(resources.total_held_qty(account, &stock).unwrap() - resources.sellable_before_reservation(account, &stock).unwrap(), 100);
        let live = session.state.accounts[&account].position(&stock).unwrap();
        assert_eq!(session.state.accounts[&account].cash().cents(), cash.cents() + 123);
        assert_eq!((live.qty(), live.sellable(), live.t1_locked()), (500, 300, 200));
    }

    /// 非空挂卖预留的封前事实：authority 预留增长后，旧 snapshot 不回灌；
    /// 新 seal 反映增长，证明旧值是封前事实而非空转断言。
    #[test]
    fn sealed_resources_keep_non_empty_sell_reservation_isolated_from_authority_growth() {
        let mut session = session();
        // 用 Player 账户：seed helper 对 NPC 账户会先撤销同向旧挂单，无法累积两笔。
        let account = AccountId(0);
        let stock = StockId(0);
        session.state.accounts.get_mut(&account).unwrap().fixture_insert_position(
            stock,
            crate::account::Position::from_restored_parts(300, 100, 300_000, 0),
        );
        session.seed_order_for_test(
            account,
            crate::Intent::PlaceLimit {
                code: stock,
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_100)),
                qty: 50,
            },
            &mut Vec::new(),
        );
        session.hydrate_or_validate_envelope_ledger().unwrap();
        // 前置：挂卖预留确实非空（quote_setup(0) 的账户 1 无其他挂单）。
        let live_before = session.state.envelope_ledger.live_reservations().unwrap();
        assert_eq!(
            live_before.get(account).and_then(|row| row.sell.get(&stock).copied()),
            Some(50),
            "前置失败：预留为空，后续断言将空转"
        );

        let resources = super::super::decision_resources::DecisionResourceSnapshot::seal_on(&session.read_view()).unwrap();
        assert_eq!(resources.total_held_qty(account, &stock).unwrap(), 300);
        assert_eq!(resources.sellable_before_reservation(account, &stock).unwrap(), 200);
        assert_eq!(resources.reserved_sell_qty(account, &stock).unwrap(), 50);
        assert_eq!(resources.available_sell_qty(account, &stock).unwrap(), 150);

        // authority 预留增长：再挂 30 股并重新水化（反向断言状态确实变了）。
        session.seed_order_for_test(
            account,
            crate::Intent::PlaceLimit {
                code: stock,
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_100)),
                qty: 30,
            },
            &mut Vec::new(),
        );
        session.hydrate_or_validate_envelope_ledger().unwrap();
        let live_after = session.state.envelope_ledger.live_reservations().unwrap();
        assert_eq!(
            live_after.get(account).and_then(|row| row.sell.get(&stock).copied()),
            Some(80)
        );

        // 旧 snapshot 保持封前事实；新 seal 反映增长。
        assert_eq!(resources.reserved_sell_qty(account, &stock).unwrap(), 50);
        assert_eq!(resources.available_sell_qty(account, &stock).unwrap(), 150);
        let fresh = super::super::decision_resources::DecisionResourceSnapshot::seal_on(&session.read_view()).unwrap();
        assert_eq!(fresh.reserved_sell_qty(account, &stock).unwrap(), 80);
        assert_eq!(fresh.available_sell_qty(account, &stock).unwrap(), 120);
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
        let expected = session
            .receive_private_intent(account, intent.clone())
            .unwrap();
        let tick_before = session.state.tick;
        let (view, writes) = npc_split(&mut session);
        let through_fields = receive_private_intent_on(
            writes.ingress,
            writes.ingress_receipt_cursors,
            account,
            intent,
        )
        .unwrap();
        assert_eq!(
            (through_fields.account_ordinal, through_fields.stock_ordinal),
            (expected.account_ordinal + 1, expected.stock_ordinal + 1)
        );
        assert_eq!(view.tick, tick_before);
    }
}
