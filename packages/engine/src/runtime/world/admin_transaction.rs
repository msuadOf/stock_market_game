use super::*;
use crate::runtime::protocol::{AdminGroupCandidate, AdminPhase};
use std::collections::{BTreeMap, BTreeSet};

type PanicPayload = Box<dyn std::any::Any + Send>;
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

impl World {
    /// 同步准备、校验并提交行政账务；宿主仅可在成功返回后发布候选。
    /// inspect 只校验候选和构造待发布值，不执行 observer、I/O 或 session 安装。
    /// 业务拒绝仅撤本事务；通信、协议或 panic 失败永久终止并回收全部 worker。
    pub fn with_admin_transaction<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, RuntimeError> {
        self.with_admin_scope(grouped, None, inspect)
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
        self.with_admin_scope(grouped, Some(information), inspect)
    }

    fn with_admin_scope<P>(
        &mut self,
        grouped: Vec<(GroupIdx, Vec<AdminOperation>)>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, RuntimeError> {
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
        self.admin_next_id = self
            .admin_next_id
            .checked_add(1)
            .ok_or_else(|| invalid("行政事务 id 已耗尽"))?;
        let id = AdminTransactionId(self.admin_next_id);
        let mut operations = BTreeMap::new();
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
            operations.retain(|_, ops| !ops.is_empty());
        }
        let expected: BTreeMap<_, Vec<_>> = operations
            .iter()
            .map(|(&group, ops)| {
                (
                    group,
                    ops.iter()
                        .map(|op| op.coordinates().0)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                )
            })
            .collect();
        let mut sent = BTreeMap::new();
        match self.execute_admin(id, operations, information, &expected, &mut sent, inspect) {
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
                    self.worker_shutdown.store(true, Ordering::SeqCst);
                    for (who, handle) in std::mem::take(&mut self.handles) {
                        let diagnostic = match handle.join() {
                            Ok(Ok(())) => None,
                            Ok(Err(error)) => Some(format!("；收尾 join {who} 失败：{error}")),
                            Err(_) => Some(format!("；收尾 join {who} 再次 panic")),
                        };
                        if let Some(diagnostic) = diagnostic {
                            self.dead_reason
                                .as_mut()
                                .expect("已判死")
                                .push_str(&diagnostic);
                        }
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
        if failure.terminal {
            return Err(invalid(
                self.dead_reason.as_ref().expect("终止失败已判死").clone(),
            ));
        }
        Err(failure.error)
    }

    fn execute_admin<P>(
        &mut self,
        id: AdminTransactionId,
        operations: BTreeMap<GroupIdx, Vec<AdminOperation>>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        expected: &BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
        sent: &mut BTreeMap<GroupIdx, Vec<super::super::LocalIdx>>,
        inspect: impl FnOnce(&AdminCandidate) -> Result<P, RuntimeError>,
    ) -> Result<P, AdminFailure> {
        for (&group, ops) in &operations {
            self.send_admin(
                group,
                AdminPhase::Prepare,
                GroupIn::AdminPrepare {
                    id,
                    operations: ops.clone(),
                    information: information.clone(),
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
                    self.validate_information_candidate(&candidate, information.as_ref())?;
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
        let candidate = AdminCandidate {
            id,
            groups: prepared.into_values().collect(),
        };
        let value =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inspect(&candidate))) {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => {
                    self.abort_admin_preserving_error(id, expected, &error)?;
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
        let domains = candidate.groups.iter().fold(crate::runtime::source::MirrorDomains::default(), |all, group| all.union(group.mirror_domains));
        let next_revision = if domains.is_empty() {
            self.non_tick_revision
        } else {
            match self.non_tick_revision.checked_add(1) {
                Some(revision) => revision,
                None => {
                    let error = invalid("非 tick 成功来源游标已耗尽，拒绝首个 commit");
                    self.abort_admin_preserving_error(id, expected, &error)?;
                    return Err(AdminFailure { error, payload: None, terminal: false, committing: false });
                }
            }
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
        self.non_tick_revision = next_revision;
        self.pending_mirror_domains = self.pending_mirror_domains.union(domains);
        Ok(value)
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
                        self.information_roster.get(candidate.group as usize).and_then(|roster| roster.get(*local as usize)) != Some(account)
                            || snapshot.information.owner() != *account || snapshot.belief.npc() != *account
                    })
                    || (payload.observation.is_none() && !info.personals.is_empty())
                {
                    return Err(invalid(format!("组 {} 信息候选回执身份/输入不一致", candidate.group)));
                }
                Ok(())
            }
            _ => Err(invalid(format!("组 {} 信息候选作用域错配", candidate.group))),
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
        match self.up_rx.try_recv() {
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
        match recv_deadline(&self.up_rx, Phase::Seal, &self.watchdog)? {
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
            match recv_deadline(&self.up_rx, Phase::Seal, &self.watchdog) {
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
            for (who, handle) in std::mem::take(&mut self.handles) {
                match handle.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => diagnostics.push(format!("{who}：{error}")),
                    Err(p) => {
                        diagnostics.push(format!("{who} 未运输 panic"));
                        if payload.is_none() {
                            payload = Some(p);
                        }
                    }
                }
            }
        }
        (payload, diagnostics.join("；"))
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
        spec.watchdog = WatchdogConfig {
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
    fn successful_non_tick_writes_advance_source_without_published_tick() {
        let mut world = World::spawn(spec()).unwrap();
        let initial = world.mirror_source();
        for revision in 1..=2 {
            world.with_admin_transaction(credit(), |_| Ok(())).unwrap();
            let source = world.mirror_source();
            assert_eq!((source.tick, source.generation), (initial.tick, initial.generation));
            assert_eq!(source.non_tick_revision, revision,
                "成功 Admin 不是 attempt id；同 tick 连续写必须有独立成功来源");
            assert!(world.pending_mirror_domains().accounts_full);
        }
        let source = world.mirror_source();
        let pending = world.pending_mirror_domains();
        let rejected = world.with_admin_transaction(credit(), |_| Err::<(), _>(invalid("纯 inspect 业务拒绝")));
        assert!(rejected.is_err());
        assert_eq!(world.mirror_source(), source, "失败不推进成功来源");
        assert_eq!(world.pending_mirror_domains(), pending, "失败不清旧 dirty 原因");
        world.shutdown().unwrap();
    }

    #[test]
    fn non_tick_revision_exhaustion_rejects_before_first_commit() {
        let mut world = World::spawn(spec()).unwrap();
        world.non_tick_revision = u64::MAX;
        let before = accounts(&mut world);
        let source = world.mirror_source();
        let pending = world.pending_mirror_domains();
        let rejected = world.with_admin_transaction(credit(), |_| Ok(()));
        assert!(rejected.is_err(), "耗尽必须在首个不可撤 commit 前拒绝");
        assert_eq!(accounts(&mut world), before, "不得先 commit 后发现来源游标耗尽");
        assert_eq!(world.mirror_source(), source);
        assert_eq!(world.pending_mirror_domains(), pending);
        world.shutdown().unwrap();
    }
    fn accounts(w: &mut World) -> Vec<Vec<GroupAccount>> {
        for tx in &w.group_txs {
            tx.send(GroupIn::EchoAccounts).unwrap();
        }
        let mut result = vec![Vec::new(); w.n_groups];
        for _ in 0..w.n_groups {
            match recv_deadline(&w.up_rx, Phase::Seal, &w.watchdog).unwrap() {
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
        let mut w = World::spawn(spec()).unwrap();
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
        let mut w = World::spawn(spec()).unwrap();
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
        let mut w = World::spawn(spec()).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
            let mut w = World::spawn(s).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
        let mut w = World::spawn(traded_spec()).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
        let mut w = World::spawn(spec()).unwrap();
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
        let mut w = World::spawn(spec()).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
        let mut w = World::spawn(traded_spec()).unwrap();
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
            let mut w = World::spawn(spec()).unwrap();
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
                let mut w = World::spawn(spec()).unwrap();
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
            let mut w = World::spawn(traded_spec()).unwrap();
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
        let mut w = World::spawn(spec()).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
            let mut w = World::spawn(spec()).unwrap();
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
        let mut w = World::spawn(s).unwrap();
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
        let mut w = World::spawn(spec()).unwrap();
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
            let mut w = World::spawn(spec()).unwrap();
            w.run_ticks(1).unwrap();
            let before = accounts(&mut w);
            w.group_txs[0]
                .send(GroupIn::AdminPrepare {
                    id: AdminTransactionId(91),
                    operations: vec![dividend(0, 1)],
                    information: None,
                })
                .unwrap();
            assert!(matches!(
                recv_deadline(&w.up_rx, Phase::Seal, &w.watchdog).unwrap(),
                UpMsg::AdminReceipt { result: Ok(_), .. }
            ));
            let message = match mode {
                0 => GroupIn::Start {
                    tick: 2,
                    snapshot: Arc::new(crate::runtime::protocol::TickSnapshot {
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
                    rights: Vec::new(),
                })),
            };
            w.group_txs[0].send(message).unwrap();
            assert!(matches!(
                recv_deadline(&w.up_rx, Phase::Seal, &w.watchdog).unwrap(),
                UpMsg::Failed { .. }
            ));
            match recv_deadline(&w.up_rx, Phase::Seal, &w.watchdog).unwrap() {
                UpMsg::GroupFinal(g) => assert_eq!(g.accounts, before[0]),
                other => panic!("期望Final，收到{other:?}"),
            }
        }
    }
    #[test]
    fn admin_review_severed_entry_joins_and_kills_even_empty_transaction() {
        let mut w = World::spawn(spec()).unwrap();
        w.sever();
        assert!(w.with_admin_transaction(Vec::new(), |_| Ok(())).is_err());
        assert!(w.dead_reason.is_some());
        assert!(w.handles.is_empty());
        assert!(w.run_ticks(1).is_err());
    }
    #[test]
    fn admin_review_prepare_terminal_closes_old_day_end_abort_window() {
        let mut w = World::spawn(spec()).unwrap();
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
            let mut w = World::spawn(spec()).unwrap();
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
