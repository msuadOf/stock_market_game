//! 就绪批的账户资源/同股 FIFO/报价依赖偏序与拓扑布局；来源与身份仅定位事实，
//! 不作为交易优先级（Sol 终审·冗余收口后单遍遍历）。

use super::{IntentCandidate, StepFatal};
use crate::{AccountId, Intent, Side, StockId};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ResourceLane {
    Cash(AccountId),
    Shares(AccountId, StockId),
}

/// receipt 只在同账户的资源 lane 内可比；阶段表示就绪时间。
/// 股票 gate 独立登记实际并发受理顺序。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum AccountReceipt {
    Received(u64),
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
        if let super::IntentCandidateKey::Npc { account, .. } = candidate.key() {
            if *account != candidate.owner() {
                return Err(invariant("NPC receipt belongs to another account"));
            }
        }
        if let Some((account_ordinal, _)) = candidate.ingress_order() {
            return Ok(AccountReceipt::Received(account_ordinal));
        }
        match candidate.key() {
            super::IntentCandidateKey::Npc { .. } | super::IntentCandidateKey::Player { .. } => {
                Err(invariant("queued candidate is missing its receive ordinal"))
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
    plan.finish_layout()
}

/// 仅拥有一个 ready batch 的候选索引与偏序。
/// 账户资源余额仍由 SealAllocationSnapshot snapshot 与 validator 管理。
/// Sol 终审·冗余收口：原 `stock_gates`（未盖 receive ordinal 的同股批以并行
/// 遍历登记到达序）删除——唯一生产调用（ready_ingress）先为全部候选 stamp
/// receive ordinal（`stamp_ready_receipts`），同股序由 FIFO 链边直接承载，
/// gate 形态生产不可达；未盖章同股批改为显式 invariant 拒绝（不静默取并行
/// 到达序这种无契约顺序）。
struct ReadyAdmissionPlan {
    candidates: Vec<IntentCandidate>,
    successors: Vec<Vec<usize>>,
    incoming: Vec<usize>,
}

impl ReadyAdmissionPlan {
    fn prepare(
        candidates: Vec<IntentCandidate>,
        receipts: &mut AccountReceipts,
        has_dependencies: bool,
    ) -> Result<(Self, bool), StepFatal> {
        let mut resource_groups = HashMap::<ResourceLane, Vec<(usize, AccountReceipt)>>::new();
        let mut stock_receipts = BTreeMap::<StockId, Vec<(usize, Option<u64>)>>::new();
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
            stock_receipts
                .entry(stock.clone())
                .or_default()
                .push((index, candidate.ingress_order().map(|(_, ordinal)| ordinal)));
        }
        let has_conflicts = has_dependencies
            || stock_receipts.values().any(|receipts| receipts.len() >= 2)
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
        };
        if !has_conflicts {
            return Ok((plan, false));
        }
        plan.build_resource_edges(resource_groups)?;
        if has_dependencies {
            plan.add_quote_dependencies()?;
        }
        for (_stock, mut receipts) in stock_receipts {
            if receipts.len() < 2 {
                continue;
            }
            if receipts.iter().all(|(_, ordinal)| ordinal.is_some()) {
                receipts.sort_unstable_by_key(|(_, ordinal)| *ordinal);
                for pair in receipts.windows(2) {
                    let (Some(before), Some(after)) = (pair[0].1, pair[1].1) else {
                        return Err(invariant("stock receive ordinal disappeared"));
                    };
                    if before == after {
                        return Err(invariant("conflicting requests share a stock receipt"));
                    }
                    plan.add_edge(pair[0].0, pair[1].0)?;
                }
            } else if receipts.iter().all(|(_, ordinal)| ordinal.is_none()) {
                // 原 gate 形态（并行登记到达序）随 Sol 终审·冗余收口删除：
                // 未盖章候选无真实 receive receipt，ADR-0032 契约下不存在可
                // 定义的受理顺序——显式拒绝而非取无契约的并行到达序。
                return Err(invariant(
                    "unstamped same-stock requests have no defined admission order",
                ));
            } else {
                return Err(invariant(
                    "ready stock batch mixes received and unstamped requests",
                ));
            }
        }
        Ok((plan, true))
    }

    fn add_edge(&mut self, before: usize, after: usize) -> Result<(), StepFatal> {
        self.successors[before].push(after);
        self.incoming[after] = self.incoming[after]
            .checked_add(1)
            .ok_or_else(|| invariant("local admission edge count overflow"))?;
        Ok(())
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

    fn finish_layout(mut self) -> Result<Vec<IntentCandidate>, StepFatal> {
        // 资源 lane 与 quote 依赖边直接拓扑排序（原并行 gate 到达边已随
        // gate 形态删除——Sol 终审·冗余收口：同股序由 FIFO 链边承载，
        // 单遍遍历即全部受理事实）；无关股票的输出布局不定义交易优先级。
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

fn code(intent: &Intent) -> &StockId {
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

    fn quote_request(index: u64, account: u64, stock: StockId, cancel: bool) -> IntentCandidate {
        let code = stock;
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
        .with_ingress_order(index, index)
    }

    #[test]
    fn quote_replacements_wait_for_every_cancel_across_a_resource_fork() {
        let first = IntentCandidateKey::player(0);
        let second = IntentCandidateKey::player(1);
        let candidates = vec![
            quote_request(0, 1, StockId(0), true),
            quote_request(1, 1, StockId(0), true),
            quote_request(2, 1, StockId(0), false)
                .with_predecessors(vec![first.clone(), second.clone()]),
            // 最后撤单完成后两个方向均就绪；买单另有异股现金 lane 后继。
            IntentCandidate::new(
                IntentCandidateKey::player(3),
                AccountId(1),
                Intent::PlaceLimit {
                    code: StockId(0),
                    side: Side::Sell,
                    price: crate::LimitPrice::Fixed(Money::from_cents(110)),
                    qty: 100,
                },
            )
            .with_ingress_order(3, 3)
            .with_predecessors(vec![first, second]),
            quote_request(4, 1, StockId(1), false),
            quote_request(5, 2, StockId(0), false),
        ];
        for _threads in [1, 4] {
            // rayon 退役（批 7）：install 的串行等价=直接执行（缩进保留）。
            if true {
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
            }
        }
    }

    #[test]
    fn quote_dependency_rejects_missing_reversed_or_unrelated_predecessors() {
        let cancel = quote_request(0, 1, StockId(0), true);
        let place =
            quote_request(1, 1, StockId(0), false).with_predecessors(vec![cancel.key().clone()]);
        let cases = [
            vec![place.clone()],
            vec![place.clone(), cancel.clone()],
            vec![quote_request(0, 2, StockId(0), true), place.clone()],
            vec![quote_request(0, 1, StockId(1), true), place.clone()],
            vec![quote_request(0, 1, StockId(0), false), place.clone()],
            vec![
                cancel.clone(),
                quote_request(1, 1, StockId(0), true).with_predecessors(vec![cancel.key().clone()]),
            ],
            vec![
                cancel.clone(),
                quote_request(1, 1, StockId(0), false)
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
        let x = StockId(0);
        let y = StockId(1);
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
                .with_ingress_order(index as u64, index as u64)
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
        let stock = StockId(0);
        let candidates = vec![
            IntentCandidate::new(
                IntentCandidateKey::npc(AccountId(1), 0),
                AccountId(1),
                Intent::Cancel {
                    code: stock.clone(),
                    id: OrderId(7),
                },
            )
            .with_ingress_order(0, 0),
            IntentCandidate::new(
                IntentCandidateKey::npc(AccountId(1), 1),
                AccountId(1),
                Intent::PlaceLimit {
                    code: stock.clone(),
                    side: Side::Sell,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            )
            .with_ingress_order(1, 1),
            IntentCandidate::new(
                IntentCandidateKey::npc(AccountId(1), 2),
                AccountId(1),
                Intent::PlaceLimit {
                    code: stock.clone(),
                    side: Side::Sell,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            )
            .with_ingress_order(2, 2),
            IntentCandidate::new(
                IntentCandidateKey::player(0),
                AccountId(2),
                Intent::PlaceLimit {
                    code: stock,
                    side: Side::Buy,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            )
            .with_ingress_order(0, 3),
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
        let buy = quote_request(0, 1, StockId(0), false);
        let sell = |index| {
            IntentCandidate::new(
                IntentCandidateKey::player(index),
                owner,
                Intent::PlaceLimit {
                    code: StockId(1),
                    side: Side::Sell,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            )
            .with_ingress_order(index, index)
        };
        let (plan, conflicts) = ReadyAdmissionPlan::prepare(
            vec![
                buy,
                sell(1),
                quote_request(2, 1, StockId(2), false),
                sell(3),
            ],
            &mut AccountReceipts::default(),
            false,
        )
        .unwrap();
        assert!(conflicts);
        // known-6 #3 处置（B 类重锚，ADR-0032 §19/R3）：同账户同股对的
        // 两条契约边**并存**——资源 lane（Shares）链边 + 同股 FIFO 链边
        //（97e4953a 起真实 receive receipt 建链）⇒ successors[1]=[3,3] 双边、
        // incoming[3]=2（旧期望 [[2],[3],[],[]]/[0,0,1,1] 为单边时代形状，
        // 保留于本注释；incoming/successors 双计一致，executor 逐后继减计数）。
        assert_eq!(plan.successors, vec![vec![2], vec![3, 3], vec![], vec![]]);
        assert_eq!(plan.incoming, vec![0, 0, 1, 2]);
    }

    /// Sol 终审·冗余收口：原 gates 半段（run_stock_gates 并行遍历报告环）随
    /// gate 形态删除退役——环检测由 finish_layout 单遍拓扑排序承担；
    /// 原 `damaged_atomic_dependency_count_is_reported`（直接构造
    /// StockAdmission 验证原子计数下溢负路径）随 StockAdmission 退役。
    ///（增量一百：清理清理批遗留的重复 `#[test]` attribute——零行为变更。）
    #[test]
    fn damaged_dependency_cycle_is_reported_by_layout() {
        let plan = || ReadyAdmissionPlan {
            candidates: vec![
                quote_request(0, 1, StockId(0), false),
                quote_request(1, 1, StockId(1), false),
            ],
            successors: vec![vec![1], vec![0]],
            incoming: vec![1, 1],
        };
        assert!(plan()
            .finish_layout()
            .unwrap_err()
            .to_string()
            .contains("dependency cycle"));
    }

    #[test]
    fn mixed_request_phases_keep_cash_receipts_when_layout_is_reversed() {
        let owner = AccountId(1);
        let make = |key, stock: StockId, account_ordinal, stock_ordinal| {
            IntentCandidate::new(
                key,
                owner,
                Intent::PlaceLimit {
                    code: stock,
                    side: Side::Buy,
                    price: crate::LimitPrice::Fixed(Money::from_cents(100)),
                    qty: 100,
                },
            )
            .with_ingress_order(account_ordinal, stock_ordinal)
        };
        let candidates = vec![
            make(IntentCandidateKey::plan_chain(7), StockId(2), 2, 2),
            make(IntentCandidateKey::player(5), StockId(1), 1, 1),
            make(IntentCandidateKey::npc(owner, 9), StockId(0), 0, 0),
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
    fn queued_candidates_require_real_receipts_and_validate_npc_ownership() {
        let missing = IntentCandidate::new(
            IntentCandidateKey::player(0),
            AccountId(0),
            quote_request(0, 0, StockId(0), false).intent().clone(),
        );
        let missing_error =
            admit_ready_batch(vec![missing], &mut AccountReceipts::default()).unwrap_err();
        assert!(missing_error
            .to_string()
            .contains("missing its receive ordinal"));

        let wrong_owner = IntentCandidate::new(
            IntentCandidateKey::npc(AccountId(1), 0),
            AccountId(2),
            quote_request(0, 2, StockId(0), false).intent().clone(),
        )
        .with_ingress_order(0, 0);
        let owner_error =
            admit_ready_batch(vec![wrong_owner], &mut AccountReceipts::default()).unwrap_err();
        assert!(owner_error
            .to_string()
            .contains("NPC receipt belongs to another account"));
    }

    #[test]
    fn plan_receipt_scan_keeps_progress_before_later_validation_error() {
        let owner = AccountId(1);
        let plan = IntentCandidate::new(
            IntentCandidateKey::plan_chain(0),
            owner,
            quote_request(0, 1, StockId(0), false).intent().clone(),
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
        let first = quote_request(0, 1, StockId(0), false);
        let second = quote_request(1, 1, StockId(1), false);
        let admitted =
            admit_ready_batch(vec![second, first], &mut AccountReceipts::default()).unwrap();
        assert_eq!(admitted[0].key(), &IntentCandidateKey::player(0));
        assert_eq!(admitted[1].key(), &IntentCandidateKey::player(1));
    }
}
