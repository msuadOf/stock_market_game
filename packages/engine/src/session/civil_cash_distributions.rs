//! 现金权益候选绑定真实行政基线、原 typed operand 与同次提交后的完整账户。

use super::administrative_mirror::CivilInformationError;
use super::corporate_actions::distributions_day_end::{
    DistributionIntent, DistributionSource, DistributionsDayEndError, DistributionsDayEndInput,
    WithholdingSource,
};
use super::*;
use crate::runtime::drive::AdministrativeBaseline;
use crate::runtime::group::GroupAccount;
use crate::runtime::world::{
    AdminCandidate, AdminOperation, AdminOperationStage, PreparedAdministrativeStages,
};
use crate::runtime::{GroupIdx, LocalIdx, RuntimeError, StockIdx};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
enum CashPreparationError {
    #[error("{0}")]
    Runtime(#[from] RuntimeError),
    #[error("{0}")]
    Distribution(#[from] DistributionsDayEndError),
    #[error("{0}")]
    Company(#[from] crate::company::CompanySystemError),
}

fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: detail.into(),
    }
}

struct IntentPosition {
    intent_index: usize,
    stage: u32,
    group: GroupIdx,
    group_index: usize,
    local: LocalIdx,
    operation_index: usize,
}

pub(super) struct PreparedCivilCash {
    day: crate::CivilDate,
    intents: Vec<DistributionIntent>,
    positions: Vec<IntentPosition>,
    expected_accounts: BTreeMap<(GroupIdx, LocalIdx), GroupAccount>,
}

fn operation(intent: &DistributionIntent, local: LocalIdx) -> Result<AdminOperation, RuntimeError> {
    let (stock, amount, tax) = match intent {
        DistributionIntent::DividendGrossCredit { stock, amount, .. } => (*stock, *amount, false),
        DistributionIntent::DividendWithholding { stock, amount, .. } => (*stock, *amount, true),
        DistributionIntent::StockDistributionCredit { .. } => {
            return Err(invalid("现金付款入口不支持股份 movement"));
        }
    };
    let stock = StockIdx::try_from(stock.0).map_err(|_| invalid("现金权益证券坐标溢出"))?;
    if amount.cents() <= 0 {
        return Err(invalid("现金权益 typed operand 必须为正分额"));
    }
    Ok(if tax {
        AdminOperation::WithholdingTax {
            local,
            stock,
            amount: amount.cents(),
        }
    } else {
        AdminOperation::CashDividend {
            local,
            stock,
            amount: amount.cents(),
        }
    })
}

impl GameSession {
    /// 只安装到 civil shadow；真实账户、公司付款与税账均在同一个 Admin 成功尾发布。
    pub(super) fn prepare_civil_cash_distributions(
        &mut self,
        day: crate::CivilDate,
        baseline: &AdministrativeBaseline,
    ) -> Result<(Vec<AdminOperationStage>, PreparedCivilCash), SessionError> {
        let result: Result<_, CashPreparationError> = (|| {
            let mut owners = BTreeMap::new();
            let mut available = BTreeMap::new();
            let mut raw_accounts = BTreeMap::new();
            for (index, id) in baseline.roster.iter().enumerate() {
                if baseline.per_group == 0 {
                    return Err(invalid("现金权益真实分组宽度为零").into());
                }
                let group = GroupIdx::try_from(index / baseline.per_group)
                    .map_err(|_| invalid("现金权益 group 溢出"))?;
                let local = LocalIdx::try_from(index % baseline.per_group)
                    .map_err(|_| invalid("现金权益 local 溢出"))?;
                let raw = baseline
                    .accounts
                    .get(usize::from(group))
                    .filter(|(actual, _)| *actual == group)
                    .and_then(|(_, rows)| rows.get(local as usize))
                    .ok_or_else(|| invalid("现金权益缺真实完整组账户"))?;
                if owners.insert(*id, (group, local)).is_some() {
                    return Err(invalid("现金权益名册重复账户").into());
                }
                available.insert(*id, Money::from_cents(raw.cash));
                raw_accounts.insert((group, local), raw.clone());
            }
            let mut prepared = PreparedCivilCash {
                day,
                intents: Vec::new(),
                positions: Vec::new(),
                expected_accounts: BTreeMap::new(),
            };
            let Some(actions) = self.state.companies.corporate_actions() else {
                return Ok((Vec::new(), prepared));
            };
            let system = self
                .state
                .companies
                .company_system()
                .ok_or_else(|| invalid("现金权益缺 CompanySystem"))?;
            let mode = self
                .state
                .setup
                .dividend_tax_mode
                .ok_or_else(|| invalid("现金权益缺开局税务模式"))?;
            let candidate =
                actions.cash_distributions_day_end_candidate(DistributionsDayEndInput {
                    day,
                    calendar: self.state.civil_clock.calendar(),
                    catalog: &self.state.stocks,
                    company_system: system,
                    available_cash: &available,
                    tax_mode: mode,
                    flat_withholding_bp: self.state.setup.flat_withholding_bp,
                    capital_reserve_tax_sources: &BTreeMap::new(),
                })?;
            candidate
                .company_system
                .validate_cash_dividend_books(&candidate.actions.dividends)?;
            let mut plan_stocks = BTreeMap::new();
            for book in &candidate.actions.dividends {
                let stock = self
                    .state
                    .stocks
                    .id(&book.plan().stock.0)
                    .ok_or_else(|| invalid("现金分红方案证券不在目录"))?;
                if plan_stocks
                    .insert(book.plan().plan_id.clone(), stock)
                    .is_some()
                {
                    return Err(invalid("现金分红方案身份重复").into());
                }
            }
            let mut gross_sources = BTreeMap::new();
            for source in &candidate.sources {
                match source {
                    DistributionSource::SimpleSyntheticDividend {
                        payment_id,
                        plan_id,
                        holder,
                        stock,
                        gross,
                    } => {
                        if plan_stocks.get(plan_id) != Some(stock) {
                            return Err(invalid("税前股利原意图证券与方案不一致").into());
                        }
                        if let crate::company::share_registry::HolderId::Account(account) = holder {
                            if gross_sources
                                .insert(
                                    (payment_id.clone(), plan_id.clone(), *account, *stock),
                                    *gross,
                                )
                                .is_some()
                            {
                                return Err(invalid("现金股利公司来源重复").into());
                            }
                        }
                    }
                    DistributionSource::StockDistribution { .. } => {
                        return Err(invalid("现金候选包含送转来源").into());
                    }
                }
            }
            let mut gross_receipts = BTreeMap::new();
            for receipt in candidate
                .actions
                .account_gross_receipts
                .iter()
                .filter(|receipt| receipt.paid_on == day)
            {
                if gross_receipts
                    .insert(
                        (
                            receipt.payment_id.clone(),
                            receipt.plan_id.clone(),
                            receipt.account,
                        ),
                        receipt.gross,
                    )
                    .is_some()
                {
                    return Err(invalid("现金股利税前回执重复").into());
                }
            }
            let mut flat_receipts = BTreeMap::new();
            for receipt in candidate
                .actions
                .flat_withholding_receipts
                .iter()
                .filter(|receipt| receipt.paid_on == day && receipt.withheld.cents() > 0)
            {
                if flat_receipts
                    .insert(
                        (
                            receipt.payment_id.clone(),
                            receipt.plan_id.clone(),
                            receipt.account,
                        ),
                        receipt.withheld,
                    )
                    .is_some()
                {
                    return Err(invalid("简税代扣回执重复").into());
                }
            }
            let mut collections = BTreeMap::new();
            for book in &candidate.actions.dividend_tax_books {
                if let Some(receipt) = book
                    .collections()
                    .last()
                    .filter(|receipt| receipt.day == day && receipt.collected.cents() > 0)
                {
                    let stock = self
                        .state
                        .stocks
                        .id(&book.stock().0)
                        .ok_or_else(|| invalid("个人收缴证券不在目录"))?;
                    if collections
                        .insert(
                            (book.account(), stock, receipt.event_id.clone()),
                            receipt.collected,
                        )
                        .is_some()
                    {
                        return Err(invalid("个人收缴回执重复").into());
                    }
                }
            }
            let mut stages: Vec<AdminOperationStage> = Vec::new();
            let mut flat_offsets = BTreeMap::<GroupIdx, usize>::new();
            let mut prior_tax = None;
            for (intent_index, intent) in candidate.intents.iter().enumerate() {
                let (account, is_tax) = match intent {
                    DistributionIntent::DividendGrossCredit {
                        account,
                        payment_id,
                        plan_id,
                        stock,
                        amount,
                    } => {
                        if gross_sources.remove(&(
                            payment_id.clone(),
                            plan_id.clone(),
                            *account,
                            *stock,
                        )) != Some(*amount)
                            || gross_receipts.remove(&(
                                payment_id.clone(),
                                plan_id.clone(),
                                *account,
                            )) != Some(*amount)
                        {
                            return Err(invalid("现金股利缺唯一同源公司来源或税前回执").into());
                        }
                        (*account, false)
                    }
                    DistributionIntent::DividendWithholding {
                        account,
                        stock,
                        amount,
                        source,
                    } => {
                        let actual = match source {
                            WithholdingSource::FlatPayment {
                                payment_id,
                                plan_id,
                            } => {
                                if plan_stocks.get(plan_id) != Some(stock) {
                                    return Err(invalid("简税原意图证券与付款方案不一致").into());
                                }
                                flat_receipts.remove(&(
                                    payment_id.clone(),
                                    plan_id.clone(),
                                    *account,
                                ))
                            }
                            WithholdingSource::IndividualCollection { collection_id } => {
                                collections.remove(&(*account, *stock, collection_id.clone()))
                            }
                        };
                        if actual != Some(*amount) {
                            return Err(invalid("扣税意图缺唯一真实候选收缴来源").into());
                        }
                        (*account, true)
                    }
                    DistributionIntent::StockDistributionCredit { .. } => {
                        return Err(invalid("现金付款候选包含股份 movement").into());
                    }
                };
                let &(group, local) = owners
                    .get(&account)
                    .ok_or_else(|| invalid("现金权益账户不在真实 Owner 名册"))?;
                let typed = operation(intent, local)?;
                // 按原意图连续段分 stage，不重排同账户的 credit／tax，保留 checked overflow 顺序。
                if prior_tax != Some(is_tax) {
                    stages.push(AdminOperationStage {
                        operations: Vec::new(),
                    });
                    prior_tax = Some(is_tax);
                }
                let stage =
                    u32::try_from(stages.len() - 1).map_err(|_| invalid("现金权益 stage 溢出"))?;
                let rows = &mut stages.last_mut().expect("刚建立当前连续段").operations;
                let group_index = match rows.iter().position(|(actual, _)| *actual == group) {
                    Some(index) => {
                        rows[index].1.push(typed);
                        index
                    }
                    None => {
                        rows.push((group, vec![typed]));
                        rows.len() - 1
                    }
                };
                let offset = flat_offsets.entry(group).or_default();
                prepared.positions.push(IntentPosition {
                    intent_index,
                    stage,
                    group,
                    group_index,
                    local,
                    operation_index: *offset,
                });
                *offset = offset
                    .checked_add(1)
                    .ok_or_else(|| invalid("现金权益 flat index 溢出"))?;
                // 未触及的真实账户沿基线保持原值，不以零值或另一账户代替。
                let raw = prepared
                    .expected_accounts
                    .entry((group, local))
                    .or_insert_with(|| raw_accounts[&(group, local)].clone());
                raw.cash = match typed {
                    AdminOperation::CashDividend { amount, .. } => raw.cash.checked_add(amount),
                    AdminOperation::WithholdingTax { amount, .. } => {
                        raw.cash.checked_sub(amount).filter(|cash| *cash >= 0)
                    }
                    _ => unreachable!("operation 只生成现金权益两种原语"),
                }
                .ok_or_else(|| invalid("现金权益完整账户预期现金溢出或不足"))?;
            }
            if !gross_sources.is_empty()
                || !gross_receipts.is_empty()
                || !flat_receipts.is_empty()
                || !collections.is_empty()
            {
                return Err(invalid("现金候选存在未被原 typed 意图消费的付款或收缴事实").into());
            }
            for (account, expected_cash) in &candidate.projected_available_cash {
                let &(group, local) = owners
                    .get(account)
                    .ok_or_else(|| invalid("现金权益候选额外账户"))?;
                let raw = prepared
                    .expected_accounts
                    .get(&(group, local))
                    .unwrap_or(&raw_accounts[&(group, local)]);
                if raw.cash != expected_cash.cents() {
                    return Err(invalid("现金权益候选与完整账户预期金额不一致").into());
                }
            }
            if candidate.projected_available_cash.len() != available.len() {
                return Err(invalid("现金权益候选漏真实账户").into());
            }
            prepared.intents = candidate.intents;
            let SessionCompanies::CompanySystem(state) = &mut self.state.companies else {
                unreachable!("候选来自同一公司容器")
            };
            state.system = Arc::new(candidate.company_system);
            state.corporate_actions = candidate.actions;
            Ok((stages, prepared))
        })();
        result.map_err(|source| {
            SessionError::CivilInformation(CivilInformationError::new(day, source))
        })
    }
}

impl PreparedCivilCash {
    pub(super) fn inspect(
        &self,
        candidate: &AdminCandidate,
        stages: Option<&PreparedAdministrativeStages<'_>>,
    ) -> Result<(), SessionError> {
        let verify = (|| {
            if self.positions.is_empty() {
                if stages.is_some() {
                    return Err(invalid("空现金权益意图不应含阶段来源"));
                }
                return Ok(());
            }
            let stages = stages.ok_or_else(|| invalid("现金权益缺同 pending 实际阶段来源"))?;
            let mut expected = BTreeMap::new();
            let mut tax = BTreeMap::new();
            for position in &self.positions {
                let typed = operation(&self.intents[position.intent_index], position.local)?;
                let key = (position.group, position.stage, position.operation_index);
                if expected.insert(key, typed).is_some() {
                    return Err(invalid("现金权益原意图坐标重复"));
                }
                if let AdminOperation::WithholdingTax {
                    local,
                    stock,
                    amount,
                } = typed
                {
                    tax.insert(key, (position.group_index, local, stock, amount));
                }
            }
            for operand in stages.operands() {
                let key = (operand.group, operand.stage_index, operand.operation_index);
                if expected.remove(&key).as_ref() != Some(operand.operation) {
                    return Err(invalid("现金权益原意图与借用的真实 typed operand 不一致"));
                }
            }
            if !expected.is_empty() {
                return Err(invalid("现金权益漏原 typed operand"));
            }
            for projection in stages.tax_projections() {
                if projection.id != candidate.id {
                    return Err(invalid("现金权益税阶段不属于同一次 Admin id"));
                }
                let stage = projection
                    .stage_index
                    .ok_or_else(|| invalid("现金权益税阶段缺 stage index"))?;
                for actual in &projection.operations {
                    if tax.remove(&(actual.group, stage, actual.operation_index))
                        != Some((
                            actual.group_index,
                            actual.local,
                            actual.stock,
                            actual.amount,
                        ))
                    {
                        return Err(invalid("现金权益收缴来源与真实税阶段投影不一致"));
                    }
                }
            }
            if !tax.is_empty() {
                return Err(invalid("现金权益遗漏实际税阶段投影"));
            }
            let mut actual = BTreeMap::new();
            for group in &candidate.groups {
                for (local, account) in &group.accounts {
                    if actual
                        .insert((group.group, *local), account.as_ref().clone())
                        .is_some()
                    {
                        return Err(invalid("现金权益最终账户重复"));
                    }
                }
            }
            if actual != self.expected_accounts {
                return Err(invalid("现金权益最终完整 GroupAccount 与原意图不一致"));
            }
            Ok(())
        })();
        verify.map_err(|source| {
            SessionError::CivilInformation(CivilInformationError::new(self.day, source))
        })
    }
}
