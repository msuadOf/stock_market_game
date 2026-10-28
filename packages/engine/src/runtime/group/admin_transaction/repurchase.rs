//! 仅在原行政准备中构造计划与执行读面，成功前不修改组权威。
use super::*;
use crate::runtime::protocol::{AdminPlanReference, CreatedRepurchasePlan, RepurchaseExecutionProjection, RepurchaseProjectionInput};

pub(super) struct PendingRepurchase {
    pub before_book: crate::plans::PlanBook,
    pub before_dirty: bool,
    pub before_projections: BTreeMap<crate::plans::PlanId, RepurchaseExecutionProjection>,
    pub created: Vec<CreatedRepurchasePlan>,
    pub projected_ids: Vec<crate::plans::PlanId>,
}

pub(super) struct RepurchaseWorking {
    pub book: crate::plans::PlanBook,
    pub projections: BTreeMap<crate::plans::PlanId, RepurchaseExecutionProjection>,
    pub created: Vec<CreatedRepurchasePlan>,
    pub operations: Vec<AdminOperation>,
    pub projected_ids: Vec<crate::plans::PlanId>,
}

impl GroupState {
    pub(in crate::runtime::group) fn hash_repurchase_projections(&self, mut hash: u64) -> u64 {
        fn date(date: crate::calendar::CivilDate) -> u64 { let (year, month, day) = date.ymd(); ((year as u64) << 16) | (u64::from(month) << 8) | u64::from(day) }
        for (id, projection) in &self.repurchase_projections {
            let purpose = match projection.purpose { crate::company::issuer_repurchase::RepurchasePurpose::ReduceCapital => 0,
                crate::company::issuer_repurchase::RepurchasePurpose::EmployeeIncentive => 1,
                crate::company::issuer_repurchase::RepurchasePurpose::ConvertibleConversion => 2,
                crate::company::issuer_repurchase::RepurchasePurpose::ValueMaintenance => 3 };
            let restriction = match projection.material_restriction { crate::runtime::protocol::RepurchaseMaterialRestriction::Clear => 0,
                crate::runtime::protocol::RepurchaseMaterialRestriction::Restricted => 1,
                crate::runtime::protocol::RepurchaseMaterialRestriction::NotModelled => 2 };
            for value in [id.owner.0, id.sequence, projection.account.0, u64::from(projection.issuer.0), u64::from(projection.stock),
                projection.announced as u64, date(projection.announced_on), date(projection.window_start_on), date(projection.window_deadline_on),
                matches!(projection.exchange, crate::calendar::CalendarExchange::Szse) as u64, purpose,
                projection.funding_amount.cents() as u64, projection.price_cap.cents() as u64, projection.max_shares,
                date(projection.unit_as_of), projection.unit_revision, projection.book_through_cursor, projection.book_gross.cents() as u64,
                projection.book_fees.cents() as u64, projection.recycled.cents() as u64, projection.inventory_shares,
                projection.last_order_day.is_some() as u64, projection.last_order_day.map_or(0, date), projection.eligibility_revision,
                date(projection.eligibility_as_of.date()), u64::from(projection.eligibility_as_of.second_of_day()), projection.issuance_in_progress.is_some() as u64, (projection.issuance_in_progress == Some(true)) as u64, restriction] {
                hash = fnv(hash, value);
            }
        }
        hash
    }

    fn treasury_account(&self, local: LocalIdx, stock: StockIdx, issuer: crate::company::CompanyId) -> Result<crate::AccountId, RuntimeError> {
        let root = self.root_projection.as_ref().ok_or_else(|| invalid("回购 binding 缺少实际名册"))?;
        let index = self.roster_offset.checked_add(local as usize).ok_or_else(|| invalid("回购 binding 名册坐标溢出"))?;
        let account = root.account_ids.get(index).copied().ok_or_else(|| invalid("回购 binding local 为虚位或越界"))?;
        let assets = self.accounts.get(local as usize).ok_or_else(|| invalid("回购 binding 账户越界"))?;
        if stock as usize >= assets.shares.len() { return Err(invalid("回购 binding stock 越界")); }
        let mut bindings = self.admin_treasury_bindings.iter().filter(|binding| binding.owner == (self.group, local) && binding.stock == stock);
        let binding = bindings.next().ok_or_else(|| invalid("回购缺少真实专户 binding"))?;
        if bindings.next().is_some() || binding.account != account || binding.issuer != issuer {
            return Err(invalid("回购 binding account/issuer 错配或重复"));
        }
        Ok(account)
    }

    fn resolve_repurchase_reference(&self, reference: AdminPlanReference, before_index: Option<usize>, created: &[CreatedRepurchasePlan]) -> Result<crate::plans::PlanId, RuntimeError> {
        match reference {
            AdminPlanReference::Existing(id) => {
                self.plan_book.entry(id).map_err(|_| invalid("Existing 必须引用原 book 的真实计划"))?;
                Ok(id)
            }
            AdminPlanReference::CreatedInThisPrepare { operation_index } => {
                if before_index.is_some_and(|index| operation_index as usize >= index) {
                    return Err(invalid("Credit 必须引用更早的 Open"));
                }
                created.iter().find(|row| row.operation_index == operation_index).map(|row| row.plan_id)
                    .ok_or_else(|| invalid("Created 引用不在原 operations 的 Open 位置"))
            }
        }
    }

    pub(super) fn prepare_repurchase_working(&self, operations: &[AdminOperation], rows: &[RepurchaseProjectionInput]) -> Result<RepurchaseWorking, RuntimeError> {
        let mut working = RepurchaseWorking { book: self.plan_book.clone(), projections: self.repurchase_projections.clone(), created: Vec::new(), operations: Vec::with_capacity(operations.len()), projected_ids: Vec::new() };
        let mut credits = BTreeMap::new();
        for (index, operation) in operations.iter().copied().enumerate() {
            let operation = match operation {
                AdminOperation::OpenIssuerRepurchase { local, stock, issuer } => {
                    let account = self.treasury_account(local, stock, issuer)?;
                    let plan_id = working.book.create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen { owner: account, issuer, stock: crate::StockId(u32::from(stock)) })
                        .map_err(|error| invalid(format!("创建回购活动计划失败：{error}")))?;
                    working.created.push(CreatedRepurchasePlan { group: self.group, operation_index: u32::try_from(index).map_err(|_| invalid("Open 位置超过 u32"))?, local, stock, issuer, plan_id });
                    working.operations.push(operation);
                    continue;
                }
                AdminOperation::SyntheticBudgetCredit { local, stock, execution_plan, amount } => {
                    let id = self.resolve_repurchase_reference(execution_plan, Some(index), &working.created)?;
                    if let AdminPlanReference::CreatedInThisPrepare { .. } = execution_plan {
                        let open = working.created.iter().find(|row| row.plan_id == id).expect("已解析创建身份");
                        if (open.local, open.stock) != (local, stock) { return Err(invalid("Credit 与 Open 坐标不一致")); }
                    }
                    if amount <= 0 { return Err(invalid("Credit 金额必须为正分额")); }
                    if credits.insert(id, amount).is_some() { return Err(invalid("同一计划重复 Credit")); }
                    AdminOperation::SyntheticBudgetCredit { local, stock, execution_plan: AdminPlanReference::Existing(id), amount }
                }
                AdminOperation::SyntheticBudgetRecycle { execution_plan, .. } if working.created.iter().any(|row| row.plan_id == execution_plan) => {
                    return Err(invalid("回购禁止新建即回收"));
                }
                _ => operation,
            };
            self.validate_repurchase_funds_in_book(operation, &working.book)?;
            working.operations.push(operation);
        }
        for open in &working.created {
            if !credits.contains_key(&open.plan_id) { return Err(invalid("每个 Open 必须恰有一次正额 Credit")); }
        }
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            let id = self.resolve_repurchase_reference(row.execution_plan, None, &working.created)?;
            let account = self.treasury_account(row.local, row.stock, row.issuer)
                .map_err(|error| invalid(format!("执行投影身份无效：{error}")))?;
            let entry = working.book.entry(id).map_err(|_| invalid("执行投影缺少计划"))?;
            let crate::plans::PlanEntry::IssuerRepurchase(plan) = entry else { return Err(invalid("执行投影身份必须为公司计划")); };
            if id.owner != account || plan.issuer() != row.issuer || plan.stock() != crate::StockId(u32::from(row.stock)) || entry.is_terminal()
                || row.projection.account != account || row.projection.issuer != row.issuer || row.projection.stock != row.stock {
                return Err(invalid("执行投影身份与真实计划、坐标不一致"));
            }
            if let AdminPlanReference::CreatedInThisPrepare { .. } = row.execution_plan {
                let open = working.created.iter().find(|open| open.plan_id == id).expect("已解析创建身份");
                if (open.local, open.stock, open.issuer) != (row.local, row.stock, row.issuer) { return Err(invalid("执行投影身份与 Open 不一致")); }
            }
            if !seen.insert(id) { return Err(invalid("重复执行投影")); }
            let projection = &row.projection;
            let is_new = working.created.iter().any(|open| open.plan_id == id);
            if is_new && (projection.book_through_cursor != 0 || projection.book_gross != crate::Money::ZERO || projection.book_fees != crate::Money::ZERO
                || projection.recycled != crate::Money::ZERO || projection.inventory_shares != 0 || projection.last_order_day.is_some()) {
                return Err(invalid("新建执行投影的初始基线必须为空"));
            }
            if projection.funding_amount.cents() <= 0 || credits.get(&id).is_some_and(|amount| *amount != projection.funding_amount.cents()) {
                return Err(invalid("执行投影额度与正额 Credit 不一致"));
            }
            if projection.price_cap.cents() <= 0 { return Err(invalid("执行投影 price_cap 必须为正")); }
            if projection.max_shares == 0 { return Err(invalid("执行投影 max_shares 必须为正")); }
            if projection.window_start_on > projection.window_deadline_on || projection.announced_on > projection.window_start_on {
                return Err(invalid("执行投影日期窗口不一致"));
            }
            if projection.book_gross.cents() < 0 || projection.book_fees.cents() < 0 || projection.recycled.cents() < 0
                || projection.book_gross.cents().checked_add(projection.book_fees.cents()).and_then(|sum| sum.checked_add(projection.recycled.cents())).is_none_or(|sum| sum > projection.funding_amount.cents()) {
                return Err(invalid("执行投影已消费基线超出额度或溢出"));
            }
            if let Some(before) = self.repurchase_projections.get(&id) {
                validate_projection_update(before, projection)?;
            }
            working.projections.insert(id, projection.clone());
            working.projected_ids.push(id);
        }
        if working.created.iter().any(|open| !seen.contains(&open.plan_id)) || credits.keys().any(|id| !seen.contains(id)) {
            return Err(invalid("缺少执行投影"));
        }
        Ok(working)
    }
}

fn validate_projection_update(before: &RepurchaseExecutionProjection, after: &RepurchaseExecutionProjection) -> Result<(), RuntimeError> {
    if before.account != after.account || before.issuer != after.issuer || before.stock != after.stock || before.funding_amount != after.funding_amount
        || before.announced_on != after.announced_on || before.window_start_on != after.window_start_on || before.window_deadline_on != after.window_deadline_on
        || before.exchange != after.exchange || before.purpose != after.purpose {
        return Err(invalid("执行投影不可变方案发生变化"));
    }
    if after.book_through_cursor < before.book_through_cursor { return Err(invalid("执行投影 cursor 倒退")); }
    if after.eligibility_revision < before.eligibility_revision || after.unit_revision < before.unit_revision { return Err(invalid("执行投影 revision 倒退")); }
    if after.eligibility_as_of < before.eligibility_as_of || after.unit_as_of < before.unit_as_of { return Err(invalid("执行投影 as_of 倒退")); }
    if after.eligibility_revision == before.eligibility_revision && (after.eligibility_as_of != before.eligibility_as_of
        || after.issuance_in_progress != before.issuance_in_progress || after.material_restriction != before.material_restriction) {
        return Err(invalid("执行资格同 revision 不得改变来源事实"));
    }
    if (!after.announced && before.announced) || (after.unit_revision == before.unit_revision && (after.price_cap != before.price_cap || after.max_shares != before.max_shares)) {
        return Err(invalid("执行投影不可变单位方案或公告发生倒退"));
    }
    if after.book_gross < before.book_gross || after.book_fees < before.book_fees || after.recycled < before.recycled
        || (after.book_through_cursor == before.book_through_cursor && (after.book_gross != before.book_gross || after.book_fees != before.book_fees || after.recycled != before.recycled || after.inventory_shares != before.inventory_shares || after.last_order_day != before.last_order_day)) {
        return Err(invalid("执行投影 cursor 与已消费基线不一致"));
    }
    Ok(())
}
