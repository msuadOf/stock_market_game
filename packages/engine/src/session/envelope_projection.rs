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
        self.project_live_envelopes_with_market_candidate(None)
    }

    fn project_live_envelopes_with_market_candidate(
        &self,
        candidate: Option<(&StockCode, &Market)>,
    ) -> Result<Vec<pipeline::Envelope>, StepFatal> {
        let mut envelopes = Vec::new();
        for (code, market) in &self.markets {
            let market = candidate
                .filter(|(candidate_code, _)| *candidate_code == code)
                .map_or(market, |(_, candidate_market)| candidate_market);
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

    /// Rebuilds the quiet-point ledger after the compatibility body commits on
    /// its private shadow. Seller live cash remains zero, while cumulative
    /// charged audit advances only by the ADR-0017 #9 gross-capped amount. This
    /// preserves an already-restored v2 seller debt and keeps every successful
    /// compatibility tick at a valid schema-v2 save quiet point.
    pub(super) fn rebase_legacy_envelope_ledger_for_quiet_point(
        &mut self,
    ) -> Result<(), StepFatal> {
        self.rebase_legacy_envelope_ledger(self.project_live_envelopes()?)
    }

    /// Atomically advances the compatibility ledger to one successfully
    /// settled route's candidate book.  The authoritative market is not
    /// replaced until this projection succeeds, so a rejected route can roll
    /// back accounts and the ledger together without exposing a partial book.
    pub(super) fn rebase_legacy_envelope_ledger_for_market_candidate(
        &mut self,
        code: &StockCode,
        market: &Market,
    ) -> Result<(), StepFatal> {
        let projected = self.project_live_envelopes_with_market_candidate(Some((code, market)))?;
        self.rebase_legacy_envelope_ledger(projected)
    }

    fn rebase_legacy_envelope_ledger(
        &mut self,
        projected_envelopes: Vec<pipeline::Envelope>,
    ) -> Result<(), StepFatal> {
        let previous = self.envelope_ledger.clone();
        let mut rebased = Vec::new();
        for projected in projected_envelopes {
            let projected_audit = projected.audit();
            let nominal = legacy_nominal_fees(
                &self.setup.config,
                projected.key().side,
                projected_audit.filled_value,
            )?;
            let before = previous
                .iter()
                .find(|(key, _)| *key == projected.key())
                .map(|(_, envelope)| envelope.audit())
                .unwrap_or(pipeline::EnvelopeAudit {
                    limit: projected_audit.limit,
                    remaining_qty: projected_audit
                        .remaining_qty
                        .checked_add(projected_audit.filled_qty)
                        .ok_or_else(|| invariant("legacy order quantity audit overflows"))?,
                    filled_qty: 0,
                    filled_value: Money::ZERO,
                    nominal: pipeline::FeeComponents::ZERO,
                    charged: pipeline::FeeComponents::ZERO,
                });
            let charged = advance_legacy_charged_audit(
                &self.setup.config,
                projected.key().side,
                before,
                nominal,
                projected_audit,
            )?;
            rebased.push(pipeline::Envelope::tick_start_existing(
                projected.key().clone(),
                projected.live().cash,
                projected.live().shares,
                pipeline::EnvelopeAudit {
                    nominal,
                    charged,
                    ..projected_audit
                },
            ));
        }
        self.envelope_ledger = pipeline::EnvelopeLedger::new(self.next_receipt_base, rebased)?;
        Ok(())
    }

    fn project_continuous_envelope(
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
            id: OrderId(order.arrival_seq),
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
        let nominal = legacy_nominal_fees(&self.setup.config, order.side, order.filled_value)?;
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

pub(super) fn legacy_nominal_fees(
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

fn advance_legacy_charged_audit(
    config: &GameConfig,
    side: Side,
    before: pipeline::EnvelopeAudit,
    nominal_after: pipeline::FeeComponents,
    order_after: pipeline::EnvelopeAudit,
) -> Result<pipeline::FeeComponents, StepFatal> {
    if before.filled_qty > order_after.filled_qty
        || before.filled_value > order_after.filled_value
        || before.nominal.commission > nominal_after.commission
        || before.nominal.stamp_tax > nominal_after.stamp_tax
        || before.nominal.transfer_fee > nominal_after.transfer_fee
    {
        return Err(invariant(
            "legacy live-order cumulative fill or nominal fee audit moved backwards",
        ));
    }
    if side == Side::Buy {
        return Ok(nominal_after);
    }
    let gross_delta = order_after
        .filled_value
        .sub(before.filled_value)
        .map_err(|error| invariant(&error.to_string()))?;
    if gross_delta == Money::ZERO {
        return Ok(before.charged);
    }
    let fill_qty = order_after
        .filled_qty
        .checked_sub(before.filled_qty)
        .ok_or_else(|| invariant("legacy filled quantity audit moved backwards"))?;
    let transition =
        pipeline::transition::FillTransition::sell(pipeline::transition::SellFillInput {
            config,
            fill_qty,
            remaining_qty_after: order_after.remaining_qty,
            filled_value_before: before.filled_value,
            gross_delta,
            nominal_before: before.nominal,
            charged_before: before.charged,
        })?;
    if transition.nominal_after != nominal_after {
        return Err(invariant(
            "legacy seller transition disagrees with projected nominal audit",
        ));
    }
    Ok(transition.charged_after)
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "GameSession::project_live_envelopes".to_owned(),
    }
}
