//! 公司来源沿同一真实订单出口运输；不计算费用、不改变实体或账户资源。
use super::*;
use crate::runtime::{RepurchaseRequestKey, RepurchaseSourceFact, RepurchaseSourceKind};

impl DeskState {
    pub(super) fn hash_repurchase_sources(&self, mut hash: u64) -> u64 {
        if !self.repurchase_requests.is_empty() {
            hash = fnv(hash, 0x5250_4c49_5645);
            hash = fnv(hash, self.repurchase_requests.len() as u64);
            for (&order, &request) in &self.repurchase_requests {
                hash = hash_identity(hash, order, request);
            }
        }
        // 即时全成或拒收后 live map 已空，待发事实仍须保留完整来源和产生顺序。
        for (group, facts) in self.repurchase_out.iter().enumerate().filter(|(_, facts)| !facts.is_empty()) {
            hash = fnv(hash, 0x5250_4f55_54);
            hash = fnv(hash, group as u64);
            hash = fnv(hash, facts.len() as u64);
            for fact in facts {
                hash = hash_identity(hash, fact.order_ref, fact.request_key);
                hash = fnv(hash, u64::from(fact.owner.0));
                hash = fnv(hash, u64::from(fact.owner.1));
                hash = match fact.kind {
                    RepurchaseSourceKind::Accepted { price, qty, purse_initial } =>
                        hash_values(hash, &[1, price as u64, u64::from(qty), purse_initial as u64]),
                    RepurchaseSourceKind::Rejected { reason } => hash_values(hash, &[2, reason as u64]),
                    RepurchaseSourceKind::Fill(fill) => hash_values(hash, &[3, u64::from(fill.order.0), fill.order.1,
                        u64::from(fill.owner.0), u64::from(fill.owner.1),
                        match fill.side { Side::Buy => 1, Side::Sell => 2 }, fill.price as u64,
                        u64::from(fill.qty), fill.commission as u64, fill.stamp_tax as u64, fill.transfer_fee as u64]),
                    RepurchaseSourceKind::Return { amount, kind } => hash_values(hash, &[4, amount as u64,
                        match kind { crate::runtime::RepurchaseReturnKind::PriceImprovement => 1,
                            crate::runtime::RepurchaseReturnKind::TerminalRefund => 2 }]),
                    RepurchaseSourceKind::Closed => fnv(hash, 5),
                };
            }
        }
        hash
    }
    pub(super) fn validate_repurchase_request(
        &self, owner: Owner, request: RepurchaseRequestKey, purse: i64,
    ) -> Result<(), RuntimeError> {
        let stock = crate::StockId(u32::from(self.setup.stock));
        if request.stock != stock || owner.0 as usize >= self.out.len() || purse <= 0 {
            return Err(over("公司请求证券、owner 组或初始 purse 无效"));
        }
        let context = self.calendar_tick.as_ref().ok_or_else(|| over("公司请求缺少真实市场 Calendar"))?;
        let row = context.before_projection().stock(stock)
            .map_err(|error| over(&format!("公司请求 Calendar 证券缺失：{error}")))?;
        if row.date_session_ordinal() != Some(request.requested_trading_day) {
            return Err(over("公司请求交易日序与真实证券 Calendar 不同"));
        }
        Ok(())
    }

    pub(super) fn open_repurchase_source(
        &mut self, order: &DeskOrder, request: RepurchaseRequestKey,
    ) -> Result<(), RuntimeError> {
        if self.repurchase_requests.contains_key(&order.order_ref) {
            return Err(over("公司来源重复绑定真实 OrderRef"));
        }
        let before = self.repurchase_requests.insert(order.order_ref, request);
        self.undo.push(DeskUndo::RepurchaseRequest { order: order.order_ref, before });
        self.note_repurchase_source(order.order_ref, order.owner, RepurchaseSourceKind::Accepted {
            price: order.price, qty: order.qty, purse_initial: order.purse,
        });
        Ok(())
    }

    pub(super) fn note_repurchase_source(&mut self, order: OrderRef, owner: Owner, kind: RepurchaseSourceKind) {
        if let Some(&request) = self.repurchase_requests.get(&order) {
            self.repurchase_out[owner.0 as usize].push(RepurchaseSourceFact {
                request_key: request, owner, order_ref: order, kind,
            });
        }
    }

    pub(super) fn close_repurchase_source(&mut self, order: OrderRef, owner: Owner) {
        self.note_repurchase_source(order, owner, RepurchaseSourceKind::Closed);
        if let Some(request) = self.repurchase_requests.remove(&order) {
            self.undo.push(DeskUndo::RepurchaseRequest { order, before: Some(request) });
        }
    }
}

fn hash_identity(mut hash: u64, order: OrderRef, request: RepurchaseRequestKey) -> u64 {
    for value in [u64::from(order.0), order.1, request.plan_id.owner.0,
        request.plan_id.sequence, u64::from(request.stock.0), request.requested_trading_day] {
        hash = fnv(hash, value);
    }
    hash
}

fn hash_values(mut hash: u64, values: &[u64]) -> u64 {
    for &value in values { hash = fnv(hash, value); }
    hash
}
