use super::*;
#[cfg(test)]
use crate::runtime::protocol::AdminFault;
use crate::runtime::protocol::{
    AdminGroupCandidate, AdminOperation, AdminPhase, AdminTransactionId,
};
use std::collections::BTreeMap;
mod repurchase;
mod stages;
use repurchase::PendingRepurchase;

/// 仅保留最后成功观察的有限输入；Arc 按内容比较，不以地址或 hash 作为相等依据。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct InformationObservationSignature {
    through: crate::calendar::CivilInstant,
    library: Arc<crate::information::PublicLibrary>,
    facts: Arc<crate::company::CompanyReadFacts>,
    market: Arc<crate::strategy::MarketView>,
    anchors:
        BTreeMap<crate::StockId, crate::observation::information_clock::InformationTradingAnchor>,
    owners_and_codes:
        BTreeMap<LocalIdx, (crate::AccountId, std::collections::BTreeSet<crate::StockId>)>,
}

/// 单次 Prepare 的行政写集与认证信息；Commit/Abort 仍传入原空输入。
pub(super) struct AdminPrepareInput {
    pub(super) operations: Vec<AdminOperation>,
    pub(super) stage_layout: Option<crate::runtime::protocol::AdminStageLayout>,
    pub(super) information: Option<crate::runtime::protocol::InformationPreparePayload>,
    pub(super) information_clock: Option<crate::runtime::world::AuthenticatedInformationClock>,
    pub(super) repurchase: Option<Vec<crate::runtime::protocol::RepurchaseProjectionInput>>,
}

/// 只保留 touched 账户前值，不借用有 i32 股数差额的普通日志。
pub(super) struct PendingAdmin {
    id: AdminTransactionId,
    before: Vec<(LocalIdx, Arc<GroupAccount>)>,
    stages: Option<crate::runtime::protocol::PreparedGroupAdminStages>,
    information: Option<PendingInformation>,
    repurchase: Option<PendingRepurchase>,
}

fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: detail.into(),
    }
}

impl GroupState {
    pub(super) fn capture_information_handover(
        &self,
        source: crate::runtime::source::WorldMirrorSource,
    ) -> Result<super::super::type_aliases::InformationHandoverCapture, RuntimeError> {
        use crate::runtime::information_handover::{
            GroupInformationHandover, InformationHandoverMaterial, InformationHandoverSignature,
        };
        let sealed_static = self.seal_acked_len.is_some()
            && self.batches_after_seal == 0
            && self.staging_deferred.is_empty();
        if source.tick != self.cur_tick
            || self.admin_pending.is_some()
            || !self.staging_deferred.is_empty()
            || self.batches_after_seal != 0
            || (self.cur_tick != self.committed_tick && !sealed_static)
        {
            return Err(invalid(format!(
                "INFO 移交捕获不是已提交静点：group={} source={source:?}",
                self.group
            )));
        }
        let root = self
            .root_projection
            .as_ref()
            .ok_or_else(|| invalid("INFO 移交捕获缺真实名册"))?;
        let roster: Vec<_> = root
            .account_ids
            .iter()
            .skip(self.roster_offset)
            .take(self.accounts.len())
            .copied()
            .collect();
        let signature = self.information_signature.as_ref().map(|signature| {
            if Some(signature.through) != self.information_observed_through {
                return Err(invalid("INFO 移交签名 through 与观察水位不同"));
            }
            let mut owners_and_codes = BTreeMap::new();
            for (&local, (owner, codes)) in &signature.owners_and_codes {
                if roster.get(local as usize) != Some(owner) {
                    return Err(invalid(format!("INFO 移交签名 local/AccountId 不属于真实名册：group={} local={local} owner={owner:?}", self.group)));
                }
                if owners_and_codes.insert(*owner, codes.clone()).is_some() {
                    return Err(invalid(format!("INFO 移交签名重复 AccountId={owner:?}")));
                }
            }
            Ok(InformationHandoverSignature {
                material: Arc::new(InformationHandoverMaterial {
                    through: signature.through,
                    library: Arc::clone(&signature.library),
                    facts: Arc::clone(&signature.facts),
                    market: Arc::clone(&signature.market),
                    anchors: signature.anchors.clone(),
                }),
                owners_and_codes,
            })
        }).transpose()?;
        let profiles = match &self.decision {
            Some((profiles, _)) => {
                if profiles.len() < roster.len() {
                    return Err(invalid("INFO 移交捕获 profile 缺少真实账户槽"));
                }
                roster
                    .iter()
                    .zip(profiles)
                    .map(|(&owner, profile)| (owner, profile.next_due, profile.seed))
                    .collect()
            }
            None => Vec::new(),
        };
        Ok((
            GroupInformationHandover {
                observed_through: self.information_observed_through,
                signature,
            },
            Arc::clone(&self.library),
            Arc::clone(self.current_company_facts()?),
            profiles,
        ))
    }

    /// 原始账户、profile 和 INFO checkpoint 是同一完整初值；全部校验后才 move 安装。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn install_initial_information_handover(
        &mut self,
        source: crate::runtime::source::WorldMirrorSource,
        roster: &[(crate::AccountId, bool)],
        handover: crate::runtime::information_handover::GroupInformationHandover,
        library: Arc<crate::information::PublicLibrary>,
        facts: Arc<crate::company::CompanyReadFacts>,
        accounts: Vec<(crate::AccountId, GroupAccount)>,
        personals: Vec<(
            crate::AccountId,
            Option<crate::runtime::world::BeliefParticipantSnapshot>,
        )>,
        attention: Vec<(crate::AccountId, u64, u64)>,
        inherited_calendar: Option<crate::runtime::world::InheritedCalendarCommit>,
    ) -> Result<(), RuntimeError> {
        if self.information_handover_installed
            || self.snapshot.is_some()
            || self.admin_last_id != 0
            || self.admin_pending.is_some()
            || self.cur_tick != self.committed_tick
            || source.tick != self.cur_tick
            || self.information_signature.is_some()
            || self.information_observed_through.is_some()
            || !self.staging_deferred.is_empty()
            || self.batches_after_seal != 0
            || !self.external_pending.is_empty()
            || !self.external_cancels.is_empty()
            || !self.pending_force_due.is_empty()
            || !self.observed.is_empty()
        {
            return Err(invalid(format!(
                "完整移交初值只能在首个 Start 前安装一次：group={}",
                self.group
            )));
        }
        let root = self
            .root_projection
            .as_ref()
            .ok_or_else(|| invalid("完整移交初值缺真实名册"))?;
        if let Some(inherited) = &inherited_calendar {
            let (owner, bindings) = self
                .calendar_owner
                .as_ref()
                .ok_or_else(|| invalid("Calendar 移交缺组构造 owner"))?;
            inherited.validate_install(
                owner,
                bindings,
                crate::session::exchange_calendar_model::SessionTickLayout {
                    ticks_per_day: self.ticks_per_day,
                    auction_ticks: self.auction_ticks,
                    closing_auction_ticks: self.closing_auction_ticks,
                },
                source,
            )?;
        }
        let expected: Vec<_> = root
            .account_ids
            .iter()
            .skip(self.roster_offset)
            .take(self.accounts.len())
            .copied()
            .collect();
        if expected != roster.iter().map(|(owner, _)| *owner).collect::<Vec<_>>()
            || accounts.len() != roster.len()
            || personals
                .iter()
                .map(|(owner, _)| *owner)
                .collect::<Vec<_>>()
                != expected
        {
            return Err(invalid(format!(
                "完整移交初值与真实组名册不同：group={}",
                self.group
            )));
        }
        for (local, ((owner, required), (account_owner, account))) in
            roster.iter().zip(&accounts).enumerate()
        {
            if account_owner != owner {
                return Err(invalid(format!(
                    "完整移交账户 owner 错位：group={} local={local}",
                    self.group
                )));
            }
            let personal = self
                .belief_states
                .get(local)
                .ok_or_else(|| invalid("完整移交个人态缺真实槽"))?;
            let incoming = &personals[local].1;
            if personal.is_some() != *required
                || incoming.is_some() != *required
                || incoming.as_ref().is_some_and(|personal| {
                    personal.information.owner() != *owner || personal.belief.npc() != *owner
                })
            {
                return Err(invalid(format!(
                    "完整移交个人态与 Required 名册不同：group={} owner={owner:?}",
                    self.group
                )));
            }
            let stocks = root.catalog.len();
            if [
                account.shares.len(),
                account.frozen_today.len(),
                account.foreign_active.len(),
                account.invested_cents.len(),
                account.recovered_cents.len(),
                account.ever_held.len(),
                account.pending_sell_qty.len(),
            ]
            .iter()
            .any(|&len| len != stocks)
                || account
                    .frozen_today
                    .iter()
                    .zip(&account.shares)
                    .any(|(&frozen, &shares)| frozen > shares)
            {
                return Err(invalid(format!(
                    "完整移交账户股票拓扑或 T+1 冻结非法：owner={owner:?}"
                )));
            }
        }
        let required: std::collections::BTreeSet<_> = roster
            .iter()
            .filter(|(_, required)| *required)
            .map(|(owner, _)| *owner)
            .collect();
        let signature = handover
            .signature
            .map(|signature| {
                if Some(signature.material.through) != handover.observed_through
                    || signature
                        .owners_and_codes
                        .keys()
                        .copied()
                        .collect::<std::collections::BTreeSet<_>>()
                        != required
                {
                    return Err(invalid(
                        "INFO 初值签名 through 或 Required owner 集合不一致",
                    ));
                }
                let mut owners_and_codes = BTreeMap::new();
                for (local, (owner, _)) in roster.iter().enumerate() {
                    if let Some(codes) = signature.owners_and_codes.get(owner) {
                        let local = LocalIdx::try_from(local)
                            .map_err(|_| invalid("INFO 初值 local 超出 LocalIdx"))?;
                        owners_and_codes.insert(local, (*owner, codes.clone()));
                    }
                }
                Ok(InformationObservationSignature {
                    through: signature.material.through,
                    library: Arc::clone(&signature.material.library),
                    facts: Arc::clone(&signature.material.facts),
                    market: Arc::clone(&signature.material.market),
                    anchors: signature.material.anchors.clone(),
                    owners_and_codes,
                })
            })
            .transpose()?;
        if signature.is_none() && handover.observed_through.is_some() && !required.is_empty() {
            return Err(invalid("INFO 初值 Required 有观察水位却缺签名"));
        }
        match &self.decision {
            Some((profiles, _))
                if profiles.len() >= roster.len()
                    && attention
                        .iter()
                        .map(|(owner, _, _)| *owner)
                        .collect::<Vec<_>>()
                        == expected => {}
            None if attention.is_empty() => {}
            _ => {
                return Err(invalid(
                    "完整移交初值 profile/attention 与真实名册不完整或错位",
                ))
            }
        }
        let mut next_accounts = self.accounts.clone();
        let mut next_decision = self.decision.clone();
        let mut next_personals = self.belief_states.clone();
        for (local, (_, personal)) in personals.into_iter().enumerate() {
            next_personals[local] = personal;
        }
        for (local, (_, account)) in accounts.into_iter().enumerate() {
            next_accounts[local] = Arc::new(account);
        }
        if let Some((profiles, _)) = &mut next_decision {
            for (local, (_, due, seed)) in attention.into_iter().enumerate() {
                profiles[local].next_due = due;
                profiles[local].seed = seed;
            }
        }
        let next_heap = next_decision
            .as_ref()
            .map(|(profiles, _)| {
                profiles
                    .iter()
                    .enumerate()
                    .map(|(local, profile)| {
                        std::cmp::Reverse((profile.next_due, local as LocalIdx))
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.accounts = next_accounts;
        self.decision = next_decision;
        self.belief_states = next_personals;
        self.due_heap = next_heap;
        self.library = library;
        self.current_company_facts = Some(facts);
        self.information_observed_through = handover.observed_through;
        self.information_signature = signature;
        self.information_handover_installed = true;
        self.inherited_calendar = inherited_calendar;
        Ok(())
    }

    /// 初值仅允许构造静点安装；恢复世界的 initial tick 可以大于零。
    pub(crate) fn install_initial_company_facts(
        &mut self,
        facts: Arc<crate::company::CompanyReadFacts>,
    ) -> Result<(), RuntimeError> {
        if self.current_company_facts.is_some()
            || self.snapshot.is_some()
            || self.admin_last_id != 0
            || self.admin_pending.is_some()
            || self.cur_tick != self.committed_tick
        {
            return Err(invalid("公司共同事实初值只能在首个 Start 前安装一次"));
        }
        self.current_company_facts = Some(facts);
        Ok(())
    }

    pub(crate) fn current_company_facts(
        &self,
    ) -> Result<&Arc<crate::company::CompanyReadFacts>, RuntimeError> {
        self.current_company_facts
            .as_ref()
            .ok_or_else(|| invalid("组缺少 current CompanyReadFacts"))
    }

    #[cfg(test)]
    fn prepare_admin(
        &mut self,
        id: AdminTransactionId,
        operations: Vec<AdminOperation>,
    ) -> Result<AdminGroupCandidate, RuntimeError> {
        self.prepare_admin_with_information_clock(id, operations, None, None)
    }

    /// 直接 Group 测试的合成 sender；生产只接收 World 私有广播证明。
    #[cfg(test)]
    fn prepare_admin_with_information(
        &mut self,
        id: AdminTransactionId,
        operations: Vec<AdminOperation>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
    ) -> Result<AdminGroupCandidate, RuntimeError> {
        let proof = information
            .as_ref()
            .and_then(|payload| payload.observation.as_ref())
            .map(|point| super::super::world::AuthenticatedInformationClock::fixture(&point.clock));
        self.prepare_admin_with_information_clock(id, operations, information, proof)
    }

    #[cfg(test)]
    fn prepare_admin_with_information_clock(
        &mut self,
        id: AdminTransactionId,
        operations: Vec<AdminOperation>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        information_clock: Option<super::super::world::AuthenticatedInformationClock>,
    ) -> Result<AdminGroupCandidate, RuntimeError> {
        self.prepare_admin_scope_with_clock(id, operations, information, None, information_clock)
    }

    /// 回购机制短 fixture 沿同一 sender 构造测试证明；生产走 World 认证后的 core。
    #[cfg(test)]
    fn prepare_admin_scope(
        &mut self,
        id: AdminTransactionId,
        operations: Vec<AdminOperation>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        repurchase: Option<Vec<crate::runtime::protocol::RepurchaseProjectionInput>>,
    ) -> Result<AdminGroupCandidate, RuntimeError> {
        let proof = information
            .as_ref()
            .and_then(|payload| payload.observation.as_ref())
            .map(|point| super::super::world::AuthenticatedInformationClock::fixture(&point.clock));
        self.prepare_admin_scope_with_clock(id, operations, information, repurchase, proof)
    }

    #[cfg(test)]
    fn prepare_admin_scope_with_clock(
        &mut self,
        id: AdminTransactionId,
        operations: Vec<AdminOperation>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        repurchase: Option<Vec<crate::runtime::protocol::RepurchaseProjectionInput>>,
        information_clock: Option<super::super::world::AuthenticatedInformationClock>,
    ) -> Result<AdminGroupCandidate, RuntimeError> {
        self.prepare_admin_ordered_scope(
            id,
            operations,
            None,
            information,
            repurchase,
            information_clock,
        )
    }

    fn prepare_admin_ordered_scope(
        &mut self,
        id: AdminTransactionId,
        operations: Vec<AdminOperation>,
        stage_layout: Option<crate::runtime::protocol::AdminStageLayout>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        repurchase: Option<Vec<crate::runtime::protocol::RepurchaseProjectionInput>>,
        information_clock: Option<super::super::world::AuthenticatedInformationClock>,
    ) -> Result<AdminGroupCandidate, RuntimeError> {
        if id.0 <= self.admin_last_id {
            return Err(invalid(format!(
                "组 {} 行政 prepare 重复/过期：{id:?}",
                self.group
            )));
        }
        self.admin_last_id = id.0;
        if self.admin_pending.is_some() {
            return Err(invalid(format!(
                "组 {} 行政 prepare 已有 pending：{id:?}",
                self.group
            )));
        }
        if !self.staging_deferred.is_empty()
            || self.batches_after_seal != 0
            || (self.seal_acked_len.is_none() && self.cur_tick != self.committed_tick)
        {
            return Err(invalid(format!("组 {} 行政 prepare 不在静点", self.group)));
        }
        if operations.is_empty() && information.is_none() && repurchase.is_none() {
            return Err(invalid("行政参与组不得没有操作"));
        }
        let repurchase_working = repurchase
            .as_ref()
            .map(|rows| self.prepare_repurchase_working(&operations, rows))
            .transpose()?;
        let operations = repurchase_working
            .as_ref()
            .map_or(operations, |working| working.operations.clone());
        let (mut working, stages) = stages::prepare_working(
            self,
            &operations,
            stage_layout.as_ref(),
            repurchase_working.is_some(),
        )?;
        if let Some(rows) = &repurchase {
            for row in rows {
                let account = self
                    .accounts
                    .get(row.local as usize)
                    .ok_or_else(|| invalid("执行投影账户越界"))?;
                working
                    .entry(row.local)
                    .or_insert_with(|| (**account).clone());
            }
        }
        // 信息计算读取同一事务资产候选，全部成功后才暂装资产 working。
        let information = information
            .as_ref()
            .map(|payload| self.prepare_information(payload, information_clock.as_ref(), &working))
            .transpose()?;
        let before = working
            .keys()
            .map(|&local| (local, Arc::clone(&self.accounts[local as usize])))
            .collect();
        for (local, account) in working {
            self.accounts[local as usize] = Arc::new(account);
        }
        let repurchase = repurchase_working.map(|working| {
            let pending = PendingRepurchase {
                sources: working
                    .projected_ids
                    .iter()
                    .filter(|id| self.repurchase_projections.contains_key(id))
                    .map(|id| (*id, self.repurchase_source_snapshot(*id)))
                    .collect(),
                before_book: std::mem::replace(&mut self.plan_book, working.book),
                before_dirty: self.plan_book_dirty,
                before_tails: std::mem::replace(&mut self.repurchase_pending_tails, working.tails),
                before_projections: std::mem::replace(
                    &mut self.repurchase_projections,
                    working.projections,
                ),
                created: working.created,
                projected_ids: working.projected_ids,
            };
            if pending.before_book != self.plan_book
                || pending.before_projections != self.repurchase_projections
            {
                self.plan_book_dirty = true;
            }
            pending
        });
        self.admin_pending = Some(PendingAdmin {
            id,
            before,
            stages,
            information,
            repurchase,
        });
        Ok(self.admin_candidate())
    }

    fn admin_candidate(&self) -> AdminGroupCandidate {
        let pending = self.admin_pending.as_ref().expect("候选必须有 pending");
        let accounts_full = pending
            .before
            .iter()
            .any(|(local, before)| before.as_ref() != self.accounts[*local as usize].as_ref());
        let mut mirror_domains = super::super::source::MirrorDomains {
            accounts_full,
            ..Default::default()
        };
        if let Some(information) = &pending.information {
            mirror_domains = mirror_domains.union(information.mirror_domains());
        }
        if let Some(repurchase) = &pending.repurchase {
            mirror_domains.plans_pending = repurchase.before_book != self.plan_book
                || repurchase.before_projections != self.repurchase_projections;
        }
        AdminGroupCandidate {
            stages: pending.stages.clone(),
            mirror_domains,
            information: pending
                .information
                .as_ref()
                .map(|info| info.prepared.clone()),
            repurchase: pending.repurchase.as_ref().map(|repurchase| {
                crate::runtime::protocol::PreparedRepurchaseResult {
                    sources: repurchase
                        .projected_ids
                        .iter()
                        .map(|id| {
                            (
                                *id,
                                repurchase
                                    .sources
                                    .get(id)
                                    .cloned()
                                    .unwrap_or_else(|| self.repurchase_source_snapshot(*id)),
                            )
                        })
                        .collect(),
                    book: self.plan_book.clone(),
                    created: repurchase.created.clone(),
                    projections: repurchase
                        .projected_ids
                        .iter()
                        .map(|id| (*id, self.repurchase_projections[id].clone()))
                        .collect(),
                }
            }),
            group: self.group,
            accounts: pending
                .before
                .iter()
                .map(|(local, _)| (*local, Arc::clone(&self.accounts[*local as usize])))
                .collect(),
            state_hash: self.state_hash(),
        }
    }

    fn complete_admin(
        &mut self,
        id: AdminTransactionId,
        phase: AdminPhase,
    ) -> Result<AdminGroupCandidate, RuntimeError> {
        if self.admin_pending.as_ref().map(|p| p.id) != Some(id) {
            return Err(invalid(format!(
                "组 {} 行政 {phase:?} id 错配：{id:?}",
                self.group
            )));
        }
        let locals: Vec<_> = self
            .admin_pending
            .as_ref()
            .expect("已校验 pending")
            .before
            .iter()
            .map(|(local, _)| *local)
            .collect();
        match phase {
            AdminPhase::Abort => self.discard_pending_admin(),
            AdminPhase::Commit => {
                let pending = self.admin_pending.take().expect("已校验 pending");
                if let Some(information) = pending.information {
                    self.commit_information(information);
                }
                // 旧日志退役后，同 tick Drain 仍必须拥有有效的 Seal 增量基线。
                self.undo.clear();
                if self.seal_acked_len.is_some() {
                    self.seal_acked_len = Some(0);
                }
                self.batches_after_seal = 0;
                self.plan_undo_saved = false;
                self.account_dirty_set.clear();
                self.committed_tick = self.cur_tick;
            }
            AdminPhase::Prepare => return Err(invalid("prepare 不能走 complete")),
        }
        Ok(AdminGroupCandidate {
            stages: None,
            mirror_domains: Default::default(),
            information: None,
            repurchase: None,
            group: self.group,
            accounts: locals
                .into_iter()
                .map(|local| (local, Arc::clone(&self.accounts[local as usize])))
                .collect(),
            state_hash: self.state_hash(),
        })
    }

    /// 退出也只撤本行政 overlay；不得普通 rollback 倒放已发布市场事实。
    pub(super) fn discard_pending_admin(&mut self) {
        if let Some(pending) = self.admin_pending.take() {
            if let Some(repurchase) = pending.repurchase {
                self.plan_book = repurchase.before_book;
                self.plan_book_dirty = repurchase.before_dirty;
                self.repurchase_projections = repurchase.before_projections;
                self.repurchase_pending_tails = repurchase.before_tails;
            }
            if let Some(information) = pending.information {
                self.abort_information(information);
            }
            for (local, account) in pending.before {
                self.accounts[local as usize] = account;
            }
        }
    }

    pub(super) fn handle_admin_message(
        &mut self,
        id: AdminTransactionId,
        phase: AdminPhase,
        input: AdminPrepareInput,
        up: &Sender<UpMsg>,
        who: super::super::protocol::WorkerId,
    ) -> bool {
        let AdminPrepareInput {
            operations,
            stage_layout,
            information,
            information_clock,
            repurchase,
        } = input;
        #[cfg(test)]
        let fault = if self.admin_fault.as_ref().is_some_and(|(p, _)| *p == phase) {
            self.admin_fault.take().map(|(_, f)| f)
        } else {
            None
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            #[cfg(test)]
            if let Some(AdminFault::Panic(payload)) = &fault {
                std::panic::panic_any(Arc::clone(payload));
            }
            if phase == AdminPhase::Prepare {
                self.prepare_admin_ordered_scope(
                    id,
                    operations,
                    stage_layout,
                    information,
                    repurchase,
                    information_clock,
                )
            } else {
                self.complete_admin(id, phase)
            }
        }));
        match outcome {
            Err(payload) => {
                self.discard_pending_admin();
                let _ = up.send(UpMsg::TaskDied { who, payload });
                false
            }
            Ok(result) => {
                #[cfg(test)]
                let mut result = result;
                #[cfg(test)]
                if let Ok(candidate) = &mut result {
                    if let Some(AdminFault::StagePrepareCorruption(mode)) = &fault {
                        let prepared = candidate
                            .stages
                            .as_mut()
                            .expect("阶段故障必须来自真实 stage prepare");
                        match mode {
                            0 => {
                                prepared.operation_indices.remove(0);
                            }
                            1 => prepared
                                .operation_indices
                                .push(prepared.operation_indices[0]),
                            2 => prepared.operation_indices.swap(0, 1),
                            3 => Arc::make_mut(&mut prepared.tax_stages[0].accounts[0].2).cash -= 1,
                            4 => prepared.tax_stages.push(prepared.tax_stages[0].clone()),
                            5 => prepared.tax_stages[0].accounts[0].0 += 1,
                            _ => panic!("未知阶段来源故障"),
                        }
                    }
                    if let Some(AdminFault::RepurchasePrepareCorruption(mode)) = fault {
                        if mode == 0 {
                            candidate.repurchase = None;
                        } else if let Some(prepared) = &mut candidate.repurchase {
                            match mode {
                                1 => prepared.created.clear(),
                                2 => prepared.created.push(prepared.created[0].clone()),
                                3 => prepared.created[0].operation_index += 1,
                                4 => {
                                    let (policy, mut cursors, entries) =
                                        prepared.book.clone().into_parts();
                                    cursors[0].next_sequence = cursors[0]
                                        .next_sequence
                                        .and_then(|value| value.checked_add(1));
                                    prepared.book = crate::plans::PlanBook::from_parts(
                                        policy, cursors, entries,
                                    )
                                    .expect("故障只改变合法游标");
                                }
                                5 => prepared.projections.clear(),
                                6 => {
                                    prepared
                                        .projections
                                        .values_mut()
                                        .next()
                                        .expect("故障有合法投影")
                                        .funding_amount = crate::Money::from_cents(999)
                                }
                                7 => {
                                    prepared
                                        .book
                                        .create_issuer_repurchase(
                                            crate::plans::RepurchaseExecutionOpen {
                                                owner: prepared.created[0].plan_id.owner,
                                                issuer: crate::company::CompanyId(1),
                                                stock: crate::StockId(1),
                                            },
                                        )
                                        .expect("故障额外添加不同证券计划");
                                }
                                _ => panic!("未知回购 Prepare 故障类型"),
                            }
                        }
                    }
                    if fault == Some(AdminFault::RepurchaseInControlReceipt) {
                        candidate.repurchase =
                            Some(crate::runtime::protocol::PreparedRepurchaseResult {
                                sources: Default::default(),
                                book: self.plan_book.clone(),
                                created: Vec::new(),
                                projections: self.repurchase_projections.clone(),
                            });
                    }
                    if fault == Some(AdminFault::WrongLocals) {
                        candidate.accounts.clear();
                    }
                }
                let fatal = phase != AdminPhase::Prepare && result.is_err();
                #[cfg(test)]
                let reply_id = if fault == Some(AdminFault::WrongId) {
                    AdminTransactionId(id.0 + 1)
                } else {
                    id
                };
                #[cfg(not(test))]
                let reply_id = id;
                #[cfg(test)]
                let reply_group = if fault == Some(AdminFault::WrongGroup) {
                    self.group + 1
                } else {
                    self.group
                };
                #[cfg(not(test))]
                let reply_group = self.group;
                #[cfg(test)]
                if fault == Some(AdminFault::Missing) {
                    return true;
                }
                #[cfg(test)]
                if fault == Some(AdminFault::Duplicate) {
                    let _ = up.send(UpMsg::AdminReceipt {
                        id: reply_id,
                        group: reply_group,
                        phase,
                        result: result.clone(),
                    });
                }
                up.send(UpMsg::AdminReceipt {
                    id: reply_id,
                    group: reply_group,
                    phase,
                    result,
                })
                .is_ok()
                    && !fatal
            }
        }
    }
}

pub(crate) fn apply_operation(
    account: &mut GroupAccount,
    stock: StockIdx,
    operation: AdminOperation,
) -> Result<(), RuntimeError> {
    let stock_coordinate = stock;
    let stock = usize::from(stock);
    // 新资金原语先校验完整账户向量；旧操作保留原有坐标与冻结检查。
    if matches!(
        operation,
        AdminOperation::CashDividend { .. }
            | AdminOperation::WithholdingTax { .. }
            | AdminOperation::ShareSplit { .. }
            | AdminOperation::ShareReduction { .. }
    ) {
        if stock >= account.shares.len() {
            return Err(invalid("stock 越界"));
        }
        if account.frozen_today[stock] > account.shares[stock] {
            return Err(invalid("冻结超过在手持仓"));
        }
    }
    match operation {
        AdminOperation::CloseIssuerRepurchase { .. }
        | AdminOperation::OpenIssuerRepurchase { .. } => {
            return Err(invalid("APPROVAL-C 同 PendingAdmin 创建计划尚未实现"));
        }
        AdminOperation::SyntheticBudgetCredit { amount, .. } => {
            return super::admin_repurchase_funds::apply_cash(
                account,
                stock_coordinate,
                amount,
                false,
            );
        }
        AdminOperation::SyntheticBudgetRecycle { amount, .. } => {
            return super::admin_repurchase_funds::apply_cash(
                account,
                stock_coordinate,
                amount,
                true,
            );
        }
        AdminOperation::PaymentDebit { amount, .. } => {
            return super::admin_funds_cost::payment_debit(account, stock_coordinate, amount)
                .map_err(|error| invalid(error.to_string()));
        }
        AdminOperation::SubscriptionRefund { amount, .. } => {
            return super::admin_funds_cost::subscription_refund(account, stock_coordinate, amount)
                .map_err(|error| invalid(error.to_string()));
        }
        AdminOperation::SharesWithCost {
            qty,
            subscription_cost,
            ..
        } => {
            return super::admin_funds_cost::shares_with_cost(
                account,
                stock_coordinate,
                qty,
                subscription_cost,
            )
            .map_err(|error| invalid(error.to_string()));
        }
        AdminOperation::CashDividend { amount, .. } => {
            if amount <= 0 {
                return Err(invalid("股利必须为正分额"));
            }
            account.cash = account
                .cash
                .checked_add(amount)
                .ok_or_else(|| invalid("股利现金上溢"))?;
        }
        AdminOperation::WithholdingTax { amount, .. } => {
            if amount <= 0 {
                return Err(invalid("扣税必须为正分额"));
            }
            if account.cash < amount {
                return Err(invalid("扣税可用现金不足"));
            }
            account.cash = account
                .cash
                .checked_sub(amount)
                .ok_or_else(|| invalid("扣税现金下溢"))?;
        }
        AdminOperation::ShareSplit { bonus_qty, .. } => {
            if bonus_qty == 0 {
                return Err(invalid("送转数量必须为正股数"));
            }
            account.shares[stock] = account.shares[stock]
                .checked_add(bonus_qty)
                .ok_or_else(|| invalid("送转股数上溢"))?;
            account.ever_held[stock] = false;
        }
        AdminOperation::ShareReduction { qty, .. } => {
            if qty == 0 {
                return Err(invalid("核减数量必须为正股数"));
            }
            if qty > account.shares[stock] - account.frozen_today[stock] {
                return Err(invalid("核减可卖在手股份不足"));
            }
            account.shares[stock] -= qty;
            if account.shares[stock] == 0 && account.pending_sell_qty[stock] == 0 {
                if account.foreign_active[stock] != 0 {
                    return Err(invalid("核减真清仓仍有在途票，不支持"));
                }
                account.invested_cents[stock] = 0;
                account.recovered_cents[stock] = 0;
                account.ever_held[stock] = true;
            } else {
                account.ever_held[stock] = false;
            }
        }
    }
    account
        .cash
        .checked_add(account.buy_purse_held_cents)
        .ok_or_else(|| invalid("现金加在途 purse 上溢"))?;
    account.shares[stock]
        .checked_add(account.pending_sell_qty[stock])
        .ok_or_else(|| invalid("在手加在途卖股上溢"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn funds_p1_dispatcher_rejects_short_vectors_without_panic_or_mutation() {
        use crate::runtime::group::InlineVec;
        for operation in [
            AdminOperation::PaymentDebit {
                local: 0,
                stock: 0,
                amount: 1,
            },
            AdminOperation::SubscriptionRefund {
                local: 0,
                stock: 0,
                amount: 1,
            },
            AdminOperation::SharesWithCost {
                local: 0,
                stock: 0,
                qty: 1,
                subscription_cost: 1,
            },
        ] {
            for short in 0..6 {
                let mut account = GroupAccount::new(100, 1);
                account.shares[0] = 20;
                match short {
                    0 => account.frozen_today = InlineVec::filled(0, 0),
                    1 => account.foreign_active = InlineVec::filled(0, 0),
                    2 => account.pending_sell_qty = InlineVec::filled(0, 0),
                    3 => account.invested_cents = InlineVec::filled(0, 0),
                    4 => account.recovered_cents = InlineVec::filled(0, 0),
                    _ => account.ever_held = InlineVec::filled(0, false),
                }
                let before = account.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    apply_operation(&mut account, 0, operation)
                }));
                assert!(
                    result.is_ok(),
                    "真实dispatcher必须显式拒绝短向量而非panic：{operation:?}/{short}"
                );
                assert!(
                    result.unwrap().is_err(),
                    "短向量不得通过：{operation:?}/{short}"
                );
                assert_eq!(account, before);
            }
        }
    }

    fn state() -> GroupState {
        let mut a = GroupAccount::new(1_000_000, 1);
        a.shares[0] = 1000;
        GroupState::new(GroupSetup {
            group: 0,
            accounts: vec![a],
            chunk: 8,
            lot: 100,
            config: Arc::new(GameConfig::proposed_defaults()),
            ticks_per_day: 100,
            decision: None,
            plans: Vec::new(),
            script: Box::new(|_, _| TickPlan::default()),
        })
    }
    fn credit() -> Vec<AdminOperation> {
        vec![AdminOperation::CashDividend {
            local: 0,
            stock: 0,
            amount: 101,
        }]
    }
    #[test]
    fn admin_02_prepare_abort_preserve_full_log_and_commit_only_retires_metadata() {
        let mut s = state();
        s.decision = Some((
            vec![crate::runtime::decision::NpcProfile {
                strategy: crate::strategy::StrategyData::retail(1.0, 100, 1.0),
                seed: 713,
                next_due: 9,
                attention_base: 0.3,
                retail_style: None,
            }],
            7,
        ));
        s.due_heap.push(std::cmp::Reverse((9, 0)));
        s.attention_evolved.push(0);
        let attention = format!(
            "{:?}/{:?}/{:?}",
            s.decision, s.due_heap, s.attention_evolved
        );
        s.cur_tick = 1;
        s.cur_round = 2;
        s.committed_tick = 0;
        s.undo.push(GroupUndo::Cash { local: 0, delta: 7 });
        s.seal_acked_len = Some(1);
        s.plan_undo_saved = true;
        s.plan_book_dirty = true;
        s.account_dirty_set.insert(0);
        let before = s.accounts.clone();
        let log = format!("{:?}", s.undo);
        let plans = format!("{:?}", s.plan_book);
        let hash = s.state_hash();
        let committed_hash = s.committed_hash;
        s.prepare_admin(AdminTransactionId(1), credit()).unwrap();
        assert_eq!((s.cur_tick, s.committed_tick, s.cur_round), (1, 0, 2));
        s.complete_admin(AdminTransactionId(1), AdminPhase::Abort)
            .unwrap();
        assert_eq!(s.accounts, before);
        assert_eq!(
            format!(
                "{:?}/{:?}/{:?}",
                s.decision, s.due_heap, s.attention_evolved
            ),
            attention
        );
        assert_eq!(s.state_hash(), hash);
        assert_eq!(format!("{:?}", s.undo), log);
        assert_eq!(format!("{:?}", s.plan_book), plans);
        assert_eq!((s.cur_tick, s.committed_tick, s.cur_round), (1, 0, 2));
        assert_eq!(s.seal_acked_len, Some(1));
        assert!(s.plan_undo_saved);
        assert!(s.account_dirty_set.contains(&0));
        s.prepare_admin(AdminTransactionId(2), credit()).unwrap();
        s.complete_admin(AdminTransactionId(2), AdminPhase::Commit)
            .unwrap();
        assert_eq!((s.cur_tick, s.committed_tick, s.cur_round), (1, 1, 2));
        assert_eq!(s.committed_hash, committed_hash);
        assert_eq!(
            format!(
                "{:?}/{:?}/{:?}",
                s.decision, s.due_heap, s.attention_evolved
            ),
            attention
        );
        assert_eq!(s.seal_acked_len, Some(0));
        assert!(s.undo.is_empty());
        assert!(!s.plan_undo_saved);
        assert!(s.plan_book_dirty);
        assert_eq!(format!("{:?}", s.plan_book), plans);
    }
    #[test]
    fn admin_11_group_id_contract_and_failed_prepare_consumes_id() {
        let mut s = state();
        s.prepare_admin(AdminTransactionId(1), credit()).unwrap();
        assert!(s.prepare_admin(AdminTransactionId(1), credit()).is_err());
        assert!(s.prepare_admin(AdminTransactionId(2), credit()).is_err());
        assert!(s
            .complete_admin(AdminTransactionId(2), AdminPhase::Abort)
            .is_err());
        s.complete_admin(AdminTransactionId(1), AdminPhase::Abort)
            .unwrap();
        assert!(
            s.prepare_admin(AdminTransactionId(2), credit()).is_err(),
            "pending 期被拒绝的 id 也已见过，不能重用"
        );
        s.prepare_admin(AdminTransactionId(3), credit()).unwrap();
        s.complete_admin(AdminTransactionId(3), AdminPhase::Commit)
            .unwrap();
        assert!(s
            .complete_admin(AdminTransactionId(3), AdminPhase::Commit)
            .is_err());
        assert!(s
            .complete_admin(AdminTransactionId(3), AdminPhase::Abort)
            .is_err());
        assert!(s.prepare_admin(AdminTransactionId(1), credit()).is_err());
    }
}

mod information;
use information::PendingInformation;

#[cfg(test)]
mod information_tests;

#[cfg(test)]
mod repurchase_funds_tests;

#[cfg(test)]
mod repurchase_approval_tests;

#[cfg(test)]
mod repurchase_source_tests;
