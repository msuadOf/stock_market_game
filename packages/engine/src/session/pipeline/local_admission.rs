//! 就绪请求进入股票 gate；来源与身份仅定位事实，不作为交易优先级。

use super::{IntentCandidate, StepFatal};
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

/// receipt 只在同账户的资源 lane 内可比；阶段表示就绪时间。
/// 股票 gate 独立登记实际并发受理顺序。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum AccountReceipt {
    PreviousCommit(u64),
    BetweenTicks(u64),
    ReadyThisTick(u64),
}

/// 只有计划命令在就绪边界新增账户局部 ordinal；队列来源的 key 已携带受理序。
#[derive(Default)]
pub(super) struct AccountReceipts {
    next_plan: HashMap<AccountId, u64>,
}

impl AccountReceipts {
    pub(super) fn observe(
        &mut self,
        candidate: &IntentCandidate,
    ) -> Result<AccountReceipt, StepFatal> {
        match candidate.key() {
            super::IntentCandidateKey::Npc {
                account,
                npc_local_index,
            } => {
                if *account != candidate.owner() {
                    return Err(invariant("NPC receipt belongs to another account"));
                }
                Ok(AccountReceipt::PreviousCommit(*npc_local_index))
            }
            super::IntentCandidateKey::Player { player_queue_index } => {
                Ok(AccountReceipt::BetweenTicks(*player_queue_index))
            }
            super::IntentCandidateKey::PlanChain { .. } => {
                let next = self.next_plan.entry(candidate.owner()).or_default();
                let receipt = AccountReceipt::ReadyThisTick(*next);
                *next = next
                    .checked_add(1)
                    .ok_or_else(|| invariant("plan account receipt overflow"))?;
                Ok(receipt)
            }
        }
    }
}

pub(super) fn admit_ready_batch(
    candidates: Vec<IntentCandidate>,
    receipts: &mut AccountReceipts,
) -> Result<Vec<IntentCandidate>, StepFatal> {
    let has_dependencies = candidates
        .iter()
        .any(|candidate| !candidate.predecessors().is_empty());
    if candidates.len() < 2 && !has_dependencies {
        for candidate in &candidates {
            receipts.observe(candidate)?;
        }
        return Ok(candidates);
    }
    let (mut plan, has_conflicts) =
        ReadyAdmissionPlan::prepare(candidates, receipts, has_dependencies)?;
    if !has_conflicts {
        return Ok(plan.candidates);
    }
    plan.run_stock_gates()?;
    plan.finish_layout()
}

/// 仅拥有一个 ready batch 的候选索引、偏序及股票受理 gate。
/// 账户资源余额仍由 P1 snapshot 与 validator 管理。
struct ReadyAdmissionPlan {
    candidates: Vec<IntentCandidate>,
    successors: Vec<Vec<usize>>,
    incoming: Vec<usize>,
    stock_gates: BTreeMap<StockCode, Mutex<Vec<usize>>>,
}

impl ReadyAdmissionPlan {
    fn prepare(
        candidates: Vec<IntentCandidate>,
        receipts: &mut AccountReceipts,
        has_dependencies: bool,
    ) -> Result<(Self, bool), StepFatal> {
        let mut resource_groups = HashMap::<ResourceLane, Vec<(usize, AccountReceipt)>>::new();
        let mut stock_counts = BTreeMap::<StockCode, usize>::new();
        for (index, candidate) in candidates.iter().enumerate() {
            let receipt = receipts.observe(candidate)?;
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
                    .push((index, receipt)),
                Intent::PlaceLimit {
                    side: Side::Sell, ..
                }
                | Intent::PlaceMarket {
                    side: Side::Sell, ..
                } => resource_groups
                    .entry(ResourceLane::Shares(owner, stock.clone()))
                    .or_default()
                    .push((index, receipt)),
                // 撤单只面向目标订单；后续计划由股票簿产生的 typed 结果唤醒。
                Intent::Cancel { .. } => {}
            }
            *stock_counts.entry(stock.clone()).or_default() += 1;
        }
        let has_conflicts = has_dependencies
            || stock_counts.values().any(|count| *count >= 2)
            || resource_groups.values().any(|entries| entries.len() >= 2);
        let mut plan = Self {
            successors: if has_conflicts {
                vec![Vec::new(); candidates.len()]
            } else {
                Vec::new()
            },
            incoming: if has_conflicts {
                vec![0; candidates.len()]
            } else {
                Vec::new()
            },
            candidates,
            stock_gates: BTreeMap::new(),
        };
        if !has_conflicts {
            return Ok((plan, false));
        }
        plan.build_resource_edges(resource_groups)?;
        if has_dependencies {
            plan.add_quote_dependencies()?;
        }
        plan.stock_gates = stock_counts
            .into_iter()
            .filter_map(|(stock, count)| (count > 1).then(|| (stock, Mutex::new(Vec::new()))))
            .collect();
        Ok((plan, true))
    }

    fn build_resource_edges(
        &mut self,
        mut resource_groups: HashMap<ResourceLane, Vec<(usize, AccountReceipt)>>,
    ) -> Result<(), StepFatal> {
        for entries in resource_groups.values_mut() {
            entries.sort_unstable_by_key(|(_, receipt)| *receipt);
            for pair in entries.windows(2) {
                if pair[0].1 == pair[1].1 {
                    return Err(invariant("conflicting requests share an account receipt"));
                }
                self.successors[pair[0].0].push(pair[1].0);
                self.incoming[pair[1].0] = self.incoming[pair[1].0]
                    .checked_add(1)
                    .ok_or_else(|| invariant("local admission edge count overflow"))?;
            }
        }
        Ok(())
    }

    fn add_quote_dependencies(&mut self) -> Result<(), StepFatal> {
        let mut positions = BTreeMap::new();
        for (index, candidate) in self.candidates.iter().enumerate() {
            if positions.insert(candidate.key(), index).is_some() {
                return Err(invariant(
                    "quote dependency has an ambiguous candidate identity",
                ));
            }
        }
        for (index, candidate) in self.candidates.iter().enumerate() {
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
                let before = &self.candidates[predecessor];
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
                self.successors[predecessor].push(index);
                self.incoming[index] = self.incoming[index]
                    .checked_add(1)
                    .ok_or_else(|| invariant("quote dependency edge count overflow"))?;
            }
        }
        Ok(())
    }

    fn run_stock_gates(&mut self) -> Result<(), StepFatal> {
        let roots = self
            .incoming
            .iter()
            .enumerate()
            .filter_map(|(index, count)| (*count == 0).then_some(index))
            .collect::<Vec<_>>();
        {
            let admission = StockAdmission {
                candidates: &self.candidates,
                successors: &self.successors,
                stock_gates: &self.stock_gates,
                remaining: std::mem::take(&mut self.incoming)
                    .into_iter()
                    .map(AtomicUsize::new)
                    .collect(),
                registered: AtomicUsize::new(0),
                poisoned: AtomicBool::new(false),
                invalid_dependency_count: AtomicBool::new(false),
            };
            // 独立 root 经 Rayon 分派；同链延续当前任务，只有就绪分叉另启任务。
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
            if admission.registered.load(Ordering::Relaxed) != self.candidates.len() {
                return Err(invariant(
                    "stock admission dependency cycle or omitted command",
                ));
            }
        }
        Ok(())
    }

    fn finish_layout(mut self) -> Result<Vec<IntentCandidate>, StepFatal> {
        // gate arrival 边与资源、quote 边合并；无关股票的输出布局不定义交易优先级。
        for gate in self.stock_gates.values() {
            let registered = gate
                .lock()
                .map_err(|_| invariant("stock admission gate lock was poisoned"))?;
            for pair in registered.windows(2) {
                self.successors[pair[0]].push(pair[1]);
            }
        }
        let mut remaining = vec![0_usize; self.candidates.len()];
        for edges in &self.successors {
            for &dependent in edges {
                remaining[dependent] = remaining[dependent]
                    .checked_add(1)
                    .ok_or_else(|| invariant("local admission edge count overflow"))?;
            }
        }
        let mut order = Vec::with_capacity(self.candidates.len());
        let mut ready = remaining
            .iter()
            .enumerate()
            .filter_map(|(index, count)| (*count == 0).then_some(index))
            .collect::<Vec<_>>();
        while let Some(index) = ready.pop() {
            order.push(index);
            for &dependent in &self.successors[index] {
                remaining[dependent] = remaining[dependent]
                    .checked_sub(1)
                    .ok_or_else(|| invariant("local admission edge count underflow"))?;
                if remaining[dependent] == 0 {
                    ready.push(dependent);
                }
            }
        }
        if order.len() != self.candidates.len() {
            return Err(invariant(
                "local admission dependency cycle or omitted command",
            ));
        }
        let mut slots = self.candidates.into_iter().map(Some).collect::<Vec<_>>();
        Ok(order
            .into_iter()
            .map(|index| slots[index].take().expect("each ready command enters once"))
            .collect())
    }
}

struct StockAdmission<'a> {
    candidates: &'a [IntentCandidate],
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
    use crate::session::pipeline::IntentCandidateKey;
    use crate::{Money, OrderId};

    fn quote_request(index: u64, account: u64, stock: &str, cancel: bool) -> IntentCandidate {
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
                price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                qty: 100,
            }
        };
        IntentCandidate::new(
            IntentCandidateKey::player(index),
            AccountId(account),
            intent,
        )
    }

    #[test]
    fn quote_replacements_wait_for_every_cancel_across_a_resource_fork() {
        let first = IntentCandidateKey::player(0);
        let second = IntentCandidateKey::player(1);
        let candidates = vec![
            quote_request(0, 1, "600001", true),
            quote_request(1, 1, "600001", true),
            quote_request(2, 1, "600001", false)
                .with_predecessors(vec![first.clone(), second.clone()]),
            // 最后撤单完成后两个方向均就绪；买单另有异股现金 lane 后继。
            IntentCandidate::new(
                IntentCandidateKey::player(3),
                AccountId(1),
                Intent::PlaceLimit {
                    code: StockCode("600001".to_owned()),
                    side: Side::Sell,
                    price: crate::LimitPrice::Fixed(Money::from_cents(110)),
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
                    let admitted =
                        admit_ready_batch(candidates.clone(), &mut AccountReceipts::default())
                            .unwrap();
                    assert_eq!(admitted.len(), candidates.len());
                    let position = |index| {
                        admitted
                            .iter()
                            .position(|candidate| {
                                candidate.key() == &IntentCandidateKey::player(index)
                            })
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
                            .map(IntentCandidate::key)
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
                admit_ready_batch(candidates, &mut AccountReceipts::default()).is_err(),
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
                IntentCandidate::new(
                    IntentCandidateKey::player(index as u64),
                    account,
                    Intent::PlaceLimit {
                        code,
                        side: Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                        qty: 100,
                    },
                )
            })
            .collect();
        for _ in 0..16 {
            let admitted = admit_ready_batch(candidates.clone(), &mut AccountReceipts::default())
                .expect("valid local admission");
            assert_eq!(admitted.len(), 4);
            for (account, keys) in [(AccountId(1), [0, 1]), (AccountId(2), [2, 3])] {
                let received = admitted
                    .iter()
                    .filter(|candidate| candidate.owner() == account)
                    .map(|candidate| candidate.key().clone())
                    .collect::<Vec<_>>();
                assert_eq!(received, keys.map(IntentCandidateKey::player));
            }
        }
    }

    #[test]
    fn same_account_sells_keep_share_receipt_order_and_cancel_is_admitted() {
        let stock = StockCode("600001".to_owned());
        let candidates = vec![
            IntentCandidate::new(
                IntentCandidateKey::npc(AccountId(1), 0),
                AccountId(1),
                Intent::Cancel {
                    code: stock.clone(),
                    id: OrderId(7),
                },
            ),
            IntentCandidate::new(
                IntentCandidateKey::npc(AccountId(1), 1),
                AccountId(1),
                Intent::PlaceLimit {
                    code: stock.clone(),
                    side: Side::Sell,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            ),
            IntentCandidate::new(
                IntentCandidateKey::npc(AccountId(1), 2),
                AccountId(1),
                Intent::PlaceLimit {
                    code: stock.clone(),
                    side: Side::Sell,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            ),
            IntentCandidate::new(
                IntentCandidateKey::player(0),
                AccountId(2),
                Intent::PlaceLimit {
                    code: stock,
                    side: Side::Buy,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            ),
        ];
        for _ in 0..16 {
            let admitted = admit_ready_batch(candidates.clone(), &mut AccountReceipts::default())
                .expect("valid local admission");
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
                    IntentCandidateKey::npc(AccountId(1), 1),
                    IntentCandidateKey::npc(AccountId(1), 2)
                ]
            );
            assert!(admitted
                .iter()
                .any(|item| item.key() == &IntentCandidateKey::npc(AccountId(1), 0)));
        }
    }

    #[test]
    fn cash_and_different_stock_share_lanes_have_independent_roots() {
        let owner = AccountId(1);
        let buy = quote_request(0, 1, "600001", false);
        let sell = |index| {
            IntentCandidate::new(
                IntentCandidateKey::player(index),
                owner,
                Intent::PlaceLimit {
                    code: StockCode("600002".to_owned()),
                    side: Side::Sell,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            )
        };
        let (plan, conflicts) = ReadyAdmissionPlan::prepare(
            vec![buy, sell(1), quote_request(2, 1, "600003", false), sell(3)],
            &mut AccountReceipts::default(),
            false,
        )
        .unwrap();
        assert!(conflicts);
        assert_eq!(plan.successors, vec![vec![2], vec![3], vec![], vec![]]);
        assert_eq!(plan.incoming, vec![0, 0, 1, 1]);
    }

    #[test]
    fn damaged_dependency_cycle_is_reported_by_gates_and_layout() {
        let plan = || ReadyAdmissionPlan {
            candidates: vec![
                quote_request(0, 1, "600001", false),
                quote_request(1, 1, "600002", false),
            ],
            successors: vec![vec![1], vec![0]],
            incoming: vec![1, 1],
            stock_gates: BTreeMap::new(),
        };
        assert!(plan()
            .run_stock_gates()
            .unwrap_err()
            .to_string()
            .contains("dependency cycle"));
        assert!(plan()
            .finish_layout()
            .unwrap_err()
            .to_string()
            .contains("dependency cycle"));
    }

    #[test]
    fn damaged_atomic_dependency_count_is_reported() {
        let candidates = vec![
            quote_request(0, 1, "600001", false),
            quote_request(1, 1, "600002", false),
        ];
        let successors = vec![vec![1], vec![]];
        let stock_gates = BTreeMap::new();
        let admission = StockAdmission {
            candidates: &candidates,
            successors: &successors,
            stock_gates: &stock_gates,
            remaining: vec![AtomicUsize::new(0), AtomicUsize::new(0)],
            registered: AtomicUsize::new(0),
            poisoned: AtomicBool::new(false),
            invalid_dependency_count: AtomicBool::new(false),
        };
        rayon::scope(|scope| admission.register(scope, 0));
        assert!(admission.invalid_dependency_count.load(Ordering::Relaxed));
        assert_eq!(admission.registered.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn mixed_request_phases_keep_cash_receipts_when_layout_is_reversed() {
        let owner = AccountId(1);
        let make = |key, stock: &str| {
            IntentCandidate::new(
                key,
                owner,
                Intent::PlaceLimit {
                    code: StockCode(stock.to_owned()),
                    side: Side::Buy,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            )
        };
        let candidates = vec![
            make(IntentCandidateKey::plan_chain(7), "600003"),
            make(IntentCandidateKey::player(5), "600002"),
            make(IntentCandidateKey::npc(owner, 9), "600001"),
        ];
        let admitted = admit_ready_batch(candidates, &mut AccountReceipts::default()).unwrap();
        assert_eq!(
            admitted
                .iter()
                .map(IntentCandidate::key)
                .collect::<Vec<_>>(),
            vec![
                &IntentCandidateKey::npc(owner, 9),
                &IntentCandidateKey::player(5),
                &IntentCandidateKey::plan_chain(7),
            ]
        );
    }

    #[test]
    fn plan_receipt_scan_keeps_progress_before_later_validation_error() {
        let owner = AccountId(1);
        let plan = IntentCandidate::new(
            IntentCandidateKey::plan_chain(0),
            owner,
            quote_request(0, 1, "600001", false).intent().clone(),
        )
        .with_predecessors(vec![IntentCandidateKey::player(99)]);
        let mut receipts = AccountReceipts::default();
        let error = admit_ready_batch(vec![plan.clone()], &mut receipts).unwrap_err();
        assert!(error.to_string().contains("predecessor is missing"));
        assert_eq!(
            receipts.observe(&plan).unwrap(),
            AccountReceipt::ReadyThisTick(1)
        );

        receipts.next_plan.insert(owner, u64::MAX - 1);
        let error = admit_ready_batch(vec![plan.clone(), plan], &mut receipts).unwrap_err();
        assert!(error.to_string().contains("plan account receipt overflow"));
        assert_eq!(receipts.next_plan[&owner], u64::MAX);
    }

    #[test]
    fn account_cash_conflict_uses_receipt_instead_of_candidate_layout() {
        let first = quote_request(0, 1, "600001", false);
        let second = quote_request(1, 1, "600002", false);
        let admitted =
            admit_ready_batch(vec![second, first], &mut AccountReceipts::default()).unwrap();
        assert_eq!(admitted[0].key(), &IntentCandidateKey::player(0));
        assert_eq!(admitted[1].key(), &IntentCandidateKey::player(1));
    }
}
