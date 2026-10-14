use super::*;

#[cfg(test)]
thread_local! {
    pub(super) static OWNED_PROJECTION_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// 任务65 #1 前置条件守卫：单存续期间费用输入（费率）不可变。
/// `validated_config` 于 new()/恢复时固定；任何静默热改在此 fail closed，
/// 而不是依赖逐 resting 单的费用重算去事后发现。
pub(in crate::session) fn verify_fee_config_unchanged(
    view: &crate::session::pipeline::npc_split::SessionReadView<'_>,
) -> Result<(), StepFatal> {
    if view.setup.config != *view.validated_config {
        return Err(StepFatal::InvariantViolation {
            description:
                "费率在订单存续期间被改动：setup.config 与已验证快照不一致（前置条件破坏，fail closed）"
                    .to_owned(),
            location: "envelope_projection::verify_fee_config_unchanged".to_owned(),
        });
    }
    Ok(())
}

struct LiveOrderProjection<'a> {
    code: &'a StockId,
    owner: AccountId,
    id: OrderId,
    side: Side,
    limit: Money,
    qty: u32,
    filled_qty: u32,
    filled_value: Money,
}

impl GameSession {
    pub fn project_live_envelopes(&self) -> Result<Vec<pipeline::Envelope>, StepFatal> {
        let mut envelopes = Vec::new();
        for (code, market) in &self.state.markets {
            for order in market.resting_orders() {
                envelopes.push(self.project_continuous_envelope(code, &order)?);
            }
        }
        envelopes.sort_by(|left, right| left.key().cmp(right.key()));
        if envelopes
            .windows(2)
            .any(|pair| pair[0].key() == pair[1].key())
        {
            return Err(invariant("live orders project duplicate envelope identity"));
        }
        Ok(envelopes)
    }

    pub fn hydrate_or_validate_envelope_ledger(&mut self) -> Result<(), StepFatal> {
        if self.state.envelope_ledger.iter().next().is_some() {
            return self.validate_existing_live_envelope_projection();
        }
        let projected = self.project_live_envelopes()?;
        if projected.iter().any(|envelope| {
            envelope.key().side == Side::Sell && envelope.audit().filled_value > Money::ZERO
        }) {
            return Err(invariant(
                "cannot hydrate a partially filled seller without charged fee history",
            ));
        }
        self.state.envelope_ledger.hydrate_or_validate(projected)?;
        self.state.envelope_ledger.validate_complete_evidence()?;
        // 新建/restore的raw事实不能携带proof；在这里逐订单建立，不把计算结果当已验证状态。
        for (_, envelope) in self.state.envelope_ledger.iter() {
            envelope.verified_live(&self.state.setup.config)?;
        }
        Ok(())
    }

    /// 恢复/水合路径的完整校验（任务65 #1：保留全量费用重算——恢复是账本与簿
    /// 一致性的重建边界，nominal/live 漂移必须在此显式拒绝）。
    pub(in crate::session) fn validate_existing_live_envelope_projection(
        &self,
    ) -> Result<(), StepFatal> {
        self.validate_existing_live_projection_with_observer(|| {})
    }

    pub(in crate::session) fn validate_existing_live_projection_with_observer(
        &self,
        observe_projected_row: impl Fn() + Sync,
    ) -> Result<(), StepFatal> {
        // restore/new生出的raw行没有proof，统一入口会完整重算一次；已有规范行复用
        // 每订单内容版本的证明，不能把“恢复保留全校验”理解成每tick重建同一财务表示。
        Self::validate_existing_live_projection_with_observer_on(&self.read_view(), observe_projected_row)
    }

    /// 读视图版本（prepare 链投机路径共用）。
    ///
    /// 任务65 #1 轻量门：不再逐 resting 单重算预留/名义费用（该值由创建、逐腿 receipt
    /// 与恢复 hydrate 权威确立；费率输入不可变由 `verify_fee_config_unchanged` 指纹守卫
    /// 兜底）。保留的防御：book↔ledger 四字段交叉核对、terminal 重叠与顺序查重、
    /// charged 界检查（validate_hydrated_charged_audit）。
    pub(in crate::session) fn validate_existing_live_projection_with_observer_on(
        view: &crate::session::pipeline::npc_split::SessionReadView<'_>,
        observe_projected_row: impl Fn() + Sync,
    ) -> Result<(), StepFatal> {
        verify_fee_config_unchanged(view)?;
        let mut identities: Vec<(AccountId, StockId, OrderId, u8)> = Vec::new();
        for (code, market) in view.markets {
            // 保留旧投影的到达序，多个计算错误仍报告相同的首错。
            let mut orders = market.resting_order_refs().collect::<Vec<_>>();
            orders.sort_by_key(|order| order.seq);
            for order in orders {
                identities.push((
                    order.owner,
                    *code,
                    order.id,
                    u8::from(order.side == Side::Sell),
                ));
                let key = pipeline::EnvelopeKey {
                    account: order.owner,
                    stock: *code,
                    order: order.id,
                    side: order.side,
                };
                let Ok(envelope) = view.envelope_ledger.get(&key) else {
                    return Err(invariant(
                        "continuous order books contain an unledgered order",
                    ));
                };
                if envelope.key() != &key {
                    return Err(invariant("ledger行键与规范订单身份不一致"));
                }
                let audit = envelope.audit();
                if envelope.pending_price().is_some()
                    || audit.limit != order.price
                    || audit.remaining_qty != order.qty
                    || audit.filled_qty != order.filled_qty
                    || audit.filled_value != order.filled_value
                {
                    return Err(invariant(
                        "continuous order books disagree with envelope audit",
                    ));
                }
                if envelope.origin() != pipeline::EnvelopeOrigin::TickStart
                    || envelope.basis() != envelope.live()
                    || envelope.spent() != pipeline::ResVec::ZERO
                    || envelope.released() != pipeline::ResVec::ZERO {
                    return Err(invariant("live envelope projection disagrees with ledger"));
                }
                envelope.verified_live(&view.setup.config)?;
                observe_projected_row();
            }
        }
        identities.sort_unstable();
        identities.dedup();
        let live_count = view.envelope_ledger.iter().count();
        if identities.len() != live_count {
            // 簿内重复身份或 ledger 残留无簿行：两侧身份集合不再一一对应。
            return Err(invariant("live orders project duplicate envelope identity"));
        }
        view.envelope_ledger.validate_complete_evidence()
    }

    pub(in crate::session) fn project_continuous_envelope(
        &self,
        code: &StockId,
        order: &Order,
    ) -> Result<pipeline::Envelope, StepFatal> {
        self.project_envelope(LiveOrderProjection {
            code,
            owner: order.owner,
            id: order.id,
            side: order.side,
            limit: order.price,
            qty: order.qty,
            filled_qty: order.filled_qty,
            filled_value: order.filled_value,
        })
    }

    /// owned 投影（hydrate/恢复路径专用）。
    /// 任务65 #1：capture 不再走此链重算费用；逐 tick 轻量门见
    /// validate_existing_live_projection_with_observer_on。
    fn project_envelope(
        &self,
        order: LiveOrderProjection<'_>,
    ) -> Result<pipeline::Envelope, StepFatal> {
        let projection = self.project_borrowed_envelope(order)?;
        #[cfg(test)]
        OWNED_PROJECTION_COUNT.with(|count| count.set(count.get() + 1));
        Ok(pipeline::Envelope::tick_start_existing(
            pipeline::EnvelopeKey {
                account: projection.account,
                stock: projection.stock.clone(),
                order: projection.order,
                side: projection.side,
            },
            projection.live.cash,
            projection.live.shares,
            projection.audit,
        ))
    }

    fn project_borrowed_envelope<'a>(
        &self,
        order: LiveOrderProjection<'a>,
    ) -> Result<pipeline::BorrowedTickStartEnvelope<'a>, StepFatal> {
        Self::project_borrowed_envelope_on(&self.read_view(), order)
    }

    /// 读视图版本（hydrate/恢复路径的费用叶子，仅读 setup.config）。
    fn project_borrowed_envelope_on<'a>(
        view: &crate::session::pipeline::npc_split::SessionReadView<'_>,
        order: LiveOrderProjection<'a>,
    ) -> Result<pipeline::BorrowedTickStartEnvelope<'a>, StepFatal> {
        // ADR-0017 #9：卖方费用从成交所得实收，cash 预留恒为零。
        let cash = match order.side {
            Side::Buy => buy_order_reservation(
                &view.setup.config,
                order.limit,
                order.qty,
                order.filled_value,
            )
            .map_err(|error| invariant(&error.to_string()))?,
            Side::Sell => Money::ZERO,
        };
        let shares = match order.side {
            Side::Buy => 0,
            Side::Sell => order.qty,
        };
        let nominal = cumulative_nominal_fees(&view.setup.config, order.side, order.filled_value)?;
        // Buy 实收等于 nominal；Sell 的逐腿封顶历史由权威 ledger 提供。
        let charged = if order.side == Side::Buy {
            nominal
        } else {
            pipeline::FeeComponents::ZERO
        };
        Ok(pipeline::BorrowedTickStartEnvelope {
            account: order.owner,
            stock: order.code,
            order: order.id,
            side: order.side,
            live: pipeline::ResVec::new(cash, shares),
            audit: pipeline::EnvelopeAudit {
                limit: order.limit,
                remaining_qty: order.qty,
                filled_qty: order.filled_qty,
                filled_value: order.filled_value,
                nominal,
                charged,
            },
        })
    }
}

pub(super) fn validate_hydrated_charged_audit(
    side: Side,
    audit: pipeline::EnvelopeAudit,
) -> Result<(), StepFatal> {
    for fee in [audit.nominal, audit.charged] {
        if fee.commission < Money::ZERO
            || fee.stamp_tax < Money::ZERO
            || fee.transfer_fee < Money::ZERO
        {
            return Err(invariant(
                "live-order fee audit contains a negative component",
            ));
        }
    }
    if audit.charged.commission > audit.nominal.commission
        || audit.charged.stamp_tax > audit.nominal.stamp_tax
        || audit.charged.transfer_fee > audit.nominal.transfer_fee
    {
        return Err(invariant(
            "live-order charged fee audit exceeds nominal fee audit",
        ));
    }
    if side == Side::Buy {
        if audit.charged != audit.nominal {
            return Err(invariant(
                "buyer charged fee audit disagrees with nominal fee audit",
            ));
        }
        return Ok(());
    }

    let charged_total = audit
        .charged
        .total()
        .map_err(|error| invariant(&error.to_string()))?;
    if charged_total > audit.filled_value {
        return Err(invariant(
            "seller charged fee audit exceeds cumulative gross value",
        ));
    }
    Ok(())
}

pub(super) fn cumulative_nominal_fees(
    config: &GameConfig,
    side: Side,
    filled_value: Money,
) -> Result<pipeline::FeeComponents, StepFatal> {
    #[cfg(test)]
    config.fee_probe.record_nominal();
    if filled_value == Money::ZERO {
        return Ok(pipeline::FeeComponents::ZERO);
    }
    Ok(pipeline::FeeComponents {
        commission: config
            .commission(filled_value)
            .map_err(|error| invariant(&error.to_string()))?,
        stamp_tax: match side {
            Side::Buy => Money::ZERO,
            Side::Sell => config
                .stamp_tax(filled_value)
                .map_err(|error| invariant(&error.to_string()))?,
        },
        transfer_fee: config
            .transfer_fee(filled_value)
            .map_err(|error| invariant(&error.to_string()))?,
    })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "GameSession::project_live_envelopes".to_owned(),
    }
}
