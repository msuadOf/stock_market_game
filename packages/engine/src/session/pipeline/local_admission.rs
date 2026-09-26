//! Admit runnable requests to stock gates without using source or identity as priority.

use super::{P2Candidate, StepFatal};
use crate::{AccountId, Intent, Side, StockCode};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ResourceLane {
    Cash(AccountId),
    Shares(AccountId, StockCode),
}

pub(super) fn admit_ready_batch(
    candidates: Vec<P2Candidate>,
) -> Result<Vec<P2Candidate>, StepFatal> {
    let has_dependencies = candidates
        .iter()
        .any(|candidate| !candidate.predecessors().is_empty());
    if candidates.len() < 2 && !has_dependencies {
        return Ok(candidates);
    }
    let mut resource_groups = HashMap::<ResourceLane, Vec<usize>>::new();
    let mut stock_counts = BTreeMap::<StockCode, usize>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let owner = candidate.owner();
        let stock = code(candidate.intent());
        match candidate.intent() {
            Intent::PlaceLimit {
                side: Side::Buy, ..
            }
            | Intent::PlaceMarket {
                side: Side::Buy, ..
            } => resource_groups
                .entry(ResourceLane::Cash(owner))
                .or_default()
                .push(index),
            Intent::PlaceLimit {
                side: Side::Sell, ..
            }
            | Intent::PlaceMarket {
                side: Side::Sell, ..
            } => resource_groups
                .entry(ResourceLane::Shares(owner, stock.clone()))
                .or_default()
                .push(index),
            // A cancellation targets an order, not this tick's cash or sellable
            // shares. Its result is delivered by the stock book; a plan that
            // needs that result schedules its next command only afterwards.
            Intent::Cancel { .. } => {}
        }
        *stock_counts.entry(stock.clone()).or_default() += 1;
    }
    if !has_dependencies && stock_counts.values().all(|count| *count < 2) {
        return Ok(candidates);
    }

    // Resource conflicts retain receipt order. A quote replacement also waits
    // for its explicit old-order cancellations; an unrelated cancel adds no
    // account-wide ordering. Identities only locate these declared predecessors.
    let mut successors = vec![Vec::new(); candidates.len()];
    let mut incoming = vec![0_usize; candidates.len()];
    for entries in resource_groups.values() {
        for pair in entries.windows(2) {
            successors[pair[0]].push(pair[1]);
            incoming[pair[1]] = incoming[pair[1]]
                .checked_add(1)
                .ok_or_else(|| invariant("local admission edge count overflow"))?;
        }
    }
    if has_dependencies {
        add_quote_dependencies(&candidates, &mut successors, &mut incoming)?;
    }
    let stock_gates = stock_counts
        .into_iter()
        .filter_map(|(stock, count)| (count > 1).then(|| (stock, Mutex::new(Vec::<usize>::new()))))
        .collect::<BTreeMap<_, _>>();
    let roots = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    {
        let admission = StockAdmission {
            candidates: &candidates,
            successors: &successors,
            stock_gates: &stock_gates,
            remaining: incoming.into_iter().map(AtomicUsize::new).collect(),
            registered: AtomicUsize::new(0),
            poisoned: AtomicBool::new(false),
            invalid_dependency_count: AtomicBool::new(false),
        };
        // Partition independent roots using Rayon. A chain continues within its
        // current task; only a ready fork spawns additional scoped work.
        rayon::scope(|scope| {
            roots
                .par_iter()
                .for_each(|root| admission.register(scope, *root));
        });
        if admission.poisoned.load(Ordering::Relaxed) {
            return Err(invariant("stock admission gate lock was poisoned"));
        }
        if admission.invalid_dependency_count.load(Ordering::Relaxed) {
            return Err(invariant("stock admission dependency count underflow"));
        }
        if admission.registered.load(Ordering::Relaxed) != candidates.len() {
            return Err(invariant(
                "stock admission dependency cycle or omitted command",
            ));
        }
    }

    // The gates supply the actual local order. Merge their edges with account
    // resource edges, then make an output layout. The layout of unrelated stocks
    // does not determine a trading priority.
    for gate in stock_gates.values() {
        let registered = gate
            .lock()
            .map_err(|_| invariant("stock admission gate lock was poisoned"))?;
        for pair in registered.windows(2) {
            successors[pair[0]].push(pair[1]);
        }
    }
    let mut remaining = vec![0_usize; candidates.len()];
    for edges in &successors {
        for &dependent in edges {
            remaining[dependent] = remaining[dependent]
                .checked_add(1)
                .ok_or_else(|| invariant("local admission edge count overflow"))?;
        }
    }
    let mut order = Vec::with_capacity(candidates.len());
    let mut ready = remaining
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    while let Some(index) = ready.pop() {
        order.push(index);
        for &dependent in &successors[index] {
            remaining[dependent] = remaining[dependent]
                .checked_sub(1)
                .ok_or_else(|| invariant("local admission edge count underflow"))?;
            if remaining[dependent] == 0 {
                ready.push(dependent);
            }
        }
    }
    if order.len() != candidates.len() {
        return Err(invariant(
            "local admission dependency cycle or omitted command",
        ));
    }
    let mut slots = candidates.into_iter().map(Some).collect::<Vec<_>>();
    Ok(order
        .into_iter()
        .map(|index| slots[index].take().expect("each ready command enters once"))
        .collect())
}

fn add_quote_dependencies(
    candidates: &[P2Candidate],
    successors: &mut [Vec<usize>],
    incoming: &mut [usize],
) -> Result<(), StepFatal> {
    let mut positions = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if positions.insert(candidate.key(), index).is_some() {
            return Err(invariant(
                "quote dependency has an ambiguous candidate identity",
            ));
        }
    }
    for (index, candidate) in candidates.iter().enumerate() {
        let mut unique = BTreeSet::new();
        for key in candidate.predecessors() {
            if !unique.insert(key) {
                return Err(invariant("quote dependency repeats a predecessor"));
            }
            let predecessor = *positions
                .get(key)
                .ok_or_else(|| invariant("quote dependency predecessor is missing"))?;
            if predecessor >= index {
                return Err(invariant(
                    "quote dependency predecessor does not precede its replacement",
                ));
            }
            let before = &candidates[predecessor];
            if before.owner() != candidate.owner()
                || code(before.intent()) != code(candidate.intent())
                || !matches!(before.intent(), Intent::Cancel { .. })
                || !matches!(
                    candidate.intent(),
                    Intent::PlaceLimit { .. } | Intent::PlaceMarket { .. }
                )
            {
                return Err(invariant(
                    "quote dependency must link an account's same-stock cancellation to its replacement",
                ));
            }
            successors[predecessor].push(index);
            incoming[index] = incoming[index]
                .checked_add(1)
                .ok_or_else(|| invariant("quote dependency edge count overflow"))?;
        }
    }
    Ok(())
}

struct StockAdmission<'a> {
    candidates: &'a [P2Candidate],
    successors: &'a [Vec<usize>],
    stock_gates: &'a BTreeMap<StockCode, Mutex<Vec<usize>>>,
    remaining: Vec<AtomicUsize>,
    registered: AtomicUsize,
    poisoned: AtomicBool,
    invalid_dependency_count: AtomicBool,
}

impl StockAdmission<'_> {
    fn register<'scope>(&'scope self, scope: &rayon::Scope<'scope>, mut index: usize) {
        loop {
            if let Some(gate) = self.stock_gates.get(code(self.candidates[index].intent())) {
                match gate.lock() {
                    Ok(mut registered) => registered.push(index),
                    Err(_) => {
                        self.poisoned.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            }
            self.registered.fetch_add(1, Ordering::Relaxed);
            let mut next = None;
            for &dependent in &self.successors[index] {
                match self.remaining[dependent].fetch_sub(1, Ordering::AcqRel) {
                    0 => {
                        self.invalid_dependency_count.store(true, Ordering::Relaxed);
                        return;
                    }
                    1 => {
                        if next.is_none() {
                            next = Some(dependent);
                        } else {
                            scope.spawn(move |scope| self.register(scope, dependent));
                        }
                    }
                    _ => {}
                }
            }
            match next {
                Some(dependent) => index = dependent,
                None => return,
            }
        }
    }
}

fn code(intent: &Intent) -> &StockCode {
    match intent {
        Intent::PlaceLimit { code, .. }
        | Intent::PlaceMarket { code, .. }
        | Intent::Cancel { code, .. } => code,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::local_admission".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::pipeline::P2CandidateKey;
    use crate::{Money, OrderId};

    fn quote_request(index: u64, account: u64, stock: &str, cancel: bool) -> P2Candidate {
        let code = StockCode(stock.to_owned());
        let intent = if cancel {
            Intent::Cancel {
                code,
                id: OrderId(index + 1),
            }
        } else {
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(100),
                qty: 100,
            }
        };
        P2Candidate::new(P2CandidateKey::player(index), AccountId(account), intent)
    }

    #[test]
    fn quote_replacements_wait_for_every_cancel_across_a_resource_fork() {
        let first = P2CandidateKey::player(0);
        let second = P2CandidateKey::player(1);
        let candidates = vec![
            quote_request(0, 1, "600001", true),
            quote_request(1, 1, "600001", true),
            quote_request(2, 1, "600001", false)
                .with_predecessors(vec![first.clone(), second.clone()]),
            // Completing the last cancellation releases both directions. The
            // buy also has a cash-lane successor on a different stock.
            P2Candidate::new(
                P2CandidateKey::player(3),
                AccountId(1),
                Intent::PlaceLimit {
                    code: StockCode("600001".to_owned()),
                    side: Side::Sell,
                    price: Money::from_cents(110),
                    qty: 100,
                },
            )
            .with_predecessors(vec![first, second]),
            quote_request(4, 1, "600002", false),
            quote_request(5, 2, "600001", false),
        ];
        for threads in [1, 4] {
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| {
                    let admitted = admit_ready_batch(candidates.clone()).unwrap();
                    assert_eq!(admitted.len(), candidates.len());
                    let position = |index| {
                        admitted
                            .iter()
                            .position(|candidate| candidate.key() == &P2CandidateKey::player(index))
                            .unwrap()
                    };
                    assert!(position(0) < position(2));
                    assert!(position(1) < position(2));
                    assert!(position(0) < position(3));
                    assert!(position(1) < position(3));
                    assert!(position(2) < position(4));
                    assert_eq!(
                        admitted
                            .iter()
                            .map(P2Candidate::key)
                            .collect::<std::collections::BTreeSet<_>>()
                            .len(),
                        candidates.len()
                    );
                });
        }
    }

    #[test]
    fn quote_dependency_rejects_missing_reversed_or_unrelated_predecessors() {
        let cancel = quote_request(0, 1, "600001", true);
        let place =
            quote_request(1, 1, "600001", false).with_predecessors(vec![cancel.key().clone()]);
        let cases = [
            vec![place.clone()],
            vec![place.clone(), cancel.clone()],
            vec![quote_request(0, 2, "600001", true), place.clone()],
            vec![quote_request(0, 1, "600002", true), place.clone()],
            vec![quote_request(0, 1, "600001", false), place.clone()],
            vec![
                cancel.clone(),
                quote_request(1, 1, "600001", true).with_predecessors(vec![cancel.key().clone()]),
            ],
            vec![
                cancel.clone(),
                quote_request(1, 1, "600001", false)
                    .with_predecessors(vec![cancel.key().clone(), cancel.key().clone()]),
            ],
            vec![cancel.clone(), cancel, place],
        ];
        for (case, candidates) in cases.into_iter().enumerate() {
            assert!(
                admit_ready_batch(candidates).is_err(),
                "invalid quote dependency case {case}"
            );
        }
    }

    #[test]
    fn crossed_account_stock_requests_keep_each_account_receipt_order() {
        let x = StockCode("600001".to_owned());
        let y = StockCode("600002".to_owned());
        let requests = [
            (AccountId(1), x.clone()),
            (AccountId(1), y.clone()),
            (AccountId(2), y),
            (AccountId(2), x),
        ];
        let candidates: Vec<_> = requests
            .into_iter()
            .enumerate()
            .map(|(index, (account, code))| {
                P2Candidate::new(
                    P2CandidateKey::player(index as u64),
                    account,
                    Intent::PlaceLimit {
                        code,
                        side: Side::Buy,
                        price: Money::from_cents(100),
                        qty: 100,
                    },
                )
            })
            .collect();
        for _ in 0..16 {
            let admitted = admit_ready_batch(candidates.clone()).expect("valid local admission");
            assert_eq!(admitted.len(), 4);
            for (account, keys) in [(AccountId(1), [0, 1]), (AccountId(2), [2, 3])] {
                let received = admitted
                    .iter()
                    .filter(|candidate| candidate.owner() == account)
                    .map(|candidate| candidate.key().clone())
                    .collect::<Vec<_>>();
                assert_eq!(received, keys.map(P2CandidateKey::player));
            }
        }
    }

    #[test]
    fn same_account_sells_keep_share_receipt_order_and_cancel_is_admitted() {
        let stock = StockCode("600001".to_owned());
        let candidates = vec![
            P2Candidate::new(
                P2CandidateKey::npc(AccountId(1), 0),
                AccountId(1),
                Intent::Cancel {
                    code: stock.clone(),
                    id: OrderId(7),
                },
            ),
            P2Candidate::new(
                P2CandidateKey::npc(AccountId(1), 1),
                AccountId(1),
                Intent::PlaceLimit {
                    code: stock.clone(),
                    side: Side::Sell,
                    price: Money::from_cents(100),
                    qty: 100,
                },
            ),
            P2Candidate::new(
                P2CandidateKey::npc(AccountId(1), 2),
                AccountId(1),
                Intent::PlaceLimit {
                    code: stock.clone(),
                    side: Side::Sell,
                    price: Money::from_cents(100),
                    qty: 100,
                },
            ),
            P2Candidate::new(
                P2CandidateKey::player(0),
                AccountId(2),
                Intent::PlaceLimit {
                    code: stock,
                    side: Side::Buy,
                    price: Money::from_cents(100),
                    qty: 100,
                },
            ),
        ];
        for _ in 0..16 {
            let admitted = admit_ready_batch(candidates.clone()).expect("valid local admission");
            assert_eq!(admitted.len(), candidates.len());
            let account_sells = admitted
                .iter()
                .filter(|item| {
                    item.owner() == AccountId(1)
                        && matches!(
                            item.intent(),
                            Intent::PlaceLimit {
                                side: Side::Sell,
                                ..
                            }
                        )
                })
                .map(|item| item.key().clone())
                .collect::<Vec<_>>();
            assert_eq!(
                account_sells,
                vec![
                    P2CandidateKey::npc(AccountId(1), 1),
                    P2CandidateKey::npc(AccountId(1), 2)
                ]
            );
            assert!(admitted
                .iter()
                .any(|item| item.key() == &P2CandidateKey::npc(AccountId(1), 0)));
        }
    }
}
