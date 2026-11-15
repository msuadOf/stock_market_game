//! 权威输出和观察者是不同的生命周期。有限的观察队列只保障在线增量，不能替代权威事实存档。
//! 每次新 generation 持有独立队列；旧 sender 或观察游标不能切到新代。
use libos::{
    observer_mailbox::{self, Receiver, Sender, TryRecvError, TrySendError},
    wake_epoch::WakeEpoch,
};
use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

struct CursorBook {
    issued: AtomicU64,
    dropped_through: AtomicU64,
    urgent_through: AtomicU64,
    observer_closed: AtomicBool,
    queued_weight: AtomicUsize,
    weight_limit: usize,
    signal: Arc<WakeEpoch>,
}
struct Ordered<T> {
    ordinal: u64,
    weight: usize,
    value: T,
}

pub struct Producer<T> {
    send: Option<Sender<Ordered<T>>>,
    book: Arc<CursorBook>,
    ordinal: u64,
}

pub struct Observer<T> {
    recv: Receiver<Ordered<T>>,
    book: Arc<CursorBook>,
    after: u64,
    capacity: NonZeroUsize,
    pending: Option<Ordered<T>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Push {
    Queued { ordinal: u64 },
    Gap { ordinal: u64 },
    ObserverClosed { ordinal: u64 },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Read<T> {
    Item { ordinal: u64, value: T },
    Empty,
    Closed,
    NeedsBaseline { through: u64 },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PumpRead<T> {
    Ready(Read<T>),
    /// 观察额度已用完。这不表示邮箱已空或已排尽。
    CreditBlocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalReadiness {
    Pending,
    Ready,
    /// 缺口已经使成功前缀无法连续交付，终止通知必须带上遗漏范围。
    Missing {
        from: u64,
        through: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeError {
    Stale { through: u64 },
    Future { issued: u64 },
}

/// 对一个可靠发送者施加窗口；此状态只能由观察者任务持有，权威端没有查询接口。
pub struct DeliveryCredit {
    pending: VecDeque<u64>,
    acknowledged_through: u64,
    limit: NonZeroUsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryError {
    WindowFull,
    CursorMismatch,
}

impl DeliveryCredit {
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            pending: VecDeque::new(),
            acknowledged_through: 0,
            limit,
        }
    }

    pub fn may_send(&self) -> bool {
        self.pending.len() < self.limit.get()
    }

    fn sent_through(&self) -> u64 {
        self.pending
            .back()
            .copied()
            .unwrap_or(self.acknowledged_through)
    }

    pub fn record_sent(&mut self, ordinal: u64) -> Result<(), DeliveryError> {
        if ordinal <= self.sent_through() {
            return Err(DeliveryError::CursorMismatch);
        }
        if !self.may_send() {
            return Err(DeliveryError::WindowFull);
        }
        self.pending.push_back(ordinal);
        Ok(())
    }

    pub fn acknowledge(&mut self, ordinal: u64) -> Result<bool, DeliveryError> {
        if ordinal <= self.acknowledged_through {
            return Ok(false);
        }
        if self.pending.front().copied() != Some(ordinal) {
            return Err(DeliveryError::CursorMismatch);
        }
        self.pending.pop_front();
        self.acknowledged_through = ordinal;
        Ok(true)
    }

    pub fn install_baseline(&mut self, ordinal: u64) {
        self.pending.clear();
        self.acknowledged_through = ordinal;
    }
}

/// 观察队列除消息数外，还分别限制消息代表的事实总量。
pub const OBSERVER_PACKET_LIMIT: NonZeroUsize =
    NonZeroUsize::new(1_024).expect("观察流消息容量必须为正");
pub const OBSERVER_IN_FLIGHT_LIMIT: NonZeroUsize =
    NonZeroUsize::new(2).expect("观察流确认窗口必须为正");
pub const OBSERVER_FACT_LIMIT: NonZeroUsize =
    NonZeroUsize::new(65_536).expect("观察流事实容量必须为正");

pub fn protocol_weight(update: &super::protocol::EngineUpdate) -> NonZeroUsize {
    use super::protocol::EngineUpdate;
    let weights = |frames: &[super::protocol::TickFrame]| {
        frames.iter().fold(0usize, |sum, frame| {
            sum.saturating_add(frame.facts.len().max(1))
        })
    };
    let units = match update {
        EngineUpdate::TickBatch(batch) => weights(&batch.frames),
        EngineUpdate::CivilUpdate(civil) => civil
            .facts
            .len()
            .max(1)
            .saturating_add(weights(&civil.refresh.intraday)),
    };
    NonZeroUsize::new(units).unwrap_or(NonZeroUsize::MIN)
}

pub fn channel<T>(capacity: NonZeroUsize) -> (Producer<T>, Observer<T>) {
    channel_with_budget(capacity, OBSERVER_FACT_LIMIT)
}

pub fn channel_with_budget<T>(
    capacity: NonZeroUsize,
    weight_limit: NonZeroUsize,
) -> (Producer<T>, Observer<T>) {
    let (send, recv) = observer_mailbox::channel(capacity);
    let book = Arc::new(CursorBook {
        issued: AtomicU64::new(0),
        dropped_through: AtomicU64::new(0),
        urgent_through: AtomicU64::new(0),
        observer_closed: AtomicBool::new(false),
        queued_weight: AtomicUsize::new(0),
        weight_limit: weight_limit.get(),
        signal: Arc::new(WakeEpoch::new()),
    });
    (
        Producer {
            send: Some(send),
            book: Arc::clone(&book),
            ordinal: 0,
        },
        Observer {
            recv,
            book,
            after: 0,
            capacity,
            pending: None,
        },
    )
}

impl<T: Send + 'static> Producer<T> {
    pub fn is_observer_closed(&self) -> bool {
        self.book.observer_closed.load(Ordering::Acquire)
    }

    pub fn published_ordinal(&self) -> u64 {
        self.ordinal
    }

    pub fn signal(&self) -> &Arc<WakeEpoch> {
        &self.book.signal
    }

    pub fn publish(&mut self, value: T) -> Push {
        self.publish_weighted(value, NonZeroUsize::MIN)
    }

    pub fn publish_weighted(&mut self, value: T, weight: NonZeroUsize) -> Push {
        self.publish_inner(value, weight, false)
    }

    /// 通知观察者必须顺序排空此前的数据，但仍不等待观察者。
    pub fn publish_weighted_urgent(&mut self, value: T, weight: NonZeroUsize) -> Push {
        self.publish_inner(value, weight, true)
    }

    fn publish_inner(&mut self, value: T, weight: NonZeroUsize, urgent: bool) -> Push {
        self.ordinal = self.ordinal.checked_add(1).expect("观察流序号已耗尽");
        let ordinal = self.ordinal;
        self.book.issued.store(ordinal, Ordering::Release);
        let weight = weight.get();
        if self.book.observer_closed.load(Ordering::Acquire) {
            Self::release_discarded(value, weight);
            return Push::ObserverClosed { ordinal };
        }

        // 权威单生产者只在内存上预留额度，观察者不持锁、不向权威返回确认。
        let reserved = self
            .book
            .queued_weight
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                // 有界例外：空队列可以承接一份不可拆分的日界更新，在它出队前拒绝继续堆积。
                used.checked_add(weight)
                    .filter(|next| used == 0 || *next <= self.book.weight_limit)
            })
            .is_ok();
        let outcome = if !reserved {
            Self::release_discarded(value, weight);
            self.mark_gap(ordinal)
        } else {
            let send = self.send.as_ref().expect("观察者 producer 已在析构");
            match send.try_send(Ordered {
                ordinal,
                weight,
                value,
            }) {
                Ok(()) => Push::Queued { ordinal },
                Err(TrySendError::Full(item)) => {
                    self.book
                        .queued_weight
                        .fetch_sub(item.weight, Ordering::AcqRel);
                    Self::release_discarded(item.value, item.weight);
                    self.mark_gap(ordinal)
                }
                Err(TrySendError::Disconnected(item)) => {
                    self.book
                        .queued_weight
                        .fetch_sub(item.weight, Ordering::AcqRel);
                    Self::release_discarded(item.value, item.weight);
                    Push::ObserverClosed { ordinal }
                }
            }
        };
        if urgent && !matches!(outcome, Push::ObserverClosed { .. }) {
            self.book.urgent_through.store(ordinal, Ordering::Release);
        }
        self.signal().notify();
        outcome
    }

    fn release_discarded(value: T, weight: usize) {
        // 普通小包避免一份额外的分配。大型输出交给通用有界析构队列；
        // 析构队列已满时遵循 LibOS 现有的就地释放合同，不等待回收。
        if weight >= 1_024 {
            super::deferred_drop::defer(value);
        }
    }

    fn mark_gap(&self, ordinal: u64) -> Push {
        // 缺口通过队列之外的水位发布，不能排进已经满载的邮箱。
        self.book
            .dropped_through
            .fetch_max(ordinal, Ordering::AcqRel);
        Push::Gap { ordinal }
    }
}

impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        drop(self.send.take());
        self.book.signal.close();
    }
}

impl<T> Drop for Observer<T> {
    fn drop(&mut self) {
        self.book.observer_closed.store(true, Ordering::Release);
    }
}

impl<T> Observer<T> {
    pub fn signal(&self) -> &Arc<WakeEpoch> {
        &self.book.signal
    }

    pub fn has_urgent(&self) -> bool {
        self.book.urgent_through.load(Ordering::Acquire) > self.after
    }

    pub fn has_gap(&self) -> bool {
        self.dropped_through() > self.after
    }

    fn dropped_through(&self) -> u64 {
        self.book.dropped_through.load(Ordering::Acquire)
    }

    /// 只接受调用方已校验的同世代权威基线；参数是基线覆盖的独立观察序号。
    pub fn resume_after(&mut self, ordinal: u64) -> Result<(), ResumeError> {
        let through = self.after.max(self.dropped_through());
        if ordinal < through {
            return Err(ResumeError::Stale { through });
        }
        let issued = self.book.issued.load(Ordering::Acquire);
        if ordinal > issued {
            return Err(ResumeError::Future { issued });
        }
        self.after = ordinal;
        Ok(())
    }

    pub fn poll(&mut self) -> Read<T> {
        loop {
            let through = self.dropped_through();
            if through > self.after {
                // 丢弃基线已经必须覆盖的旧观察项，以免等待基线期间把邮箱
                // 永远挤满。每轮最多容量个元素，生产者在另一执行上下文运行。
                for _ in 0..self.capacity.get() {
                    let item = match self.pending.take().or_else(|| self.recv.try_recv().ok()) {
                        Some(item) => item,
                        None => break,
                    };
                    if item.ordinal > through {
                        self.pending = Some(item);
                        break;
                    }
                    self.book
                        .queued_weight
                        .fetch_sub(item.weight, Ordering::AcqRel);
                }
                return Read::NeedsBaseline { through };
            }
            let item = match self
                .pending
                .take()
                .map(Ok)
                .unwrap_or_else(|| self.recv.try_recv())
            {
                Ok(item) => item,
                Err(TryRecvError::Empty) => return Read::Empty,
                Err(TryRecvError::Disconnected) => return Read::Closed,
            };
            self.book
                .queued_weight
                .fetch_sub(item.weight, Ordering::AcqRel);
            if item.ordinal <= self.after {
                continue;
            }
            if self.after.checked_add(1) != Some(item.ordinal) {
                // 非连续只可能来自已发布的缺口，后继 item 也不再能交付。
                // 基线必须覆盖被消耗的后继序号才能安全恢复。
                let through = self
                    .book
                    .dropped_through
                    .fetch_max(item.ordinal, Ordering::AcqRel)
                    .max(item.ordinal);
                return Read::NeedsBaseline { through };
            }
            self.after = item.ordinal;
            return Read::Item {
                ordinal: item.ordinal,
                value: item.value,
            };
        }
    }
}

/// 将观察游标、发送窗口与恢复围栏作为一个状态机。运输层只负责提供
/// 唤醒、尝试发包和确认；观察窗口永远不被权威推进循环读取。
pub struct ObserverPump<T> {
    observer: Observer<T>,
    credit: DeliveryCredit,
}

impl<T> ObserverPump<T> {
    pub fn new(observer: Observer<T>, window: NonZeroUsize) -> Self {
        Self {
            observer,
            credit: DeliveryCredit::new(window),
        }
    }

    pub fn signal(&self) -> &Arc<WakeEpoch> {
        self.observer.signal()
    }

    /// 按需观察者也必须主动处理缺口、关键通知与终止。
    pub fn should_poll(&self, demand: bool) -> bool {
        demand || self.observer.has_gap() || self.observer.has_urgent() || self.signal().is_closed()
    }

    pub fn poll(&mut self) -> PumpRead<T> {
        if !self.credit.may_send() && !self.observer.has_gap() {
            // close 与邮箱停止生产配对；只有关闭且已经消费全部 ordinal
            // 才能在没有发送额度时报告读完。
            return if self.signal().is_closed()
                && self.observer.book.issued.load(Ordering::Acquire) <= self.observer.after
            {
                PumpRead::Ready(Read::Closed)
            } else {
                PumpRead::CreditBlocked
            };
        }
        PumpRead::Ready(self.observer.poll())
    }

    /// 终止记录必须使用权威端记录首错时的末尾 ordinal。即使刚刚
    /// poll 得到 Empty，生产者也可能在这两个读取之间交接了成功前缀。
    /// 缺口时不能再假定该前缀可连续交付，但必须公开遗漏范围。
    pub fn terminal_readiness(&self, through: u64) -> TerminalReadiness {
        let sent = self.credit.sent_through();
        if through <= sent {
            TerminalReadiness::Ready
        } else if self.observer.has_gap() {
            TerminalReadiness::Missing {
                from: sent + 1,
                through,
            }
        } else {
            TerminalReadiness::Pending
        }
    }

    pub fn record_sent(&mut self, ordinal: u64) -> Result<(), DeliveryError> {
        self.credit.record_sent(ordinal)
    }

    pub fn acknowledge(&mut self, ordinal: u64) -> Result<bool, DeliveryError> {
        self.credit.acknowledge(ordinal)
    }

    pub fn resume_after(&mut self, ordinal: u64) -> Result<(), ResumeError> {
        self.observer.resume_after(ordinal)?;
        self.credit.install_baseline(ordinal);
        Ok(())
    }
}

/// 快照和观察增量使用同一账户可见性规则；没有成员席位就不交付私有账户。
pub fn scope_observer_snapshot(snapshot: &mut crate::Snapshot, account: Option<crate::AccountId>) {
    snapshot.accounts.retain(|id, _| Some(*id) == account);
}

/// 仅在发送任务上修改已冻结输出的副本。EventFact 的既有公开时序合同不在此改变。
pub fn scope_observer_update(
    update: &mut super::protocol::EngineUpdate,
    account: Option<crate::AccountId>,
) {
    use super::protocol::EngineUpdate;
    match update {
        EngineUpdate::TickBatch(batch) => {
            if let Some(snapshot) = &mut batch.runtime_snapshot {
                scope_observer_snapshot(snapshot, account);
            }
            if let Some(delta) = &mut batch.runtime_delta {
                delta.accounts.retain(|id, _| Some(*id) == account);
                // 当前公开 RuntimeDelta 委托没有 owner 字段，只表示原始本地席位。
                if account != Some(crate::AccountId(0)) {
                    delta.working_orders.upserts.clear();
                    delta.working_orders.removed.clear();
                }
            }
        }
        EngineUpdate::CivilUpdate(civil) => {
            scope_observer_snapshot(&mut civil.refresh.snapshot, account)
        }
    }
}

/// 在不修改全局 delta 基线、不改变现有 wire 的情况下，从精确观察游标生成首次完整委托。
pub fn seed_observer_orders(
    update: &mut super::protocol::EngineUpdate,
    seed: &mut Option<Vec<super::protocol::PlayerWorkingOrder>>,
) {
    use super::protocol::EngineUpdate;
    match update {
        EngineUpdate::CivilUpdate(_) => {
            // 权威在日界后重置自己的 runtime delta；下一批已经包含完整委托。
            seed.take();
        }
        EngineUpdate::TickBatch(batch) => {
            let Some(delta) = &mut batch.runtime_delta else {
                return;
            };
            let Some(orders) = seed.take() else { return };
            let orders_delta = &mut delta.working_orders;
            if orders_delta.reset {
                return;
            }
            let mut current = orders
                .into_iter()
                .map(|order| (order.id, order))
                .collect::<std::collections::BTreeMap<_, _>>();
            for id in orders_delta.removed.drain(..) {
                current.remove(&id);
            }
            for order in orders_delta.upserts.drain(..) {
                current.insert(order.id, order);
            }
            orders_delta.reset = true;
            orders_delta.upserts = current.into_values().collect();
        }
    }
}

#[cfg(test)]
mod tests;
