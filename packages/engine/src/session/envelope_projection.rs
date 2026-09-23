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
        self.envelope_ledger
            .hydrate_or_validate(self.project_live_envelopes()?)
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
        let cash = live_cash_reservation(
            &self.setup.config,
            order.side,
            order.limit,
            order.qty,
            order.filled_value,
        )
        .map_err(|error| invariant(&error.to_string()))?;
        let shares = match order.side {
            Side::Buy => 0,
            Side::Sell => order.qty,
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
                nominal: pipeline::FeeComponents::ZERO,
                charged: pipeline::FeeComponents::ZERO,
            },
        ))
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "GameSession::project_live_envelopes".to_owned(),
    }
}
