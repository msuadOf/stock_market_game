use super::ledger_paged_map::LedgerPagedMap;
use super::ledger_rows::SharedRows;
use super::{
    ledger_candidate, ledger_conservation, ledger_validation, Envelope, EnvelopeAudit, EnvelopeKey,
    EnvelopeOrigin, FeeComponents, ReceiptDelta, ReceiptLocalKey, ResVec, StepFatal,
};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, Weak};

#[cfg(test)]
thread_local! {
    pub(super) static EVIDENCE_VALIDATION_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static LEDGER_CONSTRUCTION_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiptKind {
    Fill,
    PriceResolved {
        requested: crate::LimitPrice,
        before: crate::Money,
        after: crate::Money,
    },
    Release,
    Reject,
    Rollover,
}

#[derive(Clone, Debug)]
pub struct EnvelopeReceipt {
    pub index: u64,
    pub local_key: ReceiptLocalKey,
    pub envelope: EnvelopeKey,
    pub kind: ReceiptKind,
    pub qty_before: u32,
    pub qty_after: u32,
    pub value_before: crate::Money,
    pub value_after: crate::Money,
    pub delta: ReceiptDelta,
    pub nominal: FeeComponents,
    pub charged: FeeComponents,
    pub charged_before: FeeComponents,
    pub charged_after: FeeComponents,
    pub deliver_qty: u32,
    pub deliver_cash: crate::Money,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(super) struct ConservationState {
    pub(super) expiry_released: ResVec,
    pub(super) sealed_spent: ResVec,
    pub(super) sealed_released: ResVec,
}

impl ConservationState {
    pub(super) const EMPTY: Self = Self {
        expiry_released: ResVec::ZERO,
        sealed_spent: ResVec::ZERO,
        sealed_released: ResVec::ZERO,
    };
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct EnvelopeLedger {
    pub(super) envelopes: SharedRows<LedgerPagedMap<Envelope>>,
    pub(super) terminal_envelopes: SharedRows<LedgerPagedMap<Envelope>>,
    pub(super) audits: SharedRows<LedgerPagedMap<EnvelopeAudit>>,
    pub(super) conservation: SharedRows<LedgerPagedMap<ConservationState>>,
    pub(super) seen_local_keys: BTreeSet<ReceiptLocalKey>,
    pub(super) next_receipt_index: u64,
    #[serde(skip)]
    evidence_cache: Arc<Mutex<Option<EvidenceCache>>>,
    #[serde(skip)]
    reservation_cache: Arc<Mutex<super::live_reservations::ReservationCache>>,
}

#[derive(Debug)]
struct EvidenceCache {
    envelopes: Weak<LedgerPagedMap<Envelope>>,
    terminal: Weak<LedgerPagedMap<Envelope>>,
    audits: Weak<LedgerPagedMap<EnvelopeAudit>>,
    conservation: Weak<LedgerPagedMap<ConservationState>>,
}

impl EvidenceCache {
    fn capture(ledger: &EnvelopeLedger) -> Self {
        Self {
            envelopes: ledger.envelopes.identity(),
            terminal: ledger.terminal_envelopes.identity(),
            audits: ledger.audits.identity(),
            conservation: ledger.conservation.identity(),
        }
    }
}

impl PartialEq for EnvelopeLedger {
    fn eq(&self, other: &Self) -> bool {
        self.envelopes == other.envelopes
            && self.terminal_envelopes == other.terminal_envelopes
            && self.audits == other.audits
            && self.conservation == other.conservation
            && self.seen_local_keys == other.seen_local_keys
            && self.next_receipt_index == other.next_receipt_index
    }
}
impl Eq for EnvelopeLedger {}

/// JSON-safe canonical projection used by the session business hash.
#[derive(serde::Serialize)]
pub(in crate::session) struct EnvelopeLedgerHashProjection<'a> {
    envelopes: Vec<(&'a EnvelopeKey, &'a Envelope)>,
    terminal_envelopes: Vec<(&'a EnvelopeKey, &'a Envelope)>,
    audits: Vec<(&'a EnvelopeKey, &'a EnvelopeAudit)>,
    conservation: Vec<(&'a EnvelopeKey, &'a ConservationState)>,
    seen_local_keys: Vec<&'a ReceiptLocalKey>,
    next_receipt_index: u64,
}

impl EnvelopeLedger {
    pub(in crate::session) fn hash_projection(&self) -> EnvelopeLedgerHashProjection<'_> {
        EnvelopeLedgerHashProjection {
            envelopes: self.envelopes.iter().collect(),
            terminal_envelopes: self.terminal_envelopes.iter().collect(),
            audits: self.audits.iter().collect(),
            conservation: self.conservation.iter().collect(),
            seen_local_keys: self.seen_local_keys.iter().collect(),
            next_receipt_index: self.next_receipt_index,
        }
    }

    pub fn new(
        next_receipt_index: u64,
        envelopes: impl IntoIterator<Item = Envelope>,
    ) -> Result<Self, StepFatal> {
        #[cfg(test)]
        LEDGER_CONSTRUCTION_COUNT.with(|count| count.set(count.get() + 1));
        // 直接构建分页行；不先分配完整 BTreeMap 再逐行转入第二份树。
        let mut values = LedgerPagedMap::default();
        let mut audits = LedgerPagedMap::default();
        let mut conservation = LedgerPagedMap::default();
        for envelope in envelopes {
            // 构造期只做结构校验；出生财务不变量属于证明建立（verified_live），
            // 不得在restore类型化fault层（如2228）之前触发。
            envelope.validate()?;
            let key = envelope.key().clone();
            let audit = envelope.audit();
            if values.insert(key.clone(), envelope).is_some() {
                return Err(ledger_validation::invariant("duplicate envelope"));
            }
            audits.insert(key.clone(), audit);
            conservation.insert(key, ConservationState::EMPTY);
        }
        Ok(Self {
            envelopes: values.into(),
            terminal_envelopes: SharedRows::default(),
            audits: audits.into(),
            conservation: conservation.into(),
            seen_local_keys: BTreeSet::new(),
            next_receipt_index,
            evidence_cache: Arc::default(),
            reservation_cache: Arc::default(),
        })
    }

    pub(in crate::session) fn live_reservations(
        &self,
    ) -> Result<super::LiveReservations, StepFatal> {
        let mut cache = self.reservation_cache.lock().map_err(|error| {
            ledger_validation::invariant(&format!("ledger 预留投影缓存锁失败：{error}"))
        })?;
        cache.project(&self.envelopes)
    }

    /// P2-4b：runtime 路径信封增量安装（upsert——新增或整值替换）。
    /// 与 [`Self::new`] 的等价合同：envelope.validate 同校验；audits 随
    /// envelope 替换；conservation 恒 EMPTY（runtime 路径无回执流，
    /// 全量重建每拍也恒 EMPTY——非 EMPTY 即结构性漂移，显式拒绝）。
    /// 原位变更后清派生缓存（reservation/evidence——全量重建路径每拍
    /// 整替新对象天然使缓存失效，增量路径显式清空对齐该语义）。
    pub(in crate::session) fn upsert_runtime_envelope(
        &mut self,
        envelope: Envelope,
    ) -> Result<(), StepFatal> {
        envelope.validate()?;
        let key = envelope.key().clone();
        let audit = envelope.audit();
        if self.conservation.get(&key).is_some_and(|c| *c != ConservationState::EMPTY) {
            return Err(ledger_validation::invariant(
                "runtime 增量安装遇到非空 conservation（runtime 路径无回执流——结构性漂移）",
            ));
        }
        self.envelopes.insert(key.clone(), envelope);
        self.audits.insert(key.clone(), audit);
        self.conservation.insert(key, ConservationState::EMPTY);
        self.clear_runtime_projection_caches();
        Ok(())
    }

    /// P2-4b：runtime 路径信封增量移除（离册：成交清余量/撤单/到期/
    /// 日终清簿）。键缺席=差分与账本漂移，显式拒绝（不静默）。
    pub(in crate::session) fn remove_runtime_envelope(
        &mut self,
        key: &EnvelopeKey,
    ) -> Result<(), StepFatal> {
        if self.envelopes.get(key).is_none() {
            return Err(ledger_validation::invariant(&format!(
                "runtime 增量移除遇到缺席键（差分与账本漂移）：{key:?}"
            )));
        }
        self.envelopes.remove(key);
        self.audits.remove(key);
        self.conservation.remove(key);
        self.clear_runtime_projection_caches();
        Ok(())
    }

    /// P2-4b：receipt 序号面对齐（全量重建每拍以 session.next_receipt_base
    /// 重建——增量路径同拍显式同步，保持 hash 投影逐位一致）。
    pub(in crate::session) fn sync_runtime_receipt_index(&mut self, next_receipt_index: u64) {
        self.next_receipt_index = next_receipt_index;
    }

    /// P2-4b：派生缓存清空（原位变更后——预留投影/证据缓存都按对象
    /// 身份失效，增量路径无整替，显式清空对齐全量重建语义）。
    fn clear_runtime_projection_caches(&mut self) {
        if let Ok(mut cache) = self.reservation_cache.lock() {
            *cache = super::live_reservations::ReservationCache::default();
        }
        if let Ok(mut cache) = self.evidence_cache.lock() {
            *cache = None;
        }
    }

    pub fn apply(&mut self, receipts: &mut [EnvelopeReceipt]) -> Result<(), StepFatal> {
        self.apply_with_normalizer(receipts, ledger_candidate::normalize)
    }

    /// 与 [`Self::apply`] 相同的守恒与错误合同；串行脊调用方（ExpiryShadow）传入
    /// 会话 compute 上下文，使 ≥512 行的聚合守恒审计归入会话 registry，
    /// 而不是借用 caller 域或在调用线程退化为串行。
    pub fn apply_in(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        compute: &std::sync::Arc<crate::compute::ComputeContext>,
    ) -> Result<(), StepFatal> {
        let mut shadow = self.clone();
        shadow.apply_private_with_normalizer(
            receipts,
            ledger_candidate::normalize,
            true,
            Some(compute),
        )?;
        *self = shadow;
        Ok(())
    }

    fn apply_with_normalizer(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        normalize: impl FnOnce(&mut u64, &[EnvelopeReceipt]) -> Result<Vec<EnvelopeReceipt>, StepFatal>,
    ) -> Result<(), StepFatal> {
        let mut shadow = self.clone();
        shadow.apply_private_with_normalizer(receipts, normalize, true, None)?;
        *self = shadow;
        Ok(())
    }

    fn apply_private_with_normalizer(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        normalize: impl FnOnce(&mut u64, &[EnvelopeReceipt]) -> Result<Vec<EnvelopeReceipt>, StepFatal>,
        validate_aggregate: bool,
        compute: Option<&std::sync::Arc<crate::compute::ComputeContext>>,
    ) -> Result<(), StepFatal> {
        let candidate = normalize(&mut self.next_receipt_index, receipts)?;
        for receipt in &candidate {
            if !self.seen_local_keys.insert(receipt.local_key.clone()) {
                return Err(ledger_validation::invariant("duplicate receipt local key"));
            }
            self.apply_one(receipt)?;
        }
        if validate_aggregate {
            ledger_conservation::validate_in(self, compute)?;
        }
        receipts.clone_from_slice(&candidate);
        Ok(())
    }

    fn apply_one(&mut self, receipt: &EnvelopeReceipt) -> Result<(), StepFatal> {
        let audit = *self
            .audits
            .get(&receipt.envelope)
            .ok_or_else(|| ledger_validation::invariant("unknown envelope audit"))?;
        ledger_validation::validate(receipt, audit, self.envelopes.get(&receipt.envelope))?;
        let audit_after = ledger_validation::next_audit(receipt, audit)?;
        self.envelopes
            .get_mut(&receipt.envelope)
            .ok_or_else(|| ledger_validation::invariant("unknown envelope"))?
            .apply(
                receipt.delta,
                audit_after,
                matches!(receipt.kind, ReceiptKind::PriceResolved { .. }),
            )?;
        ledger_conservation::record(self, receipt)?;
        self.audits.insert(receipt.envelope.clone(), audit_after);
        Ok(())
    }

    pub fn remove_terminal(&mut self, keys: &[EnvelopeKey]) -> Result<(), StepFatal> {
        let mut shadow = self.clone();
        shadow.remove_terminal_private(keys, true, None)?;
        *self = shadow;
        Ok(())
    }

    /// 与 [`Self::remove_terminal`] 相同的守恒与错误合同；compute 语义同
    /// [`Self::apply_in`]：≥512 行的聚合守恒审计归入给定会话 registry。
    pub fn remove_terminal_in(
        &mut self,
        keys: &[EnvelopeKey],
        compute: &std::sync::Arc<crate::compute::ComputeContext>,
    ) -> Result<(), StepFatal> {
        let mut shadow = self.clone();
        shadow.remove_terminal_private(keys, true, Some(compute))?;
        *self = shadow;
        Ok(())
    }

    fn remove_terminal_private(
        &mut self,
        keys: &[EnvelopeKey],
        validate_aggregate: bool,
        compute: Option<&std::sync::Arc<crate::compute::ComputeContext>>,
    ) -> Result<(), StepFatal> {
        for key in keys {
            let envelope = self
                .envelopes
                .remove(key)
                .ok_or_else(|| ledger_validation::invariant("unknown terminal envelope"))?;
            if envelope.live() != ResVec::ZERO {
                return Err(ledger_validation::invariant(
                    "terminal envelope still has live resources",
                ));
            }
            self.terminal_envelopes.insert(key.clone(), envelope);
        }
        if validate_aggregate {
            ledger_conservation::validate_in(self, compute)?;
        }
        Ok(())
    }

    /// Commit evidence owns this newly built ledger and discards it on error.
    /// Reuse the ordinary transition checks without cloning that private value twice.
    pub(super) fn replay_private_for_commit_evidence(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        terminal_keys: &[EnvelopeKey],
    ) -> Result<(), StepFatal> {
        self.apply_private_with_normalizer(receipts, ledger_candidate::normalize, true, None)?;
        self.remove_terminal_private(terminal_keys, true, None)
    }

    /// StockProcessing 持有可丢弃的逐股账本，立即逐项校验 receipt transition。
    /// 发布本轮结果前再检查一次汇总守恒；失败时丢弃完整股票轮次，最终丢弃 tick。
    pub(super) fn apply_private_for_stock_round(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        terminal_keys: &[EnvelopeKey],
    ) -> Result<(), StepFatal> {
        self.apply_private_with_normalizer(receipts, ledger_candidate::normalize, false, None)?;
        self.remove_terminal_private(terminal_keys, false, None)
    }

    /// ReceiptAggregation owns this ledger for the rest of the private tick. The caller checks
    /// complete evidence after the whole receipt batch, so each intermediate
    /// transition can use the same checked, in-place path as a stock round.
    #[cfg(any(test, feature = "verification-harness"))]
    pub(super) fn apply_private_in_delivery_order_for_receipt_aggregation(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        terminal_keys: &[EnvelopeKey],
    ) -> Result<(), StepFatal> {
        self.apply_private_with_normalizer(
            receipts,
            |next_index, receipts| ledger_candidate::index_in_order(next_index, receipts.to_vec()),
            false,
            None,
        )?;
        self.remove_terminal_private(terminal_keys, false, None)
    }

    /// 原子安装 AccountValidation 创建的 envelopes，后续由 ReceiptAggregation 校验。
    ///
    /// 此操作只创建账本行，不分配 receipt index、消费局部 receipt 身份或应用 transition。
    pub fn insert_created(
        &mut self,
        envelopes: impl IntoIterator<Item = Envelope>,
    ) -> Result<(), StepFatal> {
        let mut shadow = self.clone();
        shadow.insert_created_for_stock_round(envelopes)?;
        validate_ledger_evidence(self, None)?;
        validate_ledger_evidence(&shadow, None)?;
        *self = shadow;
        Ok(())
    }

    /// The stock round and its enclosing tick are discarded together on error.
    /// Validate every new row before writing, then use the owned ledger directly.
    pub(super) fn insert_created_for_stock_round(
        &mut self,
        envelopes: impl IntoIterator<Item = Envelope>,
    ) -> Result<(), StepFatal> {
        let mut batch_keys = BTreeSet::new();
        let mut prepared = Vec::new();
        for envelope in envelopes {
            envelope.validate()?;
            if envelope.origin() != EnvelopeOrigin::CreatedAtValidation {
                return Err(ledger_validation::invariant(
                    "插入的 envelope 并非由 AccountValidation 创建",
                ));
            }
            if envelope.live() == ResVec::ZERO {
                return Err(ledger_validation::invariant(
                    "插入的 AccountValidation envelope 缺少 live resources",
                ));
            }
            let key = envelope.key().clone();
            if !batch_keys.insert(key.clone())
                || self.envelopes.contains_key(&key)
                || self.terminal_envelopes.contains_key(&key)
                || self.audits.contains_key(&key)
                || self.conservation.contains_key(&key)
            {
                return Err(ledger_validation::invariant(
                    "插入的 AccountValidation envelope key 与账本证据冲突",
                ));
            }
            prepared.push((key, envelope));
        }
        for (key, envelope) in prepared {
            self.audits.insert(key.clone(), envelope.audit());
            self.conservation
                .insert(key.clone(), ConservationState::EMPTY);
            self.envelopes.insert(key, envelope);
        }
        Ok(())
    }

    pub fn reset_tick_state(&mut self) {
        self.envelopes.clear();
        self.terminal_envelopes.clear();
        self.audits.clear();
        self.conservation.clear();
        self.seen_local_keys.clear();
    }

    /// Rolls every still-live envelope into the next tick without discarding its
    /// cumulative fill and fee audit.
    ///
    /// Receipt identities, terminal evidence, and conservation counters are
    /// tick-local. The global receipt cursor and every live envelope's cumulative
    /// audit are cross-tick state. Work is performed on a private candidate so an
    /// invalid source ledger leaves `self` unchanged.
    pub fn rebase_live_for_next_tick(&mut self) -> Result<(), StepFatal> {
        let mut candidate = self.clone();
        candidate.rebase_private_for_tick_commit()?;
        *self = candidate;
        Ok(())
    }

    /// CommitTick owner 在失败时丢弃完整 tick candidate；只 detach 实际变化的页。
    pub(super) fn rebase_private_for_tick_commit(&mut self) -> Result<(), StepFatal> {
        validate_ledger_evidence(self, None)?;

        // 校验已证明 basis = live + spent + released；TickStart 且 basis = live
        // 的行没有本 tick 的消费事实，累积费用也已被审计。无变化时保留共享行。
        if self.terminal_envelopes.is_empty()
            && self.envelopes.values().all(|envelope| {
                envelope.origin() == EnvelopeOrigin::TickStart
                    && envelope.pending_price().is_none()
                    && envelope.basis() == envelope.live()
            })
            && self
                .conservation
                .values()
                .all(|state| *state == ConservationState::EMPTY)
        {
            self.seen_local_keys.clear();
            return Ok(());
        }

        let changed: Vec<_> = self
            .envelopes
            .iter()
            .filter_map(|(key, envelope)| {
                (envelope.origin() != EnvelopeOrigin::TickStart
                    || envelope.basis() != envelope.live()
                    || envelope.pending_price().is_some())
                .then(|| key.clone())
            })
            .collect();
        for key in changed {
            let envelope = self
                .envelopes
                .get_mut(&key)
                .expect("rebase 行来自仍拥有的 private ledger");
            envelope.rebase_for_next_tick()?;
        }

        // Source evidence was checked above. Each new basis equals its live
        // balance, so its empty conservation row is valid by construction.
        // Keep the already owned audit rows instead of cloning every live key.
        let terminal: Vec<_> = self.terminal_envelopes.keys().cloned().collect();
        for key in terminal {
            self.audits.remove(&key);
            self.conservation.remove(&key);
        }
        let changed_conservation: Vec<_> = self
            .conservation
            .iter()
            .filter_map(|(key, state)| (*state != ConservationState::EMPTY).then(|| key.clone()))
            .collect();
        for key in changed_conservation {
            *self
                .conservation
                .get_mut(&key)
                .expect("rebase 守恒行来自仍拥有的 private ledger") = ConservationState::EMPTY;
        }
        self.terminal_envelopes.clear();
        self.seen_local_keys.clear();
        // 输入完整证明已覆盖非负资源与 checked 全局总额；rebase 不增加 live，
        // 只把 basis 设为 live、清空守恒增量并删除 terminal，audit 保持原值。
        // 全部变化 Envelope 已逐行校验，可保留该构造证明供下一只读门禁复用。
        let mut cache = self.evidence_cache.lock().map_err(|error| {
            ledger_validation::invariant(&format!("ledger 审计缓存锁失败：{error}"))
        })?;
        *cache = Some(EvidenceCache::capture(self));
        Ok(())
    }

    pub const fn next_receipt_index(&self) -> u64 {
        self.next_receipt_index
    }

    pub fn get(&self, key: &EnvelopeKey) -> Result<&Envelope, StepFatal> {
        self.envelopes
            .get(key)
            .ok_or_else(|| ledger_validation::invariant("unknown envelope"))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&EnvelopeKey, &Envelope)> {
        self.envelopes.iter()
    }

    pub fn terminal_count(&self) -> usize {
        self.terminal_envelopes.len()
    }

    pub fn hydrate_or_validate(
        &mut self,
        projected: impl IntoIterator<Item = Envelope>,
    ) -> Result<(), StepFatal> {
        let mut projected: Vec<_> = projected.into_iter().collect();
        projected.sort_unstable_by(|left, right| left.key().cmp(right.key()));
        if projected
            .windows(2)
            .any(|pair| pair[0].key() == pair[1].key())
        {
            return Err(ledger_validation::invariant("duplicate envelope"));
        }
        if projected
            .iter()
            .any(|envelope| self.terminal_envelopes.contains_key(envelope.key()))
        {
            return Err(ledger_validation::invariant(
                "live projection overlaps terminal envelope evidence",
            ));
        }
        if self.envelopes.is_empty() {
            let candidate = Self::new(self.next_receipt_index, projected)?;
            self.envelopes = candidate.envelopes;
            self.audits = candidate.audits;
            self.conservation = candidate.conservation;
        } else if projected.len() != self.envelopes.len()
            || self.envelopes.iter().zip(projected.iter()).any(
                |((key, authoritative), projected)| {
                    key != projected.key() || authoritative != projected
                },
            )
        {
            return Err(ledger_validation::invariant(
                "live envelope projection disagrees with ledger",
            ));
        }
        Ok(())
    }

    /// 调用方按完整 EnvelopeKey 排序，先核对 nominal 与 charged；此门禁保留完整字段比较。
    /// 任务65 #1：逐 tick 校验改走轻量门；此完整门禁由恢复/水合路径使用。
    pub(in crate::session) fn validate_borrowed_tick_start_projection(
        &self,
        projected: &[super::BorrowedTickStartEnvelope<'_>],
    ) -> Result<(), StepFatal> {
        if projected
            .windows(2)
            .any(|pair| pair[0].identity() >= pair[1].identity())
        {
            return Err(ledger_validation::invariant("duplicate envelope"));
        }
        let mut key = EnvelopeKey {
            account: crate::AccountId(0),
            stock: crate::StockId(u32::MAX),
            order: crate::OrderId(0),
            side: crate::Side::Buy,
        };
        for projection in projected {
            projection.copy_key_into(&mut key);
            if self.terminal_envelopes.contains_key(&key) {
                return Err(ledger_validation::invariant(
                    "live projection overlaps terminal envelope evidence",
                ));
            }
        }
        if projected.len() != self.envelopes.len()
            || self
                .envelopes
                .iter()
                .zip(projected)
                .any(|((key, envelope), projection)| {
                    !projection.matches_key(key) || !projection.matches(envelope)
                })
        {
            return Err(ledger_validation::invariant(
                "live envelope projection disagrees with ledger",
            ));
        }
        Ok(())
    }

    pub fn validate_conservation(&self) -> Result<(), StepFatal> {
        ledger_conservation::validate(self)
    }

    /// Validates every authoritative envelope and its matching evidence rows.
    ///
    /// This read-only gate is intended for adapters that must reject an
    /// incomplete or contradictory ledger before deriving downstream input.
    pub fn validate_complete_evidence(&self) -> Result<(), StepFatal> {
        validate_ledger_evidence(self, None)
    }

    /// 与 [`Self::validate_complete_evidence`] 相同的校验合同；ReceiptAggregation
    /// 链必须传入会话 compute 上下文，让 ≥512 行的并行 row 审计归入会话 registry，
    /// 而不是 caller 当前的池。
    pub fn validate_complete_evidence_in(
        &self,
        compute: &std::sync::Arc<crate::compute::ComputeContext>,
    ) -> Result<(), StepFatal> {
        validate_ledger_evidence(self, Some(compute))
    }
}

fn validate_ledger_evidence(
    ledger: &EnvelopeLedger,
    compute: Option<&std::sync::Arc<crate::compute::ComputeContext>>,
) -> Result<(), StepFatal> {
    // 缓存覆盖下方完整校验的全部输入。Weak 保留 allocation identity，避免 ABA，
    // 且 Arc::make_mut 会在 mutation 时使该 identity 失效。
    let mut cache = ledger.evidence_cache.lock().map_err(|error| {
        ledger_validation::invariant(&format!("ledger 审计缓存锁失败：{error}"))
    })?;
    if cache.as_ref().is_some_and(|cached| {
        ledger.envelopes.matches(&cached.envelopes)
            && ledger.terminal_envelopes.matches(&cached.terminal)
            && ledger.audits.matches(&cached.audits)
            && ledger.conservation.matches(&cached.conservation)
    }) {
        return Ok(());
    }
    validate_ledger_evidence_uncached(ledger, compute)?;
    *cache = Some(EvidenceCache::capture(ledger));
    Ok(())
}

fn validate_ledger_evidence_uncached(
    ledger: &EnvelopeLedger,
    compute: Option<&std::sync::Arc<crate::compute::ComputeContext>>,
) -> Result<(), StepFatal> {
    #[cfg(test)]
    EVIDENCE_VALIDATION_COUNT.with(|count| count.set(count.get() + 1));
    let dense_checked = if ledger.envelopes.len() >= 512
        || ledger.terminal_envelopes.len() >= 512 - ledger.envelopes.len()
    {
        let rows: Vec<_> = ledger
            .envelopes
            .iter()
            .chain(ledger.terminal_envelopes.iter())
            .collect();
        let checked = ledger_conservation::map_rows(&rows, compute, |(key, envelope)| {
            let state = ledger
                .conservation
                .get(key)
                .ok_or_else(|| ledger_validation::invariant("missing conservation row"))?;
            let totals = ledger_conservation::check_row(envelope, state)?;
            // evidence 首错先保存；原序守恒和 checked 全局汇总优先报告。
            Ok((
                totals,
                validate_ledger_evidence_row_with_conservation(ledger, key, envelope, Some(state)),
            ))
        });
        ledger_conservation::validate_checked_rows(
            checked
                .iter()
                .map(|result| result.as_ref().map(|row| row.0).map_err(Clone::clone)),
        )?;
        Some(checked)
    } else {
        ledger_conservation::validate(ledger)?;
        None
    };
    if ledger
        .envelopes
        .values()
        .any(|envelope| envelope.live() == ResVec::ZERO)
    {
        return Err(ledger_validation::invariant(
            "live envelope row has no live resources",
        ));
    }
    if ledger
        .terminal_envelopes
        .values()
        .any(|envelope| envelope.live() != ResVec::ZERO)
    {
        return Err(ledger_validation::invariant(
            "terminal envelope row still has live resources",
        ));
    }
    let expected_rows = ledger
        .envelopes
        .len()
        .checked_add(ledger.terminal_envelopes.len())
        .ok_or_else(|| ledger_validation::invariant("ledger row count overflow"))?;
    if ledger.audits.len() != expected_rows || ledger.conservation.len() != expected_rows {
        return Err(ledger_validation::invariant(
            "ledger evidence rows do not match envelope rows",
        ));
    }
    if let Some(checked) = dense_checked {
        for result in checked {
            result?.1?;
        }
    } else {
        for (key, envelope) in ledger
            .envelopes
            .iter()
            .chain(ledger.terminal_envelopes.iter())
        {
            validate_ledger_evidence_row(ledger, key, envelope)?;
        }
    }
    Ok(())
}

fn validate_ledger_evidence_row(
    ledger: &EnvelopeLedger,
    key: &EnvelopeKey,
    envelope: &Envelope,
) -> Result<(), StepFatal> {
    validate_ledger_evidence_row_with_conservation(
        ledger,
        key,
        envelope,
        ledger.conservation.get(key),
    )
}

fn validate_ledger_evidence_row_with_conservation(
    ledger: &EnvelopeLedger,
    key: &EnvelopeKey,
    envelope: &Envelope,
    conservation: Option<&ConservationState>,
) -> Result<(), StepFatal> {
    envelope.validate()?;
    if envelope.key() != key {
        return Err(ledger_validation::invariant("ledger行键与规范订单身份不一致"));
    }
    if ledger.envelopes.contains_key(key) && ledger.terminal_envelopes.contains_key(key) {
        return Err(ledger_validation::invariant(
            "live and terminal envelope rows overlap",
        ));
    }
    if ledger.audits.get(key).copied() != Some(envelope.audit()) {
        return Err(ledger_validation::invariant(
            "envelope audit row disagrees with envelope",
        ));
    }
    if conservation.is_none() {
        return Err(ledger_validation::invariant(
            "envelope has no conservation row",
        ));
    }
    Ok(())
}
