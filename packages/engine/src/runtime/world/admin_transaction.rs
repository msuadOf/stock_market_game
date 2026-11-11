use super::*;
use crate::runtime::protocol::{AdminGroupCandidate, AdminPhase};
use std::collections::{BTreeMap, BTreeSet};
mod stages;
pub(crate) use stages::{AdminOperationStage, PreparedAdministrativeStages};

type PanicPayload = Box<dyn std::any::Any + Send>;

/// wasm join 的 io::Error 是显式失败诊断，不是可 resume_unwind 的 native panic payload。
#[cfg(any(test, target_family = "wasm"))]
fn admin_wasm_join_diagnostic(who: WorkerId, error: std::io::Error) -> String {
    format!("{who} wasm join 失败：kind={:?}；{error}", error.kind())
}
/// 同一行政作用域预先算出的 Prepare 写集与回执校验基线。
struct AdminPreparation {
    operations: BTreeMap<GroupIdx, Vec<AdminOperation>>,
    information: Option<crate::runtime::protocol::InformationPreparePayload>,
    stage_layouts: Option<BTreeMap<GroupIdx, crate::runtime::protocol::AdminStageLayout>>,
    projections: BTreeMap<GroupIdx, Vec<crate::runtime::protocol::RepurchaseProjectionInput>>,
    desks: Vec<crate::runtime::protocol::DeskExRightsRequest>,
    before_books: BTreeMap<GroupIdx, crate::plans::PlanBook>,
}

struct AdminFailure {
    error: RuntimeError,
    payload: Option<PanicPayload>,
    terminal: bool,
    committing: bool,
}
impl From<RuntimeError> for AdminFailure {
    fn from(error: RuntimeError) -> Self {
        Self {
            error,
            payload: None,
            terminal: true,
            committing: false,
        }
    }
}
fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: detail.into(),
    }
}

/// 仅由同一个 World 行政作用域铸造；不从可克隆的公开候选取得确认能力。
pub(crate) struct PreparedAdministrativeSource {
    id: AdminTransactionId,
    before: crate::runtime::source::WorldMirrorSource,
    after: crate::runtime::source::WorldMirrorSource,
    captured: crate::runtime::source::MirrorDomains,
}

impl PreparedAdministrativeSource {
    pub(crate) fn id(&self) -> AdminTransactionId {
        self.id
    }
    pub(crate) fn before(&self) -> crate::runtime::source::WorldMirrorSource {
        self.before
    }
    pub(crate) fn after(&self) -> crate::runtime::source::WorldMirrorSource {
        self.after
    }
    pub(crate) fn captured(&self) -> crate::runtime::source::MirrorDomains {
        self.captured
    }
}

impl World {
    /// 同步准备、校验并提交行政账务；宿主仅可在成功返回后发布候选。
    /// inspect 只校验候选和构造待发布值，不执行 observer、I/O 或 session 安装。
    /// 业务拒绝仅撤本事务；通信、协议或 panic 失败永久终止并回收全部 worker。
    pub fn with_admin_transaction<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, RuntimeError> {
        self.with_admin_scope(grouped, None, None, inspect)
    }

    /// 与纯行政入口共用同一 id、pending 与三阶段；全部 live 组必须参与。
    /// 只准备一个真实观察点，不代表已接生产 civil 或多观察点原子编排。
    /// library 须由唯一 dispatcher 基于已提交库生成；inspect 必须校验
    /// 完整 session candidate（包括未变个人引用）与该库一致，不在此认证历史来源。
    pub fn with_information_transaction<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: crate::runtime::protocol::InformationPreparePayload,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, RuntimeError> {
        self.with_admin_scope(grouped, Some(information), None, inspect)
    }

    /// 仅转发到唯一行政作用域，不分配计划、不另作信用或 Commit 后安装。
    #[cfg(test)]
    pub(crate) fn with_repurchase_transaction<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        repurchase: crate::runtime::protocol::RepurchasePreparePayload,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, RuntimeError> {
        self.with_admin_scope(grouped, information, Some(repurchase), inspect)
    }

    fn with_admin_scope<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        repurchase: Option<crate::runtime::protocol::RepurchasePreparePayload>,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, RuntimeError> {
        self.with_administrative_inspection(
            grouped,
            information,
            repurchase,
            (None, Vec::new()),
            inspect,
            |_candidate, _source, value| Ok(value),
        )
        .map(|(value, _unconsumed_ack)| value)
    }

    /// ack 留在 World 内，只有同次真实 Commit 全部成功才交给唯一镜像安装者。
    pub(crate) fn with_prepared_administrative_transaction<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        repurchase: Option<crate::runtime::protocol::RepurchasePreparePayload>,
        inspect: impl FnOnce(&AdminCandidate, &PreparedAdministrativeSource) -> Result<P, RuntimeError>,
    ) -> Result<(P, crate::runtime::source::PreparedMirrorAck), RuntimeError> {
        self.with_administrative_inspection(
            grouped,
            information,
            repurchase,
            (None, Vec::new()),
            |_candidate| Ok(()),
            |candidate, source, ()| inspect(candidate, source),
        )
    }

    /// 两个检查点共用一个 candidate 和一次 Commit；旧入口不需要构造 prepared 来源。
    fn with_administrative_inspection<L, P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        repurchase: Option<crate::runtime::protocol::RepurchasePreparePayload>,
        staged_desks: (
            Option<BTreeMap<GroupIdx, crate::runtime::protocol::AdminStageLayout>>,
            Vec<crate::runtime::protocol::DeskExRightsRequest>,
        ),
        inspect_candidate: impl FnOnce(&AdminCandidate) -> Result<L, RuntimeError>,
        inspect_prepared: impl FnOnce(
            &AdminCandidate,
            &PreparedAdministrativeSource,
            L,
        ) -> Result<P, RuntimeError>,
    ) -> Result<(P, crate::runtime::source::PreparedMirrorAck), RuntimeError> {
        let (stage_layouts, desks) = staged_desks;
        if let Some(reason) = &self.dead_reason {
            return Err(invalid(format!("world 已死亡，不可行政处理：{reason}")));
        }
        if self.group_txs.len() != self.n_groups || self.desk_txs.len() != self.n_stocks {
            return self.finish_admin_failure(
                AdminTransactionId(self.admin_next_id),
                &BTreeMap::new(),
                invalid("行政入口通道面与 world 拓扑不一致").into(),
            );
        }
        if let Some(point) = information
            .as_ref()
            .and_then(|payload| payload.observation.as_ref())
        {
            self.validate_information_clock(&point.clock, point.through, &point.as_of_anchors)?;
        }
        self.admin_next_id = self
            .admin_next_id
            .checked_add(1)
            .ok_or_else(|| invalid("行政事务 id 已耗尽"))?;
        let id = AdminTransactionId(self.admin_next_id);
        if let Some(payload) = &information {
            let mut stocks = BTreeSet::new();
            if (!payload.price_unit_changes.is_empty() && payload.observation.is_none())
                || payload.price_unit_changes.iter().any(|change| {
                    change.expected_last_close <= 0
                        || change.reference_price <= 0
                        || !stocks.insert(change.stock)
                        || !desks.contains(change)
                })
            {
                return Err(invalid(
                    "个人价格单位转换缺少同次真实 Desk 除权请求或观察作用域",
                ));
            }
        }
        let mut seen_stocks = BTreeSet::new();
        if desks.iter().any(|request| {
            usize::from(request.stock) >= self.n_stocks || !seen_stocks.insert(request.stock)
        }) {
            return Err(invalid("除权交易部越界或重复"));
        }
        let mut operations = BTreeMap::new();
        let mut projections = BTreeMap::new();
        let has_repurchase_scope = repurchase.is_some();
        if let Some(payload) = repurchase {
            for (group, rows) in payload.groups {
                if group as usize >= self.n_groups || projections.insert(group, rows).is_some() {
                    return Err(invalid(format!("回购投影 group 越界或重复：{group}")));
                }
            }
        }
        for (group, ops) in grouped {
            if group as usize >= self.n_groups {
                return Err(invalid(format!("行政 group 越界：{group}")));
            }
            if operations.contains_key(&group) {
                return Err(invalid(format!("行政 group 重复：{group}")));
            }
            operations.insert(group, ops);
        }
        if information.is_some() {
            for group in 0..self.n_groups as GroupIdx {
                operations.entry(group).or_insert_with(Vec::new);
            }
        } else {
            operations.retain(|group, ops| !ops.is_empty() || projections.contains_key(group));
        }
        for &group in projections.keys() {
            operations.entry(group).or_insert_with(Vec::new);
        }
        let expected: BTreeMap<_, Vec<_>> = operations
            .iter()
            .map(|(&group, ops)| {
                (
                    group,
                    ops.iter()
                        .map(|op| op.coordinates().0)
                        .chain(
                            projections
                                .get(&group)
                                .into_iter()
                                .flatten()
                                .map(|row| row.local),
                        )
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                )
            })
            .collect();
        let mut sent = BTreeMap::new();
        // 在任何 Prepare 前只读完整原簿；用于校验回执，不提前发布推算身份。
        let before_books = if has_repurchase_scope {
            match self.collect_plans() {
                Ok(books) => books.into_iter().collect(),
                Err(error) => return self.finish_admin_failure(id, &sent, error.into()),
            }
        } else {
            BTreeMap::new()
        };
        match self.execute_admin(
            id,
            AdminPreparation {
                operations,
                information,
                stage_layouts,
                projections,
                desks,
                before_books,
            },
            &expected,
            &mut sent,
            inspect_candidate,
            inspect_prepared,
        ) {
            Ok(value) => Ok(value),
            Err(failure) => self.finish_admin_failure(id, &sent, failure),
        }
    }

    fn finish_admin_failure<P>(
        &mut self,
        id: AdminTransactionId,
        sent: &BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
        mut failure: AdminFailure,
    ) -> Result<P, RuntimeError> {
        let mut host_cleanup_diagnostics = String::new();
        if failure.terminal {
            self.day_end_prior_public_views = None;
            self.dead_reason = Some(format!("行政事务 {id:?} 失败：{}", failure.error));
            // catch 只负责运输及回收；次生错误不得替换首次 panic payload。
            let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.terminate_admin(id, sent, !failure.committing)
            }));
            match cleanup {
                Ok((payload, diagnostics)) => {
                    if failure.payload.is_none() {
                        failure.payload = payload;
                    }
                    if !diagnostics.is_empty() {
                        host_cleanup_diagnostics.push_str(&diagnostics);
                        self.dead_reason
                            .as_mut()
                            .expect("已判死")
                            .push_str(&format!("；收尾：{diagnostics}"));
                    }
                }
                Err(payload) => {
                    self.dead_reason
                        .as_mut()
                        .expect("已判死")
                        .push_str("；行政收尾再次 panic，强制回收");
                    self.sever();
                    for (who, result) in self.join_worker_results_with_context(super::ExitReconcileContext {
                        phase: super::ExitReconcilePhase::Administrative,
                        last_committed_tick: Some(self.global.committed_tick),
                        active_tick: None,
                        round: None,
                        candidate_unpublished: false,
                    }) {
                        if failure.payload.is_none() {
                            failure.payload = result
                                .as_ref()
                                .err()
                                .and_then(RuntimeError::retained_panic_payload)
                                .and_then(|payload| payload.take());
                        }
                        if let Err(error) = &result {
                            self.dead_reason
                                .as_mut()
                                .expect("已判死")
                                .push_str(&format!("；收尾 join {who} 失败：{error}"));
                        }
                        self.dead_worker_results.push((who, result));
                    }
                    if failure.payload.is_none() {
                        failure.payload = Some(payload);
                    }
                }
            }
        }
        if let Some(payload) = failure.payload {
            std::panic::resume_unwind(payload);
        }
        if matches!(&failure.error, RuntimeError::HostAborted { .. }) {
            if host_cleanup_diagnostics.is_empty() {
                return Err(failure.error);
            }
            return Err(RuntimeError::HostAbortedCleanup {
                primary: Box::new(failure.error),
                detail: host_cleanup_diagnostics,
            });
        }
        if failure.terminal {
            return Err(invalid(
                self.dead_reason.as_ref().expect("终止失败已判死").clone(),
            ));
        }
        Err(failure.error)
    }

    fn execute_admin<L, P>(
        &mut self,
        id: AdminTransactionId,
        preparation: AdminPreparation,
        expected: &BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
        sent: &mut BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
        inspect_candidate: impl FnOnce(&AdminCandidate) -> Result<L, RuntimeError>,
        inspect_prepared: impl FnOnce(
            &AdminCandidate,
            &PreparedAdministrativeSource,
            L,
        ) -> Result<P, RuntimeError>,
    ) -> Result<(P, crate::runtime::source::PreparedMirrorAck), AdminFailure> {
        let AdminPreparation {
            operations,
            information,
            stage_layouts,
            projections,
            desks,
            before_books,
        } = preparation;
        let information_clock = information
            .as_ref()
            .and_then(|payload| payload.observation.as_ref())
            .map(|point| {
                self.authenticate_information_clock(
                    &point.clock,
                    point.through,
                    Arc::new(point.as_of_anchors.clone()),
                )
            })
            .transpose()?;
        for (&group, ops) in &operations {
            self.send_admin(
                group,
                AdminPhase::Prepare,
                GroupIn::AdminPrepare {
                    id,
                    operations: ops.clone(),
                    stage_layout: stage_layouts
                        .as_ref()
                        .and_then(|layouts| layouts.get(&group))
                        .cloned(),
                    information: information.clone(),
                    repurchase: projections.get(&group).cloned(),
                    information_clock: information_clock.clone(),
                },
            )?;
            sent.insert(group, expected[&group].clone());
        }
        let mut prepared = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut rejection = None;
        while seen.len() < expected.len() {
            let (group, result) =
                self.receive_admin(id, AdminPhase::Prepare, expected, &mut seen)?;
            match result {
                Ok(candidate) => {
                    stages::validate_group_stage_evidence(
                        id,
                        &candidate,
                        &operations[&group],
                        stage_layouts
                            .as_ref()
                            .and_then(|layouts| layouts.get(&group)),
                    )?;
                    self.validate_information_candidate(&candidate, information.as_ref())?;
                    self.validate_repurchase_candidate(
                        &candidate,
                        &operations[&group],
                        projections.get(&group),
                        before_books.get(&group),
                    )?;
                    prepared.insert(group, candidate);
                }
                Err(error) => {
                    if rejection.is_none() {
                        rejection = Some(error);
                    }
                }
            }
        }
        self.admin_quiet()?;
        if let Some(error) = rejection {
            let subset = expected
                .iter()
                .filter(|(group, _)| prepared.contains_key(group))
                .map(|(&g, ls)| (g, ls.clone()))
                .collect();
            self.abort_admin_preserving_error(id, &subset, &error)?;
            return Err(AdminFailure {
                error,
                payload: None,
                terminal: false,
                committing: false,
            });
        }
        let mut prepared_desks = Vec::new();
        for request in &desks {
            match self.prepare_desk_ex_rights(id, *request) {
                Ok(candidate) => prepared_desks.push(candidate),
                Err(error) if !error.terminal => {
                    self.abort_admin_preserving_error(id, expected, &error.error)?;
                    self.control_desk_ex_rights(id, AdminPhase::Abort, &prepared_desks)?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
        }
        let candidate = AdminCandidate {
            id,
            groups: prepared.into_values().collect(),
            desks: prepared_desks.clone(),
        };
        // 保留旧 World 入口先检查真实 candidate 的契约；拒绝或 panic 均不进入来源准备。
        let legacy_value = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inspect_candidate(&candidate)
        })) {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                self.abort_admin_preserving_error(id, expected, &error)?;
                self.control_desk_ex_rights(id, AdminPhase::Abort, &prepared_desks)?;
                return Err(AdminFailure {
                    error,
                    payload: None,
                    terminal: false,
                    committing: false,
                });
            }
            Err(payload) => {
                return Err(AdminFailure {
                    error: invalid("行政候选检查 panic"),
                    payload: Some(payload),
                    terminal: true,
                    committing: false,
                });
            }
        };
        let domains = candidate.groups.iter().fold(
            crate::runtime::source::MirrorDomains::default(),
            |all, group| all.union(group.mirror_domains),
        );
        let next_revision = if domains.is_empty() && prepared_desks.is_empty() {
            self.non_tick_revision
        } else {
            match self.non_tick_revision.checked_add(1) {
                Some(revision) => revision,
                None => {
                    let error = invalid("非 tick 成功来源游标已耗尽，拒绝首个 commit");
                    self.abort_admin_preserving_error(id, expected, &error)?;
                    self.control_desk_ex_rights(id, AdminPhase::Abort, &prepared_desks)?;
                    return Err(AdminFailure {
                        error,
                        payload: None,
                        terminal: false,
                        committing: false,
                    });
                }
            }
        };
        let before = self.mirror_source();
        let mut after = before;
        after.non_tick_revision = next_revision;
        let captured = self.pending_mirror_domains.union(domains);
        let source = PreparedAdministrativeSource {
            id,
            before,
            after,
            captured,
        };
        let ack = crate::runtime::source::PreparedMirrorAck { domains: captured };
        let value = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            inspect_prepared(&candidate, &source, legacy_value)
        })) {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                self.abort_admin_preserving_error(id, expected, &error)?;
                self.control_desk_ex_rights(id, AdminPhase::Abort, &prepared_desks)?;
                return Err(AdminFailure {
                    error,
                    payload: None,
                    terminal: false,
                    committing: false,
                });
            }
            Err(payload) => {
                return Err(AdminFailure {
                    error: invalid("行政候选检查 panic"),
                    payload: Some(payload),
                    terminal: true,
                    committing: false,
                })
            }
        };
        let next_public_views = if prepared_desks.is_empty() {
            None
        } else {
            let mut views = self.public_views.clone();
            for desk in &prepared_desks {
                views[usize::from(desk.request.stock)] = desk.view.clone();
            }
            Some(views)
        };
        if !expected.is_empty() {
            // 首个 commit 发送前关窗；任何部分提交失败均不可复用 world。
            self.day_end_prior_public_views = None;
            self.control_admin(id, AdminPhase::Commit, expected)
                .map_err(|mut failure| {
                    failure.committing = true;
                    failure
                })?;
        }
        if !prepared_desks.is_empty() {
            self.day_end_prior_public_views = None;
            self.control_desk_ex_rights(id, AdminPhase::Commit, &prepared_desks)
                .map_err(|mut failure| {
                    failure.committing = true;
                    failure
                })?;
        }
        // 公开快照在首 Commit 前构造，成功尾只移动。
        if let Some(views) = next_public_views {
            self.public_views = views;
        }
        self.non_tick_revision = next_revision;
        self.pending_mirror_domains = captured;
        Ok((value, ack))
    }

    fn prepare_desk_ex_rights(
        &mut self,
        id: AdminTransactionId,
        request: crate::runtime::protocol::DeskExRightsRequest,
    ) -> Result<crate::runtime::protocol::DeskExRightsCandidate, AdminFailure> {
        self.desk_txs[usize::from(request.stock)]
            .send(DeskMsg::ExRightsPrepare { id, request })
            .map_err(|_| invalid("除权 Prepare 发送失败"))?;
        self.receive_desk_ex_rights(id, AdminPhase::Prepare, request)
    }

    fn control_desk_ex_rights(
        &mut self,
        id: AdminTransactionId,
        phase: AdminPhase,
        requests: &[crate::runtime::protocol::DeskExRightsCandidate],
    ) -> Result<(), AdminFailure> {
        for request in requests {
            self.desk_txs[usize::from(request.request.stock)]
                .send(DeskMsg::ExRightsComplete { id, phase })
                .map_err(|_| invalid("除权完成消息发送失败"))?;
            if self.receive_desk_ex_rights(id, phase, request.request)? != *request {
                return Err(invalid("除权完成回执与 Prepare 候选不一致").into());
            }
        }
        Ok(())
    }

    fn receive_desk_ex_rights(
        &mut self,
        id: AdminTransactionId,
        phase: AdminPhase,
        request: crate::runtime::protocol::DeskExRightsRequest,
    ) -> Result<crate::runtime::protocol::DeskExRightsCandidate, AdminFailure> {
        match recv_deadline(&self.up_rx, Phase::Seal, &self.deadline)? {
            UpMsg::DeskAdminReceipt {
                id: actual,
                stock,
                phase: actual_phase,
                result,
            } if actual == id && stock == request.stock && actual_phase == phase => match result {
                Ok(actual) if actual.request == request => Ok(actual),
                Ok(_) => Err(invalid("除权回执与原请求不一致").into()),
                Err(error) if phase == AdminPhase::Prepare => Err(AdminFailure {
                    error,
                    payload: None,
                    terminal: false,
                    committing: false,
                }),
                Err(error) => Err(error.into()),
            },
            UpMsg::TaskDied { who, payload } => Err(AdminFailure {
                error: invalid(format!("除权交易部 panic：{who}")),
                payload: Some(payload),
                terminal: true,
                committing: false,
            }),
            UpMsg::Failed { who, error } => Err(RuntimeError::WorkerFailed {
                who,
                error: Box::new(error),
            }
            .into()),
            other => Err(invalid(format!("除权收到非预期消息：{other:?}")).into()),
        }
    }

    /// 读面通信失败不能留下可重试的半收齐通道；沿既有 terminal 回收保首错。
    pub(crate) fn stop_failed_administrative_read(&mut self, error: RuntimeError) -> RuntimeError {
        self.stop_after_fatal_failure(
            self.global.committed_tick,
            0,
            "administrative_baseline",
            error,
            FatalCleanupMode::StopImmediately,
            None,
        )
    }

    fn validate_repurchase_candidate(
        &self,
        candidate: &AdminGroupCandidate,
        operations: &[AdminOperation],
        rows: Option<&Vec<crate::runtime::protocol::RepurchaseProjectionInput>>,
        before: Option<&crate::plans::PlanBook>,
    ) -> Result<(), RuntimeError> {
        use crate::runtime::protocol::{AdminPlanReference, CreatedRepurchasePlan};
        let Some(rows) = rows else {
            return if candidate.repurchase.is_none() {
                Ok(())
            } else {
                Err(invalid("非回购作用域收到执行候选"))
            };
        };
        let prepared = candidate
            .repurchase
            .as_ref()
            .ok_or_else(|| invalid("回购 Prepare 缺少完整候选"))?;
        let mut expected_book = before
            .ok_or_else(|| invalid("回购 Prepare 缺少事务前完整簿"))?
            .clone();
        let mut expected_created = Vec::new();
        for (index, operation) in operations.iter().enumerate() {
            if let AdminOperation::OpenIssuerRepurchase {
                local,
                stock,
                issuer,
            } = *operation
            {
                let account = self
                    .information_roster
                    .get(candidate.group as usize)
                    .and_then(|roster| roster.get(local as usize))
                    .copied()
                    .ok_or_else(|| invalid("回购 Open 的真实名册坐标缺失"))?;
                let plan_id = expected_book
                    .create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
                        owner: account,
                        issuer,
                        stock: crate::StockId(u32::from(stock)),
                    })
                    .map_err(|error| invalid(format!("回购回执验证重放失败：{error}")))?;
                expected_created.push(CreatedRepurchasePlan {
                    group: candidate.group,
                    operation_index: u32::try_from(index)
                        .map_err(|_| invalid("回购 Open 位置溢出"))?,
                    local,
                    stock,
                    issuer,
                    plan_id,
                });
            }
        }
        for operation in operations {
            if let AdminOperation::CloseIssuerRepurchase { execution_plan, .. } = *operation {
                expected_book
                    .close_issuer_repurchase(execution_plan)
                    .map_err(|error| invalid(error.to_string()))?;
            }
        }
        if prepared.created != expected_created || prepared.book != expected_book {
            return Err(invalid(
                "回购 Prepare 完整 book/OwnerSequence/created 回执不一致",
            ));
        }
        let mut seen = BTreeSet::new();
        for row in rows {
            let id = match row.execution_plan {
                AdminPlanReference::Existing(id) => id,
                AdminPlanReference::CreatedInThisPrepare { operation_index } => {
                    expected_created
                        .iter()
                        .find(|created| created.operation_index == operation_index)
                        .ok_or_else(|| invalid("回购投影 Created 位置不符"))?
                        .plan_id
                }
            };
            if !seen.insert(id) || prepared.projections.get(&id) != Some(&row.projection) {
                return Err(invalid("回购 Prepare 执行投影漏失、重复或被替换"));
            }
        }
        for (id, projection) in &prepared.projections {
            let entry = prepared
                .book
                .entry(*id)
                .map_err(|_| invalid("回购投影含未知计划"))?;
            let crate::plans::PlanEntry::IssuerRepurchase(plan) = entry else {
                return Err(invalid("回购投影来源不是公司计划"));
            };
            if id.owner != projection.account
                || plan.issuer() != projection.issuer
                || plan.stock() != crate::StockId(u32::from(projection.stock))
            {
                return Err(invalid("回购投影与完整簿身份不符"));
            }
        }
        if prepared.sources.keys().copied().collect::<BTreeSet<_>>() != seen {
            return Err(invalid("回购 Prepare 来源集合存在额外或遗漏计划"));
        }
        for (id, source) in &prepared.sources {
            let projection = &prepared.projections[id];
            if source.consumed_cursor > projection.book_through_cursor
                || projection.book_through_cursor > source.tail_cursor
                || source.tail_cursor < source.consumed_cursor
                || source.facts.len() as u64 != source.tail_cursor - source.consumed_cursor
                || source.facts.iter().enumerate().any(|(index, fact)| {
                    source.consumed_cursor.checked_add(index as u64 + 1) != Some(fact.cursor)
                        || fact.source.request_key.plan_id != *id
                        || fact.source.request_key.stock
                            != crate::StockId(u32::from(projection.stock))
                })
            {
                return Err(invalid("回购 Prepare 来源 cursor 或计划身份不一致"));
            }
        }
        if prepared
            .projections
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
            != seen
        {
            return Err(invalid("回购 Prepare 投影集合存在额外或遗漏计划"));
        }
        if !expected_created.is_empty() && !candidate.mirror_domains.plans_pending {
            return Err(invalid("回购创建候选遗漏 plans_pending"));
        }
        Ok(())
    }

    fn validate_information_candidate(
        &self,
        candidate: &AdminGroupCandidate,
        payload: Option<&crate::runtime::protocol::InformationPreparePayload>,
    ) -> Result<(), RuntimeError> {
        match (candidate.information.as_ref(), payload) {
            (None, None) => Ok(()),
            (Some(info), Some(payload)) => {
                if !Arc::ptr_eq(&info.library, &payload.library)
                    || !Arc::ptr_eq(&info.current_facts, &payload.current_facts)
                    || info.observed_through != payload.observation.as_ref().map(|p| p.through)
                    || info.personals.windows(2).any(|pair| pair[0].0 >= pair[1].0)
                    || info.personals.iter().any(|(local, account, snapshot)| {
                        self.information_roster
                            .get(candidate.group as usize)
                            .and_then(|roster| roster.get(*local as usize))
                            != Some(account)
                            || snapshot.information.owner() != *account
                            || snapshot.belief.npc() != *account
                    })
                    || (payload.observation.is_none() && !info.personals.is_empty())
                {
                    return Err(invalid(format!(
                        "组 {} 信息候选回执身份/输入不一致",
                        candidate.group
                    )));
                }
                Ok(())
            }
            _ => Err(invalid(format!(
                "组 {} 信息候选作用域错配",
                candidate.group
            ))),
        }
    }

    fn abort_admin_preserving_error(
        &mut self,
        id: AdminTransactionId,
        expected: &BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
        original: &RuntimeError,
    ) -> Result<(), AdminFailure> {
        self.control_admin(id, AdminPhase::Abort, expected)
            .map_err(|mut failure| {
                failure.error = invalid(format!(
                    "行政原始拒绝：{original}；撤销失败：{}",
                    failure.error
                ));
                failure
            })
    }

    fn control_admin(
        &mut self,
        id: AdminTransactionId,
        phase: AdminPhase,
        expected: &BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
    ) -> Result<(), AdminFailure> {
        for &group in expected.keys() {
            let msg = match phase {
                AdminPhase::Commit => GroupIn::AdminCommit { id },
                AdminPhase::Abort => GroupIn::AdminAbort { id },
                AdminPhase::Prepare => return Err(invalid("行政控制相位非法").into()),
            };
            self.send_admin(group, phase, msg)?;
        }
        let mut seen = BTreeSet::new();
        while seen.len() < expected.len() {
            let (_, result) = self.receive_admin(id, phase, expected, &mut seen)?;
            result.map_err(AdminFailure::from)?;
        }
        self.admin_quiet()
    }

    /// 仅检查已经入队的额外消息，不引入等待未来消息的第四阶段。
    fn admin_quiet(&mut self) -> Result<(), AdminFailure> {
        match super::super::upstream::try_recv_up(&self.up_rx, Phase::Seal)? {
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(()),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err(invalid("行政收齐后上行通道断连").into())
            }
            Ok(UpMsg::TaskDied { who, payload }) => Err(AdminFailure {
                error: invalid(format!("行政收齐后 worker panic：{who}")),
                payload: Some(payload),
                terminal: true,
                committing: false,
            }),
            Ok(UpMsg::Failed { who, error }) => Err(RuntimeError::WorkerFailed {
                who,
                error: Box::new(error),
            }
            .into()),
            Ok(other) => Err(invalid(format!("行政收齐后仍有非预期上行：{other:?}")).into()),
        }
    }

    fn send_admin(
        &mut self,
        group: GroupIdx,
        phase: AdminPhase,
        message: GroupIn,
    ) -> Result<(), RuntimeError> {
        #[cfg(test)]
        if self.admin_send_fault_for_test == Some((phase, group)) {
            self.admin_send_fault_for_test = None;
            let (disconnected, receiver) = channel();
            drop(receiver);
            return disconnected
                .send(message)
                .map_err(|_| RuntimeError::SendFailed {
                    detail: format!("行政 {phase:?} 真实断连发送：group={group}"),
                });
        }
        self.group_txs[group as usize]
            .send(message)
            .map_err(|_| RuntimeError::SendFailed {
                detail: format!("行政 {phase:?} 发送失败：group={group}"),
            })
    }

    fn receive_admin(
        &mut self,
        id: AdminTransactionId,
        phase: AdminPhase,
        expected: &BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
        seen: &mut BTreeSet<GroupIdx>,
    ) -> Result<(GroupIdx, Result<AdminGroupCandidate, RuntimeError>), AdminFailure> {
        match recv_deadline(&self.up_rx, Phase::Seal, &self.deadline)? {
            UpMsg::AdminReceipt {
                id: actual,
                group,
                phase: actual_phase,
                result,
            } => {
                let locals = expected
                    .get(&group)
                    .ok_or_else(|| invalid(format!("行政 {phase:?} 非参与组回执：{group}")))?;
                if actual != id || actual_phase != phase || !seen.insert(group) {
                    return Err(invalid(format!("行政回执重复/错位：期望 {id:?}/{phase:?}，收到 {actual:?}/{actual_phase:?}/group={group}")).into());
                }
                if let Ok(candidate) = &result {
                    if phase != AdminPhase::Prepare && candidate.stages.is_some() {
                        return Err(invalid("行政控制回执不得携带阶段 Prepare 候选").into());
                    }
                    if phase != AdminPhase::Prepare && candidate.repurchase.is_some() {
                        return Err(invalid("行政控制回执不得携带回购 Prepare 候选").into());
                    }
                    if candidate.group != group
                        || candidate
                            .accounts
                            .iter()
                            .map(|(local, _)| *local)
                            .collect::<Vec<_>>()
                            != *locals
                    {
                        return Err(invalid(format!(
                            "行政回执 touched 集合错误：group={group}, phase={phase:?}"
                        ))
                        .into());
                    }
                }
                Ok((group, result))
            }
            UpMsg::TaskDied { who, payload } => Err(AdminFailure {
                error: invalid(format!("行政 worker panic：{who}")),
                payload: Some(payload),
                terminal: true,
                committing: false,
            }),
            UpMsg::Failed { who, error } => Err(RuntimeError::WorkerFailed {
                who,
                error: Box::new(error),
            }
            .into()),
            other => Err(invalid(format!("行政 {phase:?} 收到非预期消息：{other:?}")).into()),
        }
    }

    /// 不发送普通 Abort/Drain；已发布 tick、市场句柄和交易部事实均不倒放。
    fn terminate_admin(
        &mut self,
        id: AdminTransactionId,
        expected: &BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
        abort_pending: bool,
    ) -> (Option<PanicPayload>, String) {
        let mut diagnostics = Vec::new();
        if abort_pending {
            for &group in expected.keys() {
                if self.group_txs[group as usize]
                    .send(GroupIn::AdminAbort { id })
                    .is_err()
                {
                    diagnostics.push(format!("组 {group} 行政清理发送失败"));
                }
            }
        }
        for (group, tx) in self.group_txs.iter().enumerate() {
            if tx.send(GroupIn::Shutdown).is_err() {
                diagnostics.push(format!("组 {group} Shutdown 发送失败"));
            }
        }
        for (stock, tx) in self.desk_txs.iter().enumerate() {
            if tx.send(DeskMsg::Shutdown).is_err() {
                diagnostics.push(format!("交易部 {stock} Shutdown 发送失败"));
            }
        }
        self.worker_shutdown.store(true, Ordering::SeqCst);
        let mut groups: Vec<Option<GroupFinalState>> = (0..self.n_groups).map(|_| None).collect();
        let mut desks: Vec<Option<DeskFinalState>> = (0..self.n_stocks).map(|_| None).collect();
        let mut payload = None;
        while groups.iter().any(Option::is_none) || desks.iter().any(Option::is_none) {
            match recv_deadline(&self.up_rx, Phase::Seal, &self.deadline) {
                Ok(UpMsg::GroupFinal(final_state)) => {
                    let index = final_state.group as usize;
                    match groups.get_mut(index) {
                        Some(slot) if slot.is_none() => *slot = Some(*final_state),
                        _ => diagnostics.push(format!("GroupFinal 重复/越界：{index}")),
                    }
                }
                Ok(UpMsg::DeskFinal(final_state)) => {
                    let index = final_state.stock as usize;
                    match desks.get_mut(index) {
                        Some(slot) if slot.is_none() => *slot = Some(*final_state),
                        _ => diagnostics.push(format!("DeskFinal 重复/越界：{index}")),
                    }
                }
                Ok(UpMsg::TaskDied { who, payload: p }) => {
                    diagnostics.push(format!("收尾 worker panic：{who}"));
                    if payload.is_none() {
                        payload = Some(p);
                    }
                }
                Ok(UpMsg::AdminReceipt {
                    result: Err(error), ..
                })
                | Ok(UpMsg::Failed { error, .. }) => diagnostics.push(error.to_string()),
                Ok(UpMsg::AdminReceipt { .. }) => {}
                Ok(other) => diagnostics.push(format!("收尾非预期消息：{other:?}")),
                Err(error) => {
                    diagnostics.push(format!("Final 不完整：{error}"));
                    break;
                }
            }
        }
        self.worker_shutdown.store(true, Ordering::SeqCst);
        if groups.iter().all(Option::is_some) && desks.iter().all(Option::is_some) {
            let report = ShutdownReport {
                group_finals: groups.into_iter().map(|g| g.expect("已验证")).collect(),
                desk_finals: desks.into_iter().map(|d| d.expect("已验证")).collect(),
                worker_results: Vec::new(),
                drained_minute_bars: std::mem::take(&mut self.pending_day_bars),
            };
            match self.join_workers(report) {
                Ok(report) => self.dead_finals = Some(report),
                Err(error) => diagnostics.push(error.to_string()),
            }
        } else {
            for (who, result) in self.join_worker_results_with_context(super::ExitReconcileContext {
                phase: super::ExitReconcilePhase::Administrative,
                last_committed_tick: Some(self.global.committed_tick),
                active_tick: None,
                round: None,
                candidate_unpublished: false,
            }) {
                if payload.is_none() {
                    payload = result
                        .as_ref()
                        .err()
                        .and_then(RuntimeError::retained_panic_payload)
                        .and_then(|payload| payload.take());
                }
                if let Err(error) = &result {
                    diagnostics.push(format!("{who}：{error}"));
                }
                self.dead_worker_results.push((who, result));
            }
        }
        (payload, diagnostics.join("；"))
    }
}

#[cfg(test)]
mod wasm_join_diagnostic_tests {
    use super::admin_wasm_join_diagnostic;
    use crate::facade_slot::arena::SlotArena;
    use crate::facade_slot::codec::{decode_join_state, JoinState, GEN_BITS_DEFAULT};
    use crate::facade_slot::platform::NullPlatform;
    use crate::facade_slot::roles::join_typed;
    use crate::facade_slot::test_supervisor::{write_dead, DeadKind, DeadOutcome};
    use crate::runtime::protocol::WorkerId;
    use std::sync::atomic::Ordering;

    fn assert_dead_join_diagnostic(who: WorkerId, kind: DeadKind, code: u32, expected: &str) {
        let arena = SlotArena::alloc(1, GEN_BITS_DEFAULT);
        assert_eq!(
            write_dead(&arena, &NullPlatform, 0, 0, kind, code),
            DeadOutcome::Written
        );
        let error = join_typed::<(), _>(&arena, &NullPlatform, 0, 0)
            .expect_err("真实 DEAD join 必须交出 io::Error");
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
        let rendered = error.to_string();
        assert!(rendered.contains(expected));
        let diagnostic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            admin_wasm_join_diagnostic(who, error)
        }))
        .expect("wasm join 失败必须返回诊断，不能伪装 panic payload");
        assert!(
            diagnostic.contains(&who.to_string()),
            "诊断必须保留真实 worker 身份：{diagnostic}"
        );
        assert!(
            diagnostic.contains("Other"),
            "诊断必须保留 io::ErrorKind：{diagnostic}"
        );
        assert!(
            diagnostic.contains(&rendered),
            "死亡种类与结构化 code 不能丢失：{diagnostic}"
        );
        assert_eq!(
            decode_join_state(arena.join_word(0).load(Ordering::SeqCst), GEN_BITS_DEFAULT),
            JoinState::Consumed
        );
    }

    #[test]
    fn admin_wasm_join_hardtrap_keeps_real_identity_kind_code_and_consumes_join() {
        assert_dead_join_diagnostic(
            WorkerId::Group(3),
            DeadKind::HardTrap,
            0xD5,
            "FACADE_DEAD_HARDTRAP:0xD5",
        );
    }

    #[test]
    fn admin_wasm_join_panic_signal_is_io_diagnostic_and_consumes_join() {
        assert_dead_join_diagnostic(
            WorkerId::Desk(5),
            DeadKind::Panic,
            0xA6,
            "FACADE_DEAD_PANIC:0xA6",
        );
    }

    #[test]
    fn 通用故障清理将真实_wasm_join_错误作为诊断而不是_panic() {
        let arena = SlotArena::alloc(1, GEN_BITS_DEFAULT);
        assert_eq!(write_dead(&arena, &NullPlatform, 0, 0, DeadKind::HardTrap, 0xD5), DeadOutcome::Written);
        let error = join_typed::<(), _>(&arena, &NullPlatform, 0, 0).expect_err("必须得到真实 DEAD join 错误");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::super::wasm_fatal_join_failure(WorkerId::Group(3), error)))
            .expect("join 的 io::Error 只能成为诊断，不能作为 panic 恢复");
        assert!(matches!(result, crate::runtime::protocol::RuntimeError::ProtocolInvariant { detail } if detail.contains("group-3") && detail.contains("FACADE_DEAD_HARDTRAP:0xD5")));
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::invalid;
    use crate::orderbook::Side;
    use crate::runtime::group::{PlaceSpec, TickPlan};
    use crate::runtime::protocol::{AdminFault, AdminOperation, AdminPhase};
    use crate::runtime::skeleton_tests::basic_spec;
    use crate::runtime::world::AdminCandidate;
    use std::collections::BTreeMap;

    fn spec() -> WorldSpec {
        let mut spec = basic_spec(2, 1, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
        spec.ticks_per_day = 100;
        spec.deadline = DeadlineConfig {
            normal: Duration::from_millis(300),
            long: Duration::from_millis(300),
        };
        spec
    }
    fn traded_spec() -> WorldSpec {
        let mut s = spec();
        s.script_for = Box::new(|group| {
            Box::new(move |tick, _| {
                let mut plan = TickPlan::default();
                if tick == 1 {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: if group == 0 { Side::Buy } else { Side::Sell },
                        price: 1000,
                        qty: 100,
                    });
                }
                plan
            })
        });
        s
    }
    fn dividend(local: u32, amount: i64) -> AdminOperation {
        AdminOperation::CashDividend {
            local,
            stock: 0,
            amount,
        }
    }
    fn credit() -> Vec<(GroupIdx, Vec<AdminOperation>)> {
        vec![(0, vec![dividend(0, 101)]), (1, vec![dividend(1, 202)])]
    }

    #[test]
    fn 行政收尾必须保留宿主故障的原始类型与阶段() {
        let mut world = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
        ).unwrap();
        let error = RuntimeError::HostAborted { phase: Phase::Drain, who: None };
        world.mark_host_aborted(&error);
        let result = world.finish_admin_failure::<()>(
            AdminTransactionId(1),
            &BTreeMap::new(),
            super::AdminFailure::from(error),
        );
        let primary = match result {
            Err(RuntimeError::HostAbortedCleanup { primary, detail }) => {
                assert!(!detail.is_empty(), "次生清理诊断不得丢失");
                *primary
            }
            Err(error) => error,
            Ok(()) => panic!("宿主永久故障不得返回成功"),
        };
        assert!(matches!(primary, RuntimeError::HostAborted { phase: Phase::Drain, who: None }));
        assert!(world.is_terminal());
        assert!(world.run_ticks(1).is_err());
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn 行政宿主故障与上行断连并存时保留主因和次生诊断() {
        use std::error::Error;
        let mut world = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
        ).unwrap();
        let who = WorkerId::Group(1);
        let original = RuntimeError::HostAborted { phase: Phase::Drain, who: Some(who) };
        world.mark_host_aborted(&original);
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(sender);
        world.up_rx = receiver;
        let error = world.finish_admin_failure::<()>(
            AdminTransactionId(1),
            &BTreeMap::new(),
            super::AdminFailure::from(original),
        ).expect_err("上行断连不得覆盖宿主故障");
        assert!(matches!(&error, RuntimeError::HostAbortedCleanup { detail, .. } if detail.contains("Final 不完整")));
        // thiserror 把 Box<RuntimeError> 本身作为 source 的动态类型。
        let source = error.source()
            .and_then(|source| source.downcast_ref::<Box<RuntimeError>>())
            .map(Box::as_ref);
        assert!(matches!(source, Some(RuntimeError::HostAborted { phase: Phase::Drain, who: Some(actual) }) if *actual == who));
        assert!(error.to_string().contains("Final 不完整"));
        assert!(world.is_terminal());
    }

    #[test]
    fn successful_non_tick_writes_advance_source_without_published_tick() {
        let mut world = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        let initial = world.mirror_source();
        for revision in 1..=2 {
            world.with_admin_transaction(credit(), |_| Ok(())).unwrap();
            let source = world.mirror_source();
            assert_eq!(
                (source.tick, source.generation),
                (initial.tick, initial.generation)
            );
            assert_eq!(
                source.non_tick_revision, revision,
                "成功 Admin 不是 attempt id；同 tick 连续写必须有独立成功来源"
            );
            assert!(world.pending_mirror_domains().accounts_full);
        }
        let source = world.mirror_source();
        let pending = world.pending_mirror_domains();
        let rejected = world
            .with_admin_transaction(credit(), |_| Err::<(), _>(invalid("纯 inspect 业务拒绝")));
        assert!(rejected.is_err());
        assert_eq!(world.mirror_source(), source, "失败不推进成功来源");
        assert_eq!(
            world.pending_mirror_domains(),
            pending,
            "失败不清旧 dirty 原因"
        );
        world.shutdown().unwrap();
    }

    #[test]
    fn non_tick_revision_exhaustion_rejects_before_first_commit() {
        let mut world = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        world.non_tick_revision = u64::MAX;
        let before = accounts(&mut world);
        let source = world.mirror_source();
        let pending = world.pending_mirror_domains();
        let rejected = world.with_admin_transaction(credit(), |_| Ok(()));
        assert!(rejected.is_err(), "耗尽必须在首个不可撤 commit 前拒绝");
        assert_eq!(
            accounts(&mut world),
            before,
            "不得先 commit 后发现来源游标耗尽"
        );
        assert_eq!(world.mirror_source(), source);
        assert_eq!(world.pending_mirror_domains(), pending);
        world.shutdown().unwrap();
    }
    #[test]
    fn defer_due_revision_exhaustion_rejects_before_send() {
        let mut world = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        world.non_tick_revision = u64::MAX;
        let source = world.mirror_source();
        let pending = world.pending_mirror_domains();
        let before = accounts(&mut world);
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            world.defer_due_group_for_test(0, vec![0], 99);
        }));
        assert!(rejected.is_err(), "耗尽必须在发送测试控制消息前显式拒绝");
        assert_eq!(world.mirror_source(), source);
        assert_eq!(world.pending_mirror_domains(), pending);
        assert_eq!(accounts(&mut world), before);
        world.shutdown().unwrap();
    }

    fn accounts(w: &mut World) -> Vec<Vec<GroupAccount>> {
        for tx in &w.group_txs {
            tx.send(GroupIn::EchoAccounts).unwrap();
        }
        let mut result = vec![Vec::new(); w.n_groups];
        for _ in 0..w.n_groups {
            match recv_deadline(&w.up_rx, Phase::Seal, &w.deadline).unwrap() {
                UpMsg::GroupEcho { group, accounts } => result[group as usize] = accounts,
                other => panic!("账户回声错位：{other:?}"),
            }
        }
        result
    }
    fn reject(_: &AdminCandidate) -> Result<(), RuntimeError> {
        Err(RuntimeError::ProtocolInvariant {
            detail: "候选拒绝".into(),
        })
    }

    #[test]
    fn admin_01_two_groups_all_operations_persist_through_real_tick() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        let before = accounts(&mut w);
        let ops = vec![
            (
                0,
                vec![
                    dividend(0, 101),
                    AdminOperation::WithholdingTax {
                        local: 0,
                        stock: 0,
                        amount: 1,
                    },
                ],
            ),
            (
                1,
                vec![
                    AdminOperation::ShareSplit {
                        local: 1,
                        stock: 0,
                        bonus_qty: 101,
                    },
                    AdminOperation::ShareReduction {
                        local: 1,
                        stock: 0,
                        qty: 1,
                    },
                ],
            ),
        ];
        let c = w.with_admin_transaction(ops, |c| Ok(c.clone())).unwrap();
        assert_eq!(c.groups.len(), 2);
        w.run_ticks(1).unwrap();
        let after = accounts(&mut w);
        let mut expected = before;
        expected[0][0].cash += 100;
        expected[1][1].shares[0] += 100;
        assert_eq!(after, expected);
        w.shutdown().unwrap();
    }
    #[test]
    fn admin_02_rejection_restores_complete_accounts_and_new_id_retries() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        let before = accounts(&mut w);
        let mut failed_id = None;
        assert!(w
            .with_admin_transaction(credit(), |c| {
                failed_id = Some(c.id);
                reject(c)
            })
            .is_err());
        assert_eq!(accounts(&mut w), before);
        let id = w.with_admin_transaction(credit(), |c| Ok(c.id)).unwrap();
        assert_ne!(Some(id), failed_id);
        let after = accounts(&mut w);
        assert_eq!(after[1][1].cash, before[1][1].cash + 202);
    }
    #[test]
    fn admin_03_other_group_rejects_and_original_order_is_not_netted() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        let before = accounts(&mut w);
        for (operation, reason) in [
            (
                AdminOperation::WithholdingTax {
                    local: 1,
                    stock: 0,
                    amount: 2_000_000,
                },
                "扣税可用现金不足",
            ),
            (dividend(99, 1), "local 越界"),
            (
                AdminOperation::ShareSplit {
                    local: 1,
                    stock: 99,
                    bonus_qty: 1,
                },
                "stock 越界",
            ),
        ] {
            let error = w
                .with_admin_transaction(
                    vec![(0, vec![dividend(0, 1)]), (1, vec![operation])],
                    |_| Ok(()),
                )
                .unwrap_err();
            assert!(error.to_string().contains(reason), "{error}");
            assert_eq!(accounts(&mut w), before);
        }
        assert!(w
            .with_admin_transaction(
                vec![(
                    0,
                    vec![
                        AdminOperation::WithholdingTax {
                            local: 0,
                            stock: 0,
                            amount: 2_000_000
                        },
                        dividend(0, 2_000_000)
                    ]
                )],
                |_| Ok(())
            )
            .is_err());
        assert_eq!(accounts(&mut w), before);
    }
    #[test]
    fn admin_04_large_share_abort_and_aggregate_overflow() {
        let mut s = spec();
        s.initial_shares = vec![vec![vec![0]; 2]; 2];
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        let before = accounts(&mut w);
        assert!(w
            .with_admin_transaction(
                vec![(
                    1,
                    vec![
                        AdminOperation::ShareSplit {
                            local: 1,
                            stock: 0,
                            bonus_qty: u32::MAX
                        },
                        AdminOperation::ShareReduction {
                            local: 1,
                            stock: 0,
                            qty: 2_500_000_000
                        }
                    ]
                )],
                reject
            )
            .is_err());
        assert_eq!(accounts(&mut w), before);
        for (cash, held, shares, pending, op) in [
            (i64::MAX, 0, 0, 0, dividend(0, 1)),
            (i64::MAX - 100, 100, 0, 0, dividend(0, 1)),
            (
                1_000_000,
                0,
                u32::MAX,
                0,
                AdminOperation::ShareSplit {
                    local: 0,
                    stock: 0,
                    bonus_qty: 1,
                },
            ),
            (
                1_000_000,
                0,
                u32::MAX - 10,
                10,
                AdminOperation::ShareSplit {
                    local: 0,
                    stock: 0,
                    bonus_qty: 1,
                },
            ),
        ] {
            let mut s = spec();
            s.initial_cash = cash;
            s.initial_purse_held = vec![vec![held; 2]; 2];
            s.initial_shares = vec![vec![vec![shares]; 2]; 2];
            s.initial_pending_sell = vec![vec![vec![pending]; 2]; 2];
            let mut w = World::spawn(
                s,
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .unwrap();
            let before = accounts(&mut w);
            assert!(w
                .with_admin_transaction(vec![(0, vec![op])], |_| Ok(()))
                .is_err());
            assert_eq!(accounts(&mut w), before);
        }
    }
    #[test]
    fn admin_05_flat_cost_basis_and_frozen_pending_are_preserved() {
        let mut s = spec();
        s.initial_shares = vec![vec![vec![1000]; 2]; 2];
        s.initial_invested = vec![vec![vec![900_000]; 2]; 2];
        s.initial_recovered = vec![vec![vec![100_000]; 2]; 2];
        s.initial_frozen_today = vec![vec![vec![100]; 2]; 2];
        s.initial_pending_sell = vec![vec![vec![50]; 2]; 2];
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        let before = accounts(&mut w);
        let op = AdminOperation::ShareReduction {
            local: 1,
            stock: 0,
            qty: 900,
        };
        w.with_admin_transaction(vec![(1, vec![op])], |_| Ok(()))
            .unwrap();
        let after = accounts(&mut w);
        let mut expected = before.clone();
        expected[1][1].shares[0] = 100;
        assert_eq!(after, expected);
        assert!(w
            .with_admin_transaction(
                vec![(
                    1,
                    vec![AdminOperation::ShareReduction {
                        local: 1,
                        stock: 0,
                        qty: 1
                    }]
                )],
                |_| Ok(())
            )
            .is_err());
        let mut s = spec();
        s.initial_shares = vec![vec![vec![100]; 2]; 2];
        s.initial_invested = vec![vec![vec![90000]; 2]; 2];
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        let before = accounts(&mut w);
        let zero = vec![(
            0,
            vec![AdminOperation::ShareReduction {
                local: 0,
                stock: 0,
                qty: 100,
            }],
        )];
        assert!(w.with_admin_transaction(zero.clone(), reject).is_err());
        assert_eq!(accounts(&mut w), before);
        w.with_admin_transaction(zero, |_| Ok(())).unwrap();
        let a = accounts(&mut w);
        assert_eq!(a[0][0].invested_cents[0], 0);
        assert_eq!(a[0][0].recovered_cents[0], 0);
        assert!(a[0][0].ever_held[0]);
        w.with_admin_transaction(
            vec![(
                0,
                vec![AdminOperation::ShareSplit {
                    local: 0,
                    stock: 0,
                    bonus_qty: 1,
                }],
            )],
            |_| Ok(()),
        )
        .unwrap();
        assert!(!accounts(&mut w)[0][0].ever_held[0]);
    }
    #[test]
    fn admin_06_rejection_preserves_published_tick_and_next_step() {
        let mut w = World::spawn(
            traded_spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        let published = w.publication.load();
        let before = accounts(&mut w);
        assert!(before[0][0].frozen_today[0] > 0);
        let market = w.collect_desk_resting().unwrap();
        assert!(w.with_admin_transaction(credit(), reject).is_err());
        assert_eq!(w.collect_desk_resting().unwrap(), market);
        assert_eq!(accounts(&mut w), before);
        assert_eq!(w.global.committed_tick, 1);
        assert!(Arc::ptr_eq(&published, &w.publication.load()));
        w.run_ticks(1).unwrap();
        assert_eq!(w.global.committed_tick, 2);
    }
    #[test]
    fn admin_07_rejection_keeps_real_day_end_abort_legal() {
        let mut s = spec();
        s.initial_frozen_today = vec![vec![vec![100]; 2]; 2];
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        let before = accounts(&mut w);
        w.run_day_end_public().unwrap();
        assert_eq!(accounts(&mut w)[0][0].frozen_today[0], 0);
        assert!(w.with_admin_transaction(credit(), reject).is_err());
        w.abort_day_end();
        assert_eq!(accounts(&mut w), before);
    }
    #[test]
    fn admin_08_commit_closes_old_day_end_and_next_tick_abort_keeps_credit() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        w.run_day_end_public().unwrap();
        w.with_admin_transaction(credit(), |_| Ok(())).unwrap();
        let baseline = accounts(&mut w);
        w.abort_day_end();
        assert_eq!(accounts(&mut w), baseline);
        w.validator = Box::new(|_| Err("拒绝下一拍".into()));
        assert!(w.run_ticks(1).is_err());
        assert_eq!(accounts(&mut w), baseline);
        assert_eq!(w.global.committed_tick, 1);
    }
    #[test]
    fn admin_09_closed_days_without_start_keep_first_commit() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.with_admin_transaction(credit(), |_| Ok(())).unwrap();
        let baseline = accounts(&mut w);
        assert!(w.with_admin_transaction(credit(), reject).is_err());
        assert_eq!(accounts(&mut w), baseline);
        w.run_ticks(1).unwrap();
        assert_eq!(accounts(&mut w), baseline);
    }
    #[test]
    fn admin_12_seal_commit_drain_abort_restores_orders_and_resources() {
        let mut s = spec();
        s.script_for = Box::new(|g| {
            Box::new(move |tick, _| {
                let mut p = TickPlan::default();
                if tick == 1 && g == 0 {
                    p.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: Side::Buy,
                        price: 900,
                        qty: 100,
                    });
                }
                p
            })
        });
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        w.with_admin_transaction(credit(), |_| Ok(())).unwrap();
        let baseline = accounts(&mut w);
        let orders = w.collect_desk_resting().unwrap();
        assert_eq!(orders.0[0].len(), 1);
        assert!(baseline[0][0].buy_purse_held_cents > 0);
        w.run_day_end_public().unwrap();
        assert_eq!(accounts(&mut w)[0][0].buy_purse_held_cents, 0);
        w.abort_day_end();
        assert_eq!(accounts(&mut w), baseline);
        assert_eq!(w.collect_desk_resting().unwrap(), orders);
        w.run_ticks(1).unwrap();
        let report = w.shutdown().unwrap();
        assert!(report.worker_results.iter().all(|(_, r)| r.is_ok()));
    }
    #[test]
    fn admin_13_candidate_panic_keeps_payload_joins_and_kills_world() {
        let mut w = World::spawn(
            traded_spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        let published = w.publication.load();
        let baseline = accounts(&mut w);
        assert!(baseline[0][0].frozen_today[0] > 0);
        let marker = Arc::new(17_u8);
        let payload = marker.clone();
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _: Result<(), _> =
                w.with_admin_transaction(credit(), |_| std::panic::panic_any(payload));
        }))
        .unwrap_err();
        assert!(Arc::ptr_eq(err.downcast_ref::<Arc<u8>>().unwrap(), &marker));
        assert!(w.handles.is_empty());
        assert!(w.dead_reason.is_some());
        assert!(w.run_ticks(1).is_err());
        assert!(w.run_day_end_public().is_err());
        assert!(w.with_admin_transaction(credit(), |_| Ok(())).is_err());
        assert_eq!(w.global.committed_tick, 1);
        assert!(Arc::ptr_eq(&published, &w.publication.load()));
        let report = w.dead_finals.as_ref().unwrap();
        for (group, expected) in report.group_finals.iter().zip(&baseline) {
            assert_eq!(&group.accounts, expected);
        }
    }
    #[test]
    fn admin_10_partial_prepare_and_commit_send_failure_kill_and_join() {
        for phase in [AdminPhase::Prepare, AdminPhase::Commit] {
            let mut w = World::spawn(
                spec(),
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .unwrap();
            w.run_ticks(1).unwrap();
            w.run_day_end_public().unwrap();
            let published = w.publication.load();
            w.admin_send_fault_for_test = Some((phase, 1));
            let mut inspected = false;
            assert!(w
                .with_admin_transaction(credit(), |_| {
                    inspected = true;
                    Ok(())
                })
                .is_err());
            assert_eq!(inspected, phase == AdminPhase::Commit);
            assert!(w.handles.is_empty());
            assert!(w.dead_reason.is_some());
            assert!(w.run_ticks(1).is_err());
            assert!(Arc::ptr_eq(&published, &w.publication.load()));
            if phase == AdminPhase::Commit {
                assert!(w.day_end_prior_public_views.is_none());
            }
        }
    }
    #[test]
    fn admin_11_real_channel_receipt_id_group_locals_and_missing_are_fatal() {
        for phase in [AdminPhase::Prepare, AdminPhase::Commit, AdminPhase::Abort] {
            for fault in [
                AdminFault::WrongId,
                AdminFault::WrongGroup,
                AdminFault::WrongLocals,
                AdminFault::Missing,
                AdminFault::Duplicate,
            ] {
                let mut w = World::spawn(
                    spec(),
                    crate::calendar::TradingCalendar::current_default_calendar()
                        .expect("显式 raw Calendar 政策合法"),
                )
                .unwrap();
                w.run_ticks(1).unwrap();
                let published = w.publication.load();
                w.group_txs[0]
                    .send(GroupIn::AdminFaultForTest {
                        phase,
                        fault: fault.clone(),
                    })
                    .unwrap();
                // 重复回执的另一参与者故意缺失，不能用重复计数填补缺组。
                if fault == AdminFault::Duplicate {
                    w.group_txs[1]
                        .send(GroupIn::AdminFaultForTest {
                            phase,
                            fault: AdminFault::Missing,
                        })
                        .unwrap();
                }
                let mut published_candidate = false;
                let result = w.with_admin_transaction(credit(), |c| {
                    if phase == AdminPhase::Abort {
                        reject(c)
                    } else {
                        Ok(())
                    }
                });
                if result.is_ok() {
                    published_candidate = true;
                }
                assert!(result.is_err(), "{phase:?}/{fault:?}");
                assert!(!published_candidate);
                assert!(w.handles.is_empty());
                assert!(w.dead_reason.is_some());
                assert!(w.run_ticks(1).is_err());
                assert!(Arc::ptr_eq(&published, &w.publication.load()));
            }
        }
    }
    #[test]
    fn admin_13_worker_prepare_and_commit_panic_transport_original_and_join() {
        for phase in [AdminPhase::Prepare, AdminPhase::Commit] {
            let mut w = World::spawn(
                traded_spec(),
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .unwrap();
            w.run_ticks(1).unwrap();
            let published = w.publication.load();
            let baseline = accounts(&mut w);
            assert!(baseline[0][0].frozen_today[0] > 0);
            let marker = Arc::new(format!("行政 worker panic {phase:?}"));
            w.group_txs[1]
                .send(GroupIn::AdminFaultForTest {
                    phase,
                    fault: AdminFault::Panic(Arc::clone(&marker)),
                })
                .unwrap();
            let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                w.with_admin_transaction(credit(), |_| Ok(()))
            }))
            .unwrap_err();
            assert!(Arc::ptr_eq(
                err.downcast_ref::<Arc<String>>().unwrap(),
                &marker
            ));
            assert!(w.handles.is_empty());
            assert!(w.dead_reason.is_some());
            assert!(w.with_admin_transaction(credit(), |_| Ok(())).is_err());
            assert!(Arc::ptr_eq(&published, &w.publication.load()));
            let report = w.dead_finals.as_ref().unwrap();
            assert_eq!(report.group_finals.len(), 2);
            for (group, before) in report.group_finals.iter().zip(&baseline) {
                for (account, expected) in group.accounts.iter().zip(before) {
                    assert_eq!(account.shares, expected.shares);
                    assert_eq!(account.frozen_today, expected.frozen_today);
                    assert_eq!(account.invested_cents, expected.invested_cents);
                    assert_eq!(account.recovered_cents, expected.recovered_cents);
                }
            }
            assert_eq!(report.group_finals[1].accounts, baseline[1]);
            if phase == AdminPhase::Prepare {
                assert_eq!(report.group_finals[0].accounts, baseline[0]);
            }
        }
    }
    #[test]
    fn admin_08_partial_start_does_not_undo_committed_admin_or_tick() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        w.run_day_end_public().unwrap();
        w.with_admin_transaction(credit(), |_| Ok(())).unwrap();
        let baseline = accounts(&mut w);
        let published = w.publication.load();
        let (dead, rx) = channel();
        drop(rx);
        let real = std::mem::replace(&mut w.group_txs[1], dead);
        assert!(w.run_ticks(1).is_err());
        drop(real);
        assert!(w.handles.is_empty());
        w.abort_day_end();
        for (group, expected) in w
            .dead_finals
            .as_ref()
            .unwrap()
            .group_finals
            .iter()
            .zip(&baseline)
        {
            assert_eq!(&group.accounts, expected);
        }
        assert!(Arc::ptr_eq(&published, &w.publication.load()));
        assert_eq!(w.global.committed_tick, 1);
    }
    #[test]
    fn admin_05_flat_reduction_rejects_real_inflight_buy_and_real_rebuy_starts_new_cost() {
        let mut s = spec();
        s.initial_invested = vec![vec![vec![900_000]; 2]; 2];
        s.script_for = Box::new(|group| {
            Box::new(move |tick, _| {
                let mut p = TickPlan::default();
                if tick == 1 && group == 0 {
                    p.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: Side::Buy,
                        price: 980,
                        qty: 100,
                    });
                }
                p
            })
        });
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        let before = accounts(&mut w);
        assert!(before[0][0].foreign_active[0] > 0);
        assert!(w
            .with_admin_transaction(
                vec![(
                    0,
                    vec![AdminOperation::ShareReduction {
                        local: 0,
                        stock: 0,
                        qty: 1000
                    }]
                )],
                |_| Ok(())
            )
            .is_err());
        assert_eq!(accounts(&mut w), before);
        let mut s = spec();
        s.initial_invested = vec![vec![vec![900_000]; 2]; 2];
        s.script_for = Box::new(|group| {
            Box::new(move |tick, _| {
                let mut p = TickPlan::default();
                if tick == 1 {
                    p.places.push(PlaceSpec {
                        local: if group == 0 { 0 } else { 1 },
                        stock: 0,
                        side: if group == 0 { Side::Buy } else { Side::Sell },
                        price: 1000,
                        qty: 100,
                    });
                }
                p
            })
        });
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.with_admin_transaction(
            vec![(
                0,
                vec![AdminOperation::ShareReduction {
                    local: 0,
                    stock: 0,
                    qty: 1000,
                }],
            )],
            |_| Ok(()),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        let a = accounts(&mut w);
        assert_eq!(a[0][0].shares[0], 100);
        assert_eq!(a[0][0].invested_cents[0], 100_000);
        assert!(!a[0][0].ever_held[0]);
    }
    #[test]
    fn admin_04_exact_aggregate_boundary_and_invalid_values() {
        let mut s = spec();
        s.initial_cash = i64::MAX - 101;
        s.initial_purse_held = vec![vec![100; 2]; 2];
        s.initial_shares = vec![vec![vec![u32::MAX - 11]; 2]; 2];
        s.initial_pending_sell = vec![vec![vec![10]; 2]; 2];
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.with_admin_transaction(
            vec![(
                0,
                vec![
                    dividend(0, 1),
                    AdminOperation::ShareSplit {
                        local: 0,
                        stock: 0,
                        bonus_qty: 1,
                    },
                ],
            )],
            |_| Ok(()),
        )
        .unwrap();
        let a = accounts(&mut w);
        assert_eq!(
            a[0][0].cash.checked_add(a[0][0].buy_purse_held_cents),
            Some(i64::MAX)
        );
        assert_eq!(
            a[0][0].shares[0].checked_add(a[0][0].pending_sell_qty[0]),
            Some(u32::MAX)
        );
        for op in [
            dividend(0, 0),
            dividend(0, -1),
            AdminOperation::WithholdingTax {
                local: 0,
                stock: 0,
                amount: 0,
            },
            AdminOperation::WithholdingTax {
                local: 0,
                stock: 0,
                amount: -1,
            },
            AdminOperation::ShareSplit {
                local: 0,
                stock: 0,
                bonus_qty: 0,
            },
            AdminOperation::ShareReduction {
                local: 0,
                stock: 0,
                qty: 0,
            },
        ] {
            let mut w = World::spawn(
                spec(),
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .unwrap();
            let before = accounts(&mut w);
            assert!(w
                .with_admin_transaction(vec![(0, vec![op])], |_| Ok(()))
                .is_err());
            assert_eq!(accounts(&mut w), before);
        }
    }
    #[test]
    fn admin_01_normal_npc_decisions_continue_after_credit() {
        use crate::runtime::decision::{DecisionSetup, NpcProfile};
        let mut s = spec();
        s.initial_shares = vec![vec![vec![0]; 2]; 2];
        s.decision = Some(DecisionSetup {
            profiles: (0..4)
                .map(|seed| NpcProfile {
                    strategy: crate::strategy::StrategyData::retail(1.0, 100, 1.0),
                    seed,
                    next_due: 0,
                    attention_base: 1.0,
                    retail_style: None,
                })
                .collect(),
            attention_gap_ticks: 0,
        });
        let mut w = World::spawn(
            s,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.with_admin_transaction(credit(), |_| Ok(())).unwrap();
        w.run_ticks(3).unwrap();
        let attention = w.collect_group_attention().unwrap();
        assert_eq!(attention.len(), 2);
        assert!(attention
            .iter()
            .all(|(_, a)| a.len() == 2 && a.iter().all(|(_, due, _)| *due > 3)));
        assert!(w
            .report_log
            .iter()
            .any(|t| t.desk_resting.iter().any(|(_, n)| *n > 0)));
        let r = w.shutdown().unwrap();
        assert!(r.group_finals.iter().all(|g| g.decide_calls_total >= 3));
        let cash: i64 = r
            .group_finals
            .iter()
            .flat_map(|g| &g.accounts)
            .map(|a| a.cash)
            .sum();
        let fees: i64 = r.desk_finals.iter().map(|d| d.fee_sink).sum();
        assert_eq!(cash + fees, 4_000_303);
    }
    #[test]
    fn admin_00_empty_duplicate_group_and_id_exhaustion_are_explicit() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        let before = accounts(&mut w);
        let mut calls = 0;
        w.with_admin_transaction(vec![(1, Vec::new())], |c| {
            calls += 1;
            assert!(c.groups.is_empty());
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(accounts(&mut w), before);
        for ops in [
            vec![(0, vec![dividend(0, 1)]), (0, vec![dividend(1, 1)])],
            vec![(9, vec![dividend(0, 1)])],
        ] {
            assert!(w.with_admin_transaction(ops, |_| Ok(())).is_err());
            assert_eq!(accounts(&mut w), before);
        }
        w.admin_next_id = u64::MAX;
        assert!(w.with_admin_transaction(credit(), |_| Ok(())).is_err());
        assert_eq!(accounts(&mut w), before);
    }
    #[test]
    fn admin_11_pending_rejects_start_continue_and_day_end_without_market_rollback() {
        for mode in 0..3 {
            let mut w = World::spawn(
                spec(),
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .unwrap();
            w.run_ticks(1).unwrap();
            let before = accounts(&mut w);
            w.group_txs[0]
                .send(GroupIn::AdminPrepare {
                    id: AdminTransactionId(91),
                    operations: vec![dividend(0, 1)],
                    stage_layout: None,
                    information: None,
                    repurchase: None,
                    information_clock: None,
                })
                .unwrap();
            assert!(matches!(
                recv_deadline(&w.up_rx, Phase::Seal, &w.deadline).unwrap(),
                UpMsg::AdminReceipt { result: Ok(_), .. }
            ));
            let message = match mode {
                0 => GroupIn::Start {
                    tick: 2,
                    snapshot: Arc::new(crate::runtime::protocol::TickSnapshot {
                        repurchase_eligibility: Default::default(),
                        calendar: w.prepare_raw_calendar_tick().unwrap(),
                        tick: 2,
                        civil_date: w.civil_date,
                        market_minute: 0,
                        stocks: Arc::new(w.public_views.clone()),
                    }),
                },
                1 => GroupIn::Continue { tick: 1, round: 1 },
                _ => GroupIn::DayEnd(Box::new(crate::runtime::protocol::DayEndDirective {
                    tick: 1,
                    day: 0,
                    civil_date: w.civil_date,
                    calendar: crate::runtime::protocol::DayEndCalendar::ManualDayEnd(
                        w.calendar_source.day(w.civil_date).unwrap(),
                    ),
                    rights: Vec::new(),
                })),
            };
            w.group_txs[0].send(message).unwrap();
            match (
                mode,
                recv_deadline(&w.up_rx, Phase::Seal, &w.deadline).unwrap(),
            ) {
                (0, UpMsg::PendingStartRejected(rejection)) => {
                    assert_eq!(rejection.group, 0);
                    assert_eq!((rejection.tick, rejection.prior_tick), (2, 1));
                    assert_eq!(rejection.calendar.before_input().tick, 1);
                    assert_eq!(rejection.calendar.after_input().tick, 2);
                    assert!(
                        matches!(rejection.error, RuntimeError::ProtocolInvariant { detail } if detail.contains("行政事务 pending"))
                    );
                }
                (
                    1 | 2,
                    UpMsg::Failed {
                        who: WorkerId::Group(0),
                        error,
                    },
                ) => {
                    assert!(
                        matches!(error, RuntimeError::ProtocolInvariant { detail } if detail.contains("行政事务 pending"))
                    );
                }
                (_, other) => panic!("pending门须保原错误与精确来源阶段：{other:?}"),
            }
            match recv_deadline(&w.up_rx, Phase::Seal, &w.deadline).unwrap() {
                UpMsg::GroupFinal(g) => assert_eq!(g.accounts, before[0]),
                other => panic!("期望Final，收到{other:?}"),
            }
        }
    }
    #[test]
    fn admin_review_severed_entry_joins_and_kills_even_empty_transaction() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.sever();
        assert!(w.with_admin_transaction(Vec::new(), |_| Ok(())).is_err());
        assert!(w.dead_reason.is_some());
        assert!(w.handles.is_empty());
        assert!(w.run_ticks(1).is_err());
    }
    #[test]
    fn admin_review_prepare_terminal_closes_old_day_end_abort_window() {
        let mut w = World::spawn(
            spec(),
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        w.run_ticks(1).unwrap();
        w.run_day_end_public().unwrap();
        w.admin_send_fault_for_test = Some((AdminPhase::Prepare, 1));
        assert!(w.with_admin_transaction(credit(), |_| Ok(())).is_err());
        let views = w.public_views.clone();
        let published = w.publication.load();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.abort_day_end())).is_ok()
        );
        assert_eq!(w.public_views, views);
        assert!(Arc::ptr_eq(&published, &w.publication.load()));
        assert!(w.handles.is_empty());
    }
    #[test]
    fn admin_review_already_queued_extra_receipt_prevents_success() {
        for phase in [AdminPhase::Prepare, AdminPhase::Commit, AdminPhase::Abort] {
            let mut w = World::spawn(
                spec(),
                crate::calendar::TradingCalendar::current_default_calendar()
                    .expect("显式 raw Calendar 政策合法"),
            )
            .unwrap();
            let (tx, rx) = channel();
            tx.send(UpMsg::AdminReceipt {
                id: AdminTransactionId(1),
                group: 0,
                phase,
                result: Err(invalid("确定已入队的额外回执")),
            })
            .unwrap();
            drop(tx);
            w.up_rx = rx;
            assert!(
                w.control_admin(AdminTransactionId(1), phase, &BTreeMap::new())
                    .is_err(),
                "{phase:?}收齐空集合后不应忽略已排队额外消息"
            );
        }
    }
}

#[cfg(test)]
mod information_tests;

#[cfg(all(test, not(target_family = "wasm")))]
mod calendar_information_intersection_tests;

#[cfg(test)]
mod funds_cost_tests;

#[cfg(test)]
mod repurchase_funds_tests;

#[cfg(test)]
mod repurchase_approval_tests;
