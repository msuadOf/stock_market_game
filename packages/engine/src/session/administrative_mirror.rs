//! 同次真实行政候选的专用镜像；不消费 tick ingress、回执或同步覆盖表。

use super::civil_information::PreparedCivilFields;
use super::*;
use crate::runtime::drive::{AdministrativeBaseline, RuntimeDriver};
use crate::runtime::world::{
    AdminCandidate, AdminOperation, AdminOperationStage, InformationPreparePayload,
    PreparedAdministrativeSource, PreparedAdministrativeStages,
};
use crate::runtime::{GroupIdx, RuntimeError};
use std::sync::Arc;

/// 自然日 INFO 事务失败；保留真实 cause，内部 runtime 协议不作为公开 API。
#[derive(Debug, thiserror::Error)]
#[error("自然日 INFO 事务失败：date={date}，{source}")]
pub struct CivilInformationError {
    date: crate::calendar::CivilDate,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl CivilInformationError {
    pub(super) fn new(
        date: crate::calendar::CivilDate,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            date,
            source: Box::new(source),
        }
    }

    fn from_mirror(date: crate::calendar::CivilDate, source: AdministrativeMirrorError) -> Self {
        match source {
            AdministrativeMirrorError::Runtime(source) => Self::new(date, source),
            source => Self {
                date,
                source: Box::new(source),
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AdministrativeMirrorError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Session(#[from] StepFatal),
    #[error(
        "行政入口 civil_date 不一致：Session={expected}，World={actual}；请先完成原自然日同步"
    )]
    CivilDateMismatch {
        expected: crate::calendar::CivilDate,
        actual: crate::calendar::CivilDate,
    },
}

fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: detail.into(),
    }
}

/// 全部可失败投影在首 Commit 前完成；成功尾只移动已验证的值。
struct PreparedAdministrativeMirror {
    accounts: super::account_book::AccountBook,
    markets: Option<BTreeMap<crate::StockId, crate::market::Market>>,
    personals: Vec<(usize, AccountId, SharedRuntimePersonal)>,
    personal_maps: Option<AdministrativePersonalMaps>,
    library: Option<Arc<crate::information::PublicLibrary>>,
    plans: Option<crate::plans::PlanBook>,
    revision: u64,
    install_sequence: u64,
    changed: bool,
    #[cfg(feature = "simulation-diagnostics")]
    causal: crate::diagnostics::causal::CausalCollector,
}

struct AdministrativePersonalMaps {
    attention: AccountPagedMap<NpcAttentionState>,
    beliefs: AccountPagedMap<BeliefParticipantState>,
    histories: AccountPagedMap<crate::experience::PersonalHistoryReadLedger>,
    retail: AccountPagedMap<RetailExperienceState>,
}

/// 仅在同次 prepare callback 内借用；不构造第二份 authority 或持久 token。
pub(super) struct PreparedAdministrativeProjection<'a> {
    original: &'a GameSession,
    prepared: &'a PreparedAdministrativeMirror,
}

impl PreparedAdministrativeProjection<'_> {
    /// clone 和页分离均在首 Commit 前；None 域显式重置为原 Session 同版值。
    pub(super) fn clone_into_civil_shadow(&self, shadow: &mut GameSession) {
        shadow.state.accounts = self.prepared.accounts.clone();
        if let Some(markets) = &self.prepared.markets {
            shadow.state.markets = markets.clone();
        }
        let (attention, beliefs, histories, retail) = match &self.prepared.personal_maps {
            Some(maps) => (
                &maps.attention,
                &maps.beliefs,
                &maps.histories,
                &maps.retail,
            ),
            None => (
                &self.original.state.npc_attention,
                &self.original.state.belief_participants,
                &self.original.state.history_reads,
                &self.original.state.retail_experience,
            ),
        };
        shadow.state.npc_attention = attention.clone();
        shadow.state.belief_participants = beliefs.clone();
        shadow.state.history_reads = histories.clone();
        shadow.state.retail_experience = retail.clone();
        shadow.state.runtime_heap_dirty = true;
        #[cfg(feature = "simulation-diagnostics")]
        {
            shadow.state.causal = self.prepared.causal.clone();
        }
        shadow.state.library = Arc::clone(
            self.prepared
                .library
                .as_ref()
                .unwrap_or(&self.original.state.library),
        );
        shadow.state.plans = self
            .prepared
            .plans
            .as_ref()
            .unwrap_or(&self.original.state.plans)
            .clone();
        shadow
            .revision
            .store(self.prepared.revision, std::sync::atomic::Ordering::Relaxed);
        shadow.runtime_mirror_install_sequence = self.prepared.install_sequence;
    }
}

// 仅供 World 沿既有错误通道 Abort；原 SessionError 保存在同一调用栈中。
const CIVIL_PREPARATION_REJECTED: &str = "civil INFO precommit callback 已拒绝候选";

impl AdministrativeBaseline {
    /// 保留个人态其余字段，只叠同源真实 scheduler 两字段；cache 仍保原 raw 候选。
    fn attention_at(
        &self,
        id: AccountId,
        raw: &NpcAttentionState,
    ) -> Result<NpcAttentionState, RuntimeError> {
        let &(due, seed) = self
            .attention
            .get(&id)
            .ok_or_else(|| invalid(format!("行政完整调度缺账户：{id:?}")))?;
        let mut attention = raw.clone();
        attention.next_attention_candidate_tick = due;
        attention.rng_state = seed;
        Ok(attention)
    }

    fn account_id(
        &self,
        group: GroupIdx,
        local: crate::runtime::LocalIdx,
    ) -> Result<AccountId, RuntimeError> {
        if local as usize >= self.per_group {
            return Err(invalid("行政候选 local 超出真实分组宽度"));
        }
        let index = usize::from(group)
            .checked_mul(self.per_group)
            .and_then(|start| start.checked_add(local as usize))
            .ok_or_else(|| invalid("行政候选 Owner 坐标溢出"))?;
        self.roster
            .get(index)
            .copied()
            .ok_or_else(|| invalid(format!("行政候选没有真实账户：group={group} local={local}")))
    }
}

impl GameSession {
    /// 付款候选与行政镜像共用这一份真实完整基线；首 Prepare 前还须核来源未变。
    pub(super) fn prepare_civil_administrative_baseline(
        &self,
    ) -> Result<AdministrativeBaseline, SessionError> {
        let baseline = {
            let cell = self.runtime_driver.as_ref().ok_or_else(|| {
                SessionError::CivilInformation(CivilInformationError::new(
                    self.civil_date(),
                    invalid("civil 基线前尚未接管 runtime"),
                ))
            })?;
            let mut driver = cell.lock().map_err(|_| {
                SessionError::CivilInformation(CivilInformationError::new(
                    self.civil_date(),
                    invalid("civil 基线 driver 锁被 poison"),
                ))
            })?;
            driver
                .prepare_administrative_baseline(self)
                .map_err(|source| {
                    SessionError::CivilInformation(CivilInformationError::new(
                        self.civil_date(),
                        source,
                    ))
                })?
        };
        self.validate_administrative_baseline(&baseline)
            .map_err(|source| {
                SessionError::CivilInformation(CivilInformationError::new(
                    self.civil_date(),
                    source,
                ))
            })?;
        Ok(baseline)
    }

    /// 短借用只持 driver 锁；不进入普通 tick 的 freeze、take 或 inject 玩家意图路径。
    #[cfg(test)]
    pub(in crate::session) fn apply_administrative_operations<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<InformationPreparePayload>,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, AdministrativeMirrorError> {
        self.apply_administrative_scope(grouped, information, None, |candidate, _projection| {
            inspect(candidate).map(|value| (None, value))
        })
    }

    pub(in crate::session) fn apply_repurchase_administrative_operations<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<InformationPreparePayload>,
        repurchase: crate::runtime::RepurchasePreparePayload,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, AdministrativeMirrorError> {
        self.apply_administrative_scope(
            grouped,
            information,
            Some(repurchase),
            |candidate, _projection| inspect(candidate).map(|value| (None, value)),
        )
    }

    pub(in crate::session) fn apply_civil_information_administrative_operations<P>(
        &mut self,
        baseline: AdministrativeBaseline,
        stages: Vec<AdminOperationStage>,
        information: InformationPreparePayload,
        desks: Vec<crate::runtime::DeskExRightsRequest>,
        repurchase: Option<crate::runtime::RepurchasePreparePayload>,
        prepare: impl FnOnce(
            &AdminCandidate,
            &PreparedAdministrativeProjection<'_>,
            Option<&PreparedAdministrativeStages<'_>>,
        ) -> Result<(PreparedCivilFields, P), SessionError>,
    ) -> Result<P, SessionError> {
        let date = self.civil_date();
        let mut original_error = None;
        let has_repurchase = repurchase.is_some();
        let result = self.apply_prepared_administrative_scope(
            Vec::new(),
            Some(information),
            repurchase,
            Some(baseline),
            (
                (!stages.is_empty() || !desks.is_empty() || has_repurchase).then_some(stages),
                desks,
            ),
            |candidate, projection, stages| match prepare(candidate, projection, stages) {
                Ok((fields, value)) => Ok((Some(fields), value)),
                Err(error) => {
                    original_error = Some(error);
                    Err(invalid(CIVIL_PREPARATION_REJECTED))
                }
            },
        );
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                // Abort／通信 terminal 保首错，不能被较早的业务拒绝遮蔽。
                if let Some(fatal) = self.poison_reason() {
                    return Err(SessionError::Step(fatal.clone()));
                }
                if let AdministrativeMirrorError::Session(fatal) = error {
                    return Err(SessionError::Step(fatal));
                }
                // callback 失败后唯一余路为 World Abort；健康返回才取回原值，不解析文本。
                if let Some(original) = original_error {
                    return Err(original);
                }
                Err(SessionError::CivilInformation(
                    CivilInformationError::from_mirror(date, error),
                ))
            }
        }
    }

    fn apply_administrative_scope<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<InformationPreparePayload>,
        repurchase: Option<crate::runtime::RepurchasePreparePayload>,
        prepare: impl FnOnce(
            &AdminCandidate,
            &PreparedAdministrativeProjection<'_>,
        ) -> Result<(Option<PreparedCivilFields>, P), RuntimeError>,
    ) -> Result<P, AdministrativeMirrorError> {
        self.apply_prepared_administrative_scope(
            grouped,
            information,
            repurchase,
            None,
            (None, Vec::new()),
            |candidate, projection, _| prepare(candidate, projection),
        )
    }

    fn apply_prepared_administrative_scope<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<InformationPreparePayload>,
        repurchase: Option<crate::runtime::RepurchasePreparePayload>,
        baseline: Option<AdministrativeBaseline>,
        staged_desks: (
            Option<Vec<AdminOperationStage>>,
            Vec<crate::runtime::DeskExRightsRequest>,
        ),
        prepare: impl FnOnce(
            &AdminCandidate,
            &PreparedAdministrativeProjection<'_>,
            Option<&PreparedAdministrativeStages<'_>>,
        ) -> Result<(Option<PreparedCivilFields>, P), RuntimeError>,
    ) -> Result<P, AdministrativeMirrorError> {
        let (stages, desks) = staged_desks;
        if stages.is_some() && !grouped.is_empty() {
            return Err(invalid("有序现金阶段不得混入另一份 flat 请求").into());
        }
        self.require_healthy()?;
        if self.runtime_driver.is_none() {
            self.construct_runtime_driver_lazy()?;
        }
        let cell = self
            .runtime_driver
            .take()
            .expect("行政入口已完成 lazy handover");
        // catch 只保证锁单元与原 panic 一起归还；不把 panic 转成可恢复业务拒绝。
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut driver = cell.lock().map_err(|_| {
                self.poison_runtime_step(StepFatal::InvariantViolation {
                    location: "GameSession::apply_administrative_operations".into(),
                    description: "行政入口的常驻 driver 锁被 poison".into(),
                })
            })?;
            if driver.world_committed_tick() != self.tick()
                || driver.mirror_source().authority_epoch != self.runtime_authority_epoch
            {
                return Err(AdministrativeMirrorError::Session(
                    self.poison_runtime_step(StepFatal::InvariantViolation {
                        location: "GameSession::apply_administrative_operations".into(),
                        description: "行政入口 World 与 Session tick/authority 不一致".into(),
                    }),
                ));
            }
            let world_civil_date = driver.mirror_source().civil_date;
            if !driver.administrative_world_is_terminal() && world_civil_date != self.civil_date() {
                return Err(AdministrativeMirrorError::CivilDateMismatch {
                    expected: self.civil_date(),
                    actual: world_civil_date,
                });
            }
            let baseline_result = match baseline {
                Some(baseline) if driver.mirror_source() == baseline.source => Ok(baseline),
                Some(_) => Err(invalid("civil 付款候选后的实际行政来源已改变")),
                None => driver.prepare_administrative_baseline(self),
            };
            let mut baseline = match baseline_result {
                Ok(value) => value,
                Err(error) => {
                    self.retain_administrative_terminal(&driver, &error);
                    return Err(error.into());
                }
            };
            self.validate_administrative_baseline(&baseline)?;
            // 完整账户与个人态已与 World 和 Session 对拍；后续仅需原信息引用。
            // 立即释放不再读取的快照，降低日终事务峰值内存。
            // 保留外层组数，供回购 PlanBook 分片使用。
            for (_, accounts) in &mut baseline.accounts {
                *accounts = Vec::new();
            }
            let baseline_information: Vec<_> = std::mem::take(&mut baseline.personals)
                .into_iter()
                .map(|(_, id, value)| (id, value.map(|snapshot| snapshot.information)))
                .collect();
            #[cfg(feature = "simulation-diagnostics")]
            let acquisition_time = information
                .as_ref()
                .and_then(|payload| payload.observation.as_ref())
                .map(|_| self.causal_static_time())
                .transpose()?;
            let revision = self.external_revision();
            let sequence = self.runtime_mirror_install_sequence;
            let expected_information = information.clone();
            let inspect =
                |candidate: &AdminCandidate,
                 source: &PreparedAdministrativeSource,
                 stages: Option<&PreparedAdministrativeStages<'_>>| {
                    let prepared = self.prepare_administrative_mirror(
                        &baseline,
                        &baseline_information,
                        candidate,
                        source,
                        expected_information.as_ref(),
                        #[cfg(feature = "simulation-diagnostics")]
                        acquisition_time,
                        revision,
                        sequence,
                    )?;
                    let projection = PreparedAdministrativeProjection {
                        original: self,
                        prepared: &prepared,
                    };
                    let (civil, value) = prepare(candidate, &projection, stages)?;
                    if self.external_revision() != revision
                        || self.runtime_mirror_install_sequence != sequence
                        || self.tick() != baseline.source.tick
                        || self.runtime_authority_epoch != baseline.source.authority_epoch
                    {
                        return Err(invalid("行政 prepare 的 Session 来源在 inspect 期间改变"));
                    }
                    Ok((value, prepared, civil))
                };
            let result = match stages {
                Some(stages) if !desks.is_empty() || repurchase.is_some() => driver
                    .with_prepared_staged_administrative_and_desks(
                        stages,
                        information,
                        desks,
                        repurchase,
                        |candidate, source, stages| inspect(candidate, source, Some(stages)),
                    ),
                Some(stages) => driver.with_prepared_staged_administrative_transaction(
                    stages,
                    information,
                    |candidate, source, stages| inspect(candidate, source, Some(stages)),
                ),
                None => driver.with_prepared_administrative_transaction(
                    grouped,
                    information,
                    repurchase,
                    |candidate, source| inspect(candidate, source, None),
                ),
            };
            let ((value, prepared, civil), ack) = match result {
                Ok(value) => value,
                Err(error) => {
                    self.retain_administrative_terminal(&driver, &error);
                    return Err(error.into());
                }
            };
            self.install_administrative_mirror(&mut driver, prepared);
            if let Some(fields) = civil {
                let (next_date, retained) = fields.install_into(self);
                if let Some(retained) = retained {
                    driver.publish_prepared_retained(retained);
                }
                driver.install_administrative_civil_date(next_date);
                driver.adopt_session_seq(self);
            }
            driver.ack_prepared_mirror(ack);
            Ok(value)
        }));
        self.runtime_driver = Some(cell);
        match outcome {
            Ok(value) => value,
            Err(payload) => {
                self.poison_runtime_step(StepFatal::InvariantViolation {
                    location: "GameSession::apply_administrative_operations".into(),
                    description: "行政运行时panic；原payload继续重抛，driver锁保留poison".into(),
                });
                std::panic::resume_unwind(payload)
            }
        }
    }

    fn retain_administrative_terminal(&mut self, driver: &RuntimeDriver, error: &RuntimeError) {
        if driver.administrative_world_is_terminal() {
            self.poison_runtime_step(StepFatal::InvariantViolation {
                location: "GameSession::apply_administrative_operations".into(),
                description: format!("行政运行时已经终止：{error}"),
            });
        }
    }

    pub(super) fn administrative_account(
        &self,
        id: AccountId,
        raw: &crate::runtime::group::GroupAccount,
    ) -> Result<Account, RuntimeError> {
        let stocks = self.state.stocks.len();
        if [
            raw.shares.len(),
            raw.pending_sell_qty.len(),
            raw.frozen_today.len(),
            raw.invested_cents.len(),
            raw.recovered_cents.len(),
        ]
        .iter()
        .any(|width| *width != stocks)
        {
            return Err(invalid(format!("行政账户股票维度错误：account={id:?}")));
        }
        let mut account = self
            .account(id)
            .ok_or_else(|| invalid(format!("行政账户不在Session名册：{id:?}")))?
            .clone();
        let mut positions = account.positions().clone();
        for stock in 0..stocks {
            let code = StockId(u32::try_from(stock).map_err(|_| invalid("行政股票下标超出u32"))?);
            let qty = raw.shares[stock]
                .checked_add(raw.pending_sell_qty[stock])
                .ok_or_else(|| invalid(format!("行政持仓溢出：account={id:?} stock={code:?}")))?;
            if raw.frozen_today[stock] > qty {
                return Err(invalid("行政T+1冻结超过真实持仓"));
            }
            if qty == 0 {
                positions.remove(&code);
            } else {
                positions.insert(
                    code,
                    crate::account::Position::from_restored_parts(
                        qty,
                        raw.frozen_today[stock],
                        raw.invested_cents[stock],
                        raw.recovered_cents[stock],
                    ),
                );
            }
        }
        let cash = raw
            .cash
            .checked_add(raw.buy_purse_held_cents)
            .ok_or_else(|| invalid("行政现金加在途占用溢出"))?;
        account.fixture_set_cash(Money::from_cents(cash));
        account.fixture_set_positions(positions);
        Ok(account)
    }

    fn validate_administrative_baseline(
        &self,
        baseline: &AdministrativeBaseline,
    ) -> Result<(), RuntimeError> {
        if baseline.source.tick != self.tick()
            || baseline.source.authority_epoch != self.runtime_authority_epoch
            || baseline.plans != self.state.plans
        {
            return Err(invalid("行政完整基线与Session来源或PlanBook不一致"));
        }
        let mut seen = BTreeSet::new();
        for (group, rows) in &baseline.accounts {
            for (local, raw) in rows.iter().enumerate() {
                let local = crate::runtime::LocalIdx::try_from(local)
                    .map_err(|_| invalid("行政基线local溢出"))?;
                // 尾部 padding 不是账户；只接受完整零值，不能忽略真实资源。
                let id = match baseline.account_id(*group, local) {
                    Ok(id) => id,
                    Err(error) => {
                        if raw
                            != &crate::runtime::group::GroupAccount::new(0, self.state.stocks.len())
                        {
                            return Err(error);
                        }
                        continue;
                    }
                };
                if !seen.insert(id) {
                    return Err(invalid("行政完整基线重复真实账户"));
                }
                let projected = self.administrative_account(id, raw)?;
                let current = self
                    .account(id)
                    .ok_or_else(|| invalid("行政基线缺Session账户"))?;
                let positions_equal = projected.positions().len() == current.positions().len()
                    && projected.positions().iter().all(|(stock, left)| {
                        current.position(stock).is_some_and(|right| {
                            left.qty() == right.qty()
                                && left.t1_locked() == right.t1_locked()
                                && left.invested_cents() == right.invested_cents()
                                && left.recovered_cents() == right.recovered_cents()
                        })
                    });
                if projected.cash() != current.cash() || !positions_equal {
                    return Err(invalid(format!("行政基线账户镜像陈旧：{id:?}")));
                }
            }
        }
        if seen.len() != baseline.roster.len() {
            return Err(invalid("行政完整基线缺少真实账户"));
        }
        for (_, id, snapshot) in &baseline.personals {
            match snapshot {
                None if !self.runtime_has_belief_participant(*id) => {}
                Some(value) => {
                    let participant = self
                        .state
                        .belief_participants
                        .get(id)
                        .ok_or_else(|| invalid("行政基线缺个人认识"))?;
                    let attention = baseline.attention_at(*id, &value.attention)?;
                    if participant.watchlist() != &value.watchlist
                        || participant.price_memory() != &value.price_memory
                        || participant.information() != &value.information
                        || participant.belief() != &value.belief
                        || self.state.npc_attention.get(id) != Some(&attention)
                        || self.state.history_reads.get(id) != Some(&value.history_reads)
                        || self.state.retail_experience.get(id) != value.retail_experience.as_ref()
                    {
                        return Err(invalid(format!("行政基线个人镜像陈旧：{id:?}")));
                    }
                }
                None => return Err(invalid(format!("行政基线错误缺失个人态：{id:?}"))),
            }
        }
        Ok(())
    }

    fn prepare_administrative_mirror(
        &self,
        baseline: &AdministrativeBaseline,
        baseline_information: &[(AccountId, Option<crate::information::NpcInformationState>)],
        candidate: &AdminCandidate,
        source: &PreparedAdministrativeSource,
        information: Option<&InformationPreparePayload>,
        #[cfg(feature = "simulation-diagnostics")] acquisition_time: Option<
            crate::diagnostics::causal::FactTime,
        >,
        revision: u64,
        sequence: u64,
    ) -> Result<PreparedAdministrativeMirror, RuntimeError> {
        if source.id() != candidate.id
            || source.before() != baseline.source
            || source.after().authority_epoch != self.runtime_authority_epoch
            || source.after().tick != self.tick()
        {
            return Err(invalid("行政候选未绑定同一次真实Prepare来源"));
        }
        let mut markets = None;
        if !candidate.desks.is_empty() {
            let mut next = self.state.markets.clone();
            let mut seen = BTreeSet::new();
            for desk in &candidate.desks {
                let stock = crate::StockId(u32::from(desk.request.stock));
                if !seen.insert(stock) {
                    return Err(invalid("行政候选重复交易部"));
                }
                let market = next
                    .get_mut(&stock)
                    .ok_or_else(|| invalid("行政交易部不在市场目录"))?;
                if market.last_close().cents() != desk.request.expected_last_close
                    || market.last_price().cents() != desk.view.last_price
                    || desk.view.last_close != desk.request.reference_price
                    || desk.request.reference_price <= 0
                {
                    return Err(invalid("交易部除权候选与 Session 同源行情不一致"));
                }
                market.restore_prices(
                    market.last_price(),
                    Money::from_cents(desk.request.reference_price),
                );
            }
            markets = Some(next);
        }
        let plans = baseline.plans_after(candidate)?;
        if source.captured().attention_full || (source.captured().plans_pending && plans.is_none())
        {
            return Err(invalid(
                "行政镜像缺同次完整PlanBook或独立attention候选；拒绝确认该域",
            ));
        }
        let changed = !source.captured().is_empty() || !candidate.desks.is_empty();
        let next_revision = if changed {
            revision
                .checked_add(1)
                .ok_or_else(|| invalid("行政Session revision已耗尽，拒绝首Commit"))?
        } else {
            revision
        };
        let install_sequence = if changed {
            sequence
                .checked_add(1)
                .ok_or_else(|| invalid("行政mirror install_sequence已耗尽，拒绝首Commit"))?
        } else {
            sequence
        };
        let mut accounts = BTreeMap::new();
        let mut personals = BTreeMap::new();
        for group in &candidate.groups {
            for (local, raw) in &group.accounts {
                let id = baseline.account_id(group.group, *local)?;
                if accounts
                    .insert(id, self.administrative_account(id, raw)?)
                    .is_some()
                {
                    return Err(invalid("行政候选重复账户"));
                }
            }
            if let Some(info) = &group.information {
                for (local, id, value) in &info.personals {
                    let expected = baseline.account_id(group.group, *local)?;
                    if expected != *id
                        || value.information.owner() != *id
                        || value.belief.npc() != *id
                    {
                        return Err(invalid("行政INFO候选属主错误"));
                    }
                    let index = usize::from(group.group) * baseline.per_group + *local as usize;
                    let mut snapshot = value.clone();
                    if let Some(group) = &snapshot.retail_experience {
                        snapshot.retail_experience =
                            Some(match self.runtime_retail_experience_for(id) {
                                Some(existing) => existing.merged_with_group_authority(group),
                                None => group.clone(),
                            });
                    }
                    if personals.insert(*id, (index, *id, snapshot)).is_some() {
                        return Err(invalid("行政INFO候选重复个人态"));
                    }
                }
            }
        }
        let library = information.map(|info| Arc::clone(&info.library));
        let reference_library = library.as_deref().unwrap_or(self.state.library.as_ref());
        for (id, old) in baseline_information {
            let current_information = personals
                .get(id)
                .map(|(_, _, value)| &value.information)
                .or(old.as_ref());
            if let Some(current_information) = current_information {
                let mut rebuilt = crate::information::NpcInformationState::new(*id);
                for (_, records) in current_information.companies() {
                    for record in records {
                        rebuilt
                            .record_acquisition(
                                *id,
                                reference_library,
                                record.id,
                                record.observed_at,
                            )
                            .map_err(|error| {
                                invalid(format!(
                                    "行政完整个人引用不属于同批库：account={id:?} {error}"
                                ))
                            })?;
                    }
                }
                if &rebuilt != current_information {
                    return Err(invalid("行政完整个人引用的company/kind与库不一致"));
                }
            }
        }
        // AccountBook 的页分离也在 Commit 前完成，成功尾不再 insert 或分配页。
        let mut account_book = self.state.accounts.clone();
        for (id, account) in accounts {
            account_book.insert(id, account);
        }
        let personals: Vec<_> = personals.into_values().collect();
        // 保持 attention 缺失先于 causal 校验的既有错误优先级。
        for (_, id, value) in &personals {
            baseline.attention_at(*id, &value.attention)?;
        }
        #[cfg(feature = "simulation-diagnostics")]
        let causal = {
            let mut causal = self.state.causal.clone();
            for (_, id, snapshot) in &personals {
                let original = self.state.belief_participants.get(id);
                for (company, records) in snapshot.information.companies() {
                    for record in records {
                        if original.is_some_and(|old| {
                            old.information().observed_at_of(record.id).is_some()
                        }) {
                            continue;
                        }
                        let through = information
                            .and_then(|payload| payload.observation.as_ref())
                            .ok_or_else(|| invalid("行政新增获知缺少同次真实 observation"))?
                            .through;
                        if record.observed_at != through {
                            return Err(invalid("行政新增获知时间不属于同次 observation"));
                        }
                        let published = match record.kind {
                            crate::information::AcquiredKind::Report => {
                                reference_library
                                    .report(record.id, through)
                                    .map_err(|error| {
                                        invalid(format!(
                                            "行政新增报告获知缺少同批公开材料：{error}"
                                        ))
                                    })?
                                    .published_at
                            }
                            crate::information::AcquiredKind::Announcement => {
                                reference_library
                                    .announcement(record.id, through)
                                    .map_err(|error| {
                                        invalid(format!(
                                            "行政新增公告获知缺少同批公开材料：{error}"
                                        ))
                                    })?
                                    .published_at
                            }
                        };
                        let mut time = acquisition_time
                            .ok_or_else(|| invalid("行政新增获知缺少同次 civil 静点"))?;
                        time.civil = through;
                        causal.record(
                            time,
                            crate::diagnostics::causal::CausalFactKind::Acquisition {
                                account: *id,
                                company: *company,
                                publication: u64::from(record.id.value()),
                                published,
                                acquired: record.observed_at,
                            },
                        );
                    }
                }
            }
            causal
        };
        // 行政候选与 driver 缓存共享同一组不可变值；不变的字段沿用 live 页。
        let mut shared_personals = Vec::with_capacity(personals.len());
        let personal_maps = if personals.is_empty() {
            None
        } else {
            let mut maps = AdministrativePersonalMaps {
                attention: self.state.npc_attention.clone(),
                beliefs: self.state.belief_participants.clone(),
                histories: self.state.history_reads.clone(),
                retail: self.state.retail_experience.clone(),
            };
            for (index, id, value) in personals {
                let adjusted = baseline.attention_at(id, &value.attention)?;
                let mut raw_attention = Arc::new(value.attention);
                if let Some(old) = maps
                    .attention
                    .get_arc(&id)
                    .filter(|old| old.as_ref() == &adjusted)
                {
                    if raw_attention.as_ref() == &adjusted {
                        raw_attention = Arc::clone(old);
                    }
                } else {
                    let session_attention = if raw_attention.as_ref() == &adjusted {
                        Arc::clone(&raw_attention)
                    } else {
                        Arc::new(adjusted)
                    };
                    let _ = maps.attention.insert_arc(id, session_attention);
                }
                let participant = if let Some(old) = maps.beliefs.get_arc(&id).filter(|old| {
                    old.watchlist() == &value.watchlist
                        && old.price_memory() == &value.price_memory
                        && old.information() == &value.information
                        && old.belief() == &value.belief
                }) {
                    Arc::clone(old)
                } else {
                    let next = Arc::new(BeliefParticipantState::new(
                        value.watchlist,
                        value.price_memory,
                        value.information,
                        value.belief,
                    ));
                    let _ = maps.beliefs.insert_arc(id, Arc::clone(&next));
                    next
                };
                let history_reads = if let Some(old) = maps
                    .histories
                    .get_arc(&id)
                    .filter(|old| old.as_ref() == &value.history_reads)
                {
                    Arc::clone(old)
                } else {
                    let next = Arc::new(value.history_reads);
                    let _ = maps.histories.insert_arc(id, Arc::clone(&next));
                    next
                };
                let retail_experience = value.retail_experience.map(|row| {
                    if let Some(old) = maps.retail.get_arc(&id).filter(|old| old.as_ref() == &row) {
                        Arc::clone(old)
                    } else {
                        let next = Arc::new(row);
                        let _ = maps.retail.insert_arc(id, Arc::clone(&next));
                        next
                    }
                });
                shared_personals.push((
                    index,
                    id,
                    SharedRuntimePersonal {
                        attention: raw_attention,
                        participant,
                        history_reads,
                        retail_experience,
                    },
                ));
            }
            Some(maps)
        };
        Ok(PreparedAdministrativeMirror {
            accounts: account_book,
            markets,
            personals: shared_personals,
            personal_maps,
            library,
            plans,
            revision: next_revision,
            install_sequence,
            changed,
            #[cfg(feature = "simulation-diagnostics")]
            causal,
        })
    }

    fn install_administrative_mirror(
        &mut self,
        driver: &mut RuntimeDriver,
        prepared: PreparedAdministrativeMirror,
    ) {
        if !prepared.changed {
            return;
        }
        self.state.accounts = prepared.accounts;
        if let Some(markets) = prepared.markets {
            self.state.markets = markets;
        }
        for (index, id, snapshot) in prepared.personals {
            driver.install_cached_personal_at(index, id, snapshot);
        }
        if let Some(maps) = prepared.personal_maps {
            self.state.npc_attention = maps.attention;
            self.state.belief_participants = maps.beliefs;
            self.state.history_reads = maps.histories;
            self.state.retail_experience = maps.retail;
            self.state.runtime_heap_dirty = true;
        }
        if let Some(library) = prepared.library {
            self.state.library = library;
        }
        if let Some(plans) = prepared.plans {
            self.state.plans = plans;
        }
        #[cfg(feature = "simulation-diagnostics")]
        {
            self.state.causal = prepared.causal;
        }
        self.revision
            .store(prepared.revision, std::sync::atomic::Ordering::Relaxed);
        self.runtime_mirror_install_sequence = prepared.install_sequence;
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "administrative_mirror_tests.rs"]
mod tests;
