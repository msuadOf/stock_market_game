//! 受限拆缩股盘前候选；真实账户操作留给同一 Admin 事务。
use super::distributions_day_end::*;
use crate::company::share_registry::settlement_day::{
    FrozenOpeningMovement, OpeningMovementRef, ShareUnitIdentity,
};
use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, LotConsumption, MovementScope, NetAcquisition,
    ShareDayRequest, ShareRestriction,
};
use crate::company::share_split::{ShareSplitEventPlan, ShareSplitStatus};
fn contextual(
    plan: &ShareSplitEventPlan,
    day: crate::CivilDate,
    stock: Option<crate::StockId>,
    source: impl Into<DistributionsDayEndFailure>,
) -> DistributionsDayEndError {
    DistributionsDayEndError {
        day,
        event_id: plan.event_id.clone(),
        stock,
        account: None,
        source: Box::new(source.into()),
    }
}

impl DistributionsDayEndCandidate {
    pub(crate) fn prepare_split_opening(
        &mut self,
        input: &DistributionsDayEndInput<'_>,
    ) -> Result<(), DistributionsDayEndError> {
        for index in 0..self.actions.share_splits.len() {
            let book = &self.actions.share_splits[index];
            if book.status() != &ShareSplitStatus::Registered
                || book.plan().ex_rights_on != input.day
            {
                continue;
            }
            let plan = book.plan().clone();
            let stock = input.catalog.id(&plan.stock.0);
            let fail = |detail: &str| DistributionsDayEndError {
                day: input.day,
                event_id: plan.event_id.clone(),
                stock,
                account: None,
                source: Box::new(DistributionsDayEndFailure::Invalid(detail.into())),
            };
            let stock = stock.ok_or_else(|| fail("拆缩股证券不在目录"))?;
            let receipt = book
                .receipt()
                .ok_or_else(|| fail("拆缩股缺冻结换算"))?
                .clone();
            let mut changes = Vec::new();
            let mut source_lots = Vec::new();
            for holder in &receipt.holders {
                // REG 保留真实卖尽后的空 holder；其无旧股、无新股，也无来源 lot 可绑定。
                if holder.original_shares == 0
                    && holder.new_shares == 0
                    && holder.original_lots.is_empty()
                {
                    continue;
                }
                let HolderId::Account(account) = holder.holder else {
                    return Err(fail("拆缩股只支持真实账户"));
                };
                if holder
                    .original_lots
                    .iter()
                    .any(|lot| lot.restriction != ShareRestriction::Unrestricted)
                    || !input.available_cash.contains_key(&account)
                {
                    return Err(fail("拆缩股来源存在限售或缺少真实账户"));
                }
                let delta = i128::from(holder.new_shares) - i128::from(holder.original_shares);
                let acquisition = (delta > 0).then(|| NetAcquisition {
                    lot_id: format!("share-transform:{}:account-{}", plan.event_id, account.0),
                    source: AcquisitionSource::CorporateAction {
                        event: plan.event_id.clone(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                });
                let consume = if delta < 0 {
                    let mut remaining =
                        u64::try_from(-delta).map_err(|_| fail("缩股核减股数溢出"))?;
                    let mut rows = Vec::new();
                    for lot in &holder.original_lots {
                        let qty = remaining.min(lot.qty);
                        if qty > 0 {
                            rows.push(LotConsumption {
                                lot_id: lot.id.clone(),
                                qty,
                            });
                            remaining -= qty;
                        }
                    }
                    if remaining != 0 {
                        return Err(fail("缩股来源不足"));
                    }
                    Some(rows)
                } else {
                    None
                };
                source_lots.push(
                    holder
                        .original_lots
                        .iter()
                        .map(|lot| lot.id.clone())
                        .collect(),
                );
                changes.push(DayNetChange {
                    holder: holder.holder.clone(),
                    change: delta,
                    acquisition,
                    consume,
                });
                if delta != 0 {
                    self.intents.push(DistributionIntent::ShareSplitAdjustment {
                        event_id: plan.event_id.clone(),
                        account,
                        stock,
                        change: delta,
                        available_on: input.day,
                    });
                    self.sources.push(DistributionSource::ShareSplit {
                        event_id: plan.event_id.clone(),
                        holder: holder.holder.clone(),
                        stock,
                        change: delta,
                    });
                }
            }
            let registry = self
                .actions
                .registries
                .iter_mut()
                .find(|registry| {
                    registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                })
                .ok_or_else(|| fail("拆缩股缺名册"))?;
            let movement_id = format!("share-transform:{}", plan.event_id);
            let movement = FrozenOpeningMovement {
                reference: OpeningMovementRef {
                    movement_event_id: movement_id.clone(),
                    registration_event_id: plan.event_id.clone(),
                    source_lots,
                    from_unit: registry.current_unit(),
                    to_unit: ShareUnitIdentity::ReDenominated {
                        movement_event_id: movement_id.clone(),
                    },
                },
                request: ShareDayRequest {
                    event_id: movement_id,
                    day: input.day,
                    scope: MovementScope::ShareReDenomination {
                        basis: plan.approval_reference.clone(),
                    },
                    changes,
                },
            };
            let predecessor = registry
                .settlement_frontier()
                .map_err(|error| contextual(&plan, input.day, Some(stock), error))?;
            *registry = registry
                .prepare_opening_movement(&predecessor, &movement)
                .map_err(|error| contextual(&plan, input.day, Some(stock), error))?;
            self.company_system
                .record_share_split_credit(
                    &plan.issuer,
                    &plan.event_id,
                    input.day,
                    receipt.issued_shares_before,
                    receipt.issued_shares_after,
                )
                .map_err(|error| contextual(&plan, input.day, Some(stock), error))?;
            self.actions.share_splits[index]
                .mark_settled(input.day)
                .map_err(|error| contextual(&plan, input.day, Some(stock), error))?;
        }
        self.company_system
            .validate_share_split_books(&self.actions.share_splits)
            .map_err(|error| DistributionsDayEndError {
                day: input.day,
                event_id: "split-opening".into(),
                stock: None,
                account: None,
                source: Box::new(DistributionsDayEndFailure::Company(error)),
            })?;
        Ok(())
    }
}
