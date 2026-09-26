use super::*;

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
        for (code, market) in &self.markets {
            for order in market.resting_orders() {
                envelopes.push(self.project_continuous_envelope(code, &order)?);
            }
        }
        for (code, orders) in &self.auction_orders {
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
        let mut projected = self.project_live_envelopes()?;
        let has_authoritative_ledger = self.envelope_ledger.iter().next().is_some();
        if has_authoritative_ledger {
            // Resting orders independently project identity, resources, remaining quantity,
            // cumulative fills and nominal fees. Only ADR-0017 #9's actually charged seller fee
            // history is ledger-owned. Validate that history before preserving it for the strict
            // full-envelope comparison; missing/extra identities still fail closed below.
            for envelope in &mut projected {
                let Ok(authoritative) = self.envelope_ledger.get(envelope.key()) else {
                    continue;
                };
                let projected_audit = envelope.audit();
                let authoritative_audit = authoritative.audit();
                if authoritative_audit.nominal != projected_audit.nominal {
                    return Err(invariant(
                        "live-order nominal fee audit disagrees with cumulative filled value",
                    ));
                }
                validate_hydrated_charged_audit(envelope.key().side, authoritative_audit)?;
                *envelope = pipeline::Envelope::tick_start_existing(
                    envelope.key().clone(),
                    envelope.live().cash,
                    envelope.live().shares,
                    pipeline::EnvelopeAudit {
                        charged: authoritative_audit.charged,
                        ..projected_audit
                    },
                );
            }
        } else if projected.iter().any(|envelope| {
            envelope.key().side == Side::Sell && envelope.audit().filled_value > Money::ZERO
        }) {
            return Err(invariant(
                "cannot hydrate a partially filled seller without charged fee history",
            ));
        }
        self.envelope_ledger.hydrate_or_validate(projected)
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
        // ADR-0017 #9 settles a seller's fees from gross proceeds, so the
        // escrow ledger never reserves seller cash.
        let cash = match order.side {
            Side::Buy => buy_order_reservation(
                &self.setup.config,
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
        let nominal = cumulative_nominal_fees(&self.setup.config, order.side, order.filled_value)?;
        // Buyer actual fees always equal nominal fees. Seller charges are path-dependent because
        // every fill leg is gross-capped; only the authoritative ledger can supply that history.
        let charged = if order.side == Side::Buy {
            nominal
        } else {
            pipeline::FeeComponents::ZERO
        };
        Ok(pipeline::Envelope::tick_start_existing(
            pipeline::EnvelopeKey {
                account: order.owner,
                stock: order.code.clone(),
                order: order.id,
                side: order.side,
            },
            cash,
            shares,
            pipeline::EnvelopeAudit {
                limit: order.limit,
                remaining_qty: order.qty,
                filled_qty: order.filled_qty,
                filled_value: order.filled_value,
                nominal,
                charged,
            },
        ))
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
