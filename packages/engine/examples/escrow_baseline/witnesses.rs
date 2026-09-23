use crate::{
    frames,
    scenarios::{enqueue, order, seeded},
    CorpusError,
};
use engine::{AccountId, Intent, OrderId, Side, StockCode};
use serde_json::{json, Value};

pub fn collect(seed: u64) -> Result<Vec<Value>, CorpusError> {
    let mut records = Vec::new();
    let mut malformed = seeded(1_000, 10_000_000, seed)?;
    enqueue(&mut malformed, &[order(Side::Buy, 1_000, 0)])?;
    records.push(
        json!({"kind":"divergence_witness","ids":[5],"name":"auction-zero-quantity-prevalidation",
        "frame":frames::step(&mut malformed)?,"new_expected_variant":"IntentRejected"}),
    );
    let mut cage = seeded(1_000, 10_000_000, seed)?;
    for _ in 0..4 {
        cage.step()?;
    }
    let before_id = cage.save()?.next_order_id;
    enqueue(
        &mut cage,
        &[order(Side::Buy, 1_100, 100), order(Side::Buy, 1_000, 100)],
    )?;
    let rejected = frames::step(&mut cage)?;
    if cage.save()?.next_order_id != before_id + 1 {
        return Err(CorpusError::Invariant(
            "old cage refusal consumed ID".to_owned(),
        ));
    }
    records.push(json!({"kind":"divergence_witness","ids":[3,8],"name":"cage-reject-then-valid-control",
        "before_next_order_id":before_id,"frame":rejected,"new_expected_next_order_id":before_id+2}));

    let mut cancel = seeded(1_000, 100_501, seed)?;
    for _ in 0..4 {
        cancel.step()?;
    }
    enqueue(&mut cancel, &[order(Side::Buy, 1_000, 100)])?;
    let accepted = frames::step(&mut cancel)?;
    let id = cancel.save()?.next_order_id - 1;
    enqueue(
        &mut cancel,
        &[
            Intent::Cancel {
                code: StockCode("600001".to_owned()),
                id: OrderId(id),
            },
            order(Side::Buy, 1_000, 100),
        ],
    )?;
    records.push(
        json!({"kind":"divergence_witness","ids":[2],"name":"sealed-cancel-reuses-cash-old-side",
        "control":accepted,"frame":frames::step(&mut cancel)?}),
    );

    let mut same_tick = seeded(1_000, 10_000_000, seed)?;
    for _ in 0..4 {
        same_tick.step()?;
    }
    let id = same_tick.save()?.next_order_id;
    enqueue(
        &mut same_tick,
        &[
            order(Side::Buy, 1_000, 100),
            Intent::Cancel {
                code: StockCode("600001".to_owned()),
                id: OrderId(id),
            },
        ],
    )?;
    records.push(json!({"kind":"divergence_witness","ids":[4],"name":"same-tick-place-cancel","frame":frames::step(&mut same_tick)?}));

    let mut shares = seeded(1_000, 10_000_000, seed)?;
    for _ in 0..4 {
        shares.step()?;
    }
    let mut save = shares.save()?;
    let position = save
        .snapshot
        .accounts
        .get_mut(&AccountId(0))
        .and_then(|account| account.positions.get_mut(&StockCode("600001".to_owned())))
        .ok_or_else(|| CorpusError::Invariant("owned position missing".to_owned()))?;
    position.t1_locked = 2_400;
    shares = engine::GameSession::restore(&save)?;
    enqueue(&mut shares, &[order(Side::Sell, 1_000, 100)])?;
    records.push(json!({"kind":"share_control","name":"owned-but-t1-locked","before":save.snapshot,"frame":frames::step(&mut shares)?}));
    let mut oversell = seeded(1_000, 10_000_000, seed)?;
    enqueue(&mut oversell, &[order(Side::Sell, 1_000, 2_500)])?;
    records.push(
        json!({"kind":"share_control","name":"oversell","frame":frames::step(&mut oversell)?}),
    );
    Ok(records)
}
