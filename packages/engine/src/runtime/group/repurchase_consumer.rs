//! 公司来源校验只推进原 ChildTicket；资源仍由原 Fill/Entity 入口落账。
use super::*;
use crate::runtime::{RepurchaseReturnKind, RepurchaseSourceFact, RepurchaseSourceKind};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RepurchasePendingTail {
    pub(super) cursor: u64,
    pub(super) facts: Vec<RepurchasePendingFact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RepurchasePendingFact {
    pub(super) cursor: u64,
    pub(super) tick: u64,
    pub(super) round: u32,
    pub(super) source: RepurchaseSourceFact,
}

#[derive(Default)]
pub(super) struct PreparedRepurchaseBatch {
    tickets: BTreeMap<(crate::AccountId, crate::StockId), Option<ChildTicket>>,
    tails: BTreeMap<crate::plans::PlanId, Vec<RepurchasePendingFact>>,
}

impl GroupState {
    /// 所有公司字段、原事实与数值先在原票 clone 上预检，错误前不触及任何权威状态。
    pub(super) fn prepare_repurchase_batch(
        &self,
        batch: &DeskBatch,
    ) -> Result<PreparedRepurchaseBatch, RuntimeError> {
        let code = crate::StockId(u32::from(batch.stock));
        let mut locals = std::collections::BTreeSet::new();
        for fill in &batch.fills {
            self.verify_company_raw_stock(batch.stock, fill.owner.1, u64::from(fill.order.0))?;
            locals.insert(fill.owner.1);
        }
        for receipt in &batch.receipts {
            self.verify_company_raw_stock(
                batch.stock,
                receipt.owner.1,
                u64::from(receipt.order.0),
            )?;
            locals.insert(receipt.owner.1);
        }
        for entity in &batch.entities {
            if let Entity::Closed {
                local,
                order_serial,
            } = *entity
            {
                self.verify_company_raw_stock(batch.stock, local, order_serial >> 32)?;
            }
            locals.insert(entity_local(entity));
        }
        for source in &batch.repurchase {
            locals.insert(source.owner.1);
        }
        locals.retain(|&local| self.company_child_at(local, batch.stock));
        if locals.is_empty() && batch.repurchase.is_empty() {
            return Ok(PreparedRepurchaseBatch::default());
        }

        let mut prepared = PreparedRepurchaseBatch::default();
        let mut expected_fills: BTreeMap<LocalIdx, Vec<FillFact>> = BTreeMap::new();
        let mut expected_entities: BTreeMap<LocalIdx, Vec<Entity>> = BTreeMap::new();
        let mut expected_receipts: BTreeMap<LocalIdx, Vec<crate::runtime::DeskReceipt>> =
            BTreeMap::new();
        let mut progress = std::collections::BTreeSet::new();
        for source in &batch.repurchase {
            let (account, original) = self.repurchase_source_ticket(batch.stock, source)?;
            let key = (account, code);
            locals.insert(source.owner.1);
            let next = prepared
                .tickets
                .entry(key)
                .or_insert_with(|| Some(original.clone()));
            let ticket = next
                .as_mut()
                .ok_or_else(|| invalid("Closed 后再次出现来源"))?;
            let source_meta = ticket.repurchase.as_ref().expect("入口已核冻结来源");
            if source_meta.request_key != source.request_key {
                return Err(invalid("来源不等于冻结 RequestKey"));
            }
            let order = order_id_of(&source.order_ref);
            if ticket.order_id.is_some_and(|known| known != order) {
                return Err(invalid("来源 OrderRef 与原票身份不同"));
            }

            // 批内多 Fill 共用一次位置比较；本地 cursor 不充当入站防重放证明。
            let is_progress = matches!(
                source.kind,
                RepurchaseSourceKind::Fill(_)
                    | RepurchaseSourceKind::Return {
                        kind: RepurchaseReturnKind::PriceImprovement,
                        ..
                    }
            );
            if is_progress
                && progress.insert(key)
                && source_meta
                    .last_progress_position
                    .is_some_and(|old| (batch.tick, batch.round) <= old)
            {
                return Err(invalid("同或更旧 Batch 位置重复推进公司票"));
            }
            match source.kind {
                RepurchaseSourceKind::Accepted {
                    price,
                    qty,
                    purse_initial,
                } => {
                    if source_meta.phase != RepurchaseChildPhase::Submitted
                        || ticket.order_id.is_some()
                        || (price, qty, purse_initial)
                            != (ticket.price, ticket.qty, source_meta.purse_initial)
                    {
                        return Err(invalid("Accepted phase、身份或发送数值不一致"));
                    }
                    ticket.order_id = Some(order);
                    ticket.repurchase.as_mut().expect("冻结来源在位").phase =
                        RepurchaseChildPhase::Accepted;
                }
                RepurchaseSourceKind::Rejected { reason } => {
                    if source_meta.phase != RepurchaseChildPhase::Submitted
                        || ticket.order_id.is_some()
                        || !place_rejection(reason)
                    {
                        return Err(invalid("Rejected 必须首次绑定真实下单拒收"));
                    }
                    ticket.order_id = Some(order);
                    ticket.repurchase.as_mut().expect("冻结来源在位").phase =
                        RepurchaseChildPhase::Rejected;
                    expected_receipts.entry(source.owner.1).or_default().push(
                        crate::runtime::DeskReceipt {
                            order: source.order_ref,
                            owner: source.owner,
                            kind: reason,
                        },
                    );
                }
                RepurchaseSourceKind::Fill(fill) => {
                    if source_meta.phase != RepurchaseChildPhase::Accepted
                        || ticket.order_id != Some(order)
                        || fill.owner != source.owner
                        || fill.order != source.order_ref
                        || fill.side != Side::Buy
                    {
                        return Err(invalid("Fill 缺少已 Accepted 的同单来源"));
                    }
                    fill.validate()
                        .map_err(|error| invalid(format!("Fill 数值非法：{error}")))?;
                    if fill.price > ticket.price {
                        return Err(invalid("Buy Fill 超出发送限价"));
                    }
                    let charge = fill_charge(&fill)?;
                    ticket.remaining = ticket
                        .remaining
                        .checked_sub(fill.qty)
                        .ok_or_else(|| invalid("成交数量超过原票余量"))?;
                    ticket.charged_cents = ticket
                        .charged_cents
                        .checked_add(charge)
                        .ok_or_else(|| invalid("成交扣款累计溢出"))?;
                    expected_fills.entry(source.owner.1).or_default().push(fill);
                    expected_entities.entry(source.owner.1).or_default().push(
                        Entity::BoughtShares {
                            local: source.owner.1,
                            qty: fill.qty,
                        },
                    );
                }
                RepurchaseSourceKind::Return { amount, kind } => {
                    if amount <= 0 || ticket.order_id != Some(order) {
                        return Err(invalid("Return 金额必须为正且绑定同单"));
                    }
                    let before = held(ticket)?;
                    let meta = ticket.repurchase.as_mut().expect("冻结来源在位");
                    match kind {
                        RepurchaseReturnKind::PriceImprovement
                            if meta.phase == RepurchaseChildPhase::Accepted => {}
                        RepurchaseReturnKind::TerminalRefund
                            if matches!(
                                meta.phase,
                                RepurchaseChildPhase::Accepted | RepurchaseChildPhase::Rejected
                            ) && amount == before =>
                        {
                            meta.phase = RepurchaseChildPhase::TerminalReturned;
                        }
                        _ => return Err(invalid("Return phase、种类或终结全退金额不一致")),
                    }
                    meta.returned_cumulative = meta
                        .returned_cumulative
                        .checked_add(amount)
                        .ok_or_else(|| invalid("退款累计溢出"))?;
                    expected_entities
                        .entry(source.owner.1)
                        .or_default()
                        .push(Entity::CashRefund {
                            local: source.owner.1,
                            amount,
                        });
                }
                RepurchaseSourceKind::Closed => {
                    if source_meta.phase == RepurchaseChildPhase::Submitted
                        || ticket.order_id != Some(order)
                        || held(ticket)? != 0
                    {
                        return Err(invalid("Closed 必须已有同单身份且 H 为零"));
                    }
                    expected_entities
                        .entry(source.owner.1)
                        .or_default()
                        .push(Entity::Closed {
                            local: source.owner.1,
                            order_serial: order.0,
                        });
                    *next = None;
                }
            }
            if let Some(ticket) = next {
                held(ticket)?;
                if is_progress {
                    ticket
                        .repurchase
                        .as_mut()
                        .expect("冻结来源在位")
                        .last_progress_position = Some((batch.tick, batch.round));
                }
            }
            let tail = prepared
                .tails
                .entry(source.request_key.plan_id)
                .or_default();
            let cursor = tail
                .last()
                .map(|fact| fact.cursor)
                .or_else(|| {
                    self.repurchase_pending_tails
                        .get(&source.request_key.plan_id)
                        .map(|old| old.cursor)
                })
                .or_else(|| {
                    self.repurchase_projections
                        .get(&source.request_key.plan_id)
                        .map(|p| p.book_through_cursor)
                })
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| invalid("本计划来源 cursor 溢出"))?;
            tail.push(RepurchasePendingFact {
                cursor,
                tick: batch.tick,
                round: batch.round,
                source: *source,
            });
        }

        // 逐次比较本地原序，不用一次 find 或总金额相等掩盖重复、漏项或假实体。
        for local in locals {
            let original_fills: Vec<_> = batch
                .fills
                .iter()
                .filter(|fill| fill.owner.1 == local)
                .copied()
                .collect();
            let original_entities: Vec<_> = batch
                .entities
                .iter()
                .filter(|entity| entity_local(entity) == local)
                .copied()
                .collect();
            let original_receipts: Vec<_> = batch
                .receipts
                .iter()
                .filter(|receipt| receipt.owner.1 == local && place_rejection(receipt.kind))
                .copied()
                .collect();
            if original_fills.as_slice()
                != expected_fills.get(&local).map_or(&[][..], Vec::as_slice)
                || !same_entity_streams(
                    &original_entities,
                    expected_entities.get(&local).map_or(&[][..], Vec::as_slice),
                )
                || original_receipts.as_slice()
                    != expected_receipts.get(&local).map_or(&[][..], Vec::as_slice)
            {
                return Err(invalid(
                    "公司 source 与原 Fill/Entity/Rejected receipt 的次数或顺序不同",
                ));
            }
            self.validate_repurchase_resources(batch, local)?;
        }
        Ok(prepared)
    }

    fn repurchase_source_ticket(
        &self,
        stock: StockIdx,
        source: &RepurchaseSourceFact,
    ) -> Result<(crate::AccountId, &ChildTicket), RuntimeError> {
        if source.owner.0 != self.group
            || source.order_ref.0 != stock
            || source.order_ref.1 >= (1_u64 << 32)
            || source.request_key.stock != crate::StockId(u32::from(stock))
        {
            return Err(invalid("raw owner、stock 或 OrderRef serial 域非法"));
        }
        let root = self
            .root_projection
            .as_ref()
            .ok_or_else(|| invalid("来源缺少实际名册"))?;
        let index = self
            .roster_offset
            .checked_add(source.owner.1 as usize)
            .ok_or_else(|| invalid("来源名册坐标溢出"))?;
        let account = root
            .account_ids
            .get(index)
            .copied()
            .ok_or_else(|| invalid("来源 local 不在实际名册"))?;
        let assets = self
            .accounts
            .get(source.owner.1 as usize)
            .ok_or_else(|| invalid("来源 local 越界"))?;
        if stock as usize >= assets.shares.len()
            || root.catalog.code(source.request_key.stock).is_none()
        {
            return Err(invalid("来源证券不在实际目录或账户域"));
        }
        let entry = self
            .plan_book
            .entry(source.request_key.plan_id)
            .map_err(|error| invalid(format!("未知 canonical PlanId：{error}")))?;
        let crate::plans::PlanEntry::IssuerRepurchase(plan) = entry else {
            return Err(invalid("来源 PlanId 不是公司 origin"));
        };
        if source.request_key.plan_id.owner != account
            || plan.stock() != source.request_key.stock
            || entry.is_terminal()
            || self
                .plan_book
                .active_entry(account, &source.request_key.stock)
                .map(|entry| entry.plan_id())
                != Some(plan.plan_id())
        {
            return Err(invalid("canonical 计划、账户或证券不一致"));
        }
        let mut bindings = self
            .admin_treasury_bindings
            .iter()
            .filter(|binding| binding.owner == source.owner && binding.stock == stock);
        let binding = bindings
            .next()
            .ok_or_else(|| invalid("来源缺少真实 treasury binding"))?;
        if bindings.next().is_some()
            || binding.account != account
            || binding.issuer != plan.issuer()
            || self
                .current_company_facts()
                .map_err(|error| invalid(format!("公司目录不可用：{error}")))?
                .issuer_of_stock(source.request_key.stock)
                .map_err(|error| invalid(format!("发行人目录不可用：{error}")))?
                != plan.issuer()
        {
            return Err(invalid("来源 treasury binding 不唯一或 issuer 错配"));
        }
        let ticket = self
            .child_in_flight
            .get(&(account, source.request_key.stock))
            .ok_or_else(|| invalid("来源缺少原 ChildTicket"))?;
        let meta = ticket
            .repurchase
            .as_ref()
            .ok_or_else(|| invalid("原票缺少发送前冻结来源"))?;
        if meta.request_key != source.request_key
            || ticket.side != Side::Buy
            || ticket.qty == 0
            || ticket.price <= 0
            || ticket.remaining > ticket.qty
            || (meta.phase == RepurchaseChildPhase::Submitted
                && (ticket.order_id.is_some()
                    || ticket.remaining != ticket.qty
                    || ticket.charged_cents != 0
                    || meta.returned_cumulative != 0))
        {
            return Err(invalid("来源与冻结原票数值或 RequestKey 不一致"));
        }
        held(ticket)?;
        Ok((account, ticket))
    }

    /// 只预检原资源入口将执行的 checked 运算；候选标量不安装，也不形成第二份资源账。
    fn validate_repurchase_resources(
        &self,
        batch: &DeskBatch,
        local: LocalIdx,
    ) -> Result<(), RuntimeError> {
        let account = self
            .accounts
            .get(local as usize)
            .ok_or_else(|| invalid("原事实 local 越界"))?;
        let stock = batch.stock as usize;
        let mut cash = account.cash;
        let mut held = account.buy_purse_held_cents;
        let mut shares = account.shares[stock];
        let mut frozen = account.frozen_today[stock];
        let mut invested = account.invested_cents[stock];
        let mut foreign = account.foreign_active[stock];
        if cash < 0 || held < 0 {
            return Err(invalid("账户原现金或在途买占用为负"));
        }
        for fill in batch.fills.iter().filter(|fill| fill.owner.1 == local) {
            let gross = fill
                .price
                .checked_mul(i64::from(fill.qty))
                .ok_or_else(|| invalid("原 Fill gross 溢出"))?;
            invested = invested
                .checked_add(gross)
                .ok_or_else(|| invalid("原 Fill 成本基溢出"))?;
            held = held
                .checked_sub(fill_charge(fill)?)
                .filter(|held| *held >= 0)
                .ok_or_else(|| invalid("原 Fill 使账户在途买占用下穿"))?;
        }
        for entity in batch
            .entities
            .iter()
            .filter(|entity| entity_local(entity) == local)
        {
            match *entity {
                Entity::CashRefund { amount, .. } => {
                    cash = cash
                        .checked_add(amount)
                        .ok_or_else(|| invalid("原退款现金溢出"))?;
                    held = held
                        .checked_sub(amount)
                        .filter(|held| *held >= 0)
                        .ok_or_else(|| invalid("原退款使账户在途买占用下穿"))?;
                }
                Entity::BoughtShares { qty, .. } => {
                    i32::try_from(qty).map_err(|_| invalid("原股份实体超出 Undo 增量域"))?;
                    shares = shares
                        .checked_add(qty)
                        .ok_or_else(|| invalid("原股份实体持仓溢出"))?;
                    frozen = frozen
                        .checked_add(qty)
                        .ok_or_else(|| invalid("原股份实体 T+1 冻结溢出"))?;
                }
                Entity::Closed { .. } => {
                    foreign = foreign
                        .checked_sub(1)
                        .ok_or_else(|| invalid("原 Closed 缺少在途登记"))?;
                }
                _ => return Err(invalid("公司 Buy 收到卖腿资源实体")),
            }
        }
        cash.checked_add(held)
            .ok_or_else(|| invalid("原资源使公开现金总额溢出"))?;
        Ok(())
    }

    /// 原 Ref 可独立暴露错股；binding 只证明能力，不改变其它股票的 raw origin。
    fn verify_company_raw_stock(
        &self,
        batch_stock: StockIdx,
        local: LocalIdx,
        raw_stock: u64,
    ) -> Result<(), RuntimeError> {
        if raw_stock != u64::from(batch_stock)
            && (self.company_child_at(local, batch_stock)
                || StockIdx::try_from(raw_stock)
                    .is_ok_and(|stock| self.company_child_at(local, stock)))
        {
            return Err(invalid("原事实的 raw 证券与 Batch.stock 不同"));
        }
        Ok(())
    }

    /// 通用 Investor 生命周期入口只能跳过公司票；真实推进由校验后的来源独占。
    pub(super) fn company_child_at(&self, local: LocalIdx, stock: StockIdx) -> bool {
        let Some(account) = self.root_projection.as_ref().and_then(|root| {
            self.roster_offset
                .checked_add(local as usize)
                .and_then(|index| root.account_ids.get(index))
        }) else {
            return false;
        };
        let code = crate::StockId(u32::from(stock));
        self.child_in_flight
            .get(&(*account, code))
            .is_some_and(|ticket| ticket.repurchase.is_some())
            || matches!(
                self.plan_book.active_entry(*account, &code),
                Some(crate::plans::PlanEntry::IssuerRepurchase(_))
            )
    }

    /// 只有完整预检成功才安装原票与来源尾；此处不落账户资源、不推进 Investor API。
    pub(super) fn install_repurchase_batch(&mut self, prepared: PreparedRepurchaseBatch) {
        for ((account, code), ticket) in prepared.tickets {
            let before = self.child_in_flight.remove(&(account, code));
            if let Some(ticket) = ticket {
                self.child_in_flight.insert((account, code), ticket);
            }
            self.push_undo(GroupUndo::ChildInFlight {
                account,
                code,
                prev: before,
            });
        }
        for (plan_id, facts) in prepared.tails {
            let previous = self
                .repurchase_pending_tails
                .get(&plan_id)
                .map(|tail| (tail.facts.len(), tail.cursor));
            let tail = self.repurchase_pending_tails.entry(plan_id).or_default();
            tail.cursor = facts.last().expect("只安装非空追加").cursor;
            tail.facts.extend(facts);
            self.push_undo(GroupUndo::RepurchaseTail { plan_id, previous });
        }
    }

    pub(super) fn hash_repurchase_pending_tails(&self, mut hash: u64) -> u64 {
        for (plan, tail) in &self.repurchase_pending_tails {
            hash = values(
                hash,
                &[
                    0x5250_5441_494c,
                    plan.owner.0,
                    plan.sequence,
                    tail.cursor,
                    tail.facts.len() as u64,
                ],
            );
            for fact in &tail.facts {
                let source = fact.source;
                hash = values(
                    hash,
                    &[
                        fact.cursor,
                        fact.tick,
                        u64::from(fact.round),
                        source.request_key.plan_id.owner.0,
                        source.request_key.plan_id.sequence,
                        u64::from(source.request_key.stock.0),
                        source.request_key.requested_trading_day,
                        u64::from(source.owner.0),
                        u64::from(source.owner.1),
                        u64::from(source.order_ref.0),
                        source.order_ref.1,
                    ],
                );
                hash = match source.kind {
                    RepurchaseSourceKind::Accepted {
                        price,
                        qty,
                        purse_initial,
                    } => values(
                        hash,
                        &[1, price as u64, u64::from(qty), purse_initial as u64],
                    ),
                    RepurchaseSourceKind::Rejected { reason } => values(hash, &[2, reason as u64]),
                    RepurchaseSourceKind::Fill(fill) => values(
                        hash,
                        &[
                            3,
                            u64::from(fill.order.0),
                            fill.order.1,
                            u64::from(fill.owner.0),
                            u64::from(fill.owner.1),
                            matches!(fill.side, Side::Buy) as u64,
                            fill.price as u64,
                            u64::from(fill.qty),
                            fill.commission as u64,
                            fill.stamp_tax as u64,
                            fill.transfer_fee as u64,
                        ],
                    ),
                    RepurchaseSourceKind::Return { amount, kind } => values(
                        hash,
                        &[
                            4,
                            amount as u64,
                            matches!(kind, RepurchaseReturnKind::TerminalRefund) as u64,
                        ],
                    ),
                    RepurchaseSourceKind::Closed => fnv(hash, 5),
                };
            }
        }
        hash
    }
}

fn values(mut hash: u64, fields: &[u64]) -> u64 {
    for &field in fields {
        hash = fnv(hash, field);
    }
    hash
}

/// 原 Desk 的 maker Closed 可先于 BoughtShares；两条实体子流各守原序与次数。
fn same_entity_streams(original: &[Entity], expected: &[Entity]) -> bool {
    let bought = |entity: &&Entity| matches!(entity, Entity::BoughtShares { .. });
    original
        .iter()
        .filter(bought)
        .eq(expected.iter().filter(bought))
        && original
            .iter()
            .filter(|entity| !bought(entity))
            .eq(expected.iter().filter(|entity| !bought(entity)))
}

fn invalid(detail: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: format!("回购来源校验：{detail}"),
    }
}

fn held(ticket: &ChildTicket) -> Result<i64, RuntimeError> {
    let meta = ticket
        .repurchase
        .as_ref()
        .ok_or_else(|| invalid("原票缺少冻结来源"))?;
    if meta.purse_initial <= 0 || ticket.charged_cents < 0 || meta.returned_cumulative < 0 {
        return Err(invalid("原票 initial、charged 或 returned 数值域非法"));
    }
    meta.purse_initial
        .checked_sub(ticket.charged_cents)
        .and_then(|held| held.checked_sub(meta.returned_cumulative))
        .filter(|held| *held >= 0)
        .ok_or_else(|| invalid("原票 H 下穿或溢出"))
}

fn fill_charge(fill: &FillFact) -> Result<i64, RuntimeError> {
    fill.price
        .checked_mul(i64::from(fill.qty))
        .and_then(|gross| gross.checked_add(fill.commission))
        .and_then(|charge| charge.checked_add(fill.transfer_fee))
        .ok_or_else(|| invalid("原 Fill 扣款溢出"))
}

fn entity_local(entity: &Entity) -> LocalIdx {
    match *entity {
        Entity::CashProceeds { local, .. }
        | Entity::CashRefund { local, .. }
        | Entity::BoughtShares { local, .. }
        | Entity::RefundedShares { local, .. }
        | Entity::Closed { local, .. } => local,
    }
}

fn place_rejection(reason: crate::runtime::ReceiptKind) -> bool {
    matches!(
        reason,
        crate::runtime::ReceiptKind::PriceOutOfBand
            | crate::runtime::ReceiptKind::PriceCageExceeded
            | crate::runtime::ReceiptKind::NotRoundLot
            | crate::runtime::ReceiptKind::ExceedsOrderCap
            | crate::runtime::ReceiptKind::PhaseRejected
            | crate::runtime::ReceiptKind::AuctionLimitOrderRequired
    )
}
