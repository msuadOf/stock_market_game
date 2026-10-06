use super::*;

#[cfg(test)]
thread_local! {
    pub(super) static OWNED_PROJECTION_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct LiveOrderProjection<'a> {
    code: &'a StockCode,
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
        for (code, orders) in &self.state.auction_orders {
            for order in orders {
                envelopes.push(self.project_auction_envelope(code, order)?);
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
        self.state.envelope_ledger.hydrate_or_validate(projected)
    }

    pub(in crate::session) fn validate_existing_live_envelope_projection(
        &self,
    ) -> Result<(), StepFatal> {
        self.validate_existing_live_projection_with_observer(|| {})
    }

    pub(in crate::session) fn validate_existing_live_projection_with_observer(
        &self,
        observe_projected_row: impl Fn() + Sync,
    ) -> Result<(), StepFatal> {
        let mut projected = Vec::new();
        for (code, market) in &self.state.markets {
            // 保留旧投影的到达序，多个计算错误仍报告相同的首错。
            let mut orders = market.resting_order_refs().collect::<Vec<_>>();
            orders.sort_by_key(|order| order.seq);
            for order in orders {
                projected.push(self.project_borrowed_envelope(LiveOrderProjection {
                    code,
                    owner: order.owner,
                    id: order.id,
                    side: order.side,
                    limit: order.price,
                    qty: order.qty,
                    filled_qty: order.filled_qty,
                    filled_value: order.filled_value,
                })?);
                observe_projected_row();
            }
        }
        for (code, orders) in &self.state.auction_orders {
            for order in orders {
                projected.push(self.project_borrowed_envelope(LiveOrderProjection {
                    code,
                    owner: order.owner,
                    id: OrderId(order.order_id),
                    side: order.side,
                    limit: order.limit,
                    qty: order.qty,
                    filled_qty: 0,
                    filled_value: Money::ZERO,
                })?);
            }
        }
        projected.sort_by(|left, right| left.identity().cmp(&right.identity()));
        if projected
            .windows(2)
            .any(|pair| pair[0].identity() == pair[1].identity())
        {
            return Err(invariant("live orders project duplicate envelope identity"));
        }
        let mut key = pipeline::EnvelopeKey {
            account: AccountId(0),
            stock: StockCode(String::new()),
            order: OrderId(0),
            side: Side::Buy,
        };
        for projection in &mut projected {
            projection.copy_key_into(&mut key);
            let Ok(authoritative) = self.state.envelope_ledger.get(&key) else {
                // 缺失身份继续进入完整比较门禁，保留既有错误选择顺序。
                continue;
            };
            let audit = authoritative.audit();
            if audit.nominal != projection.audit.nominal {
                return Err(invariant(
                    "live-order nominal fee audit disagrees with cumulative filled value",
                ));
            }
            validate_hydrated_charged_audit(projection.side, audit)?;
            projection.audit.charged = audit.charged;
        }
        self.state
            .envelope_ledger
            .validate_borrowed_tick_start_projection(&projected)
    }

    pub(in crate::session) fn project_continuous_envelope(
        &self,
        code: &StockCode,
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

    pub(in crate::session) fn project_continuous_envelope_facts<'a>(
        &self,
        code: &'a StockCode,
        order: &Order,
    ) -> Result<pipeline::BorrowedTickStartEnvelope<'a>, StepFatal> {
        self.project_borrowed_envelope(LiveOrderProjection {
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

    fn project_auction_envelope(
        &self,
        code: &StockCode,
        order: &AuctionOrderSnap,
    ) -> Result<pipeline::Envelope, StepFatal> {
        self.project_envelope(LiveOrderProjection {
            code,
            owner: order.owner,
            id: OrderId(order.order_id),
            side: order.side,
            limit: order.limit,
            qty: order.qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
        })
    }

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
        // ADR-0017 #9：卖方费用从成交所得实收，cash 预留恒为零。
        let cash = match order.side {
            Side::Buy => buy_order_reservation(
                &self.state.setup.config,
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
        let nominal =
            cumulative_nominal_fees(&self.state.setup.config, order.side, order.filled_value)?;
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
