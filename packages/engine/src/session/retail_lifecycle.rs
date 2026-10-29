//! 两个默认 Retail Vec 共用未发布 candidate，最外操作成功后一起 move。

use super::*;
use crate::runtime::retail_lifecycle::{BatchContext, Fact, FactKind, Termination};

pub(crate) struct Candidate {
    pub events: Vec<RetailOrderDiagnosticEvent>,
    pub decisions: Vec<RetailDecisionTrace>,
}

impl GameSession {
    pub(crate) fn append_runtime_retail_facts_at(
        &self,
        driver: &crate::runtime::drive::RuntimeDriver,
        candidate: &mut Candidate,
        facts: Vec<Fact>,
        target_tick: u64,
    ) -> Result<(), StepFatal> {
        let fail = |description| StepFatal::InvariantViolation {
            location: "runtime::retail_lifecycle::prepare".to_owned(),
            description,
        };
        for fact in facts {
            let stock = fact.context.stock();
            match fact.context {
                BatchContext::Tick { tick, .. } if tick != target_tick => {
                    return Err(fail(format!(
                        "Tick 生命周期时点错误：{tick} != {}",
                        target_tick
                    )))
                }
                BatchContext::Drain {
                    ended_date,
                    final_tick,
                    ..
                } if ended_date != self.civil_date() || final_tick != target_tick => {
                    return Err(fail(format!(
                        "Drain 生命周期时点错误：{ended_date:?}/{final_tick}"
                    )))
                }
                _ => {}
            }
            let (order, owner) = match fact.kind {
                FactKind::Submitted { order, owner, .. }
                | FactKind::Filled { order, owner, .. }
                | FactKind::Terminated { order, owner, .. } => (order, owner),
            };
            if order.0 != stock
                || order.1 > u64::from(u32::MAX)
                || usize::try_from(owner.1)
                    .map_or(true, |local| local >= driver.accounts_per_group())
            {
                return Err(fail(format!("生命周期身份域错误：{order:?}/{owner:?}")));
            }
            let account = driver
                .account_by_owner(owner)
                .ok_or_else(|| fail(format!("生命周期 Owner 不在真实名册：{owner:?}")))?;
            let entity = self
                .account(account)
                .ok_or_else(|| fail(format!("生命周期 AccountId 不存在：{account:?}")))?;
            if entity.kind() != AccountKind::Retail {
                continue;
            }
            let code = StockId(u32::from(stock));
            if !self.state.markets.contains_key(&code) {
                return Err(fail(format!("生命周期 StockId 不存在：{code:?}")));
            }
            let order_id = OrderId((u64::from(order.0) << 32) | order.1);
            let event = match fact.kind {
                FactKind::Submitted { side, qty, .. } => RetailOrderDiagnosticEvent::Submitted {
                    account,
                    code,
                    side,
                    order_id,
                    qty,
                },
                FactKind::Filled { side, qty, .. } => RetailOrderDiagnosticEvent::Filled {
                    account,
                    code,
                    side,
                    order_id,
                    qty,
                },
                FactKind::Terminated {
                    qty,
                    end: Termination::Canceled,
                    ..
                } => RetailOrderDiagnosticEvent::Canceled {
                    account,
                    code,
                    order_id,
                    remaining_qty: qty,
                },
                FactKind::Terminated {
                    qty,
                    end: Termination::Aborted,
                    ..
                } => RetailOrderDiagnosticEvent::Aborted {
                    account,
                    code,
                    order_id,
                    remaining_qty: qty,
                },
            };
            candidate.events.push(event);
        }
        Ok(())
    }

    pub(crate) fn append_runtime_retail_rejections(
        &self,
        candidate: &mut Candidate,
        rejections: Vec<(AccountId, StockId, RejectionReason)>,
    ) -> Result<(), StepFatal> {
        let mut by_account =
            std::collections::BTreeMap::<AccountId, Vec<(StockId, RejectionReason)>>::new();
        for (account, code, reason) in rejections {
            let entity = self
                .account(account)
                .ok_or_else(|| StepFatal::InvariantViolation {
                    location: "runtime::retail_lifecycle::prepare".to_owned(),
                    description: format!("拒单 AccountId 不存在：{account:?}"),
                })?;
            if entity.kind() == AccountKind::Retail {
                by_account.entry(account).or_default().push((code, reason));
            }
        }
        for (account, items) in by_account {
            candidate
                .events
                .extend(items.into_iter().map(|(code, reason)| {
                    RetailOrderDiagnosticEvent::Rejected {
                        account,
                        code,
                        reason,
                    }
                }));
        }
        Ok(())
    }

    pub(crate) fn publish_runtime_retail(&mut self, candidate: Candidate) {
        self.state.last_retail_order_events = candidate.events;
        self.state.last_retail_decisions = candidate.decisions;
    }
}
