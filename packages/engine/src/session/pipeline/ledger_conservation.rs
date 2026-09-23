use super::{
    conservation::{ConservationBasis, ConservationRow},
    ledger::ConservationState,
    Envelope, EnvelopeLedger, EnvelopeOrigin, EnvelopeReceipt, JournalRank, ResVec, StepFatal,
};

pub(super) fn record(
    ledger: &mut EnvelopeLedger,
    receipt: &EnvelopeReceipt,
) -> Result<(), StepFatal> {
    let state = ledger
        .conservation
        .get_mut(&receipt.envelope)
        .ok_or_else(|| super::ledger_validation::invariant("missing conservation state"))?;
    if receipt.local_key.journal() == JournalRank::PreSeal {
        state.p0_released = state.p0_released.checked_add(receipt.delta.released)?;
    } else {
        state.sealed_spent = state.sealed_spent.checked_add(receipt.delta.spent)?;
        state.sealed_released = state.sealed_released.checked_add(receipt.delta.released)?;
    }
    Ok(())
}

pub(super) fn validate(ledger: &EnvelopeLedger) -> Result<(), StepFatal> {
    let mut totals = [ResVec::ZERO; 5];
    for (key, envelope) in ledger
        .envelopes
        .iter()
        .chain(ledger.terminal_envelopes.iter())
    {
        let state = ledger
            .conservation
            .get(key)
            .ok_or_else(|| super::ledger_validation::invariant("missing conservation row"))?;
        let row = row(envelope, state)?;
        row.validate_with_preseal(state.p0_released)?;
        match row.basis {
            ConservationBasis::TickStart(start, released, _) => {
                totals[0] = totals[0].checked_add(start)?;
                totals[1] = totals[1].checked_add(released)?;
            }
            ConservationBasis::P3Created(created) => totals[0] = totals[0].checked_add(created)?,
        }
        totals[2] = totals[2].checked_add(row.sealed_spent)?;
        totals[3] = totals[3].checked_add(row.sealed_released)?;
        totals[4] = totals[4].checked_add(row.commit_live)?;
    }
    if totals[0]
        != totals[1]
            .checked_add(totals[2])?
            .checked_add(totals[3])?
            .checked_add(totals[4])?
    {
        return Err(super::ledger_validation::invariant(
            "aggregate conservation row mismatch",
        ));
    }
    Ok(())
}

fn row(envelope: &Envelope, state: &ConservationState) -> Result<ConservationRow, StepFatal> {
    let basis = match envelope.origin() {
        EnvelopeOrigin::TickStart => ConservationBasis::TickStart(
            envelope.basis(),
            state.p0_released,
            envelope.basis().checked_sub(state.p0_released)?,
        ),
        EnvelopeOrigin::P3Created => ConservationBasis::P3Created(envelope.basis()),
    };
    Ok(ConservationRow {
        key: envelope.key().clone(),
        basis,
        sealed_spent: state.sealed_spent,
        sealed_released: state.sealed_released,
        commit_live: envelope.live(),
    })
}
