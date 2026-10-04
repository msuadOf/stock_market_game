//! 信息驱动更新（K5a 行 154）：触发因果类型 + `BeliefBook` 的更新语义。
//! 更新只由显式 [`BeliefCause`] 驱动——信念模块不存在任何 per-tick /
//! per-observation 变更路径。

use crate::account::StockCode;
use crate::accounting::reports::ReportKind;
use crate::accounting::AccountingPeriod;
use crate::calendar::CivilInstant;
use crate::information::{NpcObservationContext, PublicationId, PublishedReport};
use crate::orderbook::OrderId;

use super::facts::{extract_annual_facts, AnnualFacts};
use super::{
    belief_horizon_days, capability_center, estimate_by_method, initial_forecast, observe_growth,
    revise_forecast, revision_lambda_bp, ValuationOutcome, ValuationUnavailable,
};

use crate::strategy::beliefs::{BeliefBook, BeliefEntry, BeliefError, BeliefInputs};

/// 信念更新因果（唯一变更入口；携带事件/公布 id 供追溯）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BeliefCause {
    /// 新获知的定期材料：年报更新全年基准，中期以同范围同比修订增长。
    NewMaterial { report: PublicationId },
    /// 会计更正：直接重估（丢弃旧预测，绕过 λ 混合；记录公布 id）。
    Correction { report: PublicationId },
    /// 离散信用违约事件：直接重估（记录公告 id）。
    CreditDefault { announcement: PublicationId },
    /// 既有预期到期：同事实重估 + 重新锚定期限。
    HorizonExpired,
    /// 受挫经历确认：信心 −1000bp（同一订单只更新一次）。
    ExperienceFailure { order: OrderId },
    /// 真实获利退出：信心 +500bp（同一订单只更新一次）。
    ProfitableExit { order: OrderId },
}

/// 因果记录（存档面：追溯最后一次信念变更的触发事件）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CauseRecord {
    pub cause: BeliefCause,
    pub as_of_trading_day: u64,
}

/// 受挫经历的信心增量（K5a 行 154）。
pub(crate) const FAILURE_CONFIDENCE_DELTA_BP: i32 = -1_000;
/// 真实获利退出的信心增量。
pub(crate) const PROFITABLE_EXIT_CONFIDENCE_DELTA_BP: i32 = 500;
/// 信心上界（0..=10000bp——K5 行 133 的明示评分定义；饱和是定义的一部分，
/// 不是掩盖异常的 clamp）。
pub(crate) const CONFIDENCE_MAX_BP: u16 = 10_000;

pub(crate) fn own_known_report_priority(report: &PublishedReport) -> (AccountingPeriod, bool, u32) {
    (
        report.reports.period,
        matches!(
            report.reports.scope,
            crate::accounting::consolidation::ScopeId::Consolidated(_)
        ),
        report.reports.version.sequence,
    )
}

pub(crate) fn preferred_own_report(
    ctx: &NpcObservationContext<'_, impl Sized>,
    company: &crate::company::CompanyId,
) -> Result<Option<PublicationId>, BeliefError> {
    let mut best = None;
    for record in ctx
        .acquired_reports()
        .into_iter()
        .filter(|record| &record.company == company)
    {
        let report = ctx.report(record.id)?;
        let key = own_known_report_priority(report);
        if best.as_ref().is_none_or(|(_, priority)| key > *priority) {
            best = Some((record.id, key));
        }
    }
    Ok(best.map(|(id, _)| id))
}

/// 信心调整（界限内饱和）。
pub(crate) fn apply_confidence_delta(current_bp: u16, delta_bp: i32) -> u16 {
    let next = i32::from(current_bp) + delta_bp;
    u16::try_from(next.clamp(0, i32::from(CONFIDENCE_MAX_BP)))
        .expect("clamped to the 0..=10000 bp scoring range")
}

/// 距到期剩余交易日（0 = 已到期）。
pub(crate) fn horizon_remaining_trading_days(anchor: u64, horizon: u16, as_of: u64) -> u64 {
    let expiry = anchor.saturating_add(u64::from(horizon));
    expiry.saturating_sub(as_of)
}

impl BeliefBook {
    /// 经历触发：只动信心（`delta_bp` = −1000/+500、界限饱和），估值/预测
    /// 不动；同一订单只更新一次（重复触发幂等——字节不变）。
    pub(crate) fn apply_experience(
        &mut self,
        stock: &StockCode,
        cause: BeliefCause,
        order: OrderId,
        delta_bp: i32,
        as_of_trading_day: u64,
    ) -> Result<(), BeliefError> {
        let entry = self
            .entries
            .get_mut(stock)
            .ok_or(BeliefError::NoBeliefEntry)?;
        if !entry.applied_experience_orders.insert(order.0) {
            return Ok(());
        }
        entry.confidence_bp = apply_confidence_delta(entry.confidence_bp, delta_bp);
        entry.last_cause = Some(CauseRecord {
            cause,
            as_of_trading_day,
        });
        Ok(())
    }

    /// 定期材料：年报更新全年事实，中期补充同 scope 的年度基准而不年化。
    /// `direct = true`（更正）直接重估，丢弃旧预测并重置信心。
    pub(crate) fn apply_material(
        &mut self,
        stock: &StockCode,
        cause: BeliefCause,
        report_id: PublicationId,
        inputs: &BeliefInputs<'_, impl Sized>,
        direct: bool,
    ) -> Result<(), BeliefError> {
        let report = inputs.ctx.report(report_id)?;
        if matches!(
            report.reports.kind,
            ReportKind::Quarter | ReportKind::HalfYear
        ) {
            return self.apply_interim_material(stock, cause, report_id, inputs, direct);
        }
        self.ensure_annual_for(report_id, report, inputs)?;
        if !direct
            && self
                .entries
                .get(stock)
                .is_some_and(|entry| entry.used_report_ids.contains(&report_id))
        {
            return Ok(());
        }
        let observed_at = observed_at_of(inputs, report_id);
        let facts = extract_annual_facts(report, &inputs.company, observed_at)?;
        self.write_derived_entry(stock, cause, report_id, facts, inputs, direct);
        Ok(())
    }

    fn apply_interim_material(
        &mut self,
        stock: &StockCode,
        cause: BeliefCause,
        report_id: PublicationId,
        inputs: &BeliefInputs<'_, impl Sized>,
        direct: bool,
    ) -> Result<(), BeliefError> {
        let report = inputs.ctx.report(report_id)?;
        let observation = super::facts::extract_interim_growth(
            report,
            &inputs.company,
            observed_at_of(inputs, report_id),
        )?;
        let (annual_id, annual_at) = self
            .latest_own_annual_for_scope(inputs, Some(&report.reports.scope))?
            .ok_or(BeliefError::NoOwnAnnualMaterial)?;
        let facts =
            extract_annual_facts(inputs.ctx.report(annual_id)?, &inputs.company, annual_at)?;
        if !self.entries.contains_key(stock) {
            self.write_derived_entry(
                stock,
                cause.clone(),
                annual_id,
                facts.clone(),
                inputs,
                false,
            );
        }
        let entry = self.entries.get(stock).ok_or(BeliefError::NoBeliefEntry)?;
        if entry.used_report_ids.contains(&report_id) {
            return Ok(());
        }
        let forecast = if direct {
            initial_forecast(observation, self.assumptions.growth_deviation_bp)
        } else {
            revise_forecast(
                &entry.forecast,
                observation,
                revision_lambda_bp(capability_center(&self.profile)),
                self.assumptions.growth_deviation_bp,
            )
        };
        let valuation = self.valuation_for(&facts, forecast.growth_bp, inputs);
        let entry = self
            .entries
            .get_mut(stock)
            .ok_or(BeliefError::NoBeliefEntry)?;
        entry.forecast = forecast;
        if direct {
            entry.confidence_bp = forecast.initial_confidence_bp();
        }
        entry.valuation = valuation;
        entry.used_report_ids = vec![annual_id, report_id];
        entry.anchor_trading_day = inputs.as_of_trading_day;
        entry.last_cause = Some(CauseRecord {
            cause,
            as_of_trading_day: inputs.as_of_trading_day,
        });
        Ok(())
    }

    /// 信用违约：公告须属本公司；以本人已知**最新年报**直接重估。
    pub(crate) fn apply_credit_default(
        &mut self,
        stock: &StockCode,
        cause: BeliefCause,
        announcement_id: PublicationId,
        inputs: &BeliefInputs<'_, impl Sized>,
    ) -> Result<(), BeliefError> {
        let announcement = inputs.ctx.announcement(announcement_id)?;
        if announcement.company != inputs.company {
            return Err(BeliefError::MaterialNotForCompany {
                report: announcement_id,
                company: inputs.company.clone(),
            });
        }
        let (report_id, observed_at) = self
            .latest_own_annual(inputs)?
            .ok_or(BeliefError::NoOwnAnnualMaterial)?;
        let report = inputs.ctx.report(report_id)?;
        let facts = extract_annual_facts(report, &inputs.company, observed_at)?;
        self.write_derived_entry(stock, cause, report_id, facts, inputs, true);
        Ok(())
    }

    /// 到期：同事实重估（预测/信心不动），锚推进到当前日。
    pub(crate) fn apply_horizon_expiry(
        &mut self,
        stock: &StockCode,
        cause: BeliefCause,
        inputs: &BeliefInputs<'_, impl Sized>,
    ) -> Result<(), BeliefError> {
        let entry = self.entries.get(stock).ok_or(BeliefError::NoBeliefEntry)?;
        let remaining = horizon_remaining_trading_days(
            entry.anchor_trading_day,
            entry.horizon_trading_days,
            inputs.as_of_trading_day,
        );
        if remaining > 0 {
            return Err(BeliefError::HorizonNotElapsed {
                remaining_trading_days: remaining,
            });
        }
        let baseline_scope = entry
            .used_report_ids
            .first()
            .map(|report| {
                inputs
                    .ctx
                    .report(*report)
                    .map(|report| report.reports.scope.clone())
            })
            .transpose()?;
        let (report_id, observed_at) = self
            .latest_own_annual_for_scope(inputs, baseline_scope.as_ref())?
            .ok_or(BeliefError::NoOwnAnnualMaterial)?;
        let report = inputs.ctx.report(report_id)?;
        let facts = extract_annual_facts(report, &inputs.company, observed_at)?;
        let valuation = self.valuation_for(&facts, entry.forecast.growth_bp, inputs);
        let entry = self
            .entries
            .get_mut(stock)
            .ok_or(BeliefError::NoBeliefEntry)?;
        entry.valuation = valuation;
        if entry.used_report_ids.first() != Some(&report_id) {
            entry.used_report_ids = vec![report_id];
        }
        entry.anchor_trading_day = inputs.as_of_trading_day;
        entry.last_cause = Some(CauseRecord {
            cause,
            as_of_trading_day: inputs.as_of_trading_day,
        });
        Ok(())
    }

    /// 由事实写入条目（形成/修订/直接重估共用）。`direct` = 丢弃旧预测与
    /// 信心、按初始规则重来；否则 λ 修订且信心保持。
    fn write_derived_entry(
        &mut self,
        stock: &StockCode,
        cause: BeliefCause,
        report_id: PublicationId,
        facts: AnnualFacts,
        inputs: &BeliefInputs<'_, impl Sized>,
        direct: bool,
    ) {
        let deviation_bp = self.assumptions.growth_deviation_bp;
        let observation = observe_growth(&facts);
        let old = self.entries.get(stock);
        let forecast = match (direct, old) {
            (true, _) | (false, None) => initial_forecast(observation, deviation_bp),
            (false, Some(old)) => revise_forecast(
                &old.forecast,
                observation,
                revision_lambda_bp(capability_center(&self.profile)),
                deviation_bp,
            ),
        };
        let confidence_bp = if direct {
            forecast.initial_confidence_bp()
        } else {
            old.map(|old| old.confidence_bp)
                .unwrap_or_else(|| forecast.initial_confidence_bp())
        };
        let entry = BeliefEntry {
            company: inputs.company.clone(),
            method: self.analysis.method_for_company_kind(inputs.kind),
            forecast,
            confidence_bp,
            valuation: self.valuation_for(&facts, forecast.growth_bp, inputs),
            used_report_ids: vec![report_id],
            anchor_trading_day: inputs.as_of_trading_day,
            horizon_trading_days: belief_horizon_days(&self.profile),
            last_cause: Some(CauseRecord {
                cause,
                as_of_trading_day: inputs.as_of_trading_day,
            }),
            applied_experience_orders: old
                .map(|old| old.applied_experience_orders.clone())
                .unwrap_or_default(),
        };
        self.entries.insert(stock.clone(), entry);
    }

    /// 方法分发（任务 17 纯函数链接；`None` = 零基本权重 ⇒ 类型化
    /// MethodDisabled，不代换其他模型）。
    fn valuation_for(
        &self,
        facts: &AnnualFacts,
        growth_bp: Option<i32>,
        inputs: &BeliefInputs<'_, impl Sized>,
    ) -> ValuationOutcome {
        match self.analysis.method_for_company_kind(inputs.kind) {
            None => ValuationOutcome::Unavailable {
                reason: ValuationUnavailable::MethodDisabled,
            },
            Some(method) => estimate_by_method(
                method,
                facts,
                &self.assumptions,
                growth_bp,
                inputs.total_issued_shares,
            ),
        }
    }

    fn ensure_annual_for(
        &self,
        report_id: PublicationId,
        report: &PublishedReport,
        inputs: &BeliefInputs<'_, impl Sized>,
    ) -> Result<(), BeliefError> {
        if report.company != inputs.company {
            return Err(BeliefError::MaterialNotForCompany {
                report: report_id,
                company: inputs.company.clone(),
            });
        }
        if report.reports.kind != ReportKind::Annual {
            return Err(BeliefError::MaterialNotAnnual {
                report: report_id,
                kind: report.reports.kind,
            });
        }
        Ok(())
    }

    /// 本人已知最新年报（期间序，同期取最高版本序——更正版本优先）。
    fn latest_own_annual(
        &self,
        inputs: &BeliefInputs<'_, impl Sized>,
    ) -> Result<Option<(PublicationId, CivilInstant)>, BeliefError> {
        self.latest_own_annual_for_scope(inputs, None)
    }

    fn latest_own_annual_for_scope(
        &self,
        inputs: &BeliefInputs<'_, impl Sized>,
        scope: Option<&crate::accounting::consolidation::ScopeId>,
    ) -> Result<Option<(PublicationId, CivilInstant)>, BeliefError> {
        let mut best: Option<(PublicationId, CivilInstant, (AccountingPeriod, bool, u32))> = None;
        for entry in inputs.ctx.acquired_reports() {
            if entry.company != inputs.company {
                continue;
            }
            let report = inputs.ctx.report(entry.id)?;
            if report.reports.kind != ReportKind::Annual {
                continue;
            }
            if scope.is_some_and(|scope| scope != &report.reports.scope) {
                continue;
            }
            let key = own_known_report_priority(report);
            if best.as_ref().is_none_or(|(_, _, best_key)| key > *best_key) {
                best = Some((entry.id, entry.observed_at, key));
            }
        }
        Ok(best.map(|(id, observed_at, _)| (id, observed_at)))
    }
}

/// 材料的首次获知时点（上下文按获知钉死版本——`ctx.report` 已成功 ⇒
/// 条目必在，expect 是已证不变量）。
fn observed_at_of(inputs: &BeliefInputs<'_, impl Sized>, id: PublicationId) -> CivilInstant {
    inputs
        .ctx
        .acquired_reports()
        .into_iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.observed_at)
        .expect("ctx.report success implies the acquisition entry exists")
}
