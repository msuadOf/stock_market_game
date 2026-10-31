//! Simple 回购执行适配；唯一公司 book 消费真实来源，所有安装共用原 civil Admin。
use super::*;
use crate::company::issuer_repurchase::*;
use crate::runtime::drive::AdministrativeBaseline;
use crate::runtime::world::{AdminCandidate, AdminOperation, AdminOperationStage};
use crate::runtime::world::{
    AdminPlanReference, RepurchaseExecutionProjection, RepurchaseMaterialRestriction,
};
use crate::runtime::{RepurchasePreparePayload, RepurchaseProjectionInput};
use std::sync::Arc;

fn invalid(detail: impl Into<String>) -> SessionError {
    SessionError::UnsupportedCapitalBoundary(format!(
        "回购执行：{}；请反馈方案、日期与存档",
        detail.into()
    ))
}
fn checked(error: impl std::fmt::Display) -> SessionError {
    invalid(error.to_string())
}

impl GameSession {
    /// Simple 是封闭的公司事实集合；只认证本模型实际存在的事项，不将外部未知规则改写为 Clear。
    pub(super) fn repurchase_eligibility(
        &self,
        plan: &IssuerRepurchasePlan,
        at: crate::CivilInstant,
    ) -> Result<(Option<bool>, RepurchaseMaterialRestriction, bool), SessionError> {
        let system = self
            .company_system()
            .ok_or_else(|| invalid("仅支持 Simple 公司源"))?;
        if !matches!(
            system.config(),
            crate::company::config::CompanySystemConfig::Simple(_)
        ) {
            return Err(invalid("仅支持 Simple"));
        }
        let actions = self
            .corporate_actions()
            .ok_or_else(|| invalid("缺少公司行为集合"))?;
        system
            .validate_rights_offering_books(&actions.rights_offerings)
            .map_err(checked)?;
        system
            .validate_cash_dividend_books(&actions.dividends)
            .map_err(checked)?;
        system
            .validate_stock_distribution_books(&actions.stock_distributions)
            .map_err(checked)?;
        system
            .validate_share_split_books(&actions.share_splits)
            .map_err(checked)?;
        system
            .validate_issuer_repurchase_books(&actions.issuer_repurchases)
            .map_err(checked)?;
        let pending_issuance = actions
            .rights_offerings
            .iter()
            .filter(|book| book.plan().issuer == plan.issuer)
            .any(|book| {
                book.settlement().is_none()
                    || book
                        .settlement()
                        .is_some_and(|s| s.settlement_on > at.date())
                    || book.credited_on().is_some_and(|day| day > at.date())
                    || (book.settlement().is_some_and(|s| !s.failed)
                        && book.credited_on().is_none())
            });
        // 尚无邀请书／注册启动 producer，不能从 Approved 推断实施起点。
        if pending_issuance {
            return Err(invalid(
                "存在待实施发行，但无法证明发送认购邀请书／注册启动与新增股份登记区间",
            ));
        }
        if plan.purpose != RepurchasePurpose::ReduceCapital
            || plan.approval_unit_event_id.is_some()
            || actions
                .share_splits
                .iter()
                .any(|book| book.plan().issuer == plan.issuer)
            || actions
                .stock_distributions
                .iter()
                .any(|book| book.plan().issuer == plan.issuer && book.credited_on().is_none())
        {
            return Err(invalid(
                "当前只支持 ReduceCapital、基准股份单位及无交叠送转／拆缩股的真实执行",
            ));
        }
        let published = self
            .state
            .library
            .announcements_for_company(&plan.issuer, at);
        let announced = published.iter().any(|entry| matches!(&entry.content, crate::information::AnnouncementContent::IssuerRepurchase(value) if value.plan == *plan));
        // 所有当前 Simple 非回购资本决策必须已依法披露；更正处于决策过程时仍受限。
        let restricted = has_issuer_correction(self.pending_report_corrections(), plan.issuer)
            || actions.dividends.iter().any(|book| book.plan().issuer == plan.issuer
                && !published.iter().any(|entry| matches!(&entry.content, crate::information::AnnouncementContent::CashDividend(value) if value.plan == *book.plan())))
            || actions.rights_offerings.iter().any(|book| book.plan().issuer == plan.issuer
                && !published.iter().any(|entry| matches!(&entry.content, crate::information::AnnouncementContent::RightsOffering(value) if value.plan == *book.plan())))
            || actions.stock_distributions.iter().any(|book| book.plan().issuer == plan.issuer
                && !published.iter().any(|entry| matches!(&entry.content, crate::information::AnnouncementContent::StockDistribution(value) if value == book.plan())))
            || actions.issuer_repurchases.iter().any(|book| book.plan().issuer == plan.issuer && book.plan().event_id != plan.event_id
                && !published.iter().any(|entry| matches!(&entry.content, crate::information::AnnouncementContent::IssuerRepurchase(value) if value.plan == *book.plan())));
        if published.iter().any(|entry| {
            matches!(
                entry.content,
                crate::information::AnnouncementContent::Shock(_)
            )
        }) {
            return Err(invalid(
                "Simple 中混入无法证明终结的 Shock，重大事项范围未知",
            ));
        }
        Ok((
            Some(false),
            if restricted {
                RepurchaseMaterialRestriction::Restricted
            } else {
                RepurchaseMaterialRestriction::Clear
            },
            announced,
        ))
    }

    fn repurchase_projection_eligibility(
        &self,
        plan: &IssuerRepurchasePlan,
        closing: RepurchaseBindingClosingState,
        at: crate::CivilInstant,
    ) -> Result<(Option<bool>, RepurchaseMaterialRestriction, bool), SessionError> {
        if closing != RepurchaseBindingClosingState::Closed {
            return self.repurchase_eligibility(plan, at);
        }
        // 退役方案只保留真实公告；不为后续无关公司行为重新认证下单资格。
        let announced = self.state.library.announcements_for_company(&plan.issuer, at).iter().any(|entry| matches!(&entry.content, crate::information::AnnouncementContent::IssuerRepurchase(value) if value.plan == *plan));
        Ok((None, RepurchaseMaterialRestriction::NotModelled, announced))
    }

    pub(crate) fn repurchase_live_eligibility(
        &self,
        at: crate::CivilInstant,
    ) -> Result<
        BTreeMap<crate::plans::PlanId, crate::runtime::world::RepurchaseEligibility>,
        SessionError,
    > {
        let Some(actions) = self.corporate_actions() else {
            return Ok(BTreeMap::new());
        };
        let mut result = BTreeMap::new();
        for binding in &actions.repurchase_execution_bindings {
            if binding.closing_state != RepurchaseBindingClosingState::Active {
                continue;
            }
            let book = actions
                .issuer_repurchases
                .iter()
                .find(|book| book.plan().event_id == binding.event_id)
                .ok_or_else(|| invalid("现场资格缺少 book"))?;
            let (issuance_in_progress, material_restriction, announced) =
                self.repurchase_eligibility(book.plan(), at)?;
            result.insert(
                binding.plan_id,
                crate::runtime::world::RepurchaseEligibility {
                    as_of: at,
                    issuance_in_progress,
                    material_restriction,
                    announced,
                },
            );
        }
        Ok(result)
    }

    pub(crate) fn repurchase_restore_projections(
        &self,
    ) -> Result<BTreeMap<crate::plans::PlanId, RepurchaseExecutionProjection>, SessionError> {
        let Some(actions) = self.corporate_actions() else {
            return Ok(BTreeMap::new());
        };
        self.validate_repurchase_bindings().map_err(checked)?;
        let mut result = BTreeMap::new();
        for binding in &actions.repurchase_execution_bindings {
            let book = actions
                .issuer_repurchases
                .iter()
                .find(|book| book.plan().event_id == binding.event_id)
                .ok_or_else(|| invalid("binding 没有唯一 book"))?;
            let plan = book.plan();
            if binding.closing_state == RepurchaseBindingClosingState::Draining {
                return Err(invalid("日终不允许尚未完成 Drain 的回购 binding"));
            }
            let at = crate::CivilInstant::new(self.civil_date(), 0).map_err(checked)?;
            let (issuance, restriction, announced) =
                self.repurchase_projection_eligibility(plan, binding.closing_state, at)?;
            let gross = book
                .fills()
                .iter()
                .try_fold(Money::ZERO, |sum, fill| sum.add(fill.gross))
                .map_err(checked)?;
            let fees = book
                .fills()
                .iter()
                .try_fold(Money::ZERO, |sum, fill| sum.add(fill.fees))
                .map_err(checked)?;
            let inventory = book.current_inventory_shares(&[]).map_err(checked)?;
            if announced != (book.status() != &IssuerRepurchaseStatus::Approved) {
                return Err(invalid("公司 book 公告状态与真实公开库不一致"));
            }
            let closed = binding.closing_state == RepurchaseBindingClosingState::Closed;
            if closed
                != matches!(
                    book.status(),
                    IssuerRepurchaseStatus::Completed | IssuerRepurchaseStatus::Cancelled
                )
                || (book.last_order_day().is_some() && binding.applied_fact_cursor == 0)
                || self
                    .runtime_plans_snapshot()
                    .entry(binding.plan_id)
                    .map_err(checked)?
                    .execution()
                    .is_some()
            {
                return Err(invalid(
                    "日终公司 book、cursor、closing 或残留 execution 不一致",
                ));
            }
            result.insert(
                binding.plan_id,
                RepurchaseExecutionProjection {
                    account: binding.account,
                    issuer: plan.issuer,
                    stock: u16::try_from(
                        self.stock_id(&plan.stock.0)
                            .ok_or_else(|| invalid("证券不在目录"))?
                            .0,
                    )
                    .map_err(checked)?,
                    announced,
                    announced_on: plan.announced_on,
                    window_start_on: plan.window_start_on,
                    window_deadline_on: plan.window_deadline_on,
                    exchange: plan.exchange,
                    purpose: plan.purpose.clone(),
                    funding_amount: plan.total_budget,
                    price_cap: plan.price_cap_per_share,
                    max_shares: plan.max_shares,
                    unit_as_of: plan.approved_on,
                    unit_revision: 0,
                    book_through_cursor: binding.applied_fact_cursor,
                    book_gross: gross,
                    book_fees: fees,
                    recycled: book.withdrawn_remainder().unwrap_or(Money::ZERO),
                    inventory_shares: inventory,
                    last_order_day: book.last_order_day(),
                    eligibility_revision: 0,
                    eligibility_as_of: at,
                    issuance_in_progress: issuance,
                    material_restriction: restriction,
                },
            );
        }
        Ok(result)
    }

    /// 只在 shadow 上记录；组来源前缀要在同次 inspect 逐字段相等，Abort 不消费。
    pub(super) fn prepare_repurchase_sources(
        &mut self,
        day: CivilDate,
        baseline: &AdministrativeBaseline,
    ) -> Result<(), SessionError> {
        let Some(actions) = self.corporate_actions() else {
            return Ok(());
        };
        if baseline.repurchase.len() != actions.repurchase_execution_bindings.len() {
            return Err(invalid("真实 Group 投影与公司 binding 集合不相等"));
        }
        let mut candidate = actions.clone();
        for binding in &mut candidate.repurchase_execution_bindings {
            let (projection, source) = baseline
                .repurchase
                .get(&binding.plan_id)
                .ok_or_else(|| invalid("缺少 Group 来源"))?;
            if binding.applied_fact_cursor != source.consumed_cursor
                || projection.book_through_cursor != source.consumed_cursor
                || source.child_active
                || source.facts.len() as u64
                    != source
                        .tail_cursor
                        .checked_sub(source.consumed_cursor)
                        .ok_or_else(|| invalid("来源 cursor 倒退"))?
            {
                return Err(invalid("Drain 后 cursor 或 child 不一致"));
            }
            let book = candidate
                .issuer_repurchases
                .iter_mut()
                .find(|book| book.plan().event_id == binding.event_id)
                .ok_or_else(|| invalid("来源 event 缺少公司 book"))?;
            let plan = book.plan().clone();
            if book.status() == &IssuerRepurchaseStatus::Approved && plan.announced_on < day {
                return Err(invalid("公司 book 错过真实公告日"));
            }
            if book.status() == &IssuerRepurchaseStatus::Approved && plan.announced_on == day {
                let at = crate::CivilInstant::from_hms(day, 18, 0, 0).map_err(checked)?;
                if !self.repurchase_eligibility(&plan, at)?.2 {
                    return Err(invalid("方案公告日没有真实公告"));
                }
                book.announce(day).map_err(checked)?;
            }
            let stock_id = self
                .stock_id(&plan.stock.0)
                .ok_or_else(|| invalid("来源证券缺失"))?;
            let model = super::exchange_calendar_model::calendar_day_model(
                self.state.civil_clock.calendar(),
                self.state.setup.start_date,
                day,
                &[(stock_id, plan.exchange)],
            )
            .map_err(checked)?;
            let expected_day = model
                .stock(stock_id)
                .map_err(checked)?
                .sessions_before_date();
            let mut qty = 0_u64;
            let mut gross = Money::ZERO;
            let mut fees = Money::ZERO;
            for (index, fact) in source.facts.iter().enumerate() {
                if source.consumed_cursor.checked_add(index as u64 + 1) != Some(fact.cursor)
                    || fact.tick == 0
                    || fact.tick > self.tick()
                    || fact.source.request_key.requested_trading_day != expected_day
                    || fact.source.request_key.plan_id != binding.plan_id
                    || fact.source.request_key.stock
                        != self
                            .stock_id(&plan.stock.0)
                            .ok_or_else(|| invalid("来源证券不在目录"))?
                {
                    return Err(invalid("来源前缀错序、未来 tick 或身份错配"));
                }
                // 单次 civil 仅消费本日市场事实，跨日残流不能被当前日期冒领。
                if fact.tick <= self.tick().saturating_sub(self.state.setup.ticks_per_day) {
                    return Err(invalid("来源 tick 不属于本日"));
                }
                book.set_last_order_day(day);
                if let crate::runtime::RepurchaseSourceKind::Fill(fill) = fact.source.kind {
                    fill.validate().map_err(checked)?;
                    if fill.side != Side::Buy || fill.stamp_tax != 0 {
                        return Err(invalid("发行人回购必须为真实买腿且买入无印花税"));
                    }
                    qty = qty
                        .checked_add(u64::from(fill.qty))
                        .ok_or_else(|| invalid("来源股数溢出"))?;
                    gross = gross
                        .add(Money::from_cents(
                            fill.price
                                .checked_mul(i64::from(fill.qty))
                                .ok_or_else(|| invalid("来源金额溢出"))?,
                        ))
                        .map_err(checked)?;
                    fees = fees
                        .add(Money::from_cents(
                            fill.commission
                                .checked_add(fill.transfer_fee)
                                .ok_or_else(|| invalid("来源费用溢出"))?,
                        ))
                        .map_err(checked)?;
                }
            }
            if qty > 0 {
                book.record_fill(
                    RepurchaseFillRecord {
                        stock: plan.stock.clone(),
                        day,
                        shares: qty,
                        gross,
                        fees,
                    },
                    &[],
                )
                .map_err(checked)?;
            }
            binding.applied_fact_cursor = source.tail_cursor;
        }
        *self
            .state
            .companies
            .corporate_actions_mut()
            .expect("已校验公司源") = candidate;
        Ok(())
    }

    /// ordinary REG 之后准备精确回收与后续日终注销；数值与公司事实仍只在同次 Admin 发布。
    pub(super) fn prepare_repurchase_closing(
        &mut self,
        day: CivilDate,
        baseline: &AdministrativeBaseline,
    ) -> Result<(Vec<AdminOperationStage>, Option<RepurchasePreparePayload>), SessionError> {
        let Some(actions) = self.corporate_actions() else {
            return Ok((Vec::new(), None));
        };
        if actions.repurchase_execution_bindings.is_empty() {
            return Ok((Vec::new(), None));
        }
        let mut candidate = actions.clone();
        let mut system = self
            .company_system()
            .ok_or_else(|| invalid("缺少 CompanySystem"))?
            .clone();
        let mut ops: BTreeMap<u16, Vec<AdminOperation>> = BTreeMap::new();
        let mut rows: BTreeMap<u16, Vec<RepurchaseProjectionInput>> = BTreeMap::new();
        for binding in &mut candidate.repurchase_execution_bindings {
            let index = baseline
                .roster
                .iter()
                .position(|id| *id == binding.account)
                .ok_or_else(|| invalid("专户不在真实名册"))?;
            let group = u16::try_from(index / baseline.per_group).map_err(checked)?;
            let local = u32::try_from(index % baseline.per_group).map_err(checked)?;
            let actual = &baseline.accounts[group as usize].1[local as usize];
            if actual.buy_purse_held_cents != 0
                || actual.foreign_active.iter().any(|v| *v != 0)
                || actual.pending_sell_qty.iter().any(|v| *v != 0)
            {
                return Err(invalid("Drain 后专户仍有占用"));
            }
            let book = candidate
                .issuer_repurchases
                .iter_mut()
                .find(|book| book.plan().event_id == binding.event_id)
                .ok_or_else(|| invalid("closing 缺少公司 book"))?;
            let plan = book.plan().clone();
            let stock = u16::try_from(
                self.stock_id(&plan.stock.0)
                    .ok_or_else(|| invalid("证券不在目录"))?
                    .0,
            )
            .map_err(checked)?;
            if matches!(
                book.status(),
                IssuerRepurchaseStatus::Announced | IssuerRepurchaseStatus::Executing
            ) && day >= plan.window_start_on
                && (day >= plan.window_deadline_on
                    || plan
                        .max_shares
                        .checked_sub(book.current_inventory_shares(&[]).map_err(checked)?)
                        .is_some_and(|qty| qty < 100)
                    || book.remaining_budget().map_err(checked)? == Money::ZERO)
            {
                let registry = candidate
                    .registries
                    .iter()
                    .find(|registry| registry.stock() == &plan.stock)
                    .ok_or_else(|| invalid("回收缺少 REG"))?;
                let intent = crate::runtime::group::repurchase_funds_candidate::closed_window_recycle_candidate(book, registry,
                    self.account(binding.account).ok_or_else(|| invalid("回收专户缺失"))?,
                    crate::runtime::group::repurchase_funds_candidate::RepurchasePlanRead { plans: &baseline.plans, execution_plan: binding.plan_id },
                    &self.state.stocks, actual, day).map_err(checked)?;
                if intent.amount > Money::ZERO {
                    ops.entry(group)
                        .or_default()
                        .push(AdminOperation::SyntheticBudgetRecycle {
                            local,
                            stock,
                            execution_plan: binding.plan_id,
                            amount: intent.amount.cents(),
                        });
                }
                book.complete(day, intent.amount).map_err(checked)?;
                system
                    .record_issuer_repurchase_completion(
                        &plan.issuer,
                        &plan.event_id,
                        day,
                        crate::accounting::AccountingAmount::from_money(
                            book.total_spent().map_err(checked)?,
                        ),
                        crate::accounting::AccountingAmount::from_money(intent.amount),
                    )
                    .map_err(checked)?;
                binding.closing_state = RepurchaseBindingClosingState::Closed;
                ops.entry(group)
                    .or_default()
                    .push(AdminOperation::CloseIssuerRepurchase {
                        local,
                        stock,
                        execution_plan: binding.plan_id,
                    });
            } else if book.status() == &IssuerRepurchaseStatus::Completed
                && book.completed_on().is_some_and(|on| on < day)
            {
                let inventory = book.current_inventory_shares(&[]).map_err(checked)?;
                if inventory > 0 {
                    let registry = candidate
                        .registries
                        .iter()
                        .find(|registry| registry.stock() == &plan.stock)
                        .ok_or_else(|| invalid("注销缺少 REG"))?;
                    let event = format!("issuer-repurchase-cancellation:{}", plan.event_id);
                    let receipt = registry
                        .receipt_by_event(&event)
                        .ok_or_else(|| invalid("typed day 缺少注销 movement"))?;
                    if receipt.request.day != day {
                        return Err(invalid("注销 movement 日期不一致"));
                    }
                    let issued = registry
                        .issued_shares()
                        .checked_add(inventory)
                        .ok_or_else(|| invalid("注销前股本溢出"))?;
                    book.record_cancellation(day, inventory, &[])
                        .map_err(checked)?;
                    system
                        .record_issuer_repurchase_cancellation(
                            &plan.issuer,
                            &plan.event_id,
                            day,
                            inventory,
                            issued,
                        )
                        .map_err(checked)?;
                    ops.entry(group)
                        .or_default()
                        .push(AdminOperation::ShareReduction {
                            local,
                            stock,
                            qty: u32::try_from(inventory).map_err(checked)?,
                        });
                }
            }
            let mut projection = baseline.repurchase[&binding.plan_id].0.clone();
            let (issuance, restriction, announced) = self.repurchase_projection_eligibility(
                &plan,
                binding.closing_state,
                crate::CivilInstant::from_hms(day, 18, 0, 0).map_err(checked)?,
            )?;
            projection.issuance_in_progress = issuance;
            projection.material_restriction = restriction;
            projection.announced = announced;
            projection.eligibility_revision = projection
                .eligibility_revision
                .checked_add(1)
                .ok_or_else(|| invalid("资格 revision 溢出"))?;
            projection.eligibility_as_of =
                crate::CivilInstant::from_hms(day, 18, 0, 0).map_err(checked)?;
            projection.book_through_cursor = binding.applied_fact_cursor;
            projection.book_gross = book
                .fills()
                .iter()
                .try_fold(Money::ZERO, |sum, fill| sum.add(fill.gross))
                .map_err(checked)?;
            projection.book_fees = book
                .fills()
                .iter()
                .try_fold(Money::ZERO, |sum, fill| sum.add(fill.fees))
                .map_err(checked)?;
            projection.inventory_shares = book.current_inventory_shares(&[]).map_err(checked)?;
            projection.recycled = book.withdrawn_remainder().unwrap_or(Money::ZERO);
            projection.last_order_day = book.last_order_day();
            rows.entry(group)
                .or_default()
                .push(RepurchaseProjectionInput {
                    local,
                    stock,
                    issuer: plan.issuer,
                    execution_plan: AdminPlanReference::Existing(binding.plan_id),
                    projection,
                });
        }
        system
            .validate_issuer_repurchase_books(&candidate.issuer_repurchases)
            .map_err(checked)?;
        *self.state.companies.system_arc_mut().expect("已校验公司源") = Arc::new(system);
        *self
            .state
            .companies
            .corporate_actions_mut()
            .expect("已校验公司源") = candidate;
        Ok((
            vec![AdminOperationStage {
                operations: ops.into_iter().collect(),
            }],
            Some(RepurchasePreparePayload {
                groups: rows.into_iter().collect(),
            }),
        ))
    }
}

pub(super) fn inspect_repurchase_sources(
    candidate: &AdminCandidate,
    baseline: &BTreeMap<
        crate::plans::PlanId,
        (
            RepurchaseExecutionProjection,
            crate::runtime::world::RepurchasePreparedSource,
        ),
    >,
) -> Result<(), SessionError> {
    #[cfg(test)]
    if REJECT_REPURCHASE_INSPECT.with(|slot| slot.replace(false)) {
        return Err(invalid("测试在真实回购 civil inspect 拒绝"));
    }
    let mut seen = BTreeSet::new();
    for group in &candidate.groups {
        if let Some(prepared) = &group.repurchase {
            for (id, source) in &prepared.sources {
                if !seen.insert(*id) {
                    return Err(invalid("同次来源回执计划重复"));
                }
                if baseline.get(id).map(|row| &row.1) != Some(source) {
                    return Err(invalid("来源读取与本次 Prepare 前缀不一致"));
                }
            }
        }
    }
    if seen.len() != baseline.len() {
        return Err(invalid("同次回购来源回执有遗漏"));
    }
    Ok(())
}

#[cfg(test)]
std::thread_local! { static REJECT_REPURCHASE_INSPECT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
pub(super) fn reject_next_inspect() {
    REJECT_REPURCHASE_INSPECT.with(|slot| assert!(!slot.replace(true)));
}

fn has_issuer_correction(
    corrections: &[CompanyReportCorrection],
    issuer: crate::company::CompanyId,
) -> bool {
    corrections
        .iter()
        .any(|correction| correction.company == issuer)
}

#[cfg(test)]
mod eligibility_tests {
    use super::*;
    #[test]
    fn pending_corrections_restrict_only_their_issuer() {
        let correction = CompanyReportCorrection {
            operation_id: "更正".into(),
            company: crate::company::CompanyId(1),
            supersedes: crate::information::PublicationId::new(1),
            entries: Vec::new(),
            reason: "更正".into(),
        };
        assert!(!has_issuer_correction(
            std::slice::from_ref(&correction),
            crate::company::CompanyId(0)
        ));
        assert!(has_issuer_correction(
            &[correction],
            crate::company::CompanyId(1)
        ));
    }
}
